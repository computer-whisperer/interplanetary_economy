//! Trajectory design in the full field: impulsive burns joined by coasts,
//! made continuous by multiple shooting, with total Δv minimized (M1a.md,
//! Autopilot: Correction).
//!
//! A path is a chain of nodes: the times the ship passes them and its state
//! leaving each. Arcs between nodes are integrated in the field. At a burn
//! node only the position must be continuous (the velocity's jump is the
//! burn); at a coast node, the whole state. Coast nodes split long arcs so
//! that Newton's method sees moderate sensitivities; their times stay at
//! fixed fractions between the burn nodes around them. The ends are burns
//! that join the path to where it starts and where it goes: a station's
//! reference orbit, or a low circular orbit.
//!
//! The unknowns are the burn nodes' times and every node's state, each
//! scaled by its node's distance from the body that pulls hardest there
//! (one global scale left a LEO departure too ill-conditioned to solve). A
//! first phase finds a continuous path near the seed (minimum-norm Newton
//! steps); the second minimizes the total Δv by the generalized reduced
//! gradient method, which keeps every iterate continuous. An SQP that let
//! iterates leave the constraints, with an ℓ1 merit function, took 51
//! iterations on a Hohmann rendezvous where this takes 9.

use crate::ephem::BodyId;
use crate::gravity::Field;
use crate::integrate::{Dop853, Tolerance};
use crate::linalg;
use crate::reference::Reference;
use crate::time::{Epoch, SECONDS_PER_DAY};
use glam::DVec3;

const TOL: Tolerance = Tolerance {
    rtol: 1e-12,
    atol: 1e-9,
};

/// The path is continuous and meets its ends when no scaled residual
/// exceeds this: 0.07 mm in LEO, 4 m at the Moon's distance from Earth.
/// A tenth of it was below the noise of integrating a 5-day arc from LEO.
const FEASIBLE: f64 = 1e-8;

/// Burns count as `sqrt(|Δv|² + SMOOTH²)`, km/s, so that one can shrink to
/// nothing without a kink in the cost.
const SMOOTH: f64 = 1e-4;

/// A departure this close to a window's edge in a seed (s) starts held
/// there, as in continuation.
const HELD_S: f64 = 1.0;

/// What leaving a day outside the window adds to the cost a search sees,
/// km/s (it grows with the square of the time).
const WINDOW_PENALTY: f64 = 1.0;

/// The search stops when the quadratic model predicts the next step would
/// save less than this, km/s (0.1 mm/s): measured at the end of a lunar
/// flyby transfer's search, the cost's noise is ~1e-9 km/s.
const CONVERGED_KMS: f64 = 1e-7;

const FEASIBILITY_ITERATIONS: usize = 30;

/// Designs a continuation may take toward one duration.
const CONTINUATION_STEPS: usize = 400;
const ITERATIONS: usize = 100;

/// Where a path starts or ends.
#[derive(Clone, Copy, Debug)]
pub enum End<'a> {
    /// On a station's reference orbit, at a time within its span: a
    /// rendezvous.
    Orbit(&'a Reference),
    /// At an apsis of a transfer that meets a circular orbit there, burning
    /// along the velocity.
    Low(LowOrbit),
    /// At a body, matching its velocity: a rendezvous with a small body.
    Body(BodyId),
    /// On a hyperbola about a body with the given excess velocity (km/s),
    /// with no burn: leaving the body's system on its way out (as a leg's
    /// end), arriving into it (as a start).
    Excess { body: BodyId, v_inf: DVec3 },
    /// At the periapsis of a hyperbola about a body with excess speed
    /// `v_inf` (km/s), in any direction, at `radius`: leaving the body's
    /// system after a burn there along the velocity (as a leg's end), or
    /// arriving before one (as a start). The stand-in for escapes and
    /// captures at a given excess velocity until those can be aimed
    /// (M1a.md, step 9).
    Hyperbola {
        body: BodyId,
        radius: f64,
        v_inf: f64,
    },
    /// Through a body's atmosphere, for aero-capable ships (as a leg's end
    /// only): the arrival's periapsis (of its vacuum conic) at `radius`,
    /// where the pass brakes it, no faster than `max_speed` (km/s); `after`
    /// is the propulsive Δv that follows (an aerocapture's periapsis raise,
    /// an entry's terminal descent; see [`crate::surface`]).
    Atmosphere {
        body: BodyId,
        radius: f64,
        after: f64,
        max_speed: f64,
    },
}

/// The excess velocity of a hyperbola about a body of `gm` through `s` (a
/// state relative to it): where the ship goes, or (`outgoing` false) where
/// it came from. `None` if `s` is not on a hyperbola.
pub fn excess_velocity(gm: f64, s: &[f64; 6], outgoing: bool) -> Option<DVec3> {
    let (r, v) = (DVec3::new(s[0], s[1], s[2]), DVec3::new(s[3], s[4], s[5]));
    let speed2 = v.length_squared() - 2.0 * gm / r.length();
    if speed2 <= 0.0 {
        return None;
    }
    let e = ((v.length_squared() - gm / r.length()) * r - r.dot(v) * v) / gm;
    let ecc = e.length();
    let (p, q) = (e / ecc, r.cross(v).normalize().cross(e / ecc));
    let (cos, sin) = (-1.0 / ecc, (1.0 - 1.0 / (ecc * ecc)).sqrt());
    let way = if outgoing {
        cos * p + sin * q
    } else {
        -(cos * p - sin * q)
    };
    Some(speed2.sqrt() * way)
}

/// A circular orbit about a body: a parking orbit in LEO, a low lunar
/// orbit.
#[derive(Clone, Copy, Debug)]
pub struct LowOrbit {
    pub body: BodyId,
    /// Radius, km.
    pub radius: f64,
    /// Inclination to the body's equator, radians. `None` leaves the plane
    /// free, as for a parking orbit whose plane launch timing picks. The
    /// node is free either way.
    pub inclination: Option<f64>,
}

/// Nodes at increasing times, and the ship's state leaving each but the
/// last.
#[derive(Clone, Debug)]
pub struct Path {
    pub epoch: Epoch,
    /// Node times, s after `epoch`.
    pub times: Vec<f64>,
    /// The state leaving each node but the last (after any burn there), in
    /// the field's frame, km and km/s.
    pub states: Vec<[f64; 6]>,
    /// Whether the ship may burn at each node; the ends always do.
    pub burns: Vec<bool>,
    /// Inner burn nodes that are close passes of a body.
    pub passes: Vec<Pass>,
}

/// A burn at the periapsis of a close pass (a powered flyby), no lower than
/// `min_radius` on either side of it: held at that floor, the burn is also
/// perpendicular to the radius.
#[derive(Clone, Copy, Debug)]
pub struct Pass {
    pub node: usize,
    pub body: BodyId,
    pub min_radius: f64,
}

impl Path {
    /// A path with only coast nodes between its ends.
    pub fn coasting(epoch: Epoch, times: Vec<f64>, states: Vec<[f64; 6]>) -> Path {
        let n = times.len();
        Path {
            epoch,
            times,
            states,
            burns: (0..n).map(|k| k == 0 || k == n - 1).collect(),
            passes: Vec::new(),
        }
    }
}

/// A path to design: what it joins, which of its times are fixed, and when
/// it may leave (s after the seed's epoch).
#[derive(Clone, Copy)]
pub struct Leg<'a> {
    /// The field, whose origin the path's states are measured from.
    pub field: &'a Field<'a>,
    /// Fields centered on bodies (with the same bodies as `field`): an arc
    /// starting within one's center's sphere of influence integrates in it.
    /// Near LEO, a barycentric frame's coordinates (~1.5e8 km) leave ~15 cm
    /// of noise per step, far above what continuity there asks.
    pub locals: &'a [Field<'a>],
    pub from: End<'a>,
    pub to: End<'a>,
    pub depart: Option<f64>,
    pub arrive: Option<f64>,
    pub duration: Option<f64>,
    /// When the ship may leave. A search that would leave outside it is
    /// held at the nearer edge.
    pub window: Option<(f64, f64)>,
}

#[derive(Clone, Copy, Debug)]
pub struct Burn {
    /// s after the path's epoch.
    pub t: f64,
    /// km/s.
    pub dv: DVec3,
}

/// A designed path.
#[derive(Clone, Debug)]
pub struct Design {
    pub path: Path,
    pub burns: Vec<Burn>,
    /// Total Δv, km/s.
    pub dv: f64,
    /// Search iterations taken.
    pub iterations: usize,
}

impl Design {
    pub fn departure(&self) -> f64 {
        self.path.times[0]
    }

