//! Transfers designed in the full field against published values: Whitley
//! & Martinez, "Options for Staging Orbits in Cislunar Space" (IEEE
//! Aerospace 2016; NTRS 20160003079), for Orion and the 9:2 NRHO.
//!
//! - NRHO to a polar 100 km low lunar orbit: 730 m/s in 0.5 days.
//! - LEO to the NRHO with a powered lunar flyby, after the burn leaving
//!   LEO: 409 m/s in 5.4 days (163.1 flyby + 246.0 insertion), 428.5 in
//!   5.1 days.
//! - NRHO back to Earth with a powered flyby: 411.5 m/s in 5.0 days (221.5
//!   departure + 190 flyby), to entry interface; here to a 400 km LEO
//!   perigee.
//!
//! Measured (2026-10-13): 681 m/s; 397 (170 + 227) and 402 m/s; and 392
//! (215 + 177) in 6.1 days, the optimum when the duration is free.

use orbit::ephem::Ephemeris;
use orbit::families::{Catalog, Member};
use orbit::gravity::Field;
use orbit::legs::{self, Direction, Search};
use orbit::reference::{Reference, continue_orbit};
use orbit::shooting::{Design, LowOrbit};
use orbit::time::{Epoch, SECONDS_PER_DAY};

fn nearest<'a>(cat: &'a Catalog, family: &str, days: f64) -> &'a Member {
    let to_days = |m: &Member| m.orbit.period * cat.time_s / SECONDS_PER_DAY;
    cat.family(family)
        .unwrap()
        .members
        .iter()
        .min_by(|a, b| {
            (to_days(a) - days)
                .abs()
                .total_cmp(&(to_days(b) - days).abs())
        })
        .unwrap()
}

/// The 9:2 NRHO (6.57 days) for four revolutions from 2045-03-01.
fn nrho(field: &Field) -> Reference {
    let eph = Ephemeris::standard();
    let cat = Catalog::standard();
    let m = nearest(cat, "l2-halo-south", 6.57);
    continue_orbit(eph, field, cat, m.orbit, Epoch::from_ymd(2045, 3, 1), 4, 16).unwrap()
}

/// A search over one revolution of `reference`, starting at revolution
/// `from`.
fn one_revolution(reference: &Reference, from: f64, sizes: Vec<f64>, flyby: bool) -> Search {
    let period = reference.period_s();
    Search {
        times: (0..32).map(|k| (from + k as f64 / 32.0) * period).collect(),
        sizes,
        flyby_sizes: if flyby {
            vec![0.0, 0.1, 0.2, 0.3]
        } else {
            vec![]
        },
        directions: 30,
        flyby_directions: 12,
        span: 8.0 * SECONDS_PER_DAY,
        designs: 6,
    }
}

fn burns_ms(d: &Design) -> Vec<f64> {
    d.burns
        .iter()
        .map(|b| (b.dv.length() * 1e3).round())
        .collect()
}

fn within(value: f64, published: f64, what: &str) {
    println!("{what}: {value:.1} m/s, published {published}");
    assert!(
        (value / published - 1.0).abs() <= 0.10,
        "{what}: {value} m/s against {published}"
    );
}

#[test]
fn nrho_to_low_lunar_orbit() {
    let eph = Ephemeris::standard();
    let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
    let field = Field::all(eph, Some(earth)).with_oblateness();
    let light = Field::new(eph, Some(earth), &[eph.sun(), earth, moon]);
    let nrho = nrho(&field);
    let llo = LowOrbit {
        body: moon,
        radius: eph.body(moon).radius + 100.0,
        inclination: Some(90f64.to_radians()),
    };
    let search = one_revolution(&nrho, 0.0, vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6], false);
    let (designs, _) = legs::search(
        &field,
        &light,
        &nrho,
        llo,
        Direction::Down,
        &Search {
            span: 3.0 * SECONDS_PER_DAY,
            ..search
        },
        Some(0.5 * SECONDS_PER_DAY),
    );
    let best = &designs[0];
    println!("burns {:?} m/s", burns_ms(best));
    within(best.dv * 1e3, 730.0, "NRHO to polar LLO in 0.5 days");
}

#[test]
fn leo_to_nrho_by_lunar_flyby() {
    let eph = Ephemeris::standard();
    let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
    let field = Field::all(eph, Some(earth)).with_oblateness();
    let light = Field::new(eph, Some(earth), &[eph.sun(), earth, moon]);
    let nrho = nrho(&field);
    let leo = LowOrbit {
        body: earth,
        radius: 6_778.0,
        inclination: None,
    };
    let search = one_revolution(&nrho, 2.0, vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.8], true);
    let (designs, _) = legs::search(&field, &light, &nrho, leo, Direction::Up, &search, None);
    let free = &designs[0];
    println!(
        "free duration: {:?} m/s in {:.2} days",
        burns_ms(free),
        legs::days(free)
    );
    let leg = legs::leg(&field, &nrho, leo, Direction::Up);
    let days = [5.1, 5.4];
    let durations: Vec<f64> = days.iter().map(|d| d * SECONDS_PER_DAY).collect();
    let frontier = leg.frontier(free, &durations, 0.25 * SECONDS_PER_DAY);
    for ((d, published), design) in days.iter().zip([428.5, 409.1]).zip(&frontier) {
        let design = design.as_ref().expect("continuation reaches it");
        let burns = burns_ms(design);
        println!("{d} days: {burns:?} m/s");
        assert_eq!(burns.len(), 3, "a flyby burn");
        assert!(
            (3_000.0..3_200.0).contains(&burns[0]),
            "leaving LEO: {burns:?}"
        );
        within(
            burns[1..].iter().sum(),
            published,
            &format!("after LEO, {d} days"),
        );
    }
}

#[test]
fn nrho_to_leo_by_lunar_flyby() {
    let eph = Ephemeris::standard();
    let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
    let field = Field::all(eph, Some(earth)).with_oblateness();
    let light = Field::new(eph, Some(earth), &[eph.sun(), earth, moon]);
    let nrho = nrho(&field);
    let leo = LowOrbit {
        body: earth,
        radius: 6_778.0,
        inclination: None,
    };
    let search = one_revolution(&nrho, 0.0, vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.8], true);
    let (designs, _) = legs::search(&field, &light, &nrho, leo, Direction::Down, &search, None);
    let free = &designs[0];
    println!(
        "free duration: {:?} m/s in {:.2} days",
        burns_ms(free),
        legs::days(free)
    );
    let leg = legs::leg(&field, &nrho, leo, Direction::Down);
    let frontier = leg.frontier(free, &[5.0 * SECONDS_PER_DAY], 0.25 * SECONDS_PER_DAY);
    let design = frontier[0].as_ref().expect("continuation reaches 5 days");
    let burns = burns_ms(design);
    println!("5 days: {burns:?} m/s");
    assert_eq!(burns.len(), 3, "a flyby burn");
    within(
        burns[..2].iter().sum(),
        411.5,
        "before the LEO capture burn, 5 days",
    );
}
