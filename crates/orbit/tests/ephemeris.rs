//! Ephemerides against JPL Horizons (DE441) state vectors.

use orbit::consts::{MU_EARTH, MU_SUN};
use orbit::ephem::{BodyId, Ephemeris};
use orbit::kepler::State;
use orbit::time::Epoch;

struct Row {
    t: Epoch,
    r: glam::DVec3,
    v: glam::DVec3,
}

fn fixture(name: &str) -> Vec<Row> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| {
            let f: Vec<f64> = l.split(',').map(|x| x.trim().parse().unwrap()).collect();
            Row {
                t: Epoch::from_jd(f[0]),
                r: glam::DVec3::new(f[1], f[2], f[3]),
                v: glam::DVec3::new(f[4], f[5], f[6]),
            }
        })
        .collect()
}

/// Max position error (km) and velocity error (km/s) of `state` over a
/// fixture.
fn max_errors(fixture_name: &str, state: impl Fn(Epoch) -> State) -> (f64, f64) {
    fixture(fixture_name)
        .iter()
        .fold((0.0f64, 0.0f64), |(dr, dv), row| {
            let s = state(row.t);
            (
                dr.max((s.r - row.r).length()),
                dv.max((s.v - row.v).length()),
            )
        })
}

fn relative(eph: &Ephemeris, body: BodyId, center: BodyId, t: Epoch) -> State {
    let (b, c) = (eph.barycentric(body, t), eph.barycentric(center, t));
    State {
        r: b.r - c.r,
        v: b.v - c.v,
    }
}

/// DE440 against Horizons' DE441, 2025–2080. Measured 2026-10-13: Earth
/// 0.1 m, the Moon from Earth 8 m (the two ephemerides' lunar solutions
/// differ slightly), the Sun, Mars and Jupiter under 1 mm.
#[test]
fn de440_matches_horizons() {
    let eph = Ephemeris::standard();
    let id = |s: &str| eph.lookup(s).unwrap();
    let (sun, earth, moon) = (id("sun"), id("earth"), id("moon"));
    type StateAt<'a> = Box<dyn Fn(Epoch) -> State + 'a>;
    let checks: [(&str, &str, StateAt, f64); 5] = [
        (
            "Earth",
            "horizons_earth.csv",
            Box::new(|t| eph.state(earth, t)),
            0.01,
        ),
        (
            "Moon from Earth",
            "horizons_moon.csv",
            Box::new(|t| relative(eph, moon, earth, t)),
            0.05,
        ),
        (
            "Sun from barycenter",
            "horizons_sun.csv",
            Box::new(|t| eph.barycentric(sun, t)),
            0.01,
        ),
        (
            "Mars",
            "horizons_mars.csv",
            Box::new(|t| eph.state(id("mars"), t)),
            0.01,
        ),
        (
            "Jupiter",
            "horizons_jupiter.csv",
            Box::new(|t| eph.state(id("jupiter"), t)),
            0.01,
        ),
    ];
    for (label, file, state, max_dr) in checks {
        let (dr, dv) = max_errors(file, state);
        println!(
            "{label}: max |dr| = {:.3} m, max |dv| = {:.3} mm/s",
            dr * 1e3,
            dv * 1e6
        );
        assert!(dr < max_dr, "{label}: position error {dr} km");
        assert!(dv < 1e-6, "{label}: velocity error {dv} km/s");
    }
}

/// The constants this crate uses directly agree with the DE440 masses.
#[test]
fn constants_match_de440() {
    let eph = Ephemeris::standard();
    let gm = |s: &str| eph.body(eph.lookup(s).unwrap()).gm;
    assert!((gm("sun") / MU_SUN - 1.0).abs() < 1e-12, "{}", gm("sun"));
    assert!(
        (gm("earth") / MU_EARTH - 1.0).abs() < 1e-8,
        "{}",
        gm("earth")
    );
}

/// Small bodies, integrated from their Horizons states at 2040-01-01,
/// against Horizons over 2040–2080. Measured worst errors (2026-10-13): from
/// 6 km (2001 FR85) to 8,700 km (Ryugu, after close approaches in the 2070s);
/// Bennu 3,900 km after its 2060 Earth flyby. Horizons also models the large
/// asteroids' pull and, for some bodies, thermal (Yarkovsky) drift, and close
/// flybys amplify any difference. The fits these replaced held 0.002 au
/// (~300,000 km). Tolerances are about twice the measured worst.
#[test]
fn small_bodies_match_horizons() {
    let eph = Ephemeris::standard();
    for (body, max_km) in [
        ("ryugu", 20_000.0),
        ("2008-ev5", 1_500.0),
        ("bennu", 8_000.0),
        ("apophis", 500.0),
        ("2000-sg344", 1_000.0),
        ("2017-sv19", 100.0),
        ("2001-fr85", 20.0),
    ] {
        let id = eph.lookup(body).unwrap();
        let mut by_decade = [0.0f64; 4];
        for row in fixture(&format!("horizons_{body}.csv")) {
            let err = (eph.state(id, row.t).r - row.r).length();
            let decade = (((row.t.0 - Epoch::from_ymd(2040, 1, 1).0) / 3652.5) as usize).min(3);
            by_decade[decade] = by_decade[decade].max(err);
        }
        let worst = by_decade.iter().copied().fold(0.0, f64::max);
        println!("{body}: worst error by decade {by_decade:.0?} km");
        assert!(worst < max_km, "{body}: {worst} km");
    }
}

