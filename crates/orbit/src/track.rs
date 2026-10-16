//! Paths sampled for drawing (M1c.md, Mission plans): states with their
//! velocities, spaced by the local dynamics, between which cubic Hermite
//! interpolation recovers the path.

use crate::ephem::BodyId;
use crate::gravity::Field;
use crate::shooting::hardest;
use crate::time::{Epoch, SECONDS_PER_DAY};
use glam::DVec3;

/// Samples are this far apart in the angle of the motion about the body
/// pulling hardest, radians: 2°, where Hermite interpolation of a circular
/// orbit is off by r·θ⁴/384, 2.6 cm in LEO.
const ANGLE: f64 = 2.0 * std::f64::consts::PI / 180.0;

/// And at most this far apart in time, s: a few a day in cruise.
const LONGEST_S: f64 = 6.0 * 3_600.0;

/// A motion relative to `center` (the solar-system barycenter if `None`),
/// km and km/s, sampled at times in seconds after `epoch`.
#[derive(Clone, Debug)]
pub struct Track {
    pub center: Option<BodyId>,
    pub epoch: Epoch,
    /// Non-decreasing. A time appears twice where the velocity jumps (a
    /// burn), or the position (where two motions meet).
    pub times: Vec<f64>,
    pub states: Vec<[f64; 6]>,
    /// Where it coasts in a closed orbit (a stay), the orbit's period, s:
    /// a revolution about a moment shows where the ship is.
    pub revolution: Option<f64>,
}

impl Track {
    pub fn new(center: Option<BodyId>, epoch: Epoch) -> Track {
        Track {
            center,
            epoch,
            times: Vec::new(),
            states: Vec::new(),
            revolution: None,
        }
    }

    /// Appends samples of `state`, a continuous motion relative to
    /// `field`'s origin (the track's center), from `t0` to `t1` (s after
    /// the track's epoch): both ends, and between them every [`ANGLE`] of
    /// the motion about the body pulling hardest, at most [`LONGEST_S`]
    /// apart.
    pub fn extend(&mut self, field: &Field, t0: f64, t1: f64, state: impl Fn(f64) -> [f64; 6]) {
        assert_eq!(
            field.center(),
            self.center,
            "the field's origin is the track's"
        );
        assert!(
            self.times.last().is_none_or(|&t| t <= t0) && t0 <= t1,
            "samples run forward"
        );
        let mut t = t0;
        let mut s = state(t);
        loop {
            self.times.push(t);
            self.states.push(s);
            if t >= t1 {
                return;
            }
            let dt = self.spacing(field, t, &s);
            // The last step is split evenly with the one before rather
            // than left a sliver.
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

    /// The time to the next sample after state `s` at `t`: [`ANGLE`] over
    /// the rate the motion turns about the body pulling hardest (its speed
    /// relative to it, or the circular speed there if faster, over the
    /// distance).
    fn spacing(&self, field: &Field, t: f64, s: &[f64; 6]) -> f64 {
        let when = Epoch(self.epoch.0 + t / SECONDS_PER_DAY);
        let r = DVec3::new(s[0], s[1], s[2]);
        let (b, d) = hardest(field, when, r);
        let v = (DVec3::new(s[3], s[4], s[5]) - field.body_state(b, when).v).length();
        let circular = (field.ephemeris().body(b).gm / d).sqrt();
        (ANGLE * d / v.max(circular)).min(LONGEST_S)
    }

    /// Whether the motion is the center's own: every state zero, as for a
    /// ship resting on or at a body.
    pub fn rests(&self) -> bool {
        self.center.is_some() && self.states.iter().all(|s| s.iter().all(|&x| x == 0.0))
    }

    /// First and last sample times.
    pub fn span(&self) -> Option<(f64, f64)> {
        Some((*self.times.first()?, *self.times.last()?))
    }

    /// The state at `t` (s after the epoch), by cubic Hermite interpolation
    /// between the samples around it (the later of two at one time); `None`
    /// outside the span.
    pub fn at(&self, t: f64) -> Option<[f64; 6]> {
        let (first, last) = self.span()?;
        if t < first || t > last {
            return None;
        }
        let b = self.times.partition_point(|&s| s <= t);
        if b == self.times.len() {
            return self.states.last().copied();
        }
        let a = b - 1;
        let h = self.times[b] - self.times[a];
        let u = (t - self.times[a]) / h;
        let (pa, va) = split(&self.states[a]);
        let (pb, vb) = split(&self.states[b]);
        let (u2, u3) = (u * u, u * u * u);
        let p = (2.0 * u3 - 3.0 * u2 + 1.0) * pa
            + (u3 - 2.0 * u2 + u) * h * va
            + (-2.0 * u3 + 3.0 * u2) * pb
            + (u3 - u2) * h * vb;
        let v = (6.0 * u2 - 6.0 * u) / h * pa
            + (3.0 * u2 - 4.0 * u + 1.0) * va
            + (-6.0 * u2 + 6.0 * u) / h * pb
            + (3.0 * u2 - 2.0 * u) * vb;
        Some([p.x, p.y, p.z, v.x, v.y, v.z])
    }
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
        let gaps: Vec<f64> = track.times.windows(2).map(|w| w[1] - w[0]).collect();
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
}
