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
//!
//! A solve lands on the member of the family nearest its guess, not on "the"
//! orbit: solves a year apart differ by hundreds to tens of thousands of km,
//! and a ship switched between them pays for it. A station's orbit is
//! therefore extended from the one it is on ([`Reference::extend`]), never
//! solved afresh (M1b.md, step 3 plan, part 1).

use crate::ephem::{BodyId, Ephemeris};
use crate::families::{Catalog, PeriodicOrbit};
use crate::gravity::Field;
use crate::integrate::{DenseStep, Dop853, Tolerance, Trajectory};
use crate::linalg::{self, SparseRows};
use crate::time::{Epoch, SECONDS_PER_DAY};
use glam::DVec3;

const TOL: Tolerance = Tolerance {
    rtol: 1e-12,
    atol: 1e-9,
};

/// Arcs count as continuous when no discontinuity exceeds this, scaled by
/// the Earth–Moon distance and velocity units: ~4 m and 0.01 mm/s. Tighter
/// is below the noise of integrating through an NRHO's perilune passes.
const CONTINUOUS: f64 = 1e-8;

/// Patch points per revolution in the multiple shooting.
pub const PATCHES_PER_REVOLUTION: usize = 16;

/// How far a catalog guess is from a solution, scaled: its arcs break by up
/// to this (an NRHO's by 8e-2, 30,000 km, near perilune; a halo's by 1e-2),
/// and the solved orbit sits 1,000–40,000 km from the CR3BP orbit
/// (measured 2026-10-17).
const GUESS_ERROR: f64 = 1e-1;

/// An anchored unknown's step is scaled by this: moving it 1 m costs the
/// minimum-norm step as much as moving a free unknown 10 km, so a solve
/// leaves its anchor revolution where the solve before put it, but for the
/// departure it inherited (below the tolerance by the choice of
/// [`settle_revolutions`]) and a like share of the rest (measured 1e-9
/// scaled for an NRHO, 1e-10 for the halos; at 1e-3 the NRHO's was 3e-8).
/// The anchor's first patch is an unknown too: fixed, an L1 halo's
/// extension does not converge. Rejected: anchoring loosely first and
/// tightening from the converged solution (a loose pass displaces the
/// anchor in directions a stiff pass can only undo by changing the whole
/// free part, which an unstable orbit makes prohibitive: 3% back an
/// iteration).
const ANCHOR_SCALE: f64 = 1e-4;

/// Revolutions a solve spans at most, the settle included. An anchored
/// solve must connect to one state, and a long one of an NRHO does not
/// converge: 83 revolutions stall at 1e-4 residual, and past its first
/// years steps of 14, 7 and 4 revolutions (40, 33 and 30 with the settle)
/// fail nearly every time where 2 (28) converge; a halo's year (36)
/// converges. [`Reference::extend`] solves in steps of this less the settle,
/// and halves a step that does not converge.
const MOST_SOLVE: usize = 40;

/// Two bodies' rotating frame at one moment (Earth and the Moon, or the Sun
/// and a planet): axes with x from the primary toward the secondary and z
/// along the secondary's orbital angular momentum about it, the
/// separation, its rate, and the rotation rate.
#[derive(Clone, Copy, Debug)]
pub struct RotatingFrame {
    pub x: DVec3,
    pub y: DVec3,
    pub z: DVec3,
    pub distance: f64,
    pub distance_rate: f64,
    pub rate: f64,
}

impl RotatingFrame {
    pub fn at(eph: &Ephemeris, primary: BodyId, secondary: BodyId, t: Epoch) -> Self {
        let (e, m) = (eph.barycentric(primary, t), eph.barycentric(secondary, t));
        let (r, v) = (m.r - e.r, m.v - e.v);
        let h = r.cross(v);
        let distance = r.length();
        let x = r / distance;
        let z = h.normalize();
        RotatingFrame {
            x,
            y: z.cross(x),
            z,
            distance,
            distance_rate: r.dot(v) / distance,
            rate: h.length() / (distance * distance),
        }
    }

