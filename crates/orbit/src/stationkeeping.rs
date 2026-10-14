//! Station-keeping: holding a ship near a reference orbit with imperfect
//! navigation and burns, and what that costs per year, which is an orbit's
//! upkeep (M1a.md, Flight rules).
//!
//! At each maneuver time the ship estimates its state (with navigation
//! error), predicts its deviation from the reference a horizon ahead through
//! the reference's state transition matrix, and if that exceeds a deadband,
//! burns to cancel the predicted position miss (with execution error).
//!
//! Which interval and horizon work depends on the orbit (an NRHO must not
//! burn at perilune; a very unstable halo cannot aim two revolutions
//! ahead), so [`upkeep`] tries several and keeps the cheapest that holds
//! the ship near the reference, as mission designers tune it per orbit.

use crate::gravity::Field;
use crate::integrate::{Dop853, Tolerance};
use crate::linalg;
use crate::reference::Reference;
use crate::time::SECONDS_PER_DAY;

const TOL: Tolerance = Tolerance {
    rtol: 1e-12,
    atol: 1e-9,
};

/// Seconds in a Julian year.
const YEAR_S: f64 = 365.25 * SECONDS_PER_DAY;

/// Navigation and burn errors, 1σ per axis.
#[derive(Clone, Copy, Debug)]
pub struct Errors {
    /// State knowledge at each maneuver, km and km/s.
    pub position_km: f64,
    pub velocity_kms: f64,
    /// Burn execution error as a fraction of the burn.
    pub execution: f64,
}

impl Errors {
    /// Orbit determination of 1 km and 1 cm/s (3σ), as in Guzzetti et al.,
    /// "Stationkeeping analysis for spacecraft in lunar near rectilinear halo
    /// orbits" (AAS 17-395, 2017); 1% execution error is this game's
    /// assumption.
    pub const TYPICAL: Errors = Errors {
        position_km: 1.0 / 3.0,
        velocity_kms: 0.01e-3 / 3.0,
        execution: 0.01,
    };
}

/// When and how the ship corrects its course.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    /// Time between maneuver opportunities, s.
    pub interval_s: f64,
    /// How far ahead each maneuver aims at the reference, s.
    pub horizon_s: f64,
    /// Predicted deviations smaller than this need no burn, km.
    pub deadband_km: f64,
}

/// What holding the orbit cost over one simulated run.
#[derive(Clone, Copy, Debug)]
pub struct Run {
    pub dv_per_year_kms: f64,
    pub maneuvers: usize,
    /// The ship's largest distance from the reference at a maneuver time.
    pub worst_deviation_km: f64,
}

/// Deterministic random numbers: splitmix64, and Gaussian samples as the
/// sum of twelve uniforms (integer and add arithmetic only, so runs are
/// bit-identical across machines).
struct Rng(u64);

impl Rng {
    fn uniform(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        (z >> 11) as f64 / (1u64 << 53) as f64
    }

    fn gaussian(&mut self) -> f64 {
        (0..12).map(|_| self.uniform()).sum::<f64>() - 6.0
    }
}

