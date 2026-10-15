//! The map view's camera (M1c.md, step 3 plan, item 6): focused on a
//! body and following it, a turntable about the frame's z axis (ecliptic
//! north in fixed frames; step 4 plan, item 2), zoomed by altitude above
//! the body (`min_altitude`) to 1e10 km, and flown to a new focus in 0.6
//! s.

use damascene_core::PointerButton;
use glam::{DMat3, DVec3};
use orbit::ephem::{BodyId, Ephemeris};
use orbit::time::Epoch;

/// The closest the eye comes to a body reaching `extent` km from its
/// center (`bodies::extent`), km above that: a tenth of it, at most 10.
pub fn min_altitude(extent: f64) -> f64 {
    (extent / 10.0).min(10.0)
}

/// The farthest the eye goes from the focus, km.
pub const MAX_DISTANCE_KM: f64 = 1e10;

/// Radians of orbit per logical pixel dragged.
const ORBIT_PER_PX: f64 = 0.005;

/// Each wheel line scales the altitude by this.
const ZOOM_PER_LINE: f64 = 1.25;

/// Seconds a refocus takes.
const REFOCUS_S: f64 = 0.6;

/// Elevation stops short of the poles, where the turntable's up
/// degenerates, radians.
const MAX_ELEVATION: f64 = 1.55;

/// A new focus starts this many times its extent away.
const FOCUS_EXTENTS: f64 = 5.0;

#[derive(Clone, Debug)]
pub struct MapCamera {
    pub focus: BodyId,
    /// From the focus's center, km.
    pub distance: f64,
    /// Of the eye about the focus, radians, about the frame's axes:
    /// azimuth in its x-y plane, elevation above it.
    pub azimuth: f64,
    pub elevation: f64,
    /// Vertical field of view, radians.
    pub fov_y: f64,
    refocus: Option<Refocus>,
    dragging: Option<(f32, f32)>,
}

/// A flight to the focus: from where the view's center was, kept
/// relative to the focus as it moves.
#[derive(Clone, Copy, Debug)]
struct Refocus {
    /// The view's center less the focus's position, when the flight began.
    offset: DVec3,
    from_distance: f64,
    to_distance: f64,
    /// Seconds into the flight.
    elapsed: f64,
}

/// Where a new focus is first seen from, km from its center.
fn start_distance(eph: &Ephemeris, b: BodyId) -> f64 {
    crate::bodies::extent(eph, b) * FOCUS_EXTENTS
}

impl MapCamera {
    pub fn new(eph: &Ephemeris, focus: BodyId) -> MapCamera {
        MapCamera {
            focus,
            distance: start_distance(eph, focus),
            azimuth: -1.2,
            elevation: 0.35,
            fov_y: 50f64.to_radians(),
            refocus: None,
            dragging: None,
        }
    }

    /// Flies to `body` from where the view is at `t` (mid-flight too),
    /// keeping its direction.
    pub fn focus_on(&mut self, eph: &Ephemeris, body: BodyId, t: Epoch) {
        if body == self.focus {
            return;
        }
        let (center, distance) = self.center(eph, t);
        let to_distance = start_distance(eph, body);
        self.refocus = Some(Refocus {
            offset: center - eph.barycentric_position(body, t),
            from_distance: distance,
            to_distance,
            elapsed: 0.0,
        });
        self.focus = body;
        self.distance = to_distance;
    }

    /// Sets the altitude above the focus's surface (a shaped body's
    /// greatest radius), km, within the camera's limits.
    pub fn set_altitude(&mut self, eph: &Ephemeris, km: f64) {
        let extent = crate::bodies::extent(eph, self.focus);
        self.distance = (extent + km.max(min_altitude(extent))).min(MAX_DISTANCE_KM);
    }

    /// Sets the eye's direction from the focus, radians; the elevation
    /// stops short of the poles.
    pub fn set_direction(&mut self, azimuth: f64, elevation: f64) {
        self.azimuth = azimuth.rem_euclid(std::f64::consts::TAU);
        self.elevation = elevation.clamp(-MAX_ELEVATION, MAX_ELEVATION);
    }

    pub fn flying(&self) -> bool {
        self.refocus.is_some()
    }

    /// Advances a refocus by `dt` seconds of wall time.
    pub fn tick(&mut self, dt: f64) {
        if let Some(r) = self.refocus.as_mut() {
            r.elapsed += dt;
            if r.elapsed >= REFOCUS_S {
                self.refocus = None;
            }
        }
    }

