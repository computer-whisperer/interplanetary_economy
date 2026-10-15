//! Bodies' orientations (M1c.md, step 6 plan, item 4): the IAU WGCCRE
//! 2015 rotation models (`data/rotation.toml`, built by `tools/rotation/`
//! from NAIF's pck00011), evaluated as SPICE evaluates them.
//!
//! A body's frame: z along its north pole, x through its prime meridian
//! (longitude 0), y at 90° east. The models give the pole in the ICRS and
//! the meridian's angle W along the equator from the node of the body's
//! equator on the ICRS equator; [`Rotations::axes`] gives the frame in the
//! ephemeris's (ecliptic J2000).
//!
//! Earth's meridian is the IERS Earth rotation angle at UT1 instead: the
//! IAU's linear formula is 0.05° off it at J2000 itself.

use crate::ephem::OBLIQUITY_J2000;
use crate::time::Epoch;
use glam::{DMat3, DVec3};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

/// TT − UT1, s, for Earth's rotation: its 2024 value. It cannot be
/// predicted; each 10 s is 0.04° of Earth's turn.
pub const DELTA_T_S: f64 = 69.0;

/// The Earth rotation angle (IERS Conventions 2010, eq. 5.15), radians, at
/// `ut1` days from J2000 UT1.
pub fn earth_rotation_angle(ut1: f64) -> f64 {
    std::f64::consts::TAU * (0.779_057_273_264 + 1.002_737_811_911_354_5 * ut1).rem_euclid(1.0)
}

const DAYS_PER_CENTURY: f64 = 36_525.0;

#[derive(Deserialize)]
struct File {
    body: HashMap<String, Model>,
    #[serde(default)]
    system: HashMap<String, System>,
}

/// One body's model: polynomials and periodic terms, degrees (see
/// `tools/rotation/build.py`).
#[derive(Clone, Debug, Deserialize)]
struct Model {
    ra: [f64; 3],
    dec: [f64; 3],
    pm: [f64; 3],
    system: Option<u32>,
    #[serde(default)]
    ra_terms: Vec<f64>,
    #[serde(default)]
    dec_terms: Vec<f64>,
    #[serde(default)]
    pm_terms: Vec<f64>,
}

/// A planetary system's angles for its bodies' periodic terms: degrees,
/// per century, per century squared.
#[derive(Clone, Debug, Deserialize)]
struct System {
    angles: Vec<[f64; 3]>,
}

/// The models of the game's bodies, by ephemeris id.
#[derive(Clone, Debug)]
pub struct Rotations {
    models: HashMap<String, (Model, Vec<[f64; 3]>)>,
}

