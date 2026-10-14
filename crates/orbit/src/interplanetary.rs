//! Legs between planetary systems (M1a.md, Autopilot, playbook entry 1): a
//! low orbit about one planet to a low orbit about another, to a small
//! body, or out to an escape with a given excess velocity.
//!
//! Seeds come from patched conics: the heliocentric Lambert arc between the
//! two systems ([`crate::lambert`], as [`crate::transfer`] solves it), and
//! at each planet a hyperbola with that arc's excess velocity there and its
//! periapsis at the low orbit. They are designed in a barycentric field
//! whose arcs near a planet integrate about it ([`crate::shooting`]), which
//! closes the patches and finds the planes of the hyperbolas.

use crate::consts::MU_SUN;
use crate::ephem::BodyId;
use crate::gravity::Field;
use crate::kepler::{self, State};
use crate::lambert;
use crate::linalg;
use crate::shooting::{Design, End, Leg, Path};
use crate::time::{Epoch, SECONDS_PER_DAY};
use glam::DVec3;
use rayon::prelude::*;

/// How long the seed follows each planet's hyperbola at least, s; the
/// Lambert arc carries it between.
const NEAR_PLANET_S: f64 = 3.0 * SECONDS_PER_DAY;

/// Newton steps patching the hyperbolas to the heliocentric arc.
const PATCHES: usize = 8;

/// Longest cruise arc between nodes, s.
const CRUISE_ARC_S: f64 = 10.0 * SECONDS_PER_DAY;

/// Nodes this long after leaving a planet (and before reaching one), s,
/// where the path is most sensitive.
const NEAR_NODES_S: [f64; 3] = [3_600.0, 4.0 * 3_600.0, SECONDS_PER_DAY];

/// A design costing more than this times the leg's patched conics
/// ([`patched`]) has left the seed's family of paths, and is rejected.
/// Seen in M1a's gate sweep: LEO to near-Earth asteroids at ~110 km/s, ~52
/// km/s at each end, from seeds whose patching diverged (departures below
/// ~1 km/s of excess speed, to excess speeds up to 115 km/s). Designs that
/// keep their family cost 0.85–1.9× patched conics.
const BEYOND_PATCHED: f64 = 3.0;

/// The periapsis state, relative to a body of `gm`, of a hyperbola with
/// periapsis radius `rp` and excess velocity `v_inf`: leaving the body
/// (`outgoing`) or arriving at it. Its plane contains `v_inf`; `turn`
/// (radians) picks which plane, turning about `v_inf` from the one that
/// contains the ecliptic pole.
pub fn periapsis(gm: f64, rp: f64, v_inf: DVec3, turn: f64, outgoing: bool) -> State {
    let speed = v_inf.length();
    let u = v_inf / speed;
    let ecc = 1.0 + rp * speed * speed / gm;
    // The asymptote is at true anomaly ±acos(-1/e).
    let (cos, sin) = (-1.0 / ecc, (1.0 - 1.0 / (ecc * ecc)).sqrt());
    let a = (DVec3::Z - DVec3::Z.dot(u) * u).normalize_or(u.any_orthonormal_vector());
    let normal = turn.cos() * a + turn.sin() * u.cross(a);
    // Outgoing, the asymptote is cos p + sin q (q = n × p); arriving, the
    // velocity far out is -(cos p - sin q). Solving each for p:
    let w = normal.cross(u);
    let p = if outgoing {
        cos * u - sin * w
    } else {
        -cos * u - sin * w
    };
    let q = normal.cross(p);
    State {
        r: rp * p,
        v: (speed * speed + 2.0 * gm / rp).sqrt() * q,
    }
}

/// What a seed's ends are: a low orbit about a planet, or a pass through
/// its atmosphere (`body`, a periapsis radius), a small body to meet, or an
/// escape with an excess velocity (leaving the planet the leg starts at, or
/// arriving at the one it ends at).
fn kind(end: &End) -> Option<(BodyId, Option<f64>)> {
    match end {
        End::Low(low) => Some((low.body, Some(low.radius))),
        End::Atmosphere { body, radius, .. } => Some((*body, Some(*radius))),
        End::Body(b) => Some((*b, None)),
        End::Excess { body, .. } => Some((*body, None)),
        End::Orbit(_) | End::Hyperbola { .. } => None,
    }
}

