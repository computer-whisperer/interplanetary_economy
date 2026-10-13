//! Lambert's problem: the conic connecting two positions in a given time.
//!
//! Izzo's algorithm (D. Izzo, "Revisiting Lambert's problem", 2015),
//! zero-revolution case, prograde about +z. Follows the structure of the
//! poliastro/lamberthub implementations, including their correction to the
//! paper's initial guess for T₁ < T < T₀.

use glam::DVec3;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug)]
pub struct LambertSolution {
    /// Velocity on the transfer conic at departure (km/s).
    pub v1: DVec3,
    /// Velocity on the transfer conic at arrival (km/s).
    pub v2: DVec3,
}

/// Solves for the prograde, zero-revolution transfer from `r1` to `r2` in
/// `tof` seconds about a body with gravitational parameter `mu`.
///
/// Prograde means angular momentum along +z; a transfer angle beyond 180°
/// (a Type II transfer) follows from the geometry. Returns `None` for
/// degenerate geometry (collinear positions) or if iteration fails.
pub fn solve(mu: f64, r1: DVec3, r2: DVec3, tof: f64) -> Option<LambertSolution> {
    let c = r2 - r1;
    let (c_n, r1_n, r2_n) = (c.length(), r1.length(), r2.length());
    let s = (r1_n + r2_n + c_n) / 2.0;
    let ir1 = r1 / r1_n;
    let ir2 = r2 / r2_n;
    let ih = ir1.cross(ir2).try_normalize()?;

    let mut lambda = (1.0 - (c_n / s).min(1.0)).sqrt();
    let (it1, it2) = if ih.z < 0.0 {
        lambda = -lambda;
        (ir1.cross(ih), ir2.cross(ih))
    } else {
        (ih.cross(ir1), ih.cross(ir2))
    };

    let t = (2.0 * mu / s.powi(3)).sqrt() * tof;
    let x = householder(initial_guess(t, lambda), t, lambda)?;
    let y = compute_y(x, lambda);

    let gamma = (mu * s / 2.0).sqrt();
    let rho = (r1_n - r2_n) / c_n;
    let sigma = (1.0 - rho * rho).sqrt();
    let vr1 = gamma * ((lambda * y - x) - rho * (lambda * y + x)) / r1_n;
    let vr2 = -gamma * ((lambda * y - x) + rho * (lambda * y + x)) / r2_n;
    let vt1 = gamma * sigma * (y + lambda * x) / r1_n;
    let vt2 = gamma * sigma * (y + lambda * x) / r2_n;
    Some(LambertSolution {
        v1: vr1 * ir1 + vt1 * it1,
        v2: vr2 * ir2 + vt2 * it2,
    })
}

/// The transfer angle swept prograde (about +z) from `r1` to `r2`, radians in
/// [0, 2π). Below π is a Type I transfer, above is Type II.
pub fn prograde_angle(r1: DVec3, r2: DVec3) -> f64 {
    let angle = (r1.dot(r2) / (r1.length() * r2.length()))
        .clamp(-1.0, 1.0)
        .acos();
    if r1.cross(r2).z < 0.0 {
        2.0 * PI - angle
    } else {
        angle
    }
}

fn compute_y(x: f64, lambda: f64) -> f64 {
    (1.0 - lambda * lambda * (1.0 - x * x)).sqrt()
}

fn initial_guess(t: f64, lambda: f64) -> f64 {
    let t0 = lambda.acos() + lambda * (1.0 - lambda * lambda).sqrt();
    let t1 = 2.0 * (1.0 - lambda.powi(3)) / 3.0;
    if t >= t0 {
        (t0 / t).powf(2.0 / 3.0) - 1.0
    } else if t < t1 {
        2.5 * t1 / t * (t1 - t) / (1.0 - lambda.powi(5)) + 1.0
    } else {
        (2f64.ln() * (t / t0).ln() / (t1 / t0).ln()).exp() - 1.0
    }
}

/// Non-dimensional time of flight for `x`, minus the target `t0`.
fn tof_residual(x: f64, y: f64, t0: f64, lambda: f64) -> f64 {
    let t = if (0.6f64.sqrt()..1.4f64.sqrt()).contains(&x) {
        // Near-parabolic: Battin's series avoids the 0/0 in the closed form.
        let eta = y - lambda * x;
        let s1 = (1.0 - lambda - x * eta) / 2.0;
        let q = 4.0 / 3.0 * hyp2f1b(s1);
        (eta.powi(3) * q + 4.0 * lambda * eta) / 2.0
    } else {
        let psi = if x < 1.0 {
            (x * y + lambda * (1.0 - x * x)).clamp(-1.0, 1.0).acos()
        } else {
            ((y - x * lambda) * (x * x - 1.0).sqrt()).asinh()
        };
        (psi / (1.0 - x * x).abs().sqrt() - x + lambda * y) / (1.0 - x * x)
    };
    t - t0
}