    pub fn duration(&self) -> f64 {
        self.path.times.last().unwrap() - self.path.times[0]
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    /// No continuous path near the seed; the worst scaled residual left.
    Infeasible(f64),
    /// The path passes below this body's surface.
    Collision(BodyId),
    /// A pass through an atmosphere faster than it survives (km/s).
    TooFast(f64),
}

/// The unknowns' layout: the burn nodes' times, then each node's state,
/// each scaled by the distance from the body that pulls hardest at the
/// node in the seed, that distance's circular speed, and their ratio.
struct Layout {
    /// Each node's time as a weighted sum of the burn nodes' time unknowns.
    time_of: Vec<Vec<(usize, f64)>>,
    burn_nodes: usize,
    nodes: usize,
    /// Each node's length and speed scales, and each time unknown's scale.
    length: Vec<f64>,
    speed: Vec<f64>,
    time: Vec<f64>,
    /// Close passes, and whether each is held at its floor.
    passes: Vec<(Pass, bool)>,
    /// Each arc's frame: an index into the leg's local fields, or the
    /// leg's field.
    frames: Vec<Option<usize>>,
}

impl Layout {
    /// The layout for `path`, whose nodes are at `positions`, with arcs in
    /// the first of `locals` whose center's sphere of influence their first
    /// node is in.
    fn new(field: &Field, locals: &[Field], path: &Path, positions: &[DVec3]) -> Layout {
        let n = path.times.len();
        assert!(n >= 2 && path.states.len() == n - 1 && path.burns.len() == n);
        assert!(path.burns[0] && path.burns[n - 1], "the ends are burns");
        let burn_index: Vec<usize> = (0..n).filter(|&k| path.burns[k]).collect();
        let mut time_of = Vec::with_capacity(n);
        for k in 0..n {
            let after = burn_index.partition_point(|&b| b <= k) - 1;
            if burn_index[after] == k {
                time_of.push(vec![(after, 1.0)]);
            } else {
                let (a, b) = (burn_index[after], burn_index[after + 1]);
                let f = (path.times[k] - path.times[a]) / (path.times[b] - path.times[a]);
                time_of.push(vec![(after, 1.0 - f), (after + 1, f)]);
            }
        }
        let (length, speed): (Vec<f64>, Vec<f64>) = (0..n)
            .map(|k| local_scales(field, path.epoch, path.times[k], positions[k]))
            .unzip();
        let time = burn_index.iter().map(|&k| length[k] / speed[k]).collect();
        assert!(
            path.passes
                .iter()
                .all(|p| p.node > 0 && p.node < n - 1 && path.burns[p.node]),
            "passes are at inner burn nodes"
        );
        Layout {
            time_of,
            burn_nodes: burn_index.len(),
            nodes: n,
            length,
            speed,
            time,
            // Released at first even when the seed is at the floor (as in
            // continuation, where the search then dives back toward the
            // surface before it is held again): held from the start, the
            // flyby is too rigid to linearize, and continuation's restores
            // failed (LEO to DRO, measured 2026-10-13).
            passes: path.passes.iter().map(|&p| (p, false)).collect(),
            frames: (0..n - 1)
                .map(|k| {
                    let when = Epoch(path.epoch.0 + path.times[k] / SECONDS_PER_DAY);
                    locals.iter().position(|l| {
                        let c = l.center().expect("local fields have centers");
                        let eph = field.ephemeris();
                        (positions[k] - field.body_position(c, when)).length()
                            < eph.sphere_of_influence(c, when)
                    })
                })
                .collect(),
        }
    }

    /// How natural each unknown is as a free one in the search (higher is
    /// freer): the burn nodes' times, then the velocities at inner burns,
    /// then at the ends; positions and coasts are left to follow. Searching
    /// over the stiff ones (a LEO departure's velocity) stalled the search
    /// in their noise.
    fn freedom(&self) -> Vec<u8> {
        let mut rank = vec![3; self.burn_nodes];
        for k in 0..self.nodes - 1 {
            let burn = self.time_of[k].len() == 1;
            for d in 0..6 {
                rank.push(match (burn, d < 3, k == 0) {
                    (true, false, false) => 2,
                    (true, false, true) => 1,
                    _ => 0,
                });
            }
        }
        rank
    }

    fn unknowns(&self) -> usize {
        self.burn_nodes + 6 * (self.nodes - 1)
    }

    fn state_var(&self, k: usize, d: usize) -> usize {
        self.burn_nodes + 6 * k + d
    }

    /// Scale of node `k`'s state component `d`.
    fn scale(&self, k: usize, d: usize) -> f64 {
        if d < 3 { self.length[k] } else { self.speed[k] }
    }

    /// Node `k`'s time scale.
    fn time_scale(&self, k: usize) -> f64 {
        self.length[k] / self.speed[k]
    }

    fn pack(&self, path: &Path) -> Vec<f64> {
        let mut z = vec![0.0; self.unknowns()];
        for k in 0..self.nodes {
            if let [(v, w)] = self.time_of[k][..]
                && w == 1.0
            {
                z[v] = path.times[k] / self.time[v];
            }
        }
        for (k, s) in path.states.iter().enumerate() {
            for d in 0..6 {
                z[self.state_var(k, d)] = s[d] / self.scale(k, d);
            }
        }
        z
    }

    fn times(&self, z: &[f64]) -> Vec<f64> {
        self.time_of
            .iter()
            .map(|deps| deps.iter().map(|&(v, w)| w * z[v] * self.time[v]).sum())
            .collect()
    }

    fn states(&self, z: &[f64]) -> Vec<[f64; 6]> {
        (0..self.nodes - 1)
            .map(|k| std::array::from_fn(|d| z[self.state_var(k, d)] * self.scale(k, d)))
            .collect()
    }
}

/// The distance at `r` (at `t`, s after `epoch`) from the body that pulls
/// hardest, and the circular speed there. In a body-centered field the Sun
/// is left out unless it is the center: its pull is nearly uniform there,
/// and the center's own fall toward it cancels it.
fn local_scales(field: &Field, epoch: Epoch, t: f64, r: DVec3) -> (f64, f64) {
    let eph = field.ephemeris();
    let when = Epoch(epoch.0 + t / SECONDS_PER_DAY);
    let first = field.bodies().next();
    let (gm, d) = field
        .bodies()
        .filter(|&b| b != eph.sun() || first == Some(b) || field.center().is_none())
        .map(|b| (eph.body(b).gm, (r - field.body_state(b, when).r).length()))
        .max_by(|a, b| (a.0 / (a.1 * a.1)).total_cmp(&(b.0 / (b.1 * b.1))))
        .expect("the field has bodies");
    (d, (gm / d).sqrt())
}

/// Constraint values and total Δv at the unknowns, with their derivatives
/// when asked.
struct Eval {
    c: Vec<f64>,
    /// Rows of ∂c/∂z.
    jac: Vec<Vec<f64>>,
    cost: f64,
    grad: Vec<f64>,
    /// Each node's state arriving (before any burn), from the second on.
    arrivals: Vec<[f64; 6]>,
}

/// One end's conditions on the ship's state there, and its burn.
struct EndTerms {
    /// (value, ∂/∂state, ∂/∂t), raw.
    rows: Vec<(f64, [f64; 6], f64)>,
    /// Row scales.
    scales: Vec<f64>,
    cost: f64,
    cost_state: [f64; 6],
    cost_t: f64,
    /// The burn, as the ship leaving (the start) or joining (the end) sees
    /// it: the velocity to add.
    burn: DVec3,
}

impl<'a> Leg<'a> {
    /// The state of `body` relative to the field's origin at `t` (s after
    /// `epoch`), with its acceleration.
    fn body(&self, body: BodyId, epoch: Epoch, t: f64) -> (DVec3, DVec3, DVec3) {
        let at = |s: f64| {
            self.field
                .body_state(body, Epoch(epoch.0 + s / SECONDS_PER_DAY))
        };
        let b = at(t);
        let h = 60.0;
        let a = (at(t + h).v - at(t - h).v) / (2.0 * h);
        (b.r, b.v, a)
    }

    /// The position and velocity of the origin of a frame centered on
    /// `center` (the solar-system barycenter if `None`) relative to the
    /// field's origin at `t`.
    fn origin_of(&self, center: Option<BodyId>, epoch: Epoch, t: f64) -> (DVec3, DVec3) {
        let when = Epoch(epoch.0 + t / SECONDS_PER_DAY);
        match (center, self.field.center()) {
            (c, f) if c == f => (DVec3::ZERO, DVec3::ZERO),
            (Some(c), _) => {
                let s = self.field.body_state(c, when);
                (s.r, s.v)
            }
            (None, Some(f)) => {
                let s = self.field.ephemeris().barycentric(f, when);
                (-s.r, -s.v)
            }
            (None, None) => unreachable!(),
        }
    }

