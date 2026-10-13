//! Transfer windows: the departure periods when a pair of bodies is cheap
//! to travel between.

use crate::time::Epoch;
use crate::transfer::{Porkchop, Transfer};

#[derive(Clone, Copy, Debug)]
pub struct Window {
    /// First and last departures within the margin of the best total excess
    /// speed.
    pub open: Epoch,
    pub close: Epoch,
    /// The transfer with the least total excess speed in the window.
    pub best: Transfer,
    /// The window runs into the edge of the searched departure range.
    pub truncated: bool,
    /// The best transfer's time of flight is at the edge of the searched
    /// range, so a better one may take longer or shorter.
    pub tof_limited: bool,
}

/// A local minimum is only a window if its total excess speed is within
/// this factor of the season's best: off-season dips are minima too.
const SEASON_FACTOR: f64 = 1.5;

/// The season compared against: half a synodic period either side, capped
/// so that for small bodies with synodic periods of decades the comparison
/// stays local and does not depend on how wide a range was searched.
const SEASON_MAX_DAYS: f64 = 730.0;

/// Finds windows in a porkchop: each local minimum (over departure dates) of
/// the best total excess speed, and the surrounding departures within
/// `margin` km/s of it.
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
    let v_inf: Vec<f64> = best
        .iter()
        .map(|t| t.map_or(f64::INFINITY, |t| t.v_inf_total()))
        .collect();
    let n = v_inf.len();
    let steps = |days: f64| ((days / grid.depart_step).round() as usize).clamp(1, n.max(1));
    let reach = steps(synodic_days.min(365.25) / 3.0);
    let season = steps((synodic_days / 2.0).min(SEASON_MAX_DAYS));

    let mut windows: Vec<Window> = Vec::new();
    for i in 1..n.saturating_sub(1) {
        if !v_inf[i].is_finite() {
            continue;
        }
        let (lo, hi) = (i.saturating_sub(reach), (i + reach).min(n - 1));
        // Strict on the left so a flat bottom yields one minimum.
        let is_min =
            (lo..i).all(|k| v_inf[k] > v_inf[i]) && (i + 1..=hi).all(|k| v_inf[k] >= v_inf[i]);
        let season_best = v_inf[i.saturating_sub(season)..=(i + season).min(n - 1)]
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);
        if !is_min || v_inf[i] > SEASON_FACTOR * season_best {
            continue;
        }
        let limit = v_inf[i] + margin;
        let mut open = i;
        while open > 0 && v_inf[open - 1] <= limit {
            open -= 1;
        }
        let mut close = i;
        while close + 1 < n && v_inf[close + 1] <= limit {
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
        match windows.last_mut() {
            Some(prev) if prev.close.0 >= window.open.0 => {
                if window.best.v_inf_total() < prev.best.v_inf_total() {
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
    use crate::transfer::{Grid, default_tof_range};

    #[test]
    fn earth_mars_windows_2040s() {
        let eph = Ephemeris::standard();
        let (earth, mars) = (eph.lookup("earth").unwrap(), eph.lookup("mars").unwrap());
        let (start, end) = (Epoch::from_ymd(2040, 1, 1), Epoch::from_ymd(2046, 6, 1));
        let grid = Grid::covering(start, end, 2.0, 80.0, 520.0, 4.0);
        let porkchop = Porkchop::compute(&eph, earth, mars, grid);
        let synodic = eph.synodic_period_days(earth, mars, start);
        assert!((synodic - 780.0).abs() < 5.0, "{synodic}");

        let windows = find_windows(&porkchop, synodic, 0.5);
        for w in &windows {
            println!(
                "{} – {} best {} v∞ {:.2} + {:.2} km/s, C3 {:.2}",
                w.open,
                w.close,
                w.best.depart,
                w.best.v_inf_depart,
                w.best.v_inf_arrive,
                w.best.c3()
            );
        }
        // Opportunities in late 2041, late 2043 and around the new year 2046
        // (the handbook's minimum-C3 departures: 2041-10-19, 2043-11-15,
        // 2046-01-22). Total excess speed also weighs Mars arrival speed.
        let expected = [(2041, 10, 19), (2043, 11, 15), (2046, 1, 22)];
        assert_eq!(windows.len(), 3);
        for (w, (y, m, d)) in windows.iter().zip(expected) {
            let offset = w.best.depart.0 - Epoch::from_ymd(y, m, d).0;
            assert!(offset.abs() < 45.0, "{} vs {y}-{m}-{d}", w.best.depart);
            assert!(w.open.0 <= w.best.depart.0 && w.best.depart.0 <= w.close.0);
            assert!(
                (5.0..6.5).contains(&w.best.v_inf_total()),
                "{}",
                w.best.v_inf_total()
            );
            assert!(!w.truncated && !w.tof_limited);
        }
    }

    fn windows_for(to: &str, start: Epoch, end: Epoch) -> (Porkchop, Vec<Window>) {
        let eph = Ephemeris::standard();
        let (earth, to) = (eph.lookup("earth").unwrap(), eph.lookup(to).unwrap());
        let (lo, hi) = default_tof_range(&eph, earth, to, start);
        let pc = Porkchop::compute(
            &eph,
            earth,
            to,
            Grid::covering(start, end, 2.0, lo, hi, 4.0),
        );
        let synodic = eph.synodic_period_days(earth, to, start);
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
            ("mars", 2040, 2050),
        ] {
            let (pc, windows) =
                windows_for(to, Epoch::from_ymd(y0, 1, 1), Epoch::from_ymd(y1, 1, 1));
            let best = pc.best_by_departure();
            assert!(!windows.is_empty());
            for w in &windows {
                for (i, t) in best.iter().enumerate() {
                    let d = pc.grid.depart(i);
                    if d.0 >= w.open.0 && d.0 <= w.close.0 {
                        let v = t.unwrap().v_inf_total();
                        assert!(
                            v <= w.best.v_inf_total() + 0.5 + 1e-9,
                            "{to}: {d} at {v:.2} in window best {:.2}",
                            w.best.v_inf_total()
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
                .map(|w| {
                    (
                        w.best.depart.to_string(),
                        format!("{:.3}", w.best.v_inf_total()),
                    )
                })
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
