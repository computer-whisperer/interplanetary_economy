//! The time-expanded transport network: every way to move between two
//! places of the route graph (`data/routes.toml`, M1a.md step 11), by the
//! period a ship leaves and the period it arrives.

use crate::calendar::Calendar;
use orbit::ephem::{BodyId, Ephemeris};
use orbit::routes::{PlaceId, RouteGraph};
use orbit::transfer::{Grid, Porkchop, default_tof_range};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

/// The route graph the game uses.
pub fn standard(eph: &Ephemeris) -> RouteGraph {
    RouteGraph::from_toml(include_str!("../../../data/routes.toml"), eph)
        .expect("data/routes.toml is valid")
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdgeKind {
    /// A leg within one planetary system. It takes days, so it leaves and
    /// arrives in the same period.
    Local,
    /// The cheapest transfer between systems leaving in its period.
    Interplanetary,
}

#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub from: PlaceId,
    pub to: PlaceId,
    pub leave: usize,
    pub arrive: usize,
    /// Total Δv, all of it loaded at `from`.
    pub dv: f64,
    /// Place to place.
    pub days: f64,
    pub kind: EdgeKind,
    /// Through an atmosphere: aero-capable ships only.
    pub aero: bool,
}

/// Porkchop resolution for interplanetary edges, days.
const DEPART_STEP: f64 = 2.0;
const TOF_STEP: f64 = 4.0;

/// Every leg within a system in every period.
pub fn local_edges(graph: &RouteGraph, cal: &Calendar) -> Vec<Edge> {
    graph
        .legs()
        .iter()
        .flat_map(|l| {
            (0..cal.periods).map(move |p| Edge {
                from: l.from,
                to: l.to,
                leave: p,
                arrive: p,
                dv: l.dv,
                days: l.days,
                kind: EdgeKind::Local,
                aero: l.aero,
            })
        })
        .collect()
}

/// The pairs of places the optimizer moves between systems: from a place
/// ships can leave to one they can arrive at, in different systems, one of
/// them Earth's. (Mars–asteroid and asteroid–asteroid routes are left out
/// for now.)
pub fn pairs(eph: &Ephemeris, graph: &RouteGraph) -> Vec<(PlaceId, PlaceId)> {
    let earth = eph.lookup("earth").expect("Earth is in the ephemeris");
    let mut out = Vec::new();
    for (a, pa) in graph.places() {
        for (b, pb) in graph.places() {
            let (Some(ga), Some(gb)) = (pa.gateway, pb.gateway) else {
                continue;
            };
            if pa.system == pb.system || (pa.system != earth && pb.system != earth) {
                continue;
            }
            if ga.depart.is_some() && (gb.arrive.is_some() || gb.pass.is_some()) {
                out.push((a, b));
            }
        }
    }
    out
}

