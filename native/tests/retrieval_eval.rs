//! Retrieval evaluation with the REAL model: Dense-only vs BM25-only vs Hybrid (RRF).
//! Reports Recall@1/3/5 and MRR@8 per query category, a small RRF parameter grid,
//! and per-stage latency. Skipped if the model has not been fetched.
//!
//!   cargo test --release --test retrieval_eval -- --nocapture
//!
//! The pipeline mirrors production exactly (same chunker, embedder, BM25 and
//! fusion functions); `engine_matches_harness` checks it against Engine itself.

use legalsearch::cache::Cache;
use legalsearch::chunker::{chunk_document, Chunk, ChunkConfig};
use legalsearch::embed::{model_version, Counter, Embed, Kind, OnnxEmbedder};
use legalsearch::engine::Engine;
use legalsearch::hybrid::{fuse, HybridConfig, CANDIDATE_HYBRID};
use legalsearch::lexical::Bm25Index;
use legalsearch::protocol::{PageText, Request, Response};
use legalsearch::search::{top_k, Matrix};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

const K: usize = 8;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn model_dir() -> Option<PathBuf> {
    if std::env::var_os("LEGALSEARCH_ORT_DLL").is_none() {
        std::env::set_var("LEGALSEARCH_ORT_DLL", repo().join("native/runtime/onnxruntime.dll"));
    }
    let d =
        std::env::var_os("LEGALSEARCH_MODEL_DIR").map(PathBuf::from).unwrap_or_else(|| repo().join("models/current"));
    d.join("model.onnx").is_file().then_some(d)
}

struct Query {
    set: &'static str,
    cat: String,
    q: String,
    pages: Vec<u32>,
}

struct Doc {
    pages: Vec<PageText>,
    chunks: Vec<Chunk>,
    matrix: Matrix,
    bm25: Bm25Index,
}

fn load_set(file: &str, set: &'static str, emb: &mut OnnxEmbedder) -> (Doc, Vec<Query>) {
    let j: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(repo().join("tests/fixtures").join(file)).unwrap()).unwrap();
    let pages: Vec<PageText> = j["pages"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, t)| PageText { page: i as u32 + 1, text: t.as_str().unwrap().into() })
        .collect();
    let chunks = chunk_document(&pages, &ChunkConfig::default(), &Counter(&*emb));
    let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
    let mut matrix = Matrix::new(emb.dim());
    for v in emb.embed(&texts, Kind::Passage).unwrap() {
        if matrix.dim != v.len() {
            matrix = Matrix::new(v.len());
        }
        matrix.push(&v);
    }
    let bm25 = Bm25Index::build(texts.iter().copied());
    let queries = j["queries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| Query {
            set,
            cat: q["cat"].as_str().map(String::from).unwrap_or_else(|| "semantic".into()),
            q: q["q"].as_str().unwrap().into(),
            pages: match q.get("pages") {
                Some(p) => p.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u32).collect(),
                None => vec![q["page"].as_u64().unwrap() as u32],
            },
        })
        .collect();
    (Doc { pages, chunks, matrix, bm25 }, queries)
}

#[derive(Default, Clone, Copy)]
struct M {
    n: u32,
    r1: u32,
    r3: u32,
    r5: u32,
    rr: f64,
}
impl M {
    fn add(&mut self, rank: Option<usize>) {
        self.n += 1;
        if let Some(r) = rank {
            self.r1 += (r <= 1) as u32;
            self.r3 += (r <= 3) as u32;
            self.r5 += (r <= 5) as u32;
            self.rr += 1.0 / r as f64;
        }
    }
    fn fmt(&self) -> String {
        let n = self.n.max(1) as f64;
        format!(
            "R@1 {:>5.1}%  R@3 {:>5.1}%  R@5 {:>5.1}%  MRR {:.3}  (n={})",
            100.0 * self.r1 as f64 / n,
            100.0 * self.r3 as f64 / n,
            100.0 * self.r5 as f64 / n,
            self.rr / n,
            self.n
        )
    }
    fn mrr(&self) -> f64 {
        self.rr / self.n.max(1) as f64
    }
}

fn rank_of(order: &[usize], chunks: &[Chunk], want: &[u32]) -> Option<usize> {
    order.iter().position(|&i| want.contains(&chunks[i].page)).map(|p| p + 1)
}

#[derive(Clone, Copy)]
enum Sys {
    Dense,
    Bm25,
    Hybrid(HybridConfig),
}

fn run(sys: Sys, scores: &[f32], lex: &[(usize, f32)]) -> Vec<usize> {
    match sys {
        Sys::Dense => top_k(scores, K).into_iter().map(|x| x.0).collect(),
        Sys::Bm25 => lex.iter().take(K).map(|x| x.0).collect(),
        Sys::Hybrid(cfg) => fuse(scores, lex, &cfg, K),
    }
}

