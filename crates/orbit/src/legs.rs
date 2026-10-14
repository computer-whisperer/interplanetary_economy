//! Legs between station orbits and low circular orbits in cislunar space
//! (M1a.md, Autopilot, playbook entries 2 and 5): LEO to and from the
//! Lagrange-point orbits and low lunar orbit, those orbits to low lunar
//! orbit, and station to station. [`plan`] is the entry point.
//!
//! Seeds come from fans: trajectories leaving one end (or, integrated
//! backward, reaching it) with burns of several sizes in many directions.
//! From a station toward a low orbit, each is followed to its first
//! periapsis about the low orbit's body, directly or after a lunar flyby,
//! where a second fan (a burn there, possibly none) continues; toward a
//! station, to its closest approach to the station; between two low orbits,
//! from periapses of the far one. Seeds are designed in the full field
//! ([`crate::shooting`]), which makes them meet both ends exactly and
//! minimizes their total Δv.

use crate::ephem::BodyId;
use crate::gravity::Field;
use crate::integrate::{DenseStep, Dop853, Tolerance};
use crate::reference::Reference;
use crate::shooting::{Design, End, Leg, LowOrbit, Pass, Path};
use crate::time::{Epoch, SECONDS_PER_DAY};
use glam::DVec3;
use rayon::prelude::*;

const TOL: Tolerance = Tolerance {
    rtol: 1e-12,
    atol: 1e-9,
};

/// Seeds' arcs last at most this long, s.
const ARC_S: f64 = 0.5 * SECONDS_PER_DAY;

/// Fan trajectories are abandoned this far from the body they look for a
/// periapsis about, km.
const BEYOND_KM: f64 = 1_000_000.0;

/// Burns at a lunar flyby lie within this angle of the velocity there
/// (relative to the Moon), whose size they mostly change: cos of 60°.
const FLYBY_CONE_COS: f64 = 0.5;

/// Lowest lunar flyby, km above the mean radius (Artemis I flew 130 km).
const FLYBY_ALTITUDE_KM: f64 = 100.0;

/// Which way a transfer runs between the station's orbit and the low orbit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Direction {
    /// From the station's orbit to the low orbit.
    Down,
    /// From the low orbit to the station's orbit.
    Up,
}

impl Direction {
    /// The direction fans integrate in, from the station's orbit.
    fn sign(self) -> f64 {
        match self {
            Direction::Down => 1.0,
            Direction::Up => -1.0,
        }
    }
}

/// A burn from an origin state, and the periapsis it leads to.
#[derive(Clone, Copy, Debug)]
pub struct Shot {
    /// Index of the origin in the fan's list.
    pub origin: usize,
    /// km/s, added to the origin's velocity.
    pub burn: DVec3,
    /// s after the fan's epoch.
    pub periapsis_at: f64,
    pub periapsis: [f64; 6],
    /// Distance from the body there, and the speed relative to it, km and
    /// km/s.
    pub radius: f64,
    pub speed: f64,
}

/// The first periapsis about `body` of a ship at `state` at `t` (s after
/// `epoch`), integrating forward (`dir` = 1) or backward (-1) for at most
/// `span` s: its time and state. `None` if there is none, or the ship goes
/// below the surface of any of the field's bodies or farther than
/// [`BEYOND_KM`] from `body` first.
pub fn periapsis(
    field: &Field,
    epoch: Epoch,
    t: f64,
    state: [f64; 6],
    dir: f64,
    span: f64,
    body: BodyId,
) -> Option<(f64, [f64; 6])> {
    let ode = field.ode(epoch);
    let when = |t: f64| Epoch(epoch.0 + t / SECONDS_PER_DAY);
    // The rate at which the distance from the body changes, along the
    // direction of integration.
    let rate = |t: f64, s: &[f64; 6]| -> (f64, f64) {
        let b = field.body_state(body, when(t));
        let rho = DVec3::new(s[0], s[1], s[2]) - b.r;
        (
            dir * rho.dot(DVec3::new(s[3], s[4], s[5]) - b.v),
            rho.length(),
        )
    };
    let mut run = Dop853::new(&ode, t, state, TOL);
    let end = t + dir * span;
    let mut approaching = rate(t, &state).0 < 0.0;
    while run.t != end {
        let step: DenseStep<6> = run.step(&ode, end).ok()?;
        if field
            .inside(when(run.t), DVec3::new(run.y[0], run.y[1], run.y[2]))
            .is_some()
        {
            return None;
        }
        let (r, distance) = rate(run.t, &run.y);
        if distance > BEYOND_KM {
            return None;
        }
        if approaching && r >= 0.0 {
            // Bisect the step's interpolant for the turn.
            let (mut lo, mut hi) = (step.t0, step.t0 + step.h);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if rate(mid, &step.at(mid)).0 < 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let tp = 0.5 * (lo + hi);
            return Some((tp, step.at(tp)));
        }
        approaching = r < 0.0;
    }
    None
}

