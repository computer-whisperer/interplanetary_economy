//! Paths sampled for drawing (M1c.md, Mission plans): states with their
//! velocities, spaced by the local dynamics, between which cubic Hermite
//! interpolation recovers the path. A stay keeps less (M1b.md, step 2
//! plan, part 6): in a low orbit, states a few hours apart from its one
//! integration, from which its samples are integrated when asked; at a
//! station, a window of the vessel's orbit.

use crate::ephem::BodyId;
use crate::gravity::Field;
use crate::integrate::{Dop853, Tolerance, Trajectory};
use crate::reference::Reference;
use crate::shooting::hardest;
use crate::time::{Epoch, SECONDS_PER_DAY};
use glam::DVec3;
use std::fmt;
use std::sync::Arc;

/// Samples are this far apart in the angle of the motion about the body
/// pulling hardest, radians: 2°, where Hermite interpolation of a circular
/// orbit is off by r·θ⁴/384, 2.6 cm in LEO.
const ANGLE: f64 = 2.0 * std::f64::consts::PI / 180.0;

/// And at most this far apart in time, s: a few a day in cruise.
const LONGEST_S: f64 = 6.0 * 3_600.0;

/// A coast keeps its states at whole multiples of this many days since
/// J2000 (00:00, 06:00, 12:00 and 18:00): a day's hash reads one, and a
/// sample made between them integrates at most this long (under a
/// millisecond in LEO).
pub const KEPT_DAYS: f64 = 0.25;

/// A motion relative to `center` (the solar-system barycenter if `None`),
/// km and km/s, at times in seconds after `epoch`.
#[derive(Clone, Debug)]
pub struct Track {
    pub center: Option<BodyId>,
    pub epoch: Epoch,
    /// Where it coasts in a closed orbit (a stay), the orbit's period, s:
    /// a revolution about a moment shows where the ship is.
    pub revolution: Option<f64>,
    motion: Motion,
}

#[derive(Clone)]
enum Motion {
    /// Samples, non-decreasing in time. A time appears twice where the
    /// velocity jumps (a burn), or the position (where two motions meet).
    Samples {
        times: Vec<f64>,
        states: Vec<[f64; 6]>,
    },
    /// Coasting in `field`: states kept from one integration, its first
    /// and last among them, in time order; between two, the motion is the
    /// earlier integrated to the later.
    Coast {
        field: Arc<Field<'static>>,
        tol: Tolerance,
        kept: Vec<(f64, [f64; 6])>,
    },
    /// Following `reference` over `span` (its samples spaced in `field`).
    Reference {
        field: Arc<Field<'static>>,
        reference: Arc<Reference>,
        span: (f64, f64),
    },
}

impl fmt::Debug for Motion {
    /// What determines the motion, to the bit (but a field's
    /// description): a reference's path is integrated from its patch
    /// points.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Motion::Samples { times, states } => f
                .debug_struct("Samples")
                .field("times", times)
                .field("states", states)
                .finish(),
            Motion::Coast { tol, kept, .. } => f
                .debug_struct("Coast")
                .field("tol", tol)
                .field("kept", kept)
                .finish(),
            Motion::Reference {
                reference, span, ..
            } => f
                .debug_struct("Reference")
                .field("span", span)
                .field("epoch", &reference.epoch)
                .field("revolutions", &reference.revolutions)
                .field("patches", &reference.patches)
                .finish(),
        }
    }
}

impl Track {
    /// No samples yet, to [`Track::extend`].
    pub fn new(center: Option<BodyId>, epoch: Epoch) -> Track {
        Track {
            center,
            epoch,
            revolution: None,
            motion: Motion::Samples {
                times: Vec::new(),
                states: Vec::new(),
            },
        }
    }

    /// Resting at `center` (on or at a body) for `span` s.
    pub fn still(center: BodyId, epoch: Epoch, span: f64) -> Track {
        Track {
            center: Some(center),
            epoch,
            revolution: None,
            motion: Motion::Samples {
                times: vec![0.0, span],
                states: vec![[0.0; 6]; 2],
            },
        }
    }

