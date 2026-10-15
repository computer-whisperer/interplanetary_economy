//! The bodies' brightness and color (M1c.md, Realism): each body's
//! magnitude at 1 au from the Sun and the observer by its phase angle,
//! from published phase curves; its color at full phase, from published
//! spectra (`tools/photometry/colors.py`); and the gain that brings a
//! sphere's shading to both (`render::Sphere::gain`).
//!
//! The planets' curves are Mallama and Hilton 2018 ("Computing apparent
//! planetary magnitudes for The Astronomical Almanac", Astronomy and
//! Computing 25, 10-24; arXiv 1808.01973), as JPL Horizons computes them,
//! but Earth's: Robinson 2025 (PSJ; arXiv 2507.22258), fitted to
//! Earthshine and spacecraft photometry. Mallama's full phase was
//! extrapolated by a model that swapped forward and backward scattering
//! (Robinson's Sec. 6.1): it is 0.77 mag brighter at full phase, 0.91
//! at 30° and 0.68 at 90°; past ~116° it is the fainter, by 1.2 mag at
//! 144°. The Moon's is Allen's, as Horizons'. Phobos, Deimos and the
//! small bodies take the IAU's H, G system (Bowell et al. 1989) with
//! G = 0.15: Horizons applies no phase law to Phobos and Deimos.

use glam::DVec3;
use orbit::ephem::{Body, BodyKind};
use render::Sphere;
use render::scene::{AU_KM, disk_mean};

/// The Sun's V magnitude at 1 au, as the stars' fluxes take it.
pub const V_SUN: f64 = -26.74;

/// Uranus's flattening, for its planetographic latitudes.
const URANUS_FLATTENING: f64 = 0.0022927;

/// Earth's visual phase curve (Robinson 2025, Eq. 14): its disk's mean
/// intensity a Henyey-Greenstein phase function of the scattering angle
/// 180° − α, of asymmetry `EARTH_G`, normalized at full phase, times its
/// geometric albedo. The fit's visual (400-700 nm) albedo, 0.23, scaled
/// to V by the physical model's V over visual (0.226 / 0.242).
const EARTH_G: f64 = -0.33;
const EARTH_ALBEDO_V: f64 = 0.23 * 0.226 / 0.242;

/// Earth's mean radius, km, its disk's size for its albedo.
const EARTH_RADIUS_KM: f64 = 6371.0;

/// The slope parameter the small bodies take, the IAU's default.
const SLOPE: f64 = 0.15;

/// A disk's gain is held past this phase angle, degrees: a thinner
/// crescent's shading carries too little light to scale (Venus's gain is
/// 83 at 170°). Its point follows its curve on.
const DISK_MAX_PHASE: f64 = 170.0;

/// Where a body is seen from, as its curve needs it.
#[derive(Clone, Copy, Debug)]
pub struct Seen {
    /// The phase angle, degrees.
    pub alpha: f64,
    /// The observer's and the Sun's planetocentric latitudes over its
    /// equator, degrees.
    pub observer_lat: f64,
    pub sun_lat: f64,
}

impl Seen {
    /// `sphere` seen from `eye` with the Sun at `sun` (km, the scene's
    /// coordinates).
    pub fn from(sphere: &Sphere, eye: DVec3, sun: DVec3) -> Seen {
        let (to_eye, to_sun) = (eye - sphere.center, sun - sphere.center);
        let pole = sphere.axes.z_axis;
        let lat = |d: DVec3| d.normalize().dot(pole).clamp(-1.0, 1.0).asin().to_degrees();
        Seen {
            alpha: to_eye.angle_between(to_sun).to_degrees(),
            observer_lat: lat(to_eye),
            sun_lat: lat(to_sun),
        }
    }
}

/// A body's phase curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Mercury,
    Venus,
    Earth,
    Mars,
    Jupiter,
    /// The globe alone: the rings are drawn with their own photometry
    /// ([`saturn_with_rings`] checks them).
    Saturn,
    Uranus,
    Neptune,
    Moon,
    /// The H, G system: its absolute magnitude H.
    Small {
        h: f64,
    },
}

fn poly(x: f64, c: &[f64]) -> f64 {
    c.iter().rev().fold(0.0, |acc, &k| acc * x + k)
}

