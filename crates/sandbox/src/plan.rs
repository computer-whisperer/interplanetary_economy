//! A mission plan in the sandbox (M1c.md, step 4 plan, item 4; step 7
//! plan, item 2): its ships designed in the background, in parallel on a
//! pool of their own (the UI's rebuilds use rayon's global pool, and
//! would wait behind them), each item kept as it arrives; a ship edited
//! designed again from where the edit takes effect.

use glam::DVec3;
use orbit::ephem::Ephemeris;
use orbit::mission::{
    Checkpoint, Item, ItemKind, Marker, MarkerKind, Plan, Planner, Step, World, resume_from,
};
use orbit::places::Places;
use orbit::time::{Epoch, SECONDS_PER_DAY};
use orbit::track::Track;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, LazyLock};

/// What a designer reports about a ship.
enum Message {
    /// It started: where it is before its first step.
    Started(Checkpoint),
    /// Items, and where the ship is after them.
    Items(Vec<Item>, Checkpoint),
    /// It could not start: why.
    Refused(String),
    Done,
}

/// A ship of the plan and what has been designed of it.
#[derive(Clone, Debug)]
pub struct Ship {
    pub name: String,
    pub items: Vec<Item>,
    pub done: bool,
    /// Why it could not start, or why its plan stops.
    pub failure: Option<String>,
    /// Its first leg's place in the plan's legs, counted in the file.
    pub first_leg: usize,
}

/// A ship's design: its generation (each edit starts another; a
/// designer of an older one stops after the step it is on, and what it
/// sent is dropped), and where the ship is before each step designed.
struct Design {
    generation: u64,
    live: Arc<AtomicU64>,
    checkpoints: Vec<Checkpoint>,
}

/// The fields legs are designed in, built once.
static WORLD: LazyLock<World> = LazyLock::new(World::standard);

