//! Ephemerides against JPL Horizons (DE441) state vectors.

use orbit::consts::{AU_KM, MU_EARTH, MU_SUN};
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
            Box::new(|t| relative(&eph, moon, earth, t)),
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

/// The piecewise fits in `data/neas.toml` hold positions to 0.002 au except
/// across close Earth flybys (2000 SG344 has one inside a segment). This also
/// checks that the Rust model reproduces the fit script's.
#[test]
fn small_bodies_match_horizons() {
    let eph = Ephemeris::standard();
    for (body, file, max_au) in [
        ("ryugu", "horizons_ryugu.csv", 0.002),
        ("2008-ev5", "horizons_2008ev5.csv", 0.002),
        ("2001-fr85", "horizons_2001fr85.csv", 0.003),
        ("2000-sg344", "horizons_2000sg344.csv", 0.011),
    ] {
        let id = eph.lookup(body).unwrap();
        let (dr, dv) = max_errors(file, |t| eph.state(id, t));
        println!(
            "{body}: max |dr| = {:.4} au, |dv| = {:.0} m/s",
            dr / AU_KM,
            dv * 1e3
        );
        assert!(
            dr / AU_KM < max_au,
            "{body}: position error {} au",
            dr / AU_KM
        );
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
