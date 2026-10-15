//! How bodies look (M1c.md, step 3 plan, item 3; step 6 plan, item 4):
//! spheres turned by their rotation models, with their maps once read
//! (`maps`); without, Lambertian of their Bond albedo, in their color
//! (`photometry::color`); each held to its published brightness and
//! color where it has a phase curve (`photometry`), which leaves of its
//! albedo only how its map's parts compare. And the Sun's radiance.

use crate::photometry;
use glam::{DMat3, DVec3};
use orbit::ephem::{Body, BodyId, BodyKind, Ephemeris};
use orbit::rotation::Rotations;
use orbit::time::Epoch;
use render::scene::AU_KM;
use render::{Sphere, Surface};

/// The Bond albedo of the body `id`: for a body with a phase curve, its
/// exposure before its maps are read. NASA's planetary fact sheets
/// (Phobos and Deimos: geometric albedo, 0.071 and 0.068); small bodies
/// without a measured one take 0.1.
pub fn bond_albedo(id: &str) -> f64 {
    match id {
        "mercury" => 0.088,
        "venus" => 0.76,
        "earth" => 0.306,
        "moon" => 0.11,
        "mars" => 0.25,
        "jupiter" => 0.343,
        "saturn" => 0.342,
        "uranus" => 0.3,
        "neptune" => 0.29,
        "phobos" => 0.071,
        "deimos" => 0.068,
        _ => 0.1,
    }
}

/// The albedo per channel: the Bond albedo, in the body's color.
pub fn albedo(body: &Body) -> [f32; 3] {
    let color = photometry::color(body).unwrap_or([1.0; 3]);
    color.map(|c| (bond_albedo(&body.id) * c) as f32)
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
/// brought to its measured brightness and color as seen from `eye`
/// (`photometry`).
pub fn spheres(eph: &Ephemeris, bodies: &[BodyId], t: Epoch, eye: DVec3) -> Vec<Sphere> {
    let sun = eph.barycentric_position(eph.sun(), t);
    bodies
        .iter()
        .filter_map(|&b| {
            let mut s = sphere(eph, b, t)?;
            photometry::calibrate(eph.body(b), &mut s, eye, sun);
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
            None => Surface::Lit(albedo(eph.body(b))),
        }
    };
    Some(Sphere {
        center: eph.barycentric_position(b, t),
        radius: eph.body(b).radius,
        surface,
        axes: Rotations::standard().axes(id, t).unwrap_or(DMat3::IDENTITY),
        maps: maps.map(|m| m.maps.clone()),
        gain: [1.0; 3],
        point_scale: [1.0; 3],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn albedos_keep_their_luminance() {
        let eph = Ephemeris::standard();
        let a = albedo(eph.body(eph.lookup("mars").unwrap()));
        let luminance = 0.2126 * a[0] + 0.7152 * a[1] + 0.0722 * a[2];
        assert!((luminance - 0.25).abs() < 1e-3, "{luminance}");
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
