//! How the map view's radiance is shown (M1c.md, step 6 plan, item 1,
//! and Realism): one exposure for the whole scene, set by the focus,
//! eased when it changes and shifted by whole stops by hand, with the sky
//! lifted over it; points of light as bright as at the viewer's eye; and
//! bloom.

use crate::bodies;
use orbit::ephem::{BodyId, Ephemeris};
use orbit::time::Epoch;
use render::scene::AU_KM;

/// The level the focus's sunlit side shows at, at normal incidence (1.0
/// is the SDR white): brighter surfaces than its mean, clouds and ice,
/// reach into the headroom.
pub const KEY: f64 = 0.6;

/// The level the Sun's disk shows at (its mean), its focus: its center
/// (×1.25, limb-darkened) past the tonemap's knee and into the headroom
/// on HDR (gate item 4), its limb below the knee. At [`KEY`] it read as
/// gray; at 4 × KEY its limb darkening was lost in the shoulder.
pub const SUN_KEY: f64 = 2.0 * KEY;

/// The angle a logical pixel subtends at the viewer's eye, radians
/// (`Look::pixel`): CSS's reference pixel, 1/96 inch at 28 inches
/// (0.0213°), which hosts' display scales approximate.
pub const PIXEL: f64 = 1.0 / (96.0 * 28.0);

/// The departures from realism (Realism), each a setting in stops, 0
/// being true to life, and their defaults.
///
/// Stops the sky (the stars and the Milky Way) shows over the scene's
/// exposure, so that it shows beside a sunlit body. Over Earth's
/// exposure, +17 shows stars to about magnitude 11.
pub const BACKGROUND_LIFT: i32 = 17;

/// Stops bodies drawn as points of light show over the scene's exposure:
/// as the sky's, so that the planets show where they are, at their
/// brightness among the stars.
pub const BODY_LIFT: i32 = BACKGROUND_LIFT;

/// The most either is lifted: enough for the Milky Way to show clearly
/// (+22 over Earth's exposure) with any planet in focus (Venus's exposure
/// is 2.2 stops under Earth's). With the Sun's, 16 stops under, the
/// exposure keys reach further.
pub const MAX_LIFT: i32 = 24;

/// Stops Earth's night lights show over their radiance, so that they
/// show at the day side's exposure.
pub const NIGHT_LIGHTS: i32 = 14;

/// The most they are lifted: 6 stops past the default.
pub const MAX_NIGHT_LIGHTS: i32 = 20;

/// Seconds for exposure to close ~63% of the way to a new focus's: most
/// of it within a refocus's flight.
const EASE_S: f64 = 0.2;

/// The share of the image's energy bloom spreads: a lens's glare. At 3%
/// a body at the key level showed a haze around it; at 1% only the Sun
/// and bright limbs glare.
pub const BLOOM: f32 = 0.01;

/// The exposure that shows `focus`'s sunlit side at [`KEY`] at `t`: KEY
/// / (Bond albedo × irradiance in units of 1 au's). The Sun's shows its
/// disk at [`SUN_KEY`].
pub fn exposure_for(eph: &Ephemeris, focus: BodyId, t: Epoch) -> f64 {
    if focus == eph.sun() {
        return SUN_KEY / bodies::sun_radiance(eph);
    }
    let to_sun = eph.barycentric_position(eph.sun(), t) - eph.barycentric_position(focus, t);
    let irradiance = (AU_KM / to_sun.length()).powi(2);
    KEY / (bodies::bond_albedo(&eph.body(focus).id) * irradiance)
}

/// Exposure as shown: eased toward the focus's, in stops.
#[derive(Clone, Copy, Debug, Default)]
pub struct Exposure {
    /// log₂ of the exposure now, once ticked.
    stops: Option<f64>,
    /// Whole stops added by hand.
    pub bias: i32,
}

impl Exposure {
    /// Eases toward `target` over `dt` seconds of wall time; the first
    /// tick takes it at once.
    pub fn tick(&mut self, target: f64, dt: f64) {
        let to = target.log2();
        self.stops = Some(match self.stops {
            Some(s) => s + (to - s) * (1.0 - (-dt / EASE_S).exp()),
            None => to,
        });
    }

    /// The exposure now; `target` before the first tick (a screenshot).
    pub fn value(&self, target: f64) -> f32 {
        let stops = self.stops.unwrap_or_else(|| target.log2()) + f64::from(self.bias);
        stops.exp2() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_eases_and_shifts_by_stops() {
        let mut e = Exposure::default();
        assert_eq!(e.value(4.0), 4.0);
        e.tick(4.0, 0.016);
        e.tick(64.0, EASE_S);
        // From 2 stops to 6: after one time constant, 1 - 1/e of the way.
        let stops = f64::from(e.value(64.0)).log2();
        assert!(
            (stops - (2.0 + 4.0 * (1.0 - (-1f64).exp()))).abs() < 1e-5,
            "{stops}"
        );
        for _ in 0..300 {
            e.tick(64.0, 0.016);
        }
        assert!((e.value(64.0) - 64.0).abs() < 1e-3);
        e.bias = -2;
        assert!((e.value(64.0) - 16.0).abs() < 1e-3);
    }

    #[test]
    fn the_focus_shows_at_the_key() {
        let eph = Ephemeris::standard();
        let t = Epoch::from_ymd(2045, 3, 1);
        let earth = eph.lookup("earth").unwrap();
        let jupiter = eph.lookup("jupiter").unwrap();
        // Earth near 1 au: ~KEY / 0.306.
        let e = exposure_for(eph, earth, t);
        assert!((e * 0.306 / KEY - 1.0).abs() < 0.04, "{e}");
        // Jupiter at ~5.2 au and albedo 0.343: ~27 × brighter exposure
        // than Earth's, with its albedo's ratio.
        let ratio = exposure_for(eph, jupiter, t) / e * 0.343 / 0.306;
        assert!(ratio > 24.0 && ratio < 31.0, "{ratio}");
        assert_eq!(
            exposure_for(eph, eph.sun(), t) * bodies::sun_radiance(eph),
            SUN_KEY
        );
    }
}
