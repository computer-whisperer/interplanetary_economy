//! The time-expanded transport network: every way to move between two nodes,
//! by the period a ship leaves and the period it arrives.

use crate::calendar::Calendar;
use orbit::ephem::Ephemeris;
use orbit::network::{Network, NodeId};
use orbit::transfer::{Grid, Porkchop, Route, Transfer};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdgeKind {
    /// A fixed leg within one planetary system. It takes a few days, so it
    /// leaves and arrives in the same period.
    Local { aero: bool },
    /// The cheapest transfer leaving in its period (aero-capable ship).
    Interplanetary,
}

#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub leave: usize,
    pub arrive: usize,
    /// Total Δv, all of it loaded at `from`.
    pub dv: f64,
    /// Node to node.
    pub days: f64,
    pub kind: EdgeKind,
}

/// Porkchop resolution for interplanetary edges, days.
const DEPART_STEP: f64 = 2.0;
const TOF_STEP: f64 = 4.0;

/// Every fixed leg in every period, except launch services (Earth launches
/// are purchases, not flights).
pub fn local_edges(net: &Network, cal: &Calendar) -> Vec<Edge> {
    let legs = net.legs().iter().filter(|l| !l.launch);
    legs.flat_map(|l| {
        (0..cal.periods).map(move |p| Edge {
            from: l.from,
            to: l.to,
            leave: p,
            arrive: p,
            dv: l.dv,
            days: l.days,
            kind: EdgeKind::Local { aero: l.aero },
        })
    })
    .collect()
}

/// The interplanetary routes the optimizer considers: every gateway pair
/// with one end in Earth's system. (Mars–asteroid and asteroid–asteroid
/// routes are left out for now.)
pub fn routes(eph: &Ephemeris, net: &Network) -> Vec<Route> {
    let earth = eph.lookup("earth").expect("Earth is in the ephemeris");
    let mut out = Vec::new();
    for (a, na) in net.nodes() {
        for (b, nb) in net.nodes() {
            if na.system == nb.system || (na.system != earth && nb.system != earth) {
                continue;
            }
            let route = Route {
                from: a,
                to: b,
                aero_capable: true,
            };
            if route.validate(net).is_ok() {
                out.push(route);
            }
        }
    }
    out
}

/// For each route and each period, the lowest-Δv transfer that leaves the
/// origin node in that period and arrives before the calendar ends, if it
/// costs at most `max_dv`.
pub fn interplanetary_edges(
    eph: &Ephemeris,
    net: &Network,
    cal: &Calendar,
    routes: &[Route],
    max_dv: f64,
) -> Vec<Edge> {
    let mut edges = Vec::new();
    for &route in routes {
        let (lo, hi) = route.default_tof_range(eph, net, cal.start);
        // A departure maneuver can start before the heliocentric leg (lunar
        // flybys), so search heliocentric departures past the end.
        let lead = net
            .node(route.from)
            .gateway
            .and_then(|g| g.departure(0.0))
            .map_or(0.0, |m| m.days);
        let grid = Grid::covering(
            cal.start,
            cal.end().plus_days(lead),
            DEPART_STEP,
            lo,
            hi,
            TOF_STEP,
        );
        let pc = Porkchop::compute(eph, net, route, grid).expect("routes are valid");
        let mut best: Vec<Option<Transfer>> = vec![None; cal.periods];
        for t in pc.best_by_departure().into_iter().flatten() {
            let (Some(p), Some(_)) = (cal.period_of(t.leave()), cal.period_of(t.arrive())) else {
                continue;
            };
            if t.dv() <= max_dv && best[p].is_none_or(|b| t.dv() < b.dv()) {
                best[p] = Some(t);
            }
        }
        let route_edges: Vec<Edge> = best
            .iter()
            .flatten()
            .map(|t| Edge {
                from: route.from,
                to: route.to,
                leave: cal.period_of(t.leave()).unwrap(),
                arrive: cal.period_of(t.arrive()).unwrap(),
                dv: t.dv(),
                days: t.trip_days(),
                kind: EdgeKind::Interplanetary,
            })
            .collect();
        edges.extend(undominated(route_edges));
    }
    edges
}

/// Drops each edge for which another on the same route leaves no earlier,
/// arrives no later and costs no more: waiting for it loses nothing.
/// `edges` are sorted by departure.
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
        let net = Network::standard(&eph);
        let cal = Calendar::monthly(Epoch::from_ymd(2040, 1, 1), Epoch::from_ymd(2047, 1, 1));
        let route = Route {
            from: net.lookup("leo").unwrap(),
            to: net.lookup("lmo").unwrap(),
            aero_capable: true,
        };
        let edges = interplanetary_edges(&eph, &net, &cal, &[route], 4.5);
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
    }
}
