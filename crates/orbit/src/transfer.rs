//! Interplanetary transfers between network nodes, and porkchop grids of them.

use crate::consts::MU_SUN;
use crate::ephem::{BodyId, Ephemeris};
use crate::lambert;
use crate::network::{Maneuver, Network, NodeId};
use crate::time::{Epoch, SECONDS_PER_DAY};
use rayon::prelude::*;
use std::f64::consts::PI;

/// The heliocentric part of a transfer: excess speeds at each end.
#[derive(Clone, Copy, Debug)]
pub struct HelioTransfer {
    pub v_inf_depart: f64,
    pub v_inf_arrive: f64,
    /// Prograde transfer angle, radians; beyond π is Type II.
    pub angle: f64,
}

/// The zero-revolution prograde Lambert transfer between two heliocentric
/// bodies.
pub fn helio_transfer(
    eph: &Ephemeris,
    from: BodyId,
    to: BodyId,
    depart: Epoch,
    tof_days: f64,
) -> Option<HelioTransfer> {
    let s1 = eph.state(from, depart);
    let s2 = eph.state(to, depart.plus_days(tof_days));
    let sol = lambert::solve(MU_SUN, s1.r, s2.r, tof_days * SECONDS_PER_DAY)?;
    Some(HelioTransfer {
        v_inf_depart: (sol.v1 - s1.v).length(),
        v_inf_arrive: (sol.v2 - s2.v).length(),
        angle: lambert::prograde_angle(s1.r, s2.r),
    })
}

/// A node-to-node interplanetary transfer: a heliocentric leg starting at
/// `depart` and lasting `tof_days`, plus the gateway maneuvers at each end,
/// which may add time before and after the leg.
#[derive(Clone, Copy, Debug)]
pub struct Transfer {
    pub depart: Epoch,
    pub tof_days: f64,
    pub departure: Maneuver,
    pub arrival: Maneuver,
    pub helio: HelioTransfer,
}

impl Transfer {
    pub fn dv(&self) -> f64 {
        self.departure.dv + self.arrival.dv
    }

    /// When the ship leaves the origin node.
    pub fn leave(&self) -> Epoch {
        self.depart.plus_days(-self.departure.days)
    }

    /// When the ship reaches the destination node.
    pub fn arrive(&self) -> Epoch {
        self.depart.plus_days(self.tof_days + self.arrival.days)
    }

    /// Node to node.
    pub fn trip_days(&self) -> f64 {
        self.departure.days + self.tof_days + self.arrival.days
    }

    /// Departure energy, km²/s².
    pub fn c3(&self) -> f64 {
        self.helio.v_inf_depart.powi(2)
    }

    pub fn is_type_ii(&self) -> bool {
        self.helio.angle > PI
    }
}

/// What a transfer connects and what the ship can do.
#[derive(Clone, Copy, Debug)]
pub struct Route {
    pub from: NodeId,
    pub to: NodeId,
    /// The ship can brake in an atmosphere (aerocapture, direct entry).
    pub aero_capable: bool,
}

impl Route {
    /// Checks that the route is an interplanetary one between gateways.
    pub fn validate(&self, net: &Network) -> Result<(), String> {
        let (a, b) = (net.node(self.from), net.node(self.to));
        if a.system == b.system {
            return Err(format!(
                "{} and {} are in the same system; use fixed legs",
                a.id, b.id
            ));
        }
        let probe = 3.0;
        let dep = a
            .gateway
            .ok_or_else(|| format!("{} has no interplanetary gateway", a.id))?;
        dep.departure(probe)
            .ok_or_else(|| format!("{} cannot be departed from directly", a.id))?;
        let arr = b
            .gateway
            .ok_or_else(|| format!("{} has no interplanetary gateway", b.id))?;
        arr.arrival(probe, self.aero_capable).ok_or_else(|| {
            format!(
                "{} cannot be reached directly without an aero-capable ship",
                b.id
            )
        })?;
        Ok(())
    }

    /// The transfer departing at `depart` and taking `tof_days`, if the
    /// Lambert problem has a solution. The route must be valid.
    pub fn transfer(
        &self,
        eph: &Ephemeris,
        net: &Network,
        depart: Epoch,
        tof_days: f64,
    ) -> Option<Transfer> {
        let (a, b) = (net.node(self.from), net.node(self.to));
        let helio = helio_transfer(eph, a.system, b.system, depart, tof_days)?;
        Some(Transfer {
            depart,
            tof_days,
            departure: a.gateway?.departure(helio.v_inf_depart)?,
            arrival: b.gateway?.arrival(helio.v_inf_arrive, self.aero_capable)?,
            helio,
        })
    }

