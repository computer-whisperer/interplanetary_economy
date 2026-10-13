//! Adaptive 8th-order Runge–Kutta integration: Dormand–Prince 8(5,3)
//! (Hairer, Nørsett & Wanner, "Solving Ordinary Differential Equations I",
//! II.10), with the step control and error norm of scipy's `DOP853`, so step
//! counts compare directly.
//!
//! Deterministic across machines: the step controller's 1/8th powers are
//! three square roots, so the integrator itself uses only add, multiply,
//! divide and square root (M1a.md, Determinism).

use crate::dop853::{A, B, C, E3, E5};

const STAGES: usize = 12;
const SAFETY: f64 = 0.9;
const MIN_FACTOR: f64 = 0.2;
const MAX_FACTOR: f64 = 10.0;

/// Error tolerances: each component's local error is held below
/// `atol + rtol × |y|`, in the RMS sense.
#[derive(Clone, Copy, Debug)]
pub struct Tolerance {
    pub rtol: f64,
    pub atol: f64,
}

/// The step size fell below what the time variable can resolve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepTooSmall {
    pub t: f64,
}

/// `x^(1/8)`, as three square roots.
fn eighth_root(x: f64) -> f64 {
    x.sqrt().sqrt().sqrt()
}

/// RMS norm.
fn rms<const N: usize>(x: &[f64; N]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / N as f64).sqrt()
}

/// The solution of `y' = f(t, y)` from some start, advanced on request.
#[derive(Clone, Debug)]
pub struct Dop853<const N: usize> {
    pub t: f64,
    pub y: [f64; N],
    /// f(t, y).
    f: [f64; N],
    /// Magnitude of the next step to try; `None` until the first step.
    h_abs: Option<f64>,
    tol: Tolerance,
    /// Derivative evaluations, accepted steps and rejected attempts.
    pub evaluations: usize,
    pub steps: usize,
    pub rejected: usize,
}

impl<const N: usize> Dop853<N> {
    pub fn new(
        rhs: impl Fn(f64, &[f64; N]) -> [f64; N],
        t: f64,
        y: [f64; N],
        tol: Tolerance,
    ) -> Self {
        Dop853 {
            t,
            y,
            f: rhs(t, &y),
            h_abs: None,
            tol,
            evaluations: 1,
            steps: 0,
            rejected: 0,
        }
    }

    /// Advances to exactly `t_end`, forward or backward.
    pub fn advance_to(
        &mut self,
        rhs: impl Fn(f64, &[f64; N]) -> [f64; N],
        t_end: f64,
    ) -> Result<(), StepTooSmall> {
        let dir = if t_end >= self.t { 1.0 } else { -1.0 };
        let mut h_abs = match self.h_abs {
            Some(h) => h,
            None => self.initial_step(&rhs, t_end, dir),
        };
        while self.t != t_end {
            let min_step = 10.0 * (self.t.abs().next_up() - self.t.abs());
            h_abs = h_abs.max(min_step);
            let mut rejected = false;
            loop {
                if h_abs < min_step {
                    return Err(StepTooSmall { t: self.t });
                }
                let mut t_new = self.t + dir * h_abs;
                let clamped = dir * (t_new - t_end) > 0.0;
                if clamped {
                    t_new = t_end;
                }
                let h = t_new - self.t;
                let (y_new, f_new, k) = self.attempt(&rhs, h);
                let error = self.error_norm(&k, &y_new, h);
                let factor = SAFETY / eighth_root(error);
                if error < 1.0 {
                    let mut grow = if error == 0.0 {
                        MAX_FACTOR
                    } else {
                        factor.min(MAX_FACTOR)
                    };
                    if rejected {
                        grow = grow.min(1.0);
                    }
                    // A step cut short to land on t_end says nothing about
                    // the step size: keep the one proposed for the next call.
                    if !clamped {
                        h_abs = h.abs() * grow;
                    }
                    (self.t, self.y, self.f) = (t_new, y_new, f_new);
                    self.steps += 1;
                    break;
                }
                h_abs = h.abs() * factor.max(MIN_FACTOR);
                rejected = true;
                self.rejected += 1;
            }
        }
        self.h_abs = Some(h_abs);
        Ok(())
    }

    /// One step of size `h`: the new state, its derivative, and all stages
    /// (stage 12 being that derivative) for the error estimate.
    fn attempt(
        &mut self,
        rhs: &impl Fn(f64, &[f64; N]) -> [f64; N],
        h: f64,
    ) -> ([f64; N], [f64; N], [[f64; N]; STAGES + 1]) {
        let mut k = [[0.0; N]; STAGES + 1];
        k[0] = self.f;
        for s in 1..STAGES {
            let mut y = self.y;
            for (j, kj) in k.iter().enumerate().take(s) {
                let a = A[s][j] * h;
                if a != 0.0 {
                    for i in 0..N {
                        y[i] += a * kj[i];
                    }
                }
            }
            k[s] = rhs(self.t + C[s] * h, &y);
        }
        let mut y_new = self.y;
        for (s, ks) in k.iter().enumerate().take(STAGES) {
            let b = B[s] * h;
            if b != 0.0 {
                for i in 0..N {
                    y_new[i] += b * ks[i];
                }
            }
        }
        let f_new = rhs(self.t + h, &y_new);
        k[STAGES] = f_new;
        self.evaluations += STAGES;
        (y_new, f_new, k)
    }

