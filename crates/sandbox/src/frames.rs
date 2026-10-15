//! Reference frames for paths (M1c.md, The view, Frames; step 4 plan,
//! items 2 and 3): fixed axes centered on the camera's focus, or turning
//! with two bodies. A path's samples are mapped into the frame at their
//! own times, and drawn by the frame's pose now; the camera's turntable
//! turns with the frame's axes.

use glam::{DMat3, DVec3};
use orbit::cr3bp::Cr3bp;
use orbit::ephem::{BodyId, Ephemeris};
use orbit::families::{Catalog, PeriodicOrbit};
use orbit::integrate::{Dop853, Tolerance, Trajectory};
use orbit::places::{PlaceKind, Places};
use orbit::reference::RotatingFrame;
use orbit::time::{Epoch, SECONDS_PER_DAY};
use render::{Pose, Sample};

/// The step for the rate of a frame's axes, s.
const RATE_STEP_S: f64 = 60.0;

/// Reference loops are sampled every this much of their motion about the
/// nearer primary, radians, and at least this many times a revolution.
const LOOP_ANGLE: f64 = 2.0 * std::f64::consts::PI / 180.0;
const LOOP_SAMPLES: f64 = 180.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Frame {
    /// Ecliptic axes, centered on the camera's focus.
    Focus,
    /// Centered on the two bodies' barycenter, x from the primary to the
    /// secondary, z along the secondary's orbital angular momentum about
    /// it; scaled by their distance where `pulsating`.
    Rotating {
        primary: BodyId,
        secondary: BodyId,
        pulsating: bool,
    },
}

/// A frame's motion at one moment: the velocity of its origin, the rate
/// of its axes, and of its scale.
#[derive(Clone, Copy, Debug)]
struct Rates {
    origin: DVec3,
    axes: DMat3,
    scale: f64,
}

impl Frame {
    /// The frames the sandbox offers: (id, name, frame).
    pub fn offered(eph: &Ephemeris) -> Vec<(&'static str, &'static str, Frame)> {
        let id = |s: &str| eph.lookup(s).expect("a body in the ephemeris");
        let (sun, earth, moon, mars) = (eph.sun(), id("earth"), id("moon"), id("mars"));
        let rotating = |primary, secondary, pulsating| Frame::Rotating {
            primary,
            secondary,
            pulsating,
        };
        vec![
            ("focus", "Centered on the focus", Frame::Focus),
            (
                "earth-moon",
                "Earth–Moon, rotating",
                rotating(earth, moon, false),
            ),
            (
                "earth-moon-pulsating",
                "Earth–Moon, rotating, scaled",
                rotating(earth, moon, true),
            ),
            (
                "sun-earth",
                "Sun–Earth, rotating",
                rotating(sun, earth, false),
            ),
            ("sun-mars", "Sun–Mars, rotating", rotating(sun, mars, false)),
        ]
    }

    /// Whether the frame is known at `t`, with the camera on `focus` (a
    /// small body's orbit is known from 2040).
    pub fn covers(&self, eph: &Ephemeris, focus: BodyId, t: Epoch) -> bool {
        match *self {
            Frame::Focus => eph.covers(focus, t),
            Frame::Rotating {
                primary, secondary, ..
            } => eph.covers(primary, t) && eph.covers(secondary, t),
        }
    }

    /// Where the frame is at `t`, with the camera on `focus`.
    pub fn pose(&self, eph: &Ephemeris, focus: BodyId, t: Epoch) -> Pose {
        match *self {
            Frame::Focus => Pose {
                origin: eph.barycentric_position(focus, t),
                ..Default::default()
            },
            Frame::Rotating {
                primary,
                secondary,
                pulsating,
            } => {
                let f = RotatingFrame::at(eph, primary, secondary, t);
                let (a, b) = (
                    eph.barycentric_position(primary, t),
                    eph.barycentric_position(secondary, t),
                );
                Pose {
                    origin: a + (b - a) * mass_ratio(eph, primary, secondary),
                    axes: DMat3::from_cols(f.x, f.y, f.z),
                    scale: if pulsating { f.distance } else { 1.0 },
                }
            }
        }
    }

