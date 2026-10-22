//! Designs by their inputs (M1b.md, step 1 plan, part 4). Each step of a
//! ship's design is keyed by a hash of the key before it and what the
//! step reads (`mission::reads`), so a design is made once for its inputs
//! and found again by an edit after it, an undo, or a re-run. A step whose
//! leg meets a vessel adds the key of the vessel's design after the leg
//! met (step 3 plan, part 3), so an edit of the vessel at or before that
//! leg designs its visitors again from their meetings, and one after it
//! nothing of theirs. A ship is designed by walking its steps ([`walk`]):
//! each step is taken from the cache, or designed, and a walk waits for
//! the vessel a step meets to be designed that far; career's re-plans
//! take effect where the ship is when they were made.

use crate::hash::Fixed;
use crate::log::Replan;
use orbit::mission::{Checkpoint, Item, ItemKind, Meeting, Planner, Ship, Step, World, reads};
use orbit::time::Epoch;
use std::collections::HashMap;
use std::hash::Hasher;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub type Key = u64;

/// What a key designs: where the ship starts, or a step's items and where
/// the ship is after them.
#[derive(Clone)]
pub enum Designed {
    Start(Result<Checkpoint, String>),
    Step {
        items: Vec<Item>,
        after: Checkpoint,
    },
    /// No steps left.
    End,
}

/// The key of a ship's start: where and when.
pub fn start_key(ship: &Ship) -> Key {
    let mut h = Fixed::default();
    h.str("start")
        .debug(&ship.start.kind)
        .str(&ship.start.label)
        .f64(ship.epoch.0);
    h.finish()
}

/// Where a vessel is after the leg a meeting names, for the step that
/// meets it: the key of the vessel's design there and its checkpoint; or
/// why it is not there (the step fails).
pub type Met = Result<(Key, Checkpoint), String>;

/// Finds where a vessel is after the leg a meeting names ([`Met`]);
/// `None` while the vessel's design has not got there.
pub type Meet<'a> = dyn Fn(&Meeting) -> Option<Met> + 'a;

/// Hashes what designing `steps` reads: a wait's days, a dock's vessel,
/// and a leg's all but its manifest, which is kept as written and designs
/// nothing (M1b.md, step 3 plan, part 4).
pub fn flight(h: &mut Fixed, steps: &[Step]) {
    for s in steps {
        match s {
            Step::Wait(days) => {
                h.str("wait").f64(*days);
            }
            Step::Go(go) => {
                h.str("go")
                    .debug(&go.to)
                    .debug(&go.days)
                    .debug(&go.window)
                    .debug(&go.pin)
                    .debug(&go.aero)
                    .debug(&go.dock)
                    .debug(&go.note);
            }
            Step::Dock(d) => {
                h.str("dock").debug(&d.to).debug(&d.note);
            }
        }
    }
}

/// The key of the step from the checkpoint keyed `at`, which goes on
/// from step `next` of `ship`; `met`, where its leg meets a vessel.
pub fn step_key(at: Key, ship: &Ship, next: usize, met: Option<&Met>) -> Key {
    let mut h = Fixed::default();
    h.str("step").u64(at).u64(next as u64);
    flight(
        &mut h,
        &ship.steps[next.min(ship.steps.len())..reads(ship, next)],
    );
    match met {
        None => {}
        Some(Ok((key, _))) => {
            h.str("meets").u64(*key);
        }
        Some(Err(why)) => {
            h.str("misses").str(why);
        }
    }
    h.finish()
}

/// The vessel the step from `next` of `ship` meets, if its leg meets one
/// or its dock names one.
fn meeting(ship: &Ship, next: usize) -> Option<&Meeting> {
    ship.steps[next.min(ship.steps.len())..reads(ship, next)]
        .iter()
        .find_map(Step::meeting)
}

/// The key of the checkpoint keyed `at` with the ship staying there until
/// at least `now` ([`Checkpoint::staying`]).
pub fn staying_key(at: Key, now: Epoch) -> Key {
    let mut h = Fixed::default();
    h.str("staying").u64(at).f64(now.0);
    h.finish()
}

/// Designs kept by their keys, shared by the walks that make and use
/// them; and how many have been made.
#[derive(Clone, Default)]
pub struct Cache {
    kept: Arc<Mutex<HashMap<Key, Arc<Designed>>>>,
    made: Arc<AtomicU64>,
}

impl Cache {
    pub fn get(&self, key: Key) -> Option<Arc<Designed>> {
        self.kept.lock().unwrap().get(&key).cloned()
    }

