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

    #[test]
    fn min_norm_satisfies_and_is_orthogonal_to_the_null_space() {
        // One equation, x + 2y + 2z = 9: the smallest solution is (1, 2, 2).
        let dx = min_norm(1, 3, &[1.0, 2.0, 2.0], &[9.0]).unwrap();
        for (got, want) in dx.iter().zip([1.0, 2.0, 2.0]) {
            assert!((got - want).abs() < 1e-14, "{dx:?}");
        }
    }
}
