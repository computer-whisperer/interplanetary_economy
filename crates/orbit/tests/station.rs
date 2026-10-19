//! A vessel's station orbit (M1b.md, step 3 plan, part 2): extended by
//! whole years on demand, the same whichever request comes first; a
//! window of it is the orbit; its member's station-keeping policy.

use orbit::ephem::Ephemeris;
use orbit::families::Catalog;
use orbit::gravity::Field;
use orbit::station::{MARGIN_DAYS, Orbit};
use orbit::time::{Epoch, SECONDS_PER_DAY};
use std::sync::Arc;

fn l1(eph: &'static Ephemeris, cat: &'static Catalog, from: Epoch, until: Epoch) -> Arc<Orbit> {
    let earth = eph.lookup("earth").unwrap();
    let field = Arc::new(Field::all(eph, Some(earth)).with_oblateness());
    let m = cat.nearest("l1-halo-north", 12.0).unwrap();
    Orbit::new(eph, cat, field, "l1", m, from, until).unwrap()
}

/// Two orbits built alike, one covered to 400 days in one request and the
/// other in two (200, then 400), are the same to the bit: the years are
/// the quantum. A window of the orbit is the orbit over its span, with the
/// margin each way. The member's policy holds its orbit.
#[test]
fn cover_is_the_same_whichever_request_comes_first() {
    let eph = Ephemeris::standard();
    let cat = Catalog::standard();
    let from = Epoch::from_ymd(2045, 3, 1);
    let arrive = from.plus_days(3.0);
    let (a, b) = (l1(eph, cat, from, arrive), l1(eph, cat, from, arrive));
    let first = a.end();
    assert!(first.0 - arrive.0 >= MARGIN_DAYS, "built past the arrival");
    a.cover(from.plus_days(400.0)).unwrap();
    b.cover(from.plus_days(200.0)).unwrap();
    assert!(b.end().0 >= from.plus_days(200.0 + MARGIN_DAYS).0);
    assert!(b.end().0 < a.end().0, "a year at a time");
    b.cover(from.plus_days(400.0)).unwrap();
    let (ra, rb) = (a.reference(), b.reference());
    assert_eq!(ra.path.span(), rb.path.span());
    assert_eq!(ra.patches, rb.patches);
    assert!(ra.end().0 >= from.plus_days(400.0 + MARGIN_DAYS).0);
    // The first build is untouched by extending.
    let epoch = a.epoch();
    let seconds = |t: Epoch| (t.0 - epoch.0) * SECONDS_PER_DAY;
    let (t0, t1) = (0.0, seconds(first));
    for k in 0..=20 {
        let t = t0 + (t1 - t0) * k as f64 / 20.0;
        assert_eq!(ra.path.at(t), rb.path.at(t));
    }
    drop((ra, rb));
    let (w0, w1) = (from.plus_days(300.0), from.plus_days(330.0));
    let w = a.window((w0, w1)).unwrap();
    let (lo, hi) = w.path.span();
    assert!(
        lo <= seconds(w0.plus_days(-MARGIN_DAYS)) && hi >= seconds(w1.plus_days(MARGIN_DAYS)),
        "window {lo}..{hi} s against {}..{}",
        seconds(w0.plus_days(-MARGIN_DAYS)),
        seconds(w1.plus_days(MARGIN_DAYS))
    );
    assert!(w.extension.is_none());
    let r = a.reference();
    for k in 0..=10 {
        let t = seconds(w0) + (seconds(w1) - seconds(w0)) * k as f64 / 10.0;
        assert_eq!(w.path.at(t), r.path.at(t));
    }
    let u = a.policy().expect("a policy holds the L1 halo");
    assert!(u.dv_per_year_kms * 1e3 > 1.0 && u.dv_per_year_kms * 1e3 < 10.0);
    assert_eq!(
        a.policy().map(|u| u.policy.interval_s),
        b.policy().map(|u| u.policy.interval_s)
    );
    println!(
        "l1 policy: every {:.1} rev aiming {:.1}, {:.2} m/s a year, worst {:.0} km",
        u.policy.interval_s / a.period_s(),
        u.policy.horizon_s / a.period_s(),
        u.dv_per_year_kms * 1e3,
        u.worst_deviation_km
    );
}
