//! Designs by their inputs (M1b.md, step 1 plan, part 4). Each step of a
//! ship's design is keyed by a hash of the key before it and what the
//! step reads (`mission::reads`), so a design is made once for its inputs
//! and found again by an edit after it, an undo, or a re-run. A ship is
//! designed by walking its steps ([`walk`]): each step is taken from the
//! cache, or designed; career's re-plans take effect where the ship is
//! when they were made.

use crate::hash::Fixed;
use crate::log::Replan;
use orbit::mission::{Checkpoint, Item, ItemKind, Planner, Ship, Step, World, reads};
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

/// The key of a ship's start: where and when, and the steps a station
/// start's visit is built for.
pub fn start_key(ship: &Ship) -> Key {
    let mut h = Fixed::default();
    h.str("start")
        .debug(&ship.start.kind)
        .str(&ship.start.label)
        .f64(ship.epoch.0);
    for s in &ship.steps[..reads(ship, None)] {
        h.debug(s);
    }
    h.finish()
}

/// The key of the step from the checkpoint keyed `at`, which goes on
/// from step `next` of `ship`.
pub fn step_key(at: Key, ship: &Ship, next: usize) -> Key {
    let mut h = Fixed::default();
    h.str("step").u64(at).u64(next as u64);
    for s in &ship.steps[next.min(ship.steps.len())..reads(ship, Some(next))] {
        h.debug(s);
    }
    h.finish()
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

/// `steps` with re-plan `r` applied.
pub fn apply(steps: &mut Vec<Step>, r: &Replan) {
    steps.truncate(r.from);
    steps.extend(r.steps.iter().cloned());
}

/// Finds a step's design by its key, or makes it with the function given
/// (`None`: not to be had now).
pub type Fetch<'a> = dyn FnMut(Key, &dyn Fn() -> Designed) -> Option<Arc<Designed>> + 'a;

/// Walks `ship`'s steps from its start, with its re-plans (in the order
/// they were made), taking each step's design from `fetch(key, step)`,
/// which finds it or designs it (`None`: not to be had now). Each step of
/// the ship's design as it stands goes to `out`, in order, with its key.
///
/// A re-plan takes effect at the checkpoint before the step that holds
/// its first step replaced: there, if the ship arrived after it was made;
/// at the time it was made, if the ship was staying there then. One that
/// cannot (the step's leg had begun, or a station's visit was built
/// shorter than the new steps need) goes to `refused` with its place in
/// `replans` and why, and the ship flies on as before it.
pub fn walk(
    world: &World,
    ship: &Ship,
    replans: &[&Replan],
    fetch: &mut Fetch,
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
                let old = step_key(key, &ship, c.next());
                let Some(designed) = fetch(old, &step(world, &ship, &c)) else {
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
            if let Some(why) = Planner::resume(world, &edited, at.clone()).outstays() {
                refused(i, why);
                continue;
            }
            (ship, c, key) = (edited, at, at_key);
        }
        let k = step_key(key, &ship, c.next());
        let Some(designed) = fetch(k, &step(world, &ship, &c)) else {
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

/// The step from `c` of `ship`, designed.
fn step<'a>(world: &'a World, ship: &'a Ship, c: &'a Checkpoint) -> impl Fn() -> Designed + 'a {
    move || {
        let mut planner = Planner::resume(world, ship, c.clone());
        match planner.step() {
            Some(items) => Designed::Step {
                items,
                after: planner.checkpoint(),
            },
            None => Designed::End,
        }
    }
}
