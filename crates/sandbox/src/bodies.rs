//! How bodies look until step 6's textures (M1c.md, step 3 plan, item
//! 3): Lambertian spheres of their Bond albedo, tinted, and the Sun's
//! radiance.

use orbit::ephem::{BodyId, BodyKind, Ephemeris};
use orbit::time::Epoch;
use render::scene::AU_KM;
use render::{Sphere, Surface};

/// Bond albedo and a tint (linear RGB, scaled to the albedo's luminance)
/// by body id. Albedos: NASA's planetary fact sheets (Phobos and Deimos:
/// geometric albedo, 0.071 and 0.068); small bodies without a measured
/// one take 0.1. Tints are rough until textures come.
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

/// The albedo per channel: the Bond albedo, tinted.
fn albedo(id: &str) -> [f32; 3] {
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

/// `b` as a sphere at `t`, if the ephemeris covers it then.
pub fn sphere(eph: &Ephemeris, b: BodyId, t: Epoch) -> Option<Sphere> {
    if !eph.covers(b, t) {
        return None;
    }
    let radius = eph.body(b).radius;
    let surface = if b == eph.sun() {
        // A white Lambertian surface under the Sun at 1 au has radiance
        // E/π; the Sun's disk E/Ω, Ω = π (R☉ / 1 au)².
        Surface::Emissive([(AU_KM / radius).powi(2) as f32; 3])
    } else {
        Surface::Lit(albedo(&eph.body(b).id))
    };
    Some(Sphere {
        center: eph.barycentric_position(b, t),
        radius,
        surface,
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