    /// A press over the map view: a right drag orbits.
    pub fn press(&mut self, button: PointerButton, at: (f32, f32)) {
        if button == PointerButton::Secondary {
            self.dragging = Some(at);
        }
    }

    /// The pointer moved; `true` if the camera did.
    pub fn moved(&mut self, at: (f32, f32)) -> bool {
        let Some(from) = self.dragging else {
            return false;
        };
        let (dx, dy) = (f64::from(at.0 - from.0), f64::from(at.1 - from.1));
        self.azimuth = (self.azimuth - dx * ORBIT_PER_PX).rem_euclid(std::f64::consts::TAU);
        self.elevation = (self.elevation + dy * ORBIT_PER_PX).clamp(-MAX_ELEVATION, MAX_ELEVATION);
        self.dragging = Some(at);
        true
    }

    pub fn release(&mut self, button: PointerButton) {
        if button == PointerButton::Secondary {
            self.dragging = None;
        }
    }

    /// Ends any drag: the window lost focus.
    pub fn cancel(&mut self) {
        self.dragging = None;
    }

    /// Wheel lines in (positive) or out: the altitude above the focus's
    /// surface scales, so the approach slows near it.
    pub fn wheel(&mut self, eph: &Ephemeris, lines: f32) {
        let extent = crate::bodies::extent(eph, self.focus);
        let altitude = (self.distance - extent) * ZOOM_PER_LINE.powf(-f64::from(lines));
        self.distance = (extent + altitude.max(min_altitude(extent))).min(MAX_DISTANCE_KM);
        // A wheel during a refocus lands where it says.
        if let Some(r) = self.refocus.as_mut() {
            r.to_distance = self.distance;
        }
    }

    /// The view's center and the eye's distance from it at `t`.
    fn center(&self, eph: &Ephemeris, t: Epoch) -> (DVec3, f64) {
        let focus = eph.barycentric_position(self.focus, t);
        match self.refocus {
            Some(r) => {
                let s = (r.elapsed / REFOCUS_S).clamp(0.0, 1.0);
                let s = s * s * (3.0 - 2.0 * s);
                let distance = (r.from_distance.ln() * (1.0 - s) + r.to_distance.ln() * s).exp();
                (focus + r.offset * (1.0 - s), distance)
            }
            None => (focus, self.distance),
        }
    }

    /// The eye at `t`, its turntable about the frame's `axes` (columns)
    /// then.
    pub fn camera(&self, eph: &Ephemeris, t: Epoch, axes: DMat3) -> render::Camera {
        let (target, distance) = self.center(eph, t);
        let out = axes * self.direction();
        render::Camera::looking_at(target + out * distance, target, axes.z_axis, self.fov_y)
    }

    /// The eye's direction from the focus, in the frame's coordinates.
    fn direction(&self) -> DVec3 {
        let (ca, sa) = (self.azimuth.cos(), self.azimuth.sin());
        let (ce, se) = (self.elevation.cos(), self.elevation.sin());
        DVec3::new(ce * ca, ce * sa, se)
    }

    /// The frame's axes changed from `from` to `to`: the eye keeps its
    /// direction in the world (its elevation stopping short of the poles).
    pub fn reorient(&mut self, from: DMat3, to: DMat3) {
        let d = to.transpose() * (from * self.direction());
        self.set_direction(d.y.atan2(d.x), d.z.clamp(-1.0, 1.0).asin());
    }