/// The position-only evaluation agrees with the full state's position, to
/// rounding (the two sum the series in different orders).
#[test]
fn positions_match_states() {
    let eph = Ephemeris::standard();
    for id in eph.ids().filter(|&b| eph.body(b).gm > 0.0) {
        for k in 0..200 {
            let t = Epoch(Epoch::from_ymd(2025, 1, 1).0 + k as f64 * 100.37);
            let r = eph.barycentric(id, t).r;
            let d = (eph.barycentric_position(id, t) - r).length();
            assert!(d < 1e-15 * r.length() + 1e-9, "{}: {d} km", eph.body(id).id);
        }
    }
}

/// ICRF (equatorial) to this crate's ecliptic frame, as tools/ephem/build.py
/// rotates DE440.
fn equatorial_to_ecliptic(v: glam::DVec3) -> glam::DVec3 {
    let (c, s) = (
        orbit::ephem::OBLIQUITY_J2000.cos(),
        orbit::ephem::OBLIQUITY_J2000.sin(),
    );
    glam::DVec3::new(v.x, c * v.y + s * v.z, -s * v.y + c * v.z)
}

/// Earth's pole is ICRF z: at the June solstice the Sun stands 23.44° north
/// of the equator.
#[test]
fn earth_pole_puts_the_june_sun_north() {
    let eph = Ephemeris::standard();
    let earth = eph.lookup("earth").unwrap();
    let t = Epoch::from_ymd(2045, 6, 21);
    let sun = (eph.barycentric(eph.sun(), t).r - eph.barycentric(earth, t).r).normalize();
    let declination = sun.dot(eph.pole(earth, t).unwrap()).asin().to_degrees();
    assert!((declination - 23.44).abs() < 0.01, "{declination}");
}

/// The Moon's pole by Cassini's laws matches the IAU's (WGCCRE 2015,
/// Archinal et al. 2018, with its periodic terms) to within 0.1°.
#[test]
fn moon_pole_matches_iau() {
    let eph = Ephemeris::standard();
    let moon = eph.lookup("moon").unwrap();
    let mut worst = 0.0f64;
    for year in [2030, 2041, 2052, 2063, 2075] {
        for month in [1, 4, 7, 10] {
            let t = Epoch::from_ymd(year, month, 1);
            let d = t.0;
            let tc = d / 36_525.0;
            let e = |a: f64, b: f64| (a + b * d).to_radians();
            let (e1, e2, e3, e4) = (
                e(125.045, -0.052_992_1),
                e(250.089, -0.105_984_2),
                e(260.008, 13.012_000_9),
                e(176.625, 13.340_715_4),
            );
            let (e6, e7, e10, e13) = (
                e(357.529, 0.985_600_3),
                e(311.589, 26.405_708_4),
                e(15.134, -0.158_976_3),
                e(25.053, 12.959_008_8),
            );
            let ra = (269.9949 + 0.0031 * tc - 3.8787 * e1.sin() - 0.1204 * e2.sin()
                + 0.0700 * e3.sin()
                - 0.0172 * e4.sin()
                + 0.0072 * e6.sin()
                - 0.0052 * e10.sin()
                + 0.0043 * e13.sin())
            .to_radians();
            let dec = (66.5392 + 0.0130 * tc + 1.5419 * e1.cos() + 0.0239 * e2.cos()
                - 0.0278 * e3.cos()
                + 0.0068 * e4.cos()
                - 0.0029 * e6.cos()
                + 0.0009 * e7.cos()
                + 0.0008 * e10.cos()
                - 0.0009 * e13.cos())
            .to_radians();
            let iau = equatorial_to_ecliptic(glam::DVec3::new(
                dec.cos() * ra.cos(),
                dec.cos() * ra.sin(),
                dec.sin(),
            ));
            let ours = eph.pole(moon, t).unwrap();
            let angle = ours.angle_between(iau).to_degrees();
            assert!(angle < 0.1, "{year}-{month}: {angle}°");
            worst = worst.max(angle);
        }
    }
    println!("worst {worst:.3}°");
}

/// Phobos and Deimos, precessing ellipses fitted to Horizons, against
/// Horizons vectors off the fit's daily grid (Mars's center; DE440's "Mars"
/// is its system's barycenter, 0.2 m away). Measured 2026-10-13: Phobos
/// within 105 km and 24 m/s, Deimos within 124 km and 7 m/s, over
/// 2025–2080 (the fit's own residuals reach 107 and 133 km).
#[test]
fn moons_of_mars_match_horizons() {
    let eph = Ephemeris::standard();
    let mars = eph.lookup("mars").unwrap();
    for (id, km, kms) in [("phobos", 120.0, 0.03), ("deimos", 150.0, 0.015)] {
        let moon = eph.lookup(id).unwrap();
        let (dr, dv) = max_errors(&format!("horizons_{id}_mars.csv"), |t| {
            relative(eph, moon, mars, t)
        });
        println!("{id}: within {dr:.1} km and {:.1} m/s", dv * 1e3);
        assert!(dr < km && dv < kms, "{id}: {dr} km, {dv} km/s");
    }
}
