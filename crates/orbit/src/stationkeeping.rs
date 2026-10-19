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
use crate::track::Track;

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

/// One burn: when, in seconds after the reference's epoch, and how much.
#[derive(Clone, Copy, Debug)]
pub struct Burn {
    pub t: f64,
    pub dv_kms: f64,
}

/// What holding the orbit cost over one simulated run, and how the ship
/// flew.
#[derive(Clone, Debug)]
pub struct Run {
    pub dv_per_year_kms: f64,
    pub burns: Vec<Burn>,
    /// The ship's largest distance from the reference at a maneuver time.
    pub worst_deviation_km: f64,
    /// The ship's states (s after the reference's epoch), in time order:
    /// where it started, every [`KEPT_DAYS`](crate::track::KEPT_DAYS) as
    /// [`Track::kept_times`] spaces them, after each maneuver (the burn
    /// made), and where it ended, so that a coast through them is the
    /// flight ([`Track::coast`]).
    pub kept: Vec<(f64, [f64; 6])>,
}

impl Run {
    /// Whether the ship strayed farther than a policy may let it
    /// ([`HELD_KM`]), which ends a run early.
    pub fn lost(&self) -> bool {
        self.worst_deviation_km > HELD_KM
    }
}

/// Deterministic random numbers: splitmix64, and Gaussian samples as the
/// sum of twelve uniforms (integer and add arithmetic only, so runs are
/// bit-identical across machines).
#[derive(Clone, Copy, Debug)]
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

/// `state` with navigation error.
fn noisy(state: &[f64; 6], errors: Errors, rng: &mut Rng) -> [f64; 6] {
    std::array::from_fn(|i| {
        state[i]
            + rng.gaussian()
                * if i < 3 {
                    errors.position_km
                } else {
                    errors.velocity_kms
                }
    })
}

/// The station-keeping loop in flight on a reference: where the ship is,
/// when it next maneuvers, its random stream, and what it has done so far.
/// It flies on in pieces ([`Loop::fly_to`]); the integrator restarts where
/// a piece ends, so a flight is the same to the bit when flown in the same
/// pieces.
#[derive(Clone, Debug)]
pub struct Loop {
    policy: Policy,
    errors: Errors,
    rng: Rng,
    /// Where the ship is, s after the reference's epoch, and its state
    /// there (after any burn made there).
    pub t: f64,
    pub state: [f64; 6],
    /// Its next maneuver time.
    maneuver: f64,
    pub burns: Vec<Burn>,
    /// The ship's states, in time order: where it started, every
    /// [`KEPT_DAYS`](crate::track::KEPT_DAYS) as [`Track::kept_times`]
    /// spaces them, after each maneuver (the burn made), and where each
    /// piece ended, so that a coast through them is the flight
    /// ([`Track::coast`]).
    pub kept: Vec<(f64, [f64; 6])>,
    /// The ship's largest distance from the reference at a maneuver time.
    pub worst_deviation_km: f64,
}

