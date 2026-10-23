//! One draw of the economy (`sim::params::Economy`) on the LP's calendar:
//! the launch price by period, build and lead times in periods, surveys at
//! the expected cost of finding a deposit, and demand curves in price
//! steps. A [`Scenario`] is what the LP solves.

use crate::calendar::{Calendar, DAYS_PER_YEAR};
use orbit::routes::PlaceId;
use sim::params::{
    AsteroidDraw, Demanded, Economy, Facility, Output, ProgramKind, Propellant, PropellantSpec,
    ShipClass,
};

/// A kind of plant at one site.
#[derive(Clone, Debug)]
pub struct FacilityOption {
    pub id: String,
    pub node: PlaceId,
    /// Per t/yr of rated output.
    pub output: Output,
    /// Equipment, t per t/yr of output.
    pub alpha: f64,
    pub grade: f64,
    pub build_periods: usize,
    /// Cumulative output the deposit can yield, t.
    pub reserve: Option<f64>,
    /// Indices into [`Scenario::programs`] that must be paid before the
    /// plant is installed.
    pub programs: Vec<usize>,
}

/// A fixed cost that must be paid before some plants are installed.
#[derive(Clone, Debug)]
pub struct Program {
    pub id: String,
    pub kind: ProgramKind,
    /// $M. A survey's is its mission cost divided by the prior probability
    /// that the deposit has the resource: the expected cost of finding one.
    pub cost_musd: f64,
    /// Spending is at most `cost_musd / lead_periods` per period.
    pub lead_periods: usize,
}

#[derive(Clone, Debug)]
pub struct DemandSegment {
    pub usd_per_kg: f64,
    /// t/yr at the start; grows with the category.
    pub tpy: f64,
}

#[derive(Clone, Debug)]
pub struct DemandCategory {
    pub id: String,
    pub what: Demanded,
    pub nodes: Vec<PlaceId>,
    pub segments: Vec<DemandSegment>,
    pub growth: f64,
}

#[derive(Clone, Debug)]
pub struct Scenario {
    pub cal: Calendar,
    pub discount_rate: f64,
    /// By [`Propellant`].
    pub propellants: [PropellantSpec; 2],
    /// $/kg to LEO, per period.
    pub launch_usd_per_kg: Vec<f64>,
    pub equipment_usd_per_kg: f64,
    pub propellant_usd_per_kg: f64,
    pub budget_musd_per_year: f64,
    /// `(tonnes, usd_per_kg)` by cumulative sales.
    pub metal_steps: Vec<(f64, f64)>,
    pub surfaces: Vec<PlaceId>,
    pub ships: Vec<ShipClass>,
    pub facilities: Vec<FacilityOption>,
    pub programs: Vec<Program>,
    pub demand: Vec<DemandCategory>,
    pub asteroids: Vec<AsteroidDraw>,
}