    /// A time-of-flight range bracketing the useful transfers: 0.3–3× the
    /// Hohmann time between the two systems' mean orbits at `t`. Long Type II
    /// transfers to eccentric small bodies need the upper end.
    pub fn default_tof_range(&self, eph: &Ephemeris, net: &Network, t: Epoch) -> (f64, f64) {
        let a = |n: NodeId| eph.body(net.node(n).system).elements(t).a;
        let semi = (a(self.from) + a(self.to)) / 2.0;
        let hohmann_days = PI * (semi.powi(3) / MU_SUN).sqrt() / SECONDS_PER_DAY;
        (0.3 * hohmann_days, 3.0 * hohmann_days)
    }
}

/// Departure dates × times of flight.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub depart_start: Epoch,
    pub depart_step: f64,
    pub departs: usize,
    pub tof_min: f64,
    pub tof_step: f64,
    pub tofs: usize,
}

impl Grid {
    /// A grid covering `[start, end]` departures and `[tof_min, tof_max]`.
    pub fn covering(
        start: Epoch,
        end: Epoch,
        depart_step: f64,
        tof_min: f64,
        tof_max: f64,
        tof_step: f64,
    ) -> Self {
        Grid {
            depart_start: start,
            depart_step,
            departs: ((end.0 - start.0) / depart_step).floor() as usize + 1,
            tof_min,
            tof_step,
            tofs: ((tof_max - tof_min) / tof_step).floor() as usize + 1,
        }
    }

    pub fn depart(&self, i: usize) -> Epoch {
        self.depart_start.plus_days(i as f64 * self.depart_step)
    }

    pub fn tof(&self, j: usize) -> f64 {
        self.tof_min + j as f64 * self.tof_step
    }
}

/// Transfers over a grid, row-major by departure.
pub struct Porkchop {
    pub grid: Grid,
    cells: Vec<Option<Transfer>>,
}

impl Porkchop {
    pub fn compute(
        eph: &Ephemeris,
        net: &Network,
        route: Route,
        grid: Grid,
    ) -> Result<Self, String> {
        route.validate(net)?;
        let cells = (0..grid.departs)
            .into_par_iter()
            .flat_map_iter(|i| {
                (0..grid.tofs).map(move |j| route.transfer(eph, net, grid.depart(i), grid.tof(j)))
            })
            .collect();
        Ok(Porkchop { grid, cells })
    }

    pub fn get(&self, i: usize, j: usize) -> Option<&Transfer> {
        self.cells[i * self.grid.tofs + j].as_ref()
    }

    /// For each departure date, the lowest-Δv transfer over all times of
    /// flight.
    pub fn best_by_departure(&self) -> Vec<Option<Transfer>> {
        self.cells
            .chunks(self.grid.tofs)
            .map(|row| {
                row.iter()
                    .flatten()
                    .copied()
                    .min_by(|a, b| a.dv().total_cmp(&b.dv()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earth_mars_transfer_is_plausible() {
        let eph = Ephemeris::standard();
        let net = Network::standard(&eph);
        let route = Route {
            from: net.lookup("leo").unwrap(),
            to: net.lookup("lmo").unwrap(),
            aero_capable: false,
        };
        route.validate(&net).unwrap();
        // Near the handbook's 2033 Type II minimum (C3 7.78 on 2033-04-28,
        // arriving 2034-01-27).
        let t = route
            .transfer(&eph, &net, Epoch::from_ymd(2033, 4, 28), 274.0)
            .unwrap();
        assert!(t.is_type_ii());
        assert!((7.0..8.6).contains(&t.c3()), "{}", t.c3());
        assert!((3.4..3.8).contains(&t.departure.dv), "{:?}", t.departure);

        let (lo, hi) = route.default_tof_range(&eph, &net, Epoch::from_ymd(2040, 1, 1));
        assert!(lo < 100.0 && hi > 450.0, "{lo} {hi}");
    }

    #[test]
    fn routes_are_validated() {
        let eph = Ephemeris::standard();
        let net = Network::standard(&eph);
        let id = |s: &str| net.lookup(s).unwrap();
        let route = |from, to, aero_capable| Route {
            from: id(from),
            to: id(to),
            aero_capable,
        };
        assert!(route("leo", "eml1", false).validate(&net).is_err());
        assert!(route("llo", "lmo", false).validate(&net).is_err());
        assert!(route("leo", "mars-surface", false).validate(&net).is_err());
        assert!(route("leo", "mars-surface", true).validate(&net).is_ok());
        assert!(route("mars-surface", "leo", true).validate(&net).is_err());
        assert!(route("eml1", "ryugu", false).validate(&net).is_ok());
    }
}
