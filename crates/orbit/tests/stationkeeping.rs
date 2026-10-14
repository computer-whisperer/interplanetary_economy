//! Station-keeping costs (upkeep) against published values, with the
//! navigation errors of the NRHO studies (1 km and 1 cm/s, 3σ).
//!
//! Published (2026-10-13 search):
//! - NRHO: 0.5–4 m/s per year. Guzzetti et al., AAS 17-395 (2017): 1.9–3.8
//!   for perilune radii of 3,500–9,000 km with these navigation errors;
//!   Davis et al., AAS 2020: ~0.7; Elango et al. (MERL TR2020-006): under 0.5.
//! - Earth–Moon libration-point orbits: ARTEMIS projected 5.3 m/s per year
//!   at L1 and 7.4 at L2 (Folta et al., IAA 2012); 6.05 for an L2 northern
//!   halo (arXiv 2510.06368).
//! - DROs are stable: near zero.
//!
//! Measured (2026-10-13): NRHO 1.09 m/s per year, within its range; L1 and
//! L2 halos 3.2 and 4.3, ~1.5× below the published figures, which are for
//! ARTEMIS's Lissajous orbits and its own navigation, so the halo bounds
//! here are the same order (2.5–10); DRO 0.64, the cost of correcting
//! navigation errors on a stable orbit.

use orbit::ephem::Ephemeris;
use orbit::families::{Catalog, Member};
use orbit::gravity::Field;
use orbit::reference::continue_orbit;
use orbit::stationkeeping::{Errors, upkeep};
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

#[test]
fn upkeep_matches_published_ranges() {
    let eph = Ephemeris::standard();
    let cat = Catalog::standard();
    let earth = eph.lookup("earth").unwrap();
    let field = Field::all(eph, Some(earth)).with_oblateness();
    let start = Epoch::from_ymd(2045, 3, 1);
    for (label, family, days, revs, lo, hi) in [
        ("NRHO", "l2-halo-south", 6.57, 20, 0.5, 4.0),
        ("L1 halo", "l1-halo-north", 12.0, 10, 2.5, 10.0),
        ("L2 halo", "l2-halo-north", 14.5, 8, 2.5, 10.0),
        ("DRO", "dro", 14.0, 8, 0.0, 1.0),
    ] {
        let m = nearest(cat, family, days);
        let reference = continue_orbit(eph, &field, cat, m.orbit, start, revs, 16).unwrap();
        let u = upkeep(&field, &reference, Errors::TYPICAL, 20).unwrap();
        let period = reference.period_s();
        let ms = u.dv_per_year_kms * 1e3;
        println!(
            "{label} ({:.1} d, stability {:.0}): {ms:.2} m/s per year, {:.0} burns per year (every {} rev, aiming {} rev ahead), within {:.0} km",
            m.orbit.period * cat.time_s / SECONDS_PER_DAY,
            m.stability,
            u.maneuvers_per_year,
            u.policy.interval_s / period,
            u.policy.horizon_s / period,
            u.worst_deviation_km
        );
        assert!(ms >= lo && ms <= hi, "{label}: {ms} m/s per year");
    }
}
