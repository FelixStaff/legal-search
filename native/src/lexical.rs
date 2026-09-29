//! BM25 lexical index over the chunks of ONE document, built in memory from the
//! chunk texts already stored in the cache (no extra on-disk format).
//!
//! It is a complementary signal only (see hybrid.rs): dense retrieval stays primary.
//! Purely statistical: no stopword lists, no synonyms, no stemming, no domain rules.
//!
//!   BM25(q, d) = Σ_{t ∈ q} IDF(t) · f(t,d)·(k1+1) / ( f(t,d) + k1·(1 − b + b·|d|/avgdl) )
//!   IDF(t)     = ln( 1 + (N − n_t + 0.5) / (n_t + 0.5) )      (non-negative variant)

use std::collections::HashMap;

pub const BM25_K1: f32 = 1.2;
pub const BM25_B: f32 = 0.75;

/// Lowercases and removes diacritics from Latin letters (á→a, ü→u, ç→c). `ñ` is a
/// distinct letter in Spanish and is kept. Other scripts pass through unchanged.
fn fold(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'ē' => 'e',
        'í' | 'ì' | 'î' | 'ï' | 'ī' => 'i',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ō' => 'o',
        'ú' | 'ù' | 'û' | 'ü' | 'ū' => 'u',
        'ý' | 'ÿ' => 'y',
        'ç' => 'c',
        _ => c,
    }
}

