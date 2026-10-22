//! A game (M1b.md, step 1 plan, parts 4 and 5): its log, its economy as
//! drawn, each ship's design as it stands, the state the loop reaches over
//! them (`state`), and its mode's policies. Ships are designed in the
//! background, on the host's pool, by walks (`designs::walk`) that take
//! what the cache has and design the rest; an edit or an undo walks
//! again, and finds what it did not change in the cache. A ship whose leg
//! meets a vessel (step 3 plan, part 3) reads where the vessel is from
//! the fleet, the designs as they stand, and its walk waits for the
//! vessel's to get there.

use crate::designs::{Cache, Designed, Designs, Ended, Host, Key, Met, apply, flight, walk};
use crate::hash::Fixed;
use crate::log::{Check, Log, Mode, Replan, here_without_a_stop, minute};
use crate::params::{Draw, Economy, EconomyFile};
use crate::report;
use crate::ships::{self, Breach, Commodity};
use crate::state::{self, Course, Dry, Kind, ShipState, State, Stop, With};
use orbit::mission::{
    Checkpoint, Item, ItemKind, Manifest, MarkerKind, Meeting, Plan, Ship, Step, World,
    resume_point, when,
};
use orbit::places::Places;
use orbit::routes::RouteGraph;
use orbit::time::{Epoch, SECONDS_PER_DAY};
use std::cell::Cell;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::hash::Hasher;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, LazyLock, Mutex};

/// The fields legs are designed in, built once.
static WORLD: LazyLock<World> = LazyLock::new(World::standard);

/// The fields legs are designed and priced in.
pub fn world() -> &'static World {
    &WORLD
}

/// Designs of ships edited away kept for undo: those of this many of the
/// last designs replaced.
const KEPT_DESIGNS: usize = 64;

/// A ship of the game and its design as it stands.
pub struct ShipDesign {
    pub id: u64,
    pub name: String,
    /// Designed to its end; and why it stops, if it does.
    pub done: bool,
    pub failure: Option<String>,
    /// Its re-plans that could not take effect where the ship was (it
    /// flies on as before each): their places among its re-plans, and why.
    pub refused: Vec<(usize, String)>,
    /// What its class does not allow of it, its steps as they stand: the
    /// sandbox's warnings (career refuses them).
    pub breaches: Vec<Breach>,
    /// Another whenever its items change, unique in the process: what
    /// views of them are built for.
    pub revision: u64,
    /// Its design's steps as they stand, by key.
    steps: Vec<(Key, Arc<Designed>)>,
    /// The vessel its walk stopped at a meeting of, not designed that far
    /// yet: the walk goes on when that vessel's design does.
    waiting: Option<u64>,
    /// A hash of what it is designed from: the log's ship (its name, class
    /// and load aside) and its re-plans.
    inputs: Key,
    /// Every key its walks took, shown or not (a re-plan's decision reads
    /// the step it replaced): shared with its walk in the background,
    /// which adds a key before it keeps a design, so that designs are let
    /// go only when no walk of a ship has taken them.
    used: Arc<Mutex<HashSet<Key>>>,
    live: Arc<AtomicBool>,
}

impl ShipDesign {
    /// What the ship does, so far as it is designed.
    pub fn items(&self) -> impl DoubleEndedIterator<Item = &Item> {
        self.steps.iter().flat_map(|(_, d)| match &**d {
            Designed::Step { items, .. } | Designed::Start(Ok((_, items))) => items.as_slice(),
            _ => &[],
        })
    }

    /// A hash of what it is designed from, which changes only when the
    /// ship (its name, class and load aside) or its re-plans do.
    pub fn inputs(&self) -> Key {
        self.inputs
    }

    /// Where the ship is before each step designed: its start's and each
    /// step's checkpoint after it.
    fn checkpoints(&self) -> impl Iterator<Item = &Checkpoint> {
        self.steps.iter().filter_map(|(_, d)| match &**d {
            Designed::Start(Ok((c, _))) | Designed::Step { after: c, .. } => Some(c),
            _ => None,
        })
    }
}

/// A ship's revision not given before in the process.
fn revision() -> u64 {
    static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    LAST.fetch_add(1, Ordering::Relaxed) + 1
}

/// What a ship's walk reports.
enum Event {
    /// The `n`th step of the design as it stands.
    Shown(usize, Key, Arc<Designed>),
    /// A re-plan that could not take effect: its place among the ship's
    /// re-plans, and why.
    Refused(usize, String),
    /// The walk stopped at a meeting of `vessel`, whose design it saw as
    /// `seen`, not there yet.
    Waiting {
        vessel: u64,
        seen: Seen,
    },
    Ended(Ended),
    Panicked(String),
}

/// What a walk saw of a vessel: how many steps of its design, and whether
/// its walk had ended.
type Seen = (usize, bool);

/// The game's ships as their designs stand, for the legs that meet them
/// and the ships docked to them (M1b.md, step 3 plan, part 3; step 4 plan,
/// part 2): shared with the walks in the background, which read where a
/// vessel is from it.
#[derive(Default)]
struct Fleet {
    by_id: HashMap<u64, Vessel>,
}

/// A ship as its design stands: its steps as they stand, its design's
/// steps, and whether its walk has ended.
struct Vessel {
    steps: Vec<Step>,
    designed: Vec<(Key, Arc<Designed>)>,
    done: bool,
}

impl Fleet {
    fn seen(&self, id: u64) -> Seen {
        self.by_id
            .get(&id)
            .map_or((0, false), |v| (v.designed.len(), v.done))
    }

    /// Where the vessel `m` names is after the leg named ([`Met`]): the
    /// design step that holds the leg, and its checkpoint; where the
    /// vessel is docked there, where its own vessel is then. `None` while
    /// the designs have not got there (the vessel waited on goes to
    /// `waited`).
    fn met(&self, m: &Meeting, waited: &mut Option<u64>) -> Option<Met> {
        let (key, c) = match self.met_alone(m)? {
            Ok(found) => found,
            Err(why) => return Some(Err(why)),
        };
        let Some(host) = c.docked_to() else {
            return Some(Ok((key, c)));
        };
        let at = self.at(host, c.since(), waited)?;
        Some(
            at.map(|(k2, c2, _)| (chain(key, k2), c2))
                .map_err(|why| format!("ship {} is docked to ship {host} there: {why}", m.ship)),
        )
    }

    /// [`Fleet::met`], the vessel's own design: its checkpoint after the
    /// leg named, docked or not.
    fn met_alone(&self, m: &Meeting) -> Option<Met> {
        let label = format!("ship {}", m.ship);
        let Some(v) = self.by_id.get(&m.ship) else {
            return Some(Err(format!("no ship has id {}", m.ship)));
        };
        let Some(leg) = m.leg else {
            return v.designed.first().map(|(key, d)| match &**d {
                Designed::Start(Ok((c, _))) => Ok((*key, c.clone())),
                Designed::Start(Err(why)) => Err(format!("{label} cannot start: {why}")),
                _ => unreachable!("a design's first step is its start"),
            });
        };
        match v.steps.get(leg) {
            None => {
                return Some(Err(format!(
                    "{label} has {} steps, no leg {leg}",
                    v.steps.len()
                )));
            }
            Some(Step::Wait(_)) => {
                return Some(Err(format!("step {leg} of {label} is a wait, not a leg")));
            }
            Some(Step::Go(_) | Step::Dock(_)) => {}
        }
        for (key, d) in &v.designed {
            match &**d {
                Designed::Start(Err(why)) => {
                    return Some(Err(format!("{label} cannot start: {why}")));
                }
                Designed::Start(Ok(_)) => {}
                Designed::Step { items, after } => {
                    if let Some(ItemKind::Failed(why)) = items.last().map(|i| &i.kind) {
                        return Some(Err(format!("{label} does not reach leg {leg}: {why}")));
                    }
                    if after.next() > leg {
                        return Some(Ok((*key, after.clone())));
                    }
                }
                Designed::End => {
                    return Some(Err(format!("{label}'s plan ends before leg {leg}")));
                }
            }
        }
        v.done
            .then(|| Err(format!("{label} is not designed to leg {leg}")))
    }

