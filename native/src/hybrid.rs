//! Hybrid ranking: dense retrieval (primary) fused with BM25 (complementary) by
//! weighted Reciprocal Rank Fusion. Ranks, not raw scores, are combined, because
//! cosine and BM25 scales are not comparable.
//!
//!   RRF(d) = 1/(k + r_dense(d)) + λ · 1/(k + r_bm25(d))      (ranks are 1-based;
//!   a term is omitted when d is not in that list's candidate set)

use crate::search::top_k;

#[derive(Debug, Clone, Copy)]
pub struct HybridConfig {
    /// false = dense-only (the previous behaviour).
    pub enabled: bool,
    /// RRF stabilisation constant k.
    pub rrf_k: f32,
    /// Weight λ of the BM25 list (dense has weight 1).
    pub lambda: f32,
    /// Candidates taken from each list before fusion.
    pub candidates: usize,
}

/// Best configuration of the RRF grid in tests/retrieval_eval.rs. The initially
/// requested values (k = 60, λ = 0.5) degraded purely semantic queries
/// (semantic MRR 0.890 -> 0.748), because with k = 60 dense rank differences are
/// tiny compared with the BM25 #1 bonus. k = 10, λ = 0.25 keeps dense dominant.
pub const CANDIDATE_HYBRID: HybridConfig = HybridConfig { enabled: true, rrf_k: 10.0, lambda: 0.25, candidates: 40 };

/// Production default: dense-only (identical to LegalSearch 1.0) until the
/// candidate is validated on held-out, multi-chunk documents. Set
/// `enabled: true` here, or LEGALSEARCH_HYBRID=1 in the environment, to enable it.
pub const DEFAULT_HYBRID: HybridConfig = HybridConfig { enabled: false, ..CANDIDATE_HYBRID };

impl Default for HybridConfig {
    fn default() -> Self {
        DEFAULT_HYBRID
    }
}

