//! Rotation models (M1c.md, step 6 plan, item 4) against JPL Horizons:
//! each body's apparent sub-observer point, from Earth's center (Earth's
//! from the Moon's), on dates through 2035 (`tools/rotation/fixtures.sh`).
//!
//! Horizons gives the point where the line from the body's center toward
//! the observer meets its ellipsoid (the fixture's radii), in planetodetic
//! coordinates on the spheroid of the first and last radii (SPICE's
//! recgeo: a triaxial body's point is off that spheroid, at a height). So
//! that line, in the body's frame at the time the light left, is taken
//! through the same steps. Horizons corrects for light time, not for the
//! observer's aberration (with it, every body was ~20″ off).

use glam::DVec3;
use orbit::ephem::Ephemeris;
use orbit::rotation::Rotations;
use orbit::time::{Epoch, SECONDS_PER_DAY};

const C_KM_S: f64 = 299_792.458;

struct Row {
    t: Epoch,
    lon: f64,
    lat: f64,
}

/// The fixture's rows, whether its longitudes are west-positive, and its
/// ellipsoid's radii.
fn fixture(id: &str) -> (Vec<Row>, bool, DVec3) {
    let path = format!(
        "{}/tests/fixtures/horizons_subobs_{id}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let west = text
        .lines()
        .find(|l| l.starts_with("Target pole/equ"))
        .expect("a pole line")
        .contains("West-longitude positive");
    let radii: Vec<f64> = text
        .lines()
        .find(|l| l.starts_with("Target radii"))
        .and_then(|l| l.split(':').nth(1))
        .and_then(|l| l.split("km").next())
        .expect("a radii line")
        .split(',')
        .map(|x| x.trim().parse().unwrap())
        .collect();
    let body = text
        .split("$$SOE")
        .nth(1)
        .and_then(|s| s.split("$$EOE").next())
        .expect("a table");
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let rows = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split(',').map(str::trim).collect();
            let (date, time) = f[0].split_once(' ').expect("a date and a time");
            let d: Vec<&str> = date.split('-').collect();
            let month = months.iter().position(|m| *m == d[1]).expect("a month") as u32 + 1;
            let (h, m) = time.split_once(':').expect("hours and minutes");
            let t = Epoch::from_ymd(d[0].parse().unwrap(), month, d[2].parse().unwrap())
                .plus_days((h.parse::<f64>().unwrap() + m.parse::<f64>().unwrap() / 60.0) / 24.0);
            Row {
                t,
                lon: f[3].parse().unwrap(),
                lat: f[4].parse().unwrap(),
            }
        })
        .collect();
    (rows, west, DVec3::new(radii[0], radii[1], radii[2]))
}

/// Where the line from `target` toward `observer` at `t`, in the target's
/// frame when the light left it, meets the ellipsoid of `radii`: the
/// normal's east longitude and latitude there, degrees.
fn sub_observer(
    eph: &Ephemeris,
    r: &Rotations,
    target: &str,
    observer: &str,
    t: Epoch,
    radii: DVec3,
) -> (f64, f64) {
    let (b, o) = (eph.lookup(target).unwrap(), eph.lookup(observer).unwrap());
    let seen = eph.barycentric_position(o, t);
    let mut left = t;
    for _ in 0..4 {
        let light = (eph.barycentric_position(b, left) - seen).length() / C_KM_S;
        left = Epoch(t.0 - light / SECONDS_PER_DAY);
    }
    let toward = (eph.barycentric_position(b, left) - seen).normalize();
    let v: DVec3 = r.axes(target, left).unwrap().transpose() * -toward;
    // The point is s v, on the ellipsoid; its geodetic coordinates are
    // taken on the spheroid of the first and last radii (as SPICE's
    // recgeo), where a triaxial body's point is off it, at some height.
    let p = v / (v / radii).length();
    let (a, f) = (radii.x, 1.0 - radii.z / radii.x);
    let e2 = f * (2.0 - f);
    let rho = p.x.hypot(p.y);
    let mut lat = p.z.atan2(rho * (1.0 - e2));
    // Slow to converge where the body is very flat (Deimos, f = 0.35).
    for _ in 0..200 {
        let n = a / (1.0 - e2 * lat.sin().powi(2)).sqrt();
        let h = if lat.cos().abs() > 1e-9 {
            rho / lat.cos() - n
        } else {
            p.z.abs() / lat.sin().abs() - n * (1.0 - e2)
        };
        lat = p.z.atan2(rho * (1.0 - e2 * n / (n + h)));
    }
    (p.y.atan2(p.x).to_degrees(), lat.to_degrees())
}

#[test]
fn sub_observer_points_match_horizons() {
    let eph = Ephemeris::standard();
    let r = Rotations::standard();
    // Degrees, on the body (longitude × cos latitude). The IAU models are
    // what Horizons uses, and most agree within 0.0005°. Not quite: the
    // Moon (Horizons: DE441's librations, which the IAU series
    // approximates; 0.003°), Earth (Horizons: measured orientation; here
    // the rotation angle with ΔT = 69 s and no nutation; 0.0025°), and
    // Jupiter and Saturn (0.002°, not found).
    let bodies = [
        ("sun", "earth", 0.001),
        ("mercury", "earth", 0.001),
        ("venus", "earth", 0.001),
        ("moon", "earth", 0.005),
        ("mars", "earth", 0.001),
        ("phobos", "earth", 0.001),
        ("deimos", "earth", 0.001),
        ("jupiter", "earth", 0.005),
        ("saturn", "earth", 0.005),
        ("uranus", "earth", 0.001),
        ("neptune", "earth", 0.001),
        ("earth", "moon", 0.005),
    ];
    let mut failed = Vec::new();
    for (target, observer, bound) in bodies {
        let (rows, west, radii) = fixture(target);
        let mut worst: f64 = 0.0;
        for row in &rows {
            let (lon, lat) = sub_observer(eph, r, target, observer, row.t, radii);
            let lon = if west { -lon } else { lon };
            let dlon = (lon - row.lon + 540.0).rem_euclid(360.0) - 180.0;
            let off = (dlon * lat.to_radians().cos()).hypot(lat - row.lat);
            worst = worst.max(off);
        }
        println!("{target}: worst {worst:.5}° over {} dates", rows.len());
        if worst > bound {
            failed.push(format!("{target}: {worst:.5}° (bound {bound}°)"));
        }
    }
    assert!(failed.is_empty(), "{failed:?}");
}
