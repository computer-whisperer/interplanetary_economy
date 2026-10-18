//! Catalog orbits carried into the full model.

use orbit::ephem::Ephemeris;
use orbit::families::{Catalog, Member, PeriodicOrbit};
use orbit::gravity::Field;
use orbit::integrate::{DenseStep, Dop853, Tolerance, Trajectory};
use orbit::places::{PlaceKind, Places};
use orbit::reference::{
    PATCHES_PER_REVOLUTION, Reference, RotatingFrame, continue_orbit, settle_revolutions,
};
use orbit::stationkeeping::{Errors, Policy, simulate, upkeep};
use orbit::time::{Epoch, SECONDS_PER_DAY};
use std::time::Instant;

const YEAR_S: f64 = 365.25 * SECONDS_PER_DAY;

#[test]
fn frame_round_trip() {
    let eph = Ephemeris::standard();
    let cat = Catalog::standard();
    let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
    let frame = RotatingFrame::at(eph, earth, moon, Epoch::from_ymd(2045, 3, 1));
    let s = [0.8, 0.1, -0.05, 0.02, 0.3, 0.01];
    let back = frame.to_rotating(cat.system.mu, &frame.to_inertial(cat.system.mu, &s));
    for i in 0..6 {
        assert!(
            (back[i] - s[i]).abs() < 1e-12,
            "{i}: {} vs {}",
            back[i],
            s[i]
        );
    }
}

/// The CR3BP orbit's closest and farthest distances from the Moon, km.
fn cr3bp_range(cat: &Catalog, orbit: &PeriodicOrbit) -> (f64, f64) {
    let ode = cat.system.ode();
    let mut run = Dop853::new(
        &ode,
        0.0,
        orbit.state,
        Tolerance {
            rtol: 1e-12,
            atol: 1e-15,
        },
    );
    let (mut lo, mut hi) = (f64::INFINITY, 0.0f64);
    for k in 1..=400 {
        run.advance_to(&ode, orbit.period * k as f64 / 400.0)
            .unwrap();
        let d = ((run.y[0] - 1.0 + cat.system.mu).powi(2) + run.y[1].powi(2) + run.y[2].powi(2))
            .sqrt()
            * cat.length_km;
        (lo, hi) = (lo.min(d), hi.max(d));
    }
    (lo, hi)
}

/// `reference`'s closest and farthest distances from the Moon, km, at
/// `samples` times over `(t0, t1)` (seconds after its epoch).
fn moon_range(
    eph: &Ephemeris,
    reference: &Reference,
    (t0, t1): (f64, f64),
    samples: usize,
) -> (f64, f64) {
    let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
    let (mut lo, mut hi) = (f64::INFINITY, 0.0f64);
    for k in 0..=samples {
        let sec = t0 + (t1 - t0) * k as f64 / samples as f64;
        let t = Epoch(reference.epoch.0 + sec / SECONDS_PER_DAY);
        let s = reference.path.at(sec);
        let to_moon = eph.barycentric_position(moon, t) - eph.barycentric_position(earth, t);
        let d = (glam::DVec3::new(s[0], s[1], s[2]) - to_moon).length();
        (lo, hi) = (lo.min(d), hi.max(d));
    }
    (lo, hi)
}

/// Whether distances from the Moon `(lo, hi)` keep the CR3BP orbit's
/// shape, `cr` (scaled by the real Earth–Moon distance, which varies ±5%).
fn shape_held((lo, hi): (f64, f64), cr: (f64, f64)) -> bool {
    lo > 0.7 * cr.0 && lo < 1.4 * cr.0 && hi > 0.8 * cr.1 && hi < 1.2 * cr.1
}

/// The NRHO, an L1 halo and a DRO carry into the full model for six
/// revolutions, keeping their shapes: their closest and farthest distances
/// from the Moon stay near the CR3BP orbit's.
#[test]
fn orbits_continue_into_the_full_model() {
    let eph = Ephemeris::standard();
    let cat = Catalog::standard();
    let earth = eph.lookup("earth").unwrap();
    let field = Field::all(eph, Some(earth)).with_oblateness();
    let start = Epoch::from_ymd(2045, 3, 1);
    for (family, days) in [
        ("l2-halo-south", 6.57),
        ("l1-halo-north", 12.0),
        ("dro", 14.0),
    ] {
        let m = cat.nearest(family, days).unwrap();
        let reference = continue_orbit(eph, &field, cat, m.orbit, start, 6, 1).unwrap();
        assert!(reference.ahead.is_empty());
        let cr = cr3bp_range(cat, &m.orbit);
        let (lo, hi) = moon_range(eph, &reference, reference.path.span(), 2000);
        println!(
            "{family} ({:.2} d): {lo:.0}-{hi:.0} km from the Moon over {:.0} days (CR3BP {:.0}-{:.0})",
            m.orbit.period * cat.time_s / SECONDS_PER_DAY,
            reference.path.span().1 / SECONDS_PER_DAY,
            cr.0,
            cr.1
        );
        assert!(shape_held((lo, hi), cr), "{family}: {lo}-{hi} km");
    }
}