    /// `end`'s conditions on the ship's state `s` at `t`, with rows scaled
    /// by `length` and `speed`; `None` outside a reference orbit's span.
    /// `leaving` says the ship leaves the end there.
    #[allow(clippy::too_many_arguments)]
    fn end_terms(
        &self,
        end: &End,
        epoch: Epoch,
        t: f64,
        s: &[f64; 6],
        leaving: bool,
        length: f64,
        speed: f64,
    ) -> Option<EndTerms> {
        let (r, v) = (DVec3::new(s[0], s[1], s[2]), DVec3::new(s[3], s[4], s[5]));
        let join = |dv: DVec3| if leaving { dv } else { -dv };
        match end {
            End::Orbit(reference) => {
                let tau = t + (epoch.0 - reference.epoch.0) * SECONDS_PER_DAY;
                let (lo, hi) = reference.path.span();
                if tau < lo || tau > hi {
                    return None;
                }
                // The reference's state in the leg's field's frame, and its
                // acceleration there.
                let (or, ov) = self.origin_of(reference.center, epoch, t);
                let x = reference.path.at(tau);
                let x = [
                    x[0] + or.x,
                    x[1] + or.y,
                    x[2] + or.z,
                    x[3] + ov.x,
                    x[4] + ov.y,
                    x[5] + ov.z,
                ];
                let a = self.field.ode(epoch)(t, &x);
                let (rr, vr, ar) = (
                    DVec3::new(x[0], x[1], x[2]),
                    DVec3::new(x[3], x[4], x[5]),
                    DVec3::new(a[3], a[4], a[5]),
                );
                let rows = (0..3)
                    .map(|i| {
                        let mut ds = [0.0; 6];
                        ds[i] = 1.0;
                        ((r - rr)[i], ds, -vr[i])
                    })
                    .collect();
                let dv = v - vr;
                let size = (dv.length_squared() + SMOOTH * SMOOTH).sqrt();
                let u = dv / size;
                Some(EndTerms {
                    rows,
                    scales: vec![length; 3],
                    cost: size,
                    cost_state: [0.0, 0.0, 0.0, u.x, u.y, u.z],
                    cost_t: -u.dot(ar),
                    burn: join(dv),
                })
            }
            End::Low(_) | End::Hyperbola { .. } => {
                // An apsis at a radius, and the speed a burn along the
                // velocity reaches there: a circular orbit's, or a
                // hyperbola's.
                let eph = self.field.ephemeris();
                let (body, radius, inclination, target) = match *end {
                    End::Low(low) => (
                        low.body,
                        low.radius,
                        low.inclination,
                        (eph.body(low.body).gm / low.radius).sqrt(),
                    ),
                    End::Hyperbola {
                        body,
                        radius,
                        v_inf,
                    } => (
                        body,
                        radius,
                        None,
                        (v_inf * v_inf + 2.0 * eph.body(body).gm / radius).sqrt(),
                    ),
                    _ => unreachable!(),
                };
                let (br, bv, ba) = self.body(body, epoch, t);
                let (rho, nu) = (r - br, v - bv);
                let dist = rho.length();
                let rho_hat = rho / dist;
                let cat = |a: DVec3, b: DVec3| [a.x, a.y, a.z, b.x, b.y, b.z];
                let mut rows = vec![
                    (dist - radius, cat(rho_hat, DVec3::ZERO), -rho_hat.dot(bv)),
                    (rho.dot(nu), cat(nu, rho), -bv.dot(nu) - rho.dot(ba)),
                ];
                let mut scales = vec![length, length * speed];
                if let Some(inc) = inclination {
                    let pole = eph
                        .pole(body, Epoch(epoch.0 + t / SECONDS_PER_DAY))
                        .expect("an inclination needs the body's pole");
                    let h = rho.cross(nu);
                    let h_hat = h / h.length();
                    let q = (pole - h_hat.dot(pole) * h_hat) / h.length();
                    let (d_rho, d_nu) = (nu.cross(q), q.cross(rho));
                    rows.push((
                        h_hat.dot(pole) - inc.cos(),
                        cat(d_rho, d_nu),
                        -d_rho.dot(bv) - d_nu.dot(ba),
                    ));
                    scales.push(1.0);
                }
                let moving = nu.length();
                let gap = moving - target;
                let size = (gap * gap + SMOOTH * SMOOTH).sqrt();
                let along = nu / moving * (gap / size);
                Some(EndTerms {
                    rows,
                    scales,
                    cost: size,
                    cost_state: cat(DVec3::ZERO, along),
                    cost_t: -along.dot(ba),
                    burn: join(nu / moving * gap),
                })
            }
            End::Body(body) => {
                let (br, bv, ba) = self.body(*body, epoch, t);
                let rows = (0..3)
                    .map(|i| {
                        let mut ds = [0.0; 6];
                        ds[i] = 1.0;
                        ((r - br)[i], ds, -bv[i])
                    })
                    .collect();
                let dv = v - bv;
                let size = (dv.length_squared() + SMOOTH * SMOOTH).sqrt();
                let u = dv / size;
                Some(EndTerms {
                    rows,
                    scales: vec![length; 3],
                    cost: size,
                    cost_state: [0.0, 0.0, 0.0, u.x, u.y, u.z],
                    cost_t: -u.dot(ba),
                    burn: join(dv),
                })
            }
            End::Atmosphere {
                body,
                radius,
                after,
                ..
            } => {
                assert!(!leaving, "an atmosphere ends a leg");
                let (br, bv, ba) = self.body(*body, epoch, t);
                let (rho, nu) = (r - br, v - bv);
                let dist = rho.length();
                let rho_hat = rho / dist;
                let cat = |a: DVec3, b: DVec3| [a.x, a.y, a.z, b.x, b.y, b.z];
                Some(EndTerms {
                    rows: vec![
                        (dist - radius, cat(rho_hat, DVec3::ZERO), -rho_hat.dot(bv)),
                        (rho.dot(nu), cat(nu, rho), -bv.dot(nu) - rho.dot(ba)),
                    ],
                    scales: vec![length, length * speed],
                    cost: *after,
                    cost_state: [0.0; 6],
                    cost_t: 0.0,
                    burn: *after * nu.normalize(),
                })
            }
            End::Excess { body, v_inf } => {
                // Derivatives by central differences: the asymptote's
                // algebra is long, and the function cheap and smooth.
                let gm = self.field.ephemeris().body(*body).gm;
                let at = |t: f64, s: &[f64; 6]| -> Option<DVec3> {
                    let b = self
                        .field
                        .body_state(*body, Epoch(epoch.0 + t / SECONDS_PER_DAY));
                    let rel: [f64; 6] =
                        std::array::from_fn(|d| s[d] - if d < 3 { b.r[d] } else { b.v[d - 3] });
                    Some(excess_velocity(gm, &rel, !leaving)? - *v_inf)
                };
                let value = at(t, s)?;
                let mut ds = [[0.0; 6]; 3];
                for d in 0..6 {
                    let h = if d < 3 { 1e-3 } else { 1e-6 };
                    let (mut hi, mut lo) = (*s, *s);
                    hi[d] += h;
                    lo[d] -= h;
                    let g = (at(t, &hi)? - at(t, &lo)?) / (2.0 * h);
                    for (i, row) in ds.iter_mut().enumerate() {
                        row[d] = g[i];
                    }
                }
                let dt = (at(t + 1.0, s)? - at(t - 1.0, s)?) / 2.0;
                let rows = (0..3).map(|i| (value[i], ds[i], dt[i])).collect();
                Some(EndTerms {
                    rows,
                    scales: vec![speed; 3],
                    cost: 0.0,
                    cost_state: [0.0; 6],
                    cost_t: 0.0,
                    burn: DVec3::ZERO,
                })
            }
        }
    }

    /// Arc `k`'s field, and the state of its origin relative to the leg's
    /// field's origin at `t` with that state's rate (zero for the leg's own
    /// field).
    fn frame(
        &self,
        layout: &Layout,
        epoch: Epoch,
        k: usize,
        t: f64,
    ) -> (&Field<'a>, [f64; 6], [f64; 6]) {
        match layout.frames[k] {
            None => (self.field, [0.0; 6], [0.0; 6]),
            Some(i) => {
                let local = &self.locals[i];
                let (r, v, a) = self.body(local.center().unwrap(), epoch, t);
                (
                    local,
                    [r.x, r.y, r.z, v.x, v.y, v.z],
                    [v.x, v.y, v.z, a.x, a.y, a.z],
                )
            }
        }
    }

    /// Arc `k` from `x` at `t0` to `t1`, integrated in its frame: the end
    /// (in the leg's field's frame) and, if asked, the state transition
    /// matrix (the same in either frame). `None` if it fails or passes
    /// below a body's surface.
    #[allow(clippy::too_many_arguments)]
    fn arc(
        &self,
        layout: &Layout,
        epoch: Epoch,
        k: usize,
        t0: f64,
        x: &[f64; 6],
        t1: f64,
        derivs: bool,
    ) -> Option<[f64; 42]> {
        let (field, b0, _) = self.frame(layout, epoch, k, t0);
        let (_, b1, _) = self.frame(layout, epoch, k, t1);
        let above = |t: f64, y: &[f64]| {
            field
                .inside(
                    Epoch(epoch.0 + t / SECONDS_PER_DAY),
                    DVec3::new(y[0], y[1], y[2]),
                )
                .is_none()
        };
        let mut y = [0.0; 42];
        for d in 0..6 {
            y[d] = x[d] - b0[d];
        }
        if derivs {
            let ode_stm = field.ode_stm(epoch);
            for d in 0..6 {
                y[6 + d * 7] = 1.0;
            }
            let mut run = Dop853::new(&ode_stm, t0, y, TOL);
            while run.t != t1 {
                run.step_to(&ode_stm, t1).ok()?;
                if !above(run.t, &run.y) {
                    return None;
                }
            }
            y = run.y;
        } else {
            let ode = field.ode(epoch);
            let start: [f64; 6] = y[..6].try_into().unwrap();
            let mut run = Dop853::new(&ode, t0, start, TOL);
            while run.t != t1 {
                run.step_to(&ode, t1).ok()?;
                if !above(run.t, &run.y) {
                    return None;
                }
            }
            y[..6].copy_from_slice(&run.y);
        }
        for d in 0..6 {
            y[d] += b1[d];
        }
        Some(y)
    }

    /// The rate of a state `s` (in the leg's field's frame) at `t` under arc
    /// `k`'s field: its rate in the arc's frame plus the frame's.
    fn rate(&self, layout: &Layout, epoch: Epoch, k: usize, t: f64, s: &[f64; 6]) -> [f64; 6] {
        let (field, b, db) = self.frame(layout, epoch, k, t);
        let local: [f64; 6] = std::array::from_fn(|d| s[d] - b[d]);
        let f = field.ode(epoch)(t, &local);
        std::array::from_fn(|d| f[d] + db[d])
    }