    /// A CR3BP state (barycentric, rotating, in units of separation and
    /// 1/rate) in primary-centered inertial km and km/s, scaled to this
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

/// A trajectory in the full model, km and km/s, relative to `center` (the
/// solar-system barycenter if `None`), with time in seconds after `epoch`.
#[derive(Clone, Debug)]
pub struct Reference {
    pub epoch: Epoch,
    pub center: Option<BodyId>,
    pub revolutions: usize,
    /// Patch points: (seconds after `epoch`, state).
    pub patches: Vec<(f64, [f64; 6])>,
    /// The anchor revolution: the patch points of the first revolution
    /// solved past the end, from the last patch on, where [`Self::extend`]
    /// anchors. Empty for a reference built to be left as it is (a
    /// `settle` of one).
    pub ahead: Vec<(f64, [f64; 6])>,
    pub path: Trajectory<6>,
}

impl Reference {
    pub fn state(&self, t: Epoch) -> [f64; 6] {
        self.path.at((t.0 - self.epoch.0) * SECONDS_PER_DAY)
    }

    pub fn end(&self) -> Epoch {
        Epoch(self.epoch.0 + self.path.span().1 / SECONDS_PER_DAY)
    }

    /// Mean time per revolution, s.
    pub fn period_s(&self) -> f64 {
        let (t0, t1) = self.path.span();
        (t1 - t0) / self.revolutions as f64
    }

    /// The orbit extended by `revs` revolutions (M1b.md, step 3 plan, part
    /// 1): the next revolutions solved as [`continue_orbit`] solves, in
    /// steps of at most [`MOST_SOLVE`] revolutions with the settle, with the
    /// anchor revolution's patches weighted to stay ([`ANCHOR_SCALE`]), the
    /// rest guessed as the anchor's last patch flown on and then the catalog
    /// orbit phased on, and `settle` more beyond, the first of them the next
    /// anchor. The path so far does not change; the anchor moves within the
    /// shooting's tolerance. On an error the reference keeps the steps that
    /// converged (`revolutions` tells how far).
    pub fn extend(
        &mut self,
        eph: &Ephemeris,
        field: &Field,
        cat: &Catalog,
        orbit: PeriodicOrbit,
        revs: usize,
        settle: usize,
    ) -> Result<(), NotContinuous> {
        assert!(
            settle >= 1,
            "a solve's end is free: at least one revolution settles"
        );
        // A step that did not converge is halved, and the size that did
        // is kept for the steps after: an NRHO's steps of 14, 7 and 4
        // revolutions fail nearly every time where 2 converge, and each
        // failure costs seconds.
        let mut step = MOST_SOLVE.saturating_sub(settle).max(1);
        let mut left = revs;
        while left > 0 {
            step = step.min(left);
            loop {
                match self.extend_once(eph, field, cat, orbit, step, settle) {
                    Ok(()) => break,
                    Err(_) if step > 1 => step = step.div_ceil(2),
                    Err(e) => return Err(e),
                }
            }
            left -= step;
        }
        Ok(())
    }