    /// Coasting in `field` (integrated to `tol`), from the states `kept`
    /// of one integration (s after `epoch`, in time order, its first and
    /// last among them).
    pub fn coast(
        field: Arc<Field<'static>>,
        tol: Tolerance,
        epoch: Epoch,
        kept: Vec<(f64, [f64; 6])>,
    ) -> Track {
        assert!(
            !kept.is_empty() && kept.windows(2).all(|w| w[0].0 < w[1].0),
            "kept states run forward"
        );
        Track {
            center: field.center(),
            epoch,
            revolution: None,
            motion: Motion::Coast { field, tol, kept },
        }
    }

    /// Following `reference` from `epoch` for `span` s.
    pub fn following(
        field: Arc<Field<'static>>,
        reference: Arc<Reference>,
        epoch: Epoch,
        span: f64,
    ) -> Track {
        assert_eq!(
            field.center(),
            reference.center,
            "the field's origin is the reference's"
        );
        Track {
            center: reference.center,
            epoch,
            revolution: Some(reference.period_s()),
            motion: Motion::Reference {
                field,
                reference,
                span: (0.0, span),
            },
        }
    }

    /// The times kept for a coast from `epoch` for `span` s: its start,
    /// each whole multiple of [`KEPT_DAYS`] within it, and its end.
    pub fn kept_times(epoch: Epoch, span: f64) -> Vec<f64> {
        let mut times = vec![0.0];
        let mut m = (epoch.0 / KEPT_DAYS).floor() + 1.0;
        loop {
            let t = (m * KEPT_DAYS - epoch.0) * SECONDS_PER_DAY;
            if t >= span {
                break;
            }
            if t > 0.0 {
                times.push(t);
            }
            m += 1.0;
        }
        if span > 0.0 {
            times.push(span);
        }
        times
    }

    /// Appends samples of `state`, a continuous motion relative to
    /// `field`'s origin (the track's center), from `t0` to `t1` (s after
    /// the track's epoch): both ends, and between them every [`ANGLE`] of
    /// the motion about the body pulling hardest, at most [`LONGEST_S`]
    /// apart. Only to a track of samples.
    pub fn extend(&mut self, field: &Field, t0: f64, t1: f64, state: impl Fn(f64) -> [f64; 6]) {
        assert_eq!(
            field.center(),
            self.center,
            "the field's origin is the track's"
        );
        let Motion::Samples { times, states } = &mut self.motion else {
            panic!("samples extend a track of samples");
        };
        assert!(
            times.last().is_none_or(|&t| t <= t0) && t0 <= t1,
            "samples run forward"
        );
        for (t, s) in sampled(field, self.epoch, t0, t1, state) {
            times.push(t);
            states.push(s);
        }
    }

    /// Whether its samples are made when asked (a stay's), rather than
    /// held.
    pub fn makes_samples(&self) -> bool {
        !matches!(self.motion, Motion::Samples { .. })
    }

    /// Its samples from `t0` to `t1` ([`Track::samples`]) as a track of
    /// samples, its revolution kept.
    pub fn part(&self, t0: f64, t1: f64) -> Track {
        let (times, states) = self.samples(t0, t1).into_iter().unzip();
        Track {
            center: self.center,
            epoch: self.epoch,
            revolution: self.revolution,
            motion: Motion::Samples { times, states },
        }
    }

    /// Whether the motion is the center's own: every state zero, as for a
    /// ship resting on or at a body.
    pub fn rests(&self) -> bool {
        match &self.motion {
            Motion::Samples { states, .. } => {
                self.center.is_some() && states.iter().all(|s| s.iter().all(|&x| x == 0.0))
            }
            _ => false,
        }
    }

    /// First and last times.
    pub fn span(&self) -> Option<(f64, f64)> {
        match &self.motion {
            Motion::Samples { times, .. } => Some((*times.first()?, *times.last()?)),
            Motion::Coast { kept, .. } => Some((kept.first()?.0, kept.last()?.0)),
            Motion::Reference { span, .. } => Some(*span),
        }
    }

    /// The first state, and its time.
    pub fn first(&self) -> Option<(f64, [f64; 6])> {
        let (t, _) = self.span()?;
        Some((t, self.at(t)?))
    }

    /// The last state, and its time.
    pub fn last(&self) -> Option<(f64, [f64; 6])> {
        let (_, t) = self.span()?;
        Some((t, self.at(t)?))
    }