impl Curve {
    /// `body`'s curve, if it has one.
    pub fn of(body: &Body) -> Option<Curve> {
        Some(match body.id.as_str() {
            "mercury" => Curve::Mercury,
            "venus" => Curve::Venus,
            "earth" => Curve::Earth,
            "mars" => Curve::Mars,
            "jupiter" => Curve::Jupiter,
            "saturn" => Curve::Saturn,
            "uranus" => Curve::Uranus,
            "neptune" => Curve::Neptune,
            "moon" => Curve::Moon,
            // Horizons' V(1,0).
            "phobos" => Curve::Small { h: 11.8 },
            "deimos" => Curve::Small { h: 12.89 },
            _ => match body.kind {
                BodyKind::SmallBody { h_mag: Some(h), .. } => Curve::Small { h },
                _ => return None,
            },
        })
    }

    /// The phase angles the curve is taken over, degrees: where Horizons
    /// takes it (Mercury's, Venus's and the Moon's to 180°, past their
    /// observations: Mercury's span 2.1° to 169.5°), else to its
    /// observations' end. Past it, its body follows its shading's law.
    pub fn range(&self) -> (f64, f64) {
        match self {
            Curve::Mercury | Curve::Venus | Curve::Moon => (0.0, 180.0),
            // The data Robinson's fit is taken over.
            Curve::Earth => (0.0, 144.0),
            Curve::Mars => (0.0, 120.0),
            Curve::Jupiter => (0.0, 130.0),
            Curve::Saturn => (0.0, 150.0),
            Curve::Uranus => (0.0, 154.0),
            Curve::Neptune => (0.0, 133.0),
            Curve::Small { .. } => (0.0, 120.0),
        }
    }

    /// V(1, α): the magnitude at 1 au from the Sun and the observer.
    pub fn magnitude(&self, seen: &Seen) -> f64 {
        let a = seen.alpha;
        match *self {
            // Eq. 2.
            Curve::Mercury => poly(
                a,
                &[
                    -0.613,
                    6.3280e-2,
                    -1.6336e-3,
                    3.3644e-5,
                    -3.4265e-7,
                    1.6893e-9,
                    -3.0334e-12,
                ],
            ),
            // Eqs. 3 and 4: a glory near 0°, forward scattering past 163.7°.
            Curve::Venus if a <= 163.7 => {
                poly(a, &[-4.384, -1.044e-3, 3.687e-4, -2.814e-6, 8.938e-9])
            }
            Curve::Venus => poly(a, &[236.05828, -2.81914, 8.39034e-3]),
            Curve::Earth => {
                let g = EARTH_G;
                let phase =
                    ((1.0 + g * g + 2.0 * g * a.to_radians().cos()) / (1.0 + g).powi(2)).powf(-1.5);
                let size = (EARTH_RADIUS_KM / AU_KM).powi(2);
                V_SUN - 2.5 * (EARTH_ALBEDO_V * size * phase).log10()
            }
            // Eqs. 6 and 7, without the terms for the longitude seen and
            // the season (to 0.12 mag against Horizons; the map carries
            // its longitudes).
            Curve::Mars if a <= 50.0 => poly(a, &[-1.601, 0.02267, -0.0001302]),
            Curve::Mars => poly(a, &[-0.367, -0.02573, 0.0003445]),
            // Eqs. 8 and 9 (Cassini's, past 12°).
            Curve::Jupiter if a <= 12.0 => poly(a, &[-9.395, -3.7e-4, 6.16e-4]),
            Curve::Jupiter => {
                -9.428
                    - 2.5 * poly(a / 180.0, &[1.0, -1.507, -0.363, -0.062, 2.809, -1.876]).log10()
            }
            // Eqs. 11 and 12 (Pioneer's, past 6°).
            Curve::Saturn if a <= 6.0 => poly(a, &[-8.95, -3.7e-4, 6.16e-4]),
            Curve::Saturn => poly(a, &[-8.94, 2.446e-4, 2.672e-4, -1.505e-6, 4.767e-9]),
            // Eqs. 14 and 15: the poles brighter; Voyager's phase curve
            // past what Earth sees (3.2°), as Horizons takes them.
            Curve::Uranus => {
                let graphic = |lat: f64| {
                    (lat.to_radians().tan() / (1.0 - URANUS_FLATTENING).powi(2))
                        .atan()
                        .to_degrees()
                        .abs()
                };
                let phi = (graphic(seen.observer_lat) + graphic(seen.sun_lat)) / 2.0;
                let phase = if a <= 3.2 {
                    0.0
                } else {
                    poly(a, &[0.0, 6.587e-3, 1.045e-4])
                };
                -7.110 - 8.4e-4 * phi + phase
            }
            // Eqs. 16 (after 2000) and 17: Voyager's past what Earth sees
            // (1.9°), as Horizons takes them.
            Curve::Neptune if a <= 1.9 => -7.00,
            Curve::Neptune => poly(a, &[-7.00, 7.944e-3, 9.617e-5]),
            Curve::Moon => 0.23 + 0.026 * a + 4e-9 * a.powi(4),
            Curve::Small { h } => {
                let t = (a.to_radians() / 2.0).tan();
                let phi1 = (-3.33 * t.powf(0.63)).exp();
                let phi2 = (-1.87 * t.powf(1.22)).exp();
                h - 2.5 * ((1.0 - SLOPE) * phi1 + SLOPE * phi2).log10()
            }
        }
    }
}