/// The four station places' catalog orbits: (place, member).
fn stations(cat: &Catalog) -> Vec<(String, Member)> {
    Places::standard()
        .iter()
        .filter_map(|p| match &p.kind {
            PlaceKind::Station { family, period } => {
                Some((p.id.clone(), *cat.nearest(family, *period).unwrap()))
            }
            _ => None,
        })
        .collect()
}

/// The largest difference between two states in the shooting's scaled
/// units (1 = the Earth–Moon distance, or its velocity unit), as
/// `continue_orbit` measures its arcs' discontinuities.
fn apart(cat: &Catalog, a: &[f64; 6], b: &[f64; 6]) -> f64 {
    let unit = [cat.length_km, cat.length_km / cat.time_s];
    (0..6).fold(0.0, |m: f64, i| m.max((a[i] - b[i]).abs() / unit[i / 3]))
}

/// How an extension left the reference: how far the anchor revolution's
/// patches moved (the first of them replaces the old last patch), and the
/// discontinuity where the new path starts, both scaled as [`apart`].
fn seam(cat: &Catalog, before: &Reference, after: &Reference) -> (f64, f64) {
    let moved = before
        .ahead
        .iter()
        .zip(&after.patches[before.patches.len() - 1..])
        .map(|((ta, sa), (tb, sb))| apart(cat, sa, sb).max((ta - tb).abs() / cat.time_s))
        .fold(0.0, f64::max);
    let end = before.path.span().1;
    let first_new = &after.path.steps()[before.path.steps().len()];
    (moved, apart(cat, &before.path.at(end), &first_new.at(end)))
}

/// Each station's orbit extended (M1b.md, step 3 plan, part 1): the anchor
/// revolution stays within the shooting's tolerance (1e-8 scaled: ~4 m,
/// 0.01 mm/s), the path runs on as continuously as its arcs, the period
/// and the shape hold, and the path so far is untouched.
#[test]
fn extended_orbits_run_on() {
    let eph = Ephemeris::standard();
    let cat = Catalog::standard();
    let earth = eph.lookup("earth").unwrap();
    let field = Field::all(eph, Some(earth)).with_oblateness();
    let start = Epoch::from_ymd(2045, 3, 1);
    for (place, m) in stations(cat) {
        let settle = settle_revolutions(m.stability);
        // A start that does not converge is retried a day or two later, as
        // the gate and the planner retry.
        let mut r = (0..3)
            .find_map(|late| {
                continue_orbit(
                    eph,
                    &field,
                    cat,
                    m.orbit,
                    start.plus_days(late as f64),
                    8,
                    settle,
                )
                .ok()
            })
            .unwrap();
        assert_eq!(r.ahead.len(), PATCHES_PER_REVOLUTION + 1);
        let cr = cr3bp_range(cat, &m.orbit);
        for k in 0..2 {
            let before = r.clone();
            let end = before.path.span().1;
            r.extend(eph, &field, cat, m.orbit, 8, settle).unwrap();
            let (moved, gap) = seam(cat, &before, &r);
            println!("{place} extension {k}: anchor moved {moved:.1e}, seam {gap:.1e}");
            assert!(moved < 1e-8, "{place} {k}: the anchor moved {moved:e}");
            assert!(gap < 1e-8, "{place} {k}: the seam's discontinuity {gap:e}");
            let (old, new) = (
                before.path.steps(),
                &r.path.steps()[..before.path.steps().len()],
            );
            assert!(
                old.iter()
                    .zip(new)
                    .all(|(a, b)| a.t0 == b.t0 && a.h == b.h && a.at(a.t0) == b.at(b.t0)),
                "{place} {k}: the path so far changed"
            );
            assert!(r.path.span().1 > end);
            assert!(
                (r.period_s() / before.period_s() - 1.0).abs() < 0.01,
                "{place} {k}: {} s a revolution, then {}",
                before.period_s(),
                r.period_s()
            );
            let range = moon_range(eph, &r, (end, r.path.span().1), 500);
            assert!(shape_held(range, cr), "{place} {k}: {range:?} km");
        }
        assert_eq!(r.revolutions, 24);
        assert_eq!(r.patches.len(), 24 * PATCHES_PER_REVOLUTION + 1);
    }
}