/// Deterministic lexical tokenizer.
///
/// - NFKC-like width folding is left to the PDF text; here: lowercase + diacritic folding.
/// - A token is a maximal run of alphanumeric characters.
/// - `.`, `/`, `-` and `,` *between two digits* stay inside the token, so
///   identifiers such as `8.1`, `1234/2020`, `05001-23-31` and `1.000` are single terms.
/// - Single-letter tokens are dropped (they carry no lexical information); single
///   digits are kept (`artículo 8`).
pub fn tokenize(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().flat_map(|c| c.to_lowercase()).map(fold).collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.is_empty() {
            let mut it = cur.chars();
            let single_letter = matches!((it.next(), it.next()), (Some(c), None) if !c.is_numeric());
            if !single_letter {
                out.push(std::mem::take(cur));
            } else {
                cur.clear();
            }
        }
    };
    for i in 0..chars.len() {
        let c = chars[i];
        if c.is_alphanumeric() {
            cur.push(c);
        } else if matches!(c, '.' | '/' | '-' | ',')
            && i > 0
            && i + 1 < chars.len()
            && chars[i - 1].is_ascii_digit()
            && chars[i + 1].is_ascii_digit()
            && !cur.is_empty()
        {
            cur.push(c);
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

pub struct Bm25Index {
    vocab: HashMap<Box<str>, u32>,
    /// postings[term] = [(doc, term frequency)], doc ids ascending.
    postings: Vec<Vec<(u32, u16)>>,
    doc_len: Vec<u32>,
    avgdl: f32,
    #[cfg(test)]
    pub(crate) fail_for_test: bool,
}

impl Bm25Index {
    pub fn build<'a, I: IntoIterator<Item = &'a str>>(docs: I) -> Self {
        let mut vocab: HashMap<Box<str>, u32> = HashMap::new();
        let mut postings: Vec<Vec<(u32, u16)>> = Vec::new();
        let mut doc_len = Vec::new();
        let mut tf: HashMap<u32, u16> = HashMap::new();
        for (d, text) in docs.into_iter().enumerate() {
            tf.clear();
            let toks = tokenize(text);
            doc_len.push(toks.len() as u32);
            for t in toks {
                let next = vocab.len() as u32;
                let id = *vocab.entry(t.into_boxed_str()).or_insert(next);
                if id as usize == postings.len() {
                    postings.push(Vec::new());
                }
                let e = tf.entry(id).or_insert(0);
                *e = e.saturating_add(1);
            }
            let mut terms: Vec<(u32, u16)> = tf.iter().map(|(&t, &f)| (t, f)).collect();
            terms.sort_unstable();
            for (t, f) in terms {
                postings[t as usize].push((d as u32, f));
            }
        }
        postings.iter_mut().for_each(|p| p.shrink_to_fit());
        vocab.shrink_to_fit();
        let n = doc_len.len().max(1) as f32;
        let avgdl = (doc_len.iter().map(|&l| l as f64).sum::<f64>() as f32 / n).max(1.0);
        Bm25Index {
            vocab,
            postings,
            doc_len,
            avgdl,
            #[cfg(test)]
            fail_for_test: false,
        }
    }

    pub fn len(&self) -> usize {
        self.doc_len.len()
    }

    pub fn is_empty(&self) -> bool {
        self.doc_len.is_empty()
    }

    /// IDF of a (tokenized) term; 0 if the term does not occur.
    pub fn idf(&self, term: &str) -> f32 {
        let Some(&t) = self.vocab.get(term) else { return 0.0 };
        let n_t = self.postings[t as usize].len() as f32;
        let n = self.doc_len.len() as f32;
        (1.0 + (n - n_t + 0.5) / (n_t + 0.5)).ln()
    }

    /// Sparse BM25 scores (doc, score) for documents sharing at least one query
    /// term, sorted by score descending then doc ascending. Empty when the query
    /// has no known term: the caller then relies on dense retrieval only.
    pub fn search(&self, query: &str) -> Vec<(usize, f32)> {
        #[cfg(test)]
        if self.fail_for_test {
            panic!("injected BM25 failure");
        }
        let mut terms = tokenize(query);
        terms.sort_unstable();
        terms.dedup();
        let mut acc: HashMap<u32, f32> = HashMap::new();
        for term in &terms {
            let Some(&t) = self.vocab.get(term.as_str()) else { continue };
            let idf = self.idf(term);
            for &(d, f) in &self.postings[t as usize] {
                let f = f as f32;
                let norm = 1.0 - BM25_B + BM25_B * self.doc_len[d as usize] as f32 / self.avgdl;
                *acc.entry(d).or_insert(0.0) += idf * f * (BM25_K1 + 1.0) / (f + BM25_K1 * norm);
            }
        }
        let mut out: Vec<(usize, f32)> =
            acc.into_iter().filter(|(_, s)| *s > 0.0).map(|(d, s)| (d as usize, s)).collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }

    /// Approximate heap footprint (bytes), for diagnostics.
    pub fn heap_bytes(&self) -> usize {
        let vocab: usize = self.vocab.keys().map(|k| k.len() + 16 + 4 + 8).sum();
        let post: usize = self.postings.iter().map(|p| p.capacity() * 8 + 24).sum();
        vocab + post + self.doc_len.capacity() * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_unicode_spanish() {
        assert_eq!(tokenize("Artículo 8.1 de la CONVENCIÓN"), ["articulo", "8.1", "de", "la", "convencion"]);
        assert_eq!(tokenize("año, niño; pingüino"), ["año", "niño", "pinguino"]);
        assert_eq!(
            tokenize("expediente 1234/2020 (rad. 05001-23-31)"),
            ["expediente", "1234/2020", "rad", "05001-23-31"]
        );
        assert_eq!(tokenize("art. 25 y 8"), ["art", "25", "8"]); // "y" dropped, digits kept
        assert_eq!(tokenize("fin. 2020. Sigue"), ["fin", "2020", "sigue"]); // sentence dot is a separator
        assert_eq!(tokenize("pro-persona"), ["pro", "persona"]); // hyphen between letters splits
        assert_eq!(tokenize("“Caso CAJAR vs. Colombia”"), ["caso", "cajar", "vs", "colombia"]);
        assert!(tokenize("  ... ¿? — ").is_empty());
        assert_eq!(tokenize("東京 Ωmega"), ["東京", "ωmega"]);
    }

    #[test]
    fn idf_rare_terms_weigh_more() {
        let idx = Bm25Index::build(["la corte decide", "la corte observa", "la sala convencionalidad"]);
        assert!(idx.idf("convencionalidad") > idx.idf("corte"));
        assert!(idx.idf("corte") > idx.idf("la"));
        assert!(idx.idf("la") > 0.0, "non-negative IDF variant");
        assert_eq!(idx.idf("inexistente"), 0.0);
        // exact value for n_t = 1, N = 3: ln(1 + 2.5/1.5)
        assert!((idx.idf("convencionalidad") - (1.0f32 + 2.5 / 1.5).ln()).abs() < 1e-6);
    }

    #[test]
    fn basic_ranking() {
        let idx = Bm25Index::build([
            "responsabilidad del estado por omision",
            "articulo 8.1 garantias judiciales plazo razonable",
            "el plazo razonable del articulo 8.1 y el articulo 25",
        ]);
        let r = idx.search("artículo 8.1");
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|(d, _)| *d == 1 || *d == 2));
        assert!(r[0].1 >= r[1].1);
        // doc 2 contains "articulo" twice
        assert_eq!(idx.search("articulo 25")[0].0, 2);
    }

    #[test]
    fn document_length_normalization() {
        let filler = " relleno".repeat(60);
        let idx = Bm25Index::build(["pro persona", &format!("pro persona{filler}"), "otra cosa"]);
        let r = idx.search("principio pro persona");
        assert_eq!(r[0].0, 0, "same tf, shorter doc must rank first");
        assert!(r[0].1 > r[1].1);
    }

    #[test]
    fn term_frequency_saturates() {
        let idx = Bm25Index::build(["riesgo", "riesgo riesgo riesgo riesgo riesgo riesgo riesgo riesgo", "nada"]);
        let r = idx.search("riesgo");
        let ratio = r.iter().map(|x| x.1).fold(0.0f32, f32::max) / r.iter().map(|x| x.1).fold(f32::MAX, f32::min);
        assert!(ratio < BM25_K1 + 1.0, "tf contribution is bounded by k1+1");
    }

    #[test]
    fn empty_and_unknown_queries() {
        let idx = Bm25Index::build(["la corte", "el tribunal"]);
        assert!(idx.search("").is_empty());
        assert!(idx.search("¿? ... —").is_empty());
        assert!(idx.search("y a o").is_empty()); // only single letters
        assert!(idx.search("inexistente palabra").is_empty());
        let empty = Bm25Index::build(std::iter::empty::<&str>());
        assert!(empty.is_empty() && empty.search("corte").is_empty());
    }

    #[test]
    fn accents_do_not_matter() {
        let idx = Bm25Index::build(["Convención Americana sobre Derechos Humanos", "otra"]);
        assert_eq!(idx.search("convencion americana")[0].0, 0);
        assert_eq!(idx.search("CONVENCIÓN")[0].0, 0);
    }
}
