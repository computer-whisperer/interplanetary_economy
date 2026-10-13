//! End-to-end check of ephemerides + Lambert against NASA's mission design
//! handbook: L. M. Burke, R. D. Falck, M. L. McGuire, "Interplanetary Mission
//! Design Handbook: Earth-to-Mars Mission Opportunities 2026 to 2045",
//! NASA/TM-2010-216764, Tables 2–11 (minimum departure energy rows).
//!
//! The handbook's departures are from the Earth–Moon barycenter, which is
//! what `Transfer` uses for Earth: from it, DE440 gives C3 within 0.08% of
//! every row with consistent dates; from Earth's center, which swings ~12 m/s
//! around it monthly, up to 0.9% off (measured 2026-10-13). The handbook
//! searched on a 2-day grid.
//!
//! Two handbook quirks, both confirmed against JPL Horizons (DE441) Earth and
//! Mars state vectors (2026-10-12):
//! - The 2035, 2037 and 2039 rows have inconsistent dates: Lambert from
//!   DE441 positions at the tabulated dates gives C3 of 35–1215 km²/s², not
//!   the tabulated 10–19. The tabulated C3 *values* are right: they are the
//!   minima 63–81 days later. Those rows are checked by value only.
//! - 2026 Type I: DE441 at the tabulated dates gives the tabulated 11.10, but
//!   the true minimum (10.72 by DE441) sits at a 179.3° transfer angle, in a
//!   notch beside the 180° ridge that a 2-day grid misses. Searches may find
//!   minima below the handbook's.

use orbit::ephem::Ephemeris;
use orbit::time::Epoch;
use orbit::transfer::Transfer;
use std::f64::consts::PI;

struct Minimum {
    type_ii: bool,
    depart: (i32, u32, u32),
    arrive: (i32, u32, u32),
    c3: f64,
    dates_consistent: bool,
}

const fn row(type_ii: bool, depart: (i32, u32, u32), arrive: (i32, u32, u32), c3: f64) -> Minimum {
    Minimum {
        type_ii,
        depart,
        arrive,
        c3,
        dates_consistent: true,
    }
}

const fn value_only(
    type_ii: bool,
    depart: (i32, u32, u32),
    arrive: (i32, u32, u32),
    c3: f64,
) -> Minimum {
    Minimum {
        type_ii,
        depart,
        arrive,
        c3,
        dates_consistent: false,
    }
}

#[rustfmt::skip]
const HANDBOOK: &[Minimum] = &[
    row(false, (2026, 11, 14), (2027, 8, 9), 11.11),
    row(true, (2026, 10, 31), (2027, 8, 19), 9.144),
    row(false, (2028, 12, 10), (2029, 7, 20), 9.048),
    row(true, (2028, 12, 2), (2029, 10, 16), 8.928),
    row(false, (2031, 1, 28), (2031, 8, 6), 9.00),
    row(true, (2031, 2, 23), (2032, 1, 9), 8.237),
    row(false, (2033, 4, 6), (2033, 10, 1), 8.412),
    row(true, (2033, 4, 28), (2034, 1, 27), 7.781),
    value_only(false, (2035, 4, 21), (2035, 11, 3), 10.19),
    value_only(true, (2035, 6, 12), (2036, 7, 28), 17.52),
    value_only(false, (2037, 6, 2), (2037, 12, 17), 17.07),
    value_only(true, (2037, 6, 18), (2038, 7, 19), 14.84),
    value_only(false, (2039, 7, 19), (2040, 2, 16), 18.65),
    value_only(true, (2039, 7, 15), (2040, 7, 9), 12.17),
    row(false, (2041, 10, 31), (2042, 6, 20), 14.86),
    row(true, (2041, 10, 19), (2042, 8, 31), 9.818),
    row(false, (2043, 11, 23), (2044, 7, 20), 9.032),
    row(true, (2043, 11, 15), (2044, 9, 16), 8.969),
    row(false, (2046, 1, 8), (2046, 7, 27), 9.061),
    row(true, (2046, 1, 22), (2046, 12, 18), 8.587),
];

fn ymd((y, m, d): (i32, u32, u32)) -> Epoch {
    Epoch::from_ymd(y, m, d)
}

fn c3(eph: &Ephemeris, depart: Epoch, arrive: Epoch) -> Option<(f64, bool)> {
    let (earth, mars) = (eph.lookup("earth").unwrap(), eph.lookup("mars").unwrap());
    let t = Transfer::compute(eph, earth, mars, depart, arrive.0 - depart.0)?;
    Some((t.c3(), t.angle > PI))
}

/// Minimum C3 of one transfer type over a box of departure and arrival dates
/// (days relative to the centers): (C3, departure).
fn search(
    eph: &Ephemeris,
    type_ii: bool,
    dep0: Epoch,
    arr0: Epoch,
    dep_days: i32,
    arr_days: i32,
) -> (f64, Epoch) {
    let mut best = (f64::INFINITY, dep0);
    for di in -dep_days..=dep_days {
        let dep = dep0.plus_days(di as f64);
        for ai in -arr_days..=arr_days {
            let arr = arr0.plus_days(ai as f64);
            if arr.0 - dep.0 < 60.0 {
                continue;
            }
            if let Some((c3, ii)) = c3(eph, dep, arr)
                && ii == type_ii
                && c3 < best.0
            {
                best = (c3, dep);
            }
        }
    }
    best
}

#[test]
fn c3_matches_handbook() {
    let eph = Ephemeris::standard();
    for m in HANDBOOK {
        let label = format!("{} Type {}", m.depart.0, if m.type_ii { "II" } else { "I" });
        let (dep, arr) = (ymd(m.depart), ymd(m.arrive));
        if m.dates_consistent {
            // The tabulated transfer itself.
            let (at_dates, ii) = c3(eph, dep, arr).unwrap();
            assert_eq!(ii, m.type_ii, "{label}");
            let rel = at_dates / m.c3 - 1.0;
            // The best transfer nearby is at least as good, at about the same time.
            let (best, best_dep) = search(eph, m.type_ii, dep, arr, 40, 80);
            let shift = best_dep.0 - dep.0;
            println!(
                "{label}: handbook {:6.3}  at its dates {at_dates:6.3} ({:+.2}%)  best {best:6.3} on {best_dep} ({shift:+.0} d)",
                m.c3,
                rel * 100.0
            );
            assert!(
                rel.abs() < 0.002,
                "{label}: C3 at handbook dates off by {rel}"
            );
            assert!(best <= m.c3 * 1.002, "{label}: best {best}");
            assert!(
                shift.abs() <= 2.0,
                "{label}: best departure shifted {shift} d"
            );
        } else {
            let mid = Epoch((dep.0 + arr.0) / 2.0);
            let (best, best_dep) = search(
                eph,
                m.type_ii,
                dep.plus_days(70.0),
                mid.plus_days(140.0),
                150,
                260,
            );
            let rel = best / m.c3 - 1.0;
            println!(
                "{label}: handbook {:6.3}  (dates inconsistent)        best {best:6.3} on {best_dep} ({:+.2}%)",
                m.c3,
                rel * 100.0
            );
            assert!(rel.abs() < 0.002, "{label}: minimum C3 off by {rel}");
        }
    }
}