/// Gauss hypergeometric ₂F₁(3, 1; 5/2; z), by its series.
fn hyp2f1b(z: f64) -> f64 {
    if z >= 1.0 {
        return f64::INFINITY;
    }
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 0..1000 {
        let k = k as f64;
        term *= (3.0 + k) * (1.0 + k) / (2.5 + k) * z / (k + 1.0);
        let next = sum + term;
        if next == sum {
            break;
        }
        sum = next;
    }
    sum
}

/// Householder (quartic) iteration on the time-of-flight equation.
fn householder(mut x: f64, t0: f64, lambda: f64) -> Option<f64> {
    let l2 = lambda * lambda;
    for _ in 0..50 {
        let y = compute_y(x, lambda);
        let f = tof_residual(x, y, t0, lambda);
        let t = f + t0;
        let omx2 = 1.0 - x * x;
        let d1 = (3.0 * t * x - 2.0 + 2.0 * lambda.powi(3) * x / y) / omx2;
        let d2 = (3.0 * t + 5.0 * x * d1 + 2.0 * (1.0 - l2) * lambda.powi(3) / y.powi(3)) / omx2;
        let d3 =
            (7.0 * x * d2 + 8.0 * d1 - 6.0 * (1.0 - l2) * lambda.powi(5) * x / y.powi(5)) / omx2;
        let next =
            x - f * ((d1 * d1 - f * d2 / 2.0) / (d1 * (d1 * d1 - f * d2) + d3 * f * f / 6.0));
        if !next.is_finite() {
            return None;
        }
        if (next - x).abs() < 1e-13 {
            return Some(next);
        }
        x = next;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::{MU_EARTH, MU_SUN};
    use crate::kepler::{State, propagate};

    /// Curtis, Orbital Mechanics for Engineering Students, Example 5.2.
    #[test]
    fn curtis_example_5_2() {
        let sol = solve(
            MU_EARTH,
            DVec3::new(5000.0, 10_000.0, 2100.0),
            DVec3::new(-14_600.0, 2500.0, 7000.0),
            3600.0,
        )
        .unwrap();
        let expected_v1 = DVec3::new(-5.9925, 1.9254, 3.2456);
        let expected_v2 = DVec3::new(-3.3125, -4.1966, -0.38529);
        assert!((sol.v1 - expected_v1).length() < 1e-3, "{:?}", sol.v1);
        assert!((sol.v2 - expected_v2).length() < 1e-3, "{:?}", sol.v2);
    }

    /// Every solution, propagated from r1 for tof, must land on r2.
    #[test]
    fn solutions_reach_target() {
        let mut rng = SplitMix(7);
        let au = crate::consts::AU_KM;
        let mut checked = 0;
        for _ in 0..2000 {
            let r1 = DVec3::new(
                rng.range(-1.6, 1.6),
                rng.range(-1.6, 1.6),
                rng.range(-0.1, 0.1),
            ) * au;
            let r2 = DVec3::new(
                rng.range(-1.6, 1.6),
                rng.range(-1.6, 1.6),
                rng.range(-0.1, 0.1),
            ) * au;
            if r1.length() < 0.3 * au || r2.length() < 0.3 * au {
                continue;
            }
            let tof = rng.range(20.0, 700.0) * 86_400.0;
            let Some(sol) = solve(MU_SUN, r1, r2, tof) else {
                panic!("no solution for r1={r1:?} r2={r2:?} tof={tof}");
            };
            let end = propagate(State { r: r1, v: sol.v1 }, tof, MU_SUN);
            let miss = (end.r - r2).length();
            assert!(
                miss < 1e-6 * r2.length(),
                "miss {miss} km for r1={r1:?} r2={r2:?} tof={tof}"
            );
            assert!((end.v - sol.v2).length() < 1e-6 * sol.v2.length());
            // Prograde: angular momentum along +z.
            assert!(r1.cross(sol.v1).z > 0.0);
            checked += 1;
        }
        assert!(checked > 1500);
    }

    struct SplitMix(u64);
    impl SplitMix {
        fn range(&mut self, lo: f64, hi: f64) -> f64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            lo + (hi - lo) * (z >> 11) as f64 / (1u64 << 53) as f64
        }
    }
}