    /// The constraints and cost at `z`; `None` where the arcs cannot be
    /// integrated, the nodes are out of order, or an end leaves its
    /// reference orbit's span. A departure outside the window adds a
    /// penalty to the cost, [`WINDOW_PENALTY`] per day squared.
    fn evaluate(&self, layout: &Layout, epoch: Epoch, z: &[f64], derivs: bool) -> Option<Eval> {
        let times = layout.times(z);
        if times.windows(2).any(|w| w[1] <= w[0]) {
            return None;
        }
        // A wild step can carry times off the ephemeris's tables (seen in a
        // line search between station orbits: 24 years back).
        let (first, last) = self.field.ephemeris().span();
        let at = |t: f64| epoch.0 + t / SECONDS_PER_DAY;
        if at(times[0]) < first.0 || at(times[layout.nodes - 1]) > last.0 {
            return None;
        }

        let states = layout.states(z);
        let arcs = layout.nodes - 1;
        // Each arc's end and, with derivatives, its state transition matrix;
        // none if an arc passes below a body's surface (with point masses,
        // the field would sling it through).
        let mut ends: Vec<[f64; 42]> = Vec::with_capacity(arcs);
        for k in 0..arcs {
            ends.push(self.arc(layout, epoch, k, times[k], &states[k], times[k + 1], derivs)?);
        }
        let arrival = |k: usize| -> [f64; 6] { ends[k - 1][..6].try_into().unwrap() };
        let n = layout.unknowns();
        let mut c = Vec::new();
        let mut jac: Vec<Vec<f64>> = Vec::new();
        let mut cost = 0.0;
        let mut grad = vec![0.0; n];

        // Raw partials into a scaled row: of node k's leaving state, its
        // time, and its arriving state (through the arc that reaches it).
        let d_state = |row: &mut [f64], k: usize, g: &[f64; 6]| {
            for d in 0..6 {
                row[layout.state_var(k, d)] += g[d] * layout.scale(k, d);
            }
        };
        let d_time = |row: &mut [f64], k: usize, g: f64| {
            for &(v, w) in &layout.time_of[k] {
                row[v] += g * w * layout.time[v];
            }
        };
        let d_arrival = |row: &mut [f64], k: usize, g: &[f64; 6]| {
            let j = k - 1;
            let phi = |r: usize, c: usize| ends[j][6 + r * 6 + c];
            let through: [f64; 6] = std::array::from_fn(|c| (0..6).map(|r| g[r] * phi(r, c)).sum());
            d_state(row, j, &through);
            let f_end = self.rate(layout, epoch, j, times[k], &arrival(k));
            d_time(row, k, (0..6).map(|r| g[r] * f_end[r]).sum());
            let f_start = self.rate(layout, epoch, j, times[j], &states[j]);
            d_time(
                row,
                j,
                -(0..6).map(|c| through[c] * f_start[c]).sum::<f64>(),
            );
        };

        // The ends.
        let last = layout.nodes - 1;
        for (node, end, leaving) in [(0, &self.from, true), (last, &self.to, false)] {
            let s = if leaving { states[0] } else { arrival(last) };
            let terms = self.end_terms(
                end,
                epoch,
                times[node],
                &s,
                leaving,
                layout.length[node],
                layout.speed[node],
            )?;
            for ((value, ds, dt), sc) in terms.rows.iter().zip(&terms.scales) {
                c.push(value / sc);
                if derivs {
                    let mut row = vec![0.0; n];
                    let ds = ds.map(|x| x / sc);
                    if leaving {
                        d_state(&mut row, node, &ds);
                    } else {
                        d_arrival(&mut row, node, &ds);
                    }
                    d_time(&mut row, node, dt / sc);
                    jac.push(row);
                }
            }
            cost += terms.cost;
            if derivs {
                if leaving {
                    d_state(&mut grad, node, &terms.cost_state);
                } else {
                    d_arrival(&mut grad, node, &terms.cost_state);
                }
                d_time(&mut grad, node, terms.cost_t);
            }
        }

        // Continuity at the inner nodes, and their burns.
        for (k, &x) in states.iter().enumerate().skip(1) {
            let y = arrival(k);
            let dims = if layout.time_of[k].len() == 1 { 3 } else { 6 };
            for d in 0..dims {
                c.push((x[d] - y[d]) / layout.scale(k, d));
                if derivs {
                    let mut row = vec![0.0; n];
                    let mut e = [0.0; 6];
                    e[d] = 1.0 / layout.scale(k, d);
                    d_state(&mut row, k, &e);
                    e[d] = -1.0 / layout.scale(k, d);
                    d_arrival(&mut row, k, &e);
                    jac.push(row);
                }
            }
            if dims == 3 {
                let dv = DVec3::new(x[3] - y[3], x[4] - y[4], x[5] - y[5]);
                let size = (dv.length_squared() + SMOOTH * SMOOTH).sqrt();
                cost += size;
                if derivs {
                    let u = dv / size;
                    d_state(&mut grad, k, &[0.0, 0.0, 0.0, u.x, u.y, u.z]);
                    d_arrival(&mut grad, k, &[0.0, 0.0, 0.0, -u.x, -u.y, -u.z]);
                }
            }
        }

        // Close passes: the burn at the periapsis of the arc arriving; held,
        // at the floor and the periapsis of the arc leaving too, so that a
        // burn with an inward part cannot take that arc lower (measured: up
        // to 0.17 km below the floor). Asking the latter of every pass made
        // all flyby burns tangential, which lost the cheapest DRO and L2
        // halo flybys; asking it only once a held burn pointed inward, the
        // restore to it failed (LEO to DRO, 2026-10-13).
        for &(pass, held) in &layout.passes {
            let k = pass.node;
            let (br, bv, ba) = self.body(pass.body, epoch, times[k]);
            let (x, y) = (states[k], arrival(k));
            let rho = DVec3::new(x[0], x[1], x[2]) - br;
            let (l, v) = (layout.length[k], layout.speed[k]);
            let sides: &[bool] = if held { &[false, true] } else { &[false] };
            for &leaving in sides {
                let s = if leaving { x } else { y };
                let nu = DVec3::new(s[3], s[4], s[5]) - bv;
                c.push(rho.dot(nu) / (l * v));
                if derivs {
                    let mut row = vec![0.0; n];
                    let (a, b) = (nu / (l * v), rho / (l * v));
                    if leaving {
                        d_state(&mut row, k, &[a.x, a.y, a.z, b.x, b.y, b.z]);
                    } else {
                        d_state(&mut row, k, &[a.x, a.y, a.z, 0.0, 0.0, 0.0]);
                        d_arrival(&mut row, k, &[0.0, 0.0, 0.0, b.x, b.y, b.z]);
                    }
                    d_time(&mut row, k, (-bv.dot(nu) - rho.dot(ba)) / (l * v));
                    jac.push(row);
                }
            }
            if held {
                let u = rho / rho.length();
                c.push((rho.length() - pass.min_radius) / l);
                if derivs {
                    let mut row = vec![0.0; n];
                    d_state(&mut row, k, &[u.x / l, u.y / l, u.z / l, 0.0, 0.0, 0.0]);
                    d_time(&mut row, k, -u.dot(bv) / l);
                    jac.push(row);
                }
            }
        }

        // Leaving outside the window.
        if let (None, Some((lo, hi))) = (self.depart, self.window) {
            let outside = (lo - times[0]).max(0.0) - (times[0] - hi).max(0.0);
            let days = outside / SECONDS_PER_DAY;
            cost += WINDOW_PENALTY * days * days;
            if derivs {
                d_time(&mut grad, 0, -2.0 * WINDOW_PENALTY * days / SECONDS_PER_DAY);
            }
        }

        // Fixed times.
        let mut fixed = |value: f64, terms: &[(usize, f64)]| {
            let sc = terms
                .iter()
                .map(|&(k, _)| layout.time_scale(k))
                .fold(0.0, f64::max);
            c.push(value / sc);
            if derivs {
                let mut row = vec![0.0; n];
                for &(k, w) in terms {
                    d_time(&mut row, k, w / sc);
                }
                jac.push(row);
            }
        };
        if let Some(t) = self.depart {
            fixed(times[0] - t, &[(0, 1.0)]);
        }
        if let Some(t) = self.arrive {
            fixed(times[last] - t, &[(last, 1.0)]);
        }
        if let Some(d) = self.duration {
            fixed(times[last] - times[0] - d, &[(last, 1.0), (0, -1.0)]);
        }
        Some(Eval {
            c,
            jac,
            cost,
            grad,
            arrivals: (1..=last).map(arrival).collect(),
        })
    }

    /// Designs the leg from a seed path: a continuous path near it, then
    /// the least total Δv nearby.
    pub fn design(&self, seed: &Path) -> Result<Design, Failure> {
        let epoch = seed.epoch;
        let last = seed.times.len() - 1;
        let ode = self.field.ode(epoch);
        let mut run = Dop853::new(&ode, seed.times[last - 1], seed.states[last - 1], TOL);
        run.advance_to(&ode, seed.times[last])
            .map_err(|_| Failure::Infeasible(f64::INFINITY))?;
        let positions: Vec<DVec3> = seed
            .states
            .iter()
            .chain([&run.y])
            .map(|s| DVec3::new(s[0], s[1], s[2]))
            .collect();
        let mut layout = Layout::new(self.field, self.locals, seed, &positions);
        // An active set: passes below their floors are held there, and a
        // departure the penalty did not keep inside the window at its nearer
        // edge (a hard edge alone let a search drift days away, too far to
        // pull back); then the search runs again. Neither is released.
        let mut leg = *self;
        if let (None, Some((lo, hi))) = (leg.depart, leg.window) {
            let t0 = seed.times[0];
            if (t0 - lo).abs() < HELD_S || (t0 - hi).abs() < HELD_S {
                leg.depart = Some(t0.clamp(lo, hi));
            }
        }
        let mut z = leg.restore(&layout, epoch, layout.pack(seed))?;
        let mut iterations = 0;
        for _ in 0..3 {
            let (searched, more) = leg.minimize(&layout, epoch, z)?;
            (z, iterations) = (searched, iterations + more);
            let times = layout.times(&z);
            let states = layout.states(&z);
            let mut changed = false;
            for (pass, held) in &mut layout.passes {
                let (br, _, _) = leg.body(pass.body, epoch, times[pass.node]);
                let r = states[pass.node];
                if !*held && (DVec3::new(r[0], r[1], r[2]) - br).length() < pass.min_radius {
                    *held = true;
                    changed = true;
                }
            }
            if let (None, Some((lo, hi))) = (leg.depart, leg.window)
                && (times[0] < lo || times[0] > hi)
            {
                leg.depart = Some(times[0].clamp(lo, hi));
                changed = true;
            }
            if !changed {
                break;
            }
            z = leg.restore(&layout, epoch, z)?;
        }
        let z = leg.restore(&layout, epoch, z)?;
        let e = leg
            .evaluate(&layout, epoch, &z, false)
            .ok_or(Failure::Infeasible(f64::INFINITY))?;
        leg.finish(&layout, epoch, &z, &e, iterations)
    }

