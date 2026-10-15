//! The bodies' orbits (M1c.md, step 6 plan, item 3): each body's path
//! about its parent (the Sun's planets and small bodies, Earth's Moon,
//! Mars's moons) over a period around now, from the ephemeris. They are
//! drawn as KSP draws them: relative to the parent, placed at the
//! parent's position now, without turning, so the line passes through the
//! body in every frame.
//!
//! A body's window steps by a sixteenth of its period, so it is sampled
//! again only when now moves a step.

use crate::paths::LOOP_POSE;
use crate::settings::Lines;
use glam::DMat3;
use orbit::ephem::{BodyId, BodyKind, Ephemeris};
use orbit::time::{Epoch, SECONDS_PER_DAY};
use render::{Path, Paths, Pose, Sample};
use std::sync::Arc;

/// Windows step by this share of a period.
const STEPS: i64 = 16;

/// Samples per step: at least 384 over a period, with velocities.
const SAMPLES_PER_STEP: i64 = 24;

/// And at most this far apart, s: the Sun's reflex motion (Venus's and
/// Earth's pulls, ~1,000 km, periods down to a fraction of a year) is in
/// every path about it. Neptune's at a 384th of its period (160 days)
/// missed it by 290 km; at 10 days, by 34 m.
const MAX_SPACING_S: f64 = 10.0 * SECONDS_PER_DAY;

/// A small body's samples, closer: the camera comes within a tenth of a
/// body's extent of it (`camera::min_altitude`), 70 m from 2009 UY19's
/// center, where a pixel is 6 cm. At a day apart, 1996 FG3's line missed
/// it by 460 m in March 2045; at 6 hours, 2009 UY19's by 13 cm (2.6
/// pixels there); at 3 hours, by 5 mm.
const SMALL_BODY_SPACING_S: f64 = 0.125 * SECONDS_PER_DAY;

/// Colors (linear, as shown): planets' and moons' orbits, small bodies'
/// fainter; the focus's brighter.
const PLANET_COLOR: [f32; 3] = [0.10, 0.13, 0.18];
const SMALL_BODY_COLOR: [f32; 3] = [0.05, 0.05, 0.05];
const FOCUS_GAIN: f32 = 2.5;

/// The past's color, from the future's.
const PAST_DIM: f32 = 0.5;

/// The gap left in a line where its body is, line widths: a point of light
/// shows through it.
const GAP: f32 = 3.0;

/// A body's orbit and the window it was sampled over.
struct Orbit {
    body: BodyId,
    parent: BodyId,
    /// The orbit's period about its parent, s.
    period: f64,
    /// The window's step, on the time axis's grid of period / STEPS.
    step: Option<i64>,
    /// Samples per step.
    per_step: i64,
    samples: Vec<Sample>,
}

/// The bodies' orbits as paths, and the poses they are placed with.
pub struct Orbits {
    orbits: Vec<Orbit>,
    /// The parents, in the order of their poses after [`LOOP_POSE`].
    parents: Vec<BodyId>,
    focus: Option<BodyId>,
    /// The classes of lines shown when the paths were built.
    lines: Option<Lines>,
    paths: Arc<Paths>,
}

/// Whether `lines` shows the orbits of bodies of `kind`.
fn shown(lines: &Lines, kind: &BodyKind) -> bool {
    match kind {
        BodyKind::Planet => lines.planets,
        BodyKind::Moon => lines.moons,
        BodyKind::SmallBody { .. } => lines.small_bodies,
        BodyKind::Star => false,
    }
}

/// The body `b`'s parent, whose orbit about it is drawn: the planet its
/// system is named for, else the Sun. None for the Sun.
fn parent(eph: &Ephemeris, b: BodyId) -> Option<BodyId> {
    if b == eph.sun() {
        return None;
    }
    let system = eph.system(b);
    Some(if system == b { eph.sun() } else { system })
}

