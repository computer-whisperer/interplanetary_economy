//! Catalog orbits carried into the full model: reference trajectories that
//! stations follow (M1a.md, Orbits).
//!
//! A CR3BP orbit is sampled at patch points, each converted to Earth-centered
//! inertial coordinates in the real Earth–Moon geometry of its moment (the
//! instantaneous rotating, pulsating frame), and multiple shooting in the
//! full field makes the arcs between them continuous. CR3BP time is the
//! angle the Earth–Moon line has turned, so each patch's moment is when the
//! real line has turned that far (a fixed time unit drifts: JPL's, 382,981
//! s, is ~2% longer than the real Moon's 1/mean motion, and the error
//! compounds over revolutions). The result is
//! quasi-periodic: the Moon's eccentric, inclined orbit and the Sun reshape
//! the orbit a little each revolution.

use crate::ephem::{BodyId, Ephemeris};
use crate::families::{Catalog, PeriodicOrbit};
use crate::gravity::Field;
use crate::integrate::{DenseStep, Dop853, Tolerance, Trajectory};
use crate::linalg;
use crate::time::{Epoch, SECONDS_PER_DAY};
use glam::DVec3;

const TOL: Tolerance = Tolerance {
    rtol: 1e-12,
    atol: 1e-9,
};

/// The Earth–Moon rotating frame at one moment: Earth-centered axes with x
/// toward the Moon and z along the Moon's orbital angular momentum, the
/// separation, its rate, and the rotation rate.
#[derive(Clone, Copy, Debug)]
pub struct EarthMoonFrame {
    pub x: DVec3,
    pub y: DVec3,
    pub z: DVec3,
    pub distance: f64,
    pub distance_rate: f64,
    pub rate: f64,
}

impl EarthMoonFrame {
    pub fn at(eph: &Ephemeris, earth: BodyId, moon: BodyId, t: Epoch) -> Self {
        let (e, m) = (eph.barycentric(earth, t), eph.barycentric(moon, t));
        let (r, v) = (m.r - e.r, m.v - e.v);
        let h = r.cross(v);
        let distance = r.length();
        let x = r / distance;
        let z = h.normalize();
        EarthMoonFrame {
            x,
            y: z.cross(x),
            z,
            distance,
            distance_rate: r.dot(v) / distance,
            rate: h.length() / (distance * distance),
        }
    }

    /// A CR3BP state (barycentric, rotating, in units of separation and
    /// 1/rate) in Earth-centered inertial km and km/s, scaled to this
    /// moment's separation and rotation rate.
    pub fn to_inertial(&self, mu: f64, s: &[f64; 6]) -> [f64; 6] {
        let rho = DVec3::new(s[0] + mu, s[1], s[2]);
        let rho_dot = DVec3::new(s[3], s[4], s[5]);
        let axes = |v: DVec3| v.x * self.x + v.y * self.y + v.z * self.z;
        let r = self.distance * axes(rho);
        let v = self.distance_rate * axes(rho)
            + self.distance * self.rate * axes(rho_dot + DVec3::Z.cross(rho));
        [r.x, r.y, r.z, v.x, v.y, v.z]
    }

    /// The inverse of [`Self::to_inertial`].
    pub fn to_rotating(&self, mu: f64, s: &[f64; 6]) -> [f64; 6] {
        let (r, v) = (DVec3::new(s[0], s[1], s[2]), DVec3::new(s[3], s[4], s[5]));
        let local = |w: DVec3| DVec3::new(w.dot(self.x), w.dot(self.y), w.dot(self.z));
        let rho = local(r) / self.distance;
        let rho_dot = (local(v) - self.distance_rate * rho) / (self.distance * self.rate)
            - DVec3::Z.cross(rho);
        [rho.x - mu, rho.y, rho.z, rho_dot.x, rho_dot.y, rho_dot.z]
    }
}

/// A trajectory in the full model, Earth-centered, km and km/s, with time in
/// seconds after `epoch`.
#[derive(Clone, Debug)]
pub struct Reference {
    pub epoch: Epoch,
    /// Patch points: (seconds after `epoch`, state).
    pub patches: Vec<(f64, [f64; 6])>,
    pub path: Trajectory<6>,
}

impl Reference {
    pub fn state(&self, t: Epoch) -> [f64; 6] {
        self.path.at((t.0 - self.epoch.0) * SECONDS_PER_DAY)
    }

