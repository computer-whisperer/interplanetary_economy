//! The circular restricted three-body problem: a massless ship moving with
//! two bodies on circular orbits about their barycenter, in the rotating
//! frame where they stand still on the x axis. Units are nondimensional:
//! their separation, their combined mass, and 1/(mean motion), so the
//! smaller body sits at x = 1 - mu. The Earth–Moon case is the model JPL's
//! periodic-orbit catalog uses (M1a.md, Orbits).

use crate::integrate::{Dop853, Tolerance};
use crate::linalg;

/// A CR3BP with mass ratio `mu` = m2 / (m1 + m2).
#[derive(Clone, Copy, Debug)]
pub struct Cr3bp {
    pub mu: f64,
}

/// Which starting coordinate stays put while a symmetric periodic orbit is
/// corrected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fix {
    X,
    Z,
}

const CORRECTION: Tolerance = Tolerance {
    rtol: 1e-13,
    atol: 1e-16,
};

impl Cr3bp {
    /// The equations of motion of `[x, y, z, vx, vy, vz]`.
    pub fn ode(&self) -> impl Fn(f64, &[f64; 6]) -> [f64; 6] + '_ {
        move |_t, s| {
            let [x, y, z, vx, vy, vz] = *s;
            let mu = self.mu;
            let (d1, d2) = (self.cube(x + mu, y, z), self.cube(x - 1.0 + mu, y, z));
            let ax = 2.0 * vy + x - (1.0 - mu) * (x + mu) / d1 - mu * (x - 1.0 + mu) / d2;
            let ay = -2.0 * vx + y - (1.0 - mu) * y / d1 - mu * y / d2;
            let az = -(1.0 - mu) * z / d1 - mu * z / d2;
            [vx, vy, vz, ax, ay, az]
        }
    }

    /// The equations of motion with the state transition matrix: `[x, y, z,
    /// vx, vy, vz, Φ]`, Φ the 6×6 sensitivity of the state to the initial
    /// state, row-major.
    pub fn ode_stm(&self) -> impl Fn(f64, &[f64; 42]) -> [f64; 42] + '_ {
        let ode = self.ode();
        move |t, s| {
            let state: [f64; 6] = s[..6].try_into().unwrap();
            let mut out = [0.0; 42];
            out[..6].copy_from_slice(&ode(t, &state));
            let u = self.hessian(&state);
            let phi = &s[6..];
            for c in 0..6 {
                for r in 0..3 {
                    out[6 + r * 6 + c] = phi[(r + 3) * 6 + c];
                    let (vx, vy) = (phi[3 * 6 + c], phi[4 * 6 + c]);
                    let coriolis = match r {
                        0 => 2.0 * vy,
                        1 => -2.0 * vx,
                        _ => 0.0,
                    };
                    out[6 + (r + 3) * 6 + c] =
                        (0..3).map(|k| u[r][k] * phi[k * 6 + c]).sum::<f64>() + coriolis;
                }
            }
            out
        }
    }

    /// Second derivatives of the effective potential at a position.
    fn hessian(&self, s: &[f64; 6]) -> [[f64; 3]; 3] {
        let [x, y, z, ..] = *s;
        let mu = self.mu;
        let mut u = [[0.0; 3]; 3];
        u[0][0] = 1.0;
        u[1][1] = 1.0;
        for (m, d) in [(1.0 - mu, [x + mu, y, z]), (mu, [x - 1.0 + mu, y, z])] {
            let r2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            let r3 = r2 * r2.sqrt();
            for (i, row) in u.iter_mut().enumerate() {
                for (k, v) in row.iter_mut().enumerate() {
                    *v += m * (3.0 * d[i] * d[k] / (r3 * r2) - if i == k { 1.0 / r3 } else { 0.0 });
                }
            }
        }
        u
    }

    /// Corrects a guess at a periodic orbit symmetric about the x-z plane:
    /// one starting on it, `(x, 0, z, 0, vy, 0)`, that crosses it again
    /// perpendicularly half a period later. Holds the `fix` coordinate and
    /// adjusts the others, `vy` and the half period by Newton's method,
    /// halving a step while it makes the miss worse. Converged when the miss
    /// is under 1e-12, or under 1e-9 once no step reduces it: integration
    /// noise, amplified by an orbit's instability, sets a floor. Planar
    /// orbits (z = 0) stay planar. Returns the start and the period.
    pub fn correct_symmetric(
        &self,
        guess: [f64; 6],
        period: f64,
        fix: Fix,
    ) -> Option<([f64; 6], f64)> {
        let planar = guess[2].abs() < 1e-12;
        // Unknowns as state indices (6 stands for the half period), and the
        // constraints at the half period: y, vx, vz.
        let free: Vec<usize> = match (planar, fix) {
            (true, _) => vec![4, 6],
            (false, Fix::X) => vec![2, 4, 6],
            (false, Fix::Z) => vec![0, 4, 6],
        };
        let rows: &[usize] = if planar { &[1, 3] } else { &[1, 3, 5] };
        let z = if planar { 0.0 } else { guess[2] };
        let mut s0 = [guess[0], 0.0, z, 0.0, guess[4], 0.0];
        let mut half = period / 2.0;
        let mut at = self.half_period_state(s0, half)?;
        let miss = |end: &[f64; 42]| rows.iter().map(|&r| end[r] * end[r]).sum::<f64>().sqrt();
        for _ in 0..40 {
            if miss(&at) < 1e-12 {
                return Some((s0, 2.0 * half));
            }
            let end: [f64; 6] = at[..6].try_into().unwrap();
            let f = self.ode()(half, &end);
            let n = free.len();
            let mut jac = vec![0.0; n * n];
            for (i, &r) in rows.iter().enumerate() {
                for (k, &u) in free.iter().enumerate() {
                    jac[i * n + k] = if u == 6 { f[r] } else { at[6 + r * 6 + u] };
                }
            }
            let mut step: Vec<f64> = rows.iter().map(|&r| -at[r]).collect();
            linalg::solve(n, &mut jac, &mut step)?;
            let mut scale = 1.0;
            loop {
                let (mut trial, mut trial_half) = (s0, half);
                for (&u, d) in free.iter().zip(&step) {
                    if u == 6 {
                        trial_half += scale * d;
                    } else {
                        trial[u] += scale * d;
                    }
                }
                if let Some(next) = self
                    .half_period_state(trial, trial_half)
                    .filter(|n| miss(n) < miss(&at))
                {
                    (s0, half, at) = (trial, trial_half, next);
                    break;
                }
                scale /= 2.0;
                if scale < 1e-3 {
                    return (miss(&at) < 1e-9).then_some((s0, 2.0 * half));
                }
            }
        }
        None
    }

    /// The state and state transition matrix `half` after `s0`.
    fn half_period_state(&self, s0: [f64; 6], half: f64) -> Option<[f64; 42]> {
        if half <= 0.0 {
            return None;
        }
        let stm = self.ode_stm();
        let mut y = [0.0; 42];
        y[..6].copy_from_slice(&s0);
        for i in 0..6 {
            y[6 + i * 7] = 1.0;
        }
        let mut run = Dop853::new(&stm, 0.0, y, CORRECTION);
        run.advance_to(&stm, half).ok()?;
        Some(run.y)
    }

    /// The Jacobi constant, conserved along any trajectory.
    pub fn jacobi(&self, s: &[f64; 6]) -> f64 {
        let [x, y, z, vx, vy, vz] = *s;
        let mu = self.mu;
        let r1 = ((x + mu).powi(2) + y * y + z * z).sqrt();
        let r2 = ((x - 1.0 + mu).powi(2) + y * y + z * z).sqrt();
        x * x + y * y + 2.0 * (1.0 - mu) / r1 + 2.0 * mu / r2 - (vx * vx + vy * vy + vz * vz)
    }

    /// The stability index (|λ| + 1/|λ|)/2 of the largest eigenvalue λ of a
    /// periodic orbit's monodromy matrix (its state transition matrix over
    /// one period), as JPL's catalog reports it: 1 for a stable orbit. Uses
    /// the eigenvalues' reciprocal pairs, so only traces of `m` and `m²`.
    pub fn stability_index(monodromy: &[f64; 36]) -> f64 {
        let m2 = crate::linalg::mul(6, 6, 6, monodromy, monodromy);
        let (t1, t2) = (
            (0..6).map(|i| monodromy[i * 7]).sum::<f64>(),
            (0..6).map(|i| m2[i * 7]).sum::<f64>(),
        );
        // Two eigenvalues are 1 (the orbit's own direction and the Jacobi
        // constant); the other pairs have λ + 1/λ = p, the roots of
        // p² + a p + b - 2 = 0.
        let a = 2.0 - t1;
        let b = (a * a + 2.0 - t2) / 2.0;
        let disc = (a * a - 4.0 * (b - 2.0)).max(0.0).sqrt();
        let p = (-a + disc).abs().max((-a - disc).abs()) / 2.0;
        (p / 2.0).max(1.0)
    }

    /// `r³` for the offset (x, y, z).
    fn cube(&self, x: f64, y: f64, z: f64) -> f64 {
        let r2 = x * x + y * y + z * z;
        r2 * r2.sqrt()
    }
}