    /// The pose at `t` and how it is changing.
    fn motion(&self, eph: &Ephemeris, focus: BodyId, t: Epoch) -> (Pose, Rates) {
        let pose = self.pose(eph, focus, t);
        let rates = match *self {
            Frame::Focus => Rates {
                origin: eph.barycentric(focus, t).v,
                axes: DMat3::ZERO,
                scale: 0.0,
            },
            Frame::Rotating {
                primary,
                secondary,
                pulsating,
            } => {
                let (a, b) = (eph.barycentric(primary, t), eph.barycentric(secondary, t));
                let step = RATE_STEP_S / SECONDS_PER_DAY;
                let (before, after) = (
                    self.pose(eph, focus, Epoch(t.0 - step)),
                    self.pose(eph, focus, Epoch(t.0 + step)),
                );
                Rates {
                    origin: a.v + (b.v - a.v) * mass_ratio(eph, primary, secondary),
                    axes: (after.axes - before.axes) * (0.5 / RATE_STEP_S),
                    scale: if pulsating {
                        RotatingFrame::at(eph, primary, secondary, t).distance_rate
                    } else {
                        0.0
                    },
                }
            }
        };
        (pose, rates)
    }

    /// A barycentric state `(r, v)` (km, km/s) at `t` as a sample in the
    /// frame, at `seconds` on the paths' time axis.
    pub fn sample(
        &self,
        eph: &Ephemeris,
        focus: BodyId,
        t: Epoch,
        seconds: f64,
        r: DVec3,
        v: DVec3,
    ) -> Sample {
        let (pose, rates) = self.motion(eph, focus, t);
        let d = r - pose.origin;
        let position = pose.axes.transpose() * d / pose.scale;
        let velocity = (rates.axes.transpose() * d + pose.axes.transpose() * (v - rates.origin))
            / pose.scale
            - position * (rates.scale / pose.scale);
        Sample {
            t: seconds,
            position,
            velocity,
        }
    }
}

/// The secondary's share of the two bodies' mass: the barycenter's place
/// along the line between them.
fn mass_ratio(eph: &Ephemeris, primary: BodyId, secondary: BodyId) -> f64 {
    let (a, b) = (eph.body(primary).gm, eph.body(secondary).gm);
    b / (a + b)
}

/// The Earth–Moon rotating, scaled frame that reference loops are drawn
/// in: its coordinates are the CR3BP's.
pub fn loop_frame(eph: &Ephemeris) -> Frame {
    let id = |s: &str| eph.lookup(s).expect("a body in the ephemeris");
    Frame::Rotating {
        primary: id("earth"),
        secondary: id("moon"),
        pulsating: true,
    }
}

/// One revolution of a catalog orbit, in the CR3BP's coordinates (the
/// loop frame's) and the catalog's time unit in seconds: sampled every
/// [`LOOP_ANGLE`] of its motion about the nearer primary, at least
/// [`LOOP_SAMPLES`] times.
pub fn reference_loop(cat: &Catalog, orbit: PeriodicOrbit) -> Vec<Sample> {
    let system: &Cr3bp = &cat.system;
    let ode = system.ode();
    let tol = Tolerance {
        rtol: 1e-12,
        atol: 1e-12,
    };
    let mut run = Dop853::new(&ode, 0.0, orbit.state, tol);
    let mut steps = Vec::new();
    run.advance_dense(&ode, orbit.period, &mut steps)
        .expect("a catalog orbit integrates over its period");
    let path = Trajectory::new(steps);
    let (earth, moon) = (
        DVec3::new(-system.mu, 0.0, 0.0),
        DVec3::new(1.0 - system.mu, 0.0, 0.0),
    );
    let unit = cat.time_s;
    let mut out = Vec::new();
    let mut t: f64 = 0.0;
    loop {
        let s = path.at(t.min(orbit.period));
        let (r, v) = (DVec3::new(s[0], s[1], s[2]), DVec3::new(s[3], s[4], s[5]));
        out.push(Sample {
            t: t * unit,
            position: r,
            velocity: v / unit,
        });
        if t >= orbit.period {
            return out;
        }
        let near = (r - earth).length().min((r - moon).length());
        let step = (LOOP_ANGLE * near / v.length()).min(orbit.period / LOOP_SAMPLES);
        t = (t + step).min(orbit.period);
    }
}

