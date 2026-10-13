//! Which programs to pay for: dynamic slope scaling over the LP, then an
//! exact solve and single drops. See M2.md, "Fixed costs".

use crate::model::{Lp, Plan, ProgramSetup};
use crate::params::Scenario;
use highs::HighsModelStatus;

/// A program whose plants install less equipment than this, t, is not in
/// use.
const IN_USE_T: f64 = 1.0;

/// Installations below this, t per period, do not count as a first
/// installation.
const FIRST_INSTALL_T: f64 = 1e-3;

/// Slope-scaling solves at most.
const MAX_SCALING_SOLVES: usize = 8;

/// Slope scaling has converged when the programs in use repeat and their
/// usage moves by less than this fraction.
const USAGE_TOLERANCE: f64 = 0.2;

pub struct Outcome {
    pub plan: Plan,
    /// The programs as the plan treats them: funded or closed.
    pub setup: Vec<ProgramSetup>,
    pub solves: usize,
    /// Objective of the Earth-only plan, if solved.
    pub earth_only: Option<f64>,
    /// Objective after closing one program of the exact plan (and any
    /// program that only it made useful), by program; `None` if that solve
    /// was not optimal.
    pub drops: Vec<(usize, Option<f64>)>,
}

/// What one plan installs at the plants that need a program.
struct Usage {
    total_t: f64,
    discounted_t: f64,
    first: Option<usize>,
}

impl Usage {
    fn in_use(&self) -> bool {
        self.total_t > IN_USE_T
    }
}