/// `count` directions spread evenly over the sphere (a Fibonacci lattice).
pub fn directions(count: usize) -> Vec<DVec3> {
    let golden = std::f64::consts::PI * (3.0 - 5.0f64.sqrt());
    (0..count)
        .map(|i| {
            let z = 1.0 - (2.0 * i as f64 + 1.0) / count as f64;
            let r = (1.0 - z * z).sqrt();
            let a = golden * i as f64;
            DVec3::new(r * a.cos(), r * a.sin(), z)
        })
        .collect()
}

/// `count` directions spread evenly over the cap within the angle whose
/// cosine is `cos` of +z.
pub fn cone(count: usize, cos: f64) -> Vec<DVec3> {
    let golden = std::f64::consts::PI * (3.0 - 5.0f64.sqrt());
    (0..count)
        .map(|i| {
            let z = 1.0 - (1.0 - cos) * (i as f64 + 0.5) / count as f64;
            let r = (1.0 - z * z).sqrt();
            let a = golden * i as f64;
            DVec3::new(r * a.cos(), r * a.sin(), z)
        })
        .collect()
}

/// What a fan sends and where it looks.
#[derive(Clone, Debug)]
pub struct Spread {
    /// Burn sizes, km/s (0 for a coast), each in every direction.
    pub sizes: Vec<f64>,
    pub directions: Vec<DVec3>,
    /// If set, `directions` are relative to each origin's velocity relative
    /// to this body (+z along it), not inertial.
    pub along: Option<BodyId>,
    /// How long to follow each trajectory, s.
    pub span: f64,
    /// The body whose periapsis ends it, and the radii to keep, km.
    pub body: BodyId,
    pub radii: (f64, f64),
}

/// Burns from each of `origins` ((s after `epoch`, state)), followed (in
/// `dir`, ±1) to a periapsis about `spread.body` within `spread.radii`, in
/// parallel. The trajectories are seeds, so `field` can be lighter than a
/// design's (the Sun, Earth and Moon alone integrate 2.5× faster than every
/// body with J2).
pub fn fan(
    field: &Field,
    epoch: Epoch,
    origins: &[(f64, [f64; 6])],
    spread: &Spread,
    dir: f64,
) -> Vec<Shot> {
    let mut burns: Vec<DVec3> = Vec::new();
    for &size in &spread.sizes {
        if size == 0.0 {
            burns.push(DVec3::ZERO);
        } else {
            burns.extend(spread.directions.iter().map(|&u| size * u));
        }
    }
    let combos: Vec<(usize, DVec3)> = (0..origins.len())
        .flat_map(|o| burns.iter().map(move |&b| (o, b)))
        .collect();
    // Each origin's frame for the burns: identity, or +z along its velocity
    // relative to `spread.along`.
    let frames: Vec<[DVec3; 3]> = origins
        .iter()
        .map(|&(t, s)| match spread.along {
            None => [DVec3::X, DVec3::Y, DVec3::Z],
            Some(b) => {
                let v = DVec3::new(s[3], s[4], s[5])
                    - field.body_state(b, Epoch(epoch.0 + t / SECONDS_PER_DAY)).v;
                let z = v.normalize();
                let x = z.any_orthonormal_vector();
                [x, z.cross(x), z]
            }
        })
        .collect();
    combos
        .par_iter()
        .filter_map(|&(origin, local)| {
            let [fx, fy, fz] = frames[origin];
            let burn = local.x * fx + local.y * fy + local.z * fz;
            let (t, mut s) = origins[origin];
            for i in 0..3 {
                s[3 + i] += burn[i];
            }
            let (tp, sp) = periapsis(field, epoch, t, s, dir, spread.span, spread.body)?;
            let b = field.body_state(spread.body, Epoch(epoch.0 + tp / SECONDS_PER_DAY));
            let radius = (DVec3::new(sp[0], sp[1], sp[2]) - b.r).length();
            (radius >= spread.radii.0 && radius <= spread.radii.1).then(|| Shot {
                origin,
                burn,
                periapsis_at: tp,
                periapsis: sp,
                radius,
                speed: (DVec3::new(sp[3], sp[4], sp[5]) - b.v).length(),
            })
        })
        .collect()
}

