//! A mission plan in the sandbox (M1c.md, step 4 plan, item 4): its ships
//! designed on a background thread, in parallel on a pool of their own
//! (the UI's rebuilds use rayon's global pool, and would wait behind them),
//! each item kept as it arrives.

use glam::DVec3;
use orbit::ephem::Ephemeris;
use orbit::mission::{Item, ItemKind, Marker, MarkerKind, Plan, Planner, Step, World};
use orbit::places::Places;
use orbit::time::{Epoch, SECONDS_PER_DAY};
use orbit::track::Track;
use rayon::prelude::*;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::mpsc::{Receiver, Sender, channel};

/// What the background thread reports about a ship.
enum Message {
    Items(Vec<Item>),
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

/// A plan being designed.
pub struct Loaded {
    pub name: String,
    pub ships: Vec<Ship>,
    /// The first ship's start.
    pub start: Epoch,
    /// Changes whenever items arrive (not when a ship is only done).
    pub generation: u64,
    receiver: Receiver<(usize, Message)>,
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
        let mut first_leg = 0;
        let ships = plan
            .ships
            .iter()
            .map(|s| {
                let ship = Ship {
                    name: s.name.clone(),
                    items: Vec::new(),
                    done: false,
                    failure: None,
                    first_leg,
                };
                first_leg += s.steps.iter().filter(|s| matches!(s, Step::Go(_))).count();
                ship
            })
            .collect();
        let start = plan
            .ships
            .iter()
            .map(|s| s.epoch)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap_or(Epoch::from_ymd(2045, 1, 1));
        let (sender, receiver) = channel();
        let name = plan.name.clone();
        std::thread::spawn(move || design(plan, sender));
        Loaded {
            name,
            ships,
            start,
            generation: 0,
            receiver,
        }
    }

    /// Takes what has arrived; `true` if anything did.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((i, message)) = self.receiver.try_recv() {
            self.take(i, message);
            any = true;
        }
        any
    }

    /// Waits until every ship is designed.
    pub fn wait(&mut self) {
        while !self.done() {
            match self.receiver.recv() {
                Ok((i, message)) => self.take(i, message),
                Err(_) => return,
            }
        }
    }

    fn take(&mut self, i: usize, message: Message) {
        let ship = &mut self.ships[i];
        match message {
            Message::Items(items) => {
                for item in &items {
                    if let ItemKind::Failed(why) = &item.kind {
                        ship.failure = Some(why.clone());
                    }
                }
                ship.items.extend(items);
                self.generation += 1;
            }
            Message::Refused(why) => {
                ship.failure = Some(why);
                ship.done = true;
            }
            Message::Done => ship.done = true,
        }
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

/// Designs `plan`'s ships in parallel on a pool of their own, sending each
/// item as it is ready. A ship whose design panics is refused; once the
/// sandbox has closed, ships stop at their next item.
fn design(plan: Plan, sender: Sender<(usize, Message)>) {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("design {i}"))
        .build()
        .expect("a thread pool");
    let world = World::standard();
    pool.install(|| {
        plan.ships.par_iter().enumerate().for_each(|(i, ship)| {
            let send = |m| sender.send((i, m)).is_ok();
            let designed = catch_unwind(AssertUnwindSafe(|| match Planner::new(&world, ship) {
                Ok(mut planner) => {
                    while let Some(items) = planner.step() {
                        if !send(Message::Items(items)) {
                            return;
                        }
                    }
                    send(Message::Done);
                }
                Err(why) => {
                    send(Message::Refused(why));
                }
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
    });
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