/// Final ranking (indices into `dense_scores`), best first, at most `k`.
///
/// `lexical` is the BM25 result (sorted, best first; may be empty). When it is
/// empty or hybrid is disabled the output is exactly the dense-only Top-K.
pub fn fuse(dense_scores: &[f32], lexical: &[(usize, f32)], cfg: &HybridConfig, k: usize) -> Vec<usize> {
    if !cfg.enabled || lexical.is_empty() {
        return top_k(dense_scores, k).into_iter().map(|(i, _)| i).collect();
    }
    let dense = top_k(dense_scores, cfg.candidates.max(k));
    let mut fused: Vec<(usize, f32)> = Vec::with_capacity(dense.len() + cfg.candidates);
    let mut pos: std::collections::HashMap<usize, usize> = std::collections::HashMap::with_capacity(dense.len() * 2);
    for (r, &(i, _)) in dense.iter().enumerate() {
        pos.insert(i, fused.len());
        fused.push((i, 1.0 / (cfg.rrf_k + (r + 1) as f32)));
    }
    for (r, &(i, _)) in lexical.iter().take(cfg.candidates).enumerate() {
        if i >= dense_scores.len() {
            continue; // defensive: never index outside the document
        }
        let contrib = cfg.lambda / (cfg.rrf_k + (r + 1) as f32);
        match pos.get(&i) {
            Some(&p) => fused[p].1 += contrib,
            None => {
                pos.insert(i, fused.len());
                fused.push((i, contrib));
            }
        }
    }
    // Ties (e.g. equal RRF) are broken by the dense score, then by position.
    fused.sort_by(|a, b| b.1.total_cmp(&a.1).then(dense_scores[b.0].total_cmp(&dense_scores[a.0])).then(a.0.cmp(&b.0)));
    fused.truncate(k);
    fused.into_iter().map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The arithmetic in these tests uses the classic RRF constants.
    const CFG: HybridConfig = HybridConfig { enabled: true, rrf_k: 60.0, lambda: 0.5, candidates: 40 };

    #[test]
    fn production_default_is_dense_only() {
        assert!(!DEFAULT_HYBRID.enabled);
        let dense = [0.1, 0.9, 0.5];
        assert_eq!(fuse(&dense, &[(0, 9.0)], &DEFAULT_HYBRID, 3), vec![1, 2, 0]);
        assert!(CANDIDATE_HYBRID.enabled && CANDIDATE_HYBRID.lambda < 1.0);
    }

    #[test]
    fn empty_lexical_is_exactly_dense_only() {
        let dense = [0.1, 0.9, 0.5, 0.7];
        assert_eq!(fuse(&dense, &[], &CFG, 3), vec![1, 3, 2]);
        let off = HybridConfig { enabled: false, ..CFG };
        assert_eq!(fuse(&dense, &[(0, 9.0)], &off, 3), vec![1, 3, 2]);
    }

    #[test]
    fn rrf_formula() {
        // dense ranks: 1 -> r1, 0 -> r2 ; bm25 ranks: 0 -> r1
        let dense = [0.8, 0.9];
        let cfg = HybridConfig { lambda: 1.0, ..CFG };
        // RRF(0) = 1/62 + 1/61 > RRF(1) = 1/61
        assert_eq!(fuse(&dense, &[(0, 3.0)], &cfg, 2), vec![0, 1]);
        // with λ = 0.5: RRF(0) = 1/62 + 0.5/61 = 0.02433 > 1/61 = 0.01639
        assert_eq!(fuse(&dense, &[(0, 3.0)], &CFG, 2), vec![0, 1]);
    }

    #[test]
    fn candidates_only_in_dense_are_kept() {
        // dense ranks: 0 -> 1, 2 -> 2, 1 -> 3 ; BM25 only knows chunk 1.
        // RRF(1) = 1/63 + 0.5/61 = 0.0241 > RRF(0) = 1/61 > RRF(2) = 1/62
        let dense = [0.9, 0.1, 0.2];
        assert_eq!(fuse(&dense, &[(1, 5.0)], &CFG, 3), vec![1, 0, 2]);
    }

    #[test]
    fn candidate_only_in_bm25() {
        // 50 chunks; chunk 49 is dense rank 50 (outside the dense Top-40) but BM25 #1.
        let dense: Vec<f32> = (0..50).map(|i| 1.0 - i as f32 * 0.01).collect();
        // λ = 0.5: RRF(49) = 0.5/61 = 0.0082 < dense rank-8 RRF 1/68 = 0.0147 -> not in Top-8.
        assert!(!fuse(&dense, &[(49, 7.0)], &CFG, 8).contains(&49));
        // λ = 1: RRF(49) = 1/61 = 0.0164 > 1/68 -> a BM25-only candidate can enter the Top-8.
        let r = fuse(&dense, &[(49, 7.0)], &HybridConfig { lambda: 1.0, ..CFG }, 8);
        assert_eq!(r[0], 0);
        assert!(r.contains(&49));
    }

    #[test]
    fn relevant_in_both_lists_wins() {
        // dense ranks: 1,3,4,0,2 ; BM25: 3 (#1), 0 (#2)
        // RRF(3) = 1/62 + 0.5/61 = 0.0243 ; RRF(0) = 1/64 + 0.5/62 = 0.0237 ; RRF(1) = 1/61 = 0.0164
        let dense = [0.2, 0.95, 0.1, 0.9, 0.3];
        let r = fuse(&dense, &[(3, 4.0), (0, 1.0)], &CFG, 5);
        assert_eq!(r, vec![3, 0, 1, 4, 2]);
    }

    #[test]
    fn agreement_beats_a_lexical_only_hit() {
        // 50 chunks. Dense #1 is BM25 #2; dense #30 is BM25 #1.
        // RRF(0) = 1/61 + 0.5/62 = 0.0245 > RRF(29) = 1/90 + 0.5/61 = 0.0193
        let dense: Vec<f32> = (0..50).map(|i| 1.0 - i as f32 * 0.01).collect();
        let r = fuse(&dense, &[(29, 9.0), (0, 8.0)], &CFG, 8);
        assert_eq!(r[..2], [0, 29]);
    }

    #[test]
    fn out_of_range_lexical_ids_are_ignored() {
        let dense = [0.3, 0.2];
        assert_eq!(fuse(&dense, &[(99, 1.0)], &CFG, 2), vec![0, 1]);
    }
}
