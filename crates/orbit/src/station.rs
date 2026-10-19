//! A vessel's station orbit (M1b.md, step 3 plan, part 2): a reference
//! built where the vessel arrives and extended a year at a time on demand,
//! shared by the vessel's checkpoints and stays, and the station-keeping
//! policy that holds it.

use crate::ephem::Ephemeris;
use crate::families::{Catalog, Member};
use crate::gravity::Field;
use crate::integrate::{DenseStep, Trajectory};
use crate::reference::{Reference, continue_orbit, settle_revolutions};
use crate::stationkeeping::{Errors, Upkeep, upkeep};
use crate::time::{Epoch, SECONDS_PER_DAY};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock, RwLockReadGuard};

/// Days the orbit reaches past what a step asks for, each way: legs are
/// designed a quarter day inside a reference's span, and a window's
/// search may shift.
pub const MARGIN_DAYS: f64 = 2.0;

/// Monte Carlo runs a policy is judged on.
const UPKEEP_RUNS: u64 = 4;

/// Policies are judged on a year of the member's orbit from here.
fn policy_epoch() -> Epoch {
    Epoch::from_ymd(2045, 3, 1)
}

/// Station-keeping policies by catalog member, judged once per process.
static POLICIES: OnceLock<Mutex<HashMap<[u64; 7], Option<Upkeep>>>> = OnceLock::new();

/// The cheapest station-keeping policy that holds a year of member `m`'s
/// orbit from [`policy_epoch`] in `field` ([`upkeep`], over
/// [`UPKEEP_RUNS`] runs), judged once per process; `None` if none does.
/// The policy is the member's, not a visit's: judged over a short visit's
/// few maneuvers, a policy can hold them and lose the ship within a year
/// (2026-10-18). The year is solved with the settle its stability asks, so
/// that it is the orbit throughout: a solve's free end leaves an unstable
/// orbit over its last revolutions. The loop's cost depends on where in
/// the orbit its maneuvers fall (M1b.md, step 3 plan, part 2), so the rate
/// is the policy's at the reference's own phase.
fn member_policy(eph: &Ephemeris, cat: &Catalog, field: &Field, m: &Member) -> Option<Upkeep> {
    let key: [u64; 7] = std::array::from_fn(|i| match i {
        6 => m.orbit.period.to_bits(),
        i => m.orbit.state[i].to_bits(),
    });
    let mut known = POLICIES.get_or_init(Default::default).lock().unwrap();
    *known.entry(key).or_insert_with(|| {
        let per_year = (365.25 / cat.days(m.orbit)).ceil() as usize;
        let settle = settle_revolutions(m.stability);
        let r = (0..3).find_map(|late| {
            let start = policy_epoch().plus_days(late as f64);
            continue_orbit(eph, field, cat, m.orbit, start, per_year, settle).ok()
        })?;
        upkeep(field, &r, r.path.span(), Errors::TYPICAL, UPKEEP_RUNS)
    })
}

/// A station orbit a vessel is on from its arrival: one continuous
/// reference, built from the catalog member and extended by whole years
/// ([`Orbit::cover`]) as far as its stays and visitors need. The years are
/// the quantum so that the sequence of solves, and so the path, is the
/// same whichever step or walk asks first; the path built never changes
/// once extended over, so what a design reads does not depend on how far
/// the orbit has been built.
pub struct Orbit {
    pub label: String,
    eph: &'static Ephemeris,
    cat: &'static Catalog,
    field: Arc<Field<'static>>,
    /// Revolutions a year, the quantum of extension.
    per_year: usize,
    /// Mean time per revolution of the first build, s: the orbit's period
    /// for every reader, however far it has been built since (the mean
    /// over the whole path moves as it grows, and a departure window
    /// sized by it designed differently resumed than fresh).
    period_s: f64,
    /// The member's station-keeping policy ([`member_policy`]).
    policy: Option<Upkeep>,
    reference: RwLock<Reference>,
}

