//! A game (M1b.md, step 1 plan, parts 4 and 5): its log, each ship's
//! design as it stands, and its mode's policies. Ships are designed in
//! the background, on the host's pool, by walks (`designs::walk`) that
//! take what the cache has and design the rest; an edit or an undo walks
//! again, and finds what it did not change in the cache.

use crate::designs::{Cache, Designed, Ended, Key, walk};
use crate::hash::Fixed;
use crate::log::{Log, Mode, Replan, minute};
use orbit::mission::{Checkpoint, Item, Plan, Ship, Step, World, resume_point};
use orbit::places::Places;
use orbit::time::{Epoch, SECONDS_PER_DAY};
use std::collections::{HashSet, VecDeque};
use std::hash::Hasher;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, LazyLock};

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
    /// Another whenever its items change, unique in the process: what
    /// views of them are built for.
    pub revision: u64,
    /// Its design's steps as they stand, by key.
    steps: Vec<(Key, Arc<Designed>)>,
    /// A hash of what it is designed from: the log's ship (its name
    /// aside) and its re-plans.
    inputs: Key,
    /// Every key its walk took, shown or not (a re-plan's decision reads
    /// the step it replaced).
    used: Vec<Key>,
    live: Arc<AtomicBool>,
}

impl ShipDesign {
    /// What the ship does, so far as it is designed.
    pub fn items(&self) -> impl DoubleEndedIterator<Item = &Item> {
        self.steps.iter().flat_map(|(_, d)| match &**d {
            Designed::Step { items, .. } => items.as_slice(),
            _ => &[],
        })
    }

    /// A hash of what it is designed from, which changes only when the
    /// ship (its name aside) or its re-plans do.
    pub fn inputs(&self) -> Key {
        self.inputs
    }