/// Saturn's magnitude with its rings (Eq. 10), where it holds: under 6.5°
/// of phase, the rings opened under 27°. The opening is the geometric
/// mean of the observer's and the Sun's latitudes; 0 (the rings' unlit
/// side seen) where they differ in sign.
pub fn saturn_with_rings(seen: &Seen) -> Option<f64> {
    let (e, s) = (seen.observer_lat, seen.sun_lat);
    let beta = if e * s > 0.0 { (e * s).sqrt() } else { 0.0 };
    if seen.alpha >= 6.5 || beta >= 27.0 {
        return None;
    }
    let sin = beta.to_radians().sin();
    Some(-8.914 - 1.825 * sin + 0.026 * seen.alpha - 0.378 * sin * (-2.25 * seen.alpha).exp())
}

/// What Saturn's rings add to its globe's light, over it, where Eq. 10
/// holds (from Earth's vantage, its share may be negative: Eq. 10's
/// globe is dimmer than Eq. 11's); else none known, 0 (a step at 6.5° of
/// phase, to 0.6 mag with the rings open). 0 for other bodies.
fn rings_share(curve: &Curve, seen: &Seen) -> f64 {
    match (curve, saturn_with_rings(seen)) {
        (Curve::Saturn, Some(both)) => 10f64.powf(-0.4 * (both - curve.magnitude(seen))) - 1.0,
        _ => 0.0,
    }
}

/// Linear values' luminance.
fn luminance(c: [f32; 3]) -> f64 {
    0.2126 * f64::from(c[0]) + 0.7152 * f64::from(c[1]) + 0.0722 * f64::from(c[2])
}

/// The Sun's color, for a body whose own is not measured.
const SUN_COLOR: [f64; 3] = [1.0; 3];

/// `body`'s color at full phase: what it reflects of the Sun's light, as
/// linear sRGB with the Sun's white, luminance 1, from the spectra
/// `tools/photometry/colors.py` names (and prints these from): the
/// planets' (their two photometric systems alone differing by up to
/// 0.09 in a channel; Earth's as its curve's source has it) and the
/// Moon's measured whole; Phobos's and Deimos's like D-type asteroids';
/// a small body's its class's mean, or the Sun's where its class is
/// unknown. None for the Sun.
pub fn color(body: &Body) -> Option<[f64; 3]> {
    Some(match body.id.as_str() {
        "mercury" => [1.141, 0.981, 0.768],
        "venus" => [1.015, 1.002, 0.938],
        "earth" => [0.946, 0.990, 1.262],
        "mars" => [1.414, 0.928, 0.491],
        "jupiter" => [1.030, 1.005, 0.860],
        "saturn" => [1.212, 0.967, 0.700],
        "uranus" => [0.676, 1.072, 1.243],
        "neptune" => [0.636, 1.072, 1.356],
        "moon" => [1.165, 0.973, 0.778],
        "phobos" | "deimos" => [1.070, 0.990, 0.890],
        _ => match &body.kind {
            BodyKind::Star => return None,
            // Its class, without a subclass's suffix (S(IV): S).
            BodyKind::SmallBody { spectral, .. } => match spectral.split('(').next() {
                Some("B") => [1.007, 1.000, 0.978],
                Some("C") => [1.014, 1.000, 0.963],
                Some("Cb") => [0.998, 1.000, 1.009],
                Some("D") => [1.070, 0.990, 0.890],
                Some("S") => [1.106, 0.987, 0.818],
                Some("Sq") => [1.095, 0.989, 0.827],
                Some("Xe") => [1.072, 0.989, 0.891],
                _ => SUN_COLOR,
            },
            _ => SUN_COLOR,
        },
    })
}

/// The flux of a curve's body at 1 au from the Sun and the observer, over
/// π (the stars' units: radiance units × sr / π).
fn true_flux(curve: &Curve, seen: &Seen) -> f64 {
    10f64.powf(-0.4 * (curve.magnitude(seen) - V_SUN))
}

/// What `sphere`'s shading gives it there, over π.
fn shaded_flux(sphere: &Sphere, alpha: f64) -> Option<f64> {
    let (albedo, shading) = sphere.reflectance()?;
    let size = (sphere.radius / AU_KM).powi(2);
    Some(luminance(albedo) * disk_mean(shading, alpha.to_radians()) * size)
}

