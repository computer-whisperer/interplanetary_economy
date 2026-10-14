//! Legs between planetary systems flown in the sandbox.
//!
//! The NASA handbook's Mars departures (Burke, Falck & McGuire,
//! NASA/TM-2010-216764), flown from LEO to a low Mars orbit on the
//! handbook's dates: the departure energy from the burn leaving LEO against
//! the handbook's C3 (a patched-conic Lambert arc from the Earth–Moon
//! barycenter, which `tests/mars_handbook.rs` reproduces to 0.08%).

use orbit::ephem::Ephemeris;
use orbit::gravity::Field;
use orbit::interplanetary;
use orbit::shooting::{End, LowOrbit};
use orbit::surface;
use orbit::time::{Epoch, SECONDS_PER_DAY};
use orbit::transfer::Transfer;

const DAY: f64 = SECONDS_PER_DAY;

struct World {
    global: Field<'static>,
    locals: Vec<Field<'static>>,
}

fn world() -> World {
    let eph = Ephemeris::standard();
    let (earth, mars) = (eph.lookup("earth").unwrap(), eph.lookup("mars").unwrap());
    World {
        global: Field::all(eph, None).with_oblateness(),
        locals: vec![
            Field::all(eph, Some(earth)).with_oblateness(),
            Field::all(eph, Some(mars)).with_oblateness(),
        ],
    }
}

/// The cheapest design over the planes of the departure and arrival
/// hyperbolas, with the dates fixed.
fn design(w: &World, from: End, to: End, depart: Epoch, arrive: Epoch) -> orbit::shooting::Design {
    let clock = std::time::Instant::now();
    let d = interplanetary::plan(&w.global, &w.locals, from, to, depart, arrive).expect("a design");
    println!(
        "{:.4} km/s, burns {:.0?} m/s, {:.1} s",
        d.dv,
        d.burns
            .iter()
            .map(|b| b.dv.length() * 1e3)
            .collect::<Vec<_>>(),
        clock.elapsed().as_secs_f64()
    );
    d
}

#[test]
fn mars_handbook_departures_through_the_sandbox() {
    let w = world();
    let eph = Ephemeris::standard();
    let (earth, mars) = (eph.lookup("earth").unwrap(), eph.lookup("mars").unwrap());
    let leo = LowOrbit {
        body: earth,
        radius: 6_778.0,
        inclination: None,
    };
    let lmo = LowOrbit {
        body: mars,
        radius: eph.body(mars).radius + 400.0,
        inclination: None,
    };
    let (gm, gm_mars) = (eph.body(earth).gm, eph.body(mars).gm);
    // Burn between a circular orbit of radius r and a hyperbola with excess
    // speed v_inf at periapsis r.
    let burn =
        |gm: f64, r: f64, v_inf: f64| (v_inf * v_inf + 2.0 * gm / r).sqrt() - (gm / r).sqrt();
    for (depart, arrive, c3) in [
        ((2031, 1, 28), (2031, 8, 6), 9.00_f64),
        ((2033, 4, 6), (2033, 10, 1), 8.412),
        ((2033, 4, 28), (2034, 1, 27), 7.781),
        ((2043, 11, 15), (2044, 9, 16), 8.969),
    ] {
        let (d0, d1) = (
            Epoch::from_ymd(depart.0, depart.1, depart.2),
            Epoch::from_ymd(arrive.0, arrive.1, arrive.2),
        );
        let d = design(&w, End::Low(leo), End::Low(lmo), d0, d1);
        let (out, into) = (d.burns[0].dv.length(), d.burns.last().unwrap().dv.length());
        let leaving = (gm / leo.radius).sqrt() + out;
        let flown = leaving * leaving - 2.0 * gm / leo.radius;
        // Patched conics: the handbook's departure energy, and the arrival
        // excess speed of the same Lambert arc.
        let arc = Transfer::compute(eph, earth, mars, d0, d1.0 - d0.0).unwrap();
        let (out_pc, into_pc) = (
            burn(gm, leo.radius, c3.sqrt()),
            burn(gm_mars, lmo.radius, arc.v_inf_arrive),
        );
        println!(
            "{depart:?}: leaving LEO {:.0} m/s (C3 {flown:.3}), patched conics {:.0} (handbook C3 {c3}); into LMO {:.0}, patched conics {:.0}",
            out * 1e3,
            out_pc * 1e3,
            into * 1e3,
            into_pc * 1e3
        );
        assert!((out / out_pc - 1.0).abs() < 0.01, "leaving LEO");
        assert!((into / into_pc - 1.0).abs() < 0.02, "into LMO");
    }
}