/// One run: a ship inserted on the reference (with navigation error) held
/// there until the last maneuver whose horizon the reference still covers,
/// or until it strays farther than [`HELD_KM`].
pub fn simulate(
    field: &Field,
    reference: &Reference,
    policy: Policy,
    errors: Errors,
    seed: u64,
) -> Option<Run> {
    let mut rng = Rng(seed);
    let (ode, stm) = (field.ode(reference.epoch), field.ode_stm(reference.epoch));
    let span = reference.path.span().1;
    let noisy = |state: &[f64; 6], rng: &mut Rng| -> [f64; 6] {
        std::array::from_fn(|i| {
            state[i]
                + rng.gaussian()
                    * if i < 3 {
                        errors.position_km
                    } else {
                        errors.velocity_kms
                    }
        })
    };
    let start = noisy(&reference.path.at(0.0), &mut rng);
    let mut ship = Dop853::new(&ode, 0.0, start, TOL);
    let (mut dv, mut maneuvers, mut worst) = (0.0, 0, 0.0f64);
    let mut t = policy.interval_s;
    while t + policy.horizon_s <= span {
        ship.advance_to(&ode, t).ok()?;
        let reference_now = reference.path.at(t);
        let truth = ship.y;
        worst = worst.max(
            (0..3)
                .map(|i| (truth[i] - reference_now[i]).powi(2))
                .sum::<f64>()
                .sqrt(),
        );
        // Lost: the policy fails, and a ship left to wander can fall
        // through the Moon's point mass, where integrating it crawls (a
        // year of NRHO took over 10 minutes).
        if worst > HELD_KM {
            break;
        }
        let estimate = noisy(&truth, &mut rng);
        let delta: [f64; 6] = std::array::from_fn(|i| estimate[i] - reference_now[i]);
        // The reference's sensitivity over the horizon.
        let mut y = [0.0; 42];
        y[..6].copy_from_slice(&reference_now);
        for d in 0..6 {
            y[6 + d * 7] = 1.0;
        }
        let mut arc = Dop853::new(&stm, t, y, TOL);
        arc.advance_to(&stm, t + policy.horizon_s).ok()?;
        let phi = |r: usize, c: usize| arc.y[6 + r * 6 + c];
        let miss: [f64; 3] = std::array::from_fn(|r| (0..6).map(|c| phi(r, c) * delta[c]).sum());
        if miss.iter().map(|x| x * x).sum::<f64>().sqrt() > policy.deadband_km {
            // Δv with Φ_rv Δv = -miss.
            let mut a: Vec<f64> = (0..3)
                .flat_map(|r| (3..6).map(move |c| (r, c)))
                .map(|(r, c)| phi(r, c))
                .collect();
            let mut burn: Vec<f64> = miss.iter().map(|m| -m).collect();
            linalg::solve(3, &mut a, &mut burn)?;
            let size = burn.iter().map(|x| x * x).sum::<f64>().sqrt();
            let mut state = ship.y;
            for i in 0..3 {
                state[3 + i] += burn[i] + size * errors.execution * rng.gaussian();
            }
            ship = Dop853::new(&ode, t, state, TOL);
            dv += size;
            maneuvers += 1;
        }
        t += policy.interval_s;
    }
    let simulated = t - policy.interval_s;
    Some(Run {
        dv_per_year_kms: dv * YEAR_S / simulated,
        maneuvers,
        worst_deviation_km: worst,
    })
}

/// A policy's average over Monte Carlo runs.
#[derive(Clone, Copy, Debug)]
pub struct Upkeep {
    pub policy: Policy,
    pub dv_per_year_kms: f64,
    pub maneuvers_per_year: f64,
    pub worst_deviation_km: f64,
}

/// A policy holds the orbit if no run strays farther than this, km.
const HELD_KM: f64 = 500.0;

/// Deadband for the candidate policies, km.
const DEADBAND_KM: f64 = 10.0;

/// The cheapest of a few station-keeping policies (maneuvers every half or
/// whole revolution, aiming half, one or two revolutions ahead) that holds
/// the ship within 500 km of the reference in every one of `runs` runs.
/// `None` if none does.
pub fn upkeep(field: &Field, reference: &Reference, errors: Errors, runs: u64) -> Option<Upkeep> {
    let period = reference.period_s();
    let mut best: Option<Upkeep> = None;
    for interval in [0.5, 1.0] {
        for horizon in [0.5, 1.0, 2.0] {
            let policy = Policy {
                interval_s: interval * period,
                horizon_s: horizon * period,
                deadband_km: DEADBAND_KM,
            };
            let results: Option<Vec<Run>> = (0..runs)
                .map(|seed| simulate(field, reference, policy, errors, seed))
                .collect();
            let Some(results) = results else { continue };
            let worst = results
                .iter()
                .map(|r| r.worst_deviation_km)
                .fold(0.0, f64::max);
            if worst.is_nan() || worst > HELD_KM {
                continue;
            }
            let n = results.len() as f64;
            let dv = results.iter().map(|r| r.dv_per_year_kms).sum::<f64>() / n;
            let simulated_years = (reference.path.span().1 - horizon * period) / YEAR_S;
            let maneuvers =
                results.iter().map(|r| r.maneuvers).sum::<usize>() as f64 / n / simulated_years;
            if best.is_none_or(|b| dv < b.dv_per_year_kms) {
                best = Some(Upkeep {
                    policy,
                    dv_per_year_kms: dv,
                    maneuvers_per_year: maneuvers,
                    worst_deviation_km: worst,
                });
            }
        }
    }
    best
}