/// The burn into (or out of) a circular orbit of `radius` about a body of
/// `gm` at periapsis, for a trajectory with periapsis `rp` and speed `vp`
/// there, were the periapsis moved to `radius` at the same energy.
fn low_burn(gm: f64, radius: f64, rp: f64, vp: f64) -> f64 {
    let at = (vp * vp + 2.0 * gm * (1.0 / radius - 1.0 / rp))
        .max(0.0)
        .sqrt();
    (at - (gm / radius).sqrt()).abs()
}

/// One leg of a fan chain: from `origin` (s after the epoch, state) with
/// `burn`, to a periapsis at `end`.
#[derive(Clone, Copy, Debug)]
struct Link {
    at: f64,
    origin: [f64; 6],
    burn: DVec3,
    end: f64,
}

/// A seed path for a transfer, with its estimated total Δv.
#[derive(Clone, Debug)]
pub struct Seed {
    pub path: Path,
    pub estimate: f64,
}

/// What a search between a station's orbit and a low orbit tries.
#[derive(Clone, Debug)]
pub struct Search {
    /// Times on the reference to leave from (`Down`) or arrive at (`Up`), s
    /// after its epoch.
    pub times: Vec<f64>,
    /// Burn sizes at the station's orbit, km/s.
    pub sizes: Vec<f64>,
    /// Burn sizes at a lunar flyby (0 for none); empty to seed direct
    /// transfers only.
    pub flyby_sizes: Vec<f64>,
    /// Burn directions at the station's orbit and at a flyby (within 60°
    /// of the velocity there).
    pub directions: usize,
    pub flyby_directions: usize,
    /// Durations to keep seeds within, s; the longer bounds how far fans
    /// follow their trajectories.
    pub durations: (f64, f64),
    /// Seeds to design.
    pub designs: usize,
}

/// Seeds for transfers between `reference` and `low`, cheapest estimate
/// first: direct ones, and via a lunar flyby when `search.flyby_sizes` is
/// set. Fans fly in `light`, seeds again in `field`.
pub fn seeds(
    field: &Field,
    light: &Field,
    reference: &Reference,
    low: &LowOrbit,
    direction: Direction,
    search: &Search,
) -> Vec<Seed> {
    let eph = field.ephemeris();
    let epoch = reference.epoch;
    let dir = direction.sign();
    let gm = eph.body(low.body).gm;
    let origins: Vec<(f64, [f64; 6])> = search
        .times
        .iter()
        .map(|&t| (t, reference.path.at(t)))
        .collect();
    let dirs = directions(search.directions);
    let to_low = |sizes: &[f64], directions: Vec<DVec3>, along: Option<BodyId>| Spread {
        sizes: sizes.to_vec(),
        directions,
        along,
        span: search.durations.1,
        body: low.body,
        radii: (eph.body(low.body).radius, 3.0 * low.radius),
    };
    let mut chains: Vec<(Vec<Link>, f64)> = Vec::new();
    for shot in fan(
        light,
        epoch,
        &origins,
        &to_low(&search.sizes, dirs.clone(), None),
        dir,
    ) {
        let (at, origin) = origins[shot.origin];
        let estimate = shot.burn.length() + low_burn(gm, low.radius, shot.radius, shot.speed);
        let link = Link {
            at,
            origin,
            burn: shot.burn,
            end: shot.periapsis_at,
        };
        chains.push((vec![link], estimate));
    }
    let moon = eph.lookup("moon").unwrap();
    if !search.flyby_sizes.is_empty() && low.body != moon {
        let radius = eph.body(moon).radius;
        let flybys = fan(
            light,
            epoch,
            &origins,
            &Spread {
                sizes: search.sizes.clone(),
                directions: dirs.clone(),
                along: None,
                span: search.durations.1,
                body: moon,
                radii: (radius + 50.0, radius + 5_000.0),
            },
            dir,
        );
        let passes: Vec<(f64, [f64; 6])> = flybys
            .iter()
            .map(|f| (f.periapsis_at, f.periapsis))
            .collect();
        let onward = to_low(
            &search.flyby_sizes,
            cone(search.flyby_directions, FLYBY_CONE_COS),
            Some(moon),
        );
        for shot in fan(light, epoch, &passes, &onward, dir) {
            let first = &flybys[shot.origin];
            let (at, origin) = origins[first.origin];
            let estimate = first.burn.length()
                + shot.burn.length()
                + low_burn(gm, low.radius, shot.radius, shot.speed);
            let links = vec![
                Link {
                    at,
                    origin,
                    burn: first.burn,
                    end: first.periapsis_at,
                },
                Link {
                    at: first.periapsis_at,
                    origin: first.periapsis,
                    burn: shot.burn,
                    end: shot.periapsis_at,
                },
            ];
            chains.push((links, estimate));
        }
    }
    // Direct and flyby seeds alternate, each cheapest first: flybys win
    // the estimates at long durations, and would crowd out the direct
    // seeds that short ones need.
    let (shortest, longest) = search.durations;
    chains.retain(|(links, _)| {
        let d = (links.last().unwrap().end - links[0].at).abs();
        d >= shortest && d <= longest
    });
    chains.sort_by(|a, b| a.1.total_cmp(&b.1));
    let (direct, flyby): (Vec<_>, Vec<_>) = chains.into_iter().partition(|c| c.0.len() == 1);
    let mut kinds = [direct.into_iter(), flyby.into_iter()];
    let mut out = Vec::new();
    let mut turn = 0;
    while out.len() < search.designs {
        let Some((links, estimate)) = kinds[turn % 2]
            .next()
            .or_else(|| kinds[(turn + 1) % 2].next())
        else {
            break;
        };
        turn += 1;
        if let Some(path) = chain_path(field, epoch, direction, &links) {
            out.push(Seed { path, estimate });
        }
    }
    out
}