#[test]
fn dense_vs_bm25_vs_hybrid() {
    let Some(dir) = model_dir() else {
        eprintln!("SKIP: model not found (run scripts/fetch-model.ps1)");
        return;
    };
    let mut emb = OnnxEmbedder::load(&dir).unwrap();
    let (fixture, mut queries) = load_set("corpus.json", "fixture", &mut emb);
    let (eval, q2) = load_set("eval_corpus.json", "eval", &mut emb);
    queries.extend(q2);
    eprintln!(
        "fixture: {} pages / {} chunks; eval: {} pages / {} chunks; BM25 heap ≈ {} KB + {} KB",
        fixture.pages.len(),
        fixture.chunks.len(),
        eval.pages.len(),
        eval.chunks.len(),
        fixture.bm25.heap_bytes() / 1024,
        eval.bm25.heap_bytes() / 1024
    );

    // Pre-compute query vectors, dense scores, BM25 hits and per-stage latency.
    let mut prepared = Vec::new();
    let (mut t_dense, mut t_bm25, mut t_fuse) = (0f64, 0f64, 0f64);
    const REPS: usize = 5;
    for q in &queries {
        let doc = if q.set == "fixture" { &fixture } else { &eval };
        let mut scores = Vec::new();
        let mut lex = Vec::new();
        for _ in 0..REPS {
            let t = Instant::now();
            let u = emb.embed(&[q.q.as_str()], Kind::Query).unwrap().remove(0);
            scores = doc.matrix.scores(&u);
            let _ = top_k(&scores, CANDIDATE_HYBRID.candidates);
            t_dense += t.elapsed().as_secs_f64();
            let t = Instant::now();
            lex = doc.bm25.search(&q.q);
            t_bm25 += t.elapsed().as_secs_f64();
            let t = Instant::now();
            let _ = fuse(&scores, &lex, &CANDIDATE_HYBRID, K);
            t_fuse += t.elapsed().as_secs_f64();
        }
        prepared.push((q, scores, lex));
    }
    let n = (queries.len() * REPS) as f64;
    eprintln!(
        "\nLATENCY (mean per query, in-process): dense_latency_ms={:.2}  bm25_latency_ms={:.3}  fusion_latency_ms={:.3}  total_latency_ms={:.2}",
        1e3 * t_dense / n,
        1e3 * t_bm25 / n,
        1e3 * t_fuse / n,
        1e3 * (t_dense + t_bm25 + t_fuse) / n
    );

    let evaluate = |sys: Sys, filter: &dyn Fn(&Query) -> bool| -> (M, BTreeMap<String, M>) {
        let mut all = M::default();
        let mut by: BTreeMap<String, M> = BTreeMap::new();
        for (q, scores, lex) in &prepared {
            if !filter(q) {
                continue;
            }
            let doc = if q.set == "fixture" { &fixture } else { &eval };
            let r = rank_of(&run(sys, scores, lex), &doc.chunks, &q.pages);
            all.add(r);
            by.entry(format!("{}/{}", q.set, q.cat)).or_default().add(r);
        }
        (all, by)
    };

    let requested = HybridConfig { enabled: true, rrf_k: 60.0, lambda: 0.5, candidates: 40 };
    let candidate = CANDIDATE_HYBRID;
    let systems = [
        ("DENSE_ONLY", Sys::Dense),
        ("BM25_ONLY", Sys::Bm25),
        ("HYBRID_RRF k=60 λ=0.5 (requested initial values)", Sys::Hybrid(requested)),
        ("HYBRID_RRF k=10 λ=0.25 (best of grid)", Sys::Hybrid(candidate)),
    ];
    let semantic = |q: &Query| q.set == "fixture" || q.cat == "paraphrase";
    let lexical = |q: &Query| q.set == "eval" && q.cat != "paraphrase";
    for (name, sys) in systems {
        let (all, by) = evaluate(sys, &|_| true);
        let (sem, _) = evaluate(sys, &semantic);
        let (lexq, _) = evaluate(sys, &lexical);
        eprintln!("\n{name}\n  ALL        {}\n  SEMANTIC   {}\n  LEXICAL    {}", all.fmt(), sem.fmt(), lexq.fmt());
        for (cat, m) in by {
            eprintln!("    {cat:<20} {}", m.fmt());
        }
    }

    // Per-query detail (rank per system) for the report.
    eprintln!("\nPER QUERY (rank of first relevant page in top-8; - = not found)   dense | bm25 | hybrid");
    for (q, scores, lex) in &prepared {
        let doc = if q.set == "fixture" { &fixture } else { &eval };
        let r = |s| rank_of(&run(s, scores, lex), &doc.chunks, &q.pages).map_or("-".to_string(), |x| x.to_string());
        eprintln!(
            "  [{}/{:<10}] {:<75} {:>2} | {:>2} | {:>2} | {:>2}",
            q.set,
            q.cat,
            q.q.chars().take(75).collect::<String>(),
            r(Sys::Dense),
            r(Sys::Bm25),
            r(Sys::Hybrid(requested)),
            r(Sys::Hybrid(candidate))
        );
    }

    // Sensitivity of the (untuned) RRF constants — informative only.
    eprintln!("\nRRF GRID (MRR all / semantic / lexical)");
    for rrf_k in [10.0, 30.0, 60.0] {
        for lambda in [0.25, 0.5, 1.0] {
            let cfg = HybridConfig { rrf_k, lambda, ..CANDIDATE_HYBRID };
            let (a, _) = evaluate(Sys::Hybrid(cfg), &|_| true);
            let (s, _) = evaluate(Sys::Hybrid(cfg), &semantic);
            let (l, _) = evaluate(Sys::Hybrid(cfg), &lexical);
            eprintln!(
                "  k={rrf_k:<4} λ={lambda:<4}  {:.3} / {:.3} / {:.3}   R@3 all {}",
                a.mrr(),
                s.mrr(),
                l.mrr(),
                a.r3
            );
        }
    }

    // Guard rails for the candidate configuration: it must not significantly
    // degrade purely semantic queries, and must not be worse overall.
    let (d_sem, _) = evaluate(Sys::Dense, &semantic);
    let (h_sem, _) = evaluate(Sys::Hybrid(CANDIDATE_HYBRID), &semantic);
    assert!(h_sem.r3 + 1 >= d_sem.r3, "semantic Recall@3 regressed: dense {} vs hybrid {}", d_sem.r3, h_sem.r3);
    assert!(h_sem.mrr() >= d_sem.mrr() - 0.05, "semantic MRR regressed");
    let (d_all, _) = evaluate(Sys::Dense, &|_| true);
    let (h_all, _) = evaluate(Sys::Hybrid(CANDIDATE_HYBRID), &|_| true);
    assert!(h_all.mrr() >= d_all.mrr(), "hybrid must not be worse than dense overall");
}

