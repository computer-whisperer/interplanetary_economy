//! Landing, ascent and passes through an atmosphere (M1a.md, Flight
//! rules): not integrated powered flight, but Δv rules fitted to flown and
//! published values.
//!
//! Landing or ascending between a low circular orbit and rest on the
//! surface costs the ideal two-burn transfer (the surface's rotation not
//! credited) plus a loss per body and direction. An aero-capable ship can
//! instead pass through an atmosphere: aerocapture brakes an arrival into an
//! orbit reaching up to a low orbit, which a burn there circularizes; entry
//! brakes it to the surface, leaving a terminal propulsive descent. Both
//! limit the speed at the pass.

use crate::ephem::{BodyId, Ephemeris};

/// Losses beyond the ideal transfer, km/s, and what an atmosphere offers.
#[derive(Clone, Copy, Debug)]
pub struct Rules {
    pub descent_loss: f64,
    pub ascent_loss: f64,
    pub atmosphere: Option<Atmosphere>,
}

/// An atmosphere's passes, for aero-capable ships.
#[derive(Clone, Copy, Debug)]
pub struct Atmosphere {
    /// Altitude of the periapsis of an aerocapture pass (its vacuum conic),
    /// and of an entry, km.
    pub capture_altitude: f64,
    pub entry_altitude: f64,
    /// The propulsive descent left after entry, km/s.
    pub terminal_dv: f64,
    /// The fastest pass a ship survives, km/s at periapsis.
    pub max_speed: f64,
}

/// The rules for `body`, by id: the Moon and Mars fitted to flown and
/// published values, Earth's atmosphere for aerocapture, and nothing for
/// bodies too small to cost more than a rendezvous.
///
/// - Moon: Apollo 11 (NASA TM X-58040) burned 23 m/s to descend from 111
///   km to 15 km and 2,081 m/s of powered descent; its ascent to a 17 × 83
///   km orbit needed 1,846 m/s. Against the ideal transfers (1,730 and
///   1,687 m/s), losses of 0.374 and 0.159 km/s.
/// - Mars: ascent to a 100 km circular orbit 4,200 m/s (Palaszewski, NTRS
///   20210026187, with 20% for gravity losses; the Mars Sample Return
///   ascent vehicle's 4,150 m/s to 100 × 500 km, NASA/TM-2013-216620, has
///   763 m/s of losses), against an ideal 3,606: a loss of 0.594. A
///   propulsive landing is taken to lose as much. Entry leaves 674 m/s of
///   propulsive deceleration and hover (Palaszewski's 60 t lander).
/// - Pass altitudes and speed limits are this game's assumptions: Earth
///   passes at 75 km up to 12.5 km/s (Stardust entered at 12.9), Mars at
///   50 km up to 8 km/s (Pathfinder entered at 7.3).
pub fn rules(eph: &Ephemeris, body: BodyId) -> Rules {
    let none = Rules {
        descent_loss: 0.0,
        ascent_loss: 0.0,
        atmosphere: None,
    };
    match eph.body(body).id.as_str() {
        "moon" => Rules {
            descent_loss: 0.374,
            ascent_loss: 0.159,
            atmosphere: None,
        },
        "mars" => Rules {
            descent_loss: 0.594,
            ascent_loss: 0.594,
            atmosphere: Some(Atmosphere {
                capture_altitude: 50.0,
                entry_altitude: 20.0,
                terminal_dv: 0.674,
                max_speed: 8.0,
            }),
        },
        "earth" => Rules {
            atmosphere: Some(Atmosphere {
                capture_altitude: 75.0,
                entry_altitude: 40.0,
                terminal_dv: 0.3,
                max_speed: 12.5,
            }),
            ..none
        },
        _ => none,
    }
}

/// The ideal two-burn Δv between a circular orbit of radius `orbit` and
/// rest on a surface of radius `surface`, about a body of `gm`, km/s: the
/// half ellipse between them, its burns at each end. The same either way.
pub fn ideal(gm: f64, surface: f64, orbit: f64) -> f64 {
    let a = (surface + orbit) / 2.0;
    let at = |r: f64| (gm * (2.0 / r - 1.0 / a)).sqrt();
    ((gm / orbit).sqrt() - at(orbit)) + at(surface)
}

/// Landing on `body` from a low circular orbit of radius `orbit`, km/s,
/// propulsively.
pub fn descent(eph: &Ephemeris, body: BodyId, orbit: f64) -> f64 {
    let b = eph.body(body);
    ideal(b.gm, b.radius, orbit) + rules(eph, body).descent_loss
}

/// Ascending from `body` to a low circular orbit of radius `orbit`, km/s.
pub fn ascent(eph: &Ephemeris, body: BodyId, orbit: f64) -> f64 {
    let b = eph.body(body);
    ideal(b.gm, b.radius, orbit) + rules(eph, body).ascent_loss
}

/// Landing on `body` from a low circular orbit of radius `orbit` through
/// its atmosphere, km/s: the burn lowering periapsis to the entry altitude,
/// then the terminal descent. `None` without an atmosphere.
pub fn entry_from_orbit(eph: &Ephemeris, body: BodyId, orbit: f64) -> Option<f64> {
    let b = eph.body(body);
    let atmosphere = rules(eph, body).atmosphere?;
    let periapsis = b.radius + atmosphere.entry_altitude;
    let a = (periapsis + orbit) / 2.0;
    let deorbit = (b.gm / orbit).sqrt() - (b.gm * (2.0 / orbit - 1.0 / a)).sqrt();
    Some(deorbit + atmosphere.terminal_dv)
}

/// The burn after an aerocapture pass at periapsis radius `pass` whose
/// apoapsis reaches `orbit`: raising periapsis to circularize there, km/s.
pub fn after_aerocapture(gm: f64, pass: f64, orbit: f64) -> f64 {
    let a = (pass + orbit) / 2.0;
    (gm / orbit).sqrt() - (gm * (2.0 / orbit - 1.0 / a)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fits give back what they were fitted to.
    #[test]
    fn apollo_and_mars_ascent() {
        let eph = Ephemeris::standard();
        let (moon, mars) = (eph.lookup("moon").unwrap(), eph.lookup("mars").unwrap());
        let r_moon = eph.body(moon).radius;
        // Apollo 11: 111 km circular to the surface, 23 + 2,081 m/s.
        let down = descent(eph, moon, r_moon + 111.0);
        assert!((down - 2.104).abs() < 0.005, "{down}");
        // Mars: 4,200 m/s to 100 km.
        let up = ascent(eph, mars, eph.body(mars).radius + 100.0);
        assert!((up - 4.200).abs() < 0.005, "{up}");
        // Palaszewski's descent from 500 km: 139 m/s deorbit and 674 below.
        let entry = entry_from_orbit(eph, mars, eph.body(mars).radius + 500.0).unwrap();
        assert!((entry - 0.813).abs() < 0.03, "{entry}");
    }
}
