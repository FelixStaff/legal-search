//! Page-aware chunking.
//!
//! Invariant: a chunk never mixes text from two pages. Each page is split into
//! sentence-like units (a purely structural split, never a relevance decision),
//! which are greedily packed into chunks of at most `max_tokens` model tokens,
//! with roughly `overlap_tokens` of trailing context repeated in the next chunk.

use crate::protocol::PageText;

pub trait TokenCounter {
    fn count(&self, text: &str) -> usize;
}

#[derive(Debug, Clone, Copy)]
pub struct ChunkConfig {
    pub max_tokens: usize,
    pub overlap_tokens: usize,
    /// Chunks with fewer alphanumeric chars than this are dropped (page numbers, stray headers).
    pub min_alnum_chars: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        ChunkConfig { max_tokens: 200, overlap_tokens: 40, min_alnum_chars: 12 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub page: u32,
    /// Index of the chunk within the whole document (0-based, in reading order).
    pub chunk_index: u32,
    pub text: String,
}

/// Collapses all whitespace runs to single spaces and trims.
pub fn normalize_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for w in s.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

/// Splits normalized text after sentence punctuation followed by a space.
fn split_units(text: &str) -> Vec<&str> {
    let mut units = Vec::new();
    let mut start = 0;
    let mut prev_end_punct = false;
    for (i, c) in text.char_indices() {
        if c == ' ' && prev_end_punct {
            let u = text[start..i].trim();
            if !u.is_empty() {
                units.push(u);
            }
            start = i + 1;
        }
        prev_end_punct = matches!(c, '.' | '?' | '!' | ';' | '…');
    }
    let u = text[start..].trim();
    if !u.is_empty() {
        units.push(u);
    }
    units
}

/// Splits one over-long unit into word windows that each fit `max`.
fn split_long<'a, T: TokenCounter>(unit: &'a str, max: usize, tc: &T) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut cur: Vec<&'a str> = Vec::new();
    let mut cur_tokens = 0;
    for w in unit.split(' ') {
        let wt = tc.count(w).max(1);
        if cur_tokens + wt > max && !cur.is_empty() {
            out.push((cur.join(" "), cur_tokens));
            cur.clear();
            cur_tokens = 0;
        }
        // A single "word" larger than max (e.g. a giant URL) is hard-cut by chars.
        if wt > max {
            let chars: Vec<char> = w.chars().collect();
            let per = (chars.len() * max / wt).max(1);
            for piece in chars.chunks(per) {
                let s: String = piece.iter().collect();
                let t = tc.count(&s);
                out.push((s, t));
            }
            continue;
        }
        cur.push(w);
        cur_tokens += wt;
    }
    if !cur.is_empty() {
        out.push((cur.join(" "), cur_tokens));
    }
    out
}

fn alnum_count(s: &str) -> usize {
    s.chars().filter(|c| c.is_alphanumeric()).count()
}

/// Chunks a single page. `next_index` is the running document-level chunk index.
pub fn chunk_page<T: TokenCounter>(page: &PageText, cfg: &ChunkConfig, tc: &T, next_index: &mut u32) -> Vec<Chunk> {
    let text = normalize_ws(&page.text);
    if text.is_empty() {
        return Vec::new();
    }
    let mut units: Vec<(String, usize)> = Vec::new();
    for u in split_units(&text) {
        let t = tc.count(u);
        if t > cfg.max_tokens {
            units.extend(split_long(u, cfg.max_tokens, tc));
        } else {
            units.push((u.to_string(), t));
        }
    }

    let mut chunks = Vec::new();
    let mut cur: Vec<usize> = Vec::new(); // indices into units
    let mut cur_tokens = 0usize;
    let mut i = 0;
    let emit = |cur: &[usize], chunks: &mut Vec<Chunk>, next_index: &mut u32| {
        let s = cur.iter().map(|&j| units[j].0.as_str()).collect::<Vec<_>>().join(" ");
        if alnum_count(&s) >= cfg.min_alnum_chars {
            chunks.push(Chunk { page: page.page, chunk_index: *next_index, text: s });
            *next_index += 1;
        }
    };
    while i < units.len() {
        let t = units[i].1;
        if cur_tokens + t <= cfg.max_tokens || cur.is_empty() {
            cur.push(i);
            cur_tokens += t;
            i += 1;
            continue;
        }
        emit(&cur, &mut chunks, next_index);
        // Carry trailing units as overlap, never the whole previous chunk
        // (that would not make progress).
        let mut carry: Vec<usize> = Vec::new();
        let mut carry_tokens = 0;
        for &j in cur.iter().rev().take(cur.len().saturating_sub(1)) {
            if carry_tokens + units[j].1 > cfg.overlap_tokens {
                break;
            }
            carry.push(j);
            carry_tokens += units[j].1;
        }
        carry.reverse();
        // Make sure the next unit still fits after the overlap.
        while !carry.is_empty() && carry_tokens + t > cfg.max_tokens {
            carry_tokens -= units[carry[0]].1;
            carry.remove(0);
        }
        cur = carry;
        cur_tokens = carry_tokens;
    }
    if !cur.is_empty() {
        emit(&cur, &mut chunks, next_index);
    }
    chunks
}