/// Times of the nodes after `from` along a link to `to` (either way in
/// time), ending at `to`: at most [`ARC_S`] apart, and 1 and 4 hours from
/// each end, where the ship passes a body and its path is most sensitive.
/// With half-day arcs beside a lunar flyby, a few m/s at perilune moved the
/// next node thousands of km, past what Newton's steps could follow.
fn node_times(from: f64, to: f64) -> Vec<f64> {
    let span = (to - from).abs();
    let arcs = ((span / ARC_S).ceil() as usize).max(2);
    let mut offsets: Vec<f64> = (1..=arcs).map(|j| span * j as f64 / arcs as f64).collect();
    for near in [3_600.0, 4.0 * 3_600.0] {
        if 3.0 * near < span {
            offsets.extend([near, span - near]);
        }
    }
    offsets.sort_by(|a, b| a.total_cmp(b));
    offsets.dedup_by(|a, b| (*a - *b).abs() < 1_800.0);
    // The end stays exact.
    *offsets.last_mut().unwrap() = span;
    let dir = (to - from).signum();
    offsets.into_iter().map(|o| from + dir * o).collect()
}

/// A fan chain flown again in the design field and split into arcs (see
/// [`node_times`]), as a path in time order: a burn node where each link
/// starts, coast nodes along it. `None` if the trajectory hits a body.
fn chain_path(field: &Field, epoch: Epoch, direction: Direction, links: &[Link]) -> Option<Path> {
    let ode = field.ode(epoch);
    // Each link's states along it, in the fan's direction, ending with its
    // end.
    let mut along: Vec<Vec<(f64, [f64; 6])>> = Vec::new();
    for link in links {
        let mut flown = link.origin;
        for i in 0..3 {
            flown[3 + i] += link.burn[i];
        }
        let mut run = Dop853::new(&ode, link.at, flown, TOL);
        let mut states = Vec::new();
        for t in node_times(link.at, link.end) {
            run.advance_to(&ode, t).ok()?;
            let r = DVec3::new(run.y[0], run.y[1], run.y[2]);
            if field
                .inside(Epoch(epoch.0 + t / SECONDS_PER_DAY), r)
                .is_some()
            {
                return None;
            }
            states.push((t, run.y));
        }
        along.push(states);
    }
    let (mut times, mut states, mut burns) = (Vec::new(), Vec::new(), Vec::new());
    match direction {
        // Forward in time already: each link leaves its origin with the
        // burn added.
        Direction::Down => {
            for (link, path) in links.iter().zip(&along) {
                let mut flown = link.origin;
                for i in 0..3 {
                    flown[3 + i] += link.burn[i];
                }
                times.push(link.at);
                states.push(flown);
                burns.push(true);
                for &(t, s) in &path[..path.len() - 1] {
                    times.push(t);
                    states.push(s);
                    burns.push(false);
                }
            }
            times.push(links.last().unwrap().end);
            burns.push(true);
        }
        // Reversed: the last link's end (the low orbit) comes first. Each
        // link's coast states are the ship's states there in either
        // direction; a link's origin is where the ship, forward in time,
        // leaves with the origin state (the burn undone), except at the
        // station's orbit, which ends the path.
        Direction::Up => {
            let &(t, s) = along.last().unwrap().last().unwrap();
            times.push(t);
            states.push(s);
            burns.push(true);
            for (k, (link, path)) in links.iter().zip(&along).enumerate().rev() {
                for &(t, s) in path[..path.len() - 1].iter().rev() {
                    times.push(t);
                    states.push(s);
                    burns.push(false);
                }
                times.push(link.at);
                burns.push(true);
                if k > 0 {
                    states.push(link.origin);
                }
            }
        }
    }
    // Every burn node but the ends is a lunar flyby.
    let moon = field.ephemeris().lookup("moon").unwrap();
    let last = burns.len() - 1;
    let passes = (1..last)
        .filter(|&k| burns[k])
        .map(|node| Pass {
            node,
            body: moon,
            min_radius: field.ephemeris().body(moon).radius + FLYBY_ALTITUDE_KM,
        })
        .collect();
    Some(Path {
        epoch,
        times,
        states,
        burns,
        passes,
    })
}

