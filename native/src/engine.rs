//! Session state machine: one instance per Chrome port (one side panel).
//!
//!   hello
//!   open{sha256}            -> opened{status: ready | needs_index}
//!   index_begin             -> ack
//!   index_pages (n times)   -> ack
//!   index_end               -> progress* -> ready | error{no_text}
//!   search{sha256, query}   -> results | error{not_indexed}

use crate::cache::{Cache, LoadedIndex};
use crate::chunker::{chunk_document, looks_scanned, ChunkConfig};
use crate::embed::{Counter, Embed, Kind};
#[cfg(test)]
use crate::hybrid::CANDIDATE_HYBRID;
use crate::hybrid::{fuse, HybridConfig};
use crate::lexical::Bm25Index;
use crate::protocol::{ErrorCode, PageText, Request, Response, SearchHit, DEFAULT_K, MAX_TOTAL_CHARS};
use crate::search::Matrix;
use crate::{error, info};
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;

/// Chunks embedded between two progress events.
const PROGRESS_BATCH: usize = 32;
/// Snippet length returned to the extension (chars).
const MAX_SNIPPET_CHARS: usize = 2000;

pub type Factory = Box<dyn FnMut() -> Result<Box<dyn Embed>, String>>;

struct Pending {
    sha256: String,
    filename: String,
    page_count: u32,
    pages: BTreeMap<u32, String>,
    total_chars: usize,
}

pub struct Engine {
    factory: Factory,
    embedder: Option<Box<dyn Embed>>,
    cache: Cache,
    model_version: String,
    chunk_cfg: ChunkConfig,
    loaded: Option<LoadedIndex>,
    pending: Option<Pending>,
    hybrid: HybridConfig,
    /// BM25 index of the loaded document, rebuilt in memory from its chunk texts
    /// (sha256, index). `None` inside = build failed -> dense-only for that document.
    lexical: Option<(String, Option<Bm25Index>)>,
}

fn clean_filename(name: &str) -> String {
    // Display-only; strip any path component and control characters.
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    base.chars().filter(|c| !c.is_control()).take(255).collect()
}

impl Engine {
    pub fn new(cache: Cache, factory: Factory, model_version: &str) -> Self {
        Engine {
            factory,
            embedder: None,
            cache,
            model_version: model_version.to_string(),
            chunk_cfg: ChunkConfig::default(),
            loaded: None,
            pending: None,
            hybrid: HybridConfig::default(),
            lexical: None,
        }
    }

    pub fn with_hybrid(mut self, cfg: HybridConfig) -> Self {
        self.hybrid = cfg;
        self
    }

    pub fn with_chunk_config(mut self, cfg: ChunkConfig) -> Self {
        self.chunk_cfg = cfg;
        self
    }

    fn embedder(&mut self) -> Result<&mut Box<dyn Embed>, ErrorCode> {
        if self.embedder.is_none() {
            let t = Instant::now();
            match (self.factory)() {
                Ok(e) => {
                    info!("model loaded in {} ms", t.elapsed().as_millis());
                    self.embedder = Some(e);
                }
                Err(e) => {
                    error!("model load failed: {e}");
                    return Err(ErrorCode::ModelMissing);
                }
            }
        }
        Ok(self.embedder.as_mut().expect("just set"))
    }