    /// What the track holds, to the bit: its samples, a coast's kept
    /// states, or its reference's patch points (its path is integrated
    /// from them).
    pub fn held(&self) -> Vec<(f64, [f64; 6])> {
        match &self.motion {
            Motion::Samples { times, states } => {
                times.iter().copied().zip(states.iter().copied()).collect()
            }
            Motion::Coast { kept, .. } => kept.clone(),
            Motion::Reference { reference, .. } => reference.patches.clone(),
        }
    }

    /// The state at `t` (s after the epoch): between samples, by cubic
    /// Hermite interpolation (the later of two at one time); on a coast,
    /// the kept state before it integrated to the next, read at `t`; on a
    /// reference, the reference's. `None` outside the span.
    pub fn at(&self, t: f64) -> Option<[f64; 6]> {
        let (first, last) = self.span()?;
        if t < first || t > last {
            return None;
        }
        match &self.motion {
            Motion::Samples { times, states } => Some(hermite(times, states, t)),
            Motion::Coast { field, tol, kept } => {
                let k = kept.partition_point(|(s, _)| *s <= t) - 1;
                if kept[k].0 == t || k + 1 == kept.len() {
                    return Some(kept[k].1);
                }
                let leg = segment(field, *tol, self.epoch, kept[k], kept[k + 1].0)?;
                Some(leg.at(t - kept[k].0))
            }
            Motion::Reference { reference, .. } => {
                Some(reference.state(self.epoch.plus_days(t / SECONDS_PER_DAY)))
            }
        }
    }

    /// Samples from `t0` to `t1` (s after the epoch, within the span),
    /// every [`ANGLE`] as [`Track::extend`] spaces them: a track of
    /// samples', those in the span asked and one each side; a coast's or a
    /// reference's, made, its ends among them.
    pub fn samples(&self, t0: f64, t1: f64) -> Vec<(f64, [f64; 6])> {
        let Some((first, last)) = self.span() else {
            return Vec::new();
        };
        let (t0, t1) = (t0.max(first), t1.min(last));
        if t0 > t1 {
            return Vec::new();
        }
        match &self.motion {
            Motion::Samples { times, states } => {
                let a = times.partition_point(|&s| s < t0).saturating_sub(1);
                let b = (times.partition_point(|&s| s <= t1) + 1).min(times.len());
                times[a..b]
                    .iter()
                    .copied()
                    .zip(states[a..b].iter().copied())
                    .collect()
            }
            Motion::Coast { field, tol, kept } => {
                let from = kept.partition_point(|(s, _)| *s <= t0) - 1;
                if from + 1 == kept.len() {
                    return vec![kept[from]];
                }
                let mut out = Vec::new();
                for k in from..kept.len() - 1 {
                    let ((a, start), (b, end)) = (kept[k], kept[k + 1]);
                    if a > t1 || (a == t1 && !out.is_empty()) {
                        break;
                    }
                    let Some(leg) = segment(field, *tol, self.epoch, (a, start), b) else {
                        break;
                    };
                    // Its ends are the states kept there, as `at` has them.
                    let (lo, hi) = (t0.max(a), t1.min(b));
                    let mut made = sampled(field, self.epoch, lo, hi, |t| match t {
                        t if t == a => start,
                        t if t == b => end,
                        t => leg.at(t - a),
                    });
                    // The next begins where this one ends.
                    if hi == b && b < t1 {
                        made.pop();
                    }
                    out.extend(made);
                }
                out
            }
            Motion::Reference {
                field, reference, ..
            } => sampled(field, self.epoch, t0, t1, |t| {
                reference.state(self.epoch.plus_days(t / SECONDS_PER_DAY))
            }),
        }
    }
}

/// The coast from `(t, s)` (s after `epoch`) to `end`, integrated in
/// `field`; `None` if it cannot be.
fn segment(
    field: &Field,
    tol: Tolerance,
    epoch: Epoch,
    (t, s): (f64, [f64; 6]),
    end: f64,
) -> Option<Trajectory<6>> {
    let ode = field.ode(epoch.plus_days(t / SECONDS_PER_DAY));
    let mut run = Dop853::new(&ode, 0.0, s, tol);
    let mut steps = Vec::new();
    run.advance_dense(&ode, end - t, &mut steps).ok()?;
    Some(Trajectory::new(steps))
}

