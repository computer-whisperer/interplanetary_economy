//! The window lanes (M1c.md, step 7 plan, item 4): for each kind of leg
//! between planets in the plan (its two ends, and whether it arrives
//! through the atmosphere), each day's departures' least Δv by patched
//! conics over the durations the planner's date search tries, across the
//! ephemeris's span; computed once in the background, a lane at a time.

use orbit::mission::{Plan, Spot, Step};
use orbit::places::PlaceKind;
use orbit::time::Epoch;
use rayon::prelude::*;
use sim::game::world;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

/// A lane (and a porkchop) is shaded from its least Δv, brightest, to
/// this many times it; the brightest shade (sRGB).
pub const CEILING: f32 = 1.5;
pub const BRIGHT: [f32; 3] = [225.0, 225.0, 230.0];

/// How bright `v` km/s is shaded against `least`: 1 at it, 0 from
/// [`CEILING`] times it on (and for none).
pub fn shade(v: f32, least: f32) -> f32 {
    let shade = 1.0 - (v - least) / (least * (CEILING - 1.0));
    if shade.is_nan() {
        0.0
    } else {
        shade.clamp(0.0, 1.0)
    }
}

/// A shade (0 to 1) as sRGB, from the muted surface to [`BRIGHT`].
pub fn shaded(shade: f32) -> [u8; 4] {
    let [r, g, b, _] = damascene_core::prelude::tokens::MUTED.to_srgb_u8a();
    let dark = [f32::from(r), f32::from(g), f32::from(b)];
    let c = |k: usize| (dark[k] + (BRIGHT[k] - dark[k]) * shade).round() as u8;
    [c(0), c(1), c(2), 255]
}

/// A kind of leg between planets, and its days' least Δv.
#[derive(Clone, Debug)]
pub struct Lane {
    /// Its ends exactly, and through the atmosphere or not (two places
    /// with one label are two lanes; one place written two ways, one).
    key: String,
    /// "leo → mars r 3790 km", and "(aero)" through the atmosphere.
    pub label: String,
    /// The first day's departures', then each next day's, least Δv,
    /// km/s (NaN where none is priced); `None` while it is computed.
    pub cheapest: Option<Arc<Vec<f32>>>,
    pub first: Epoch,
    /// The least of them, km/s.
    pub least: f32,
}

impl Lane {
    /// The least Δv of the days from `from` to `to` (at least the day
    /// `from` falls in), km/s; NaN where none is known.
    pub fn least_between(&self, from: Epoch, to: Epoch) -> f32 {
        let Some(days) = &self.cheapest else {
            return f32::NAN;
        };
        let k0 = (from.0 - self.first.0).floor().max(0.0) as usize;
        let k1 = ((to.0 - self.first.0).ceil().max(0.0) as usize).max(k0 + 1);
        days.get(k0..k1.min(days.len()))
            .into_iter()
            .flatten()
            .copied()
            .filter(|v| !v.is_nan())
            .fold(f32::NAN, f32::min)
    }
}

/// The plan's window lanes, and which lane each leg between planets is
/// in.
pub struct Windows {
    pub lanes: Vec<Lane>,
    /// By ship and step.
    pub legs: HashMap<(usize, usize), usize>,
    sender: Sender<(String, Arc<Vec<f32>>)>,
    receiver: Receiver<(String, Arc<Vec<f32>>)>,
}

impl Default for Windows {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Windows {
            lanes: Vec::new(),
            legs: HashMap::new(),
            sender,
            receiver,
        }
    }
}

/// A leg between planets of a plan: its ship and step, where it leaves
/// from and goes to, and through the atmosphere or not.
struct Crossing<'p> {
    ship: usize,
    step: usize,
    from: &'p Spot,
    to: &'p Spot,
    aero: bool,
}

/// Where ship `s`'s step `step` leaves from and goes to, and whether
/// through the atmosphere; `None` for a wait.
pub fn ends(plan: &Plan, s: usize, step: usize) -> Option<(&Spot, &Spot, bool)> {
    let ship = plan.ships.get(s)?;
    let mut at = &ship.start;
    for (k, st) in ship.steps.iter().enumerate() {
        if let Step::Go(go) = st {
            if k == step {
                return Some((at, &go.to, go.aero));
            }
            at = &go.to;
        }
    }
    None
}