/// The factor that brings `sphere`'s shading, seen as `seen`, to
/// `curve`'s brightness, with the phase angle held to `[lo, hi]` (the
/// shading's phase law beyond). 1 for a glowing sphere.
fn factor(curve: &Curve, sphere: &Sphere, seen: &Seen, (lo, hi): (f64, f64)) -> f64 {
    let seen = Seen {
        alpha: seen.alpha.clamp(lo, hi),
        ..*seen
    };
    match shaded_flux(sphere, seen.alpha) {
        Some(shaded) if shaded > 0.0 => true_flux(curve, &seen) / shaded,
        _ => 1.0,
    }
}

/// The factor that brings `sphere`'s disk, seen as `seen`, to `curve`'s
/// brightness: held past the curve's range, and past [`DISK_MAX_PHASE`].
fn disk_factor(curve: &Curve, sphere: &Sphere, seen: &Seen) -> f64 {
    let (lo, hi) = curve.range();
    factor(curve, sphere, seen, (lo, hi.min(DISK_MAX_PHASE)))
}

/// What brings `sphere`'s reflectance as a whole to `color`, per channel,
/// its luminance kept: its maps' parts keep their colors over their
/// mean's. None where it reflects none of a channel.
fn tint(color: [f64; 3], sphere: &Sphere) -> Option<[f64; 3]> {
    let (albedo, _) = sphere.reflectance()?;
    if albedo.iter().any(|&a| a <= 0.0) {
        return None;
    }
    let lum = |c: [f64; 3]| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let k = luminance(albedo) / lum(color);
    Some(std::array::from_fn(|c| color[c] * k / f64::from(albedo[c])))
}

/// Whether `body`'s color is its air's, drawn on its disk over its maps:
/// Earth's blue, Rayleigh scattering's over white clouds. Its point,
/// which has no air, takes its color; its disk keeps its maps'.
fn colored_by_air(body: &Body) -> bool {
    body.id == "earth"
}

/// Brings `sphere`, `body` seen from `eye` with the Sun at `sun`, to its
/// measured brightness and color where it has them: its gain
/// (`render::Sphere::gain`) to its curve's brightness ([`disk_factor`])
/// and its color ([`color`]); its point's scale
/// (`render::Sphere::point_scale`) to what its point carries over its
/// disk: Saturn's rings' light, which is drawn only on its disk; its
/// curve's past [`DISK_MAX_PHASE`]; and the color of a body whose disk's
/// air gives it ([`colored_by_air`]).
pub fn calibrate(body: &Body, sphere: &mut Sphere, eye: DVec3, sun: DVec3) {
    let (Some(curve), Some(color)) = (Curve::of(body), color(body)) else {
        return;
    };
    let seen = Seen::from(sphere, eye, sun);
    let disk = disk_factor(&curve, sphere, &seen);
    let point =
        factor(&curve, sphere, &seen, curve.range()) / disk * (1.0 + rings_share(&curve, &seen));
    let tint = tint(color, sphere).unwrap_or([1.0; 3]);
    let (on_disk, on_point) = match colored_by_air(body) {
        true => ([1.0; 3], tint),
        false => (tint, [1.0; 3]),
    };
    sphere.gain = on_disk.map(|t| (disk * t) as f32);
    sphere.point_scale = on_point.map(|t| (point * t) as f32);
}

/// The albedo of a white, Lambertian surface that would show as `sphere`'s
/// sunlit side does at full phase, its mean (by its curve, else its
/// shading's): what the exposure is set by.
pub fn normal_albedo(curve: Option<&Curve>, sphere: &Sphere, seen: &Seen) -> Option<f64> {
    let (albedo, _) = sphere.reflectance()?;
    let full = Seen {
        alpha: 0.0,
        ..*seen
    };
    let k = curve.map_or(1.0, |c| disk_factor(c, sphere, &full));
    Some(luminance(albedo) * k)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The H, G system at 0° is H; at 30°, ~1.2 mag fainter for G = 0.15.
    #[test]
    fn small_bodies_dim_with_phase() {
        let seen = |alpha| Seen {
            alpha,
            observer_lat: 0.0,
            sun_lat: 0.0,
        };
        let c = Curve::Small { h: 18.0 };
        assert!((c.magnitude(&seen(0.0)) - 18.0).abs() < 1e-12);
        let d = c.magnitude(&seen(30.0)) - 18.0;
        assert!(d > 1.0 && d < 1.4, "{d}");
    }
}