/// The leg's Δv by patched conics: the heliocentric Lambert arc between the
/// two systems (the escape's or capture's excess velocity instead, at both
/// ends, for a leg with one), and at each end the burn between it and the
/// excess velocity there: a low orbit's at periapsis, the propulsive part
/// of a pass through an atmosphere, a small body's excess speed, none at an
/// escape or capture. `None` where the ends are not of those kinds or the
/// Lambert problem has no solution.
pub fn patched(field: &Field, from: &End, to: &End, depart: Epoch, arrive: Epoch) -> Option<f64> {
    let eph = field.ephemeris();
    let excess = |end: &End| match end {
        End::Excess { v_inf, .. } => Some(v_inf.length()),
        _ => None,
    };
    let (v_a, v_b) = match (excess(from), excess(to)) {
        (Some(v), _) | (_, Some(v)) => (v, v),
        _ => {
            let (a, b) = (kind(from)?.0, kind(to)?.0);
            let (sa, sb) = (eph.system_state(a, depart), eph.system_state(b, arrive));
            let span = (arrive.0 - depart.0) * SECONDS_PER_DAY;
            let arc = lambert::solve(MU_SUN, sa.r, sb.r, span)?;
            ((arc.v1 - sa.v).length(), (arc.v2 - sb.v).length())
        }
    };
    let burn = |end: &End, v: f64| match end {
        End::Low(low) => {
            let gm = eph.body(low.body).gm;
            Some((v * v + 2.0 * gm / low.radius).sqrt() - (gm / low.radius).sqrt())
        }
        End::Atmosphere { after, .. } => Some(*after),
        End::Body(_) => Some(v),
        End::Excess { .. } => Some(0.0),
        End::Orbit(_) | End::Hyperbola { .. } => None,
    };
    Some(burn(from, v_a)? + burn(to, v_b)?)
}