impl Loop {
    /// The ship inserted on the reference at `t0` (s after its epoch) with
    /// the loop's navigation error. Inserted on the reference to the bit,
    /// it would never leave it: the truth and the reference are one
    /// integration, navigation noise alone predicts a miss under the
    /// deadband on an NRHO, and without a burn there is no execution error
    /// (a 43-day stay made none, 2026-10-18). The dispersion stands for the
    /// arriving burn's error, which legs do not model.
    pub fn insert(
        reference: &Reference,
        t0: f64,
        policy: Policy,
        errors: Errors,
        seed: u64,
    ) -> Loop {
        let mut rng = Rng(seed);
        let start = noisy(&reference.path.at(t0), errors, &mut rng);
        Loop {
            policy,
            errors,
            rng,
            t: t0,
            state: start,
            maneuver: t0 + policy.interval_s,
            burns: Vec::new(),
            kept: vec![(t0, start)],
            worst_deviation_km: 0.0,
        }
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    /// Whether the ship strayed farther than a policy may let it
    /// ([`HELD_KM`]), which ends the flight.
    pub fn lost(&self) -> bool {
        self.worst_deviation_km > HELD_KM
    }

    /// Flies on to `t1`, keeping states and maneuvering as the policy says.
    /// `None` where the reference does not reach `t1` or a maneuver's
    /// horizon, or an arc cannot be integrated (the loop is then part way).
    /// Lost, it flies no further.
    pub fn fly_to(&mut self, field: &Field, reference: &Reference, t1: f64) -> Option<()> {
        if self.lost() || t1 <= self.t {
            return Some(());
        }
        let (_, hi) = reference.path.span();
        if t1 > hi {
            return None;
        }
        let (ode, stm) = (field.ode(reference.epoch), field.ode_stm(reference.epoch));
        // The ship is integrated to each time a state is kept at and each
        // maneuver time, in order; the first kept time is where it is.
        let mut keeps: Vec<f64> = Track::kept_times(
            reference.epoch.plus_days(self.t / SECONDS_PER_DAY),
            t1 - self.t,
        )
        .into_iter()
        .map(|t| t + self.t)
        .collect();
        *keeps.last_mut().unwrap() = t1;
        let mut keeps = keeps.into_iter().skip(1).peekable();
        let mut ship = Dop853::new(&ode, self.t, self.state, TOL);
        loop {
            let keep = keeps.peek().copied();
            let next = (self.maneuver <= t1).then_some(self.maneuver);
            let Some(t) = [keep, next].into_iter().flatten().reduce(f64::min) else {
                break;
            };
            ship.advance_to(&ode, t).ok()?;
            if keep == Some(t) {
                keeps.next();
            }
            if next == Some(t) {
                if t + self.policy.horizon_s > hi {
                    return None;
                }
                let reference_now = reference.path.at(t);
                let truth = ship.y;
                let off = (0..3)
                    .map(|i| (truth[i] - reference_now[i]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                self.worst_deviation_km = self.worst_deviation_km.max(off);
                // Lost: the policy fails, and a ship left to wander can
                // fall through the Moon's point mass, where integrating it
                // crawls (a year of NRHO took over 10 minutes).
                if self.lost() {
                    (self.t, self.state) = (t, truth);
                    self.keep(t, truth);
                    return Some(());
                }
                let estimate = noisy(&truth, self.errors, &mut self.rng);
                let delta: [f64; 6] = std::array::from_fn(|i| estimate[i] - reference_now[i]);
                // The reference's sensitivity over the horizon.
                let mut y = [0.0; 42];
                y[..6].copy_from_slice(&reference_now);
                for d in 0..6 {
                    y[6 + d * 7] = 1.0;
                }
                let mut arc = Dop853::new(&stm, t, y, TOL);
                arc.advance_to(&stm, t + self.policy.horizon_s).ok()?;
                let phi = |r: usize, c: usize| arc.y[6 + r * 6 + c];
                let miss: [f64; 3] =
                    std::array::from_fn(|r| (0..6).map(|c| phi(r, c) * delta[c]).sum());
                if miss.iter().map(|x| x * x).sum::<f64>().sqrt() > self.policy.deadband_km {
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
                        state[3 + i] +=
                            burn[i] + size * self.errors.execution * self.rng.gaussian();
                    }
                    ship = Dop853::new(&ode, t, state, TOL);
                    self.burns.push(Burn { t, dv_kms: size });
                }
                self.maneuver += self.policy.interval_s;
            }
            (self.t, self.state) = (t, ship.y);
            self.keep(t, ship.y);
        }
        Some(())
    }

    /// Keeps the state at `t`, after any burn made there: a time is kept
    /// once.
    fn keep(&mut self, t: f64, state: [f64; 6]) {
        if self.kept.last().is_some_and(|k| k.0 == t) {
            self.kept.pop();
        }
        self.kept.push((t, state));
    }

    /// What the flight cost.
    pub fn run(self) -> Run {
        let simulated = self.t - self.kept[0].0;
        let dv: f64 = self.burns.iter().map(|b| b.dv_kms).sum();
        Run {
            dv_per_year_kms: if simulated > 0.0 {
                dv * YEAR_S / simulated
            } else {
                0.0
            },
            burns: self.burns,
            worst_deviation_km: self.worst_deviation_km,
            kept: self.kept,
        }
    }
}

/// One run: a ship inserted on the reference at `t0` (s after its epoch)
/// held there until `t1`, maneuvering every `policy.interval_s` from `t0`,
/// or until it strays farther than [`HELD_KM`] ([`Loop`]). `None` where
/// the reference does not reach `t1` or a maneuver's horizon.
pub fn simulate(
    field: &Field,
    reference: &Reference,
    (t0, t1): (f64, f64),
    policy: Policy,
    errors: Errors,
    seed: u64,
) -> Option<Run> {
    let mut flight = Loop::insert(reference, t0, policy, errors, seed);
    flight.fly_to(field, reference, t1)?;
    Some(flight.run())
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
/// the ship within 500 km of the reference over `(t0, t1)` (s after its
/// epoch, each policy's horizon short of `t1`) in every one of `runs`
/// runs. `None` if none does.
pub fn upkeep(
    field: &Field,
    reference: &Reference,
    (t0, t1): (f64, f64),
    errors: Errors,
    runs: u64,
) -> Option<Upkeep> {
    let period = reference.period_s();
    let mut best: Option<Upkeep> = None;
    for interval in [0.5, 1.0] {
        for horizon in [0.5, 1.0, 2.0] {
            let policy = Policy {
                interval_s: interval * period,
                horizon_s: horizon * period,
                deadband_km: DEADBAND_KM,
            };
            let until = t1 - policy.horizon_s;
            let results: Option<Vec<Run>> = (0..runs)
                .map(|seed| simulate(field, reference, (t0, until), policy, errors, seed))
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
            let simulated_years = (until - t0) / YEAR_S;
            let maneuvers =
                results.iter().map(|r| r.burns.len()).sum::<usize>() as f64 / n / simulated_years;
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