/// `plan`'s legs between planets, in order, those the planner would
/// price (not from or to a station).
fn crossings(plan: &Plan) -> Vec<Crossing<'_>> {
    let world = world();
    let mut out = Vec::new();
    for (s, ship) in plan.ships.iter().enumerate() {
        for k in 0..ship.steps.len() {
            let Some((from, to, aero)) = ends(plan, s, k) else {
                continue;
            };
            if world.crossing(&from.kind, &to.kind, aero).is_ok() {
                out.push(Crossing {
                    ship: s,
                    step: k,
                    from,
                    to,
                    aero,
                });
            }
        }
    }
    out
}

fn label(from: &Spot, to: &Spot, aero: bool) -> String {
    let aero = if aero { " (aero)" } else { "" };
    format!("{} → {}{aero}", from.label, to.label)
}

fn key(from: &Spot, to: &Spot, aero: bool) -> String {
    format!("{:?} → {:?} {aero}", from.kind, to.kind)
}

impl Windows {
    /// The lanes for `plan`, in the order its legs first need them:
    /// those already known kept, the rest computed on `pool`.
    pub fn update(&mut self, plan: &Plan, pool: &rayon::ThreadPool) {
        let mut old: HashMap<String, Lane> =
            self.lanes.drain(..).map(|l| (l.key.clone(), l)).collect();
        self.legs.clear();
        for c in crossings(plan) {
            let key = key(c.from, c.to, c.aero);
            let lane = match self.lanes.iter().position(|l| l.key == key) {
                Some(i) => i,
                None => {
                    let lane = old.remove(&key).unwrap_or_else(|| {
                        self.compute(pool, &key, &c.from.kind, &c.to.kind, c.aero);
                        Lane {
                            key: key.clone(),
                            label: label(c.from, c.to, c.aero),
                            cheapest: None,
                            first: world().eph.span().0,
                            least: f32::NAN,
                        }
                    });
                    self.lanes.push(lane);
                    self.lanes.len() - 1
                }
            };
            self.legs.insert((c.ship, c.step), lane);
        }
    }

    /// Computes lane `key`'s days on `pool`.
    fn compute(
        &self,
        pool: &rayon::ThreadPool,
        key: &str,
        from: &PlaceKind,
        to: &PlaceKind,
        aero: bool,
    ) {
        let (key, from, to, sender) = (
            key.to_string(),
            from.clone(),
            to.clone(),
            self.sender.clone(),
        );
        pool.spawn(move || {
            let world = world();
            let Ok(c) = world.crossing(&from, &to, aero) else {
                return;
            };
            let (first, last) = world.eph.span();
            let days = (last.0 - first.0).floor() as usize;
            // A panic here would abort the process (rayon's spawn): a
            // lane that panics stays dark.
            let cheapest = catch_unwind(AssertUnwindSafe(|| {
                (0..days)
                    .into_par_iter()
                    .map(|k| {
                        let d = first.plus_days(k as f64);
                        c.durations(d)
                            .iter()
                            .filter_map(|&tof| c.dv(d, tof))
                            .fold(f64::NAN, f64::min) as f32
                    })
                    .collect::<Vec<f32>>()
            }))
            .unwrap_or_else(|_| vec![f32::NAN; days]);
            let _ = sender.send((key, Arc::new(cheapest)));
        });
    }

    /// Takes the lanes computed; `true` if any were.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok((key, cheapest)) = self.receiver.try_recv() {
            any |= self.take(&key, cheapest);
        }
        any
    }

    /// Waits until every lane is computed.
    pub fn wait(&mut self) {
        while self.lanes.iter().any(|l| l.cheapest.is_none()) {
            match self.receiver.recv() {
                Ok((key, cheapest)) => {
                    self.take(&key, cheapest);
                }
                Err(_) => return,
            }
        }
    }

    /// Lane `key`'s days, if it is still a lane.
    fn take(&mut self, key: &str, cheapest: Arc<Vec<f32>>) -> bool {
        let Some(lane) = self.lanes.iter_mut().find(|l| l.key == key) else {
            return false;
        };
        lane.least = cheapest
            .iter()
            .copied()
            .filter(|v| !v.is_nan())
            .fold(f32::NAN, f32::min);
        lane.cheapest = Some(cheapest);
        true
    }
}