    /// Keeps a design just made.
    pub fn put(&self, key: Key, designed: Designed) -> Arc<Designed> {
        let designed = Arc::new(designed);
        self.kept.lock().unwrap().insert(key, designed.clone());
        self.made.fetch_add(1, Ordering::Relaxed);
        designed
    }

    /// Designs made, kept or not.
    pub fn made(&self) -> u64 {
        self.made.load(Ordering::Relaxed)
    }

    /// Keeps only the designs `keep` says to.
    pub fn retain(&self, keep: impl Fn(Key) -> bool) {
        self.kept.lock().unwrap().retain(|&k, _| keep(k));
    }
}

/// How a walk ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Ended {
    /// Every step designed; the failure the plan stops at, if any.
    Done(Option<String>),
    /// A step's design was not to be had (`fetch` gave none).
    Waiting,
}

/// Where re-plan `r` takes effect, at checkpoint `c` of a ship with
/// `steps`: whether `c` is the checkpoint before the step that holds
/// `r.from` (or the last, when it appends).
fn holds(c: &Checkpoint, steps: &[Step], r: &Replan) -> bool {
    let go = steps[c.next().min(steps.len())..]
        .iter()
        .position(|s| matches!(s, Step::Go(_)))
        .map(|k| c.next() + k);
    c.next() <= r.from && go.is_none_or(|g| r.from <= g)
}

/// `steps` with re-plan `r` applied: those from `r.from` replaced, and
/// the manifest of the stop the ship is at (the last leg's before them)
/// where `r` sets it.
pub fn apply(steps: &mut Vec<Step>, r: &Replan) {
    steps.truncate(r.from);
    if let Some(here) = &r.here
        && let Some(Step::Go(go)) = steps.iter_mut().rev().find(|s| matches!(s, Step::Go(_)))
    {
        go.manifest = here.clone();
    }
    steps.extend(r.steps.iter().cloned());
}

/// Finds a step's design by its key, or makes it with the function given
/// (`None`: not to be had now).
pub type Fetch<'a> = dyn FnMut(Key, &dyn Fn() -> Designed) -> Option<Arc<Designed>> + 'a;

/// Walks `ship`'s steps from its start, with its re-plans (in the order
/// they were made), taking each step's design from `fetch(key, step)`,
/// which finds it or designs it (`None`: not to be had now), and where a
/// step's leg meets a vessel, where the vessel is from `meet` (`None`:
/// not designed that far yet; the walk waits). Each step of the ship's
/// design as it stands goes to `out`, in order, with its key.
///
/// A re-plan takes effect at the checkpoint before the step that holds
/// its first step replaced: there, if the ship arrived after it was made;
/// at the time it was made, if the ship was staying there then. One that
/// cannot (the step's leg had begun) goes to `refused` with its place in
/// `replans` and why, and the ship flies on as before it.
pub fn walk(
    world: &World,
    ship: &Ship,
    replans: &[&Replan],
    fetch: &mut Fetch,
    meet: &Meet,
    out: &mut dyn FnMut(Key, &Arc<Designed>),
    refused: &mut dyn FnMut(usize, String),
) -> Ended {
    let mut ship = ship.clone();
    let mut key = start_key(&ship);
    let start = {
        let ship = &ship;
        fetch(key, &|| {
            Designed::Start(Planner::new(world, ship).map(|p| p.checkpoint()))
        })
    };
    let Some(start) = start else {
        return Ended::Waiting;
    };
    out(key, &start);
    let mut c = match &*start {
        Designed::Start(Ok(c)) => c.clone(),
        Designed::Start(Err(why)) => return Ended::Done(Some(why.clone())),
        _ => unreachable!("a start's key designs a start"),
    };
    let mut pending: Vec<(usize, &Replan)> = replans.iter().copied().enumerate().collect();
    loop {
        // Re-plans taking effect here: the last made that holds here (as
        // the steps stood when it was made), and those before it. One of
        // those that does not hold here changed only steps after this
        // step's leg, which the last replaces: it is passed over.
        let mut standing = ship.steps.clone();
        let mut last = None;
        for (j, (_, r)) in pending.iter().enumerate() {
            if holds(&c, &standing, r) {
                last = Some(j);
            }
            apply(&mut standing, r);
        }
        let here: Vec<(usize, &Replan)> = match last {
            Some(last) => pending.drain(..=last).collect(),
            None => Vec::new(),
        };
        for (i, r) in here {
            if !holds(&c, &ship.steps, r) {
                continue;
            }
            let (mut at, mut at_key) = (c.clone(), key);
            if c.since().0 < r.at.0 {
                // The ship was here before the re-plan: it is staying if
                // the step from here had not left by then.
                let Some((old, designed)) = keyed(world, &ship, &c, key, meet)
                    .and_then(|(k, design)| Some((k, fetch(k, &design)?)))
                else {
                    return Ended::Waiting;
                };
                let stay = match &*designed {
                    Designed::Step { items, .. } => {
                        let left = items.iter().find(|i| i.kind != ItemKind::Stay).map_or(
                            f64::INFINITY,
                            |i| match i.kind {
                                ItemKind::Failed(_) => f64::INFINITY,
                                _ => i.start.0,
                            },
                        );
                        if left <= r.at.0 {
                            refused(
                                i,
                                format!(
                                    "a re-plan made {} changes a step whose leg had begun",
                                    r.at.stamp()
                                ),
                            );
                            continue;
                        }
                        items.iter().find(|i| i.kind == ItemKind::Stay).cloned()
                    }
                    _ => None,
                };
                at = c.staying(r.at, stay.as_ref());
                // The step replaced made the stay, which fixes a free plane.
                at_key = staying_key(old, r.at);
            }
            let mut edited = ship.clone();
            apply(&mut edited.steps, r);
            (ship, c, key) = (edited, at, at_key);
        }
        let Some((k, designed)) = keyed(world, &ship, &c, key, meet)
            .and_then(|(k, design)| Some((k, fetch(k, &design)?)))
        else {
            return Ended::Waiting;
        };
        out(k, &designed);
        match &*designed {
            Designed::Step { items, after } => {
                if let Some(ItemKind::Failed(why)) = items.last().map(|i| &i.kind) {
                    return Ended::Done(Some(why.clone()));
                }
                c = after.clone();
                key = k;
            }
            Designed::End => return Ended::Done(None),
            Designed::Start(_) => unreachable!("a step's key designs a step"),
        }
    }
}