impl Orbits {
    /// The orbits of `bodies` (those with a parent and a bound orbit about
    /// it at `t`).
    pub fn new(eph: &Ephemeris, bodies: &[BodyId], t: Epoch) -> Orbits {
        let mut parents = Vec::new();
        let orbits = bodies
            .iter()
            .filter_map(|&body| {
                let parent = parent(eph, body)?;
                let t = clamp(eph, body, t);
                let (b, p) = (eph.barycentric(body, t), eph.barycentric(parent, t));
                let (r, v) = (b.r - p.r, b.v - p.v);
                let mu = eph.body(parent).gm + eph.body(body).gm;
                let a = 1.0 / (2.0 / r.length() - v.length_squared() / mu);
                if a.is_nan() || a <= 0.0 {
                    return None;
                }
                if !parents.contains(&parent) {
                    parents.push(parent);
                }
                let period = std::f64::consts::TAU * (a.powi(3) / mu).sqrt();
                let step = period / STEPS as f64;
                let spacing = match eph.body(body).kind {
                    BodyKind::SmallBody { .. } => SMALL_BODY_SPACING_S,
                    _ => MAX_SPACING_S,
                };
                Some(Orbit {
                    body,
                    parent,
                    period,
                    step: None,
                    per_step: SAMPLES_PER_STEP.max((step / spacing).ceil() as i64),
                    samples: Vec::new(),
                })
            })
            .collect();
        assert!(
            LOOP_POSE + 1 + parents.len() <= render::paths::MAX_POSES,
            "a pose for each parent"
        );
        Orbits {
            orbits,
            parents,
            focus: None,
            lines: None,
            paths: Arc::default(),
        }
    }

    /// Samples again the orbits whose windows moved at `t`, colors the
    /// focus's, and leaves out the classes `lines` hides; `true` if the
    /// paths changed.
    pub fn update(&mut self, eph: &Ephemeris, t: Epoch, focus: BodyId, lines: Lines) -> bool {
        let mut changed = self.focus != Some(focus) || self.lines != Some(lines);
        self.focus = Some(focus);
        self.lines = Some(lines);
        let now = t.seconds();
        for orbit in &mut self.orbits {
            let step = orbit.period / STEPS as f64;
            let k = (now / step).floor() as i64;
            if orbit.step == Some(k) {
                continue;
            }
            orbit.step = Some(k);
            let n = orbit.per_step;
            orbit.samples = ((k - STEPS / 2 + 1) * n..=(k + STEPS / 2 + 1) * n)
                .filter_map(|i| {
                    let s = i as f64 * step / n as f64;
                    let t = Epoch(s / SECONDS_PER_DAY);
                    if !eph.covers(orbit.body, t) {
                        return None;
                    }
                    let (b, p) = (
                        eph.barycentric(orbit.body, t),
                        eph.barycentric(orbit.parent, t),
                    );
                    Some(Sample {
                        t: s,
                        position: b.r - p.r,
                        velocity: b.v - p.v,
                    })
                })
                .collect();
            changed = true;
        }
        if changed {
            self.paths = Arc::new(Paths {
                paths: self
                    .orbits
                    .iter()
                    .filter(|o| o.samples.len() > 1 && shown(&lines, &eph.body(o.body).kind))
                    .map(|o| {
                        let small = matches!(eph.body(o.body).kind, BodyKind::SmallBody { .. });
                        let mut color = if small {
                            SMALL_BODY_COLOR
                        } else {
                            PLANET_COLOR
                        };
                        if Some(o.body) == self.focus {
                            color = color.map(|c| c * FOCUS_GAIN);
                        }
                        Path {
                            pose: self.pose_of(o.parent),
                            samples: o.samples.clone(),
                            future: color,
                            past: color.map(|c| c * PAST_DIM),
                            gap: GAP,
                        }
                    })
                    .collect(),
            });
        }
        changed
    }

    fn pose_of(&self, parent: BodyId) -> usize {
        LOOP_POSE
            + 1
            + self
                .parents
                .iter()
                .position(|&p| p == parent)
                .expect("a parent")
    }

    pub fn paths(&self) -> Arc<Paths> {
        self.paths.clone()
    }

    /// The parents' poses at `t`, after [`LOOP_POSE`]: each at its
    /// position, the ephemeris's axes.
    pub fn poses(&self, eph: &Ephemeris, t: Epoch) -> impl Iterator<Item = Pose> {
        self.parents.iter().map(move |&p| Pose {
            origin: eph.barycentric_position(p, t),
            axes: DMat3::IDENTITY,
            scale: 1.0,
        })
    }
}

