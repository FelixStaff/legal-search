//! Compact SentencePiece-Unigram tokenizer for XLM-R style `tokenizer.json`
//! files (multilingual-e5-*).
//!
//! Why not the `tokenizers` crate at runtime: its Unigram model keeps ~234 MB of
//! live heap for this 250k-piece vocabulary (per-node HashMap trie + maps). This
//! implementation stores all pieces in one string arena and looks prefixes up by
//! binary search: ~8 MB, same output. Equivalence with `tokenizers` is verified
//! token-by-token in tests/tokenizer_equiv.rs.
//!
//! Pipeline (identical to the HF definition in tokenizer.json):
//!   1. split out special tokens (<s>, </s>, <pad>, <unk>, <mask>), matched on raw text
//!   2. normalizer: Precompiled charsmap (spm_precompiled) + Replace(" {2,}" -> " ")
//!   3. pre-tokenizer: Metaspace("▁", prepend always, split merged-with-next)
//!   4. Unigram Viterbi per word, unknown characters fused into one <unk>
//!   5. post-processor: <s> ids </s>

use serde::Deserialize;
use spm_precompiled::Precompiled;
use std::path::Path;

const K_UNK_PENALTY: f64 = 10.0;
const META: char = '▁';

pub struct SpTokenizer {
    precompiled: Precompiled,
    /// All pieces concatenated; piece `i` is `arena[offs[i]..offs[i+1]]`.
    arena: String,
    offs: Vec<u32>,
    scores: Vec<f64>,
    /// Piece ids sorted by piece bytes (duplicates resolved to the highest id, like HF).
    sorted: Vec<u32>,
    max_piece_bytes: usize,
    min_score: f64,
    unk_id: u32,
    bos_id: u32,
    eos_id: u32,
    /// Special tokens matched on raw input, longest first.
    specials: Vec<(String, u32)>,
}

