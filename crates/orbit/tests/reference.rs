//! Catalog orbits carried into the full model.

use orbit::ephem::Ephemeris;
use orbit::families::Catalog;
use orbit::gravity::Field;
use orbit::integrate::{Dop853, Tolerance};
use orbit::reference::{EarthMoonFrame, continue_orbit};
use orbit::time::{Epoch, SECONDS_PER_DAY};

#[test]
fn frame_round_trip() {
    let eph = Ephemeris::standard();
    let cat = Catalog::standard();
    let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
    let frame = EarthMoonFrame::at(eph, earth, moon, Epoch::from_ymd(2045, 3, 1));
    let s = cat.nearest("l2-halo-south", 6.57).unwrap().orbit.state;
    let back = frame.to_rotating(cat.system.mu, &frame.to_inertial(cat.system.mu, &s));
    for i in 0..6 {
        assert!((back[i] - s[i]).abs() < 1e-12, "{back:?} vs {s:?}");
    }
}

/// The NRHO, an L1 halo and a DRO carry into the full model for six
/// revolutions, keeping their shapes: their closest and farthest distances
/// from the Moon stay near the CR3BP orbit's (scaled by the real Earth–Moon
/// distance, which varies ±5%).
#[test]
fn orbits_continue_into_the_full_model() {
    let eph = Ephemeris::standard();
    let cat = Catalog::standard();
    let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
    let field = Field::all(eph, Some(earth)).with_oblateness();
    let start = Epoch::from_ymd(2045, 3, 1);
    for (family, days) in [
        ("l2-halo-south", 6.57),
        ("l1-halo-north", 12.0),
        ("dro", 14.0),
    ] {
        let m = cat.nearest(family, days).unwrap();
        let reference = continue_orbit(eph, &field, cat, m.orbit, start, 6, 16).unwrap();
        // The CR3BP orbit's range of distances from the Moon.
        let ode = cat.system.ode();
        let mut run = Dop853::new(
            &ode,
            0.0,
            m.orbit.state,
            Tolerance {
                rtol: 1e-12,
                atol: 1e-15,
            },
        );
        let (mut cr_lo, mut cr_hi) = (f64::INFINITY, 0.0f64);
        for k in 1..=400 {
            run.advance_to(&ode, m.orbit.period * k as f64 / 400.0)
                .unwrap();
            let d =
                ((run.y[0] - 1.0 + cat.system.mu).powi(2) + run.y[1].powi(2) + run.y[2].powi(2))
                    .sqrt()
                    * cat.length_km;
            (cr_lo, cr_hi) = (cr_lo.min(d), cr_hi.max(d));
        }
        let span = reference.path.span().1;
        let (mut lo, mut hi) = (f64::INFINITY, 0.0f64);
        for k in 0..=2000 {
            let sec = span * k as f64 / 2000.0;
            let t = Epoch(start.0 + sec / SECONDS_PER_DAY);
            let s = reference.path.at(sec);
            let to_moon = eph.barycentric_position(moon, t) - eph.barycentric_position(earth, t);
            let d = (glam::DVec3::new(s[0], s[1], s[2]) - to_moon).length();
            (lo, hi) = (lo.min(d), hi.max(d));
        }
        println!(
            "{family} ({:.2} d): {:.0}-{:.0} km from the Moon over {:.0} days (CR3BP {cr_lo:.0}-{cr_hi:.0})",
            m.orbit.period * cat.time_s / SECONDS_PER_DAY,
            lo,
            hi,
            span / SECONDS_PER_DAY
        );
        assert!(
            lo > 0.7 * cr_lo && lo < 1.4 * cr_lo,
            "{family}: closest {lo} km"
        );
        assert!(
            hi > 0.8 * cr_hi && hi < 1.2 * cr_hi,
            "{family}: farthest {hi} km"
        );
    }
}