impl Orbit {
    /// The orbit of catalog member `m` from `from` (less the margin) over
    /// what the arriving step needs, to `until` (plus the margin), and a
    /// year at least, with the settle its stability asks. A start that
    /// does not converge is retried a day or two earlier. A year at least:
    /// extending a short first build fails now and then at every step
    /// size (an NRHO's of 2 revolutions from 2045-03-04 and of 20 from
    /// 03-01, where 14 others of 2–20 extended; the guess's catalog tail
    /// broke by 5·10⁻¹ and no damping reduced it), where every chain from
    /// a year has extended (the kill-test's 34 years, four six-year chains,
    /// M1b.md, step 3 plan, part 1); and a visit shorter than a year then
    /// needs no extension at all.
    pub fn new(
        eph: &'static Ephemeris,
        cat: &'static Catalog,
        field: Arc<Field<'static>>,
        label: &str,
        m: &Member,
        from: Epoch,
        until: Epoch,
    ) -> Result<Arc<Orbit>, String> {
        let days = cat.days(m.orbit);
        let settle = settle_revolutions(m.stability);
        let per_year = (365.25 / days).ceil() as usize;
        let span = until.0 - from.0 + 2.0 * MARGIN_DAYS;
        let reference = (0..3)
            .find_map(|early| {
                let start = from.plus_days(-MARGIN_DAYS - early as f64);
                let revs = (((span + early as f64) / days).ceil() as usize + 1).max(per_year);
                continue_orbit(eph, &field, cat, m.orbit, start, revs, settle).ok()
            })
            .ok_or_else(|| format!("{label}'s orbit could not be continued from {from}"))?;
        let policy = member_policy(eph, cat, &field, m);
        Ok(Arc::new(Orbit {
            label: label.to_string(),
            eph,
            cat,
            field,
            per_year,
            period_s: reference.period_s(),
            policy,
            reference: RwLock::new(reference),
        }))
    }

    /// The reference, to read. Its path reaches at least where every
    /// [`Orbit::cover`] so far asked.
    pub fn reference(&self) -> RwLockReadGuard<'_, Reference> {
        self.reference.read().unwrap()
    }

    pub fn epoch(&self) -> Epoch {
        self.reference().epoch
    }

    pub fn end(&self) -> Epoch {
        self.reference().end()
    }

    /// Mean time per revolution, s (the first build's).
    pub fn period_s(&self) -> f64 {
        self.period_s
    }

    /// The member's station-keeping policy ([`member_policy`]); `None` if
    /// none holds its orbit.
    pub fn policy(&self) -> Option<Upkeep> {
        self.policy
    }

    /// Extends the orbit by whole years until its path reaches `until`
    /// plus the margin. On an error the orbit keeps the years that
    /// converged.
    pub fn cover(&self, until: Epoch) -> Result<(), String> {
        let needed = until.plus_days(MARGIN_DAYS);
        if self.end().0 >= needed.0 {
            return Ok(());
        }
        let mut r = self.reference.write().unwrap();
        while r.end().0 < needed.0 {
            let end = r.end();
            r.extend(self.eph, &self.field, self.cat, self.per_year)
                .map_err(|e| {
                    format!(
                        "{}'s orbit could not be extended past {end} ({:.1e} left)",
                        self.label, e.residual
                    )
                })?;
        }
        Ok(())
    }

    /// The orbit over `(t0, t1)` with the margin each way, extended to
    /// reach it, as a reference of its own (its patch points within it,
    /// not to be extended): what a leg leaving or reaching the orbit is
    /// designed against, and what a stay follows, so that no lock is held
    /// while a leg is designed.
    pub fn window(&self, (t0, t1): (Epoch, Epoch)) -> Result<Reference, String> {
        self.cover(t1)?;
        let r = self.reference();
        let seconds = |t: Epoch| (t.0 - r.epoch.0) * SECONDS_PER_DAY;
        let (a, b) = (
            seconds(t0.plus_days(-MARGIN_DAYS)),
            seconds(t1.plus_days(MARGIN_DAYS)),
        );
        let steps: Vec<DenseStep<6>> = r
            .path
            .steps()
            .iter()
            .filter(|s| s.t0 + s.h > a && s.t0 < b)
            .cloned()
            .collect();
        let path = Trajectory::new(steps);
        let (lo, hi) = path.span();
        Ok(Reference {
            epoch: r.epoch,
            center: r.center,
            revolutions: ((hi - lo) / self.period_s).round().max(1.0) as usize,
            patches: r
                .patches
                .iter()
                .copied()
                .filter(|(t, _)| (lo..=hi).contains(t))
                .collect(),
            extension: None,
            path,
        })
    }
}