/// The station places' catalog orbits, by place.
pub fn station_loops(places: &Places, cat: &Catalog) -> Vec<(String, Vec<Sample>)> {
    places
        .iter()
        .filter_map(|p| match &p.kind {
            PlaceKind::Station { family, period } => {
                let member = cat.nearest(family, *period)?;
                Some((p.id.clone(), reference_loop(cat, member.orbit)))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(eph: &Ephemeris) -> Vec<Frame> {
        Frame::offered(eph).into_iter().map(|(_, _, f)| f).collect()
    }

    /// In the Earth–Moon frames the Moon lies on the x axis, at its share
    /// of the distance from the barycenter (unscaled) or at 1 − μ
    /// (scaled); poses map back.
    #[test]
    fn moon_on_the_x_axis() {
        let eph = Ephemeris::standard();
        let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
        let t = Epoch::from_ymd(2045, 3, 1);
        let mu = mass_ratio(eph, earth, moon);
        let m = eph.barycentric_position(moon, t);
        let d = (m - eph.barycentric_position(earth, t)).length();
        for (pulsating, x) in [(false, (1.0 - mu) * d), (true, 1.0 - mu)] {
            let f = Frame::Rotating {
                primary: earth,
                secondary: moon,
                pulsating,
            };
            let pose = f.pose(eph, earth, t);
            let p = pose.from_world(m);
            assert!((p - DVec3::new(x, 0.0, 0.0)).length() < 1e-9 * x, "{p}");
            assert!((pose.to_world(p) - m).length() < 1e-6);
        }
    }

    /// A sample's velocity in each frame is the rate its position in the
    /// frame changes: a point flying straight past the Moon (which is
    /// still in the scaled Earth–Moon frame), by central differences.
    #[test]
    fn velocities_match_positions() {
        let eph = Ephemeris::standard();
        let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
        let t = Epoch::from_ymd(2045, 3, 1);
        let m = eph.barycentric(moon, t);
        let (r0, v) = (
            m.r + DVec3::new(2e4, -5e3, 3e3),
            m.v + DVec3::new(0.3, 0.2, -0.1),
        );
        let h = 30.0;
        for f in frames(eph) {
            let at = |dt: f64| {
                let tt = Epoch(t.0 + dt / SECONDS_PER_DAY);
                f.sample(eph, earth, tt, dt, r0 + v * dt, v)
            };
            let (a, b, c) = (at(-h), at(0.0), at(h));
            let numeric = (c.position - a.position) / (2.0 * h);
            let error = (numeric - b.velocity).length() / b.velocity.length();
            println!(
                "{f:?}: speed {:.6e}, off by {error:.2e}",
                b.velocity.length()
            );
            assert!(error < 1e-6, "{f:?}: {error}");
        }
    }

    /// Each station's loop closes, and its sampling reaches into its
    /// perilune (the NRHO's passes ~3,000 km from the Moon's center).
    #[test]
    fn station_loops_close() {
        let cat = Catalog::standard();
        let loops = station_loops(Places::standard(), cat);
        assert!(loops.len() >= 4, "{}", loops.len());
        for (id, samples) in &loops {
            let (first, last) = (samples[0], *samples.last().unwrap());
            let gap = (first.position - last.position).length() * cat.length_km;
            let moon = DVec3::new(1.0 - cat.system.mu, 0.0, 0.0);
            let perilune = samples
                .iter()
                .map(|s| (s.position - moon).length() * cat.length_km)
                .fold(f64::INFINITY, f64::min);
            println!(
                "{id}: {} samples, closes within {gap:.3} km, perilune {perilune:.0} km",
                samples.len()
            );
            assert!(gap < 1.0, "{id}: {gap} km");
            if id == "nrho" {
                assert!(perilune < 3_500.0, "{perilune} km");
            }
        }
    }
}
