//! A mission plan in the sandbox (M1c.md, step 4 plan, item 4; step 7
//! plan, item 2): a game of the sim engine (`sim::game`, M1b.md, step 1
//! plan, part 7), its ships designed in the background on a pool of
//! their own at a lower priority (`background`), with what the sandbox
//! adds: each ship's view, its legs' window lanes, and sweeps of a leg's
//! durations.

use crate::windows::Windows;
use glam::DVec3;
use orbit::ephem::Ephemeris;
use orbit::mission::{Choice, Item, ItemKind, Marker, MarkerKind, Pin, Plan, Planner, Step};
use orbit::time::{Epoch, SECONDS_PER_DAY};
use orbit::track::Track;
use rayon::prelude::*;
use sim::game::{Game, world};
use sim::log::{Log, Mode};
use sim::ships::{Breach, Commodity, Load};
use sim::state::{Dry, ShipState};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

/// A sweep of a leg's durations (M1c.md, step 7 plan, item 5): designed
/// for the ship as it was (its inputs), the plan left as it is.
#[derive(Clone, Debug)]
pub struct Sweep {
    /// Its own, so that an earlier sweep's result does not fill it.
    pub id: u64,
    /// The ship's inputs it was designed for (`ShipDesign::inputs`).
    pub inputs: u64,
    pub durations: Vec<f64>,
    /// Each duration's design, once done; `Err` why none was.
    pub options: Option<Result<Vec<Choice>, String>>,
}

/// A ship of the plan as the views show it: what has been designed of
/// it.
#[derive(Clone, Debug)]
pub struct Ship {
    pub id: u64,
    pub name: String,
    pub items: Vec<Item>,
    pub done: bool,
    /// Why it could not start, or why its plan stops.
    pub failure: Option<String>,
    /// Its first leg's place in the plan's legs, counted in the file.
    pub first_leg: usize,
    /// Another whenever its items change, unique in the process
    /// (`ShipDesign::revision`): what its lines are built for.
    pub revision: u64,
    /// What its class does not allow of it (`ShipDesign::breaches`).
    pub warnings: Vec<Breach>,
    /// Its class and what the game's loop found of it; `None` for a ship
    /// without a class (massless).
    pub tanks: Option<Tanks>,
}

/// A ship with a class as the game's loop found it (`sim::state`, M1b.md,
/// step 2 plan, parts 3 and 4).
#[derive(Clone, Debug, PartialEq)]
pub struct Tanks {
    pub class: String,
    /// Its propellant's fuel: methane or hydrogen.
    pub fuel: Commodity,
    pub state: ShipState,
    /// From when its state is pending: where the loop stopped, while a
    /// ship is still being designed.
    pub pending_from: Option<Epoch>,
}

impl Tanks {
    /// Whether its state at `t` is pending.
    pub fn pending(&self, t: Epoch) -> bool {
        self.pending_from.is_some_and(|p| t.0 >= p.0)
    }

    /// What it carries at `t`: `None` before its start, or where it is
    /// pending.
    pub fn load(&self, t: Epoch) -> Option<Load> {
        match self.pending(t) {
            true => None,
            false => self.state.load(t),
        }
    }

    /// Where it ran dry, if it did where the state is known (one found
    /// where it is pending may move as designs arrive).
    pub fn dry(&self) -> Option<Dry> {
        self.state.dry.filter(|d| !self.pending(d.t))
    }
}

impl Ship {
    /// Where it ran dry, if it did (`Tanks::dry`).
    pub fn dry(&self) -> Option<Dry> {
        self.tanks.as_ref().and_then(Tanks::dry)
    }
}

/// A sweep's result: ship and step swept, the sweep's id, its options.
type Swept = (usize, usize, u64, Result<Vec<Choice>, String>);

/// A plan being designed.
pub struct Loaded {
    game: Game,
    pub ships: Vec<Ship>,
    /// The windows of its legs between planets.
    pub windows: Windows,
    /// Sweeps of legs' durations, by ship and step.
    pub sweeps: HashMap<(usize, usize), Sweep>,
    pool: Arc<rayon::ThreadPool>,
    next_sweep: u64,
    sender: Sender<Swept>,
    receiver: Receiver<Swept>,
}

/// Each ship's first leg's place in the plan's legs.
fn first_legs(plan: &Plan) -> Vec<usize> {
    let mut first = 0;
    plan.ships
        .iter()
        .map(|s| {
            let here = first;
            first += s.steps.iter().filter(|s| matches!(s, Step::Go(_))).count();
            here
        })
        .collect()
}