/// Chunks a whole document. Pages are processed in ascending page order.
pub fn chunk_document<T: TokenCounter>(pages: &[PageText], cfg: &ChunkConfig, tc: &T) -> Vec<Chunk> {
    let mut sorted: Vec<&PageText> = pages.iter().collect();
    sorted.sort_by_key(|p| p.page);
    let mut idx = 0u32;
    let mut out = Vec::new();
    for p in sorted {
        out.extend(chunk_page(p, cfg, tc, &mut idx));
    }
    out
}

/// Total alphanumeric characters across pages; used to detect image-only (scanned) PDFs.
pub fn text_stats(pages: &[PageText]) -> (usize, usize) {
    let total: usize = pages.iter().map(|p| alnum_count(&p.text)).sum();
    let with_text = pages.iter().filter(|p| alnum_count(&p.text) >= 20).count();
    (total, with_text)
}

/// A document is considered to lack a usable text layer when, on average,
/// pages carry almost no text. Threshold is intentionally low: a real text page
/// carries thousands of characters, a scanned one ~0.
pub fn looks_scanned(pages: &[PageText], page_count: u32) -> bool {
    let (total, with_text) = text_stats(pages);
    let n = page_count.max(1) as usize;
    total < 50 || (total / n) < 25 || with_text * 10 < n // <10% of pages have text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test counter: one token per whitespace-separated word.
    struct Words;
    impl TokenCounter for Words {
        fn count(&self, t: &str) -> usize {
            t.split_whitespace().count()
        }
    }

    fn page(n: u32, t: &str) -> PageText {
        PageText { page: n, text: t.into() }
    }

    fn sentence(i: usize, words: usize) -> String {
        let mut s = (0..words).map(|w| format!("w{i}x{w}")).collect::<Vec<_>>().join(" ");
        s.push('.');
        s
    }

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(normalize_ws("  a\n\tb   c \r\n"), "a b c");
    }

    #[test]
    fn empty_and_blank_pages_produce_nothing() {
        let cfg = ChunkConfig::default();
        assert!(chunk_document(&[], &cfg, &Words).is_empty());
        assert!(chunk_document(&[page(1, ""), page(2, "   \n ")], &cfg, &Words).is_empty());
        // Page number only: dropped by min_alnum_chars.
        assert!(chunk_document(&[page(3, "  37  ")], &cfg, &Words).is_empty());
    }

    #[test]
    fn short_page_is_single_chunk() {
        let cfg = ChunkConfig::default();
        let c = chunk_document(&[page(4, "Las autoridades tenían conocimiento previo del riesgo.")], &cfg, &Words);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].page, 4);
        assert_eq!(c[0].chunk_index, 0);
    }

    #[test]
    fn chunks_respect_max_and_overlap() {
        let cfg = ChunkConfig { max_tokens: 50, overlap_tokens: 12, min_alnum_chars: 1 };
        let text: String = (0..20).map(|i| sentence(i, 10)).collect::<Vec<_>>().join(" ");
        let c = chunk_document(&[page(1, &text)], &cfg, &Words);
        assert!(c.len() >= 4, "got {}", c.len());
        for ch in &c {
            assert!(Words.count(&ch.text) <= cfg.max_tokens, "chunk too long: {}", Words.count(&ch.text));
        }
        // Consecutive chunks share the last sentence (10 words <= 12 overlap).
        for w in c.windows(2) {
            let last_sentence = w[0].text.rsplit(". ").next().unwrap();
            assert!(w[1].text.starts_with(last_sentence.trim_end_matches('.')), "no overlap between chunks");
        }
        // All content is covered, in order.
        let first_words: Vec<&str> = c.iter().map(|ch| ch.text.split(' ').next().unwrap()).collect();
        assert_eq!(first_words[0], "w0x0");
        assert!(c.last().unwrap().text.ends_with("w19x9."));
    }

    #[test]
    fn never_mixes_pages_and_indices_are_sequential() {
        let cfg = ChunkConfig { max_tokens: 30, overlap_tokens: 5, min_alnum_chars: 1 };
        let p1: String = (0..6).map(|i| sentence(i, 8)).collect::<Vec<_>>().join(" ");
        let p2: String = (100..106).map(|i| sentence(i, 8)).collect::<Vec<_>>().join(" ");
        // Out of order on purpose.
        let c = chunk_document(&[page(2, &p2), page(1, &p1)], &cfg, &Words);
        for ch in &c {
            let has_p1 = ch.text.contains("w0x") || ch.text.contains("w5x");
            let has_p2 = ch.text.contains("w100x") || ch.text.contains("w105x");
            assert!(!(has_p1 && has_p2), "chunk mixes pages: {}", ch.text);
            if ch.text.contains("w10") && ch.text.contains("x") && ch.text.starts_with("w10") {
                assert_eq!(ch.page, 2);
            }
        }
        assert!(c.iter().filter(|ch| ch.page == 1).all(|ch| !ch.text.contains("w10")));
        assert!(c.iter().filter(|ch| ch.page == 2).all(|ch| ch.text.contains("w10")));
        for (i, ch) in c.iter().enumerate() {
            assert_eq!(ch.chunk_index as usize, i);
        }
        // Pages come out ascending.
        assert!(c.windows(2).all(|w| w[0].page <= w[1].page));
    }

    #[test]
    fn long_sentence_is_split() {
        let cfg = ChunkConfig { max_tokens: 20, overlap_tokens: 5, min_alnum_chars: 1 };
        let text = sentence(0, 95); // one sentence, 95 words, no punctuation inside
        let c = chunk_document(&[page(9, &text)], &cfg, &Words);
        assert!(c.len() >= 5);
        assert!(c.iter().all(|ch| Words.count(&ch.text) <= 20 && ch.page == 9));
    }

    #[test]
    fn giant_word_is_hard_cut() {
        struct Chars;
        impl TokenCounter for Chars {
            fn count(&self, t: &str) -> usize {
                t.chars().count()
            }
        }
        let cfg = ChunkConfig { max_tokens: 10, overlap_tokens: 0, min_alnum_chars: 1 };
        let c = chunk_document(&[page(1, &"a".repeat(35))], &cfg, &Chars);
        assert!(c.iter().all(|ch| ch.text.chars().count() <= 10));
        assert_eq!(c.iter().map(|ch| ch.text.len()).sum::<usize>(), 35);
    }

    #[test]
    fn scanned_detection() {
        let blank: Vec<PageText> = (1..=10).map(|i| page(i, "")).collect();
        assert!(looks_scanned(&blank, 10));
        let mostly_blank: Vec<PageText> =
            (1..=30).map(|i| page(i, if i == 1 { "Sentencia T-123 de 2020" } else { " " })).collect();
        assert!(looks_scanned(&mostly_blank, 30));
        let text: Vec<PageText> = (1..=10).map(|i| page(i, &sentence(i as usize, 60))).collect();
        assert!(!looks_scanned(&text, 10));
    }
}
