//! A game (M1b.md, step 1 plan, parts 4 and 5): its log, its economy as
//! drawn, each ship's design as it stands, the state the loop reaches over
//! them (`state`), and its mode's policies. Ships are designed in the
//! background, on the host's pool, by walks (`designs::walk`) that take
//! what the cache has and design the rest; an edit or an undo walks
//! again, and finds what it did not change in the cache.

use crate::designs::{Cache, Designed, Ended, Key, apply, walk};
use crate::hash::Fixed;
use crate::log::{Check, Log, Mode, Replan, minute};
use crate::params::{Draw, Economy, EconomyFile};
use crate::report;
use crate::ships::{self, Breach};
use crate::state::{self, Course, Dry, ShipState, State};
use orbit::mission::{
    Checkpoint, Item, ItemKind, MarkerKind, Plan, Ship, Step, World, resume_point, when,
};
use orbit::places::Places;
use orbit::routes::RouteGraph;
use orbit::time::{Epoch, SECONDS_PER_DAY};
use std::collections::{HashSet, VecDeque};
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
            Designed::Step { items, .. } => items.as_slice(),
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
    /// The `n`th step of the design as it stands.
    Shown(usize, Key, Arc<Designed>),
    /// A re-plan that could not take effect: its place among the ship's
    /// re-plans, and why.
    Refused(usize, String),
    Ended(Ended),
    Panicked(String),
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
        let Some(fitted) = fitted else {
            continue;
        };
        let first = |s: &Ship| {
            ships::breaches(s, &fitted, economy, world().eph)
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
            // its own, numbered as its legs are (`[[replan.leg]]`).
            if let Some(b) = first(&replanned) {
                let leg = b.step.map_or(0, |s| s.saturating_sub(r.from));
                return Err(format!("replan {}: leg {}: {}", i + 1, leg + 1, b.why));
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
        let now = log
            .setup
            .now
            .or(log.setup.start)
            .or(first)
            .unwrap_or(Epoch::from_ymd(2040, 1, 1));
        log.setup.mode = mode;
        log.setup.now = (mode == Mode::Career).then_some(now);
        let mut game = Game {
            plan: log.plan.clone(),
            log,
            mode,
            economy,
            state: State::default(),
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
        let courses: Vec<Course> = self
            .ships
            .iter()
            .zip(&self.plan.ships)
            .map(|(d, ship)| Course {
                id: d.id,
                fitted: ships::read(ship, &self.economy).expect("a game's ships read (`allowed`)"),
                epoch: ship.epoch,
                items: d.items().collect(),
                done: d.done,
            })
            .collect();
        let state = state::run(&courses, &self.economy);
        self.state = state;
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
        for k in 0..self.log.plan.ships.len() {
            let inputs = self.inputs(k);
            let ship = match old.iter().position(|s| s.inputs == inputs) {
                Some(j) => {
                    let mut kept = old.swap_remove(j);
                    kept.name.clone_from(&self.log.plan.ships[k].name);
                    kept
                }
                None => self.design(k),
            };
            self.ships.push(ship);
        }
        self.plan = self.log.plan.clone();
        for k in 0..self.ships.len() {
            self.stand(k);
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
            ships::breaches(&ship, &f, &self.economy, world().eph)
        });
        self.plan.ships[k] = ship;
    }

    /// What ship `k` of the log is designed from, hashed: the ship (its
    /// name, class and load aside) and its re-plans.
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
            refused: Vec::new(),
            breaches: Vec::new(),
            revision: revision(),
            steps: Vec::new(),
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
    /// and each step of the design as it stands.
    fn spawn(&self, ship: &ShipDesign, first: Ship, replans: Vec<Replan>) {
        let (live, used) = (ship.live.clone(), ship.used.clone());
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
                        // Taken before it is kept, so that the game does
                        // not let it go in between.
                        used.lock().unwrap().insert(key);
                        Some(cache.get(key).unwrap_or_else(|| cache.put(key, design())))
                    },
                    &mut |key, designed| {
                        send(Event::Shown(n, key, designed.clone()));
                        n += 1;
                    },
                    &mut |i, why| send(Event::Refused(i, why)),
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
        if any {
            self.rerun();
        }
        any
    }

    /// Waits until every ship is designed.
    pub fn wait(&mut self) {
        let mut any = false;
        while !self.done() {
            match self.receiver.recv() {
                Ok((live, event)) => any |= self.take(&live, event),
                Err(_) => break,
            }
        }
        if any {
            self.rerun();
        }
    }

    /// An event from the walk whose flag is `live`, if it is a ship's
    /// current one; `true` if the ship's design changed.
    fn take(&mut self, live: &Arc<AtomicBool>, event: Event) -> bool {
        let Some(k) = self.ships.iter().position(|s| Arc::ptr_eq(&s.live, live)) else {
            return false;
        };
        let ship = &mut self.ships[k];
        match event {
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
                }
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
    /// edited, from its start. A new name, class or load alone keeps
    /// them, and designs nothing.
    pub fn edit(&mut self, id: u64, mut ship: Ship) -> Result<(), String> {
        self.sandbox("Editing a ship")?;
        let k = self.index(id).ok_or(format!("no ship has id {id}"))?;
        ship.id = id;
        let mut refit = self.plan.ships[k].clone();
        refit.name.clone_from(&ship.name);
        refit.class.clone_from(&ship.class);
        refit.propellant_t = ship.propellant_t;
        refit.cargo.clone_from(&ship.cargo);
        let refit = format!("{refit:?}") == format!("{ship:?}");
        let log = self.changed(|log| match refit {
            true => {
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
        let log = self.changed(|log| log.plan.ships.push(ship))?;
        let before = std::mem::replace(&mut self.log, log);
        if self.mode == Mode::Sandbox {
            self.undo.push(before);
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
    /// now (to the minute). It takes effect where the ship is: now, if it
    /// is staying where the step that holds `from` leaves from; at the
    /// end of a leg in flight; or later. Refused if its steps break the
    /// ship's class, if it changes a step whose leg has begun, or if it
    /// needs a longer station visit than was built (until step 3).
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
        let log = self.changed(|log| log.replans.push(replan.clone()))?;
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
        self.log = log;
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
    let Some(Step::Go(go)) = ship.steps.get(before.step) else {
        unreachable!("an item's step is a leg's");
    };
    Some(format!("at {}", go.to.label))
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
    let first = steps
        .iter()
        .rposition(|s| matches!(s, Step::Go(_)))
        .map_or(0, |k| k + 1);
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