    /// Designs at each of `durations` (s), by continuation from `from` (a
    /// design of this leg with any duration): the duration is moved toward
    /// each target in steps of at most `step` s, each designed from the one
    /// before, which a seed far from the target duration cannot do. A step
    /// that fails is halved, down to 1/16 of `step` (a lunar flyby return
    /// needed steps under 0.25 days near 5 days). `None` for targets the
    /// continuation could not reach.
    pub fn frontier(&self, from: &Design, durations: &[f64], step: f64) -> Vec<Option<Design>> {
        let mut leg = *self;
        let start = from.duration();
        // Outward from the starting duration on each side, so each target
        // continues from the nearest one before it.
        let mut order: Vec<usize> = (0..durations.len()).collect();
        order.sort_by(|&a, &b| {
            (durations[a] - start)
                .abs()
                .total_cmp(&(durations[b] - start).abs())
        });
        let mut out: Vec<Option<Design>> = vec![None; durations.len()];
        let (mut above, mut below) = (Some(from.clone()), Some(from.clone()));
        for i in order {
            let target = durations[i];
            let side = if target >= start {
                &mut above
            } else {
                &mut below
            };
            let Some(mut current) = side.clone() else {
                continue;
            };
            let mut reached = false;
            let mut size = step;
            for _ in 0..CONTINUATION_STEPS {
                let gap = target - current.duration();
                if gap.abs() <= 1e-6 {
                    reached = true;
                    break;
                }
                leg.duration = Some(current.duration() + gap.clamp(-size, size));
                match leg.design(&current.path) {
                    Ok(d) => {
                        current = d;
                        size = (2.0 * size).min(step);
                    }
                    // Halved from what was tried, so the retry differs.
                    Err(_) if size > step / 16.0 => size = size.min(gap.abs()) / 2.0,
                    Err(_) => break,
                }
            }
            if reached {
                out[i] = Some(current.clone());
                *side = Some(current);
            } else {
                *side = None;
            }
        }
        out
    }

    /// Minimum-norm Newton steps from `z` to a continuous path that meets
    /// its ends, each halved while it makes the residuals worse.
    fn restore(&self, layout: &Layout, epoch: Epoch, mut z: Vec<f64>) -> Result<Vec<f64>, Failure> {
        let squares = |c: &[f64]| c.iter().map(|x| x * x).sum::<f64>();
        let mut residual = f64::INFINITY;
        for _ in 0..FEASIBILITY_ITERATIONS {
            let e = self
                .evaluate(layout, epoch, &z, true)
                .ok_or(Failure::Infeasible(residual))?;
            residual = worst(&e.c);
            if residual < FEASIBLE {
                return Ok(z);
            }
            let flat: Vec<f64> = e.jac.concat();
            let minus: Vec<f64> = e.c.iter().map(|x| -x).collect();
            let step = linalg::min_norm(e.c.len(), z.len(), &flat, &minus)
                .ok_or(Failure::Infeasible(residual))?;
            let mut size = 1.0;
            loop {
                let trial: Vec<f64> = z.iter().zip(&step).map(|(a, b)| a + size * b).collect();
                if let Some(t) = self.evaluate(layout, epoch, &trial, false)
                    && squares(&t.c) < squares(&e.c)
                {
                    z = trial;
                    break;
                }
                size /= 2.0;
                if size < 1.0 / 64.0 {
                    return Err(Failure::Infeasible(residual));
                }
            }
        }
        Err(Failure::Infeasible(residual))
    }

    /// Generalized reduced gradient from a continuous path: the constraints
    /// fix a basic set of unknowns given the rest, so the search runs over
    /// the few free ones (the departure and arrival times of a two-burn
    /// transfer), with a BFGS approximation of the Hessian there and a
    /// backtracking line search on the total Δv. Each trial point is made
    /// continuous again by Newton's method on the basic unknowns, so every
    /// iterate is a path the ship could fly. Returns the unknowns and the
    /// iterations taken.
    fn minimize(
        &self,
        layout: &Layout,
        epoch: Epoch,
        mut z: Vec<f64>,
    ) -> Result<(Vec<f64>, usize), Failure> {
        let mut e = self
            .evaluate(layout, epoch, &z, true)
            .ok_or(Failure::Infeasible(f64::INFINITY))?;
        let (n, m) = (z.len(), e.c.len());
        if m >= n {
            return Ok((z, 0));
        }
        let basic = linalg::independent_columns(m, n, &e.jac.concat(), &layout.freedom())
            .ok_or(Failure::Infeasible(worst(&e.c)))?;
        let free: Vec<usize> = (0..n).filter(|j| !basic.contains(j)).collect();
        let r = free.len();
        let mut reduced = Reduced::at(&e, &basic, &free).ok_or(Failure::Infeasible(worst(&e.c)))?;
        let (mut hess, mut first) = match self.reduced_hessian(layout, epoch, &z, &reduced) {
            Some(h) => (h, false),
            None => (identity(r), true),
        };
        let mut iterations = 0;
        let mut refresh = false;
        while iterations < ITERATIONS {
            iterations += 1;
            if refresh && let Some(h) = self.reduced_hessian(layout, epoch, &z, &reduced) {
                hess = h;
            }
            let mut pz: Vec<f64> = reduced.gradient.iter().map(|g| -g).collect();
            linalg::solve(r, &mut hess.clone(), &mut pz).ok_or(Failure::Infeasible(worst(&e.c)))?;
            let slope = dot(&reduced.gradient, &pz);
            if -slope < 2.0 * CONVERGED_KMS {
                break;
            }
            let p = reduced.step(&pz);
            let mut accepted = None;
            let mut size = 1.0;
            while size >= 1.0 / 1024.0 {
                let trial: Vec<f64> = z.iter().zip(&p).map(|(a, b)| a + size * b).collect();
                if let Some((z_new, e_new)) = self.settle(layout, epoch, trial, &reduced)
                    && e_new.cost <= e.cost + 1e-4 * size * slope
                {
                    accepted = Some((z_new, e_new));
                    break;
                }
                size /= 2.0;
            }
            let Some((z_new, e_new)) = accepted else {
                break;
            };
            let Some(reduced_new) = Reduced::at(&e_new, &basic, &free) else {
                break;
            };
            let s: Vec<f64> = free.iter().map(|&j| z_new[j] - z[j]).collect();
            let mut y: Vec<f64> = reduced_new
                .gradient
                .iter()
                .zip(&reduced.gradient)
                .map(|(a, b)| a - b)
                .collect();
            if first && dot(&s, &y) > 0.0 {
                let scale = dot(&y, &y) / dot(&s, &y);
                hess.iter_mut().for_each(|h| *h *= scale);
                first = false;
            }
            bfgs(&mut hess, &s, &mut y);
            let moved = s.iter().fold(0.0f64, |a, x| a.max(x.abs()));
            (z, e, reduced) = (z_new, e_new, reduced_new);
            // A step the model got badly wrong: its curvature has changed
            // since the Hessian was measured.
            refresh = size < 0.25;
            if moved < 1e-10 {
                break;
            }
        }
        Ok((z, iterations))
    }

    /// The Hessian of the total Δv over the free unknowns, along the
    /// constraints, by differences of the reduced gradient, made positive
    /// definite: for BFGS to start from, and to start over from when a step
    /// shows the curvature has moved on (on a lunar flyby transfer it grows
    /// ~700× along the flyby burn from the seed to the optimum). `None` if
    /// a difference fails.
    fn reduced_hessian(
        &self,
        layout: &Layout,
        epoch: Epoch,
        z: &[f64],
        reduced: &Reduced,
    ) -> Option<Vec<f64>> {
        let r = reduced.free.len();
        let h = 1e-4;
        let mut hess = vec![0.0; r * r];
        for f in 0..r {
            let mut pz = vec![0.0; r];
            pz[f] = h;
            let p = reduced.step(&pz);
            let trial: Vec<f64> = z.iter().zip(&p).map(|(a, b)| a + b).collect();
            let (_, e) = self.settle(layout, epoch, trial, reduced)?;
            let moved = Reduced::at(&e, &reduced.basic, &reduced.free)?;
            for g in 0..r {
                hess[g * r + f] = (moved.gradient[g] - reduced.gradient[g]) / h;
            }
        }
        for f in 0..r {
            for g in 0..f {
                let mean = 0.5 * (hess[f * r + g] + hess[g * r + f]);
                hess[f * r + g] = mean;
                hess[g * r + f] = mean;
            }
        }
        // Shifted toward the identity (scaled to the diagonal) until it
        // factors as positive definite.
        let scale = (0..r).map(|i| hess[i * r + i].abs()).sum::<f64>() / r as f64;
        let mut shift = 0.0;
        while !positive_definite(r, &hess) {
            let add = if shift == 0.0 {
                1e-6 * scale.max(1e-12)
            } else {
                shift
            };
            for i in 0..r {
                hess[i * r + i] += add;
            }
            shift = 2.0 * (shift + add);
            if shift > 1e6 * scale.max(1e-12) {
                return None;
            }
        }
        Some(hess)
    }