// ---- minimal typed view of tokenizer.json (only what we need)
#[derive(Deserialize)]
struct Json {
    added_tokens: Vec<AddedToken>,
    normalizer: NormalizerJson,
    model: ModelJson,
}
#[derive(Deserialize)]
struct AddedToken {
    id: u32,
    content: String,
    #[serde(default)]
    special: bool,
}
#[derive(Deserialize)]
struct NormalizerJson {
    normalizers: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
struct ModelJson {
    #[serde(rename = "type")]
    kind: String,
    unk_id: Option<u32>,
    vocab: Vec<(String, f64)>,
}

impl SpTokenizer {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("read tokenizer: {e}"))?;
        Self::from_json(&bytes)
    }

    pub fn from_json(bytes: &[u8]) -> Result<Self, String> {
        let j: Json = serde_json::from_slice(bytes).map_err(|e| format!("tokenizer json: {e}"))?;
        if j.model.kind != "Unigram" {
            return Err(format!("unsupported tokenizer model {}", j.model.kind));
        }
        let pre = j
            .normalizer
            .normalizers
            .into_iter()
            .find(|n| n.get("type").and_then(|t| t.as_str()) == Some("Precompiled"))
            .ok_or("tokenizer: no Precompiled normalizer")?;
        let b64 = pre.get("precompiled_charsmap").and_then(|v| v.as_str()).ok_or("tokenizer: no charsmap")?;
        let charsmap = base64_decode(b64).ok_or("tokenizer: bad charsmap base64")?;
        let precompiled = Precompiled::from(&charsmap).map_err(|e| format!("precompiled: {e}"))?;

        let n = j.model.vocab.len();
        let total: usize = j.model.vocab.iter().map(|(p, _)| p.len()).sum();
        let mut arena = String::with_capacity(total);
        let mut offs = Vec::with_capacity(n + 1);
        let mut scores = Vec::with_capacity(n);
        let mut min_score = f64::INFINITY;
        let mut max_piece_bytes = 0;
        for (piece, score) in &j.model.vocab {
            offs.push(arena.len() as u32);
            arena.push_str(piece);
            scores.push(*score);
            min_score = min_score.min(*score);
            max_piece_bytes = max_piece_bytes.max(piece.len());
        }
        offs.push(arena.len() as u32);
        drop(j.model.vocab);

        let mut sorted: Vec<u32> = (0..n as u32).collect();
        let piece = |i: u32| &arena.as_bytes()[offs[i as usize] as usize..offs[i as usize + 1] as usize];
        // Stable sort by bytes, then keep the LAST id of equal pieces (HF's HashMap insert overwrites).
        sorted.sort_by(|&a, &b| piece(a).cmp(piece(b)).then(a.cmp(&b)));
        let mut dedup: Vec<u32> = Vec::with_capacity(n);
        for id in sorted {
            if let Some(last) = dedup.last_mut() {
                if piece(*last) == piece(id) {
                    *last = id;
                    continue;
                }
            }
            dedup.push(id);
        }
        dedup.shrink_to_fit();

        let mut specials: Vec<(String, u32)> =
            j.added_tokens.into_iter().filter(|t| t.special).map(|t| (t.content, t.id)).collect();
        specials.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        let find = |s: &str| specials.iter().find(|(c, _)| c == s).map(|(_, id)| *id);
        let bos_id = find("<s>").ok_or("no <s>")?;
        let eos_id = find("</s>").ok_or("no </s>")?;
        let unk_id = j.model.unk_id.ok_or("no unk id")?;

        Ok(SpTokenizer {
            precompiled,
            arena,
            offs,
            scores,
            sorted: dedup,
            max_piece_bytes,
            min_score,
            unk_id,
            bos_id,
            eos_id,
            specials,
        })
    }

    fn piece(&self, id: u32) -> &[u8] {
        &self.arena.as_bytes()[self.offs[id as usize] as usize..self.offs[id as usize + 1] as usize]
    }

    fn lookup(&self, key: &[u8]) -> Option<u32> {
        self.sorted.binary_search_by(|&id| self.piece(id).cmp(key)).ok().map(|i| self.sorted[i])
    }

    /// Precompiled charsmap + collapse runs of 2+ spaces.
    fn normalize(&self, s: &str) -> String {
        let n = self.precompiled.normalize_string(s);
        let mut out = String::with_capacity(n.len());
        let mut spaces = 0;
        for c in n.chars() {
            if c == ' ' {
                spaces += 1;
                continue;
            }
            if spaces > 0 {
                out.push(' ');
                spaces = 0;
            }
            out.push(c);
        }
        if spaces > 0 {
            out.push(' ');
        }
        out
    }

    /// Viterbi segmentation of one Metaspace word (HF `encode_optimized`).
    fn encode_word(&self, word: &str, out: &mut Vec<u32>) {
        let bytes = word.as_bytes();
        let size = bytes.len();
        let unk_score = self.min_score - K_UNK_PENALTY;
        const NONE: usize = usize::MAX;
        // (best score, start of last piece, piece id)
        let mut best: Vec<(f64, usize, u32)> = vec![(0.0, NONE, 0); size + 1];
        let mut start = 0;
        while start < size {
            let base = best[start].0;
            let mblen = word[start..].chars().next().map(char::len_utf8).unwrap_or(1);
            let mut has_single = false;
            let max_len = self.max_piece_bytes.min(size - start);
            for len in 1..=max_len {
                let end = start + len;
                if !word.is_char_boundary(end) {
                    continue;
                }
                if let Some(id) = self.lookup(&bytes[start..end]) {
                    let cand = self.scores[id as usize] + base;
                    let node = &mut best[end];
                    if node.1 == NONE || cand > node.0 {
                        *node = (cand, start, id);
                    }
                    if len == mblen {
                        has_single = true;
                    }
                }
            }
            if !has_single {
                let cand = unk_score + base;
                let node = &mut best[start + mblen];
                if node.1 == NONE || cand > node.0 {
                    *node = (cand, start, self.unk_id);
                }
            }
            start += mblen;
        }
        // Backtrack; consecutive unknown pieces are fused and looked up as one string.
        let mut rev: Vec<u32> = Vec::new();
        let mut end = size;
        let mut unk_start: Option<usize> = None;
        let mut unk_end = 0;
        while end > 0 {
            let (_, s, id) = best[end];
            if id == self.unk_id {
                if unk_start.is_none() {
                    unk_end = end;
                }
                unk_start = Some(s);
            } else {
                if let Some(us) = unk_start.take() {
                    rev.push(self.lookup(&bytes[us..unk_end]).unwrap_or(self.unk_id));
                }
                rev.push(id);
            }
            end = s;
        }
        if let Some(us) = unk_start {
            rev.push(self.lookup(&bytes[us..unk_end]).unwrap_or(self.unk_id));
        }
        out.extend(rev.into_iter().rev());
    }

    /// Normalizes + pre-tokenizes + segments one raw segment (no special tokens inside).
    fn encode_segment(&self, raw: &str, out: &mut Vec<u32>) {
        let normalized = self.normalize(raw);
        if normalized.is_empty() {
            return;
        }
        let mut s: String = normalized.replace(' ', "▁");
        if !s.starts_with(META) {
            s.insert(0, META);
        }
        // Split "merged with next": every ▁ (except at position 0) starts a new word.
        let mut word_start = 0;
        for (i, c) in s.char_indices() {
            if c == META && i > word_start {
                self.encode_word(&s[word_start..i], out);
                word_start = i;
            }
        }
        if word_start < s.len() {
            self.encode_word(&s[word_start..], out);
        }
    }

    /// Token ids without special tokens and without truncation.
    pub fn encode_ids(&self, text: &str) -> Vec<u32> {
        let mut out = Vec::new();
        let mut rest = text;
        loop {
            // earliest special-token occurrence (longest wins at the same position)
            let next = self
                .specials
                .iter()
                .filter_map(|(c, id)| rest.find(c.as_str()).map(|p| (p, c.len(), *id)))
                .min_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
            match next {
                Some((p, len, id)) => {
                    if p > 0 {
                        self.encode_segment(&rest[..p], &mut out);
                    }
                    out.push(id);
                    rest = &rest[p + len..];
                }
                None => {
                    if !rest.is_empty() || out.is_empty() {
                        self.encode_segment(rest, &mut out);
                    }
                    break;
                }
            }
        }
        out
    }

    /// `<s> ids </s>`, truncated so the whole sequence fits `max_len` tokens.
    pub fn encode_for_model(&self, text: &str, max_len: usize) -> Vec<u32> {
        let mut ids = self.encode_ids(text);
        ids.truncate(max_len.saturating_sub(2));
        let mut out = Vec::with_capacity(ids.len() + 2);
        out.push(self.bos_id);
        out.extend(ids);
        out.push(self.eos_id);
        out
    }

    pub fn count(&self, text: &str) -> usize {
        self.encode_ids(text).len()
    }

    pub fn pad_id(&self) -> u32 {
        self.specials.iter().find(|(c, _)| c == "<pad>").map(|(_, id)| *id).unwrap_or(1)
    }

    /// Approximate heap footprint, for diagnostics.
    pub fn heap_bytes(&self) -> usize {
        self.arena.capacity() + self.offs.capacity() * 4 + self.scores.capacity() * 8 + self.sorted.capacity() * 4
    }
}

/// Standard base64 (with padding) decoder; only used for the charsmap.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }
    let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for q in bytes.chunks(4) {
        let pad = q.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 {
            return None;
        }
        let mut n = 0u32;
        for &c in &q[..4 - pad] {
            n = (n << 6) | val(c)?;
        }
        n <<= 6 * pad as u32;
        let chunk = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        out.extend_from_slice(&chunk[..3 - pad]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::base64_decode;

    #[test]
    fn base64() {
        assert_eq!(base64_decode("").unwrap(), b"");
        assert_eq!(base64_decode("TWFu").unwrap(), b"Man");
        assert_eq!(base64_decode("TWE=").unwrap(), b"Ma");
        assert_eq!(base64_decode("TQ==").unwrap(), b"M");
        assert!(base64_decode("TQ=").is_none());
        assert!(base64_decode("T$==").is_none());
    }
}