/// `t`, or the nearest moment the ephemeris knows `b` at.
fn clamp(eph: &Ephemeris, b: BodyId, t: Epoch) -> Epoch {
    let (first, last) = eph.span_of(b);
    Epoch(t.0.clamp(first.0, last.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each orbit's line passes through its body now: its path, placed by
    /// its parent's pose, at the moment. Within a fifth of a pixel of its
    /// center seen from the closest the camera comes (`min_altitude` over
    /// its extent, 50° across 1080 pixels), the shapes read.
    #[test]
    fn lines_pass_through_their_bodies() {
        let eph = Ephemeris::standard();
        crate::maps::wait().expect("data/bodies reads");
        let bodies = crate::bodies::drawn(eph);
        let t = Epoch::from_ymd(2045, 3, 1).plus_days(0.37);
        let mut orbits = Orbits::new(eph, &bodies, t);
        assert!(orbits.update(eph, t, eph.lookup("earth").unwrap(), Lines::default()));
        let poses: Vec<Pose> = orbits.poses(eph, t).collect();
        let paths = orbits.paths();
        let (mut checked, mut worst) = (0, 0f64);
        for (o, path) in orbits
            .orbits
            .iter()
            .filter(|o| o.samples.len() > 1)
            .zip(&paths.paths)
        {
            let pose = poses[path.pose - LOOP_POSE - 1];
            let at = pose.to_world(path.at(t.seconds()).expect("now is in the window"));
            let body = eph.barycentric_position(o.body, t);
            let off = (at - body).length();
            let extent = crate::bodies::extent(eph, o.body);
            let closest = extent + crate::camera::min_altitude(extent);
            let pixel = closest * 2.0 * 25f64.to_radians().tan() / 1080.0;
            println!(
                "{}: {off:.2e} km off, {:.3} px from {closest:.0} km",
                eph.body(o.body).id,
                off / pixel
            );
            worst = worst.max(off / pixel);
            checked += 1;
        }
        assert!(worst < 0.2, "{worst} px");
        assert!(checked >= 12, "{checked}");
        // Unmoved, nothing is sampled again.
        assert!(!orbits.update(eph, t, eph.lookup("earth").unwrap(), Lines::default()));
    }

    /// Each class of orbit is left out alone when hidden, and comes back
    /// when shown.
    #[test]
    fn hidden_classes_are_left_out() {
        let eph = Ephemeris::standard();
        let bodies = crate::bodies::drawn(eph);
        let t = Epoch::from_ymd(2045, 3, 1);
        let earth = eph.lookup("earth").unwrap();
        let mut orbits = Orbits::new(eph, &bodies, t);
        let count = |orbits: &Orbits, kind: fn(&BodyKind) -> bool| {
            orbits
                .orbits
                .iter()
                .filter(|o| o.samples.len() > 1 && kind(&eph.body(o.body).kind))
                .count()
        };
        let all = Lines::default();
        orbits.update(eph, t, earth, all);
        let total = orbits.paths().paths.len();
        // Each class's switch, and the kinds of bodies it holds.
        type Class = (fn(&mut Lines) -> &mut bool, fn(&BodyKind) -> bool);
        let classes: [Class; 3] = [
            (|l| &mut l.planets, |k| matches!(k, BodyKind::Planet)),
            (|l| &mut l.moons, |k| matches!(k, BodyKind::Moon)),
            (
                |l| &mut l.small_bodies,
                |k| matches!(k, BodyKind::SmallBody { .. }),
            ),
        ];
        for (field, kind) in classes {
            let n = count(&orbits, kind);
            assert!(n > 0);
            let mut lines = all;
            *field(&mut lines) = false;
            assert!(orbits.update(eph, t, earth, lines));
            assert_eq!(orbits.paths().paths.len(), total - n);
            assert!(orbits.update(eph, t, earth, all));
            assert_eq!(orbits.paths().paths.len(), total);
        }
    }

    #[test]
    fn periods_and_parents() {
        let eph = Ephemeris::standard();
        let ids = ["earth", "moon", "phobos", "jupiter"].map(|id| eph.lookup(id).unwrap());
        let orbits = Orbits::new(eph, &ids, Epoch::from_ymd(2045, 3, 1));
        let days: Vec<f64> = orbits
            .orbits
            .iter()
            .map(|o| o.period / SECONDS_PER_DAY)
            .collect();
        // Sidereal: a year, a month, Phobos's 7.65 h, Jupiter's 11.86 years
        // (osculating, so within a few percent).
        for (got, want) in days.iter().zip([365.26, 27.32, 0.3189, 4332.6]) {
            assert!((got / want - 1.0).abs() < 0.03, "{got} vs {want}");
        }
        let moon = &orbits.orbits[1];
        assert_eq!(moon.parent, eph.lookup("earth").unwrap());
        assert_eq!(orbits.orbits[2].parent, eph.lookup("mars").unwrap());
        assert_eq!(orbits.parents.len(), 3);
    }
}