    /// Where vessel `host` is at `t` ([`Host`]; M1b.md, step 4 plan, part
    /// 2), by its design: stopped, at the checkpoint before the stay that
    /// spans `t` (or before the step `t` falls short of), until the flight
    /// after it leaves (never, as far as its plan goes, after a trailing
    /// stay or its plan's end); in flight at `t`, at its next stop, from
    /// its arrival. A vessel docked there is where its own vessel is.
    /// `None` while the designs do not reach `t` (the vessel waited on
    /// goes to `waited`).
    fn at(&self, host: u64, t: Epoch, waited: &mut Option<u64>) -> Option<Host> {
        self.at_through(host, t, waited, 0)
    }

    fn at_through(
        &self,
        host: u64,
        t: Epoch,
        waited: &mut Option<u64>,
        depth: usize,
    ) -> Option<Host> {
        let label = format!("ship {host}");
        if depth > 2 * (self.by_id.len() + 1) {
            return Some(Err(format!("{label} is docked through a cycle")));
        }
        let Some(v) = self.by_id.get(&host) else {
            return Some(Err(format!("no ship has id {host}")));
        };
        // The stop found: the key of the design that put the vessel
        // there, its checkpoint, and when the vessel leaves it. The key
        // returned hashes the two things the stop is to a ship docked
        // there: that key and the departure, so that an edit of the vessel
        // after the departure, leaving it as it was, re-keys nothing.
        let mut found: Option<(Key, &Checkpoint, Option<Epoch>)> = None;
        let mut current: Option<(Key, &Checkpoint)> = None;
        for (key, d) in &v.designed {
            match &**d {
                Designed::Start(Err(why)) => {
                    return Some(Err(format!("{label} cannot start: {why}")));
                }
                Designed::Start(Ok((c, _))) => {
                    if t.0 < c.since().0 - 1e-9 {
                        return Some(Err(format!(
                            "{label} starts {}, after {}",
                            when(c.since()),
                            when(t)
                        )));
                    }
                    current = Some((*key, c));
                }
                Designed::Step { items, after } => {
                    let (at_key, at) = current.expect("a design starts with its start");
                    if let Some(ItemKind::Failed(_)) = items.last().map(|i| &i.kind) {
                        // The plan stops here: the vessel stays where it was.
                        found = Some((at_key, at, None));
                        break;
                    }
                    let first = items.first().map_or(after.since(), |i| i.start);
                    if t.0 < first.0 {
                        // Between steps: at the stop before, until this
                        // step's flight leaves; a dock step, until the
                        // vessel docked to leaves.
                        found = Some(match items.first().map(|i| (&i.kind, i.start)) {
                            Some((ItemKind::Docked(h2), td)) => {
                                match self.at_through(*h2, td, waited, depth + 1)? {
                                    Ok((k2, _, until)) => (chain(at_key, k2), at, until),
                                    Err(why) => return Some(Err(why)),
                                }
                            }
                            _ => (at_key, at, items.iter().find(|i| flies(i)).map(|i| i.start)),
                        });
                        break;
                    }
                    if let Some(i) = items.iter().find(|i| i.start.0 <= t.0 && t.0 < i.end.0) {
                        if i.kind == ItemKind::Stay {
                            // Stopped, until the flight after the stay; a
                            // trailing stay: for ever, once the plan's end
                            // is designed.
                            let leaves = items
                                .iter()
                                .filter(|j| flies(j))
                                .find(|j| j.start.0 >= i.end.0 - 1e-9)
                                .map(|j| j.start);
                            if leaves.is_none() && !v.done {
                                *waited = Some(host);
                                return None;
                            }
                            found = Some((at_key, at, leaves));
                            break;
                        }
                        // In flight: at the next stop, from its arrival.
                        return self.at_through(host, after.since(), waited, depth + 1);
                    }
                    current = Some((*key, after));
                }
                Designed::End => {
                    let (k, c) = current.expect("a design starts with its start");
                    found = Some((k, c, None));
                    break;
                }
            }
        }
        let (at_key, c, until) = match found {
            Some(found) => found,
            None => {
                let Some((k, c)) = current.filter(|_| v.done) else {
                    *waited = Some(host);
                    return None;
                };
                (k, c, None)
            }
        };
        match c.docked_to() {
            None => Some(Ok((host_key(at_key, until), c.clone(), until))),
            Some(h2) => {
                let inner = self.at_through(h2, t, waited, depth + 1)?;
                Some(
                    inner
                        .map(|(k2, c2, u2)| {
                            let until = earliest(until, u2);
                            (host_key(chain(at_key, k2), until), c2, until)
                        })
                        .map_err(|why| format!("{label} is docked to ship {h2}: {why}")),
                )
            }
        }
    }
}

/// The key of a vessel's stop to a ship docked there: the key of the
/// design that put the vessel there, and when it leaves.
fn host_key(at: Key, until: Option<Epoch>) -> Key {
    let mut h = Fixed::default();
    h.u64(at).debug(&until);
    h.finish()
}

/// The earlier of two departures, `None` being never.
fn earliest(a: Option<Epoch>, b: Option<Epoch>) -> Option<Epoch> {
    match (a, b) {
        (Some(a), Some(b)) => Some(Epoch(a.0.min(b.0))),
        (a, None) => a,
        (None, b) => b,
    }
}

/// Whether `item` is flown: a leg, landing, ascent or capture (a stay, a
/// dock and a failure are not).
fn flies(item: &Item) -> bool {
    matches!(
        item.kind,
        ItemKind::Leg | ItemKind::Landing | ItemKind::Ascent | ItemKind::Capture
    )
}

/// One key of two.
fn chain(a: Key, b: Key) -> Key {
    let mut h = Fixed::default();
    h.u64(a).u64(b);
    h.finish()
}

/// The fleet as a walk reads it ([`Designs`]), noting the vessel it waited
/// on, and how far it had seen it designed.
struct Lookup<'a> {
    fleet: &'a Mutex<Fleet>,
    waited: Cell<Option<(u64, Seen)>>,
}

impl Designs for Lookup<'_> {
    fn met(&self, m: &Meeting) -> Option<Met> {
        let fleet = self.fleet.lock().unwrap();
        let mut waited = Some(m.ship);
        let met = fleet.met(m, &mut waited);
        if met.is_none() {
            let v = waited.unwrap_or(m.ship);
            self.waited.set(Some((v, fleet.seen(v))));
        }
        met
    }

    fn at(&self, host: u64, t: Epoch) -> Option<Host> {
        let fleet = self.fleet.lock().unwrap();
        let mut waited = Some(host);
        let at = fleet.at(host, t, &mut waited);
        if at.is_none() {
            let v = waited.unwrap_or(host);
            self.waited.set(Some((v, fleet.seen(v))));
        }
        at
    }
}

/// A career action made and not yet judged (M1b.md, step 3 plan, part 6):
/// pending until the ships are designed to their ends, then refused if a
/// vessel runs dry or a transfer falls short where none did before it.
struct Pending {
    /// The log before it, to go back to; none for a log's own ships.
    before: Option<Log>,
    /// What ran dry or short before it.
    found: Found,
    /// What it is, for a message.
    what: String,
    /// Its place in a log being read back ("replan 2"), if it is one.
    label: Option<String>,
}

/// What the loop found wrong: the ships that ran dry, and the transfers
/// that fell short (ship, step, a take, the commodity).
#[derive(Clone, Default, PartialEq)]
struct Found {
    dry: BTreeSet<u64>,
    short: BTreeSet<(u64, usize, bool, usize)>,
}

pub struct Game {
    log: Log,
    /// The plan as it stands: the log's ships with their re-plans.
    plan: Plan,
    mode: Mode,
    /// Drawn from the log's seed, on the standard route graph.
    economy: Economy,
    /// The loop's, over the designs as they stand.
    state: State,
    /// Career: the time the game has reached.
    now: Epoch,
    /// Career: the action made and not yet judged.
    pending: Option<Pending>,
    /// Career: a log's re-plans being read back, each made again in turn
    /// (by its place in the log), and the log's now, for when they are.
    replay: VecDeque<(usize, Replan)>,
    finally: Option<Epoch>,
    /// Career: the last action refused by the projection, and why.
    refused: Option<String>,
    path: Option<PathBuf>,
    undo: Vec<Log>,
    ships: Vec<ShipDesign>,
    cache: Cache,
    /// The ships as their designs stand, for meetings.
    fleet: Arc<Mutex<Fleet>>,
    /// Keys of designs replaced, for undo.
    kept: VecDeque<Vec<Key>>,
    pool: Arc<rayon::ThreadPool>,
    sender: Sender<(Arc<AtomicBool>, Event)>,
    receiver: Receiver<(Arc<AtomicBool>, Event)>,
}

