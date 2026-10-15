//! How bodies look (M1c.md, step 3 plan, item 3; step 6 plan, item 4):
//! spheres turned by their rotation models, with their maps once read
//! (`maps`); without, Lambertian of their Bond albedo, tinted. And the
//! Sun's radiance.

use crate::photometry::{self, Curve, Seen};
use glam::{DMat3, DVec3};
use orbit::ephem::{BodyId, BodyKind, Ephemeris};
use orbit::rotation::Rotations;
use orbit::time::Epoch;
use render::scene::AU_KM;
use render::{Sphere, Surface};

/// Bond albedo and a tint (linear RGB, scaled to the albedo's luminance)
/// by body id. Albedos: NASA's planetary fact sheets (Phobos and Deimos:
/// geometric albedo, 0.071 and 0.068); small bodies without a measured
/// one take 0.1. Tints are rough: a body with maps takes its color from
/// them.
fn look(id: &str) -> (f64, [f64; 3]) {
    match id {
        "mercury" => (0.088, [1.0, 0.92, 0.85]),
        "venus" => (0.76, [1.0, 0.94, 0.78]),
        "earth" => (0.306, [0.75, 0.85, 1.0]),
        "moon" => (0.11, [1.0, 0.96, 0.92]),
        "mars" => (0.25, [1.0, 0.62, 0.42]),
        "jupiter" => (0.343, [1.0, 0.9, 0.75]),
        "saturn" => (0.342, [1.0, 0.92, 0.74]),
        "uranus" => (0.3, [0.75, 0.95, 1.0]),
        "neptune" => (0.29, [0.6, 0.74, 1.0]),
        "phobos" => (0.071, [1.0, 0.95, 0.9]),
        "deimos" => (0.068, [1.0, 0.95, 0.9]),
        _ => (0.1, [1.0, 1.0, 1.0]),
    }
}

/// The Bond albedo of the body `id`.
pub fn bond_albedo(id: &str) -> f64 {
    look(id).0
}

/// The albedo per channel: the Bond albedo, tinted.
pub fn albedo(id: &str) -> [f32; 3] {
    let (bond, tint) = look(id);
    let luminance = 0.2126 * tint[0] + 0.7152 * tint[1] + 0.0722 * tint[2];
    tint.map(|c| (bond * c / luminance) as f32)
}

/// The bodies drawn, in the order the focus select lists them: the Sun,
/// planets and moons, then small bodies with a known size.
pub fn drawn(eph: &Ephemeris) -> Vec<BodyId> {
    let mut ids: Vec<BodyId> = eph.ids().filter(|&b| eph.body(b).radius > 0.0).collect();
    ids.sort_by_key(|&b| matches!(eph.body(b).kind, BodyKind::SmallBody { .. }));
    ids
}

/// The Sun's limb darkening, linear-law coefficients in red, green and
/// blue: approximate fits to its center-to-limb profiles near 610, 540 and
/// 465 nm (Neckel and Labs 1994), the limb at μ = 0.1 about 0.46, 0.40
/// and 0.33 of the center.
pub const SUN_DARKENING: [f32; 3] = [0.60, 0.67, 0.75];

/// The Sun's radiance: a white Lambertian surface under the Sun at 1 au
/// has radiance E/π; the Sun's disk E/Ω, Ω = π (R☉ / 1 au)².
pub fn sun_radiance(eph: &Ephemeris) -> f64 {
    (AU_KM / eph.body(eph.sun()).radius).powi(2)
}

/// `bodies` as spheres at `t` (those the ephemeris covers then), each
/// brought to its measured brightness as seen from `eye`
/// (`photometry`).
pub fn spheres(eph: &Ephemeris, bodies: &[BodyId], t: Epoch, eye: DVec3) -> Vec<Sphere> {
    let sun = eph.barycentric_position(eph.sun(), t);
    bodies
        .iter()
        .filter_map(|&b| {
            let mut s = sphere(eph, b, t)?;
            if let Some(curve) = Curve::of(eph.body(b)) {
                let seen = Seen::from(&s, eye, sun);
                s.gain = photometry::gain(&curve, &s, &seen);
                s.rings_share = photometry::rings_share(&curve, &seen) as f32;
            }
            Some(s)
        })
        .collect()
}

/// `b` as a sphere at `t`, if the ephemeris covers it then, with no gain.
pub fn sphere(eph: &Ephemeris, b: BodyId, t: Epoch) -> Option<Sphere> {
    if !eph.covers(b, t) {
        return None;
    }
    let id = &eph.body(b).id;
    let maps = crate::maps::loaded(id);
    let surface = if b == eph.sun() {
        Surface::Emissive {
            radiance: [sun_radiance(eph) as f32; 3],
            darkening: SUN_DARKENING,
        }
    } else {
        match maps {
            Some(m) => Surface::Lit(m.albedo),
            None => Surface::Lit(albedo(id)),
        }
    };
    Some(Sphere {
        center: eph.barycentric_position(b, t),
        radius: eph.body(b).radius,
        surface,
        axes: Rotations::standard().axes(id, t).unwrap_or(DMat3::IDENTITY),
        maps: maps.map(|m| m.maps.clone()),
        gain: 1.0,
        rings_share: 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn albedos_keep_their_luminance() {
        let a = albedo("mars");
        let luminance = 0.2126 * a[0] + 0.7152 * a[1] + 0.0722 * a[2];
        assert!((luminance - 0.25).abs() < 1e-6);
    }

    #[test]
    fn drawn_bodies_lead_with_the_sun_and_have_sizes() {
        let eph = Ephemeris::standard();
        let ids = drawn(eph);
        assert_eq!(ids[0], eph.sun());
        assert!(ids.iter().all(|&b| eph.body(b).radius > 0.0));
        let t = Epoch::from_ymd(2030, 1, 1);
        let bennu = eph.lookup("bennu").unwrap();
        assert!(sphere(eph, bennu, t).is_none(), "before its integration");
        assert!(sphere(eph, eph.lookup("moon").unwrap(), t).is_some());
    }
}
