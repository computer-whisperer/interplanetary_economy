//! Small dense linear algebra for Newton's method on trajectories, using
//! only add, multiply, divide and square root so results are bit-identical
//! across machines (M1a.md, Determinism). Matrices are row-major slices.

/// Solves `a x = b` in place by Gaussian elimination with partial pivoting:
/// `a` is n×n, `b` becomes `x`. `None` if `a` is singular to working
/// precision.
pub fn solve(n: usize, a: &mut [f64], b: &mut [f64]) -> Option<()> {
    assert_eq!(a.len(), n * n);
    assert_eq!(b.len(), n);
    let scale = a.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    for col in 0..n {
        let pivot = (col..n)
            .max_by(|&i, &j| a[i * n + col].abs().total_cmp(&a[j * n + col].abs()))
            .unwrap();
        if a[pivot * n + col].abs() <= 1e-14 * scale {
            return None;
        }
        if pivot != col {
            for k in 0..n {
                a.swap(col * n + k, pivot * n + k);
            }
            b.swap(col, pivot);
        }
        let p = a[col * n + col];
        for row in col + 1..n {
            let f = a[row * n + col] / p;
            if f != 0.0 {
                for k in col..n {
                    a[row * n + k] -= f * a[col * n + k];
                }
                b[row] -= f * b[col];
            }
        }
    }
    for row in (0..n).rev() {
        let s: f64 = (row + 1..n).map(|k| a[row * n + k] * b[k]).sum();
        b[row] = (b[row] - s) / a[row * n + row];
    }
    Some(())
}

/// The smallest `dx` with `j dx = f`, for `j` m×n with full row rank
/// (m ≤ n): `jᵀ (j jᵀ)⁻¹ f`. The Newton step of an underdetermined
/// shooting problem.
pub fn min_norm(m: usize, n: usize, j: &[f64], f: &[f64]) -> Option<Vec<f64>> {
    assert_eq!(j.len(), m * n);
    assert_eq!(f.len(), m);
    let mut jjt = vec![0.0; m * m];
    for r in 0..m {
        for c in r..m {
            let s: f64 = (0..n).map(|k| j[r * n + k] * j[c * n + k]).sum();
            jjt[r * m + c] = s;
            jjt[c * m + r] = s;
        }
    }
    let mut y = f.to_vec();
    solve(m, &mut jjt, &mut y)?;
    Some(
        (0..n)
            .map(|k| (0..m).map(|r| j[r * n + k] * y[r]).sum())
            .collect(),
    )
}

/// A sparse matrix by rows: each row's (column, value) entries.
#[derive(Clone, Debug)]
pub struct SparseRows {
    pub cols: usize,
    pub rows: Vec<Vec<(usize, f64)>>,
}

