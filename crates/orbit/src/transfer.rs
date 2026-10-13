//! Heliocentric transfers between bodies (zero-revolution Lambert arcs) and
//! porkchop grids of them: the autopilot's starting guesses for
//! interplanetary legs (M1a.md, playbook entry 1).

use crate::consts::MU_SUN;
use crate::ephem::{BodyId, Ephemeris};
use crate::lambert;
use crate::time::{Epoch, SECONDS_PER_DAY};
use rayon::prelude::*;
use std::f64::consts::PI;

/// A zero-revolution prograde Lambert arc from one body to another.
#[derive(Clone, Copy, Debug)]
pub struct Transfer {
    pub depart: Epoch,
    pub tof_days: f64,
    /// Hyperbolic excess speeds relative to each body, km/s.
    pub v_inf_depart: f64,
    pub v_inf_arrive: f64,
    /// Prograde transfer angle, radians; beyond π is Type II.
    pub angle: f64,
}

impl Transfer {
    /// The arc leaving `from` at `depart` and reaching `to` after
    /// `tof_days`, if the Lambert problem has a solution.
    pub fn compute(
        eph: &Ephemeris,
        from: BodyId,
        to: BodyId,
        depart: Epoch,
        tof_days: f64,
    ) -> Option<Self> {
        let s1 = eph.state(from, depart);
        let s2 = eph.state(to, depart.plus_days(tof_days));
        let sol = lambert::solve(MU_SUN, s1.r, s2.r, tof_days * SECONDS_PER_DAY)?;
        Some(Transfer {
            depart,
            tof_days,
            v_inf_depart: (sol.v1 - s1.v).length(),
            v_inf_arrive: (sol.v2 - s2.v).length(),
            angle: lambert::prograde_angle(s1.r, s2.r),
        })
    }

    pub fn arrive(&self) -> Epoch {
        self.depart.plus_days(self.tof_days)
    }

    /// Departure energy, km²/s².
    pub fn c3(&self) -> f64 {
        self.v_inf_depart.powi(2)
    }

    /// Excess speed at both ends, km/s: what porkchops and windows rank by.
    /// It is the arc's Δv from rest relative to one body to rest relative
    /// to the other; a burn deep in a gravity well (the Oberth effect) or
    /// braking in an atmosphere costs less.
    pub fn v_inf_total(&self) -> f64 {
        self.v_inf_depart + self.v_inf_arrive
    }

    pub fn is_type_ii(&self) -> bool {
        self.angle > PI
    }
}

/// A time-of-flight range bracketing the useful transfers between two
/// bodies: 0.3–3× the Hohmann time between their mean orbits at `t`. Long
/// Type II transfers to eccentric small bodies need the upper end.
pub fn default_tof_range(eph: &Ephemeris, from: BodyId, to: BodyId, t: Epoch) -> (f64, f64) {
    let a = |b: BodyId| eph.body(b).elements(t).a;
    let semi = (a(from) + a(to)) / 2.0;
    let hohmann_days = PI * (semi.powi(3) / MU_SUN).sqrt() / SECONDS_PER_DAY;
    (0.3 * hohmann_days, 3.0 * hohmann_days)
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
    pub fn compute(eph: &Ephemeris, from: BodyId, to: BodyId, grid: Grid) -> Self {
        assert_ne!(from, to, "a porkchop needs two bodies");
        let cells = (0..grid.departs)
            .into_par_iter()
            .flat_map_iter(|i| {
                (0..grid.tofs)
                    .map(move |j| Transfer::compute(eph, from, to, grid.depart(i), grid.tof(j)))
            })
            .collect();
        Porkchop { grid, cells }
    }

    pub fn get(&self, i: usize, j: usize) -> Option<&Transfer> {
        self.cells[i * self.grid.tofs + j].as_ref()
    }

    /// For each departure date, the transfer with the least total excess
    /// speed over all times of flight.
    pub fn best_by_departure(&self) -> Vec<Option<Transfer>> {
        self.cells
            .chunks(self.grid.tofs)
            .map(|row| {
                row.iter()
                    .flatten()
                    .copied()
                    .min_by(|a, b| a.v_inf_total().total_cmp(&b.v_inf_total()))
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
        let (earth, mars) = (eph.lookup("earth").unwrap(), eph.lookup("mars").unwrap());
        // Near the handbook's 2033 Type II minimum (C3 7.78 on 2033-04-28,
        // arriving 2034-01-27).
        let t = Transfer::compute(&eph, earth, mars, Epoch::from_ymd(2033, 4, 28), 274.0).unwrap();
        assert!(t.is_type_ii());
        assert!((7.0..8.6).contains(&t.c3()), "{}", t.c3());

        let (lo, hi) = default_tof_range(&eph, earth, mars, Epoch::from_ymd(2040, 1, 1));
        assert!(lo < 100.0 && hi > 450.0, "{lo} {hi}");
    }
}
