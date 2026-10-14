//! Legs between low Mars orbit and Phobos, planned in a Mars-centered
//! field (the Sun, Mars with J2, Jupiter), against Hohmann's transfer
//! between circular orbits, which they nearly are: Phobos's eccentricity is
//! 0.015 and its plane within 1.1° of Mars's equator. (M0's network had
//! 1.21 km/s each way from 300 km, the same Hohmann.)

use orbit::ephem::Ephemeris;
use orbit::gravity::Field;
use orbit::legs;
use orbit::shooting::{End, LowOrbit};
use orbit::time::{Epoch, SECONDS_PER_DAY};

#[test]
fn low_mars_orbit_to_phobos_and_back() {
    let eph = Ephemeris::standard();
    let id = |s: &str| eph.lookup(s).unwrap();
    let (mars, phobos) = (id("mars"), id("phobos"));
    let field = Field::new(eph, Some(mars), &[eph.sun(), mars, id("jupiter")]).with_oblateness();
    let gm = eph.body(mars).gm;
    let lmo = LowOrbit {
        body: mars,
        radius: eph.body(mars).radius + 300.0,
        inclination: None,
    };
    // Hohmann between the low orbit and Phobos's mean radius.
    let (r1, r2) = (lmo.radius, 9_376.0);
    let a = (r1 + r2) / 2.0;
    let hohmann = ((gm * (2.0 / r1 - 1.0 / a)).sqrt() - (gm / r1).sqrt())
        + ((gm / r2).sqrt() - (gm * (2.0 / r2 - 1.0 / a)).sqrt());
    let duration = std::f64::consts::PI * (a.powi(3) / gm).sqrt();
    let start = Epoch::from_ymd(2045, 3, 1);
    for (label, from, to) in [
        ("up", End::Low(lmo), End::Body(phobos)),
        ("down", End::Body(phobos), End::Low(lmo)),
    ] {
        let clock = std::time::Instant::now();
        let d = legs::plan(
            &field,
            &field,
            start,
            from,
            to,
            (0.0, SECONDS_PER_DAY),
            &[duration],
        )[0]
        .clone()
        .expect("a design");
        println!(
            "{label}: {:.0} m/s in {:.2} h, burns {:.0?}; Hohmann {:.0} ({:.1} s)",
            d.dv * 1e3,
            d.duration() / 3600.0,
            d.burns
                .iter()
                .map(|b| b.dv.length() * 1e3)
                .collect::<Vec<_>>(),
            hohmann * 1e3,
            clock.elapsed().as_secs_f64()
        );
        assert!((d.dv / hohmann - 1.0).abs() < 0.03, "{label}");
    }
}