/// The key of the step from `c` of `ship` (`c` keyed `at`), and its
/// design; `None` while the vessel its leg meets is not designed that far
/// (`meet`).
fn keyed<'a>(
    world: &'a World,
    ship: &'a Ship,
    c: &'a Checkpoint,
    at: Key,
    meet: &Meet,
) -> Option<(Key, impl Fn() -> Designed + 'a)> {
    let met = match meeting(ship, c.next()) {
        None => None,
        Some(m) => Some(meet(m)?),
    };
    let key = step_key(at, ship, c.next(), met.as_ref());
    let design = move || {
        let mut planner = Planner::resume(world, ship, c.clone());
        let items = match &met {
            None => planner.step(),
            Some(met) => planner.meet(met.as_ref().map(|(_, c)| c).map_err(Clone::clone)),
        };
        match items {
            Some(items) => Designed::Step {
                items,
                after: planner.checkpoint(),
            },
            None => Designed::End,
        }
    };
    Some((key, design))
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit::ephem::Ephemeris;
    use orbit::mission::{Plan, Spot};
    use orbit::places::Places;

    /// A dock is a design input (M1b.md, step 4 plan, part 1): a leg's
    /// `dock` and a dock step's vessel change the step's key; a dock's
    /// manifest does not.
    #[test]
    fn a_dock_is_keyed() {
        let src = r#"
            name = "keys"
            [[ship]]
            id = 0
            name = "Depot"
            start = { at = "leo", epoch = "2045-03-01" }
            [[ship.leg]]
            to = "nrho"
            days = [5.0]
            [[ship]]
            id = 1
            name = "Tug"
            start = { at = "leo", epoch = "2045-03-06" }
            [[ship.leg]]
            to = { ship = 0, leg = 0 }
            days = [5.0]
            [[ship.leg]]
            dock = { ship = 0, leg = 0 }
        "#;
        let plan = Plan::parse(src, Ephemeris::standard(), Places::standard()).unwrap();
        let tug = &plan.ships[1];
        let key = |ship: &Ship, next: usize| step_key(7, ship, next, None);
        let mut docked = tug.clone();
        let Step::Go(go) = &mut docked.steps[0] else {
            panic!("a leg")
        };
        go.dock = true;
        assert_ne!(key(tug, 0), key(&docked, 0));
        assert_eq!(
            key(tug, 1),
            key(&docked, 1),
            "the dock step's key is its own"
        );
        let mut other = tug.clone();
        let Step::Dock(d) = &mut other.steps[1] else {
            panic!("a dock")
        };
        d.to = Spot::meeting(Meeting { ship: 0, leg: None }, d.to.kind.clone());
        assert_ne!(key(tug, 1), key(&other, 1));
        let mut loaded = tug.clone();
        loaded.steps[1].manifest_mut().unwrap().propellant_t = Some(10.0);
        assert_eq!(key(tug, 1), key(&loaded, 1), "a manifest designs nothing");
    }
}