impl Drop for Game {
    /// Its walks stop after the design they are on.
    fn drop(&mut self) {
        for s in &self.ships {
            s.live.store(false, Ordering::Relaxed);
        }
    }
}

/// `ship` with `replans` (its own, in order) applied in turn, but those
/// `refused`: its steps as they stand.
fn standing(ship: &Ship, replans: &[&Replan], refused: &[(usize, String)]) -> Ship {
    let mut ship = ship.clone();
    for (i, r) in replans.iter().enumerate() {
        if !refused.iter().any(|(j, _)| *j == i) {
            apply(&mut ship.steps, r);
        }
    }
    ship
}

/// Where a standing step's manifest is written in the log: the plan's
/// step, a re-plan's leg (by its place in the log and among the legs), or
/// a re-plan's `here`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Plan(usize),
    Leg(usize, usize),
    Here(usize),
}

impl Owner {
    /// The manifest written there, for ship `k`.
    fn manifest<'a>(&self, log: &'a Log, k: usize) -> &'a Manifest {
        let leg = |step: &'a Step| {
            step.manifest()
                .expect("a manifest's owner is a leg or a dock")
        };
        match *self {
            Owner::Plan(i) => leg(&log.plan.ships[k].steps[i]),
            Owner::Leg(j, n) => leg(&log.replans[j].steps[n]),
            Owner::Here(j) => log.replans[j].here.as_ref().expect("a `here` set"),
        }
    }

    /// When it was set: a re-plan's `here` at the re-plan's time; a leg's
    /// manifest with the leg.
    fn at(&self, log: &Log) -> Option<Epoch> {
        match *self {
            Owner::Here(j) => Some(log.replans[j].at),
            _ => None,
        }
    }
}

/// For each of ship `k`'s steps as they stand (its re-plans applied in
/// turn, but those `refused`), where its manifest was written, in order:
/// the plan's step or the re-plan's leg that made it, then each re-plan's
/// `here` that set it anew. The last is the manifest standing.
fn owners(log: &Log, k: usize, refused: &[(usize, String)]) -> Vec<Vec<Owner>> {
    let id = log.plan.ships[k].id;
    let mut steps = log.plan.ships[k].steps.clone();
    let mut owners: Vec<Vec<Owner>> = (0..steps.len()).map(|i| vec![Owner::Plan(i)]).collect();
    let mine = log.replans.iter().enumerate().filter(|(_, r)| r.ship == id);
    for (i, (j, r)) in mine.enumerate() {
        if refused.iter().any(|(n, _)| *n == i) {
            continue;
        }
        steps.truncate(r.from);
        owners.truncate(r.from);
        if r.here.is_some()
            && let Some(g) = steps.iter().rposition(Step::is_stop)
        {
            owners[g].push(Owner::Here(j));
        }
        steps.extend(r.steps.iter().cloned());
        owners.extend((0..r.steps.len()).map(|n| vec![Owner::Leg(j, n)]));
    }
    owners
}

/// Writes the manifests of ship `k`'s legs, its steps as they stand
/// (`standing`, as [`standing`] has them), to where each stands in the
/// log ([`owners`], the last of each).
fn remanifest(log: &mut Log, k: usize, refused: &[(usize, String)], standing: &[Step]) {
    let owners = owners(log, k, refused);
    debug_assert_eq!(owners.len(), standing.len());
    for (chain, step) in owners.iter().zip(standing) {
        let Some(manifest) = step.manifest().cloned() else {
            continue;
        };
        match *chain.last().expect("a step has an owner") {
            Owner::Plan(i) => {
                if let Some(mine) = log.plan.ships[k].steps[i].manifest_mut() {
                    *mine = manifest;
                }
            }
            Owner::Leg(j, n) => {
                if let Some(mine) = log.replans[j].steps[n].manifest_mut() {
                    *mine = manifest;
                }
            }
            Owner::Here(j) => log.replans[j].here = Some(manifest),
        }
    }
}

/// Why `log` cannot be played in `mode` with `economy`, if it cannot:
/// each ship's class and load are read (`ships::read`); and career, which
/// refuses an action that breaks a ship's class when it is made, has each
/// ship start at LEO, where it is bought, and keep within its class as
/// each of its re-plans leaves its steps (M1b.md, step 2 plan, part 2).
fn allowed(log: &Log, mode: Mode, economy: &Economy) -> Result<(), String> {
    for ship in &log.plan.ships {
        let fail = |e: String| format!("ship {:?}: {e}", ship.name);
        let fitted = ships::read(ship, economy).map_err(fail)?;
        if mode == Mode::Sandbox {
            continue;
        }
        if ship.start.source.as_str() != Some("leo") {
            return Err(fail(format!(
                "a ship starts at LEO, where it is bought, not {}",
                ship.start.label
            )));
        }
        if ship
            .cargo
            .get(Commodity::Metal.id())
            .is_some_and(|&t| t > 0.0)
        {
            return Err(fail(
                "a ship is bought at LEO, where metal is not sold".into(),
            ));
        }
        let Some(fitted) = fitted else {
            continue;
        };
        let first = |s: &Ship| {
            ships::breaches(s, &fitted, economy, world().eph, &log.plan.ships)
                .into_iter()
                .next()
        };
        if let Some(b) = first(ship) {
            return Err(fail(b.to_string()));
        }
        let mut replanned = ship.clone();
        for (i, r) in log.replans.iter().enumerate() {
            if r.ship != ship.id {
                continue;
            }
            apply(&mut replanned.steps, r);
            // The steps before it were checked: what it breaks is among
            // its own, numbered as its legs are (`[[replan.leg]]`), or
            // the manifest it sets (`here`).
            if let Some(b) = first(&replanned) {
                let at = match b.step {
                    Some(s) if s < r.from => "here".to_string(),
                    step => format!("leg {}", step.map_or(0, |s| s - r.from) + 1),
                };
                return Err(format!("replan {}: {at}: {}", i + 1, b.why));
            }
        }
    }
    Ok(())
}

impl Game {
    /// `log` played in its own mode, its ships designed on `pool`; why
    /// not, if it cannot be.
    pub fn new(log: Log, pool: Arc<rayon::ThreadPool>) -> Result<Game, String> {
        let mode = log.setup.mode;
        Game::in_mode(log, mode, pool)
    }

    /// `log` played in `mode`. A career game's log opens in the sandbox as
    /// a sandbox log (its re-plans kept as they were made); a sandbox
    /// log played in career starts from its start, else its first ship's.
    pub fn in_mode(mut log: Log, mode: Mode, pool: Arc<rayon::ThreadPool>) -> Result<Game, String> {
        let economy = EconomyFile::standard().draw(
            world().eph,
            RouteGraph::standard(),
            &mut log.setup.seed.map_or(Draw::Central, Draw::seeded),
        );
        allowed(&log, mode, &economy)?;
        let (sender, receiver) = channel();
        let first = log
            .plan
            .ships
            .iter()
            .map(|s| s.epoch)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let start = log
            .setup
            .start
            .or(first)
            .unwrap_or(Epoch::from_ymd(2040, 1, 1));
        let now = log.setup.now.unwrap_or(start);
        log.setup.mode = mode;
        log.setup.now = (mode == Mode::Career).then_some(now);
        // Career reads its log back as the game it was (M1b.md, step 3
        // plan, part 6): its ships first, judged together, then each
        // re-plan made again when it was made, judged in turn; the clock
        // comes to the log's now after the last.
        let career = mode == Mode::Career;
        let replay: VecDeque<(usize, Replan)> = match career {
            true => std::mem::take(&mut log.replans)
                .into_iter()
                .enumerate()
                .collect(),
            false => VecDeque::new(),
        };
        let pending = (career && !log.plan.ships.is_empty()).then(|| Pending {
            before: None,
            found: Found::default(),
            what: "the plan's ships".into(),
            label: None,
        });
        let mut game = Game {
            plan: log.plan.clone(),
            log,
            mode,
            economy,
            state: State::default(),
            now: if career { start } else { now },
            pending,
            replay,
            finally: career.then_some(now),
            refused: None,
            path: None,
            undo: Vec::new(),
            ships: Vec::new(),
            cache: Cache::default(),
            fleet: Arc::default(),
            kept: VecDeque::new(),
            pool,
            sender,
            receiver,
        };
        game.reshaped();
        Ok(game)
    }