    pub fn end(&self) -> Epoch {
        Epoch(self.epoch.0 + self.path.span().1 / SECONDS_PER_DAY)
    }
}

/// Multiple shooting did not make the arcs continuous.
#[derive(Clone, Debug, PartialEq)]
pub struct NotContinuous {
    /// The worst discontinuity left, scaled (1 = the Earth–Moon distance or
    /// velocity unit).
    pub residual: f64,
}

/// Seconds after `from` (negative for a negative `angle`) when the
/// Earth–Moon line has turned `angle` radians: in steps of under half a
/// radian, each found by Newton's method on the angle turned.
fn turn_time(
    eph: &Ephemeris,
    earth: BodyId,
    moon: BodyId,
    from: Epoch,
    angle: f64,
) -> Result<f64, NotContinuous> {
    let frame = |t: f64| EarthMoonFrame::at(eph, earth, moon, Epoch(from.0 + t / SECONDS_PER_DAY));
    let steps = (angle.abs() / 0.5).ceil().max(1.0) as usize;
    let step = angle / steps as f64;
    let (mut t, mut at) = (0.0, frame(0.0));
    for _ in 0..steps {
        let turned = |f: &EarthMoonFrame| at.x.cross(f.x).dot(at.z).atan2(at.x.dot(f.x));
        let mut dt = step / at.rate;
        let mut converged = false;
        for _ in 0..20 {
            let f = frame(t + dt);
            let miss = turned(&f) - step;
            // Epochs resolve ~1 µs, ~3e-12 rad of the Moon's motion.
            if miss.abs() < 1e-11 {
                converged = true;
                break;
            }
            dt -= miss / f.rate;
        }
        if !converged {
            return Err(NotContinuous {
                residual: f64::INFINITY,
            });
        }
        t += dt;
        at = frame(t);
    }
    Ok(t)
}

