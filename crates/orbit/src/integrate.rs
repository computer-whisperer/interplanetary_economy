//! Adaptive 8th-order Runge–Kutta integration: Dormand–Prince 8(5,3)
//! (Hairer, Nørsett & Wanner, "Solving Ordinary Differential Equations I",
//! II.10), with the step control and error norm of scipy's `DOP853`, so step
//! counts compare directly.
//!
//! Deterministic across machines: the step controller's 1/8th powers are
//! three square roots, so the integrator itself uses only add, multiply,
//! divide and square root (M1a.md, Determinism).
//!
//! Dense output (DOP853's 7th-order interpolant, three extra derivative
//! evaluations per step) turns a run into a [`Trajectory`] that can be
//! evaluated at any time.

use crate::dop853::{A, A_EXTRA, B, C, C_EXTRA, D, E3, E5};

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
        self.advance(&rhs, t_end, None)
    }

    /// Advances to exactly `t_end`, appending each step's interpolant to
    /// `out`.
    pub fn advance_dense(
        &mut self,
        rhs: impl Fn(f64, &[f64; N]) -> [f64; N],
        t_end: f64,
        out: &mut Vec<DenseStep<N>>,
    ) -> Result<(), StepTooSmall> {
        self.advance(&rhs, t_end, Some(out))
    }

    fn advance(
        &mut self,
        rhs: &impl Fn(f64, &[f64; N]) -> [f64; N],
        t_end: f64,
        mut out: Option<&mut Vec<DenseStep<N>>>,
    ) -> Result<(), StepTooSmall> {
        let dir = if t_end >= self.t { 1.0 } else { -1.0 };
        let mut h_abs = match self.h_abs {
            Some(h) => h,
            None => self.initial_step(rhs, t_end, dir),
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
                let (y_new, f_new, k) = self.attempt(rhs, h);
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
                    if let Some(out) = out.as_deref_mut() {
                        out.push(self.dense_step(rhs, h, &k, y_new));
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

    /// The interpolant of the step of size `h` from the current state to
    /// `y_new`, with stages `k` (scipy's `_dense_output_impl`).
    fn dense_step(
        &mut self,
        rhs: &impl Fn(f64, &[f64; N]) -> [f64; N],
        h: f64,
        k: &[[f64; N]; STAGES + 1],
        y_new: [f64; N],
    ) -> DenseStep<N> {
        let mut kx = [[0.0; N]; 16];
        kx[..=STAGES].copy_from_slice(k);
        for (e, s) in (STAGES + 1..16).enumerate() {
            let mut y = self.y;
            for (j, kj) in kx.iter().enumerate().take(s) {
                let a = A_EXTRA[e][j] * h;
                if a != 0.0 {
                    for i in 0..N {
                        y[i] += a * kj[i];
                    }
                }
            }
            kx[s] = rhs(self.t + C_EXTRA[e] * h, &y);
        }
        self.evaluations += 3;
        let mut f = [[0.0; N]; 7];
        for i in 0..N {
            let dy = y_new[i] - self.y[i];
            let (f_old, f_new) = (k[0][i], k[STAGES][i]);
            f[0][i] = dy;
            f[1][i] = h * f_old - dy;
            f[2][i] = 2.0 * dy - h * (f_new + f_old);
            for (m, row) in D.iter().enumerate() {
                f[3 + m][i] = h * row.iter().zip(&kx).map(|(d, ks)| d * ks[i]).sum::<f64>();
            }
        }
        DenseStep {
            t0: self.t,
            h,
            y0: self.y,
            f,
        }
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

/// One step's 7th-order interpolant (scipy's `Dop853DenseOutput`).
#[derive(Clone, Debug)]
pub struct DenseStep<const N: usize> {
    pub t0: f64,
    pub h: f64,
    y0: [f64; N],
    f: [[f64; N]; 7],
}

impl<const N: usize> DenseStep<N> {
    /// The state at `t`, within the step (or slightly outside it, less
    /// accurately).
    pub fn at(&self, t: f64) -> [f64; N] {
        let x = (t - self.t0) / self.h;
        std::array::from_fn(|i| {
            let f = |m: usize| self.f[m][i];
            let inner = f(4) + (1.0 - x) * (f(5) + x * f(6));
            let inner = f(2) + (1.0 - x) * (f(3) + x * inner);
            self.y0[i] + x * (f(0) + (1.0 - x) * (f(1) + x * inner))
        })
    }
}

/// A forward integration's interpolants, end to end: the solution at any
/// time in its span.
#[derive(Clone, Debug)]
pub struct Trajectory<const N: usize> {
    steps: Vec<DenseStep<N>>,
}

impl<const N: usize> Trajectory<N> {
    /// Steps from one forward run, in order.
    pub fn new(steps: Vec<DenseStep<N>>) -> Self {
        assert!(!steps.is_empty(), "a trajectory needs at least one step");
        let adjacent = |a: &DenseStep<N>, b: &DenseStep<N>| {
            (b.t0 - (a.t0 + a.h)).abs() <= 1e-12 * b.t0.abs().max(1.0)
        };
        assert!(
            steps.iter().all(|s| s.h > 0.0) && steps.windows(2).all(|w| adjacent(&w[0], &w[1])),
            "steps must run forward, end to end"
        );
        Trajectory { steps }
    }

    /// First and last times covered.
    pub fn span(&self) -> (f64, f64) {
        let last = self.steps.last().unwrap();
        (self.steps[0].t0, last.t0 + last.h)
    }

    /// The state at `t`. Panics outside the span, give or take rounding.
    pub fn at(&self, t: f64) -> [f64; N] {
        let (t0, t1) = self.span();
        let slack = 1e-12 * t0.abs().max(t1.abs()).max(1.0);
        assert!(
            t >= t0 - slack && t <= t1 + slack,
            "{t} is outside the trajectory's span {t0}..{t1}"
        );
        let k = self.steps.partition_point(|s| s.t0 <= t).saturating_sub(1);
        self.steps[k].at(t)
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

    /// The interpolant between steps is as accurate as the steps.
    #[test]
    fn dense_output_matches_kepler() {
        let y0 = [7_000.0, 0.0, 0.0, 0.0, 9.5, 1.0];
        let tol = Tolerance {
            rtol: 1e-12,
            atol: 1e-15,
        };
        let mut ode = Dop853::new(kepler, 0.0, y0, tol);
        let mut steps = Vec::new();
        ode.advance_dense(kepler, 86_400.0, &mut steps).unwrap();
        let path = Trajectory::new(steps);
        assert_eq!(path.span(), (0.0, 86_400.0));
        let mut worst: f64 = 0.0;
        for k in 0..=1000 {
            let t = k as f64 * 86.4;
            let exact = propagate(state(&y0), t, MU_EARTH).unwrap();
            worst = worst.max((state(&path.at(t)).r - exact.r).length());
        }
        println!("dense output: worst error {worst:.2e} km");
        assert!(worst < 1e-4, "{worst} km");
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