/// Escape to a given excess velocity (the 2033 Mars arc's, measured 3 days
/// out) and capture from one back into LEO cost what a hyperbola with that
/// excess speed costs at LEO's radius.
#[test]
fn escape_and_capture() {
    let w = world();
    let eph = Ephemeris::standard();
    let (earth, mars) = (eph.lookup("earth").unwrap(), eph.lookup("mars").unwrap());
    let leo = LowOrbit {
        body: earth,
        radius: 6_778.0,
        inclination: None,
    };
    let gm = eph.body(earth).gm;
    let (d0, d1) = (Epoch::from_ymd(2033, 4, 6), Epoch::from_ymd(2033, 10, 1));
    let s = eph.system_state(earth, d0);
    let arc = orbit::lambert::solve(
        orbit::consts::MU_SUN,
        s.r,
        eph.system_state(mars, d1).r,
        (d1.0 - d0.0) * DAY,
    )
    .unwrap();
    let v_inf = arc.v1 - s.v;
    let hyperbola =
        (v_inf.length_squared() + 2.0 * gm / leo.radius).sqrt() - (gm / leo.radius).sqrt();
    let out = design(
        &w,
        End::Low(leo),
        End::Excess { body: earth, v_inf },
        d0,
        d0.plus_days(3.0),
    );
    let back = design(
        &w,
        End::Excess {
            body: earth,
            v_inf: -v_inf,
        },
        End::Low(leo),
        d0,
        d0.plus_days(3.0),
    );
    for (what, d) in [("escape", &out), ("capture", &back)] {
        println!(
            "{what}: {:.1} m/s, a hyperbola {:.1}",
            d.dv * 1e3,
            hyperbola * 1e3
        );
        assert!((d.dv / hyperbola - 1.0).abs() < 0.005, "{what}");
    }
}

/// A rendezvous with 2000 SG344 (leaving 2066-10-26, 324 days) costs what
/// patched conics say: leaving LEO with the arc's excess speed, then
/// matching the asteroid's velocity.
#[test]
fn rendezvous_with_a_small_body() {
    let w = world();
    let eph = Ephemeris::standard();
    let (earth, nea) = (
        eph.lookup("earth").unwrap(),
        eph.lookup("2000-sg344").unwrap(),
    );
    let leo = LowOrbit {
        body: earth,
        radius: 6_778.0,
        inclination: None,
    };
    let gm = eph.body(earth).gm;
    let d0 = Epoch::from_ymd(2066, 10, 26);
    let d1 = d0.plus_days(324.0);
    let arc = Transfer::compute(eph, earth, nea, d0, 324.0).unwrap();
    let patched = (arc.v_inf_depart.powi(2) + 2.0 * gm / leo.radius).sqrt()
        - (gm / leo.radius).sqrt()
        + arc.v_inf_arrive;
    let d = design(&w, End::Low(leo), End::Body(nea), d0, d1);
    println!(
        "LEO to 2000 SG344: {:.0} m/s, burns {:.0?}; patched conics {:.0}",
        d.dv * 1e3,
        d.burns
            .iter()
            .map(|b| b.dv.length() * 1e3)
            .collect::<Vec<_>>(),
        patched * 1e3
    );
    assert!((d.dv / patched - 1.0).abs() < 0.02);
}

/// Aerocapture at Mars on the 2033 Type I arc: the capture burn of 2.8
/// km/s becomes a periapsis raise of ~0.08 km/s after a pass at 50 km,
/// within the speed limit there.
#[test]
fn aerocapture_at_mars() {
    let w = world();
    let eph = Ephemeris::standard();
    let (earth, mars) = (eph.lookup("earth").unwrap(), eph.lookup("mars").unwrap());
    let leo = LowOrbit {
        body: earth,
        radius: 6_778.0,
        inclination: None,
    };
    let rules = surface::rules(eph, mars).atmosphere.unwrap();
    let (r, gm) = (eph.body(mars).radius, eph.body(mars).gm);
    let pass = r + rules.capture_altitude;
    let after = surface::after_aerocapture(gm, pass, r + 400.0);
    let to = End::Atmosphere {
        body: mars,
        radius: pass,
        after,
        max_speed: rules.max_speed,
    };
    let (d0, d1) = (Epoch::from_ymd(2033, 4, 6), Epoch::from_ymd(2033, 10, 1));
    let d = design(&w, End::Low(leo), to, d0, d1);
    let b: Vec<f64> = d.burns.iter().map(|b| b.dv.length() * 1e3).collect();
    println!("LEO to Mars by aerocapture: {b:.0?} m/s");
    assert!((b[0] - 3_550.0).abs() < 15.0, "leaving LEO");
    assert!((b[1] - after * 1e3).abs() < 1e-6);
    assert!(after > 0.05 && after < 0.12, "{after}");
}
