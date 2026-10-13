//! Ephemerides against JPL Horizons (DE441) state vectors, 2040–2076.

use orbit::consts::AU_KM;
use orbit::ephem::Ephemeris;
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

/// Max position error (km) and velocity error (km/s) over a fixture.
fn max_errors(eph: &Ephemeris, body: &str, fixture_name: &str) -> (f64, f64) {
    let id = eph.lookup(body).unwrap();
    fixture(fixture_name).iter().fold((0.0f64, 0.0f64), |(dr, dv), row| {
        let s = eph.state(id, row.t);
        (dr.max((s.r - row.r).length()), dv.max((s.v - row.v).length()))
    })
}

#[test]
fn planets_match_horizons() {
    let eph = Ephemeris::standard();
    for (body, file, max_dr, max_dv) in [
        // Table 2a trades accuracy for a 6000-year span: measured errors are
        // up to ~30,000 km (Earth) and ~125,000 km (Mars), mostly
        // along-track and not growing. Velocity errors stay near 10 m/s,
        // far below anything that moves a transfer Δv.
        ("earth", "horizons_emb.csv", 40_000.0, 0.015),
        ("mars", "horizons_mars.csv", 150_000.0, 0.015),
    ] {
        let (dr, dv) = max_errors(&eph, body, file);
        println!("{body}: max |dr| = {dr:.0} km ({:.2e} au), max |dv| = {:.2} m/s", dr / AU_KM, dv * 1e3);
        assert!(dr < max_dr, "{body}: position error {dr} km");
        assert!(dv < max_dv, "{body}: velocity error {dv} km/s");
    }
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
        let (dr, dv) = max_errors(&eph, body, file);
        println!("{body}: max |dr| = {:.4} au, |dv| = {:.0} m/s", dr / AU_KM, dv * 1e3);
        assert!(dr / AU_KM < max_au, "{body}: position error {} au", dr / AU_KM);
    }
}
