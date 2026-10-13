//! Transfer windows: the departure periods when a route is cheap.

use crate::time::Epoch;
use crate::transfer::{Porkchop, Transfer};

#[derive(Clone, Copy, Debug)]
pub struct Window {
    /// First and last dates to leave the origin node within the margin of
    /// the best Δv (heliocentric departures, less the best transfer's
    /// departure maneuver time).
    pub open: Epoch,
    pub close: Epoch,
    /// The lowest-Δv transfer departing in the window.
    pub best: Transfer,
    /// The window runs into the edge of the searched departure range.
    pub truncated: bool,
    /// The best transfer's time of flight is at the edge of the searched
    /// range, so a better one may take longer or shorter.
    pub tof_limited: bool,
}

/// A local minimum is only a window if its Δv is within this factor of the
/// season's best: off-season dips are minima too.
const SEASON_FACTOR: f64 = 1.5;

/// The season compared against: half a synodic period either side, capped
/// so that for small bodies with synodic periods of decades the comparison
/// stays local and does not depend on how wide a range was searched.
const SEASON_MAX_DAYS: f64 = 730.0;

/// Finds windows in a porkchop: each local minimum (over departure dates) of
/// the best Δv, and the surrounding departures within `margin` km/s of it.
///
/// A minimum must be the lowest within a third of the synodic period, capped
/// at a third of a year (a small body with a synodic period of years still
/// offers an opportunity each time Earth comes around), and within
/// [`SEASON_FACTOR`] of the season's best. A minimum on the edge of the
/// departure range is not a minimum of the route, only of the search, and is
/// not reported; search a wider range to see it. Overlapping windows are one
/// opportunity, described by the deeper minimum's own window.
pub fn find_windows(porkchop: &Porkchop, synodic_days: f64, margin: f64) -> Vec<Window> {
    let grid = porkchop.grid;
    let best = porkchop.best_by_departure();
    let dv: Vec<f64> = best
        .iter()
        .map(|t| t.map_or(f64::INFINITY, |t| t.dv()))
        .collect();
    let n = dv.len();
    let steps = |days: f64| ((days / grid.depart_step).round() as usize).clamp(1, n.max(1));
    let reach = steps(synodic_days.min(365.25) / 3.0);
    let season = steps((synodic_days / 2.0).min(SEASON_MAX_DAYS));

    let mut windows: Vec<Window> = Vec::new();
    for i in 1..n.saturating_sub(1) {
        if !dv[i].is_finite() {
            continue;
        }
        let (lo, hi) = (i.saturating_sub(reach), (i + reach).min(n - 1));
        // Strict on the left so a flat bottom yields one minimum.
        let is_min = (lo..i).all(|k| dv[k] > dv[i]) && (i + 1..=hi).all(|k| dv[k] >= dv[i]);
        let season_best = dv[i.saturating_sub(season)..=(i + season).min(n - 1)]
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        if !is_min || dv[i] > SEASON_FACTOR * season_best {
            continue;
        }
        let limit = dv[i] + margin;
        let mut open = i;
        while open > 0 && dv[open - 1] <= limit {
            open -= 1;
        }
        let mut close = i;
        while close + 1 < n && dv[close + 1] <= limit {
            close += 1;
        }
        let b = best[i].unwrap();
        let lead = -b.departure.days;
        let window = Window {
            open: grid.depart(open).plus_days(lead),
            close: grid.depart(close).plus_days(lead),
            best: b,
            truncated: open == 0 || close == n - 1,
            tof_limited: b.tof_days <= grid.tof(0) || b.tof_days >= grid.tof(grid.tofs - 1),
        };
        match windows.last_mut() {
            Some(prev) if prev.close.0 >= window.open.0 => {
                if window.best.dv() < prev.best.dv() {
                    *prev = window;
                }
            }
            _ => windows.push(window),
        }
    }
    windows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ephem::Ephemeris;
    use crate::network::Network;
    use crate::transfer::{Grid, Route};

    #[test]
    fn earth_mars_windows_2040s() {
        let eph = Ephemeris::standard();
        let net = Network::standard(&eph);
        let route = Route {
            from: net.lookup("leo").unwrap(),
            to: net.lookup("lmo").unwrap(),
            aero_capable: false,
        };
        let (start, end) = (Epoch::from_ymd(2040, 1, 1), Epoch::from_ymd(2046, 6, 1));
        let grid = Grid::covering(start, end, 2.0, 80.0, 520.0, 4.0);
        let porkchop = Porkchop::compute(&eph, &net, route, grid).unwrap();
        let synodic = eph.synodic_period_days(
            net.node(route.from).system,
            net.node(route.to).system,
            start,
        );
        assert!((synodic - 780.0).abs() < 5.0, "{synodic}");

        let windows = find_windows(&porkchop, synodic, 0.5);
        for w in &windows {
            println!(
                "{} – {} best {} {:.2} km/s",
                w.open,
                w.close,
                w.best.depart,
                w.best.dv()
            );
        }
        // Opportunities in late 2041, late 2043 and around the new year 2046
        // (the handbook's minimum-C3 departures: 2041-10-19, 2043-11-15,
        // 2046-01-22). The total-Δv optimum also weighs Mars arrival speed.
        let expected = [(2041, 10, 19), (2043, 11, 15), (2046, 1, 22)];
        assert_eq!(windows.len(), 3);
        for (w, (y, m, d)) in windows.iter().zip(expected) {
            let offset = w.best.depart.0 - Epoch::from_ymd(y, m, d).0;
            assert!(offset.abs() < 45.0, "{} vs {y}-{m}-{d}", w.best.depart);
            assert!(w.open.0 <= w.best.leave().0 && w.best.leave().0 <= w.close.0);
            assert!((5.0..6.5).contains(&w.best.dv()), "{}", w.best.dv());
            assert!(!w.truncated && !w.tof_limited);
        }
    }

    fn windows_for(to: &str, start: Epoch, end: Epoch) -> (Porkchop, Vec<Window>) {
        let eph = Ephemeris::standard();
        let net = Network::standard(&eph);
        let route = Route {
            from: net.lookup("leo").unwrap(),
            to: net.lookup(to).unwrap(),
            aero_capable: false,
        };
        let (lo, hi) = route.default_tof_range(&eph, &net, start);
        let pc = Porkchop::compute(
            &eph,
            &net,
            route,
            Grid::covering(start, end, 2.0, lo, hi, 4.0),
        )
        .unwrap();
        let synodic = eph.synodic_period_days(
            net.node(route.from).system,
            net.node(route.to).system,
            start,
        );
        let windows = find_windows(&pc, synodic, 0.5);
        (pc, windows)
    }

    /// Every departure inside a window is within the margin of its best.
    /// (Regression: merging overlapping windows once took their union.)
    #[test]
    fn windows_hold_only_cheap_departures() {
        for (to, y0, y1) in [
            ("itokawa", 2068, 2073),
            ("2017-sv19", 2060, 2065),
            ("lmo", 2040, 2050),
        ] {
            let (pc, windows) =
                windows_for(to, Epoch::from_ymd(y0, 1, 1), Epoch::from_ymd(y1, 1, 1));
            let best = pc.best_by_departure();
            assert!(!windows.is_empty());
            for w in &windows {
                for (i, t) in best.iter().enumerate() {
                    let d = pc.grid.depart(i).plus_days(-w.best.departure.days);
                    if d.0 >= w.open.0 && d.0 <= w.close.0 {
                        let dv = t.unwrap().dv();
                        assert!(
                            dv <= w.best.dv() + 0.5 + 1e-9,
                            "{to}: {d} at {dv:.2} in window best {:.2}",
                            w.best.dv()
                        );
                    }
                }
            }
        }
    }

    /// Windows away from the range edges do not depend on the range searched.
    #[test]
    fn windows_do_not_depend_on_search_range() {
        let key = |ws: &[Window]| -> Vec<(String, String)> {
            ws.iter()
                .filter(|w| {
                    w.best.depart.0 > Epoch::from_ymd(2046, 1, 1).0
                        && w.best.depart.0 < Epoch::from_ymd(2068, 1, 1).0
                })
                .map(|w| (w.best.depart.to_string(), format!("{:.3}", w.best.dv())))
                .collect()
        };
        for to in ["2009-uy19", "2006-bz147"] {
            let (_, narrow) =
                windows_for(to, Epoch::from_ymd(2040, 1, 1), Epoch::from_ymd(2074, 1, 1));
            let (_, wide) =
                windows_for(to, Epoch::from_ymd(2038, 1, 1), Epoch::from_ymd(2076, 1, 1));
            assert_eq!(key(&narrow), key(&wide), "{to}");
        }
    }
}