    fn ensure_loaded(&mut self, sha256: &str) -> Result<bool, ErrorCode> {
        if self.loaded.as_ref().is_some_and(|l| l.sha256 == sha256) {
            return Ok(true);
        }
        match self.cache.load(sha256, &self.model_version) {
            Ok(Some(idx)) => {
                self.loaded = Some(idx);
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(e) => {
                error!("cache load: {e}");
                Err(ErrorCode::Internal)
            }
        }
    }

    /// Handles one request; `emit` may be called several times (progress events).
    pub fn handle(&mut self, req: Request, emit: &mut dyn FnMut(Response)) {
        let id = req.id();
        let res = match req {
            Request::Hello { id } => {
                emit(Response::Hello {
                    id,
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    protocol: crate::protocol::PROTOCOL_VERSION,
                });
                Ok(())
            }
            Request::Open { id, sha256, .. } => self.open(id, sha256, emit),
            Request::IndexBegin { id, sha256, filename, page_count } => {
                self.pending = Some(Pending {
                    sha256,
                    filename: clean_filename(&filename),
                    page_count,
                    pages: BTreeMap::new(),
                    total_chars: 0,
                });
                emit(Response::Ack { id });
                Ok(())
            }
            Request::IndexPages { id, sha256, pages } => self.add_pages(id, &sha256, pages, emit),
            Request::IndexEnd { id, sha256 } => self.index_end(id, &sha256, emit),
            Request::Search { id, sha256, query, k } => self.search(id, &sha256, &query, k.unwrap_or(DEFAULT_K), emit),
        };
        if let Err(code) = res {
            emit(Response::Error { id, code });
        }
    }

    fn open(&mut self, id: u32, sha256: String, emit: &mut dyn FnMut(Response)) -> Result<(), ErrorCode> {
        if self.ensure_loaded(&sha256)? {
            let n = self.loaded.as_ref().map(|l| l.chunks.len()).unwrap_or(0) as u32;
            info!("open: cache hit ({n} chunks)");
            emit(Response::Opened { id, sha256, status: "ready", chunk_count: n });
            // Warm the model now so the first query feels instant.
            let _ = self.embedder();
        } else {
            info!("open: cache miss");
            emit(Response::Opened { id, sha256, status: "needs_index", chunk_count: 0 });
        }
        Ok(())
    }

    fn add_pages(
        &mut self,
        id: u32,
        sha256: &str,
        pages: Vec<PageText>,
        emit: &mut dyn FnMut(Response),
    ) -> Result<(), ErrorCode> {
        let Some(p) = self.pending.as_mut().filter(|p| p.sha256 == sha256) else {
            return Err(ErrorCode::InvalidRequest);
        };
        for pg in pages {
            if pg.page > p.page_count {
                return Err(ErrorCode::InvalidRequest);
            }
            p.total_chars += pg.text.len();
            if p.total_chars > MAX_TOTAL_CHARS {
                self.pending = None;
                return Err(ErrorCode::InvalidRequest);
            }
            p.pages.insert(pg.page, pg.text);
        }
        emit(Response::Ack { id });
        Ok(())
    }

    fn index_end(&mut self, id: u32, sha256: &str, emit: &mut dyn FnMut(Response)) -> Result<(), ErrorCode> {
        let Some(p) = self.pending.take().filter(|p| p.sha256 == sha256) else {
            return Err(ErrorCode::InvalidRequest);
        };
        let started = Instant::now();
        let pages: Vec<PageText> = p.pages.into_iter().map(|(page, text)| PageText { page, text }).collect();
        if looks_scanned(&pages, p.page_count) {
            info!("index: no usable text layer ({} pages)", p.page_count);
            return Err(ErrorCode::NoText);
        }
        let cfg = self.chunk_cfg;
        let emb = self.embedder()?;
        let chunks = chunk_document(&pages, &cfg, &Counter(emb.as_ref()));
        if chunks.is_empty() {
            return Err(ErrorCode::NoText);
        }
        let total = chunks.len();
        let mut matrix = Matrix::new(emb.dim());
        let mut last_pct = u32::MAX;
        for (b, group) in chunks.chunks(PROGRESS_BATCH).enumerate() {
            let texts: Vec<&str> = group.iter().map(|c| c.text.as_str()).collect();
            let vecs = emb.embed(&texts, Kind::Passage).map_err(|e| {
                error!("embed: {e}");
                ErrorCode::Internal
            })?;
            if matrix.dim != emb.dim() {
                matrix = Matrix::new(emb.dim());
            }
            for v in &vecs {
                matrix.push(v);
            }
            let done = ((b + 1) * PROGRESS_BATCH).min(total);
            let pct = (done * 100 / total) as u32;
            if pct != last_pct {
                emit(Response::Progress { id, done: done as u32, total: total as u32, pct });
                last_pct = pct;
            }
        }
        if let Err(e) = self.cache.store(sha256, &p.filename, p.page_count, &self.model_version, &chunks, &matrix) {
            // Search still works for this session; only persistence failed.
            error!("cache store: {e}");
        }
        info!("indexed {} pages, {} chunks in {} ms", p.page_count, total, started.elapsed().as_millis());
        self.loaded = Some(LoadedIndex {
            sha256: sha256.to_string(),
            chunks: chunks
                .into_iter()
                .map(|c| crate::cache::ChunkMeta { page: c.page, chunk_index: c.chunk_index, text: c.text })
                .collect(),
            matrix,
        });
        emit(Response::Ready { id, sha256: sha256.to_string(), chunk_count: total as u32 });
        Ok(())
    }

    /// BM25 hits for the loaded document. Never fails the search: any problem is
    /// logged and yields an empty list, which makes the fusion dense-only.
    fn lexical_hits(&mut self, query: &str) -> Vec<(usize, f32)> {
        if !self.hybrid.enabled {
            return Vec::new();
        }
        let Some(idx) = self.loaded.as_ref() else { return Vec::new() };
        if self.lexical.as_ref().map_or(true, |(sha, _)| sha != &idx.sha256) {
            let t = Instant::now();
            let built = catch_unwind(AssertUnwindSafe(|| Bm25Index::build(idx.chunks.iter().map(|c| c.text.as_str()))));
            let built = match built {
                Ok(b) => {
                    info!(
                        "bm25 index: {} chunks, ~{} KB, {} ms",
                        b.len(),
                        b.heap_bytes() / 1024,
                        t.elapsed().as_millis()
                    );
                    Some(b)
                }
                Err(_) => {
                    error!("bm25 build failed; continuing dense-only");
                    None
                }
            };
            self.lexical = Some((idx.sha256.clone(), built));
        }
        let Some((_, Some(bm))) = self.lexical.as_ref() else { return Vec::new() };
        match catch_unwind(AssertUnwindSafe(|| bm.search(query))) {
            Ok(hits) => hits,
            Err(_) => {
                error!("bm25 search failed; continuing dense-only");
                Vec::new()
            }
        }
    }

    fn search(
        &mut self,
        id: u32,
        sha256: &str,
        query: &str,
        k: usize,
        emit: &mut dyn FnMut(Response),
    ) -> Result<(), ErrorCode> {
        if self.pending.as_ref().is_some_and(|p| p.sha256 == sha256) {
            return Err(ErrorCode::IndexingInProgress);
        }
        if !self.ensure_loaded(sha256)? {
            return Err(ErrorCode::NotIndexed);
        }
        let t = Instant::now();
        let emb = self.embedder()?;
        let u = emb
            .embed(&[query.trim()], Kind::Query)
            .map_err(|e| {
                error!("embed query: {e}");
                ErrorCode::Internal
            })?
            .pop()
            .ok_or(ErrorCode::Internal)?;
        let idx = self.loaded.as_ref().expect("ensured");
        if u.len() != idx.matrix.dim {
            return Err(ErrorCode::Internal);
        }
        // 1-2. dense ranking (primary signal)
        let scores = idx.matrix.scores(&u);
        let dense_us = t.elapsed().as_micros();
        // 3-4. lexical ranking (complementary; empty => dense-only)
        let t_lex = Instant::now();
        let lexical = self.lexical_hits(query);
        let lex_us = t_lex.elapsed().as_micros();
        // 5. fusion
        let t_fuse = Instant::now();
        let order = fuse(&scores, &lexical, &self.hybrid, k);
        let fuse_us = t_fuse.elapsed().as_micros();
        let idx = self.loaded.as_ref().expect("ensured");
        // 6. results (the score shown is always the cosine similarity)
        let results = order
            .into_iter()
            .map(|i| {
                let c = &idx.chunks[i];
                SearchHit {
                    page: c.page,
                    chunk_index: c.chunk_index,
                    text: c.text.chars().take(MAX_SNIPPET_CHARS).collect(),
                    score: scores[i],
                }
            })
            .collect();
        let elapsed = t.elapsed().as_millis() as u32;
        info!(
            "search: {} chunks, dense {} us, bm25 {} us ({} hits), fusion {} us, total {} ms",
            idx.chunks.len(),
            dense_us,
            lex_us,
            lexical.len(),
            fuse_us,
            elapsed
        );
        let mode = if self.hybrid.enabled && !lexical.is_empty() { "hybrid" } else { "dense" };
        emit(Response::Results { id, results, mode, elapsed_ms: elapsed });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    /// Deterministic fake: hashed bag of lowercase words. Good enough to test
    /// plumbing (cache, state machine, ranking order), not semantics.
    struct Fake;
    impl Embed for Fake {
        fn dim(&self) -> usize {
            64
        }
        fn count_tokens(&self, t: &str) -> usize {
            t.split_whitespace().count()
        }
        fn embed(&mut self, texts: &[&str], _k: Kind) -> Result<Vec<Vec<f32>>, String> {
            Ok(texts
                .iter()
                .map(|t| {
                    let mut v = vec![0f32; 64];
                    for w in t.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
                        let h = w.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
                        v[(h % 64) as usize] += 1.0;
                    }
                    crate::search::l2_normalize(&mut v);
                    v
                })
                .collect())
        }
    }

    const SHA: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const OTHER: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    fn engine(loads: Rc<Cell<u32>>) -> Engine {
        Engine::new(
            Cache::open_in_memory().unwrap(),
            Box::new(move || {
                loads.set(loads.get() + 1);
                Ok(Box::new(Fake) as Box<dyn Embed>)
            }),
            "test",
        )
    }

    fn call(e: &mut Engine, r: Request) -> Vec<Response> {
        let mut out = Vec::new();
        e.handle(r, &mut |x| out.push(x));
        out
    }

    fn pages() -> Vec<PageText> {
        vec![
            PageText {
                page: 1,
                text: "La demanda fue presentada por la parte actora ante el juzgado civil del circuito.".into(),
            },
            PageText { page: 2, text: "   ".into() },
            PageText {
                page: 3,
                text:
                    "Las autoridades tenían conocimiento previo de la situación de riesgo y omitieron adoptar medidas."
                        .into(),
            },
        ]
    }

    fn index(e: &mut Engine, sha: &str) -> Vec<Response> {
        call(
            e,
            Request::IndexBegin { id: 2, sha256: sha.into(), filename: "C:\\x\\..\\doc.pdf".into(), page_count: 3 },
        );
        call(e, Request::IndexPages { id: 3, sha256: sha.into(), pages: pages() });
        call(e, Request::IndexEnd { id: 4, sha256: sha.into() })
    }

    #[test]
    fn full_flow_miss_index_search() {
        let loads = Rc::new(Cell::new(0));
        let mut e = engine(loads.clone());
        let r = call(&mut e, Request::Open { id: 1, sha256: SHA.into(), filename: "doc.pdf".into(), page_count: 3 });
        assert!(matches!(&r[0], Response::Opened { status: "needs_index", .. }));
        assert_eq!(loads.get(), 0, "model must load lazily");

        let r = index(&mut e, SHA);
        assert!(r.iter().any(|x| matches!(x, Response::Progress { pct: 100, .. })));
        assert!(matches!(r.last().unwrap(), Response::Ready { chunk_count: 2, .. }));

        let r =
            call(&mut e, Request::Search { id: 5, sha256: SHA.into(), query: "autoridades riesgo".into(), k: Some(2) });
        let Response::Results { results, .. } = &r[0] else { panic!("{r:?}") };
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].page, 3);
        assert!(results[0].score >= results[1].score);
        assert_eq!(loads.get(), 1);
    }

    #[test]
    fn second_open_is_cache_hit() {
        let mut e = engine(Rc::new(Cell::new(0)));
        index(&mut e, SHA);
        // Simulate a new session over the same cache: drop in-memory index.
        e.loaded = None;
        let r = call(&mut e, Request::Open { id: 9, sha256: SHA.into(), filename: "doc.pdf".into(), page_count: 3 });
        assert!(matches!(&r[0], Response::Opened { status: "ready", chunk_count: 2, .. }), "{r:?}");
        let r = call(&mut e, Request::Open { id: 10, sha256: OTHER.into(), filename: "b.pdf".into(), page_count: 3 });
        assert!(matches!(&r[0], Response::Opened { status: "needs_index", .. }));
    }

    #[test]
    fn search_without_index_is_error() {
        let mut e = engine(Rc::new(Cell::new(0)));
        let r = call(&mut e, Request::Search { id: 5, sha256: SHA.into(), query: "x".into(), k: None });
        assert_eq!(r, vec![Response::Error { id: 5, code: ErrorCode::NotIndexed }]);
    }

    #[test]
    fn search_during_indexing_is_reported() {
        let mut e = engine(Rc::new(Cell::new(0)));
        call(&mut e, Request::IndexBegin { id: 2, sha256: SHA.into(), filename: "a".into(), page_count: 3 });
        let r = call(&mut e, Request::Search { id: 5, sha256: SHA.into(), query: "x".into(), k: None });
        assert_eq!(r, vec![Response::Error { id: 5, code: ErrorCode::IndexingInProgress }]);
    }

    #[test]
    fn empty_or_scanned_document_is_no_text() {
        let mut e = engine(Rc::new(Cell::new(0)));
        call(&mut e, Request::IndexBegin { id: 1, sha256: SHA.into(), filename: "scan.pdf".into(), page_count: 5 });
        call(
            &mut e,
            Request::IndexPages {
                id: 2,
                sha256: SHA.into(),
                pages: (1..=5).map(|p| PageText { page: p, text: "".into() }).collect(),
            },
        );
        let r = call(&mut e, Request::IndexEnd { id: 3, sha256: SHA.into() });
        assert_eq!(r, vec![Response::Error { id: 3, code: ErrorCode::NoText }]);
        // Zero pages sent at all.
        call(&mut e, Request::IndexBegin { id: 4, sha256: SHA.into(), filename: "x.pdf".into(), page_count: 1 });
        let r = call(&mut e, Request::IndexEnd { id: 5, sha256: SHA.into() });
        assert_eq!(r, vec![Response::Error { id: 5, code: ErrorCode::NoText }]);
    }

    #[test]
    fn protocol_misuse_is_rejected() {
        let mut e = engine(Rc::new(Cell::new(0)));
        // pages without begin
        let r = call(&mut e, Request::IndexPages { id: 1, sha256: SHA.into(), pages: pages() });
        assert_eq!(r, vec![Response::Error { id: 1, code: ErrorCode::InvalidRequest }]);
        // pages for another document
        call(&mut e, Request::IndexBegin { id: 2, sha256: SHA.into(), filename: "a".into(), page_count: 3 });
        let r = call(&mut e, Request::IndexPages { id: 3, sha256: OTHER.into(), pages: pages() });
        assert_eq!(r, vec![Response::Error { id: 3, code: ErrorCode::InvalidRequest }]);
        // page beyond declared count
        let r = call(
            &mut e,
            Request::IndexPages { id: 4, sha256: SHA.into(), pages: vec![PageText { page: 4, text: "x".into() }] },
        );
        assert_eq!(r, vec![Response::Error { id: 4, code: ErrorCode::InvalidRequest }]);
        // end without begin
        let r = call(&mut e, Request::IndexEnd { id: 5, sha256: OTHER.into() });
        assert_eq!(r, vec![Response::Error { id: 5, code: ErrorCode::InvalidRequest }]);
    }

    #[test]
    fn missing_model_is_reported() {
        let mut e = Engine::new(Cache::open_in_memory().unwrap(), Box::new(|| Err("no model".into())), "t");
        let r = index(&mut e, SHA);
        assert_eq!(r, vec![Response::Error { id: 4, code: ErrorCode::ModelMissing }]);
    }

    fn search_pages(e: &mut Engine, q: &str) -> Vec<(u32, u32)> {
        let r = call(e, Request::Search { id: 7, sha256: SHA.into(), query: q.into(), k: Some(8) });
        let Response::Results { results, .. } = &r[0] else { panic!("{r:?}") };
        results.iter().map(|h| (h.page, h.chunk_index)).collect()
    }

    fn engine_with(cfg: HybridConfig) -> Engine {
        let mut e = engine(Rc::new(Cell::new(0))).with_hybrid(cfg);
        index(&mut e, SHA);
        e
    }

    #[test]
    fn query_without_lexical_terms_is_dense_only() {
        let mut hybrid = engine_with(CANDIDATE_HYBRID);
        let mut dense = engine_with(HybridConfig { enabled: false, ..CANDIDATE_HYBRID });
        for q in ["¿? … —", "y a o", "palabrasinexistente"] {
            assert_eq!(search_pages(&mut hybrid, q), search_pages(&mut dense, q), "query {q:?}");
        }
    }

    #[test]
    fn bm25_failure_falls_back_to_dense_only() {
        let mut dense = engine_with(HybridConfig { enabled: false, ..CANDIDATE_HYBRID });
        let mut e = engine_with(CANDIDATE_HYBRID);
        let _ = search_pages(&mut e, "autoridades"); // builds the lexical index
        e.lexical.as_mut().unwrap().1.as_mut().unwrap().fail_for_test = true;
        let q = "juzgado civil del circuito";
        let got = search_pages(&mut e, q);
        assert_eq!(got, search_pages(&mut dense, q), "must equal dense-only, not error");
    }

    #[test]
    fn lexical_index_follows_the_loaded_document() {
        let mut e = engine_with(CANDIDATE_HYBRID);
        let _ = search_pages(&mut e, "juzgado");
        assert_eq!(e.lexical.as_ref().unwrap().0, SHA);
        // index a second document: the lexical index must be rebuilt for it
        call(&mut e, Request::IndexBegin { id: 2, sha256: OTHER.into(), filename: "b.pdf".into(), page_count: 1 });
        call(&mut e, Request::IndexPages { id: 3, sha256: OTHER.into(), pages: vec![PageText { page: 1, text: "Texto distinto sobre el principio pro persona y su alcance en la interpretación de los tratados.".into() }] });
        call(&mut e, Request::IndexEnd { id: 4, sha256: OTHER.into() });
        let r = call(&mut e, Request::Search { id: 5, sha256: OTHER.into(), query: "pro persona".into(), k: Some(3) });
        let Response::Results { results, .. } = &r[0] else { panic!() };
        assert_eq!(results.len(), 1);
        assert_eq!(e.lexical.as_ref().unwrap().0, OTHER);
    }

    #[test]
    fn filename_is_sanitized() {
        assert_eq!(clean_filename("C:\\Users\\x\\..\\secret.pdf"), "secret.pdf");
        assert_eq!(clean_filename("../../a\u{0}b.pdf"), "ab.pdf");
    }
}