/// A patched-conic seed for a leg from `from` (a low orbit, a small body,
/// or an arrival with an excess velocity) leaving at `t0` to `to` (a low
/// orbit, a small body, or an escape) arriving at `t1` (s after `epoch`),
/// in `field`'s frame. The hyperbolas' planes turn by `turns` (radians, see
/// [`periapsis`]). `None` where the ends are not of those kinds or a conic
/// fails.
pub fn seed(
    field: &Field,
    epoch: Epoch,
    from: &End,
    to: &End,
    t0: f64,
    t1: f64,
    turns: (f64, f64),
) -> Option<Path> {
    let eph = field.ephemeris();
    let when = |t: f64| Epoch(epoch.0 + t / SECONDS_PER_DAY);
    let at = |b: BodyId, t: f64| field.body_state(b, when(t));
    let sun = eph.sun();
    let (a, a_radius) = kind(from)?;
    let (b, b_radius) = kind(to)?;
    // How long each hyperbola lasts: until the ship nears the planet's
    // sphere of influence (a slow departure lingers for weeks; patched to
    // the Sun's arc after 3 days, one left defects Newton could not close),
    // at least `NEAR_PLANET_S`, at most a third of the leg.
    let span = t1 - t0;
    let (sa, sb) = (eph.system_state(a, when(t0)), eph.system_state(b, when(t1)));
    let arc = lambert::solve(MU_SUN, sa.r, sb.r, span);
    let lingering = |body: BodyId, v_inf: Option<f64>, t: f64| -> f64 {
        let soi = eph.sphere_of_influence(body, when(t));
        let slow = v_inf.map_or(NEAR_PLANET_S, |v| soi / v);
        slow.max(NEAR_PLANET_S).min(span / 3.0)
    };
    let excess_at = |end: &End, lambert: Option<f64>| match end {
        End::Excess { v_inf, .. } => Some(v_inf.length()),
        _ => lambert,
    };
    let near_a = a_radius.map(|_| {
        let v = excess_at(to, arc.map(|x| (x.v1 - sa.v).length()));
        lingering(a, excess_at(from, v), t0)
    });
    let near_b = b_radius.map(|_| {
        let v = excess_at(from, arc.map(|x| (x.v2 - sb.v).length()));
        lingering(b, excess_at(to, v), t1)
    });
    // The node times: close to each planet, along each hyperbola, then
    // cruise.
    let mut offsets = vec![0.0, span];
    let lo = near_a.unwrap_or(0.0);
    let hi = span - near_b.unwrap_or(0.0);
    let spaced = |from: f64, to: f64| -> Vec<f64> {
        let n = ((to - from) / CRUISE_ARC_S).ceil().max(1.0) as usize;
        (1..n)
            .map(|k| from + (to - from) * k as f64 / n as f64)
            .collect()
    };
    if let Some(near) = near_a {
        offsets.extend(NEAR_NODES_S.iter().filter(|&&d| d < near));
        offsets.push(near);
        offsets.extend(spaced(NEAR_NODES_S[2].min(near), near));
    }
    if let Some(near) = near_b {
        offsets.extend(NEAR_NODES_S.iter().filter(|&&d| d < near).map(|d| span - d));
        offsets.push(span - near);
        offsets.extend(spaced(span - near, span - NEAR_NODES_S[2].min(near)));
    }
    offsets.extend(spaced(lo, hi));
    offsets.sort_by(|x, y| x.total_cmp(y));
    offsets.dedup_by(|x, y| (*x - *y).abs() < 600.0);
    let times: Vec<f64> = offsets.iter().map(|o| t0 + o).collect();

    let state = |s: State, origin: State| -> [f64; 6] {
        let (r, v) = (s.r + origin.r, s.v + origin.v);
        [r.x, r.y, r.z, v.x, v.y, v.z]
    };
    let mut states = Vec::with_capacity(times.len() - 1);
    // A hyperbola about `body` with periapsis at `t_p`: its state at `t`,
    // relative to the body.
    let hyperbola_at =
        |body: BodyId, rp: f64, v_inf: DVec3, turn: f64, outgoing: bool, t_p: f64, t: f64| {
            let gm = eph.body(body).gm;
            kepler::propagate(periapsis(gm, rp, v_inf, turn, outgoing), t - t_p, gm)
        };
    if let End::Excess { body, v_inf } = to {
        // An escape: the departure hyperbola all the way.
        assert_eq!(*body, a, "an escape is from the planet left");
        for &t in &times[..times.len() - 1] {
            let s = hyperbola_at(a, a_radius?, *v_inf, turns.0, true, t0, t)?;
            states.push(state(s, at(a, t)));
        }
    } else if let End::Excess { body, v_inf } = from {
        // A capture: the arrival hyperbola all the way.
        assert_eq!(*body, b, "a capture is into the planet reached");
        for &t in &times[..times.len() - 1] {
            let s = hyperbola_at(b, b_radius?, *v_inf, turns.1, false, t1, t)?;
            states.push(state(s, at(b, t)));
        }
    } else {
        // The hyperbolas, and the heliocentric arc between where they end:
        // patched there, not at the planets' centers, where a slow
        // departure's arc missed by over a million km after the weeks the
        // ship spends leaving.
        let arc = arc?;
        let (ta, tb) = (t0 + lo, t0 + hi);
        let helio = |body: BodyId, t: f64| eph.state(body, when(t));
        // The heliocentric states where the hyperbolas end (or the bodies,
        // without one), and the arc joining them.
        let patch = |v_a: DVec3, v_b: DVec3| -> Option<(State, State, lambert::LambertSolution)> {
            let start = match a_radius {
                Some(rp) => {
                    let s = hyperbola_at(a, rp, v_a, turns.0, true, t0, ta)?;
                    let h = helio(a, ta);
                    State {
                        r: s.r + h.r,
                        v: s.v + h.v,
                    }
                }
                None => helio(a, t0),
            };
            let end = match b_radius {
                Some(rp) => {
                    let s = hyperbola_at(b, rp, v_b, turns.1, false, t1, tb)?;
                    let h = helio(b, tb);
                    State {
                        r: s.r + h.r,
                        v: s.v + h.v,
                    }
                }
                None => helio(b, t1),
            };
            let joined = lambert::solve(MU_SUN, start.r, end.r, tb - ta)?;
            Some((start, end, joined))
        };
        // Newton's method on the excess velocities of the hyperbolas there
        // are, by differences: the velocities where they end must be the
        // arc's. (A plain update diverged: the arc's velocity is as
        // sensitive to where a slow departure's hyperbola ends as that is to
        // its excess velocity.)
        let unknowns: Vec<usize> = [a_radius.is_some(), b_radius.is_some()]
            .into_iter()
            .enumerate()
            .filter_map(|(i, on)| on.then_some(i))
            .collect();
        let mut v = [arc.v1 - sa.v, arc.v2 - sb.v];
        let mismatch = |v: &[DVec3; 2]| -> Option<Vec<f64>> {
            let (start, end, joined) = patch(v[0], v[1])?;
            let gaps = [joined.v1 - start.v, joined.v2 - end.v];
            Some(unknowns.iter().flat_map(|&i| gaps[i].to_array()).collect())
        };
        let n = 3 * unknowns.len();
        for _ in 0..PATCHES {
            let Some(f) = mismatch(&v) else { break };
            if f.iter().all(|x| x.abs() < 1e-6) {
                break;
            }
            let mut jac = vec![0.0; n * n];
            for c in 0..n {
                let mut w = v;
                let h = 1e-6;
                w[unknowns[c / 3]][c % 3] += h;
                let g = mismatch(&w)?;
                for r in 0..n {
                    jac[r * n + c] = (g[r] - f[r]) / h;
                }
            }
            let mut step: Vec<f64> = f.iter().map(|x| -x).collect();
            if linalg::solve(n, &mut jac, &mut step).is_none() {
                break;
            }
            for (c, d) in step.iter().enumerate() {
                v[unknowns[c / 3]][c % 3] += d;
            }
        }
        let (v_a, v_b) = (v[0], v[1]);
        let (start, _, joined) = patch(v_a, v_b)?;
        let cruise = State {
            r: start.r,
            v: joined.v1,
        };
        for &t in &times[..times.len() - 1] {
            let s = if a_radius.is_some() && t <= ta {
                state(
                    hyperbola_at(a, a_radius?, v_a, turns.0, true, t0, t)?,
                    at(a, t),
                )
            } else if b_radius.is_some() && t >= tb {
                state(
                    hyperbola_at(b, b_radius?, v_b, turns.1, false, t1, t)?,
                    at(b, t),
                )
            } else {
                state(kepler::propagate(cruise, t - ta, MU_SUN)?, at(sun, t))
            };
            states.push(s);
        }
    }
    Some(Path::coasting(epoch, times, states))
}