/// The closest approach of a ship at `state` at `t` (s after `epoch`) to
/// where `target` is at the same moment, within `span` s forward: its time,
/// the ship's state, and the distance. `None` if the ship goes below a
/// body's surface or farther than [`BEYOND_KM`] from the field's origin, or
/// the target's span ends first.
pub fn closest(
    field: &Field,
    epoch: Epoch,
    t: f64,
    state: [f64; 6],
    target: &Reference,
    span: f64,
) -> Option<(f64, [f64; 6], f64)> {
    let ode = field.ode(epoch);
    let offset = (epoch.0 - target.epoch.0) * SECONDS_PER_DAY;
    let (_, last) = target.path.span();
    let end = (t + span).min(last - offset);
    if end <= t {
        return None;
    }
    let gap = |t: f64, s: &[f64; 6]| {
        let x = target.path.at(t + offset);
        (DVec3::new(s[0], s[1], s[2]) - DVec3::new(x[0], x[1], x[2])).length()
    };
    let mut run = Dop853::new(&ode, t, state, TOL);
    // The step ending at the nearest sampled point, and the one after it:
    // the minimum lies in one of the two.
    let mut best: Option<(DenseStep<6>, Option<DenseStep<6>>, f64)> = None;
    let mut waiting = false;
    while run.t != end {
        let step = run.step(&ode, end).ok()?;
        let r = DVec3::new(run.y[0], run.y[1], run.y[2]);
        if r.length() > BEYOND_KM
            || field
                .inside(Epoch(epoch.0 + run.t / SECONDS_PER_DAY), r)
                .is_some()
        {
            return None;
        }
        if waiting && let Some(b) = best.as_mut() {
            b.1 = Some(step.clone());
        }
        let d = gap(run.t, &run.y);
        waiting = best.as_ref().is_none_or(|b| d < b.2);
        if waiting {
            best = Some((step, None, d));
        }
    }
    // Golden section over both steps' interpolants (time runs forward).
    let (step, after, _) = best?;
    let state_at = |t: f64| match &after {
        Some(a) if t > step.t0 + step.h => a.at(t),
        _ => step.at(t),
    };
    let (mut lo, mut hi) = (
        step.t0,
        after.as_ref().map_or(step.t0 + step.h, |a| a.t0 + a.h),
    );
    let g = 0.5 * (5.0f64.sqrt() - 1.0);
    for _ in 0..60 {
        let (a, b) = (hi - g * (hi - lo), lo + g * (hi - lo));
        if gap(a, &state_at(a)) < gap(b, &state_at(b)) {
            hi = b;
        } else {
            lo = a;
        }
    }
    let tc = 0.5 * (lo + hi);
    let sc = state_at(tc);
    Some((tc, sc, gap(tc, &sc)))
}