/// `reference` over `(t0, t1)` only (seconds after its epoch, whole
/// revolutions), as a reference of its own.
fn window(reference: &Reference, (t0, t1): (f64, f64)) -> Reference {
    let steps: Vec<DenseStep<6>> = reference
        .path
        .steps()
        .iter()
        .filter(|s| s.t0 + s.h > t0 && s.t0 < t1)
        .cloned()
        .collect();
    Reference {
        epoch: reference.epoch,
        center: reference.center,
        revolutions: ((t1 - t0) / reference.period_s()).round() as usize,
        patches: Vec::new(),
        ahead: Vec::new(),
        path: Trajectory::new(steps),
    }
}

/// Measured (M1b.md, step 3 plan, part 1): each station's orbit from
/// 2045-03-01 extended a year at a time for `YEARS` years (34, to 2079: the
/// ephemeris ends in 2080; `ONLY` picks one place), each orbit on its own
/// thread: the time and
/// memory per year, the anchors' moves, the seams, the shape; `upkeep` over
/// the first two years against the first year's; then the station-keeping
/// loop across the first three seams, each policy's Δv bucketed before the
/// seam, in the four revolutions after it, and after those, over three
/// seeded runs, the cheapest policy that holds them all.
#[test]
#[ignore]
fn station_orbits_extended_for_years() {
    let years: usize = std::env::var("YEARS").map_or(34, |v| v.parse().unwrap());
    let only = std::env::var("ONLY").ok();
    let eph = Ephemeris::standard();
    let cat = Catalog::standard();
    let earth = eph.lookup("earth").unwrap();
    let field = Field::all(eph, Some(earth)).with_oblateness();
    let start = Epoch::from_ymd(2045, 3, 1);
    std::thread::scope(|s| {
        for (place, m) in stations(cat) {
            if only.as_deref().is_some_and(|o| o != place) {
                continue;
            }
            let field = &field;
            s.spawn(move || {
                let settle = settle_revolutions(m.stability);
                let per_year = (365.25 / cat.days(m.orbit)).ceil() as usize;
                let cr = cr3bp_range(cat, &m.orbit);
                let clock = Instant::now();
                let mut r = (0..3)
                    .find_map(|late| {
                        let start = start.plus_days(late as f64);
                        continue_orbit(eph, field, cat, m.orbit, start, per_year, settle).ok()
                    })
                    .unwrap();
                let mut year_times = vec![clock.elapsed().as_secs_f64()];
                let (mut moved, mut gaps) = (Vec::new(), Vec::new());
                let mut failed = None;
                for _ in 1..years {
                    let before = r.clone();
                    let clock = Instant::now();
                    if let Err(e) = r.extend(eph, field, cat, m.orbit, per_year, settle) {
                        failed = Some(e.residual);
                        break;
                    }
                    year_times.push(clock.elapsed().as_secs_f64());
                    let (mv, gap) = seam(cat, &before, &r);
                    moved.push(mv);
                    gaps.push(gap);
                }
                let built = r.revolutions / per_year;
                let bytes = std::mem::size_of_val(r.path.steps());
                // Seconds after the epoch where year `y` of the path begins.
                let year_start = |y: usize| r.patches[y * per_year * PATCHES_PER_REVOLUTION].0;
                let held = (0..built)
                    .filter(|&y| {
                        let span = (year_start(y), year_start(y + 1));
                        shape_held(moon_range(eph, &r, span, 300), cr)
                    })
                    .count();
                let (lo, hi) = moon_range(eph, &r, r.path.span(), 300 * built);
                let max = |v: &[f64]| v.iter().copied().fold(0.0, f64::max);
                println!(
                    "{place} (settle {settle}, {per_year} revolutions a year): {built} years{} in \
                     {:.1} s ({:.1} s a year, the first {:.1}); {:.0} MB ({:.1} MB a year); anchors \
                     moved up to {:.1e}, seams up to {:.1e}; {lo:.0}-{hi:.0} km from the Moon \
                     (CR3BP {:.0}-{:.0}), shape held in {held} years",
                    failed.map_or(String::new(), |res| format!(", then failed ({res:.1e})")),
                    year_times.iter().sum::<f64>(),
                    year_times[1..].iter().sum::<f64>() / year_times[1..].len().max(1) as f64,
                    year_times[0],
                    bytes as f64 / 1e6,
                    bytes as f64 / 1e6 / built as f64,
                    max(&moved),
                    max(&gaps),
                    cr.0,
                    cr.1
                );
                if built < 4 {
                    return;
                }
                // The loop: the policy over the first two years, against
                // the first year's.
                let period = r.period_s();
                let whole = |t: f64| (t / period).round() * period;
                let clock = Instant::now();
                let one = upkeep(field, &window(&r, (0.0, whole(YEAR_S))), Errors::TYPICAL, 4);
                let two = upkeep(field, &window(&r, (0.0, whole(2.0 * YEAR_S))), Errors::TYPICAL, 4);
                let describe = |u: &Option<orbit::stationkeeping::Upkeep>| match u {
                    Some(u) => format!(
                        "{:.2} m/s a year (every {:.1} rev aiming {:.1})",
                        u.dv_per_year_kms * 1e3,
                        u.policy.interval_s / period,
                        u.policy.horizon_s / period
                    ),
                    None => "not held".to_string(),
                };
                println!(
                    "{place} upkeep: first year {}; first two years {} ({:.0} s)",
                    describe(&one),
                    describe(&two),
                    clock.elapsed().as_secs_f64()
                );
                // The loop across the first three seams.
                for seam_year in 1..=3 {
                    let at = year_start(seam_year);
                    let (from, to) = (
                        at - whole(60.0 * SECONDS_PER_DAY),
                        at + whole(200.0 * SECONDS_PER_DAY),
                    );
                    let w = window(&r, (from, to));
                    let settle_s = 4.0 * period;
                    let mut held = Vec::new();
                    for interval in [0.5, 1.0] {
                        for horizon in [0.5, 1.0, 2.0] {
                            let policy = Policy {
                                interval_s: interval * period,
                                horizon_s: horizon * period,
                                deadband_km: 10.0,
                            };
                            let runs: Vec<_> = (0..3u64)
                                .filter_map(|seed| simulate(field, &w, policy, Errors::TYPICAL, seed))
                                .filter(|run| run.worst_deviation_km <= 500.0)
                                .collect();
                            if runs.len() < 3 {
                                continue;
                            }
                            let (mut pre, mut hand, mut post) = (0.0, 0.0, 0.0);
                            for b in runs.iter().flat_map(|run| &run.burns) {
                                if b.t < at {
                                    pre += b.dv_kms;
                                } else if b.t < at + settle_s {
                                    hand += b.dv_kms;
                                } else {
                                    post += b.dv_kms;
                                }
                            }
                            let n = runs.len() as f64;
                            let total = (pre + hand + post) / n * 1e3;
                            let pre_rate = pre / n / (at - from) * YEAR_S * 1e3;
                            let post_rate =
                                post / n / (to - policy.horizon_s - at - settle_s) * YEAR_S * 1e3;
                            let hand = hand / n * 1e3;
                            let extra = hand - post_rate * settle_s / YEAR_S;
                            let worst = runs.iter().map(|run| run.worst_deviation_km).fold(0.0, f64::max);
                            held.push((
                                total,
                                format!(
                                    "every {interval} rev aiming {horizon}: {total:.2} m/s over the \
                                     window; before {pre_rate:.2} m/s a year; four revolutions after \
                                     {hand:.2} m/s ({extra:+.2} over the rate after); after \
                                     {post_rate:.2} m/s a year; worst {worst:.0} km"
                                ),
                            ));
                        }
                    }
                    held.sort_by(|x, y| x.0.total_cmp(&y.0));
                    let lines: Vec<String> = held.iter().map(|(_, l)| format!("\n  {l}")).collect();
                    println!(
                        "{place} seam at year {seam_year} ({:.0} days in): {} of 6 policies hold, \
                         cheapest first:{}",
                        at / SECONDS_PER_DAY,
                        held.len(),
                        lines.join("")
                    );
                }
            });
        }
    });
}