impl Scenario {
    /// `economy` on a calendar of `period_months` steps.
    pub fn new(economy: &Economy, period_months: u32) -> Scenario {
        let e = economy;
        let cal =
            Calendar::with_period(e.start, e.end, period_months as f64 * DAYS_PER_YEAR / 12.0);
        let [l0, l1] = e.launch_usd_per_kg;
        let launch_usd_per_kg = (0..cal.periods)
            .map(|p| l0 * (l1 / l0).powf(p as f64 / (cal.periods - 1) as f64))
            .collect();
        let build_periods = |months: f64| (months / 12.0 / cal.period_years()).round() as usize;
        let lead_periods =
            |years: f64| (years / cal.period_years() - 1e-9).ceil().max(1.0) as usize;
        // The LP knows which sites have the resource (M2's foresight): the
        // options the draw left out, and their surveys, are not its.
        let present: Vec<&Facility> = e.facilities.iter().filter(|f| f.present).collect();
        let kept: Vec<bool> = (0..e.programs.len())
            .map(|j| {
                e.programs[j].kind == ProgramKind::Technology
                    || present.iter().any(|f| f.programs.contains(&j))
            })
            .collect();
        let index: Vec<usize> = kept
            .iter()
            .scan(0, |n, &k| {
                let i = *n;
                *n += usize::from(k);
                Some(i)
            })
            .collect();
        let facilities = present
            .iter()
            .map(|f| FacilityOption {
                id: f.id.clone(),
                node: f.node,
                output: f.output,
                alpha: f.alpha(),
                grade: f.grade,
                build_periods: build_periods(f.build_months),
                reserve: f.reserve,
                programs: f.programs.iter().map(|&j| index[j]).collect(),
            })
            .collect();
        let programs = e
            .programs
            .iter()
            .zip(&kept)
            .filter(|(_, k)| **k)
            .map(|(p, _)| Program {
                id: p.id.clone(),
                kind: p.kind,
                // M2's foresight offset: the LP knows which sites have the
                // resource, so it pays for the surveys that find none too.
                cost_musd: p.cost_musd / p.prior,
                lead_periods: lead_periods(p.years),
            })
            .collect();
        let demand = e
            .demand
            .iter()
            .map(|d| {
                let q = |p: f64| d.ref_tpy * (p / d.ref_usd_per_kg).powf(-d.elasticity);
                let mut segments = Vec::new();
                let mut hi = d.max_usd_per_kg;
                while hi > d.min_usd_per_kg * 1.000_001 {
                    let lo = hi / 2.0;
                    segments.push(DemandSegment {
                        usd_per_kg: (hi * lo).sqrt(),
                        tpy: (q(lo) - q(hi)) * d.share,
                    });
                    hi = lo;
                }
                DemandCategory {
                    id: d.id.clone(),
                    what: d.what,
                    nodes: d.nodes.clone(),
                    segments,
                    growth: d.growth,
                }
            })
            .collect();
        Scenario {
            cal,
            discount_rate: e.discount_rate,
            propellants: e.propellants,
            launch_usd_per_kg,
            equipment_usd_per_kg: e.equipment_usd_per_kg,
            propellant_usd_per_kg: e.propellant_usd_per_kg,
            budget_musd_per_year: e.budget_musd_per_year,
            metal_steps: e.metal_steps.clone(),
            surfaces: e.surfaces.clone(),
            // A class without tanks (a station module, M1b.md step 4) flies
            // nothing: not a fleet of the LP.
            ships: e.ships.iter().filter(|s| s.tank > 0.0).cloned().collect(),
            facilities,
            programs,
            demand,
            asteroids: e.asteroids.clone(),
        }
    }

    pub fn propellant(&self, p: Propellant) -> &PropellantSpec {
        &self.propellants[p as usize]
    }

    /// Discount factor of period `p`, at its middle.
    pub fn discount(&self, p: usize) -> f64 {
        (1.0 + self.discount_rate).powf(-self.cal.years_at(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit::ephem::Ephemeris;
    use orbit::routes::RouteGraph;
    use sim::params::{Draw, EconomyFile};

    /// The options the draw left out, and their surveys, are not the LP's;
    /// a kept option's programs are the same ones by id.
    #[test]
    fn absent_options_and_their_surveys_are_left_out() {
        let eph = Ephemeris::standard();
        let e = EconomyFile::standard().draw(eph, RouteGraph::standard(), &mut Draw::Central);
        let sc = Scenario::new(&e, 3);
        let present: Vec<&Facility> = e.facilities.iter().filter(|f| f.present).collect();
        assert!(
            present.len() < e.facilities.len(),
            "the central world has absent options"
        );
        assert_eq!(sc.facilities.len(), present.len());
        for (j, p) in sc.programs.iter().enumerate() {
            if p.kind == ProgramKind::Survey {
                assert!(
                    sc.facilities.iter().any(|f| f.programs.contains(&j)),
                    "{}",
                    p.id
                );
            }
        }
        for (f, ef) in sc.facilities.iter().zip(&present) {
            let ids: Vec<&str> = f
                .programs
                .iter()
                .map(|&j| sc.programs[j].id.as_str())
                .collect();
            let eids: Vec<&str> = ef
                .programs
                .iter()
                .map(|&j| e.programs[j].id.as_str())
                .collect();
            assert_eq!(ids, eids, "{}", f.id);
        }
    }
}