    /// One solve of [`Self::extend`]. On an error, the reference is as it
    /// was.
    fn extend_once(
        &mut self,
        eph: &Ephemeris,
        field: &Field,
        cat: &Catalog,
        orbit: PeriodicOrbit,
        revs: usize,
        settle: usize,
    ) -> Result<(), NotContinuous> {
        assert_eq!(
            self.ahead.len(),
            PATCHES_PER_REVOLUTION + 1,
            "only a reference that kept its anchor revolution (a settle of two or more) extends"
        );
        let kept = revs * PATCHES_PER_REVOLUTION;
        let (times, states) = shoot(eph, field, cat, orbit, Start::Anchor(self), revs, settle)?;
        self.path.append(integrate(
            field,
            self.epoch,
            &times[..=kept],
            &states[..=kept],
        )?);
        // The anchor's first patch, as this solve left it, replaces the
        // record of it.
        self.patches.pop();
        self.patches
            .extend(patches(&times[..=kept], &states[..=kept]));
        self.revolutions += revs;
        self.ahead = anchor(&times, &states, kept, settle);
        Ok(())
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
    let frame = |t: f64| RotatingFrame::at(eph, earth, moon, Epoch(from.0 + t / SECONDS_PER_DAY));
    let steps = (angle.abs() / 0.5).ceil().max(1.0) as usize;
    let step = angle / steps as f64;
    let (mut t, mut at) = (0.0, frame(0.0));
    for _ in 0..steps {
        let turned = |f: &RotatingFrame| at.x.cross(f.x).dot(at.z).atan2(at.x.dot(f.x));
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

/// Revolutions a reference is solved past its usable end so that the first
/// of them, where [`Reference::extend`] anchors, is on the orbit to the
/// shooting's tolerance. A solve's end is free, so an unstable orbit leaves
/// there by the guess's error, and the error one revolution earlier is that
/// over the unstable direction's growth per revolution, λ = ν + √(ν² − 1)
/// for the catalog's stability index ν: 1 + ⌈ln([`GUESS_ERROR`] /
/// [`CONTINUOUS`]) / ln λ⌉ revolutions bring it below the tolerance (L1
/// halo 5, L2 halo 4, NRHO 26). An orbit that would take more than
/// [`MOST_SETTLE`] revolutions leaves too slowly for a settle to matter
/// within a solve, and a stable one (a DRO, ν = 1 + 2e-10 in the catalog)
/// is as good at its end as anywhere: two revolutions, the anchor and the
/// free end. Solves that long would not converge either: a DRO's of 48
/// revolutions does not, of 42 does.
pub fn settle_revolutions(stability: f64) -> usize {
    let lambda = stability + (stability * stability - 1.0).max(0.0).sqrt();
    let decay = (GUESS_ERROR / CONTINUOUS).ln() / lambda.ln();
    match decay <= MOST_SETTLE as f64 {
        true => 1 + decay.ceil().max(1.0) as usize,
        false => 2,
    }
}

/// See [`settle_revolutions`].
const MOST_SETTLE: usize = 40;

/// `orbit` carried into `field` (Earth-centered) for `revs` revolutions from
/// `start`, solved `settle` revolutions further (at least one): the first of
/// those is kept as the anchor revolution if `settle` is two or more
/// ([`settle_revolutions`]), and the rest dropped.
///
/// Variable-time multiple shooting: the patch states and epochs are both
/// unknowns (the first patch's epoch stays at `start`), so the solver can
/// shift when the ship reaches each point, as well as where. An arc's end
/// moves with its end epoch at the state's rate f(end) and with its start
/// epoch at -Φ f(start), which holds for the time-dependent field too;
/// with fixed epochs, a small mismatch between the guess's and the real
/// orbit's timing accumulates over revolutions until an unstable orbit
/// departs. Newton's steps are halved while they make the discontinuities
/// worse. The shooting's end is free, and an unstable orbit relaxes off
/// along its unstable direction there: the `settle` revolutions take that.
pub fn continue_orbit(
    eph: &Ephemeris,
    field: &Field,
    cat: &Catalog,
    orbit: PeriodicOrbit,
    start: Epoch,
    revs: usize,
    settle: usize,
) -> Result<Reference, NotContinuous> {
    assert!(
        settle >= 1,
        "a solve's end is free: at least one revolution settles"
    );
    let kept = revs * PATCHES_PER_REVOLUTION;
    let (times, states) = shoot(eph, field, cat, orbit, Start::Catalog(start), revs, settle)?;
    Ok(Reference {
        epoch: start,
        center: field.center(),
        revolutions: revs,
        patches: patches(&times[..=kept], &states[..=kept]),
        ahead: anchor(&times, &states, kept, settle),
        path: Trajectory::new(integrate(field, start, &times[..=kept], &states[..=kept])?),
    })
}

/// What a solve starts from.
enum Start<'a> {
    /// The catalog orbit at this epoch.
    Catalog(Epoch),
    /// A reference's anchor revolution, its times counted from its epoch.
    Anchor(&'a Reference),
}

fn patches(times: &[f64], states: &[[f64; 6]]) -> Vec<(f64, [f64; 6])> {
    times.iter().copied().zip(states.iter().copied()).collect()
}

/// The anchor revolution of a solve whose usable patches end at `kept`:
/// none if only the free revolution follows.
fn anchor(times: &[f64], states: &[[f64; 6]], kept: usize, settle: usize) -> Vec<(f64, [f64; 6])> {
    match settle >= 2 {
        true => patches(
            &times[kept..=kept + PATCHES_PER_REVOLUTION],
            &states[kept..=kept + PATCHES_PER_REVOLUTION],
        ),
        false => Vec::new(),
    }
}

/// The continuous trajectory through solved patches, arc by arc.
fn integrate(
    field: &Field,
    epoch: Epoch,
    times: &[f64],
    states: &[[f64; 6]],
) -> Result<Vec<DenseStep<6>>, NotContinuous> {
    let ode = field.ode(epoch);
    let mut steps = Vec::new();
    for i in 0..times.len() - 1 {
        let mut arc = Dop853::new(&ode, times[i], states[i], TOL);
        arc.advance_dense(&ode, times[i + 1], &mut steps)
            .map_err(|_| NotContinuous {
                residual: f64::INFINITY,
            })?;
    }
    Ok(steps)
}

/// The solved patch times (seconds after the epoch) and states over `revs`
/// + `settle` revolutions from `start`.
fn shoot(
    eph: &Ephemeris,
    field: &Field,
    cat: &Catalog,
    orbit: PeriodicOrbit,
    start: Start,
    revs: usize,
    settle: usize,
) -> Result<(Vec<f64>, Vec<[f64; 6]>), NotContinuous> {
    let fail = |residual: f64| NotContinuous { residual };
    let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
    let mu = cat.system.mu;
    let per_rev = PATCHES_PER_REVOLUTION;
    let n = (revs + settle) * per_rev;
    let dt = orbit.period / per_rev as f64;
    let (epoch, anchored) = match start {
        Start::Catalog(start) => (start, 0),
        Start::Anchor(before) => (before.epoch, per_rev + 1),
    };
    let frame = |t: f64| RotatingFrame::at(eph, earth, moon, Epoch(epoch.0 + t / SECONDS_PER_DAY));
    let scale = [cat.length_km, cat.length_km / cat.time_s];
    let unit = |i: usize| scale[i / 3];
    // The CR3BP orbit at the patch times, each in its moment's geometry. One
    // period, evaluated modulo the period: integrated straight on, an
    // unstable orbit would leave itself within a few revolutions. Catalog
    // orbits start on the x-z plane (apolune for an NRHO), so a half-step
    // offset keeps patch points off the plane crossings, where a perilune
    // pass makes the guess most sensitive.
    let cr3bp = cat.system.ode();
    let mut run = Dop853::new(&cr3bp, 0.0, orbit.state, TOL);
    let mut one_period = Vec::new();
    run.advance_dense(&cr3bp, orbit.period, &mut one_period)
        .map_err(|_| fail(f64::INFINITY))?;
    let one_period = Trajectory::new(one_period);
    let offset = dt / 2.0;
    let mut times = Vec::with_capacity(n + 1);
    let mut states = Vec::with_capacity(n + 1);
    // An extension's guess is the anchor revolution, then its last patch
    // flown on for as long as the departure that patch inherited stays below
    // GUESS_ERROR: `settle` - 1 revolutions, by the settle's construction,
    // or for ever when the settle is the least (a stable orbit); then the
    // catalog orbit phased on from the anchor's last patch (an anchor's
    // patches lie a whole number of revolutions from a solve's start, so
    // they share the catalog's phases) and moved onto the anchor's member
    // (`on_member`). Rejected: flying on while within
    // GUESS_ERROR of the catalog orbit (a DRO's solved member sits that far
    // from the catalog's, so its guess fell back to the catalog and the
    // solve wandered off); the anchor revolution carried on in the rotating
    // frame (its patch times, shifted by the solve to its own lunar
    // distance, mismatch later revolutions', and an NRHO's arcs then broke
    // by 30,000 km where the catalog's break by a few thousand).
    if let Start::Anchor(before) = start {
        for &(t, s) in &before.ahead {
            times.push(t);
            states.push(s);
        }
    }
    let from = times.len();
    let (base_k, base_t) = match from {
        0 => (0, 0.0),
        _ => (from - 1, times[from - 1]),
    };
    let base_epoch = Epoch(epoch.0 + base_t / SECONDS_PER_DAY);
    let catalog_phase = |k: usize| (offset + k as f64 * dt).rem_euclid(orbit.period);
    // The catalog orbit's patch guess, moved onto the member the anchor is
    // on: by the anchor's own offset from the catalog orbit at the same
    // phase, in the rotating frame (the solved member sits up to 40,000 km
    // from the catalog's; an NRHO's extension guessed from the bare catalog
    // converged only sometimes).
    let member_offset: Vec<[f64; 6]> = (0..from)
        .map(|j| {
            let anchor = frame(times[j]).to_rotating(mu, &states[j]);
            let catalog = one_period.at(catalog_phase(j));
            std::array::from_fn(|i| anchor[i] - catalog[i])
        })
        .collect();
    let on_member = |k: usize, seconds: f64| {
        let mut rotating = one_period.at(catalog_phase(k));
        if from > 0 {
            let j = (k - 1) % per_rev + 1;
            for i in 0..6 {
                rotating[i] += member_offset[j][i];
            }
        }
        frame(seconds).to_inertial(mu, &rotating)
    };
    let ode = field.ode(epoch);
    let flown_to = match settle {
        2 => n,
        _ => from + (settle - 1) * per_rev,
    };
    let mut flown = (from > 0).then(|| Dop853::new(&ode, base_t, states[from - 1], TOL));
    for k in from..=n {
        let turned = (k - base_k) as f64 * dt;
        let seconds = base_t + turn_time(eph, earth, moon, base_epoch, turned)?;
        times.push(seconds);
        let flown_state = flown
            .as_mut()
            .filter(|_| k <= flown_to)
            .and_then(|run| run.advance_to(&ode, seconds).ok().map(|_| run.y));
        states.push(match flown_state {
            Some(s) => s,
            None => {
                flown = None;
                on_member(k, seconds)
            }
        });
    }
    // Unknowns, scaled: the n + 1 states, then the epochs of patches 1 to n
    // (the first patch's epoch stays). An anchored patch's unknowns step
    // ANCHOR_SCALE times as far per unit of the minimum-norm step.
    let cols = 6 * (n + 1) + n;
    let epoch_col = |i: usize| 6 * (n + 1) + i - 1;
    let patch_of = |c: usize| {
        if c < 6 * (n + 1) {
            c / 6
        } else {
            c - 6 * (n + 1) + 1
        }
    };
    let col_scale = |c: usize| {
        if patch_of(c) < anchored {
            ANCHOR_SCALE
        } else {
            1.0
        }
    };
    // An unknown's physical change per unit of the minimum-norm step.
    let unknown_scale = |c: usize| {
        let physical = if c < 6 * (n + 1) {
            unit(c % 6)
        } else {
            cat.time_s
        };
        physical * col_scale(c)
    };
    // The anchor's displacement from where the solve before put it. The
    // step counts it as if it were the step's own, so each iteration draws
    // the anchor back, and it ends displaced only as far as continuity ahead
    // needs: a step that only minimized itself would leave the anchor where
    // the first iterations, with the guess's whole residual to fix, pushed
    // it.
    let (origin_times, origin_states) = (times.clone(), states.clone());
    let displacement = |c: usize, times: &[f64], states: &[[f64; 6]]| -> f64 {
        if c < 6 * (n + 1) {
            let (k, d) = (c / 6, c % 6);
            if k < anchored {
                states[k][d] - origin_states[k][d]
            } else {
                0.0
            }
        } else {
            let i = c - 6 * (n + 1) + 1;
            if i < anchored {
                times[i] - origin_times[i]
            } else {
                0.0
            }
        }
    };
    let stm = field.ode_stm(epoch);
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
        if residual < CONTINUOUS {
            break;
        }
        let mut jac = SparseRows {
            cols,
            rows: Vec::with_capacity(6 * n),
        };
        for i in 0..n {
            let end: [f64; 6] = ends[i][..6].try_into().unwrap();
            let rate_end = ode(times[i + 1], &end);
            let rate_start = ode(times[i], &states[i]);
            for r in 0..6 {
                let phi = |c: usize| ends[i][6 + r * 6 + c];
                let mut row: Vec<(usize, f64)> = (0..6)
                    .map(|c| (6 * i + c, phi(c) * unit(c) / unit(r)))
                    .collect();
                row.push((6 * (i + 1) + r, -1.0));
                row.push((epoch_col(i + 1), rate_end[r] * cat.time_s / unit(r)));
                if i > 0 {
                    let drift: f64 = (0..6).map(|c| phi(c) * rate_start[c]).sum();
                    row.push((epoch_col(i), -drift * cat.time_s / unit(r)));
                }
                for (c, v) in &mut row {
                    *v *= col_scale(*c);
                }
                jac.rows.push(row);
            }
        }
        let rhs: Vec<f64> = jac
            .rows
            .iter()
            .zip(&f)
            .map(|(row, fi)| {
                fi + row
                    .iter()
                    .map(|&(c, v)| v * displacement(c, &times, &states) / unknown_scale(c))
                    .sum::<f64>()
            })
            .collect();
        let step = linalg::min_norm_sparse(&jac, &rhs).ok_or(fail(residual))?;
        let change = |c: usize| step[c] * unknown_scale(c) - displacement(c, &times, &states);
        let mut size = 1.0;
        loop {
            let mut trial_states = states.clone();
            for k in 0..6 * (n + 1) {
                trial_states[k / 6][k % 6] += size * change(k);
            }
            let mut trial_times = times.clone();
            for (i, t) in trial_times.iter_mut().enumerate().skip(1) {
                *t += size * change(epoch_col(i));
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
    if residual >= CONTINUOUS {
        return Err(fail(residual));
    }
    Ok((times, states))
}
