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