    /// `z` made continuous by Newton's method on the basic unknowns alone:
    /// chord steps with `reduced`'s Jacobian (integrations without the
    /// state transition matrix), then full steps if those stall. With
    /// derivatives at the result; `None` if it does not converge.
    fn settle(
        &self,
        layout: &Layout,
        epoch: Epoch,
        mut z: Vec<f64>,
        reduced: &Reduced,
    ) -> Option<(Vec<f64>, Eval)> {
        let mut e = self.evaluate(layout, epoch, &z, false)?;
        let mut chord = Some(reduced);
        let mut fresh;
        for _ in 0..12 {
            if worst(&e.c) < FEASIBLE {
                let e = self.evaluate(layout, epoch, &z, true)?;
                return Some((z, e));
            }
            let jac = match chord {
                Some(j) => j,
                None => {
                    let d = self.evaluate(layout, epoch, &z, true)?;
                    fresh = Reduced::at(&d, &reduced.basic, &reduced.free)?;
                    &fresh
                }
            };
            let step = jac.correction(&e.c)?;
            let trial: Vec<f64> = z.iter().zip(&step).map(|(a, b)| a + b).collect();
            let t = self.evaluate(layout, epoch, &trial, false)?;
            if worst(&t.c) < 0.5 * worst(&e.c) {
                (z, e) = (trial, t);
            } else if chord.is_some() {
                chord = None;
            } else {
                return None;
            }
        }
        None
    }