    /// The eye's altitude above the focus's surface (a shaped body's
    /// greatest radius), km.
    pub fn altitude(&self, eph: &Ephemeris) -> f64 {
        self.distance - crate::bodies::extent(eph, self.focus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wheel zooms by altitude and stops 10 km above the surface and
    /// at 1e10 km; a right drag orbits about the focus, which stays at
    /// the screen's center.
    #[test]
    fn zoom_and_orbit() {
        let eph = Ephemeris::standard();
        let moon = eph.lookup("moon").unwrap();
        let mut c = MapCamera::new(eph, moon);
        for _ in 0..200 {
            c.wheel(eph, 1.0);
        }
        assert!((c.altitude(eph) - 10.0).abs() < 1e-9);
        for _ in 0..400 {
            c.wheel(eph, -1.0);
        }
        assert_eq!(c.distance, MAX_DISTANCE_KM);
        c.wheel(eph, 10.0);
        let t = Epoch::from_ymd(2045, 3, 1);
        let before = c.camera(eph, t, DMat3::IDENTITY);
        c.press(PointerButton::Secondary, (0.0, 0.0));
        assert!(c.moved((40.0, -20.0)));
        c.release(PointerButton::Secondary);
        assert!(!c.moved((80.0, 0.0)));
        let after = c.camera(eph, t, DMat3::IDENTITY);
        let focus = eph.barycentric_position(moon, t);
        assert!(((after.eye - focus).length() / c.distance - 1.0).abs() < 1e-12);
        assert!(after.forward.angle_between(before.forward) > 0.1);
        let center = after.project(focus, (800, 600)).unwrap();
        assert!((center - glam::DVec2::new(400.0, 300.0)).length() < 1e-6);
    }

    /// A refocus flies from the old focus to the new in 0.6 s, and ends
    /// on the new one.
    #[test]
    fn refocus_flies_between_moving_bodies() {
        let eph = Ephemeris::standard();
        let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
        let t = Epoch::from_ymd(2045, 3, 1);
        let mut c = MapCamera::new(eph, earth);
        c.focus_on(eph, moon, t);
        let (e, m) = (
            eph.barycentric_position(earth, t),
            eph.barycentric_position(moon, t),
        );
        let start = c.camera(eph, t, DMat3::IDENTITY);
        assert!(((start.eye - e).length() / (5.0 * 6_371.0) - 1.0).abs() < 1e-9);
        // Halfway, the view's axis passes through the midpoint.
        c.tick(0.3);
        assert!(c.flying());
        let mid = c.camera(eph, t, DMat3::IDENTITY);
        let off_axis = ((e + m) / 2.0 - mid.eye).cross(mid.forward).length();
        assert!(off_axis < 1e-3, "{off_axis} km");
        c.tick(0.4);
        assert!(!c.flying());
        let end = c.camera(eph, t, DMat3::IDENTITY);
        assert!(((end.eye - m).length() - 5.0 * 1_737.4).abs() < 1e-3);
    }

    /// A new focus picked mid-flight starts from where the view is
    /// (found in review: it jumped 352,000 km); a small body's starts five
    /// of its extents out, and stops a tenth of one over it: Bennu, its
    /// shape read, 290 m from its center at most.
    #[test]
    fn refocus_mid_flight_and_small_bodies() {
        let eph = Ephemeris::standard();
        crate::maps::wait().expect("data/bodies reads");
        let t = Epoch::from_ymd(2045, 3, 1);
        let id = |s: &str| eph.lookup(s).unwrap();
        let mut c = MapCamera::new(eph, id("earth"));
        c.focus_on(eph, id("moon"), t);
        c.tick(0.1);
        let before = c.camera(eph, t, DMat3::IDENTITY);
        c.focus_on(eph, id("mars"), t);
        let after = c.camera(eph, t, DMat3::IDENTITY);
        assert!(
            (after.eye - before.eye).length() < 1e-3,
            "{}",
            (after.eye - before.eye).length()
        );
        let mut small = MapCamera::new(eph, id("bennu"));
        let extent = crate::bodies::extent(eph, id("bennu"));
        assert!((0.28..0.30).contains(&extent), "{extent}");
        assert!((small.distance - 5.0 * extent).abs() < 1e-12);
        small.set_direction(0.0, 2.0);
        assert_eq!(small.elevation, MAX_ELEVATION);
        small.set_altitude(eph, -5.0);
        assert!((small.altitude(eph) - extent / 10.0).abs() < 1e-12);
    }

    /// Changing the frame keeps the eye where it is; in a turning frame
    /// the eye turns with it, its up the frame's z.
    #[test]
    fn frames_turn_the_turntable() {
        let eph = Ephemeris::standard();
        let t = Epoch::from_ymd(2045, 3, 1);
        let mut c = MapCamera::new(eph, eph.lookup("earth").unwrap());
        let turned = DMat3::from_rotation_z(1.1) * DMat3::from_rotation_x(0.09);
        let before = c.camera(eph, t, DMat3::IDENTITY);
        c.reorient(DMat3::IDENTITY, turned);
        let after = c.camera(eph, t, turned);
        assert!(
            (after.eye - before.eye).length() < 1e-6,
            "{}",
            (after.eye - before.eye).length()
        );
        let later = DMat3::from_rotation_z(0.2) * turned;
        let moved = c.camera(eph, t, later);
        assert!((moved.forward - DMat3::from_rotation_z(0.2) * after.forward).length() < 1e-9);
        assert!(moved.up.dot(later.z_axis) > 0.9);
    }
}
