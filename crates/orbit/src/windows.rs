//! Transfer windows: the departure periods when a route is cheap.

use crate::time::Epoch;
use crate::transfer::{Porkchop, Transfer};

#[derive(Clone, Copy, Debug)]
pub struct Window {
    /// First and last departure dates within the margin of the best Δv.
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
/// best within half a synodic period: off-season dips are minima too.
const SEASON_FACTOR: f64 = 1.5;

/// Finds windows in a porkchop: each local minimum (over departure dates) of
/// the best Δv, and the surrounding departures within `margin` km/s of it.
///
/// A minimum must be the lowest within a third of the synodic period, capped
/// at a third of a year (a small body with a synodic period of years still
/// offers an opportunity each time Earth comes around), and within
/// [`SEASON_FACTOR`] of the best within half a synodic period. A
/// minimum on
/// the edge of the departure range is not a minimum of the route, only of
/// the search, and is not reported; search a wider range to see it.
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
    let season = steps(synodic_days / 2.0);

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
        let window = Window {
            open: grid.depart(open),
            close: grid.depart(close),
            best: b,
            truncated: open == 0 || close == n - 1,
            tof_limited: b.tof_days <= grid.tof(0) || b.tof_days >= grid.tof(grid.tofs - 1),
        };
        // A shallow second minimum inside the same window is the same
        // opportunity.
        match windows.last_mut() {
            Some(prev) if prev.close.0 >= window.open.0 => {
                prev.close = Epoch(prev.close.0.max(window.close.0));
                prev.truncated |= window.truncated;
                if window.best.dv() < prev.best.dv() {
                    prev.best = window.best;
                    prev.tof_limited = window.tof_limited;
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
            assert!(w.open.0 <= w.best.depart.0 && w.best.depart.0 <= w.close.0);
            assert!((5.0..6.5).contains(&w.best.dv()), "{}", w.best.dv());
            assert!(!w.truncated && !w.tof_limited);
        }
    }
}