/// For each pair, each way of arriving (burning, or through the
/// atmosphere) and each period, the cheapest transfer that leaves the first
/// place in that period and reaches the second before the calendar ends, if
/// it costs at most `max_dv`. Each cell of a porkchop between the systems'
/// bodies is priced by both places' gateways; a ship leaves the place a
/// gateway's days before the heliocentric arc starts, and reaches the other
/// that long after it ends.
pub fn interplanetary_edges(
    eph: &Ephemeris,
    graph: &RouteGraph,
    cal: &Calendar,
    pairs: &[(PlaceId, PlaceId)],
    max_dv: f64,
) -> Vec<Edge> {
    let bodies: Vec<(BodyId, BodyId)> = pairs
        .iter()
        .map(|&(a, b)| (graph.place(a).system, graph.place(b).system))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let porkchops: HashMap<(BodyId, BodyId), Porkchop> = bodies
        .par_iter()
        .map(|&(x, y)| {
            let (lo, hi) = default_tof_range(eph, x, y, cal.start);
            let grid = Grid::covering(cal.start, cal.end(), DEPART_STEP, lo, hi, TOF_STEP);
            ((x, y), Porkchop::compute(eph, x, y, grid))
        })
        .collect();
    pairs
        .par_iter()
        .flat_map_iter(|&(a, b)| {
            let (pa, pb) = (graph.place(a), graph.place(b));
            let (ga, gb) = (pa.gateway.unwrap(), pb.gateway.unwrap());
            let (gm_a, gm_b) = (eph.body(pa.system).gm, eph.body(pb.system).gm);
            let pc = &porkchops[&(pa.system, pb.system)];
            // The cheapest by period of leaving, burning and aerocapturing.
            let mut best: [Vec<Option<Edge>>; 2] =
                [vec![None; cal.periods], vec![None; cal.periods]];
            for i in 0..pc.grid.departs {
                for j in 0..pc.grid.tofs {
                    let Some(t) = pc.get(i, j) else { continue };
                    let Some(out) = ga.departure(gm_a, t.v_inf_depart) else {
                        continue;
                    };
                    let leaves = t.depart.plus_days(-ga.days);
                    let arrives = t.arrive().plus_days(gb.days);
                    let (Some(leave), Some(arrive)) =
                        (cal.period_of(leaves), cal.period_of(arrives))
                    else {
                        continue;
                    };
                    let ways = [
                        gb.arrival(gm_b, t.v_inf_arrive),
                        gb.aerocapture(gm_b, t.v_inf_arrive),
                    ];
                    for (aero, into) in ways.into_iter().enumerate() {
                        let Some(into) = into else { continue };
                        let dv = out + into;
                        let slot = &mut best[aero][leave];
                        if dv <= max_dv && slot.is_none_or(|e| dv < e.dv) {
                            *slot = Some(Edge {
                                from: a,
                                to: b,
                                leave,
                                arrive,
                                dv,
                                days: arrives.0 - leaves.0,
                                kind: EdgeKind::Interplanetary,
                                aero: aero == 1,
                            });
                        }
                    }
                }
            }
            best.into_iter()
                .flat_map(|way| undominated(way.into_iter().flatten().collect()))
        })
        .collect()
}

/// Drops each edge for which another of the same pair and way leaves no
/// earlier, arrives no later and costs no more: waiting for it loses
/// nothing. `edges` are sorted by departure.
fn undominated(edges: Vec<Edge>) -> Vec<Edge> {
    let mut keep = Vec::with_capacity(edges.len());
    for (i, e) in edges.iter().enumerate() {
        let dominated = edges[i + 1..]
            .iter()
            .any(|f| f.arrive <= e.arrive && f.dv <= e.dv);
        if !dominated {
            keep.push(*e);
        }
    }
    keep
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit::time::Epoch;

    #[test]
    fn mars_edges_follow_the_windows() {
        let eph = Ephemeris::standard();
        let graph = standard(eph);
        let cal = Calendar::monthly(Epoch::from_ymd(2040, 1, 1), Epoch::from_ymd(2047, 1, 1));
        let pair = (graph.lookup("leo").unwrap(), graph.lookup("lmo").unwrap());
        let edges = interplanetary_edges(eph, &graph, &cal, &[pair], 4.5);
        // Cheap departures cluster around the 2041, 2043 and 2045–46 windows.
        let months: Vec<Epoch> = edges.iter().map(|e| cal.start_of(e.leave)).collect();
        for e in &edges {
            assert!(e.arrive > e.leave && e.dv <= 4.5);
        }
        for (y, m) in [(2041, 10), (2043, 11), (2046, 1)] {
            let w = Epoch::from_ymd(y, m, 1);
            assert!(
                months.iter().any(|d| (d.0 - w.0).abs() < 62.0),
                "no edge near {y}-{m}"
            );
        }
        // Mid-synodic departures are too expensive.
        let off = Epoch::from_ymd(2042, 11, 1);
        assert!(!months.iter().any(|d| (d.0 - off.0).abs() < 45.0));
        // Aerocapture at Mars beats burning into orbit.
        let cheapest = |aero: bool| {
            edges
                .iter()
                .filter(|e| e.aero == aero)
                .map(|e| e.dv)
                .fold(f64::INFINITY, f64::min)
        };
        assert!(cheapest(true) < cheapest(false) - 1.0);
    }
}