/// Seeds for rendezvous transfers from `from` to `to` (station orbits),
/// cheapest estimate first: a fan of burns from `from` at `search.times`,
/// each trajectory's closest approach to `to` within `search.durations`, kept
/// within `reach` km. The estimate adds the departure burn, the burn to
/// match `to`'s velocity there, and the miss spread over the flight.
pub fn rendezvous_seeds(
    field: &Field,
    light: &Field,
    from: &Reference,
    to: &Reference,
    search: &Search,
    reach: f64,
) -> Vec<Seed> {
    let epoch = from.epoch;
    let offset = (epoch.0 - to.epoch.0) * SECONDS_PER_DAY;
    let dirs = directions(search.directions);
    let mut combos: Vec<(f64, DVec3)> = Vec::new();
    for &t in &search.times {
        for &size in &search.sizes {
            combos.extend(dirs.iter().map(|&u| (t, size * u)));
        }
    }
    let mut found: Vec<(Link, f64)> = combos
        .par_iter()
        .filter_map(|&(t, burn)| {
            let origin = from.path.at(t);
            let mut s = origin;
            for i in 0..3 {
                s[3 + i] += burn[i];
            }
            let (tc, sc, miss) = closest(light, epoch, t, s, to, search.durations.1)?;
            if miss > reach || tc - t < search.durations.0 {
                return None;
            }
            let x = to.path.at(tc + offset);
            let matching = (DVec3::new(x[3] - sc[3], x[4] - sc[4], x[5] - sc[5])).length();
            let link = Link {
                at: t,
                origin,
                burn,
                end: tc,
            };
            Some((link, burn.length() + matching + miss / (tc - t)))
        })
        .collect();
    found.sort_by(|a, b| a.1.total_cmp(&b.1));
    found
        .into_iter()
        .filter_map(|(link, estimate)| {
            Some(Seed {
                path: chain_path(field, epoch, Direction::Down, &[link])?,
                estimate,
            })
        })
        .take(search.designs)
        .collect()
}

/// Seeds for transfers between two low orbits, `far` (low lunar orbit) and
/// `near` (LEO): a fan from periapses of `far` (each of `count` points on
/// the sphere of its radius, at each of `times`, in each plane through the
/// point at its inclination, at each hyperbolic excess speed in `excess`,
/// km/s), coasting (backward for `Up`, from `near` to `far`) to a periapsis
/// near `near`, lasting within `durations` (s).
#[allow(clippy::too_many_arguments)]
pub fn low_seeds(
    field: &Field,
    light: &Field,
    epoch: Epoch,
    far: &LowOrbit,
    near: &LowOrbit,
    direction: Direction,
    times: &[f64],
    excess: &[f64],
    count: usize,
    durations: (f64, f64),
    designs: usize,
) -> Vec<Seed> {
    let eph = field.ephemeris();
    let gm = eph.body(far.body).gm;
    let mut origins: Vec<(f64, [f64; 6])> = Vec::new();
    for &t in times {
        let when = Epoch(epoch.0 + t / SECONDS_PER_DAY);
        let b = field.body_state(far.body, when);
        let pole = eph.pole(far.body, when);
        for rho in directions(count) {
            // Normals of the planes through `rho` at the inclination.
            let normals: Vec<DVec3> = match (far.inclination, pole) {
                (Some(i), Some(p)) => {
                    let u = (p - p.dot(rho) * rho).normalize_or_zero();
                    let w = rho.cross(u);
                    let a = if u.dot(p).abs() > 1e-9 {
                        i.cos() / u.dot(p)
                    } else {
                        2.0
                    };
                    if a.abs() > 1.0 {
                        continue;
                    }
                    let c = (1.0 - a * a).sqrt();
                    vec![a * u + c * w, a * u - c * w]
                }
                _ => {
                    let u = rho.any_orthonormal_vector();
                    let w = rho.cross(u);
                    (0..8)
                        .map(|k| {
                            let q = std::f64::consts::TAU * k as f64 / 8.0;
                            q.cos() * u + q.sin() * w
                        })
                        .collect()
                }
            };
            for h in normals {
                let along = h.cross(rho);
                for &vinf in excess {
                    let speed = (vinf * vinf + 2.0 * gm / far.radius).sqrt();
                    let (r, v) = (b.r + far.radius * rho, b.v + speed * along);
                    origins.push((t, [r.x, r.y, r.z, v.x, v.y, v.z]));
                }
            }
        }
    }
    let spread = Spread {
        sizes: vec![0.0],
        directions: Vec::new(),
        along: None,
        span: durations.1,
        body: near.body,
        radii: (eph.body(near.body).radius, 3.0 * near.radius),
    };
    let gm_near = eph.body(near.body).gm;
    let mut chains: Vec<(Link, f64)> = fan(light, epoch, &origins, &spread, direction.sign())
        .into_iter()
        .map(|shot| {
            let (at, origin) = origins[shot.origin];
            let o = DVec3::new(origin[3], origin[4], origin[5])
                - field
                    .body_state(far.body, Epoch(epoch.0 + at / SECONDS_PER_DAY))
                    .v;
            let estimate = low_burn(gm, far.radius, far.radius, o.length())
                + low_burn(gm_near, near.radius, shot.radius, shot.speed);
            let link = Link {
                at,
                origin,
                burn: DVec3::ZERO,
                end: shot.periapsis_at,
            };
            (link, estimate)
        })
        .filter(|(link, _)| (link.end - link.at).abs() >= durations.0)
        .collect();
    chains.sort_by(|a, b| a.1.total_cmp(&b.1));
    chains
        .into_iter()
        .filter_map(|(link, estimate)| {
            Some(Seed {
                path: chain_path(field, epoch, direction, &[link])?,
                estimate,
            })
        })
        .take(designs)
        .collect()
}