/// A plan being designed.
pub struct Loaded {
    pub name: String,
    /// The plan as edited.
    pub plan: Plan,
    pub ships: Vec<Ship>,
    designs: Vec<Design>,
    /// The first ship's start.
    pub start: Epoch,
    /// Changes whenever items arrive or a ship is edited (not when a
    /// ship is only done).
    pub generation: u64,
    pool: Arc<rayon::ThreadPool>,
    sender: Sender<(usize, u64, Message)>,
    receiver: Receiver<(usize, u64, Message)>,
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
    /// Reads the plan at `path` and starts designing it.
    pub fn open(path: &Path) -> Result<Loaded, String> {
        let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let plan = Plan::parse(&src, Ephemeris::standard(), Places::standard())
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Loaded::start(plan))
    }

    pub fn start(plan: Plan) -> Loaded {
        let ships = plan
            .ships
            .iter()
            .zip(first_legs(&plan))
            .map(|(s, first_leg)| Ship {
                name: s.name.clone(),
                items: Vec::new(),
                done: false,
                failure: None,
                first_leg,
            })
            .collect();
        let designs = plan
            .ships
            .iter()
            .map(|_| Design {
                generation: 0,
                live: Arc::new(AtomicU64::new(0)),
                checkpoints: Vec::new(),
            })
            .collect();
        let start = plan
            .ships
            .iter()
            .map(|s| s.epoch)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap_or(Epoch::from_ymd(2045, 1, 1));
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .thread_name(|i| format!("design {i}"))
            .build()
            .expect("a thread pool");
        let (sender, receiver) = channel();
        let loaded = Loaded {
            name: plan.name.clone(),
            plan,
            ships,
            designs,
            start,
            generation: 0,
            pool: Arc::new(pool),
            sender,
            receiver,
        };
        for s in 0..loaded.ships.len() {
            loaded.design(s, None);
        }
        loaded
    }

    /// Designs ship `s` in the background for its design's generation,
    /// from `from` (else its start), sending each step's items as they
    /// are ready. A ship whose design panics is refused; once the plan is
    /// closed or the ship edited again, the designer stops after the step
    /// it is on.
    fn design(&self, s: usize, from: Option<Checkpoint>) {
        let ship = self.plan.ships[s].clone();
        let (generation, live) = (self.designs[s].generation, self.designs[s].live.clone());
        let sender = self.sender.clone();
        self.pool.spawn(move || {
            let world = &*WORLD;
            let send = |m| {
                live.load(Ordering::Relaxed) == generation
                    && sender.send((s, generation, m)).is_ok()
            };
            let designed = catch_unwind(AssertUnwindSafe(|| {
                let mut planner = match from {
                    Some(c) => Planner::resume(world, &ship, c),
                    None => match Planner::new(world, &ship) {
                        Ok(p) => {
                            if !send(Message::Started(p.checkpoint())) {
                                return;
                            }
                            p
                        }
                        Err(why) => {
                            send(Message::Refused(why));
                            return;
                        }
                    },
                };
                while let Some(items) = planner.step() {
                    if !send(Message::Items(items, planner.checkpoint())) {
                        return;
                    }
                }
                send(Message::Done);
            }));
            if let Err(panic) = designed {
                let why = panic
                    .downcast_ref::<String>()
                    .map(String::as_str)
                    .or_else(|| panic.downcast_ref::<&str>().copied())
                    .unwrap_or("no message");
                send(Message::Refused(format!("designing it panicked: {why}")));
            }
        });
    }

    /// Replaces ship `s` with `ship`, whose steps from `edited` on
    /// changed (`None`: its start changed), and designs it again from
    /// where that takes effect (`mission::resume_from`), keeping the items
    /// before.
    pub fn edit(&mut self, s: usize, ship: orbit::mission::Ship, edited: Option<usize>) {
        let design = &mut self.designs[s];
        let from = edited
            .and_then(|e| resume_from(&design.checkpoints, e))
            .cloned();
        let keep = from.as_ref().map_or(0, Checkpoint::next);
        design.checkpoints.retain(|c| c.next() <= keep);
        design.generation += 1;
        design.live.store(design.generation, Ordering::Relaxed);
        self.plan.ships[s] = ship;
        let shown = &mut self.ships[s];
        shown.name = self.plan.ships[s].name.clone();
        shown.items.retain(|i| i.step < keep);
        shown.done = false;
        shown.failure = shown.items.iter().find_map(|i| match &i.kind {
            ItemKind::Failed(why) => Some(why.clone()),
            _ => None,
        });
        for (shown, first) in self.ships.iter_mut().zip(first_legs(&self.plan)) {
            shown.first_leg = first;
        }
        self.generation += 1;
        self.design(s, from);
    }

    /// Takes what has arrived; `true` if anything did.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((s, generation, message)) = self.receiver.try_recv() {
            any |= self.take(s, generation, message);
        }
        any
    }

    /// Waits until every ship is designed.
    pub fn wait(&mut self) {
        while !self.done() {
            match self.receiver.recv() {
                Ok((s, generation, message)) => {
                    self.take(s, generation, message);
                }
                Err(_) => return,
            }
        }
    }

    /// Takes a message for ship `s`'s design of `generation`: `false`
    /// if it is of an older one.
    fn take(&mut self, s: usize, generation: u64, message: Message) -> bool {
        let (ship, design) = (&mut self.ships[s], &mut self.designs[s]);
        if generation != design.generation {
            return false;
        }
        match message {
            Message::Started(c) => design.checkpoints.push(c),
            Message::Items(items, c) => {
                for item in &items {
                    if let ItemKind::Failed(why) = &item.kind {
                        ship.failure = Some(why.clone());
                    }
                }
                ship.items.extend(items);
                design.checkpoints.push(c);
                self.generation += 1;
            }
            Message::Refused(why) => {
                ship.failure = Some(why);
                ship.done = true;
            }
            Message::Done => ship.done = true,
        }
        true
    }

    pub fn done(&self) -> bool {
        self.ships.iter().all(|s| s.done)
    }

    /// From the first ship's start to the end of the last item designed.
    pub fn span(&self) -> (Epoch, Epoch) {
        let end = self
            .ships
            .iter()
            .flat_map(|s| s.items.last())
            .map(|i| i.end.0)
            .fold(self.start.0, f64::max);
        (self.start, Epoch(end))
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
        let done = self.ships.iter().filter(|s| s.done).count();
        let failed = self.ships.iter().filter(|s| s.failure.is_some()).count();
        let mut s = if done == self.ships.len() {
            format!("{}: {} ships designed", self.name, done)
        } else {
            format!(
                "{}: designing, {done} of {} ships",
                self.name,
                self.ships.len()
            )
        };
        if failed > 0 {
            s.push_str(&format!(", {failed} failed"));
        }
        s
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

/// Where `track` is at `t` (barycentric km), if it spans it.
pub fn on_track(eph: &Ephemeris, track: &Track, t: Epoch) -> Option<DVec3> {
    let (first, last) = track.span()?;
    let s = (t.0 - track.epoch.0) * SECONDS_PER_DAY;
    if s < first - SAME_TIME_S || s > last + SAME_TIME_S {
        return None;
    }
    let s = track.at(s.clamp(first, last))?;
    let center = track
        .center
        .map_or(DVec3::ZERO, |c| eph.barycentric_position(c, t));
    Some(center + DVec3::new(s[0], s[1], s[2]))
}