    /// The design at converged unknowns: its burns, and a check that no arc
    /// passes below a body's surface.
    fn finish(
        &self,
        layout: &Layout,
        epoch: Epoch,
        z: &[f64],
        e: &Eval,
        iterations: usize,
    ) -> Result<Design, Failure> {
        let times = layout.times(z);
        let states = layout.states(z);
        let last = layout.nodes - 1;
        for k in 0..last {
            let (field, b, _) = self.frame(layout, epoch, k, times[k]);
            let ode = field.ode(epoch);
            let start: [f64; 6] = std::array::from_fn(|d| states[k][d] - b[d]);
            let mut run = Dop853::new(&ode, times[k], start, TOL);
            let mut steps = Vec::new();
            run.advance_dense(&ode, times[k + 1], &mut steps)
                .map_err(|_| Failure::Infeasible(0.0))?;
            for step in &steps {
                for i in 0..8 {
                    let t = step.t0 + step.h * i as f64 / 8.0;
                    let s = step.at(t);
                    let when = Epoch(epoch.0 + t / SECONDS_PER_DAY);
                    if let Some(b) = field.inside(when, DVec3::new(s[0], s[1], s[2])) {
                        return Err(Failure::Collision(b));
                    }
                }
            }
        }
        let mut burns = Vec::new();
        let start = self
            .end_terms(
                &self.from,
                epoch,
                times[0],
                &states[0],
                true,
                layout.length[0],
                layout.speed[0],
            )
            .unwrap();
        burns.push(Burn {
            t: times[0],
            dv: start.burn,
        });
        for k in 1..last {
            if layout.time_of[k].len() == 1 {
                let (x, y) = (states[k], e.arrivals[k - 1]);
                burns.push(Burn {
                    t: times[k],
                    dv: DVec3::new(x[3] - y[3], x[4] - y[4], x[5] - y[5]),
                });
            }
        }
        let arrival = e.arrivals[last - 1];
        if let End::Atmosphere {
            body, max_speed, ..
        } = self.to
        {
            let b = self
                .field
                .body_state(body, Epoch(epoch.0 + times[last] / SECONDS_PER_DAY));
            let speed = (DVec3::new(arrival[3], arrival[4], arrival[5]) - b.v).length();
            if speed > max_speed {
                return Err(Failure::TooFast(speed));
            }
        }
        let end = self
            .end_terms(
                &self.to,
                epoch,
                times[last],
                &arrival,
                false,
                layout.length[last],
                layout.speed[last],
            )
            .unwrap();
        burns.push(Burn {
            t: times[last],
            dv: end.burn,
        });
        Ok(Design {
            dv: burns.iter().map(|b| b.dv.length()).sum(),
            path: Path {
                epoch,
                burns: (0..layout.nodes)
                    .map(|k| layout.time_of[k].len() == 1)
                    .collect(),
                times,
                states,
                passes: layout.passes.iter().map(|&(p, _)| p).collect(),
            },
            burns,
            iterations,
        })
    }
}

/// Whether the symmetric n×n `a` is positive definite: its Cholesky
/// factorization succeeds.
fn positive_definite(n: usize, a: &[f64]) -> bool {
    let mut l = vec![0.0; n * n];
    for c in 0..n {
        let d = a[c * n + c] - (0..c).map(|k| l[c * n + k] * l[c * n + k]).sum::<f64>();
        if d <= 0.0 {
            return false;
        }
        let d = d.sqrt();
        l[c * n + c] = d;
        for r in c + 1..n {
            l[r * n + c] =
                (a[r * n + c] - (0..c).map(|k| l[r * n + k] * l[c * n + k]).sum::<f64>()) / d;
        }
    }
    true
}

fn identity(n: usize) -> Vec<f64> {
    let mut m = vec![0.0; n * n];
    for i in 0..n {
        m[i * n + i] = 1.0;
    }
    m
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The constraint Jacobian split into basic and free unknowns, J = [B N]
/// (columns permuted): basic unknowns follow from the free ones, so steps
/// are `p_basic = -B⁻¹ (c + N p_free)`.
struct Reduced {
    basic_matrix: Vec<f64>,
    basic: Vec<usize>,
    free: Vec<usize>,
    /// B⁻¹ N (m × free) and B⁻¹ c.
    x: Vec<f64>,
    y: Vec<f64>,
    /// The gradient with respect to the free unknowns along the
    /// constraints.
    gradient: Vec<f64>,
}

impl Reduced {
    fn at(e: &Eval, basic: &[usize], free: &[usize]) -> Option<Reduced> {
        let (m, r) = (basic.len(), free.len());
        let basic_matrix: Vec<f64> = (0..m)
            .flat_map(|i| basic.iter().map(move |&j| e.jac[i][j]))
            .collect();
        let mut rhs: Vec<f64> = (0..m)
            .flat_map(|i| free.iter().map(move |&j| e.jac[i][j]).chain([e.c[i]]))
            .collect();
        linalg::solve_many(m, r + 1, &mut basic_matrix.clone(), &mut rhs)?;
        let x: Vec<f64> = (0..m)
            .flat_map(|i| rhs[i * (r + 1)..i * (r + 1) + r].to_vec())
            .collect();
        let y: Vec<f64> = (0..m).map(|i| rhs[i * (r + 1) + r]).collect();
        let gradient = (0..r)
            .map(|f| e.grad[free[f]] - (0..m).map(|i| x[i * r + f] * e.grad[basic[i]]).sum::<f64>())
            .collect();
        Some(Reduced {
            basic_matrix,
            basic: basic.to_vec(),
            free: free.to_vec(),
            x,
            y,
            gradient,
        })
    }

    /// The full step for a step `pz` in the free unknowns.
    fn step(&self, pz: &[f64]) -> Vec<f64> {
        let (m, r) = (self.basic.len(), self.free.len());
        let mut p = vec![0.0; m + r];
        for (f, &j) in self.free.iter().enumerate() {
            p[j] = pz[f];
        }
        for (i, &j) in self.basic.iter().enumerate() {
            p[j] = -self.y[i] - (0..r).map(|f| self.x[i * r + f] * pz[f]).sum::<f64>();
        }
        p
    }

    /// A step in the basic unknowns that cancels residuals `c` to first
    /// order.
    fn correction(&self, c: &[f64]) -> Option<Vec<f64>> {
        let m = self.basic.len();
        let mut b: Vec<f64> = c.iter().map(|x| -x).collect();
        linalg::solve(m, &mut self.basic_matrix.clone(), &mut b)?;
        let mut p = vec![0.0; m + self.free.len()];
        for (i, &j) in self.basic.iter().enumerate() {
            p[j] = b[i];
        }
        Some(p)
    }
}

fn worst(c: &[f64]) -> f64 {
    c.iter().fold(0.0f64, |m, x| m.max(x.abs()))
}

/// Powell's damped BFGS update of `hess` (n×n) for step `s` and gradient
/// change `y`, keeping it positive definite.
fn bfgs(hess: &mut [f64], s: &[f64], y: &mut [f64]) {
    let n = s.len();
    let bs: Vec<f64> = (0..n).map(|i| dot(&hess[i * n..(i + 1) * n], s)).collect();
    let sbs = dot(s, &bs);
    let sy = dot(s, y);
    if sbs <= 0.0 {
        return;
    }
    if sy < 0.2 * sbs {
        let theta = 0.8 * sbs / (sbs - sy);
        for i in 0..n {
            y[i] = theta * y[i] + (1.0 - theta) * bs[i];
        }
    }
    let sy = dot(s, y);
    for i in 0..n {
        for j in 0..n {
            hess[i * n + j] += y[i] * y[j] / sy - bs[i] * bs[j] / sbs;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ephem::Ephemeris;

    /// About Earth alone, the cheapest transfer between circular orbits is
    /// Hohmann's, half an ellipse touching both; from a seed that is
    /// neither continuous at its arrival apsis nor the right shape.
    #[test]
    fn finds_hohmann_transfer() {
        let eph = Ephemeris::standard();
        let earth = eph.lookup("earth").unwrap();
        let field = Field::new(eph, Some(earth), &[earth]);
        let mu = eph.body(earth).gm;
        let (r1, r2) = (6_778.0, 42_164.0);
        let low = |radius| {
            End::Low(LowOrbit {
                body: earth,
                radius,
                inclination: None,
            })
        };
        let leg = Leg {
            field: &field,
            locals: &[],
            from: low(r1),
            to: low(r2),
            depart: Some(0.0),
            arrive: None,
            duration: None,
            window: None,
        };
        // Tangential, 300 m/s short, flown for 4 h in three arcs.
        let v = 10.0;
        let ode = field.ode(Epoch(16_000.0));
        let mut run = Dop853::new(&ode, 0.0, [r1, 0.0, 0.0, 0.0, v, 0.0], TOL);
        let mut times = vec![0.0];
        let mut states = vec![run.y];
        for k in 1..=3 {
            let t = 4.0 * 3_600.0 * k as f64 / 3.0;
            run.advance_to(&ode, t).unwrap();
            times.push(t);
            if k < 3 {
                states.push(run.y);
            }
        }
        let seed = Path::coasting(Epoch(16_000.0), times, states);
        let design = leg.design(&seed).unwrap();
        let a = (r1 + r2) / 2.0;
        let dv1 = (mu * (2.0 / r1 - 1.0 / a)).sqrt() - (mu / r1).sqrt();
        let dv2 = (mu / r2).sqrt() - (mu * (2.0 / r2 - 1.0 / a)).sqrt();
        let tof = std::f64::consts::PI * (a.powi(3) / mu).sqrt();
        println!(
            "{:.6} km/s in {:.1} s after {} iterations (Hohmann {:.6} in {tof:.1})",
            design.dv,
            design.duration(),
            design.iterations,
            dv1 + dv2
        );
        assert!((design.dv - (dv1 + dv2)).abs() < 1e-6, "{}", design.dv);
        assert!(
            (design.duration() - tof).abs() < 1.0,
            "{}",
            design.duration()
        );
    }

    /// Between two circular orbits about Earth alone, with free departure
    /// and arrival: Hohmann's transfer at the time the target's phase
    /// allows, found from a seed 500 s early on a slow ellipse that misses
    /// the target.
    #[test]
    fn finds_hohmann_rendezvous() {
        let eph = Ephemeris::standard();
        let earth = eph.lookup("earth").unwrap();
        let field = Field::new(eph, Some(earth), &[earth]);
        let mu = eph.body(earth).gm;
        let epoch = Epoch(16_000.0);
        let ode = field.ode(epoch);
        let (r1, r2): (f64, f64) = (7_000.0, 10_000.0);
        let (n1, n2) = ((mu / r1.powi(3)).sqrt(), (mu / r2.powi(3)).sqrt());
        let a = (r1 + r2) / 2.0;
        let tof = std::f64::consts::PI * (a.powi(3) / mu).sqrt();
        // The target's phase for a Hohmann departure at 3,000 s.
        let best = 3_000.0;
        let phase = std::f64::consts::PI + (n1 - n2) * best - n2 * tof;
        let circle = |r: f64, phase: f64| -> Reference {
            let v = (mu / r).sqrt();
            let (c, s) = (phase.cos(), phase.sin());
            let mut run = Dop853::new(&ode, 0.0, [r * c, r * s, 0.0, -v * s, v * c, 0.0], TOL);
            let mut steps = Vec::new();
            run.advance_dense(&ode, 30_000.0, &mut steps).unwrap();
            Reference {
                epoch,
                center: Some(earth),
                revolutions: 1,
                patches: Vec::new(),
                path: crate::integrate::Trajectory::new(steps),
            }
        };
        let (inner, outer) = (circle(r1, 0.0), circle(r2, phase));
        let leg = Leg {
            field: &field,
            locals: &[],
            from: End::Orbit(&inner),
            to: End::Orbit(&outer),
            depart: None,
            arrive: None,
            duration: None,
            window: None,
        };
        let t0 = best - 500.0;
        let mut start = inner.path.at(t0);
        let speed = (start[3] * start[3] + start[4] * start[4]).sqrt();
        for v in &mut start[3..5] {
            *v *= (speed + 0.5) / speed;
        }
        let mut run = Dop853::new(&ode, t0, start, TOL);
        let mut times = vec![t0];
        let mut states = vec![start];
        for k in 1..=3 {
            let t = t0 + 3_500.0 * k as f64 / 3.0;
            run.advance_to(&ode, t).unwrap();
            times.push(t);
            if k < 3 {
                states.push(run.y);
            }
        }
        let design = leg.design(&Path::coasting(epoch, times, states)).unwrap();
        let dv1 = (mu * (2.0 / r1 - 1.0 / a)).sqrt() - (mu / r1).sqrt();
        let dv2 = (mu / r2).sqrt() - (mu * (2.0 / r2 - 1.0 / a)).sqrt();
        println!(
            "{:.6} km/s leaving at {:.1} s, {:.1} s long, after {} iterations (Hohmann {:.6} at {best}, {tof:.1} s)",
            design.dv,
            design.departure(),
            design.duration(),
            design.iterations,
            dv1 + dv2
        );
        assert!((design.dv - (dv1 + dv2)).abs() < 1e-5, "{}", design.dv);
        assert!((design.departure() - best).abs() < 10.0);
        assert!((design.duration() - tof).abs() < 10.0);
    }

    /// The analytic constraint Jacobian and cost gradient match central
    /// differences: a low orbit at an inclination to an orbit, an inner burn
    /// and a coast node, in the full field.
    #[test]
    fn derivatives_match_differences() {
        let eph = Ephemeris::standard();
        let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
        let field = Field::all(eph, Some(earth)).with_oblateness();
        let epoch = Epoch::from_ymd(2045, 3, 1);
        let ode = field.ode(epoch);
        // A reference: an arc near the Moon, which the ship meets.
        let m = field.body_state(moon, epoch);
        let near = [
            m.r.x + 20_000.0,
            m.r.y,
            m.r.z + 5_000.0,
            m.v.x,
            m.v.y + 0.3,
            m.v.z,
        ];
        let mut run = Dop853::new(&ode, 0.0, near, TOL);
        let mut steps = Vec::new();
        run.advance_dense(&ode, 10.0 * 86_400.0, &mut steps)
            .unwrap();
        let target = Reference {
            epoch,
            center: Some(earth),
            revolutions: 1,
            patches: Vec::new(),
            path: crate::integrate::Trajectory::new(steps),
        };
        let leg = Leg {
            field: &field,
            locals: &[],
            from: End::Low(LowOrbit {
                body: earth,
                radius: 6_778.0,
                inclination: Some(0.5),
            }),
            to: End::Orbit(&target),
            depart: Some(100.0),
            arrive: None,
            duration: Some(3.0 * 86_400.0),
            window: None,
        };
        // A rough path: away from Earth, a burn node and a coast node.
        let start = [6_800.0, 300.0, 200.0, 0.4, 10.6, 1.5];
        let mut run = Dop853::new(&ode, 100.0, start, TOL);
        let times = [100.0, 40_000.0, 150_000.0, 3.1 * 86_400.0];
        let mut states = vec![start];
        for &t in &times[1..3] {
            run.advance_to(&ode, t).unwrap();
            let mut s = run.y;
            s[4] += 0.01;
            states.push(s);
        }
        let path = Path {
            epoch,
            times: times.to_vec(),
            states,
            burns: vec![true, true, false, true],
            passes: Vec::new(),
        };
        let positions: Vec<DVec3> = path
            .states
            .iter()
            .map(|s| DVec3::new(s[0], s[1], s[2]))
            .chain([target.path.at(3.1 * 86_400.0)].map(|s| DVec3::new(s[0], s[1], s[2])))
            .collect();
        let layout = Layout::new(&field, &[], &path, &positions);
        assert_derivatives(&leg, &layout, epoch, &layout.pack(&path));
    }

    /// The same leg with its nodes barycentric and its arcs integrated about
    /// Earth (a local frame), toward a station orbit recorded about Earth.
    #[test]
    fn derivatives_through_a_local_frame() {
        let eph = Ephemeris::standard();
        let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
        let global = Field::all(eph, None).with_oblateness();
        let locals = [Field::all(eph, Some(earth)).with_oblateness()];
        let epoch = Epoch::from_ymd(2045, 3, 1);
        let ode = locals[0].ode(epoch);
        let m = locals[0].body_state(moon, epoch);
        let near = [
            m.r.x + 20_000.0,
            m.r.y,
            m.r.z + 5_000.0,
            m.v.x,
            m.v.y + 0.3,
            m.v.z,
        ];
        let target = flown(&locals[0], epoch, 0.0, 10.0 * 86_400.0, near);
        let leg = Leg {
            field: &global,
            locals: &locals,
            from: End::Low(LowOrbit {
                body: earth,
                radius: 6_778.0,
                inclination: Some(0.5),
            }),
            to: End::Orbit(&target),
            depart: Some(100.0),
            arrive: None,
            duration: Some(3.0 * 86_400.0),
            window: None,
        };
        let barycentric = |t: f64, s: [f64; 6]| -> [f64; 6] {
            let e = eph.barycentric(earth, Epoch(epoch.0 + t / SECONDS_PER_DAY));
            [
                s[0] + e.r.x,
                s[1] + e.r.y,
                s[2] + e.r.z,
                s[3] + e.v.x,
                s[4] + e.v.y,
                s[5] + e.v.z,
            ]
        };
        let start = [6_800.0, 300.0, 200.0, 0.4, 10.6, 1.5];
        let mut run = Dop853::new(&ode, 100.0, start, TOL);
        let times = [100.0, 40_000.0, 150_000.0, 3.1 * 86_400.0];
        let mut states = vec![barycentric(times[0], start)];
        for &t in &times[1..3] {
            run.advance_to(&ode, t).unwrap();
            let mut s = run.y;
            s[4] += 0.01;
            states.push(barycentric(t, s));
        }
        let path = Path {
            epoch,
            times: times.to_vec(),
            states,
            burns: vec![true, true, false, true],
            passes: Vec::new(),
        };
        let end = barycentric(times[3], target.path.at(times[3]));
        let positions: Vec<DVec3> = path
            .states
            .iter()
            .chain([&end])
            .map(|s| DVec3::new(s[0], s[1], s[2]))
            .collect();
        let layout = Layout::new(&global, &locals, &path, &positions);
        assert!(
            layout.frames.iter().all(|f| *f == Some(0)),
            "{:?}",
            layout.frames
        );
        assert_derivatives(&leg, &layout, epoch, &layout.pack(&path));
    }

    /// Central differences of every constraint row and the cost against the
    /// analytic derivatives, at steps of 1e-5, 1e-6 and 1e-7 (scaled), any
    /// of which may match: the largest misses by its truncation at a LEO
    /// node (68 m there), the smallest by the integrator's step-size noise
    /// on arcs near the Moon, or by rounding in barycentric coordinates
    /// (~0.03 mm at Earth's distance from the Sun).
    fn assert_derivatives(leg: &Leg, layout: &Layout, epoch: Epoch, z: &[f64]) {
        let e = leg.evaluate(layout, epoch, z, true).unwrap();
        let differences = |h: f64| -> Vec<(f64, Vec<f64>)> {
            (0..z.len())
                .map(|j| {
                    let at = |dz: f64| {
                        let mut z2 = z.to_vec();
                        z2[j] += dz;
                        leg.evaluate(layout, epoch, &z2, false).unwrap()
                    };
                    let (plus, minus) = (at(h), at(-h));
                    let rows = (0..e.c.len())
                        .map(|r| (plus.c[r] - minus.c[r]) / (2.0 * h))
                        .collect();
                    ((plus.cost - minus.cost) / (2.0 * h), rows)
                })
                .collect()
        };
        let steps = [differences(1e-5), differences(1e-6), differences(1e-7)];
        let err = |analytic: f64, numeric: f64| (analytic - numeric).abs() / (1.0 + numeric.abs());
        let mut worst = 0.0f64;
        for j in 0..z.len() {
            let mut check = |analytic: f64, numeric: [f64; 3], what: &str| {
                let e = numeric
                    .iter()
                    .map(|&n| err(analytic, n))
                    .fold(f64::INFINITY, f64::min);
                worst = worst.max(e);
                assert!(
                    e < 1e-4,
                    "{what}, unknown {j}: analytic {analytic:e}, differences {numeric:?}"
                );
            };
            check(e.grad[j], steps.each_ref().map(|d| d[j].0), "cost");
            for r in 0..e.c.len() {
                let numeric = steps.each_ref().map(|d| d[j].1[r]);
                check(e.jac[r][j], numeric, &format!("row {r}"));
            }
        }
        println!("worst relative difference {worst:e}");
    }

    /// A reference: `s` at `t0` integrated to `t1`.
    fn flown(field: &Field, epoch: Epoch, t0: f64, t1: f64, s: [f64; 6]) -> Reference {
        let ode = field.ode(epoch);
        let mut run = Dop853::new(&ode, t0, s, TOL);
        let mut steps = Vec::new();
        run.advance_dense(&ode, t1, &mut steps).unwrap();
        Reference {
            epoch,
            center: field.center(),
            revolutions: 1,
            patches: Vec::new(),
            path: crate::integrate::Trajectory::new(steps),
        }
    }

    /// The other terms: a station orbit as the start, a lunar pass held at
    /// its floor as both arcs' periapsis, coasts on both sides, a
    /// low lunar orbit end at an inclination, the window penalty below its
    /// early edge, and a fixed arrival.
    #[test]
    fn derivatives_with_a_held_pass_and_window() {
        let eph = Ephemeris::standard();
        let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
        let field = Field::new(eph, Some(earth), &[eph.sun(), earth, moon]).with_oblateness();
        let epoch = Epoch::from_ymd(2045, 3, 1);
        let ode = field.ode(epoch);
        let times = [1_000.0, 30_000.0, 60_000.0, 90_000.0, 120_000.0];
        // A pass 3,000 km from the Moon, flown back to the start.
        let m = field.body_state(moon, Epoch(epoch.0 + times[2] / SECONDS_PER_DAY));
        let pass = [
            m.r.x + 3_000.0,
            m.r.y,
            m.r.z + 300.0,
            m.v.x + 0.05,
            m.v.y + 1.8,
            m.v.z + 0.1,
        ];
        let mut back = Dop853::new(&ode, times[2], pass, TOL);
        back.advance_to(&ode, times[1]).unwrap();
        let mut s1 = back.y;
        s1[5] += 0.002;
        back.advance_to(&ode, times[0]).unwrap();
        // The station passes the start with a different velocity.
        let mut on_station = back.y;
        on_station[3] -= 0.05;
        on_station[4] += 0.03;
        let mut before = Dop853::new(&ode, times[0], on_station, TOL);
        before.advance_to(&ode, -86_400.0).unwrap();
        let station = flown(&field, epoch, -86_400.0, 10.0 * 86_400.0, before.y);
        let mut s0 = station.path.at(times[0]);
        s0[3] += 0.05;
        s0[4] -= 0.03;
        let mut leave = pass;
        leave[4] += 0.05;
        let mut after = Dop853::new(&ode, times[2], leave, TOL);
        after.advance_to(&ode, times[3]).unwrap();
        let mut s3 = after.y;
        s3[0] += 1.0;
        after.advance_to(&ode, times[4]).unwrap();
        let path = Path {
            epoch,
            times: times.to_vec(),
            states: vec![s0, s1, leave, s3],
            burns: vec![true, false, true, false, true],
            passes: vec![Pass {
                node: 2,
                body: moon,
                min_radius: 1_838.0,
            }],
        };
        let positions: Vec<DVec3> = path
            .states
            .iter()
            .chain([&after.y])
            .map(|s| DVec3::new(s[0], s[1], s[2]))
            .collect();
        let mut layout = Layout::new(&field, &[], &path, &positions);
        layout.passes[0].1 = true;
        let leg = Leg {
            field: &field,
            locals: &[],
            from: End::Orbit(&station),
            to: End::Low(LowOrbit {
                body: moon,
                radius: eph.body(moon).radius + 100.0,
                inclination: Some(1.2),
            }),
            depart: None,
            arrive: Some(times[4] + 50.0),
            duration: None,
            window: Some((times[0] + 0.5 * 86_400.0, times[0] + 2.0 * 86_400.0)),
        };
        assert_derivatives(&leg, &layout, epoch, &layout.pack(&path));
    }

    /// And: LEO with the plane free as the start, a pass not held, a station
    /// orbit as the end, the window penalty past its late edge, and a fixed
    /// duration.
    #[test]
    fn derivatives_with_a_free_pass_and_late_window() {
        let eph = Ephemeris::standard();
        let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
        let field = Field::new(eph, Some(earth), &[eph.sun(), earth, moon]).with_oblateness();
        let epoch = Epoch::from_ymd(2045, 3, 1);
        let ode = field.ode(epoch);
        let start = [6_800.0, 300.0, 200.0, 0.4, 10.6, 1.5];
        let times = [100.0, 30_000.0, 80_000.0, 150_000.0, 3.0 * 86_400.0];
        let mut run = Dop853::new(&ode, times[0], start, TOL);
        let mut states = vec![start];
        for &t in &times[1..4] {
            run.advance_to(&ode, t).unwrap();
            let mut s = run.y;
            s[4] += 0.01;
            states.push(s);
        }
        run.advance_to(&ode, times[4]).unwrap();
        let mut back = Dop853::new(&ode, times[4], run.y, TOL);
        back.advance_to(&ode, 0.0).unwrap();
        let mut s = back.y;
        s[3] += 0.01;
        let station = flown(&field, epoch, 0.0, 10.0 * 86_400.0, s);
        let path = Path {
            epoch,
            times: times.to_vec(),
            states,
            burns: vec![true, false, true, false, true],
            passes: vec![Pass {
                node: 2,
                body: moon,
                min_radius: 1_838.0,
            }],
        };
        let positions: Vec<DVec3> = path
            .states
            .iter()
            .chain([&run.y])
            .map(|s| DVec3::new(s[0], s[1], s[2]))
            .collect();
        let layout = Layout::new(&field, &[], &path, &positions);
        let leg = Leg {
            field: &field,
            locals: &[],
            from: End::Low(LowOrbit {
                body: earth,
                radius: 6_778.0,
                inclination: None,
            }),
            to: End::Orbit(&station),
            depart: None,
            arrive: None,
            duration: Some(3.0 * 86_400.0),
            window: Some((-2.0 * 86_400.0, -0.5 * 86_400.0)),
        };
        assert_derivatives(&leg, &layout, epoch, &layout.pack(&path));
    }
}