/// Samples of `state`, a continuous motion relative to `field`'s origin,
/// from `t0` to `t1` (s after `epoch`): both ends, and between them every
/// [`ANGLE`] of the motion about the body pulling hardest, at most
/// [`LONGEST_S`] apart.
fn sampled(
    field: &Field,
    epoch: Epoch,
    t0: f64,
    t1: f64,
    state: impl Fn(f64) -> [f64; 6],
) -> Vec<(f64, [f64; 6])> {
    let mut out = Vec::new();
    let mut t = t0;
    let mut s = state(t);
    loop {
        out.push((t, s));
        if t >= t1 {
            return out;
        }
        let dt = spacing(field, epoch, t, &s);
        // The last step is split evenly with the one before rather than
        // left a sliver.
        t = if t + dt >= t1 {
            t1
        } else if t + 1.5 * dt >= t1 {
            t + 0.5 * (t1 - t)
        } else {
            t + dt
        };
        s = state(t);
    }
}

/// The time to the next sample after state `s` at `t` (s after `epoch`):
/// [`ANGLE`] over the rate the motion turns about the body pulling hardest
/// (its speed relative to it, or the circular speed there if faster, over
/// the distance).
fn spacing(field: &Field, epoch: Epoch, t: f64, s: &[f64; 6]) -> f64 {
    let when = Epoch(epoch.0 + t / SECONDS_PER_DAY);
    let r = DVec3::new(s[0], s[1], s[2]);
    let (b, d) = hardest(field, when, r);
    let v = (DVec3::new(s[3], s[4], s[5]) - field.body_state(b, when).v).length();
    let circular = (field.ephemeris().body(b).gm / d).sqrt();
    (ANGLE * d / v.max(circular)).min(LONGEST_S)
}

/// The state at `t` between samples, by cubic Hermite interpolation (the
/// later of two at one time).
fn hermite(times: &[f64], states: &[[f64; 6]], t: f64) -> [f64; 6] {
    let b = times.partition_point(|&s| s <= t);
    if b == times.len() {
        return *states.last().expect("a span has samples");
    }
    let a = b - 1;
    let h = times[b] - times[a];
    let u = (t - times[a]) / h;
    let (pa, va) = split(&states[a]);
    let (pb, vb) = split(&states[b]);
    let (u2, u3) = (u * u, u * u * u);
    let p = (2.0 * u3 - 3.0 * u2 + 1.0) * pa
        + (u3 - 2.0 * u2 + u) * h * va
        + (-2.0 * u3 + 3.0 * u2) * pb
        + (u3 - u2) * h * vb;
    let v = (6.0 * u2 - 6.0 * u) / h * pa
        + (3.0 * u2 - 4.0 * u + 1.0) * va
        + (-6.0 * u2 + 6.0 * u) / h * pb
        + (3.0 * u2 - 2.0 * u) * vb;
    [p.x, p.y, p.z, v.x, v.y, v.z]
}