/// [`min_norm`] for a sparse `j` whose rows touch columns only near their
/// neighbors' (multiple shooting): `j jᵀ` is then banded, and a banded
/// Cholesky factorization makes the cost linear in the number of rows.
pub fn min_norm_sparse(j: &SparseRows, f: &[f64]) -> Option<Vec<f64>> {
    let m = j.rows.len();
    assert_eq!(f.len(), m);
    // Which rows touch each column, and so the band of j jᵀ.
    let mut touching: Vec<Vec<usize>> = vec![Vec::new(); j.cols];
    for (r, row) in j.rows.iter().enumerate() {
        for &(c, _) in row {
            touching[c].push(r);
        }
    }
    let bw = touching
        .iter()
        .filter(|t| !t.is_empty())
        .map(|t| t.iter().max().unwrap() - t.iter().min().unwrap())
        .max()
        .unwrap_or(0);
    // Lower band of a = j jᵀ: a[r][r - k] at band[r * (bw + 1) + k].
    let width = bw + 1;
    let mut band = vec![0.0; m * width];
    let dense_row = |r: usize| {
        let mut v = std::collections::BTreeMap::new();
        for &(c, x) in &j.rows[r] {
            *v.entry(c).or_insert(0.0) += x;
        }
        v
    };
    let rows: Vec<_> = (0..m).map(dense_row).collect();
    for r in 0..m {
        for k in 0..=bw.min(r) {
            let s = r - k;
            band[r * width + k] = rows[r]
                .iter()
                .filter_map(|(c, x)| rows[s].get(c).map(|y| x * y))
                .sum();
        }
    }
    // Cholesky: a = l lᵀ, l in the same band storage.
    let at = |l: &[f64], r: usize, c: usize| l[r * width + (r - c)];
    for c in 0..m {
        let lo = c.saturating_sub(bw);
        let d = band[c * width] - (lo..c).map(|k| at(&band, c, k).powi(2)).sum::<f64>();
        if d <= 0.0 {
            return None;
        }
        let d = d.sqrt();
        band[c * width] = d;
        for r in c + 1..(c + width).min(m) {
            let lo = r.saturating_sub(bw);
            let s: f64 = (lo..c).map(|k| at(&band, r, k) * at(&band, c, k)).sum();
            band[r * width + (r - c)] = (band[r * width + (r - c)] - s) / d;
        }
    }
    // Forward and back substitution for y, then dx = jᵀ y.
    let mut y = f.to_vec();
    for r in 0..m {
        let lo = r.saturating_sub(bw);
        let s: f64 = (lo..r).map(|k| at(&band, r, k) * y[k]).sum();
        y[r] = (y[r] - s) / band[r * width];
    }
    for r in (0..m).rev() {
        let hi = (r + width).min(m);
        let s: f64 = (r + 1..hi).map(|k| at(&band, k, r) * y[k]).sum();
        y[r] = (y[r] - s) / band[r * width];
    }
    let mut dx = vec![0.0; j.cols];
    for (r, row) in j.rows.iter().enumerate() {
        for &(c, x) in row {
            dx[c] += x * y[r];
        }
    }
    Some(dx)
}

/// `a b` for a: n×k and b: k×m.
pub fn mul(n: usize, k: usize, m: usize, a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; n * m];
    for r in 0..n {
        for i in 0..k {
            let x = a[r * k + i];
            if x != 0.0 {
                for c in 0..m {
                    out[r * m + c] += x * b[i * m + c];
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solves_a_pivoting_system() {
        // Needs a row swap: the first pivot is zero.
        let mut a = vec![0.0, 2.0, 1.0, 1.0, 1.0, 1.0, 2.0, 1.0, 3.0];
        let x = [1.0, -2.0, 3.0];
        let mut b: Vec<f64> = (0..3)
            .map(|r| (0..3).map(|c| a[r * 3 + c] * x[c]).sum())
            .collect();
        solve(3, &mut a, &mut b).unwrap();
        for (got, want) in b.iter().zip(x) {
            assert!((got - want).abs() < 1e-14, "{b:?}");
        }
        let mut singular = vec![1.0, 2.0, 2.0, 4.0];
        assert!(solve(2, &mut singular, &mut [1.0, 2.0]).is_none());
    }

    /// The banded sparse solve agrees with the dense one on a shooting-like
    /// system: each row touches its own block and the next.
    #[test]
    fn sparse_min_norm_matches_dense() {
        let (m, n) = (12, 15);
        let mut dense = vec![0.0; m * n];
        let mut sparse = SparseRows {
            cols: n,
            rows: vec![Vec::new(); m],
        };
        for r in 0..m {
            for c in [r, r + 1, r + 3] {
                let v = 1.0 + ((r * 7 + c * 3) % 11) as f64 / 3.0;
                dense[r * n + c] = v;
                sparse.rows[r].push((c, v));
            }
        }
        let f: Vec<f64> = (0..m).map(|r| (r as f64 * 0.37).sin()).collect();
        let (a, b) = (
            min_norm(m, n, &dense, &f).unwrap(),
            min_norm_sparse(&sparse, &f).unwrap(),
        );
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-12, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn min_norm_satisfies_and_is_orthogonal_to_the_null_space() {
        // One equation, x + 2y + 2z = 9: the smallest solution is (1, 2, 2).
        let dx = min_norm(1, 3, &[1.0, 2.0, 2.0], &[9.0]).unwrap();
        for (got, want) in dx.iter().zip([1.0, 2.0, 2.0]) {
            assert!((got - want).abs() < 1e-14, "{dx:?}");
        }
    }
}