/// `orbit` carried into `field` (Earth-centered) for `revs` revolutions from
/// `start`, with `per_rev` patch points per revolution.
///
/// Variable-time multiple shooting: the patch states and epochs are both
/// unknowns (the first patch's epoch stays at `start`), so the solver can
/// shift when the ship reaches each point, as well as where. An arc's end
/// moves with its end epoch at the state's rate f(end) and with its start
/// epoch at -Φ f(start), which holds for the time-dependent field too; with fixed epochs, a small mismatch between the guess's and the
/// real orbit's timing accumulates over revolutions until an unstable orbit
/// departs. Newton's steps are halved while they make the discontinuities
/// worse. The shooting spans one more revolution, which is dropped: its end
/// is free, and an unstable orbit relaxes off along its unstable direction
/// there.
pub fn continue_orbit(
    eph: &Ephemeris,
    field: &Field,
    cat: &Catalog,
    orbit: PeriodicOrbit,
    start: Epoch,
    revs: usize,
    per_rev: usize,
) -> Result<Reference, NotContinuous> {
    let fail = |residual: f64| NotContinuous { residual };
    let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
    let mu = cat.system.mu;
    let n = (revs + 1) * per_rev;
    let dt = orbit.period / per_rev as f64;
    // Catalog orbits start on the x-z plane (apolune for an NRHO), so a
    // half-step offset keeps patch points off the plane crossings, where a
    // perilune pass makes the guess most sensitive.
    let offset = dt / 2.0;
    // The CR3BP orbit at the patch times, each in its moment's geometry. One
    // period, evaluated modulo the period: integrated straight on, an
    // unstable orbit would leave itself within a few revolutions.
    let cr3bp = cat.system.ode();
    let mut run = Dop853::new(&cr3bp, 0.0, orbit.state, TOL);
    let mut one_period = Vec::new();
    run.advance_dense(&cr3bp, orbit.period, &mut one_period)
        .map_err(|_| fail(f64::INFINITY))?;
    let one_period = Trajectory::new(one_period);
    let mut times = Vec::with_capacity(n + 1);
    let mut states = Vec::with_capacity(n + 1);
    for k in 0..=n {
        let phase = (offset + k as f64 * dt).rem_euclid(orbit.period);
        let seconds = turn_time(eph, earth, moon, start, k as f64 * dt)?;
        let frame =
            EarthMoonFrame::at(eph, earth, moon, Epoch(start.0 + seconds / SECONDS_PER_DAY));
        times.push(seconds);
        states.push(frame.to_inertial(mu, &one_period.at(phase)));
    }
    let scale = [cat.length_km, cat.length_km / cat.time_s];
    let unit = |i: usize| scale[i / 3];
    let (ode, stm) = (field.ode(start), field.ode_stm(start));
    // Each arc's end (with its state transition matrix when asked), and the
    // scaled discontinuities.
    let arcs = |times: &[f64],
                states: &[[f64; 6]],
                with_stm: bool|
     -> Option<(Vec<[f64; 42]>, Vec<f64>)> {
        let mut ends = Vec::with_capacity(n);
        let mut f = vec![0.0; 6 * n];
        for i in 0..n {
            let mut y = [0.0; 42];
            y[..6].copy_from_slice(&states[i]);
            if with_stm {
                for d in 0..6 {
                    y[6 + d * 7] = 1.0;
                }
                let mut arc = Dop853::new(&stm, times[i], y, TOL);
                arc.advance_to(&stm, times[i + 1]).ok()?;
                y = arc.y;
            } else {
                let mut arc = Dop853::new(&ode, times[i], states[i], TOL);
                arc.advance_to(&ode, times[i + 1]).ok()?;
                y[..6].copy_from_slice(&arc.y);
            }
            for d in 0..6 {
                f[6 * i + d] = -(y[d] - states[i + 1][d]) / unit(d);
            }
            ends.push(y);
        }
        Some((ends, f))
    };
    let squares = |f: &[f64]| f.iter().map(|x| x * x).sum::<f64>();
    let mut residual = f64::INFINITY;
    for _ in 0..40 {
        let (ends, f) = arcs(&times, &states, true).ok_or(fail(residual))?;
        residual = f.iter().fold(0.0, |a: f64, x| a.max(x.abs()));
        if residual < 1e-9 {
            break;
        }
        // Unknowns, scaled: the n + 1 states, then the epochs of patches 1
        // to n.
        let m = 6 * n;
        let cols = 6 * (n + 1) + n;
        let epoch_col = |i: usize| 6 * (n + 1) + i - 1;
        let mut jac = vec![0.0; m * cols];
        for i in 0..n {
            let end: [f64; 6] = ends[i][..6].try_into().unwrap();
            let rate_end = ode(times[i + 1], &end);
            let rate_start = ode(times[i], &states[i]);
            for r in 0..6 {
                let row = (6 * i + r) * cols;
                let phi = |c: usize| ends[i][6 + r * 6 + c];
                for c in 0..6 {
                    jac[row + 6 * i + c] = phi(c) * unit(c) / unit(r);
                }
                jac[row + 6 * (i + 1) + r] = -1.0;
                jac[row + epoch_col(i + 1)] = rate_end[r] * cat.time_s / unit(r);
                if i > 0 {
                    let drift: f64 = (0..6).map(|c| phi(c) * rate_start[c]).sum();
                    jac[row + epoch_col(i)] = -drift * cat.time_s / unit(r);
                }
            }
        }
        let step = linalg::min_norm(m, cols, &jac, &f).ok_or(fail(residual))?;
        let mut size = 1.0;
        loop {
            let mut trial_states = states.clone();
            for (k, d) in step[..6 * (n + 1)].iter().enumerate() {
                trial_states[k / 6][k % 6] += size * d * unit(k % 6);
            }
            let mut trial_times = times.clone();
            for i in 1..=n {
                trial_times[i] += size * step[epoch_col(i)] * cat.time_s;
            }
            let forward = trial_times.windows(2).all(|w| w[1] > w[0]);
            if forward
                && let Some((_, tf)) = arcs(&trial_times, &trial_states, false)
                && squares(&tf) < squares(&f)
            {
                (times, states) = (trial_times, trial_states);
                break;
            }
            size /= 2.0;
            if size < 1.0 / 64.0 {
                return Err(fail(residual));
            }
        }
    }
    if residual >= 1e-9 {
        return Err(fail(residual));
    }
    // The continuous trajectory over the kept revolutions, arc by arc.
    let kept = revs * per_rev;
    let mut steps: Vec<DenseStep<6>> = Vec::new();
    for i in 0..kept {
        let mut arc = Dop853::new(&ode, times[i], states[i], TOL);
        arc.advance_dense(&ode, times[i + 1], &mut steps)
            .map_err(|_| fail(residual))?;
    }
    Ok(Reference {
        epoch: start,
        patches: times[..=kept]
            .iter()
            .copied()
            .zip(states[..=kept].iter().copied())
            .collect(),
        path: Trajectory::new(steps),
    })
}