fn split(s: &[f64; 6]) -> (DVec3, DVec3) {
    (DVec3::new(s[0], s[1], s[2]), DVec3::new(s[3], s[4], s[5]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ephem::Ephemeris;
    use crate::integrate::{Dop853, Tolerance, Trajectory};

    /// A LEO arc sampled about once a minute, interpolated between samples
    /// within a few centimeters of the integration.
    #[test]
    fn leo_between_samples() {
        let eph = Ephemeris::standard();
        let earth = eph.lookup("earth").unwrap();
        let field = Field::new(eph, Some(earth), &[eph.sun(), earth]).with_oblateness();
        let epoch = Epoch::from_ymd(2045, 3, 1);
        let ode = field.ode(epoch);
        let (r, v) = (6_778.0, (eph.body(earth).gm / 6_778.0).sqrt());
        let tol = Tolerance {
            rtol: 1e-12,
            atol: 1e-9,
        };
        let mut run = Dop853::new(&ode, 0.0, [r, 0.0, 0.0, 0.0, 0.6 * v, 0.8 * v], tol);
        let mut steps = Vec::new();
        run.advance_dense(&ode, 6.0 * 3_600.0, &mut steps).unwrap();
        let truth = Trajectory::new(steps);
        let mut track = Track::new(Some(earth), epoch);
        track.extend(&field, 0.0, 6.0 * 3_600.0, |t| truth.at(t));
        let held = track.held();
        let gaps: Vec<f64> = held.windows(2).map(|w| w[1].0 - w[0].0).collect();
        let median = {
            let mut g = gaps.clone();
            g.sort_by(f64::total_cmp);
            g[g.len() / 2]
        };
        assert!((25.0..40.0).contains(&median), "{median} s between samples");
        let worst = (0..2_000)
            .map(|k| {
                let t = 6.0 * 3_600.0 * k as f64 / 2_000.0;
                let (a, b) = (track.at(t).unwrap(), truth.at(t));
                (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
            })
            .fold(0.0, f64::max);
        assert!(worst < 1e-4, "{worst} km off between samples");
        assert!(track.at(-1.0).is_none());
    }

    /// A coast in LEO over three days, from 03:17: its states kept every
    /// 6 h from one integration, and the state at any time integrated from
    /// the kept state before it, within a millimeter of that integration's
    /// (M1b.md, step 2 plan, part 6); its samples made over a span are
    /// those states, 2° apart, their ends at the span's.
    #[test]
    fn a_coast_keeps_states_and_integrates_between() {
        let eph = Ephemeris::standard();
        let earth = eph.lookup("earth").unwrap();
        let moon = eph.lookup("moon").unwrap();
        let field =
            Arc::new(Field::new(eph, Some(earth), &[eph.sun(), earth, moon]).with_oblateness());
        let epoch = Epoch(Epoch::from_ymd(2045, 3, 1).0 + 197.0 / 1_440.0);
        let ode = field.ode(epoch);
        let (r, v) = (6_778.0, (eph.body(earth).gm / 6_778.0).sqrt());
        let tol = Tolerance {
            rtol: 1e-12,
            atol: 1e-9,
        };
        let span = 3.0 * SECONDS_PER_DAY;
        let mut run = Dop853::new(&ode, 0.0, [r, 0.0, 0.0, 0.0, 0.6 * v, 0.8 * v], tol);
        let mut steps = Vec::new();
        run.advance_dense(&ode, span, &mut steps).unwrap();
        let truth = Trajectory::new(steps);
        let times = Track::kept_times(epoch, span);
        assert_eq!(times.len(), 2 + 12, "{times:?}");
        let midnight = (Epoch::from_ymd(2045, 3, 2).0 - epoch.0) * SECONDS_PER_DAY;
        assert!(times.contains(&midnight));
        let kept: Vec<_> = times.iter().map(|&t| (t, truth.at(t))).collect();
        let track = Track::coast(field.clone(), tol, epoch, kept.clone());
        assert_eq!(track.span(), Some((0.0, span)));
        for (t, s) in &kept {
            assert_eq!(track.at(*t), Some(*s));
        }
        let worst = (0..3_000)
            .map(|k| {
                let t = span * k as f64 / 3_000.0;
                let (a, b) = (track.at(t).unwrap(), truth.at(t));
                (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt()
            })
            .fold(0.0, f64::max);
        println!("{:.3} mm off the one integration at worst", worst * 1e6);
        assert!(worst < 1e-6, "{worst} km off the one integration");
        // Over the whole span, and over a window across a kept state.
        for (t0, t1) in [
            (0.0, span),
            (midnight - 3_000.0, midnight + 3_000.0),
            (midnight - 3_000.0, midnight),
            (midnight, midnight + 3_000.0),
            (midnight, midnight),
        ] {
            let made = track.samples(t0, t1);
            assert_eq!((made[0].0, made.last().unwrap().0), (t0, t1));
            assert!(
                made.windows(2).all(|w| w[0].0 < w[1].0),
                "{t0}..{t1}: no time twice (found in review: a window ending at a kept state had it twice)"
            );
            if t0 == t1 {
                assert_eq!(made.len(), 1);
                continue;
            }
            for (t, s) in made.iter().step_by(37).chain(made.last()) {
                assert_eq!(track.at(*t).as_ref(), Some(s), "{t}");
            }
            let gap = made[1].0 - made[0].0;
            assert!((20.0..40.0).contains(&gap), "{gap} s apart");
        }
        assert!(track.at(span + 1.0).is_none());
    }
}
