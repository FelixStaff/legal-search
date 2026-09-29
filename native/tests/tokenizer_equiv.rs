//! Proves src/tokenizer.rs (compact, used in production) yields exactly the same
//! token ids as the Hugging Face `tokenizers` reference implementation.
//! Skipped when the model has not been fetched (scripts/fetch-model.ps1).

use legalsearch::tokenizer::SpTokenizer;
use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn tokenizer_path() -> Option<PathBuf> {
    let d =
        std::env::var_os("LEGALSEARCH_MODEL_DIR").map(PathBuf::from).unwrap_or_else(|| repo().join("models/current"));
    let p = d.join("tokenizer.json");
    p.is_file().then_some(p)
}

/// Deterministic pseudo-random strings mixing many scripts and awkward code points.
fn fuzz_inputs() -> Vec<String> {
    let pools: &[&[(u32, u32)]] = &[
        &[(0x20, 0x7e)],
        &[(0x20, 0x7e), (0xa0, 0xff)],
        &[(0x61, 0x7a), (0x300, 0x36f)],       // letters + combining marks
        &[(0x391, 0x3c9), (0x410, 0x44f)],     // Greek, Cyrillic
        &[(0x4e00, 0x4e80), (0x3040, 0x309f)], // CJK, Hiragana
        &[(0x1f600, 0x1f64f), (0x20, 0x40)],   // emoji
        &[(0xff01, 0xff5e), (0x2000, 0x206f)], // fullwidth, general punctuation, zero-width
        &[(0x0, 0x1f), (0x61, 0x7a)],          // control chars
        &[(0xfb00, 0xfb06), (0x2160, 0x217f)], // ligatures, roman numerals (NFKC)
        &[(0x600, 0x6ff), (0x900, 0x97f)],     // Arabic, Devanagari
    ];
    let mut x: u64 = 0x9e3779b97f4a7c15;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut out = Vec::new();
    for round in 0..600 {
        let pool = pools[round % pools.len()];
        let len = (next() % 60) as usize;
        let mut s = String::new();
        for _ in 0..len {
            let (a, b) = pool[(next() % pool.len() as u64) as usize];
            let cp = a + (next() % (b - a + 1) as u64) as u32;
            if let Some(c) = char::from_u32(cp) {
                s.push(c);
            }
            if next() % 5 == 0 {
                s.push(' ');
            }
        }
        out.push(s);
    }
    out
}

fn fixed_inputs() -> Vec<String> {
    let mut v: Vec<String> = [
        "",
        " ",
        "     ",
        "a",
        "query: el Estado sabía del peligro pero no hizo nada",
        "passage: Las autoridades tenían conocimiento previo de la situación de riesgo y omitieron adoptar medidas razonables.",
        "¿Dónde se habla de que el Estado conocía previamente el riesgo?",
        "Radicación: 05001-23-31-000-2014-01234-01. Art. 164 del C.P.A.C.A., § 3º, 1ª instancia",
        "comillas “tipográficas” y «latinas» — guiones – y … puntos suspensivos",
        "e\u{301}xito vs éxito, ﬁnal, ①②③, Ⅻ, ｆｕｌｌｗｉｄｔｈ",
        "tab\tnew\nline\r\ncarriage  double  spaces",
        "zero\u{200b}width\u{feff}bom",
        "literal <s> and </s> and <unk> <pad> <mask> tokens",
        "<s>",
        "x<s>",
        "<s></s>",
        "東京で弁護士が 法律 を読む 😀👍🏽",
        "supercalifragilisticoespialidosoanticonstitucionalmentepalabramuylargasinespacios",
        "ÁÉÍÓÚÑÜ áéíóúñü",
        "▁already▁has▁meta",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let corpus: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(repo().join("tests/fixtures/corpus.json")).unwrap()).unwrap();
    for p in corpus["pages"].as_array().unwrap() {
        v.push(format!("passage: {}", p.as_str().unwrap()));
    }
    for q in corpus["queries"].as_array().unwrap() {
        v.push(format!("query: {}", q["q"].as_str().unwrap()));
    }
    v
}

#[test]
fn identical_ids_to_reference() {
    let Some(path) = tokenizer_path() else {
        eprintln!("SKIP: tokenizer not found (run scripts/fetch-model.ps1)");
        return;
    };
    let reference = tokenizers::Tokenizer::from_file(&path).unwrap();
    let ours = SpTokenizer::from_file(&path).unwrap();
    eprintln!("compact tokenizer heap ≈ {} MB", ours.heap_bytes() >> 20);

    let mut inputs = fixed_inputs();
    inputs.extend(fuzz_inputs());
    let mut checked = 0;
    for text in &inputs {
        let want = reference.encode(text.as_str(), true).unwrap().get_ids().to_vec();
        let got = ours.encode_for_model(text, usize::MAX);
        assert_eq!(got, want, "mismatch for {text:?}");
        let want_plain = reference.encode(text.as_str(), false).unwrap().get_ids().len();
        assert_eq!(ours.count(text), want_plain, "count mismatch for {text:?}");
        checked += 1;
    }
    eprintln!("{checked} inputs identical");
}

#[test]
fn truncation_matches_reference() {
    let Some(path) = tokenizer_path() else { return };
    let mut reference = tokenizers::Tokenizer::from_file(&path).unwrap();
    reference.with_truncation(Some(tokenizers::TruncationParams { max_length: 512, ..Default::default() })).unwrap();
    let ours = SpTokenizer::from_file(&path).unwrap();
    let long = "passage: ".to_string() + &"La Sala observa que la entidad omitió actuar. ".repeat(120);
    let want = reference.encode(long.as_str(), true).unwrap().get_ids().to_vec();
    let got = ours.encode_for_model(&long, 512);
    assert_eq!(got.len(), 512);
    assert_eq!(got, want);
}