impl Loaded {
    /// Reads the plan at `path` and starts designing it: a career game's
    /// log opens as a sandbox log.
    pub fn open(path: &Path) -> Result<Loaded, String> {
        let pool = Arc::new(crate::background::pool("design"));
        let game = Game::open(path, Some(Mode::Sandbox), pool.clone())?;
        Ok(Loaded::of(game, pool))
    }

    /// An empty plan named `name`, to be saved at `path`.
    pub fn empty(name: &str, path: PathBuf) -> Loaded {
        let pool = Arc::new(crate::background::pool("design"));
        Loaded::of(Game::empty(name, path, pool.clone()), pool)
    }

    /// `plan` in the sandbox, not saved anywhere; why not, if its ships'
    /// classes or loads do not read (`Game::new`).
    pub fn start(plan: Plan) -> Result<Loaded, String> {
        let pool = Arc::new(crate::background::pool("design"));
        Ok(Loaded::of(
            Game::new(Log::sandbox(plan), pool.clone())?,
            pool,
        ))
    }

    fn of(game: Game, pool: Arc<rayon::ThreadPool>) -> Loaded {
        let (sender, receiver) = channel();
        let mut loaded = Loaded {
            game,
            ships: Vec::new(),
            windows: Windows::default(),
            sweeps: HashMap::new(),
            pool,
            next_sweep: 0,
            sender,
            receiver,
        };
        loaded.shown();
        loaded.windows.update(loaded.game.plan(), &loaded.pool);
        loaded
    }

