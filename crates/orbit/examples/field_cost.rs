//! What integrating a ship costs: time per gravity evaluation, and
//! evaluations per simulated day in low Earth orbit and on a cislunar coast.
//! `cargo run --release -p orbit --example field_cost`

use glam::DVec3;
use orbit::ephem::Ephemeris;
use orbit::gravity::Field;
use orbit::integrate::{Dop853, Tolerance};
use orbit::time::Epoch;
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let eph = Ephemeris::standard();
    let id = |s: &str| eph.lookup(s).unwrap();
    let (sun, earth, moon) = (id("sun"), id("earth"), id("moon"));
    let epoch = Epoch::from_ymd(2045, 3, 1);
    let fields = [
        (
            "Sun, Earth, Moon",
            Field::new(eph, Some(earth), &[sun, earth, moon]),
        ),
        ("all 10 bodies", Field::all(eph, Some(earth))),
        (
            "all 10 bodies + J2",
            Field::all(eph, Some(earth)).with_oblateness(),
        ),
    ];
    for (label, field) in &fields {
        let n = 2_000_000;
        let start = Instant::now();
        let mut acc = DVec3::ZERO;
        for k in 0..n {
            let t = Epoch(epoch.0 + k as f64 * 1e-4);
            acc += field.acceleration(
                black_box(t),
                black_box(DVec3::new(7000.0 + k as f64 * 1e-6, 10.0, 20.0)),
            );
        }
        let ns = start.elapsed().as_nanos() as f64 / n as f64;
        println!("{label}: {ns:.0} ns per evaluation ({:.1e})", acc.x);
    }
    let tol = Tolerance {
        rtol: 1e-11,
        atol: 1e-8,
    };
    let field = Field::all(eph, Some(earth)).with_oblateness();
    let ode = field.ode(epoch);
    let leo = (gm(eph, earth) / 6_778.137f64).sqrt();
    for (label, y0) in [
        (
            "LEO, 400 km",
            [6_778.137, 0.0, 0.0, 0.0, leo * 0.8, leo * 0.6],
        ),
        (
            "cislunar coast near L1",
            [320_000.0, 50_000.0, 10_000.0, -0.1, 0.8, 0.05],
        ),
    ] {
        let mut run = Dop853::new(&ode, 0.0, y0, tol);
        let start = Instant::now();
        run.advance_to(&ode, 10.0 * 86_400.0).unwrap();
        let ms = start.elapsed().as_secs_f64() * 1e3 / 10.0;
        println!(
            "{label}: {:.0} evaluations per day, {ms:.2} ms per simulated day",
            run.evaluations as f64 / 10.0
        );
    }
}

fn gm(eph: &Ephemeris, b: orbit::ephem::BodyId) -> f64 {
    eph.body(b).gm
}
