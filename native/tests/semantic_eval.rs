//! End-to-end semantic check with the REAL model (no fakes).
//! Skipped (with a message) when the model has not been fetched yet:
//!   powershell -File scripts/fetch-model.ps1
//! Run with output:  cargo test --release --test semantic_eval -- --nocapture

use legalsearch::cache::Cache;
use legalsearch::embed::{model_version, Embed, Kind, OnnxEmbedder};
use legalsearch::engine::Engine;
use legalsearch::protocol::{PageText, Request, Response};
use legalsearch::search::{cosine, dot};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

const SHA: &str = "abababababababababababababababababababababababababababababababab";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn model_dir() -> Option<PathBuf> {
    // Release builds do not probe native/runtime on their own; point them at it.
    if std::env::var_os("LEGALSEARCH_ORT_DLL").is_none() {
        std::env::set_var("LEGALSEARCH_ORT_DLL", repo().join("native/runtime/onnxruntime.dll"));
    }
    let d = std::env::var_os("LEGALSEARCH_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo().join("models").join("current"));
    d.join("model.onnx").is_file().then_some(d)
}

fn corpus() -> serde_json::Value {
    let s = std::fs::read_to_string(repo().join("tests/fixtures/corpus.json")).unwrap();
    serde_json::from_str(&s).unwrap()
}

fn words(s: &str) -> HashSet<String> {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
}

#[test]
fn embeddings_are_normalized_and_semantic() {
    let Some(dir) = model_dir() else {
        eprintln!("SKIP: model not found (run scripts/fetch-model.ps1)");
        return;
    };
    let mut e = OnnxEmbedder::load(&dir).unwrap();
    let q = e.embed(&["el Estado sabía del peligro pero no hizo nada"], Kind::Query).unwrap().remove(0);
    assert!(q.len() == 384 || q.len() == 768);
    assert!((dot(&q, &q) - 1.0).abs() < 1e-4, "query not normalized");
    let p = e
        .embed(
            &[
                "Las autoridades tenían conocimiento previo de la situación de riesgo y omitieron adoptar medidas razonables.",
                "La condena en costas no procede de manera automática y exige verificar la conducta de las partes.",
            ],
            Kind::Passage,
        )
        .unwrap();
    let rel = cosine(&q, &p[0]);
    let unrel = cosine(&q, &p[1]);
    eprintln!("cos(relevant)={rel:.4} cos(unrelated)={unrel:.4}");
    assert!(rel > unrel);
    // Order independence of batching: same text alone gives the same vector.
    let alone = e.embed(&[p_text()], Kind::Passage).unwrap().remove(0);
    assert!(cosine(&alone, &p[0]) > 0.999);
}

fn p_text() -> &'static str {
    "Las autoridades tenían conocimiento previo de la situación de riesgo y omitieron adoptar medidas razonables."
}

#[test]
fn fixture_queries_retrieve_the_right_pages() {
    let Some(dir) = model_dir() else {
        eprintln!("SKIP: model not found (run scripts/fetch-model.ps1)");
        return;
    };
    let c = corpus();
    let pages: Vec<PageText> = c["pages"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, t)| PageText { page: i as u32 + 1, text: t.as_str().unwrap().to_string() })
        .collect();
    let n = pages.len() as u32;

    let version = model_version(Some(&dir));
    let mut engine = Engine::new(
        Cache::open_in_memory().unwrap(),
        Box::new(move || OnnxEmbedder::load(&dir).map(|e| Box::new(e) as Box<dyn Embed>)),
        &version,
    );
    let mut out = Vec::new();
    let mut emit = |r: Response| out.push(r);
    let t = Instant::now();
    engine.handle(
        Request::IndexBegin { id: 1, sha256: SHA.into(), filename: "fixture.pdf".into(), page_count: n },
        &mut emit,
    );
    engine.handle(Request::IndexPages { id: 2, sha256: SHA.into(), pages }, &mut emit);
    engine.handle(Request::IndexEnd { id: 3, sha256: SHA.into() }, &mut emit);
    eprintln!("indexing (incl. model load): {} ms", t.elapsed().as_millis());
    assert!(matches!(out.last(), Some(Response::Ready { .. })), "{:?}", out.last());

    let mut hit1 = 0;
    let mut hit3 = 0;
    let queries = c["queries"].as_array().unwrap();
    for q in queries {
        let text = q["q"].as_str().unwrap();
        let want = q["page"].as_u64().unwrap() as u32;
        if q["noSharedWords"].as_bool() == Some(true) {
            let shared: Vec<_> = words(text).intersection(&words(q["target"].as_str().unwrap())).cloned().collect();
            assert!(shared.is_empty(), "query and target share words: {shared:?}");
        }
        let mut res = Vec::new();
        engine.handle(Request::Search { id: 9, sha256: SHA.into(), query: text.into(), k: Some(5) }, &mut |r| {
            res.push(r)
        });
        let Some(Response::Results { results, elapsed_ms, .. }) = res.pop() else { panic!() };
        let got: Vec<u32> = results.iter().map(|r| r.page).collect();
        let rank = got.iter().position(|&p| p == want);
        if rank == Some(0) {
            hit1 += 1;
        }
        if rank.is_some_and(|r| r < 3) {
            hit3 += 1;
        }
        eprintln!(
            "{:<60} want p{want:<2} got {:?} top={:.3} ({elapsed_ms} ms)",
            text,
            &got[..3.min(got.len())],
            results[0].score
        );
        if q["noSharedWords"].as_bool() == Some(true) {
            assert!(rank.is_some_and(|r| r < 3), "no-shared-words query must hit target page in top-3");
        }
    }
    let total = queries.len();
    eprintln!("recall@1 = {hit1}/{total}, recall@3 = {hit3}/{total}");
    assert!(hit3 * 10 >= total * 8, "recall@3 below 80%");
}