    /// The plan as it stands.
    pub fn plan(&self) -> &Plan {
        self.game.plan()
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    pub fn path(&self) -> Option<&Path> {
        self.game.path()
    }

    /// Writes the plan to its file: where, or why not.
    pub fn save(&mut self) -> Result<PathBuf, String> {
        self.game.save()
    }

    /// Each ship's view as the game has its design: views whose design
    /// did not change are kept, their name, warnings and tanks as they
    /// stand.
    fn shown(&mut self) {
        let firsts = first_legs(self.game.plan());
        let mut old = std::mem::take(&mut self.ships);
        let (economy, state) = (self.game.economy(), self.game.state());
        for (k, (d, first_leg)) in self.game.ships().iter().zip(firsts).enumerate() {
            let tanks = self.game.plan().ships[k].class.as_ref().map(|id| {
                let class = economy
                    .ships
                    .iter()
                    .find(|c| &c.id == id)
                    .expect("a game's ships read (`Game::new`)");
                Tanks {
                    class: id.clone(),
                    fuel: Commodity::fuel(class.propellant),
                    state: state.ships[k].clone(),
                    pending_from: (!state.done).then_some(state.until),
                }
            });
            let kept = old
                .iter()
                .position(|v| v.id == d.id && v.revision == d.revision)
                .map(|k| old.swap_remove(k));
            let ship = match kept {
                Some(mut v) => {
                    v.name.clone_from(&d.name);
                    v.first_leg = first_leg;
                    v.warnings.clone_from(&d.breaches);
                    v.tanks = tanks;
                    v
                }
                None => Ship {
                    id: d.id,
                    name: d.name.clone(),
                    items: d.items().cloned().collect(),
                    done: d.done,
                    failure: d.failure.clone(),
                    first_leg,
                    revision: d.revision,
                    warnings: d.breaches.clone(),
                    tanks,
                },
            };
            self.ships.push(ship);
        }
    }

    /// Replaces ship `s` with `ship`, designing it again from where it
    /// first changed (a new name alone is only a rename, and a new class
    /// or load designs nothing); undoable. Why not, if the game refuses it
    /// (a class or load that does not read).
    pub fn edit(&mut self, s: usize, ship: orbit::mission::Ship) -> Result<(), String> {
        let id = self.ships[s].id;
        let before = &self.game.plan().ships[s];
        let mut renamed = before.clone();
        renamed.name.clone_from(&ship.name);
        if format!("{renamed:?}") == format!("{ship:?}") {
            if before.name != ship.name {
                self.game.rename(id, &ship.name);
                self.shown();
            }
            return Ok(());
        }
        let inputs = self.game.ships()[s].inputs();
        self.game.edit(id, ship)?;
        if self.game.ships()[s].inputs() != inputs {
            self.sweeps.retain(|&(ship, _), _| ship != s);
        }
        self.changed();
        Ok(())
    }

    /// Renames ship `s`: nothing to design or draw again.
    pub fn rename(&mut self, s: usize, name: &str) {
        let id = self.ships[s].id;
        self.game.rename(id, name);
        self.shown();
    }

    /// Adds `ship` to the plan, last, with an id of its own, and designs
    /// it.
    pub fn add_ship(&mut self, ship: orbit::mission::Ship) {
        self.game
            .add_ship(ship)
            .expect("the sandbox adds any ship whose class and load read");
        self.sweeps.clear();
        self.changed();
    }

    /// Removes ship `s`; its design stops after the step it is on.
    pub fn remove_ship(&mut self, s: usize) {
        let id = self.ships[s].id;
        self.game
            .remove_ship(id)
            .expect("the sandbox removes any ship");
        self.sweeps.clear();
        self.changed();
    }

    /// Back to the plan before the last change, designs from the cache:
    /// `false` if there was none.
    pub fn undo(&mut self) -> bool {
        if !self.game.undo() {
            return false;
        }
        self.sweeps.clear();
        self.changed();
        true
    }

    /// After an edit: the views and the window lanes.
    fn changed(&mut self) {
        self.shown();
        self.windows.update(self.game.plan(), &self.pool);
    }

    /// Designs ship `s`'s leg at `step` at each of `durations` in the
    /// background, each as pinning it would design it (a station visited
    /// before it is followed for that duration), from where an edit at
    /// the step would design it from, to show them beside the leg's own
    /// options; the plan is left as it is.
    pub fn sweep(&mut self, s: usize, step: usize, durations: Vec<f64>) {
        let ship = self.game.plan().ships[s].clone();
        if !matches!(ship.steps.get(step), Some(Step::Go(_))) {
            return;
        }
        let Some(from) = self.game.resume_point(s, step) else {
            return;
        };
        let id = self.next_sweep;
        self.next_sweep += 1;
        self.sweeps.insert(
            (s, step),
            Sweep {
                id,
                inputs: self.game.ships()[s].inputs(),
                durations: durations.clone(),
                options: None,
            },
        );
        let sender = self.sender.clone();
        self.pool.spawn(move || {
            let world = world();
            // The leg pinned to `days`: its one option.
            let alone = |days: f64| -> Result<Choice, String> {
                let mut ship = ship.clone();
                if let Some(Step::Go(go)) = ship.steps.get_mut(step) {
                    go.pin = Some(Pin {
                        depart: None,
                        days: Some(days),
                    });
                }
                let mut planner = match from.clone() {
                    Some(c) => Planner::resume(world, &ship, c),
                    None => Planner::new(world, &ship)?,
                };
                loop {
                    let items = planner.step().ok_or("the leg was not reached")?;
                    // The leg itself, not the stay before it.
                    let leg = |i: &&Item| {
                        i.step == step && matches!(i.kind, ItemKind::Leg | ItemKind::Failed(_))
                    };
                    if let Some(item) = items.iter().find(leg) {
                        return Ok(match &item.kind {
                            ItemKind::Failed(_) => Choice {
                                depart: item.start,
                                days,
                                dv: None,
                            },
                            _ => item.options.first().copied().unwrap_or(Choice {
                                depart: item.start,
                                days,
                                dv: Some(item.dv),
                            }),
                        });
                    }
                }
            };
            let swept = catch_unwind(AssertUnwindSafe(|| {
                durations.par_iter().map(|&d| alone(d)).collect()
            }))
            .unwrap_or_else(|_| Err("sweeping it panicked".into()));
            let _ = sender.send((s, step, id, swept));
        });
    }

    /// Takes what has arrived.
    pub fn poll(&mut self) {
        self.windows.poll();
        if self.game.poll() {
            self.shown();
        }
        while let Ok((s, step, id, options)) = self.receiver.try_recv() {
            self.swept(s, step, id, options);
        }
    }

    /// A sweep's options, if the sweep is still wanted: the ship
    /// unchanged since.
    fn swept(&mut self, s: usize, step: usize, id: u64, options: Result<Vec<Choice>, String>) {
        let Some(sweep) = self.sweeps.get_mut(&(s, step)) else {
            return;
        };
        let inputs = self.game.ships().get(s).map(|d| d.inputs());
        if sweep.id == id && inputs == Some(sweep.inputs) {
            sweep.options = Some(options);
        }
    }

    /// Waits until every ship is designed, and every window lane
    /// computed.
    pub fn wait(&mut self) {
        self.game.wait();
        self.shown();
        self.windows.wait();
    }

    /// Waits until every sweep is done.
    pub fn wait_sweeps(&mut self) {
        while self.sweeps.values().any(|s| s.options.is_none()) {
            match self.receiver.recv() {
                Ok((s, step, id, options)) => {
                    self.swept(s, step, id, options);
                }
                Err(_) => return,
            }
        }
    }

    /// The first ship's start (2045-01-01 without ships).
    pub fn epoch(&self) -> Epoch {
        self.plan()
            .ships
            .iter()
            .map(|s| s.epoch)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap_or(Epoch::from_ymd(2045, 1, 1))
    }

    pub fn done(&self) -> bool {
        self.game.done()
    }

    /// From the first ship's start to the end of the last item designed.
    pub fn span(&self) -> (Epoch, Epoch) {
        let start = self.epoch();
        let end = self
            .ships
            .iter()
            .flat_map(|s| s.items.last())
            .map(|i| i.end.0)
            .fold(start.0, f64::max);
        (start, Epoch(end))
    }

    /// Every marker's time ([`event_time`]), sorted.
    pub fn events(&self) -> Vec<Epoch> {
        let mut t: Vec<Epoch> = self
            .ships
            .iter()
            .flat_map(|s| &s.items)
            .flat_map(|i| i.markers.iter().map(move |m| event_time(i, m)))
            .collect();
        t.sort_by(|a, b| a.0.total_cmp(&b.0));
        t
    }

    /// The status line's words: how many ships are designed, and failed.
    pub fn progress(&self) -> String {
        self.game.progress()
    }
}

/// When a marker is drawn and jumped to: a landing where the ship leaves
/// its orbit (the item's start), an ascent where it reaches one (its end),
/// the rest at their own times.
pub fn event_time(item: &Item, marker: &Marker) -> Epoch {
    match marker.kind {
        MarkerKind::Landing => item.start,
        MarkerKind::Ascent => item.end,
        _ => marker.t,
    }
}

/// Where a ship is at `t` (barycentric km), from the items whose tracks
/// span it: at an instant two share, the later item's if `later`.
pub fn position(eph: &Ephemeris, items: &[Item], t: Epoch, later: bool) -> Option<DVec3> {
    let mut covering = items
        .iter()
        .filter_map(|i| i.track.as_ref())
        .filter_map(|track| on_track(eph, track, t));
    if later {
        covering.next_back()
    } else {
        covering.next()
    }
}

/// A time this close to a track's end, s, is at its end: an item's times
/// and its track's are computed from different epochs.
const SAME_TIME_S: f64 = 1e-3;

/// Whether `track` spans `t`, within [`SAME_TIME_S`] of its ends.
pub fn spans(track: &Track, t: Epoch) -> bool {
    let s = (t.0 - track.epoch.0) * SECONDS_PER_DAY;
    track
        .span()
        .is_some_and(|(first, last)| first - SAME_TIME_S <= s && s <= last + SAME_TIME_S)
}

/// Where `track` is at `t` (barycentric km), if it spans it.
pub fn on_track(eph: &Ephemeris, track: &Track, t: Epoch) -> Option<DVec3> {
    if !spans(track, t) {
        return None;
    }
    let (first, last) = track.span()?;
    let s = (t.0 - track.epoch.0) * SECONDS_PER_DAY;
    let s = track.at(s.clamp(first, last))?;
    let center = track
        .center
        .map_or(DVec3::ZERO, |c| eph.barycentric_position(c, t));
    Some(center + DVec3::new(s[0], s[1], s[2]))
}

/// The plans in `dir`, by their files' names (without `.toml`), sorted.
pub fn plans_in(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut plans: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .filter_map(|p| Some((p.file_stem()?.to_str()?.to_string(), p)))
        .collect();
    plans.sort();
    plans
}

/// A file in `dir` for a new plan: `new-plan.toml`, else the first of
/// `new-plan-2.toml`, … not taken.
pub fn new_plan_path(dir: &Path) -> PathBuf {
    (1..)
        .map(|k| match k {
            1 => dir.join("new-plan.toml"),
            k => dir.join(format!("new-plan-{k}.toml")),
        })
        .find(|p| !p.exists())
        .expect("a free name")
}
