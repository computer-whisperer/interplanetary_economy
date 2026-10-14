//! Transfers between a station's orbit and a low circular orbit (M1a.md,
//! Autopilot, playbook entries 2 and 5): from LEO to a Lagrange-point orbit
//! and back, from those orbits down to low lunar orbit and back.
//!
//! Seeds come from fans: trajectories leaving the station's orbit (or,
//! integrated backward, reaching it) with burns of several sizes in many
//! directions, each followed to its first periapsis about a body. A direct
//! seed is one whose periapsis about the low orbit's body comes near the
//! low orbit; a flyby seed passes the Moon first, where a second fan (with
//! a burn there, possibly none) continues to the low orbit. Seeds are
//! designed in the full field ([`crate::shooting`]), which makes them meet
//! both ends exactly and minimizes their total Δv.

use crate::ephem::BodyId;
use crate::gravity::Field;
use crate::integrate::{DenseStep, Dop853, Tolerance};
use crate::reference::Reference;
use crate::shooting::{Design, End, Failure, Leg, LowOrbit, Pass, Path};
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
    /// Longest leg from the station's orbit to a periapsis, s.
    pub span: f64,
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
        span: search.span,
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
                span: search.span,
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
    chains.sort_by(|a, b| a.1.total_cmp(&b.1));
    chains
        .into_iter()
        .filter_map(|(links, estimate)| {
            Some(Seed {
                path: chain_path(field, epoch, direction, &links)?,
                estimate,
            })
        })
        .take(search.designs)
        .collect()
}

/// A fan chain flown again in the design field and split into arcs of at
/// most [`ARC_S`], as a path in time order: a burn node where each link
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
        let arcs = (((link.end - link.at).abs() / ARC_S).ceil() as usize).max(2);
        let mut run = Dop853::new(&ode, link.at, flown, TOL);
        let mut states = Vec::with_capacity(arcs);
        for j in 1..=arcs {
            let t = link.at + (link.end - link.at) * j as f64 / arcs as f64;
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

/// The leg between `reference` and `low`, with free times.
pub fn leg<'a>(
    field: &'a Field<'a>,
    reference: &'a Reference,
    low: LowOrbit,
    direction: Direction,
) -> Leg<'a> {
    let (from, to) = match direction {
        Direction::Down => (End::Orbit(reference), End::Low(low)),
        Direction::Up => (End::Low(low), End::Orbit(reference)),
    };
    Leg {
        field,
        from,
        to,
        depart: None,
        arrive: None,
        duration: None,
    }
}

/// Designs a search's cheapest seeds in parallel: the designs that
/// succeed, cheapest first, and the failures.
pub fn search(
    field: &Field,
    light: &Field,
    reference: &Reference,
    low: LowOrbit,
    direction: Direction,
    search: &Search,
    duration: Option<f64>,
) -> (Vec<Design>, Vec<Failure>) {
    let mut leg = leg(field, reference, low, direction);
    leg.duration = duration;
    let seeds = seeds(field, light, reference, &low, direction, search);
    let results: Vec<Result<Design, Failure>> =
        seeds.par_iter().map(|s| leg.design(&s.path)).collect();
    let (mut designs, mut failures) = (Vec::new(), Vec::new());
    for r in results {
        match r {
            Ok(d) => designs.push(d),
            Err(e) => failures.push(e),
        }
    }
    designs.sort_by(|a, b| a.dv.total_cmp(&b.dv));
    (designs, failures)
}

/// The design's duration in days.
pub fn days(design: &Design) -> f64 {
    design.duration() / SECONDS_PER_DAY
}
