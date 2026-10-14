//! Exploration: two-burn transfers between the NRHO and low lunar orbit,
//! and from LEO to the NRHO, seeded by fans.
//! `cargo run --release -p orbit --example transfer_probe`

use orbit::ephem::Ephemeris;
use orbit::families::{Catalog, Member};
use orbit::gravity::Field;
use orbit::legs::{self, Direction};
use orbit::reference::continue_orbit;
use orbit::shooting::LowOrbit;
use orbit::time::{Epoch, SECONDS_PER_DAY};
use std::time::Instant;

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

/// The design's lowest altitude over `body`, km.
fn lowest(field: &Field, d: &orbit::shooting::Design, body: orbit::ephem::BodyId) -> f64 {
    let ode = field.ode(d.path.epoch);
    let mut low = f64::INFINITY;
    for k in 0..d.path.states.len() {
        let mut run = orbit::integrate::Dop853::new(
            &ode,
            d.path.times[k],
            d.path.states[k],
            orbit::integrate::Tolerance {
                rtol: 1e-12,
                atol: 1e-9,
            },
        );
        let n = 2000;
        for j in 1..=n {
            let t = d.path.times[k] + (d.path.times[k + 1] - d.path.times[k]) * j as f64 / n as f64;
            run.advance_to(&ode, t).unwrap();
            let b = field.body_position(body, Epoch(d.path.epoch.0 + t / SECONDS_PER_DAY));
            low = low.min((glam::DVec3::new(run.y[0], run.y[1], run.y[2]) - b).length());
        }
    }
    low - field.ephemeris().body(body).radius
}

fn main() {
    let eph = Ephemeris::standard();
    let cat = Catalog::standard();
    let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
    let field = Field::all(eph, Some(earth)).with_oblateness();
    let light = Field::new(eph, Some(earth), &[eph.sun(), earth, moon]);
    let start = Epoch::from_ymd(2045, 3, 1);
    let args: Vec<String> = std::env::args().collect();
    let which = args.get(1).map(String::as_str).unwrap_or("down");
    let m = nearest(cat, "l2-halo-south", 6.57);
    let clock = Instant::now();
    let nrho = continue_orbit(eph, &field, cat, m.orbit, start, 4, 16).unwrap();
    println!("reference: {:.2} s", clock.elapsed().as_secs_f64());
    let period = nrho.period_s();
    let (low, direction, sizes, flyby_sizes, span, from) = match which {
        "down" => (
            LowOrbit {
                body: moon,
                radius: 1_837.4,
                inclination: Some(90f64.to_radians()),
            },
            Direction::Down,
            vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
            vec![],
            3.0 * SECONDS_PER_DAY,
            0.0,
        ),
        _ => (
            LowOrbit {
                body: earth,
                radius: 6_778.0,
                inclination: None,
            },
            if which == "return" {
                Direction::Down
            } else {
                Direction::Up
            },
            vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.8],
            vec![0.0, 0.1, 0.2, 0.3],
            8.0 * SECONDS_PER_DAY,
            2.0 * period,
        ),
    };
    let search = legs::Search {
        times: (0..32).map(|k| from + period * k as f64 / 32.0).collect(),
        sizes,
        flyby_sizes,
        directions: 30,
        flyby_directions: 12,
        span,
        designs: 24,
    };
    let duration = args
        .get(2)
        .map(|d| d.parse::<f64>().unwrap() * SECONDS_PER_DAY);
    let clock = Instant::now();
    let seeds = legs::seeds(&field, &light, &nrho, &low, direction, &search);
    println!(
        "{} seeds in {:.2} s",
        seeds.len(),
        clock.elapsed().as_secs_f64()
    );
    let mut leg = legs::leg(&field, &nrho, low, direction);
    leg.duration = duration;
    if let Ok(targets) = std::env::var("FRONTIER") {
        let targets: Vec<f64> = targets
            .split(',')
            .map(|t| t.parse::<f64>().unwrap() * SECONDS_PER_DAY)
            .collect();
        let free = seeds.iter().find_map(|s| leg.design(&s.path).ok()).unwrap();
        println!("free: {:.4} km/s in {:.2} d", free.dv, legs::days(&free));
        for step in [0.25, 0.1, 0.05] {
            let f = leg.frontier(&free, &targets, step * SECONDS_PER_DAY);
            for (t, d) in targets.iter().zip(&f) {
                println!(
                    "step {step} -> {:.2} d: {:?}",
                    t / SECONDS_PER_DAY,
                    d.as_ref().map(|d| (
                        d.dv,
                        d.burns
                            .iter()
                            .map(|b| (b.dv.length() * 1e3).round())
                            .collect::<Vec<_>>()
                    ))
                );
            }
        }
        return;
    }
    let only: usize = std::env::var("ONLY")
        .ok()
        .map_or(usize::MAX, |v| v.parse().unwrap());
    for seed in seeds.iter().take(only) {
        let clock = Instant::now();
        let result = leg.design(&seed.path);
        let secs = clock.elapsed().as_secs_f64();
        let flyby = seed.path.burns.iter().filter(|&&b| b).count() > 2;
        let span = (seed.path.times.last().unwrap() - seed.path.times[0]) / SECONDS_PER_DAY;
        match result {
            Ok(d) => println!(
                "seed {:.3} km/s ({span:.1} d{}) -> {:.4} km/s in {:.2} d, burns {:?}, {} iterations, {:.3} s, lowest over Moon {:.0} km",
                seed.estimate,
                if flyby { ", flyby" } else { "" },
                d.dv,
                legs::days(&d),
                d.burns
                    .iter()
                    .map(|b| (b.dv.length() * 1e3).round())
                    .collect::<Vec<_>>(),
                d.iterations,
                secs,
                lowest(&field, &d, moon)
            ),
            Err(e) => println!(
                "seed {:.3} km/s ({span:.1} d{}) -> {e:?}, {secs:.3} s",
                seed.estimate,
                if flyby { ", flyby" } else { "" },
            ),
        }
    }
}
