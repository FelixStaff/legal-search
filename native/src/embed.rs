//! Local sentence embeddings: multilingual-e5-small (INT8 ONNX) + compact SentencePiece tokenizer.
//!
//! f(text) = L2norm( mean_pool( encoder("query: "|"passage: " + text) ) )
//! CPU only, no network access of any kind.

use crate::chunker::TokenCounter;
use crate::search::l2_normalize;
use crate::tokenizer::SpTokenizer;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;
use std::path::Path;
use std::sync::OnceLock;

/// Bump when pooling or chunking changes. Combined with the installed model id
/// (models/current/model_id.txt) so cached indexes built with another model or
/// chunking are ignored and rebuilt.
pub const PIPELINE_VERSION: &str = "meanpool/chunk200o40/v1";

/// Cache key for "which embedding space is this index in". Cheap: does not load the model.
pub fn model_version(model_dir: Option<&Path>) -> String {
    let id = model_dir
        .and_then(|d| std::fs::read_to_string(d.join("model_id.txt")).ok())
        .map(|s| s.trim().chars().filter(|c| !c.is_control()).take(200).collect::<String>())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown-model".into());
    format!("{id}/{PIPELINE_VERSION}")
}
pub const MAX_SEQ_LEN: usize = 512;
/// Dynamic INT8 quantization computes activation scales per tensor, so padding
/// tokens from other texts in a batch slightly change a text's vector. Batch size
/// 1 keeps every embedding deterministic and independent of its neighbours; the
/// throughput cost is small because texts are short (<= ~200 tokens).
const BATCH_SIZE: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Query,
    Passage,
}

impl Kind {
    fn prefix(self) -> &'static str {
        match self {
            Kind::Query => "query: ",
            Kind::Passage => "passage: ",
        }
    }
}

pub type EmbedResult<T> = Result<T, String>;

/// Abstraction so the session logic can be tested without the real model.
pub trait Embed {
    fn dim(&self) -> usize;
    fn count_tokens(&self, text: &str) -> usize;
    /// Returns one L2-normalized vector per input, in input order.
    fn embed(&mut self, texts: &[&str], kind: Kind) -> EmbedResult<Vec<Vec<f32>>>;
}

pub struct Counter<'a>(pub &'a dyn Embed);
impl TokenCounter for Counter<'_> {
    fn count(&self, text: &str) -> usize {
        self.0.count_tokens(text)
    }
}

pub struct OnnxEmbedder {
    session: Session,
    tokenizer: SpTokenizer,
    needs_token_type_ids: bool,
    dim: usize,
}

/// Loads onnxruntime.dll once per process from an absolute path.
fn init_runtime() -> EmbedResult<()> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        let dll = crate::paths::ort_dll().ok_or("no runtime path")?;
        if !dll.is_file() {
            return Err(format!("onnxruntime.dll not found at {}", dll.display()));
        }
        ort::init_from(&dll).map_err(|e| format!("load onnxruntime: {e}"))?.with_name("legalsearch").commit();
        Ok(())
    })
    .clone()
}

impl OnnxEmbedder {
    pub fn load(model_dir: &Path) -> EmbedResult<Self> {
        init_runtime()?;
        let model = model_dir.join("model.onnx");
        let tok = model_dir.join("tokenizer.json");
        if !model.is_file() || !tok.is_file() {
            return Err(format!("model files missing in {}", model_dir.display()));
        }
        let tokenizer = SpTokenizer::from_file(&tok)?;

        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 4);
        let build = || -> Result<Session, String> {
            let e = |e: &dyn std::fmt::Display| format!("onnx session: {e}");
            Session::builder()
                .map_err(|x| e(&x))?
                .with_optimization_level(GraphOptimizationLevel::Level3)
                .map_err(|x| e(&x))?
                .with_intra_threads(threads)
                .map_err(|x| e(&x))?
                .with_inter_threads(1)
                .map_err(|x| e(&x))?
                .commit_from_file(&model)
                .map_err(|x| e(&x))
        };
        let session = build()?;
        let needs_token_type_ids = session.inputs().iter().any(|i| i.name() == "token_type_ids");
        Ok(OnnxEmbedder { session, tokenizer, needs_token_type_ids, dim: 384 })
    }

    fn run_batch(&mut self, texts: &[String]) -> EmbedResult<Vec<Vec<f32>>> {
        let encs: Vec<Vec<u32>> = texts.iter().map(|t| self.tokenizer.encode_for_model(t, MAX_SEQ_LEN)).collect();
        let b = encs.len();
        let seq = encs.iter().map(|e| e.len()).max().unwrap_or(0).max(1);
        let pad = self.tokenizer.pad_id() as i64;
        let mut ids = vec![pad; b * seq];
        let mut mask = vec![0i64; b * seq];
        for (i, e) in encs.iter().enumerate() {
            for (j, &id) in e.iter().enumerate() {
                ids[i * seq + j] = id as i64;
                mask[i * seq + j] = 1;
            }
        }
        let shape = [b as i64, seq as i64];
        let t_ids = Tensor::from_array((shape, ids)).map_err(|e| e.to_string())?;
        let t_mask = Tensor::from_array((shape, mask.clone())).map_err(|e| e.to_string())?;
        let outputs = if self.needs_token_type_ids {
            let t_tt = Tensor::from_array((shape, vec![0i64; b * seq])).map_err(|e| e.to_string())?;
            self.session.run(ort::inputs!["input_ids" => t_ids, "attention_mask" => t_mask, "token_type_ids" => t_tt])
        } else {
            self.session.run(ort::inputs!["input_ids" => t_ids, "attention_mask" => t_mask])
        }
        .map_err(|e| format!("inference: {e}"))?;

        let (oshape, data) = outputs[0].try_extract_tensor::<f32>().map_err(|e| format!("output: {e}"))?;
        if oshape.len() != 3 || oshape[0] as usize != b || oshape[1] as usize != seq {
            return Err(format!("unexpected output shape {oshape:?}"));
        }
        let hidden = oshape[2] as usize;
        self.dim = hidden;
        let mut out = Vec::with_capacity(b);
        for i in 0..b {
            let mut v = vec![0f32; hidden];
            let mut n = 0f32;
            for j in 0..seq {
                if mask[i * seq + j] == 0 {
                    continue;
                }
                n += 1.0;
                let row = &data[(i * seq + j) * hidden..(i * seq + j + 1) * hidden];
                v.iter_mut().zip(row).for_each(|(a, x)| *a += x);
            }
            if n > 0.0 {
                v.iter_mut().for_each(|a| *a /= n);
            }
            l2_normalize(&mut v);
            out.push(v);
        }
        Ok(out)
    }
}

impl Embed for OnnxEmbedder {
    fn dim(&self) -> usize {
        self.dim
    }

    fn count_tokens(&self, text: &str) -> usize {
        self.tokenizer.count(text)
    }

    fn embed(&mut self, texts: &[&str], kind: Kind) -> EmbedResult<Vec<Vec<f32>>> {
        // Sort by length so each batch pads to a similar length (much faster).
        let mut order: Vec<usize> = (0..texts.len()).collect();
        order.sort_by_key(|&i| texts[i].len());
        let mut result: Vec<Vec<f32>> = vec![Vec::new(); texts.len()];
        for batch in order.chunks(BATCH_SIZE) {
            let inputs: Vec<String> = batch.iter().map(|&i| format!("{}{}", kind.prefix(), texts[i])).collect();
            let vecs = self.run_batch(&inputs)?;
            for (&i, v) in batch.iter().zip(vecs) {
                result[i] = v;
            }
        }
        Ok(result)
    }
}