/// Designs a leg between planetary systems leaving at `depart` and arriving
/// at `arrive`, in `global` (a barycentric field) with arcs near a planet
/// integrated in `locals` (fields centered on the planets): the cheapest
/// design from [`seed`]s over the planes of both hyperbolas (each turned by
/// half a revolution or not; the turn picks which side of the planet the
/// periapsis lies on). `None` if none converges at under
/// [`BEYOND_PATCHED`] times the leg's patched conics.
pub fn plan(
    global: &Field,
    locals: &[Field],
    from: End,
    to: End,
    depart: Epoch,
    arrive: Epoch,
) -> Option<Design> {
    let t1 = (arrive.0 - depart.0) * SECONDS_PER_DAY;
    let leg = Leg {
        field: global,
        locals,
        from,
        to,
        depart: Some(0.0),
        arrive: Some(t1),
        duration: None,
        window: None,
    };
    let limit = BEYOND_PATCHED * patched(global, &from, &to, depart, arrive)?;
    let half = std::f64::consts::PI;
    [(0.0, 0.0), (0.0, half), (half, 0.0), (half, half)]
        .par_iter()
        .filter_map(|&turns| {
            leg.design(&seed(global, depart, &from, &to, 0.0, t1, turns)?)
                .ok()
        })
        .filter(|d| d.dv <= limit)
        .min_by(|a, b| a.dv.total_cmp(&b.dv))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shooting::excess_velocity;

    /// A hyperbola built from an excess velocity has that excess velocity,
    /// leaving and arriving, in any plane.
    #[test]
    fn periapsis_has_its_excess_velocity() {
        let gm = 398_600.4;
        let v_inf = DVec3::new(2.1, -1.4, 0.7);
        for turn in [0.0, 1.0, 2.5, 4.0] {
            for outgoing in [true, false] {
                let p = periapsis(gm, 6_778.0, v_inf, turn, outgoing);
                assert!(p.r.dot(p.v).abs() < 1e-9, "not a periapsis");
                assert!((p.r.length() - 6_778.0).abs() < 1e-9);
                let s = [p.r.x, p.r.y, p.r.z, p.v.x, p.v.y, p.v.z];
                let got = excess_velocity(gm, &s, outgoing).unwrap();
                assert!((got - v_inf).length() < 1e-9, "{got} vs {v_inf}");
            }
        }
    }
}
