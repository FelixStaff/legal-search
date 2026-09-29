//! Exact dense retrieval: s = V · û over L2-normalized embeddings, then Top-K.

/// In-place L2 normalization. A zero vector is left as zeros.
pub fn l2_normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Cosine similarity for arbitrary (not necessarily normalized) vectors.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let na = dot(a, a).sqrt();
    let nb = dot(b, b).sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot(a, b) / (na * nb)
    }
}

/// Row-major n×d matrix of normalized embeddings.
#[derive(Debug, Clone, Default)]
pub struct Matrix {
    pub dim: usize,
    pub data: Vec<f32>,
}

impl Matrix {
    pub fn new(dim: usize) -> Self {
        Matrix { dim, data: Vec::new() }
    }
    pub fn rows(&self) -> usize {
        if self.dim == 0 {
            0
        } else {
            self.data.len() / self.dim
        }
    }
    pub fn push(&mut self, row: &[f32]) {
        assert_eq!(row.len(), self.dim);
        self.data.extend_from_slice(row);
    }
    pub fn row(&self, i: usize) -> &[f32] {
        &self.data[i * self.dim..(i + 1) * self.dim]
    }
    /// s = V · u
    pub fn scores(&self, u: &[f32]) -> Vec<f32> {
        assert_eq!(u.len(), self.dim);
        self.data.chunks_exact(self.dim).map(|r| dot(r, u)).collect()
    }
}

/// Indices of the `k` highest scores, sorted descending. Ties keep the lower index first.
/// NaN scores are ranked last.
pub fn top_k(scores: &[f32], k: usize) -> Vec<(usize, f32)> {
    let k = k.min(scores.len());
    if k == 0 {
        return Vec::new();
    }
    let key = |s: f32| if s.is_nan() { f32::NEG_INFINITY } else { s };
    let cmp = |a: &(usize, f32), b: &(usize, f32)| key(b.1).total_cmp(&key(a.1)).then(a.0.cmp(&b.0));
    let mut idx: Vec<(usize, f32)> = scores.iter().copied().enumerate().collect();
    if k < idx.len() {
        idx.select_nth_unstable_by(k - 1, cmp);
        idx.truncate(k);
    }
    idx.sort_by(cmp);
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_gives_unit_norm() {
        let mut v = vec![3.0, 4.0, 0.0];
        l2_normalize(&mut v);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
        assert!((dot(&v, &v) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn zero_vector_stays_zero() {
        let mut v = vec![0.0; 4];
        l2_normalize(&mut v);
        assert!(v.iter().all(|x| *x == 0.0));
    }

    #[test]
    fn cosine_properties() {
        let a = [1.0, 2.0, 3.0];
        let b = [2.0, 4.0, 6.0];
        assert!((cosine(&a, &b) - 1.0).abs() < 1e-6);
        assert!((cosine(&a, &[-1.0, -2.0, -3.0]) + 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        // After normalization, dot == cosine.
        let (mut na, mut nb) = (vec![1.0, 5.0, -2.0], vec![0.5, 1.0, 3.0]);
        let c = cosine(&na, &nb);
        l2_normalize(&mut na);
        l2_normalize(&mut nb);
        assert!((dot(&na, &nb) - c).abs() < 1e-6);
    }

    #[test]
    fn matrix_scores() {
        let mut m = Matrix::new(2);
        m.push(&[1.0, 0.0]);
        m.push(&[0.0, 1.0]);
        m.push(&[std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2]);
        assert_eq!(m.rows(), 3);
        let s = m.scores(&[1.0, 0.0]);
        assert_eq!(s[0], 1.0);
        assert_eq!(s[1], 0.0);
        assert!((s[2] - 0.70710677).abs() < 1e-6);
    }

    #[test]
    fn top_k_basic() {
        let s = [0.1, 0.9, 0.5, 0.7, 0.3];
        let t = top_k(&s, 3);
        assert_eq!(t.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 3, 2]);
    }

    #[test]
    fn top_k_edges() {
        assert!(top_k(&[], 5).is_empty());
        assert!(top_k(&[1.0], 0).is_empty());
        assert_eq!(top_k(&[0.2, 0.4], 10).len(), 2);
        // ties: lower index first
        let t = top_k(&[0.5, 0.5, 0.5], 2);
        assert_eq!(t.iter().map(|x| x.0).collect::<Vec<_>>(), vec![0, 1]);
        // NaN last
        let t = top_k(&[f32::NAN, 0.1, -0.5], 3);
        assert_eq!(t.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 2, 0]);
    }

    #[test]
    fn top_k_matches_full_sort() {
        // Deterministic pseudo-random scores.
        let mut x = 12345u32;
        let s: Vec<f32> = (0..1000)
            .map(|_| {
                x = x.wrapping_mul(1103515245).wrapping_add(12345);
                (x >> 8) as f32 / (1u32 << 24) as f32
            })
            .collect();
        let mut full: Vec<usize> = (0..s.len()).collect();
        full.sort_by(|&a, &b| s[b].total_cmp(&s[a]).then(a.cmp(&b)));
        let t: Vec<usize> = top_k(&s, 10).into_iter().map(|x| x.0).collect();
        assert_eq!(t, full[..10].to_vec());
    }
}