#[test]
fn engine_matches_harness() {
    let Some(dir) = model_dir() else { return };
    let mut emb = OnnxEmbedder::load(&dir).unwrap();
    let (doc, queries) = load_set("eval_corpus.json", "eval", &mut emb);
    let version = model_version(Some(&dir));
    let d2 = dir.clone();
    let mut engine = Engine::new(
        Cache::open_in_memory().unwrap(),
        Box::new(move || OnnxEmbedder::load(&d2).map(|e| Box::new(e) as Box<dyn Embed>)),
        &version,
    )
    .with_hybrid(CANDIDATE_HYBRID);
    let sha = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
    let n = doc.pages.len() as u32;
    let mut sink = |_r: Response| {};
    engine.handle(
        Request::IndexBegin { id: 1, sha256: sha.into(), filename: "eval.pdf".into(), page_count: n },
        &mut sink,
    );
    engine.handle(Request::IndexPages { id: 2, sha256: sha.into(), pages: doc.pages.clone() }, &mut sink);
    engine.handle(Request::IndexEnd { id: 3, sha256: sha.into() }, &mut sink);
    for q in queries.iter().step_by(3) {
        let mut out = Vec::new();
        engine.handle(Request::Search { id: 9, sha256: sha.into(), query: q.q.clone(), k: Some(K) }, &mut |r| {
            out.push(r)
        });
        let Some(Response::Results { results, .. }) = out.pop() else { panic!() };
        let u = emb.embed(&[q.q.as_str()], Kind::Query).unwrap().remove(0);
        let scores = doc.matrix.scores(&u);
        let want: Vec<u32> = fuse(&scores, &doc.bm25.search(&q.q), &CANDIDATE_HYBRID, K)
            .iter()
            .map(|&i| doc.chunks[i].chunk_index)
            .collect();
        let got: Vec<u32> = results.iter().map(|h| h.chunk_index).collect();
        assert_eq!(got, want, "engine and harness disagree for {:?}", q.q);
        // The score exposed to the UI is still the cosine similarity.
        for h in &results {
            let i = doc.chunks.iter().position(|c| c.chunk_index == h.chunk_index).unwrap();
            assert!((h.score - scores[i]).abs() < 1e-5);
        }
    }
}