    /// scipy's DOP853 error norm: the 5th-order estimate, damped by the
    /// 3rd-order one.
    fn error_norm(&self, k: &[[f64; N]; STAGES + 1], y_new: &[f64; N], h: f64) -> f64 {
        let (mut e5, mut e3) = ([0.0; N], [0.0; N]);
        for i in 0..N {
            let scale = self.tol.atol + self.y[i].abs().max(y_new[i].abs()) * self.tol.rtol;
            let (mut s5, mut s3) = (0.0, 0.0);
            for (s, ks) in k.iter().enumerate() {
                s5 += E5[s] * ks[i];
                s3 += E3[s] * ks[i];
            }
            e5[i] = s5 / scale;
            e3[i] = s3 / scale;
        }
        let (n5, n3) = (
            e5.iter().map(|x| x * x).sum::<f64>(),
            e3.iter().map(|x| x * x).sum::<f64>(),
        );
        if n5 == 0.0 && n3 == 0.0 {
            return 0.0;
        }
        h.abs() * n5 / ((n5 + 0.01 * n3) * N as f64).sqrt()
    }

    /// Hairer's starting step (scipy's `select_initial_step`).
    fn initial_step(
        &mut self,
        rhs: &impl Fn(f64, &[f64; N]) -> [f64; N],
        t_end: f64,
        dir: f64,
    ) -> f64 {
        let interval = (t_end - self.t).abs();
        if interval == 0.0 {
            return 0.0;
        }
        let scale: [f64; N] =
            std::array::from_fn(|i| self.tol.atol + self.y[i].abs() * self.tol.rtol);
        let d0 = rms::<N>(&std::array::from_fn(|i| self.y[i] / scale[i]));
        let d1 = rms::<N>(&std::array::from_fn(|i| self.f[i] / scale[i]));
        let h0 = if d0 < 1e-5 || d1 < 1e-5 {
            1e-6
        } else {
            0.01 * d0 / d1
        }
        .min(interval);
        let y1: [f64; N] = std::array::from_fn(|i| self.y[i] + h0 * dir * self.f[i]);
        let f1 = rhs(self.t + h0 * dir, &y1);
        self.evaluations += 1;
        let d2 = rms::<N>(&std::array::from_fn(|i| (f1[i] - self.f[i]) / scale[i])) / h0;
        let h1 = if d1 <= 1e-15 && d2 <= 1e-15 {
            (h0 * 1e-3).max(1e-6)
        } else {
            eighth_root(0.01 / d1.max(d2))
        };
        (100.0 * h0).min(h1).min(interval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::MU_EARTH;
    use crate::kepler::{State, propagate};
    use glam::DVec3;

    fn kepler(_t: f64, y: &[f64; 6]) -> [f64; 6] {
        let r = DVec3::new(y[0], y[1], y[2]);
        let a = -MU_EARTH * r / r.length().powi(3);
        [y[3], y[4], y[5], a.x, a.y, a.z]
    }

    fn state(y: &[f64; 6]) -> State {
        State {
            r: DVec3::new(y[0], y[1], y[2]),
            v: DVec3::new(y[3], y[4], y[5]),
        }
    }

    /// An eccentric orbit (perigee 7,000 km, apogee ~35,000 km) over a day,
    /// against the universal-variable solution. scipy 1.18.1's `DOP853` on
    /// the same problem takes the same steps.
    #[test]
    fn two_body_matches_kepler_and_scipy() {
        let y0 = [7_000.0, 0.0, 0.0, 0.0, 9.5, 1.0];
        let t1 = 86_400.0;
        let exact = propagate(state(&y0), t1, MU_EARTH).unwrap();
        for (rtol, max_km, scipy_evaluations, scipy_steps) in
            [(1e-9, 1e-2, 1970, 130), (1e-12, 1e-5, 3890, 288)]
        {
            let tol = Tolerance {
                rtol,
                atol: rtol * 1e-3,
            };
            let mut ode = Dop853::new(kepler, 0.0, y0, tol);
            ode.advance_to(kepler, t1).unwrap();
            let err = (state(&ode.y).r - exact.r).length();
            println!(
                "rtol {rtol:e}: {} evaluations, error {err:.2e} km",
                ode.evaluations
            );
            assert!(err < max_km, "rtol {rtol}: {err} km");
            assert_eq!(ode.t, t1);
            assert_eq!(
                (ode.evaluations, ode.steps),
                (scipy_evaluations, scipy_steps)
            );
        }
    }

    /// Integrating forward then back returns to the start, and stopping at
    /// intermediate times does not change the result much.
    #[test]
    fn backward_and_piecewise() {
        let y0 = [7_000.0, 0.0, 0.0, 0.0, 9.5, 1.0];
        let tol = Tolerance {
            rtol: 1e-12,
            atol: 1e-15,
        };
        let mut ode = Dop853::new(kepler, 0.0, y0, tol);
        ode.advance_to(kepler, 50_000.0).unwrap();
        let forward = ode.y;
        ode.advance_to(kepler, 0.0).unwrap();
        assert!((state(&ode.y).r - state(&y0).r).length() < 1e-5);
        let mut pieces = Dop853::new(kepler, 0.0, y0, tol);
        for k in 1..=10 {
            pieces.advance_to(kepler, 5_000.0 * k as f64).unwrap();
        }
        assert!((state(&pieces.y).r - state(&forward).r).length() < 1e-5);
    }
}