/// Seeds a plan designs, and the starting designs it carries to each
/// requested duration.
const PLAN_SEEDS: usize = 8;
const PLAN_STARTS: usize = 1;
const PLAN_NEAREST: usize = 3;

/// Continuation steps along duration, s.
const PLAN_STEP: f64 = 0.25 * SECONDS_PER_DAY;

/// How far inside a station reference's span a plan keeps the ship's
/// times, s.
const PLAN_MARGIN: f64 = 0.25 * SECONDS_PER_DAY;

/// Designs the leg from `from` to `to` (in `field`, whose origin is Earth),
/// leaving within `window` (s after `epoch`), at each of `durations` (s).
/// Seeds come from the playbook for the kinds of end: a fan from a station
/// to a periapsis near a low orbit (via a lunar flyby too when the low
/// orbit is about Earth), closest approaches to a station, or a fan between
/// two low orbits. The cheapest are designed at their own durations, then
/// carried by continuation to each requested one (the cheapest, and those
/// nearest each duration); for each, the cheapest design reached. Designs
/// leave within the window. Small bodies and escapes are not cislunar: none
/// for them.
pub fn plan(
    field: &Field,
    light: &Field,
    epoch: Epoch,
    from: End,
    to: End,
    window: (f64, f64),
    durations: &[f64],
) -> Vec<Option<Design>> {
    let shortest = durations.iter().copied().fold(f64::INFINITY, f64::min);
    let longest = durations.iter().copied().fold(0.0, f64::max);
    // Seeds within a few days of the durations wanted: continuation from
    // farther ones is long, and each step a design.
    let slack = (0.25 * longest).max(SECONDS_PER_DAY);
    let seed_durations = ((shortest - slack).max(0.0), longest + slack);
    // A station's reference bounds when the ship can be there (s after
    // `epoch`), with a margin: held at its very first moment, Newton's
    // steps leave it.
    let covered = |end: &End| -> (f64, f64) {
        match end {
            End::Orbit(r) => {
                let (lo, hi) = r.path.span();
                let shift = (r.epoch.0 - epoch.0) * SECONDS_PER_DAY;
                (lo + shift + PLAN_MARGIN, hi + shift - PLAN_MARGIN)
            }
            _ => (f64::NEG_INFINITY, f64::INFINITY),
        }
    };
    let (from_lo, from_hi) = covered(&from);
    let (to_lo, to_hi) = covered(&to);
    let window = (
        window.0.max(from_lo).max(to_lo - shortest),
        window.1.min(from_hi).min(to_hi - longest),
    );
    if window.0 > window.1 {
        return vec![None; durations.len()];
    }
    let arrivals = (window.0 + shortest, window.1 + longest);
    // Times over the window (or where arrivals can fall), in a reference's
    // own seconds.
    let samples = |reference_epoch: Epoch, (lo, hi): (f64, f64)| -> Vec<f64> {
        let shift = (epoch.0 - reference_epoch.0) * SECONDS_PER_DAY;
        (0..32)
            .map(|k| lo + (hi - lo) * k as f64 / 31.0 + shift)
            .collect()
    };
    let center = field.center();
    let search = |times: Vec<f64>, flyby: bool| Search {
        times,
        sizes: vec![0.05, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.8],
        flyby_sizes: if flyby {
            vec![0.0, 0.1, 0.2, 0.3]
        } else {
            vec![]
        },
        directions: 30,
        flyby_directions: 12,
        durations: seed_durations,
        designs: 2 * PLAN_SEEDS,
    };
    let seeded: Vec<Seed> = match (&from, &to) {
        // Ends between planetary systems are `interplanetary`'s.
        (End::Body(_) | End::Excess { .. }, _) | (_, End::Body(_) | End::Excess { .. }) => {
            return vec![None; durations.len()];
        }
        (End::Orbit(r), End::Low(l)) => {
            let s = search(samples(r.epoch, window), Some(l.body) == center);
            seeds(field, light, r, l, Direction::Down, &s)
        }
        (End::Low(l), End::Orbit(r)) => {
            let s = search(samples(r.epoch, arrivals), Some(l.body) == center);
            seeds(field, light, r, l, Direction::Up, &s)
        }
        (End::Orbit(a), End::Orbit(b)) => {
            let s = search(samples(a.epoch, window), false);
            rendezvous_seeds(field, light, a, b, &s, 20_000.0)
        }
        (End::Low(a), End::Low(b)) => {
            // The far orbit is the one not about the field's origin.
            let (far, near, direction, times) = if Some(a.body) == center {
                (b, a, Direction::Up, samples(epoch, arrivals))
            } else {
                (a, b, Direction::Down, samples(epoch, window))
            };
            low_seeds(
                field,
                light,
                epoch,
                far,
                near,
                direction,
                &times,
                &[0.7, 0.8, 0.9, 1.0],
                100,
                seed_durations,
                2 * PLAN_SEEDS,
            )
        }
    };
    // Seeds leaving outside the window (an arrival fan's can) would only
    // be held at its edge: twice as many are drawn, and the first inside
    // kept.
    let seeded: Vec<Seed> = seeded
        .into_iter()
        .filter(|seed| {
            let leaves = seed.path.times[0] + (seed.path.epoch.0 - epoch.0) * SECONDS_PER_DAY;
            leaves >= window.0 && leaves <= window.1
        })
        .take(PLAN_SEEDS)
        .collect();
    // Seeds share an epoch, which the leg's times count from.
    let seed_epoch = seeded.first().map_or(epoch, |s| s.path.epoch);
    let shift = (epoch.0 - seed_epoch.0) * SECONDS_PER_DAY;
    let leg = Leg {
        field,
        locals: &[],
        from,
        to,
        depart: None,
        arrive: None,
        duration: None,
        window: Some((window.0 + shift, window.1 + shift)),
    };
    // Each seed designed at its own duration (with the duration free, the
    // search crawls along a valley where Δv falls by m/s per day).
    let mut starts: Vec<Design> = seeded
        .par_iter()
        .filter_map(|seed| {
            let own = seed.path.times.last().unwrap() - seed.path.times[0];
            Leg {
                duration: Some(own),
                ..leg
            }
            .design(&seed.path)
            .ok()
        })
        .collect();
    // The cheapest, and for each requested duration the nearest.
    starts.sort_by(|a, b| a.dv.total_cmp(&b.dv));
    let mut chosen: Vec<usize> = (0..starts.len().min(PLAN_STARTS)).collect();
    for &target in durations {
        let mut by_nearness: Vec<usize> = (0..starts.len()).collect();
        by_nearness.sort_by(|&a, &b| {
            (starts[a].duration() - target)
                .abs()
                .total_cmp(&(starts[b].duration() - target).abs())
        });
        chosen.extend(by_nearness.into_iter().take(PLAN_NEAREST));
    }
    chosen.sort();
    chosen.dedup();
    let carried: Vec<Vec<Option<Design>>> = chosen
        .par_iter()
        .map(|&i| leg.frontier(&starts[i], durations, PLAN_STEP))
        .collect();
    (0..durations.len())
        .map(|i| {
            carried
                .iter()
                .filter_map(|c| c[i].clone())
                .min_by(|a, b| a.dv.total_cmp(&b.dv))
        })
        .collect()
}