    /// Reads the log at `path` and starts designing it, in `mode` (else
    /// its own).
    pub fn open(
        path: &Path,
        mode: Option<Mode>,
        pool: Arc<rayon::ThreadPool>,
    ) -> Result<Game, String> {
        let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let log = Log::parse(&src, world().eph, Places::standard())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mode = mode.unwrap_or(log.setup.mode);
        let mut game =
            Game::in_mode(log, mode, pool).map_err(|e| format!("{}: {e}", path.display()))?;
        game.path = Some(path.to_path_buf());
        Ok(game)
    }

    /// An empty sandbox plan named `name`, to be saved at `path`.
    pub fn empty(name: &str, path: PathBuf, pool: Arc<rayon::ThreadPool>) -> Game {
        let log = Log::sandbox(Plan {
            name: name.to_string(),
            ships: Vec::new(),
            header: String::new(),
            game: None,
            replans: Vec::new(),
            checks: Vec::new(),
        });
        let mut game = Game::new(log, pool).expect("an empty plan is a game");
        game.path = Some(path);
        game
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Writes the log to its file: where, or why not.
    pub fn save(&mut self) -> Result<PathBuf, String> {
        let path = self.path.clone().ok_or("the plan has no file")?;
        std::fs::write(&path, self.log().to_toml())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }

    /// The log, with career's now as the game has it.
    pub fn log(&self) -> Log {
        let mut log = self.log.clone();
        if log.setup.mode == Mode::Career {
            log.setup.now = Some(minute(self.now));
        }
        log
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn economy(&self) -> &Economy {
        &self.economy
    }

    /// The state the loop reached (M1b.md, step 2 plan, part 3).
    pub fn state(&self) -> &State {
        &self.state
    }

    /// Runs the loop again from the start, over the designs as they stand.
    fn rerun(&mut self) {
        let courses: Vec<Course> = (0..self.ships.len())
            .map(|k| {
                let (d, ship) = (&self.ships[k], &self.plan.ships[k]);
                let items: Vec<&Item> = d.items().collect();
                let stops = self.stops(k, &items);
                Course {
                    id: d.id,
                    fitted: ships::read(ship, &self.economy)
                        .expect("a game's ships read (`allowed`)"),
                    epoch: ship.epoch,
                    items,
                    done: d.done,
                    stops,
                    bought: ship.start.source.as_str() == Some("leo"),
                }
            })
            .collect();
        let state = state::run(&courses, &self.economy);
        self.state = state;
    }

    /// Ship `k`'s stops for the loop, by step (M1b.md, step 3 plan, parts
    /// 4 and 5): with whom its leg's manifest transfers, the manifest
    /// standing when it arrives (`items` say when: a re-plan's `here` made
    /// after that does not change what was given) and the manifest as it
    /// stands when it leaves.
    fn stops(&self, k: usize, items: &[&Item]) -> Vec<Option<Stop>> {
        let ship = &self.plan.ships[k];
        let owners = owners(&self.log, k, &self.ships[k].refused);
        ship.steps
            .iter()
            .zip(&owners)
            .enumerate()
            .map(|(i, (step, chain))| {
                let (Some(to), Some(manifest)) = (step.to(), step.manifest()) else {
                    return None;
                };
                let read =
                    |m: &Manifest| ships::manifest(m).expect("a game's manifests read (`allowed`)");
                let arrived = items.iter().rev().find(|it| it.step == i).map(|it| it.end);
                let standing = chain
                    .iter()
                    .rev()
                    .find(|o| {
                        o.at(&self.log)
                            .is_none_or(|at| arrived.is_some_and(|t| at.0 <= t.0))
                    })
                    .unwrap_or(&chain[0]);
                let arrive = read(standing.manifest(&self.log, k));
                let leave = read(manifest);
                if arrive.is_empty() && leave.is_empty() {
                    return None;
                }
                let with = match to.meeting {
                    Some(m) => With::Vessel {
                        id: m.ship,
                        leg: m.leg,
                    },
                    None if to.source.as_str() == Some("leo") => With::Earth,
                    None => With::NoOne,
                };
                Some(Stop {
                    with,
                    arrive,
                    leave,
                })
            })
            .collect()
    }

    /// The plan as it stands: each ship with its re-plans.
    pub fn plan(&self) -> &Plan {
        &self.plan
    }

    pub fn ships(&self) -> &[ShipDesign] {
        &self.ships
    }

    /// Ship `id`'s place in [`Game::ships`] and the plan.
    pub fn index(&self, id: u64) -> Option<usize> {
        self.ships.iter().position(|s| s.id == id)
    }

    /// Designs made since the game began (the cache's, M1b.md, step 1
    /// plan, part 4): what an edit or an undo cost.
    pub fn designs_made(&self) -> u64 {
        self.cache.made()
    }

    /// Career: the time the game has reached.
    pub fn now(&self) -> Epoch {
        self.now
    }

    /// Career: the action made and not yet judged, if any (M1b.md, step 3
    /// plan, part 6).
    pub fn pending(&self) -> Option<&str> {
        self.pending.as_ref().map(|p| p.what.as_str())
    }

    /// Career: the last action the projection refused, and why; a log read
    /// back that was refused, as "replan N: why". Cleared by the next
    /// action.
    pub fn refused(&self) -> Option<&str> {
        self.refused.as_deref()
    }

    /// What the loop found wrong as the state stands.
    fn found(&self) -> Found {
        let mut found = Found::default();
        for s in &self.state.ships {
            if s.dry.is_some() {
                found.dry.insert(s.id);
            }
            for x in s.transfers.iter().filter(|x| x.short.is_some()) {
                found
                    .short
                    .insert((s.id, x.step, x.kind == Kind::Take, x.commodity as usize));
            }
        }
        found
    }

    /// Why the state as it stands refuses the action that led to it, if
    /// it does: the earliest vessel to run dry, or transfer to fall short,
    /// where none did `before` it (a transfer before a burn at one time,
    /// as the loop takes them: a short take is often why a ship runs dry
    /// at its departure).
    fn judged(&self, before: &Found) -> Option<String> {
        let mut found: Vec<(f64, u8, usize, String)> = Vec::new();
        for (k, s) in self.state.ships.iter().enumerate() {
            let name = &self.plan.ships[k].name;
            if let Some(dry) = &s.dry
                && !before.dry.contains(&s.id)
            {
                let items: Vec<&Item> = self.ships[k].items().collect();
                let why = format!("ship {name:?} {}", ran_dry(&items, dry));
                found.push((dry.t.0, 1, k, why));
            }
            for x in &s.transfers {
                let Some(short) = x.short else { continue };
                if before.short.contains(&(
                    s.id,
                    x.step,
                    x.kind == Kind::Take,
                    x.commodity as usize,
                )) {
                    continue;
                }
                let at = self.plan.ships[k].steps[x.step]
                    .to()
                    .expect("a manifest is a leg's or a dock's")
                    .label
                    .clone();
                let kind = match x.kind {
                    Kind::Give => "give",
                    Kind::Take => "take",
                };
                let why = format!(
                    "ship {name:?}'s {kind} of {} of {} at {at} on {} fell short: {} ({short})",
                    report::tonnes(x.asked),
                    x.commodity.name(),
                    when(x.t),
                    report::tonnes(x.got),
                );
                found.push((x.t.0, 0, k, why));
            }
        }
        found
            .into_iter()
            .min_by(|a, b| a.0.total_cmp(&b.0).then((a.1, a.2).cmp(&(b.1, b.2))))
            .map(|(_, _, _, why)| why)
    }

    /// Career acts one action at a time, each judged against the ships as
    /// designed: none while one is pending, or while the ships are being
    /// designed.
    fn may_act(&self) -> Result<(), String> {
        if let Some(p) = &self.pending {
            return Err(format!("an action is pending: {}", p.what));
        }
        if !self.designed() {
            return Err("the ships are being designed; an action waits for that".into());
        }
        Ok(())
    }

    /// Makes `change` to the log as a pending action, `what` it is: the
    /// log before it kept to go back to, and what ran dry or short before
    /// it noted.
    fn act(&mut self, what: String, log: Log) {
        let before = std::mem::replace(&mut self.log, log);
        self.pending = Some(Pending {
            before: Some(before),
            found: self.found(),
            what,
            label: None,
        });
        self.refused = None;
    }

    /// Once the ships are designed: judges the action pending, going back
    /// to the log before one refused; then makes the next re-plan of a log
    /// being read back, and brings the clock to the log's now after the
    /// last. `true` if an action was made, to be designed and judged.
    fn settle(&mut self) -> bool {
        if !self.designed() {
            return false;
        }
        if let Some(p) = self.pending.take()
            && let Some(why) = self.judged(&p.found)
        {
            self.refused = Some(match &p.label {
                Some(label) => format!("{label}: {why}"),
                None => why,
            });
            // A log read back that is refused stops there.
            self.replay.clear();
            if let Some(before) = p.before {
                self.log = before;
                self.reshaped();
            }
        }
        if let Some((i, r)) = self.replay.pop_front() {
            self.now = r.at;
            let label = format!("replan {}", i + 1);
            match self.replan(r.ship, r.from, r.steps, r.here) {
                Ok(()) => {
                    if let Some(p) = &mut self.pending {
                        p.label = Some(label);
                    }
                    return true;
                }
                Err(why) => {
                    self.refused = Some(format!("{label}: {why}"));
                    self.replay.clear();
                }
            }
        }
        if let Some(t) = self.finally.take() {
            self.now = t;
        }
        false
    }

    /// After the log's ships changed: the plan as it stands, and each ship
    /// whose inputs changed designed again (from the cache where its steps
    /// did not change), and with it each ship that reads one (meets it,
    /// docks to it or starts docked to it: their keys are the vessel's)
    /// designed again; the others go on as they were.
    fn reshaped(&mut self) {
        let mut old: Vec<ShipDesign> = std::mem::take(&mut self.ships);
        let n = self.log.plan.ships.len();
        let mut kept: Vec<Option<ShipDesign>> = (0..n)
            .map(|k| {
                let inputs = self.inputs(k);
                let j = old.iter().position(|s| s.inputs == inputs)?;
                let mut kept = old.swap_remove(j);
                kept.name.clone_from(&self.log.plan.ships[k].name);
                Some(kept)
            })
            .collect();
        // Those designed afresh: whose inputs changed, and those that meet
        // one of them, through meetings.
        let mut fresh: HashSet<u64> = (0..n)
            .filter(|&k| kept[k].is_none())
            .map(|k| self.log.plan.ships[k].id)
            .collect();
        loop {
            let more: Vec<u64> = self
                .log
                .plan
                .ships
                .iter()
                .filter(|s| !fresh.contains(&s.id) && self.meets_any(s, &fresh))
                .map(|s| s.id)
                .collect();
            if more.is_empty() {
                break;
            }
            fresh.extend(more);
        }
        for (k, kept) in kept.iter_mut().enumerate() {
            let ship = match kept.take() {
                Some(ship) if !fresh.contains(&ship.id) => ship,
                Some(ship) => {
                    old.push(ship);
                    self.placeholder(k)
                }
                None => self.placeholder(k),
            };
            self.ships.push(ship);
        }
        self.plan = self.log.plan.clone();
        for k in 0..n {
            self.stand(k);
        }
        self.publish_all();
        for k in 0..n {
            if fresh.contains(&self.ships[k].id) {
                self.ships[k] = self.design(k);
                // Its re-plans refused so far, and its breaches.
                self.stand(k);
                self.publish(k);
            }
        }
        for s in old {
            s.live.store(false, Ordering::Relaxed);
            self.kept
                .push_front(s.used.lock().unwrap().iter().copied().collect());
        }
        self.kept.truncate(KEPT_DESIGNS);
        let keep: HashSet<Key> = self
            .ships
            .iter()
            .flat_map(|s| s.used.lock().unwrap().iter().copied().collect::<Vec<_>>())
            .chain(self.kept.iter().flatten().copied())
            .collect();
        self.cache.retain(|k| keep.contains(&k));
        self.rerun();
    }

    /// Whether `ship`'s plan (the log's, with its re-plans) reads any of
    /// the ships with ids `among`: meets or docks to one, or starts docked
    /// to one.
    fn meets_any(&self, ship: &Ship, among: &HashSet<u64>) -> bool {
        let mine: Vec<&Replan> = self
            .log
            .replans
            .iter()
            .filter(|r| r.ship == ship.id)
            .collect();
        let meets = |steps: &[Step]| {
            steps
                .iter()
                .any(|s| s.meeting().is_some_and(|m| among.contains(&m.ship)))
        };
        ship.start.meeting.is_some_and(|m| among.contains(&m.ship))
            || meets(&ship.steps)
            || mine.iter().any(|r| meets(&r.steps))
    }

    /// Ship `k` of the log before it is designed: nothing of it yet.
    fn placeholder(&self, k: usize) -> ShipDesign {
        let first = &self.log.plan.ships[k];
        ShipDesign {
            id: first.id,
            name: first.name.clone(),
            done: false,
            failure: None,
            refused: Vec::new(),
            breaches: Vec::new(),
            revision: revision(),
            steps: Vec::new(),
            waiting: None,
            inputs: self.inputs(k),
            used: Arc::default(),
            live: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Ship `k` as its design stands, to the fleet the walks read
    /// meetings from.
    fn publish(&self, k: usize) {
        let (d, ship) = (&self.ships[k], &self.plan.ships[k]);
        self.fleet.lock().unwrap().by_id.insert(
            d.id,
            Vessel {
                steps: ship.steps.clone(),
                designed: d.steps.clone(),
                done: d.done,
            },
        );
    }

    /// Every ship to the fleet, and no other.
    fn publish_all(&self) {
        self.fleet.lock().unwrap().by_id.clear();
        for k in 0..self.ships.len() {
            self.publish(k);
        }
    }

    /// Where the vessel `m` names is after the leg named, as the designs
    /// stand ([`Met`]); `None` while its design has not got there.
    pub fn met(&self, m: &Meeting) -> Option<Met> {
        self.fleet.lock().unwrap().met(m, &mut None)
    }

    /// Where vessel `host` is at `t`, as the designs stand ([`Host`]; M1b.md,
    /// step 4 plan, part 2): its stop then, or its next if it is in flight,
    /// and when it leaves it; through the vessel it is docked to. `None`
    /// while the designs do not reach `t`.
    pub fn at(&self, host: u64, t: Epoch) -> Option<Host> {
        self.fleet.lock().unwrap().at(host, t, &mut None)
    }

    /// Ship `k` of the plan as it stands: the log's, with its re-plans
    /// but those refused; and what its class does not allow of it.
    fn stand(&mut self, k: usize) {
        let ship = &self.log.plan.ships[k];
        let mine: Vec<&Replan> = self
            .log
            .replans
            .iter()
            .filter(|r| r.ship == ship.id)
            .collect();
        let ship = standing(ship, &mine, &self.ships[k].refused);
        let fitted = ships::read(&ship, &self.economy).expect("a game's ships read (`allowed`)");
        self.ships[k].breaches = fitted.map_or(Vec::new(), |f| {
            ships::breaches(&ship, &f, &self.economy, world().eph, &self.log.plan.ships)
        });
        self.plan.ships[k] = ship;
    }

    /// What ship `k` of the log is designed from, hashed: the ship (its
    /// name, class, load and manifests aside) and its re-plans (their
    /// `here` aside).
    fn inputs(&self, k: usize) -> Key {
        let ship = &self.log.plan.ships[k];
        let mut h = Fixed::default();
        h.u64(ship.id).debug(&ship.start).f64(ship.epoch.0);
        flight(&mut h, &ship.steps);
        for r in self.log.replans.iter().filter(|r| r.ship == ship.id) {
            h.str("replan").u64(r.ship).f64(r.at.0).u64(r.from as u64);
            flight(&mut h, &r.steps);
        }
        h.finish()
    }

    /// Ship `k` of the log: its design as far as the cache has it, and a
    /// walk in the background for the rest.
    fn design(&self, k: usize) -> ShipDesign {
        let first = &self.log.plan.ships[k];
        let replans: Vec<&Replan> = self
            .log
            .replans
            .iter()
            .filter(|r| r.ship == first.id)
            .collect();
        let mut ship = ShipDesign {
            id: first.id,
            name: first.name.clone(),
            done: false,
            failure: None,
            refused: Vec::new(),
            breaches: Vec::new(),
            revision: revision(),
            steps: Vec::new(),
            waiting: None,
            inputs: self.inputs(k),
            used: Arc::default(),
            live: Arc::new(AtomicBool::new(true)),
        };
        let cache = self.cache.clone();
        let (used, mut refused) = (ship.used.clone(), Vec::new());
        let ended = walk(
            world(),
            first,
            &replans,
            &mut |key, _| {
                used.lock().unwrap().insert(key);
                cache.get(key)
            },
            &Lookup {
                fleet: &self.fleet,
                waited: Cell::new(None),
            },
            &mut |key, designed| {
                ship.steps.push((key, designed.clone()));
            },
            &mut |i, why| refused.push((i, why)),
        );
        ship.refused = refused;
        match ended {
            Ended::Done(failure) => {
                ship.done = true;
                ship.failure = failure;
            }
            Ended::Waiting => {
                self.spawn(&ship, first.clone(), replans.into_iter().cloned().collect())
            }
        }
        ship
    }

    /// A walk of `first` with `replans` in the background, designing what
    /// the cache lacks while `ship` is live; it reports each key it takes,
    /// and each step of the design as it stands; stopped at a meeting of a
    /// vessel not designed that far, it says which.
    fn spawn(&self, ship: &ShipDesign, first: Ship, replans: Vec<Replan>) {
        let (live, used) = (ship.live.clone(), ship.used.clone());
        let (cache, sender) = (self.cache.clone(), self.sender.clone());
        let fleet = self.fleet.clone();
        self.pool.spawn(move || {
            let send = |e| {
                let _ = sender.send((live.clone(), e));
            };
            let lookup = Lookup {
                fleet: &fleet,
                waited: Cell::new(None),
            };
            let walked = catch_unwind(AssertUnwindSafe(|| {
                let replans: Vec<&Replan> = replans.iter().collect();
                let mut n = 0;
                walk(
                    world(),
                    &first,
                    &replans,
                    &mut |key, design| {
                        // Edited away, it stops before its next design.
                        if !live.load(Ordering::Relaxed) {
                            return None;
                        }
                        // Taken before it is kept, so that the game does
                        // not let it go in between.
                        used.lock().unwrap().insert(key);
                        Some(cache.get(key).unwrap_or_else(|| cache.put(key, design())))
                    },
                    &lookup,
                    &mut |key, designed| {
                        send(Event::Shown(n, key, designed.clone()));
                        n += 1;
                    },
                    &mut |i, why| send(Event::Refused(i, why)),
                )
            }));
            send(match walked {
                Ok(Ended::Waiting) => match lookup.waited.get() {
                    Some((vessel, seen)) => Event::Waiting { vessel, seen },
                    None => Event::Ended(Ended::Waiting),
                },
                Ok(ended) => Event::Ended(ended),
                Err(panic) => {
                    let why = panic
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| panic.downcast_ref::<&str>().copied())
                        .unwrap_or("no message");
                    Event::Panicked(format!("designing it panicked: {why}"))
                }
            });
        });
    }

    /// Takes what the walks have sent, and judges a pending action once
    /// the ships are designed; `true` if any ship's design changed.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((live, event)) = self.receiver.try_recv() {
            any |= self.take(&live, event);
        }
        if any {
            self.rerun();
        }
        while self.settle() {}
        any
    }

    /// Waits until every ship is designed and every action judged.
    pub fn wait(&mut self) {
        loop {
            let mut any = false;
            while !self.designed() {
                match self.receiver.recv() {
                    Ok((live, event)) => any |= self.take(&live, event),
                    Err(_) => return,
                }
            }
            if any {
                self.rerun();
            }
            if !self.settle() && self.designed() {
                return;
            }
        }
    }

    /// An event from the walk whose flag is `live`, if it is a ship's
    /// current one; `true` if the ship's design changed. A change to a
    /// ship's design wakes the walks waiting at a meeting of it.
    fn take(&mut self, live: &Arc<AtomicBool>, event: Event) -> bool {
        let Some(k) = self.ships.iter().position(|s| Arc::ptr_eq(&s.live, live)) else {
            return false;
        };
        let ship = &mut self.ships[k];
        let changed = match event {
            // Steps the ship has (from the cache, when it was edited) come
            // again first.
            Event::Shown(n, key, designed) => {
                if n != ship.steps.len() {
                    return false;
                }
                ship.steps.push((key, designed));
                ship.revision = revision();
                true
            }
            Event::Ended(Ended::Done(failure)) => {
                ship.done = true;
                ship.failure = failure;
                ship.revision = revision();
                true
            }
            Event::Refused(i, why) => {
                if !ship.refused.iter().any(|(j, _)| *j == i) {
                    ship.refused.push((i, why));
                    self.stand(k);
                    self.publish(k);
                }
                return true;
            }
            Event::Waiting { vessel, seen } => {
                // The vessel's design may have moved on since the walk
                // looked at it.
                match self.fleet.lock().unwrap().seen(vessel) == seen {
                    true => ship.waiting = Some(vessel),
                    false => self.respawn(k),
                }
                return false;
            }
            Event::Ended(Ended::Waiting) => return false,
            Event::Panicked(why) => {
                ship.done = true;
                ship.failure = Some(why);
                ship.revision = revision();
                true
            }
        };
        self.publish(k);
        let id = self.ships[k].id;
        let woken: Vec<usize> = (0..self.ships.len())
            .filter(|&j| self.ships[j].waiting == Some(id))
            .collect();
        for j in woken {
            self.ships[j].waiting = None;
            self.respawn(j);
        }
        changed
    }

    /// Ship `k`'s walk again, from the cache: after the vessel it waited
    /// on was designed further.
    fn respawn(&self, k: usize) {
        let first = &self.log.plan.ships[k];
        let replans: Vec<Replan> = self
            .log
            .replans
            .iter()
            .filter(|r| r.ship == first.id)
            .cloned()
            .collect();
        self.spawn(&self.ships[k], first.clone(), replans);
    }

    pub fn done(&self) -> bool {
        self.designed() && self.pending.is_none() && self.replay.is_empty()
    }

    /// Every ship designed to its end.
    fn designed(&self) -> bool {
        self.ships.iter().all(|s| s.done)
    }

    /// The status line's words: how many ships are designed, and failed.
    pub fn progress(&self) -> String {
        let done = self.ships.iter().filter(|s| s.done).count();
        let failed = self.ships.iter().filter(|s| s.failure.is_some()).count();
        let mut s = if done == self.ships.len() {
            format!("{}: {} ships designed", self.plan.name, done)
        } else {
            format!(
                "{}: designing, {done} of {} ships",
                self.plan.name,
                self.ships.len()
            )
        };
        if failed > 0 {
            s.push_str(&format!(", {failed} failed"));
        }
        s
    }

    /// The sandbox's edits (M1b.md, Modes): any, at any time, each kept
    /// for undo. Career's are [`Game::replan`] and [`Game::add_ship`].
    fn sandbox(&self, what: &str) -> Result<(), String> {
        match self.mode {
            Mode::Sandbox => Ok(()),
            Mode::Career => Err(format!("{what} is the sandbox's; career re-plans from now")),
        }
    }

    /// The log with `change` made, if the game allows it (`allowed`) and
    /// it reads back as a game (`Log::check`): a re-plan's first step is
    /// counted in the log's order, its re-plans passed over among them
    /// (found in review: one from past the steps the log has was taken,
    /// and the log did not read back).
    fn changed(&self, change: impl FnOnce(&mut Log)) -> Result<Log, String> {
        let mut log = self.log();
        change(&mut log);
        log.check()?;
        allowed(&log, self.mode, &self.economy)?;
        Ok(log)
    }

    /// Replaces ship `id` with `ship` (its steps as they stand). In the
    /// sandbox, its re-plans are folded into it: its steps are those
    /// edited, from its start. A new name, class, load or manifest alone
    /// keeps them, and designs nothing.
    pub fn edit(&mut self, id: u64, mut ship: Ship) -> Result<(), String> {
        self.sandbox("Editing a ship")?;
        let k = self.index(id).ok_or(format!("no ship has id {id}"))?;
        ship.id = id;
        let mut refit = self.plan.ships[k].clone();
        refit.name.clone_from(&ship.name);
        refit.class.clone_from(&ship.class);
        refit.propellant_t = ship.propellant_t;
        refit.cargo.clone_from(&ship.cargo);
        if refit.steps.len() == ship.steps.len() {
            for (kept, edited) in refit.steps.iter_mut().zip(&ship.steps) {
                if let (Some(kept), Some(edited)) = (kept.manifest_mut(), edited.manifest()) {
                    kept.clone_from(edited);
                }
            }
        }
        let refit = format!("{refit:?}") == format!("{ship:?}");
        let refused = self.ships[k].refused.clone();
        let log = self.changed(|log| match refit {
            true => {
                remanifest(log, k, &refused, &ship.steps);
                let kept = &mut log.plan.ships[k];
                kept.name = ship.name;
                kept.class = ship.class;
                kept.propellant_t = ship.propellant_t;
                kept.cargo = ship.cargo;
            }
            false => {
                log.plan.ships[k] = ship;
                log.replans.retain(|r| r.ship != id);
            }
        })?;
        self.undo.push(std::mem::replace(&mut self.log, log));
        self.reshaped();
        Ok(())
    }

    /// Renames ship `id`: nothing is designed again, in either mode.
    pub fn rename(&mut self, id: u64, name: &str) {
        let Some(k) = self.index(id) else {
            return;
        };
        if self.mode == Mode::Sandbox {
            self.undo.push(self.log.clone());
        }
        self.log.plan.ships[k].name = name.to_string();
        self.plan.ships[k].name = name.to_string();
        self.ships[k].name = name.to_string();
    }

    /// Adds `ship`, last, with an id of its own; in career, from now on,
    /// at LEO and within its class.
    pub fn add_ship(&mut self, mut ship: Ship) -> Result<u64, String> {
        if self.mode == Mode::Career && ship.epoch.0 < self.now.0 {
            return Err(format!(
                "a ship starts from now ({}), not {}",
                self.now.stamp(),
                ship.epoch.stamp()
            ));
        }
        ship.id = self.log.plan.next_id();
        let id = ship.id;
        let name = ship.name.clone();
        let log = self.changed(|log| log.plan.ships.push(ship))?;
        match self.mode {
            Mode::Sandbox => {
                let before = std::mem::replace(&mut self.log, log);
                self.undo.push(before);
            }
            Mode::Career => {
                self.may_act()?;
                self.act(format!("ship {name:?} added"), log);
            }
        }
        self.reshaped();
        Ok(id)
    }

    pub fn remove_ship(&mut self, id: u64) -> Result<(), String> {
        self.sandbox("Removing a ship")?;
        let k = self.index(id).ok_or(format!("no ship has id {id}"))?;
        self.undo.push(self.log.clone());
        self.log.plan.ships.remove(k);
        self.log.replans.retain(|r| r.ship != id);
        self.reshaped();
        Ok(())
    }

    /// Back to the log before the last edit: designs from the cache.
    pub fn undo(&mut self) -> bool {
        let Some(before) = self.undo.pop() else {
            return false;
        };
        self.log = before;
        self.reshaped();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Career: ship `id`'s steps from `from` on replaced by `steps`, from
    /// now (to the minute), and the manifest of the stop it is at set to
    /// `here`, if given (`Replan::here`). It takes effect where the ship
    /// is: now, if it is staying where the step that holds `from` leaves
    /// from; at the end of a leg in flight; or later. Refused if its steps
    /// break the ship's class, or if it changes a step whose leg has
    /// begun.
    pub fn replan(
        &mut self,
        id: u64,
        from: usize,
        steps: Vec<Step>,
        here: Option<Manifest>,
    ) -> Result<(), String> {
        if self.mode != Mode::Career {
            return Err("re-plans are career's; the sandbox edits".into());
        }
        let k = self.index(id).ok_or(format!("no ship has id {id}"))?;
        let standing = &self.plan.ships[k];
        if from > standing.steps.len() {
            return Err(format!(
                "from step {from}, but the ship has {}",
                standing.steps.len()
            ));
        }
        if here.is_some() && !standing.steps[..from].iter().any(Step::is_stop) {
            return Err(here_without_a_stop(from));
        }
        // A leg another ship's plan meets it after is kept (M1b.md, step 3
        // plan, part 3).
        for s in &self.plan.ships {
            for (i, step) in s.steps.iter().enumerate() {
                if let Some(m) = step.meeting()
                    && m.ship == id
                    && let Some(leg) = m.leg
                    && leg >= from
                {
                    return Err(format!(
                        "ship {:?} meets it after leg {leg} (step {i} of its plan), which the \
                         re-plan replaces",
                        s.name
                    ));
                }
            }
        }
        let at = minute(self.now);
        let replan = Replan {
            ship: id,
            at,
            from,
            steps,
            here,
        };
        let log = self.changed(|log| log.replans.push(replan.clone()))?;
        self.may_act()?;
        // Walked with what the cache has, the re-plan is decided where the
        // ship is (in career, its designs to now are done). One whose step
        // is not designed yet takes effect after now; if it then cannot,
        // it is passed over (`ShipDesign::refused`).
        let mine: Vec<&Replan> = self
            .log
            .replans
            .iter()
            .filter(|r| r.ship == id)
            .chain(std::iter::once(&replan))
            .collect();
        let cache = self.cache.clone();
        let mut refused = None;
        walk(
            world(),
            &self.log.plan.ships[k],
            &mine,
            &mut |key, _| cache.get(key),
            &Lookup {
                fleet: &self.fleet,
                waited: Cell::new(None),
            },
            &mut |_, _| {},
            &mut |i, why| {
                if i == mine.len() - 1 {
                    refused = Some(why);
                }
            },
        );
        if let Some(why) = refused {
            return Err(why);
        }
        self.now = at;
        let name = &self.plan.ships[k].name;
        self.act(format!("ship {name:?} re-planned from step {from}"), log);
        self.reshaped();
        Ok(())
    }

    /// Career: moves now on to `t`, as far as designs allow: not past a
    /// departure whose design is not done, and not at all while an action
    /// is pending (nothing of one refused happens). The time reached.
    pub fn advance(&mut self, t: Epoch) -> Epoch {
        if self.pending.is_some() || !self.replay.is_empty() {
            return self.now;
        }
        let reachable = self
            .ships
            .iter()
            .zip(&self.plan.ships)
            .filter(|(s, _)| !s.done)
            .map(|(s, ship)| s.items().last().map_or(ship.epoch.0, |i| i.end.0))
            .fold(f64::INFINITY, f64::min);
        self.now = Epoch(self.now.0.max(t.0.min(reachable)));
        self.now
    }

    /// Where an edit at step `step` of ship `s` would design it again
    /// from: the first step whose design reads it (`mission::reads`);
    /// `None` if the ship is not designed that far. What a sweep of a
    /// leg's durations designs from.
    pub fn resume_point(&self, s: usize, step: usize) -> Option<Checkpoint> {
        let checkpoints: Vec<Checkpoint> = self.ships[s].checkpoints().cloned().collect();
        resume_point(&self.plan.ships[s], &checkpoints, step).cloned()
    }

    /// Whether `check` holds, saying what the game has either way.
    pub fn check(&self, check: &Check) -> Result<String, String> {
        let ship = check.ship();
        let s = self.index(ship).ok_or(format!("no ship has id {ship}"))?;
        let (d, state) = (&self.ships[s], &self.state.ships[s]);
        let items: Vec<&Item> = d.items().collect();
        let pending = |what: String| Err(format!("{what}: the state is pending"));
        match check {
            Check::At { at, place, .. } => {
                let on = format!("{} on {}", d.name, at.stamp());
                if !self.state.known(*at) {
                    return pending(on);
                }
                let here = format!("at {place}");
                let (line, held) = match state.dry_by(*at) {
                    Some(dry) => match stranded(&self.plan.ships[s], &items, dry) {
                        Some(label) => (
                            format!("{on}: {label}, stranded: ran dry {}", when(dry.t)),
                            label == here,
                        ),
                        None => (format!("{on}: {}", ran_dry(&items, dry)), false),
                    },
                    None => {
                        let item = items.iter().rfind(|i| i.start.0 <= at.0 && at.0 <= i.end.0);
                        (
                            format!(
                                "{on}: {}",
                                item.map_or("not designed there", |i| i.label.as_str())
                            ),
                            item.is_some_and(|i| i.kind == ItemKind::Stay && i.label == here),
                        )
                    }
                };
                match held {
                    true => Ok(line),
                    false => Err(format!("{line}, not {here}")),
                }
            }
            Check::Dv { step, dv, .. } => {
                let Some(leg) = items
                    .iter()
                    .find(|i| i.step == *step && i.kind == ItemKind::Leg)
                else {
                    return Err(format!("{}, step {step}: no leg", d.name));
                };
                let m = leg.dv * 1e3;
                let line = format!("{}, step {step}: {m:.1} m/s", d.name);
                match (dv[0]..=dv[1]).contains(&m) {
                    true => Ok(line),
                    false => Err(format!("{line}, not {:.1}–{:.1}", dv[0], dv[1])),
                }
            }
            Check::Propellant { at, t, .. } => {
                let on = format!("{} on {}", d.name, at.stamp());
                if !self.state.known(*at) {
                    return pending(on);
                }
                let Some(load) = state.load(*at) else {
                    return Err(format!("{on}: no load (no class, or not yet started)"));
                };
                let line = format!("{on}: {:.3} t of propellant", load.propellant());
                match (t[0]..=t[1]).contains(&load.propellant()) {
                    true => Ok(line),
                    false => Err(format!("{line}, not {:.3}–{:.3}", t[0], t[1])),
                }
            }
            Check::Dry { step, .. } => {
                if !self.state.done {
                    return pending(d.name.clone());
                }
                let ship = &self.plan.ships[s];
                let found = state.dry.map(|dry| dry_step(ship, &items, &dry));
                let line = match (&state.dry, found) {
                    (Some(dry), Some(n)) => {
                        format!("{} {}, at step {n}", d.name, ran_dry(&items, dry))
                    }
                    _ => format!("{} did not run dry", d.name),
                };
                match step {
                    _ if found == *step => Ok(line),
                    Some(n) => Err(format!("{line}; expected: ran dry at step {n}")),
                    None => Err(format!("{line}; expected: did not run dry")),
                }
            }
            Check::Cargo { at, cargo, .. } => {
                let on = format!("{} on {}", d.name, at.stamp());
                if !self.state.known(*at) {
                    return pending(on);
                }
                let Some(load) = state.load(*at) else {
                    return Err(format!("{on}: no load (no class, or not yet started)"));
                };
                let held: Vec<String> = cargo
                    .iter()
                    .map(|(c, _)| format!("{} {:.3} t", c.name(), load.cargo[*c as usize]))
                    .collect();
                let line = format!("{on}: {}", held.join(", "));
                let out: Vec<String> = cargo
                    .iter()
                    .filter(|(c, t)| !(t[0]..=t[1]).contains(&load.cargo[*c as usize]))
                    .map(|(c, t)| format!("{} not {:.3}–{:.3}", c.name(), t[0], t[1]))
                    .collect();
                match out.is_empty() {
                    true => Ok(line),
                    false => Err(format!("{line}, {}", out.join(", "))),
                }
            }
            Check::Transfer {
                step,
                kind,
                commodity,
                short,
                ..
            } => {
                let what = match kind {
                    Kind::Give => "give",
                    Kind::Take => "take",
                };
                let Some(x) = state
                    .transfers
                    .iter()
                    .find(|x| x.step == *step && x.kind == *kind && x.commodity == *commodity)
                else {
                    let none =
                        format!("{}, step {step}: no {what} of {}", d.name, commodity.name());
                    return match self.state.done {
                        true => Err(none),
                        false => pending(none),
                    };
                };
                let line = format!(
                    "{}, step {step}: {}",
                    d.name,
                    report::transfer(self.plan(), x)
                );
                match x.short.is_some() == *short {
                    true => Ok(line),
                    false if *short => Err(format!("{line}; expected: short")),
                    false => Err(format!("{line}; expected: in full")),
                }
            }
        }
    }

    /// The state at the start of each of `days` days from `from`, hashed
    /// (M1b.md, step 1 plan, part 5; step 2 plan, part 3): each ship's
    /// place, its position (by bits) and its Δv so far, its tanks and hold
    /// (by bits) and whether it ran dry; a ship not designed that far, or
    /// beyond where the loop ran, as pending.
    pub fn day_hashes(&self, from: Epoch, days: usize) -> Vec<u64> {
        (0..days)
            .map(|d| {
                let t = from.plus_days(d as f64);
                let mut h = Fixed::default();
                assert_eq!(self.ships.len(), self.state.ships.len(), "the loop ran");
                for (ship, s) in self.ships.iter().zip(&self.state.ships) {
                    h.u64(ship.id);
                    state(&mut h, ship, t);
                    tanks(&mut h, &self.state, s, t);
                }
                h.finish()
            })
            .collect()
    }
}

/// Where a ship ran dry and how short: "ran dry 2045-03-09 14:40 at
/// moon-south-pole → llo: 1317.2 m/s short".
fn ran_dry(items: &[&Item], dry: &Dry) -> String {
    format!(
        "ran dry {} {}: {} short",
        when(dry.t),
        dry.place(items[dry.item]),
        report::short(dry.short)
    )
}

/// Where a ship that ran dry is stranded, as a stay there is labelled ("at
/// llo"): the place its failed burn would have left, if that was its
/// item's first (a departure, a descent, an ascent). A ship that ran dry
/// later in a leg is in flight, and one that ran dry keeping station
/// drifts from its station.
fn stranded(ship: &Ship, items: &[&Item], dry: &Dry) -> Option<String> {
    let first = items[dry.item]
        .markers
        .iter()
        .position(|m| !matches!(m.kind, MarkerKind::Jump { .. }));
    if dry.marker.is_none() || dry.marker != first {
        return None;
    }
    let Some(before) = items[..dry.item].last() else {
        return Some(format!("at {}", ship.start.label));
    };
    // A leg may leave up to a millisecond before the ship arrives
    // (`Planner`'s slack); a burn later than its item's start is in flight.
    if (before.end.0 - dry.t.0).abs() * SECONDS_PER_DAY > 2e-3 {
        return None;
    }
    if before.kind == ItemKind::Stay {
        return Some(before.label.clone());
    }
    if let ItemKind::Docked(host) = before.kind {
        return Some(format!("docked to ship {host}"));
    }
    let to = ship
        .steps
        .get(before.step)
        .and_then(Step::to)
        .expect("an item's step is a leg's or a dock's");
    Some(format!("at {}", to.label))
}

/// The step a ship ran dry at, as its plan counts them: its item's, but
/// for upkeep paid during a wait, the wait's. A stay is the step of the leg
/// that leaves it, and the waits before that leg are spent in it from its
/// start.
fn dry_step(ship: &Ship, items: &[&Item], dry: &Dry) -> usize {
    let item = items[dry.item];
    if dry.marker.is_some() {
        return item.step;
    }
    let steps = &ship.steps[..item.step.min(ship.steps.len())];
    let first = steps.iter().rposition(Step::is_stop).map_or(0, |k| k + 1);
    let mut end = item.start.0;
    for (k, step) in steps.iter().enumerate().skip(first) {
        if let Step::Wait(days) = step {
            end += days;
            if dry.t.0 < end {
                return k;
            }
        }
    }
    item.step
}

/// Ship's tanks and hold at `t` as the loop has them, hashed into `h`.
fn tanks(h: &mut Fixed, state: &State, s: &ShipState, t: Epoch) {
    if !state.known(t) {
        h.str("load pending");
        return;
    }
    match s.load(t) {
        Some(load) => {
            h.f64(load.lox).f64(load.fuel);
            for x in load.cargo {
                h.f64(x);
            }
        }
        None => {
            h.str("no load");
        }
    }
    if let Some(dry) = s.dry_by(t) {
        h.str("dry").f64(dry.short);
    }
    for x in s.transfers.iter().filter(|x| x.t.0 <= t.0) {
        h.str("transfer")
            .u64(x.step as u64)
            .debug(&x.kind)
            .debug(&x.commodity)
            .f64(x.asked)
            .f64(x.got)
            .debug(&x.short)
            .debug(&x.cost_musd);
    }
    for p in s.purchases.iter().filter(|p| p.t.0 <= t.0) {
        h.str("bought").debug(&p.what).f64(p.tonnes).f64(p.musd);
    }
}

/// Ship's state at `t`, hashed into `h`.
fn state(h: &mut Fixed, ship: &ShipDesign, t: Epoch) {
    let Some(item) = ship.items().rfind(|i| i.start.0 <= t.0) else {
        h.str(if ship.done { "not yet" } else { "pending" });
        return;
    };
    if t.0 >= item.end.0 && !ship.done {
        h.str("pending");
        return;
    }
    h.str(&item.label).debug(&item.kind);
    if let Some(track) = &item.track
        && let Some(s) = track.at((t.0 - track.epoch.0) * SECONDS_PER_DAY)
    {
        for x in s {
            h.f64(x);
        }
    }
    let dv: f64 = ship
        .items()
        .flat_map(|i| &i.markers)
        .filter(|m| m.t.0 <= t.0)
        .map(|m| m.dv)
        .sum();
    h.f64(dv);
}