impl Rotations {
    /// `data/rotation.toml`, read once per process.
    pub fn standard() -> &'static Rotations {
        static STANDARD: OnceLock<Rotations> = OnceLock::new();
        STANDARD.get_or_init(|| {
            Rotations::parse(include_str!("../../../data/rotation.toml"))
                .expect("data/rotation.toml is valid")
        })
    }

    pub fn parse(src: &str) -> Result<Rotations, String> {
        let file: File = toml::from_str(src).map_err(|e| e.to_string())?;
        let mut models = HashMap::new();
        for (id, model) in file.body {
            let angles = match model.system {
                Some(s) => file
                    .system
                    .get(&s.to_string())
                    .ok_or_else(|| format!("{id}: no system {s}"))?
                    .angles
                    .clone(),
                None => Vec::new(),
            };
            let longest = model
                .ra_terms
                .len()
                .max(model.dec_terms.len())
                .max(model.pm_terms.len());
            if longest > angles.len() {
                return Err(format!("{id}: {longest} terms, {} angles", angles.len()));
            }
            models.insert(id, (model, angles));
        }
        Ok(Rotations { models })
    }

    /// Whether `id` has a model.
    pub fn knows(&self, id: &str) -> bool {
        self.models.contains_key(id)
    }

    /// The pole's right ascension and declination and the prime
    /// meridian's angle, radians, at `t` (TDB).
    pub fn angles(&self, id: &str, t: Epoch) -> Option<(f64, f64, f64)> {
        let (m, angles) = self.models.get(id)?;
        let d = t.0;
        let c = d / DAYS_PER_CENTURY;
        let theta: Vec<f64> = angles
            .iter()
            .map(|a| (a[0] + a[1] * c + a[2] * c * c).to_radians())
            .collect();
        let sum = |terms: &[f64], f: fn(f64) -> f64| -> f64 {
            terms.iter().zip(&theta).map(|(k, &th)| k * f(th)).sum()
        };
        let ra = m.ra[0] + m.ra[1] * c + m.ra[2] * c * c + sum(&m.ra_terms, f64::sin);
        let dec = m.dec[0] + m.dec[1] * c + m.dec[2] * c * c + sum(&m.dec_terms, f64::cos);
        let w = if id == "earth" {
            // The rotation angle runs from the intermediate origin, near
            // the ICRS's x axis; W from the node at α₀ + 90°.
            let ut1 = d - DELTA_T_S / crate::time::SECONDS_PER_DAY;
            earth_rotation_angle(ut1).to_degrees() - 90.0 - ra
        } else {
            m.pm[0] + m.pm[1] * d + m.pm[2] * d * d + sum(&m.pm_terms, f64::sin)
        };
        Some((ra.to_radians(), dec.to_radians(), w.to_radians()))
    }

    /// The body's frame at `t`, as columns in the ephemeris's frame: x
    /// through the prime meridian, y 90° east, z the north pole.
    pub fn axes(&self, id: &str, t: Epoch) -> Option<DMat3> {
        let (ra, dec, w) = self.angles(id, t)?;
        // ICRS to the body: Rz(W) Rx(90° − δ) Rz(90° + α); its inverse's
        // columns are the body's axes in the ICRS.
        let to_body = DMat3::from_rotation_z(-w)
            * DMat3::from_rotation_x(-(std::f64::consts::FRAC_PI_2 - dec))
            * DMat3::from_rotation_z(-(std::f64::consts::FRAC_PI_2 + ra));
        Some(equatorial_to_ecliptic() * to_body.transpose())
    }

    /// The body's north pole at `t`, in the ephemeris's frame.
    pub fn pole(&self, id: &str, t: Epoch) -> Option<DVec3> {
        self.axes(id, t).map(|a| a.z_axis)
    }
}

/// From the ICRS to the ephemeris's frame (ecliptic J2000): a turn of the
/// obliquity about their shared x axis.
pub fn equatorial_to_ecliptic() -> DMat3 {
    DMat3::from_rotation_x(-OBLIQUITY_J2000)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pole from right ascension and declination, and the axes' handedness.
    #[test]
    fn axes_are_a_rotation_with_the_pole_on_z() {
        let r = Rotations::standard();
        let t = Epoch::from_ymd(2050, 6, 1);
        for id in [
            "sun", "earth", "moon", "mars", "phobos", "jupiter", "neptune", "venus",
        ] {
            let axes = r.axes(id, t).unwrap();
            assert!((axes.determinant() - 1.0).abs() < 1e-12, "{id}");
            assert!((axes.transpose() * axes).abs_diff_eq(DMat3::IDENTITY, 1e-12));
            let (ra, dec, _) = r.angles(id, t).unwrap();
            let pole = equatorial_to_ecliptic()
                * DVec3::new(dec.cos() * ra.cos(), dec.cos() * ra.sin(), dec.sin());
            assert!((axes.z_axis - pole).length() < 1e-12, "{id}");
        }
    }

    /// Greenwich is where the Earth rotation angle puts it: at 2000-01-01
    /// 12:00 UT1 (the angle 280.46062°, along the equator from near the
    /// ICRS's x axis).
    #[test]
    fn greenwich_is_at_the_rotation_angle() {
        let r = Rotations::standard();
        let t = Epoch(DELTA_T_S / crate::time::SECONDS_PER_DAY);
        let greenwich = equatorial_to_ecliptic().transpose() * r.axes("earth", t).unwrap().x_axis;
        let ra = greenwich
            .y
            .atan2(greenwich.x)
            .to_degrees()
            .rem_euclid(360.0);
        assert!((ra - 280.460_62).abs() < 1e-4, "{ra}°");
    }
}