fn usage(sc: &Scenario, plan: &Plan) -> Vec<Usage> {
    let mut u: Vec<Usage> = sc
        .programs
        .iter()
        .map(|_| Usage {
            total_t: 0.0,
            discounted_t: 0.0,
            first: None,
        })
        .collect();
    for (f, installed) in sc.facilities.iter().zip(&plan.installed) {
        let first = installed.iter().position(|&x| x > FIRST_INSTALL_T);
        let total: f64 = installed.iter().sum();
        let discounted: f64 = installed
            .iter()
            .enumerate()
            .map(|(p, x)| x * sc.discount(p))
            .sum();
        for &j in &f.programs {
            u[j].total_t += total;
            u[j].discounted_t += discounted;
            u[j].first = match (u[j].first, first) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
    }
    u
}

/// Every program free and open from the start: the first sweep's model.
pub fn free(lp: &mut Lp, sc: &Scenario) -> Outcome {
    let setup = vec![
        ProgramSetup::Charged {
            ready: 0,
            musd_per_t: 0.0
        };
        sc.programs.len()
    ];
    Outcome {
        plan: lp.solve(sc, &setup),
        setup,
        solves: 1,
        earth_only: None,
        drops: Vec::new(),
    }
}

/// The best plan found when programs must be paid for, using only the
/// programs marked `allowed`.
pub fn optimise(lp: &mut Lp, sc: &Scenario, allowed: &[bool]) -> Outcome {
    let n = sc.programs.len();
    assert_eq!(allowed.len(), n);
    let earliest: Vec<usize> = sc.programs.iter().map(|p| p.lead_periods).collect();
    let mut solves = 0;

    // Slope scaling: each program's cost spread over the equipment its plants
    // installed in the previous solve.
    let mut charge = vec![0.0; n];
    let mut previous: Option<Vec<Usage>> = None;
    let used = loop {
        let setup: Vec<ProgramSetup> = (0..n)
            .map(|j| match allowed[j] {
                true => ProgramSetup::Charged {
                    ready: earliest[j],
                    musd_per_t: charge[j],
                },
                false => ProgramSetup::Closed,
            })
            .collect();
        let plan = lp.solve(sc, &setup);
        solves += 1;
        let used = usage(sc, &plan);
        let converged = previous.as_ref().is_some_and(|prev| {
            prev.iter().zip(&used).all(|(a, b)| {
                a.in_use() == b.in_use()
                    && (!b.in_use()
                        || (a.discounted_t - b.discounted_t).abs()
                            <= USAGE_TOLERANCE * b.discounted_t)
            })
        });
        if converged || solves >= MAX_SCALING_SOLVES {
            break used;
        }
        for (j, u) in used.iter().enumerate() {
            if let (true, Some(first)) = (u.in_use(), u.first) {
                charge[j] = just_in_time_cost(sc, j, first) / u.discounted_t;
            }
        }
        previous = Some(used);
    };

    // Exact: fund what the last solve used, ready at its first installation.
    let funded: Vec<ProgramSetup> = (0..n)
        .map(|j| match (used[j].in_use(), used[j].first) {
            (true, Some(first)) => ProgramSetup::Funded {
                ready: first.max(earliest[j]),
            },
            _ => ProgramSetup::Closed,
        })
        .collect();
    let mut best_setup = prune(sc, funded);
    let mut best = lp.solve(sc, &best_setup);
    solves += 1;

    let earth_only = vec![ProgramSetup::Closed; n];
    let mut earth_only_objective = optimal_objective(&best).filter(|_| best_setup == earth_only);
    if best_setup != earth_only {
        let plan = lp.solve(sc, &earth_only);
        solves += 1;
        earth_only_objective = optimal_objective(&plan);
        if better(&plan, &best) {
            best = plan;
            best_setup = earth_only.clone();
        }
    }

    // Single drops, least used first.
    let mut order: Vec<usize> = (0..n)
        .filter(|&j| best_setup[j] != ProgramSetup::Closed)
        .collect();
    order.sort_by(|&a, &b| used[a].discounted_t.total_cmp(&used[b].discounted_t));
    let mut tried = vec![best_setup.clone(), earth_only];
    let mut drops = Vec::new();
    for j in order {
        if best_setup[j] == ProgramSetup::Closed {
            continue; // an earlier drop made it useless
        }
        let mut trial = best_setup.clone();
        trial[j] = ProgramSetup::Closed;
        let trial = prune(sc, trial);
        if tried.contains(&trial) {
            continue;
        }
        tried.push(trial.clone());
        let plan = lp.solve(sc, &trial);
        solves += 1;
        drops.push((j, optimal_objective(&plan)));
        if better(&plan, &best) {
            best = plan;
            best_setup = trial;
        }
    }

    Outcome {
        plan: best,
        setup: best_setup,
        solves,
        earth_only: earth_only_objective,
        drops,
    }
}

/// Present value of program `j`'s cost, spent at its full rate just before
/// period `ready`.
fn just_in_time_cost(sc: &Scenario, j: usize, ready: usize) -> f64 {
    let prog = &sc.programs[j];
    let start = ready.saturating_sub(prog.lead_periods);
    let rate = prog.cost_musd / prog.lead_periods as f64;
    (start..ready).map(|p| rate * sc.discount(p)).sum()
}

/// Closes every open program none of whose plants can be built.
fn prune(sc: &Scenario, mut setup: Vec<ProgramSetup>) -> Vec<ProgramSetup> {
    loop {
        let buildable = |j: usize, setup: &[ProgramSetup]| {
            sc.facilities.iter().any(|f| {
                f.programs.contains(&j)
                    && f.programs.iter().all(|&i| setup[i] != ProgramSetup::Closed)
            })
        };
        let useless: Vec<usize> = (0..setup.len())
            .filter(|&j| setup[j] != ProgramSetup::Closed && !buildable(j, &setup))
            .collect();
        if useless.is_empty() {
            return setup;
        }
        for j in useless {
            setup[j] = ProgramSetup::Closed;
        }
    }
}

fn optimal_objective(plan: &Plan) -> Option<f64> {
    (plan.status == HighsModelStatus::Optimal).then_some(plan.objective)
}

fn better(a: &Plan, b: &Plan) -> bool {
    a.status == HighsModelStatus::Optimal
        && (b.status != HighsModelStatus::Optimal
            || a.objective > b.objective + 1e-6 * b.objective.abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{richest_lunar_ice, short};

    /// Lunar ice with its programs at `cost` each, everything else closed
    /// by a prohibitive cost; all programs take a year.
    fn lunar_ice_at(cost: f64) -> (Scenario, Outcome) {
        let (net, mut sc, edges) = short(8);
        let ice = richest_lunar_ice(&sc);
        let needs = sc.facilities[ice].programs.clone();
        for (j, prog) in sc.programs.iter_mut().enumerate() {
            prog.lead_periods = 4;
            prog.cost_musd = if needs.contains(&j) { cost } else { 1e9 };
        }
        let mut lp = Lp::new(&sc, &net, &edges, false);
        let out = optimise(&mut lp, &sc, &vec![true; sc.programs.len()]);
        assert_eq!(out.plan.status, HighsModelStatus::Optimal);
        (sc, out)
    }

    #[test]
    fn funds_a_cheap_program_and_skips_a_dear_one() {
        let funded = |sc: &Scenario, out: &Outcome| -> Vec<String> {
            sc.programs
                .iter()
                .zip(&out.setup)
                .filter(|(_, s)| matches!(s, ProgramSetup::Funded { .. }))
                .map(|(p, _)| p.id.clone())
                .collect()
        };
        let (sc, out) = lunar_ice_at(1.0);
        assert_eq!(funded(&sc, &out), ["lunar-ice", "survey:lunar-ice"]);
        let (sc, out) = lunar_ice_at(1e7);
        assert!(funded(&sc, &out).is_empty(), "{:?}", funded(&sc, &out));
    }
}