    /// Where the ship is before each step designed: its start's and each
    /// step's checkpoint after it.
    fn checkpoints(&self) -> impl Iterator<Item = &Checkpoint> {
        self.steps.iter().filter_map(|(_, d)| match &**d {
            Designed::Start(Ok(c)) | Designed::Step { after: c, .. } => Some(c),
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
    /// A key it took, part of the design or not (a re-plan's decision
    /// reads the step it replaced).
    Used(Key),
    /// The `n`th step of the design as it stands.
    Shown(usize, Key, Arc<Designed>),
    Ended(Ended),
    Panicked(String),
}

pub struct Game {
    log: Log,
    /// The plan as it stands: the log's ships with their re-plans.
    plan: Plan,
    mode: Mode,
    /// Career: the time the game has reached.
    now: Epoch,
    path: Option<PathBuf>,
    undo: Vec<Log>,
    ships: Vec<ShipDesign>,
    cache: Cache,
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

/// `ship` with `replans` (its own, in order) applied in turn: its steps as
/// they stand.
fn standing(ship: &Ship, replans: &[&Replan]) -> Ship {
    let mut ship = ship.clone();
    for r in replans {
        ship.steps.truncate(r.from);
        ship.steps.extend(r.steps.iter().cloned());
    }
    ship
}

impl Game {
    /// `log` played in its own mode, its ships designed on `pool`.
    pub fn new(log: Log, pool: Arc<rayon::ThreadPool>) -> Game {
        let mode = log.setup.mode;
        Game::in_mode(log, mode, pool)
    }

    /// `log` played in `mode`: a career game's log opens in the sandbox.
    pub fn in_mode(log: Log, mode: Mode, pool: Arc<rayon::ThreadPool>) -> Game {
        let (sender, receiver) = channel();
        let now = log
            .setup
            .now
            .or(log.setup.start)
            .unwrap_or(Epoch(f64::NEG_INFINITY));
        let mut game = Game {
            plan: log.plan.clone(),
            log,
            mode,
            now,
            path: None,
            undo: Vec::new(),
            ships: Vec::new(),
            cache: Cache::default(),
            kept: VecDeque::new(),
            pool,
            sender,
            receiver,
        };
        game.reshaped();
        game
    }

    /// Reads the log at `path` and starts designing it.
    pub fn open(path: &Path, pool: Arc<rayon::ThreadPool>) -> Result<Game, String> {
        let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let log = Log::parse(&src, world().eph, Places::standard())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mut game = Game::new(log, pool);
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
        });
        let mut game = Game::new(log, pool);
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

    /// After the log's ships changed: the plan as it stands, and each ship
    /// whose inputs changed designed again (from the cache where its steps
    /// did not change); the others go on as they were.
    fn reshaped(&mut self) {
        let mut old: Vec<ShipDesign> = std::mem::take(&mut self.ships);
        self.plan = self.log.plan.clone();
        for ship in &mut self.plan.ships {
            let mine: Vec<&Replan> = self
                .log
                .replans
                .iter()
                .filter(|r| r.ship == ship.id)
                .collect();
            *ship = standing(ship, &mine);
        }
        for k in 0..self.log.plan.ships.len() {
            let inputs = self.inputs(k);
            let ship = match old.iter().position(|s| s.inputs == inputs) {
                Some(j) => old.swap_remove(j),
                None => self.design(k),
            };
            self.ships.push(ship);
        }
        for s in old {
            s.live.store(false, Ordering::Relaxed);
            self.kept.push_front(s.used);
        }
        self.kept.truncate(KEPT_DESIGNS);
        let keep: HashSet<Key> = self
            .ships
            .iter()
            .flat_map(|s| s.used.iter())
            .chain(self.kept.iter().flatten())
            .copied()
            .collect();
        self.cache.retain(|k| keep.contains(&k));
    }

    /// What ship `k` of the log is designed from, hashed: the ship (its
    /// name aside) and its re-plans.
    fn inputs(&self, k: usize) -> Key {
        let ship = &self.log.plan.ships[k];
        let mut h = Fixed::default();
        h.u64(ship.id)
            .debug(&ship.start)
            .f64(ship.epoch.0)
            .debug(&ship.steps);
        for r in self.log.replans.iter().filter(|r| r.ship == ship.id) {
            h.debug(r);
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
            revision: revision(),
            steps: Vec::new(),
            inputs: self.inputs(k),
            used: Vec::new(),
            live: Arc::new(AtomicBool::new(true)),
        };
        let cache = self.cache.clone();
        let mut used = Vec::new();
        let ended = walk(
            world(),
            first,
            &replans,
            &mut |key, _| {
                used.push(key);
                cache.get(key)
            },
            &mut |key, designed| {
                ship.steps.push((key, designed.clone()));
            },
        );
        ship.used = used;
        match ended {
            Ended::Done(failure) => {
                ship.done = true;
                ship.failure = failure;
            }
            Ended::Refused(why) => {
                ship.done = true;
                ship.failure = Some(why);
            }
            Ended::Waiting => {
                self.spawn(&ship, first.clone(), replans.into_iter().cloned().collect())
            }
        }
        ship
    }

    /// A walk of `first` with `replans` in the background, designing what
    /// the cache lacks while `ship` is live; it reports each key it takes,
    /// and each step of the design as it stands.
    fn spawn(&self, ship: &ShipDesign, first: Ship, replans: Vec<Replan>) {
        let live = ship.live.clone();
        let (cache, sender) = (self.cache.clone(), self.sender.clone());
        self.pool.spawn(move || {
            let send = |e| {
                let _ = sender.send((live.clone(), e));
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
                        let designed = cache.get(key).unwrap_or_else(|| cache.put(key, design()));
                        send(Event::Used(key));
                        Some(designed)
                    },
                    &mut |key, designed| {
                        send(Event::Shown(n, key, designed.clone()));
                        n += 1;
                    },
                )
            }));
            send(match walked {
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

    /// Takes what the walks have sent; `true` if any ship's design changed.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((live, event)) = self.receiver.try_recv() {
            any |= self.take(&live, event);
        }
        any
    }

    /// Waits until every ship is designed.
    pub fn wait(&mut self) {
        while !self.done() {
            match self.receiver.recv() {
                Ok((live, event)) => {
                    self.take(&live, event);
                }
                Err(_) => return,
            }
        }
    }

    /// An event from the walk whose flag is `live`, if it is a ship's
    /// current one; `true` if the ship's design changed.
    fn take(&mut self, live: &Arc<AtomicBool>, event: Event) -> bool {
        let Some(ship) = self.ships.iter_mut().find(|s| Arc::ptr_eq(&s.live, live)) else {
            return false;
        };
        match event {
            Event::Used(key) => {
                if !ship.used.contains(&key) {
                    ship.used.push(key);
                }
                false
            }
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
            Event::Ended(Ended::Refused(why)) => {
                ship.done = true;
                ship.failure = Some(why);
                ship.revision = revision();
                true
            }
            Event::Ended(Ended::Waiting) => false,
            Event::Panicked(why) => {
                ship.done = true;
                ship.failure = Some(why);
                ship.revision = revision();
                true
            }
        }
    }

    pub fn done(&self) -> bool {
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

    /// Replaces ship `id` with `ship` (its steps as they stand). In the
    /// sandbox, its re-plans are folded into it: its steps are those
    /// edited, from its start.
    pub fn edit(&mut self, id: u64, mut ship: Ship) -> Result<(), String> {
        self.sandbox("Editing a ship")?;
        let k = self.index(id).ok_or(format!("no ship has id {id}"))?;
        self.undo.push(self.log.clone());
        ship.id = id;
        self.log.plan.ships[k] = ship;
        self.log.replans.retain(|r| r.ship != id);
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

    /// Adds `ship`, last, with an id of its own; in career, from now on.
    pub fn add_ship(&mut self, mut ship: Ship) -> Result<u64, String> {
        if self.mode == Mode::Career && ship.epoch.0 < self.now.0 {
            return Err(format!(
                "a ship starts from now ({}), not {}",
                self.now.stamp(),
                ship.epoch.stamp()
            ));
        }
        if self.mode == Mode::Sandbox {
            self.undo.push(self.log.clone());
        }
        ship.id = self.log.plan.next_id();
        let id = ship.id;
        self.log.plan.ships.push(ship);
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
    /// now (to the minute). It takes effect where the ship is: now, if it
    /// is staying where the step that holds `from` leaves from; at the
    /// end of a leg in flight; or later. Refused if it changes a step
    /// whose leg has begun, or needs a longer station visit than was
    /// built (until step 3).
    pub fn replan(&mut self, id: u64, from: usize, steps: Vec<Step>) -> Result<(), String> {
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
        let at = minute(self.now);
        let replan = Replan {
            ship: id,
            at,
            from,
            steps,
        };
        // Walked with what the cache has, the re-plan is decided where the
        // ship is (in career, its designs to now are done). One whose step
        // is not designed yet takes effect after now.
        let mine: Vec<&Replan> = self
            .log
            .replans
            .iter()
            .filter(|r| r.ship == id)
            .chain(std::iter::once(&replan))
            .collect();
        let cache = self.cache.clone();
        let ended = walk(
            world(),
            &self.log.plan.ships[k],
            &mine,
            &mut |key, _| cache.get(key),
            &mut |_, _| {},
        );
        if let Ended::Refused(why) = ended {
            return Err(why);
        }
        self.now = at;
        self.log.replans.push(replan);
        self.reshaped();
        Ok(())
    }

    /// Career: moves now on to `t`, as far as designs allow: not past a
    /// departure whose design is not done. The time reached.
    pub fn advance(&mut self, t: Epoch) -> Epoch {
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
    /// from: the first step whose design reads it (`mission::reads`), or
    /// the start (`Some(None)`); `None` if the ship is not designed that
    /// far. What a sweep of a leg's durations designs from.
    pub fn resume_point(&self, s: usize, step: usize) -> Option<Option<Checkpoint>> {
        let checkpoints: Vec<Checkpoint> = self.ships[s].checkpoints().cloned().collect();
        resume_point(&self.plan.ships[s], &checkpoints, step).map(|c| c.cloned())
    }

    /// The state at the start of each of `days` days from `from`, hashed
    /// (M1b.md, step 1 plan, part 5): each ship's place, its position (by
    /// bits) and its Δv so far; a ship not designed that far, as pending.
    pub fn day_hashes(&self, from: Epoch, days: usize) -> Vec<u64> {
        (0..days)
            .map(|d| {
                let t = from.plus_days(d as f64);
                let mut h = Fixed::default();
                for ship in &self.ships {
                    h.u64(ship.id);
                    state(&mut h, ship, t);
                }
                h.finish()
            })
            .collect()
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
