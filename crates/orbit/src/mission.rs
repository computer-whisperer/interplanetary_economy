//! Mission plans (M1c.md, Mission plans and the step 1 plan): ships, each a
//! start and a chain of legs, read from `data/missions/`. A [`Planner`]
//! designs a ship's legs one at a time, each leaving from where and when
//! the last arrived, and yields what the ship does as [`Item`]s: legs
//! flown, stays between them, and events priced by rules (landing, ascent,
//! aerocapture), with paths sampled for drawing.

use crate::ephem::{BodyId, Ephemeris};
use crate::families::Catalog;
use crate::gravity::Field;
use crate::integrate::{Dop853, Tolerance, Trajectory};
use crate::interplanetary;
use crate::legs;
use crate::places::{PlaceKind, Places};
use crate::reference::{Reference, continue_orbit};
use crate::shooting::{Design, End, Leg, LowOrbit};
use crate::stationkeeping::{Errors, upkeep};
use crate::surface;
use crate::time::{Epoch, SECONDS_PER_DAY};
use crate::track::Track;
use glam::DVec3;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

const DAY: f64 = SECONDS_PER_DAY;

const TOL: Tolerance = Tolerance {
    rtol: 1e-12,
    atol: 1e-9,
};

/// Landing and ascent take hours, days (as in the route graph).
const SURFACE_DAYS: f64 = 0.1;

/// Default departure windows, days: between low orbits and bodies about
/// Earth and the Moon, and elsewhere (a day in Mars's system, as in M1a's
/// gate sweep). From or to a station, a revolution of it.
const EARTH_WINDOW_DAYS: f64 = 5.0;
const OTHER_WINDOW_DAYS: f64 = 1.0;

/// Windows a leg within a system tries (see [`Planner::within`]).
const WITHIN_TRIES: usize = 3;

/// Between planets, the default window is a synodic period, at most this
/// long, days.
const LONGEST_WINDOW_DAYS: f64 = 3.0 * 365.25;

/// The date search between planets: steps in departure and time of flight,
/// days; designs tried, each leaving this many days from the others.
const DEPART_STEP: f64 = 1.0;
const TOF_STEP: f64 = 2.0;
const TRIES: usize = 3;
const TRIES_APART_DAYS: f64 = 5.0;

/// A leg leaving the ship's own low orbit leaves at its first chance that
/// costs at most this much more than leaving with its plane free (a
/// fraction of that design's departure burn): at once, else when the
/// planes align; else at its cheapest if that costs at most this much
/// more; else it jumps.
const PLANE_FAIR: f64 = 0.1;
const PLANE_MOST: f64 = 0.25;

/// Times over the longest wait at which the planes' alignment is looked
/// for; of those where they align, the first this many are tried, each
/// moved by these fractions of the longest wait (unless the smallest is
/// under a revolution of the ship's orbit).
const ALIGN_SAMPLES: usize = 400;
const ALIGN_TIMES: usize = 2;
const ALIGN_STEPS: [f64; 5] = [-1.0 / 120.0, -1.0 / 240.0, 0.0, 1.0 / 240.0, 1.0 / 120.0];

/// A pin this close before the ship is ready leaves when it is, days:
/// pins are kept to the minute.
const PIN_SLACK_DAYS: f64 = 0.5 / 1_440.0;

/// A station's reference starts this long before it is needed, and runs
/// this long after, days.
const VISIT_MARGIN_DAYS: f64 = 2.0;

/// Station-keeping is simulated over this many runs (`upkeep`), on a
/// reference at least this many revolutions long.
const UPKEEP_RUNS: u64 = 4;
const UPKEEP_REVOLUTIONS: f64 = 4.0;

/// A plan: ships and their legs, as written in `data/missions/`.
#[derive(Clone, Debug)]
pub struct Plan {
    pub name: String,
    pub ships: Vec<Ship>,
    /// The comments its file opens with, kept when it is written out
    /// (the rest of its comments are not).
    pub header: String,
    /// Its `[game]` table and `[[replan]]` entries, which the sim engine
    /// reads (M1b.md, step 1 plan, part 2): kept as written.
    pub game: Option<toml::Table>,
    pub replans: Vec<toml::Table>,
}

#[derive(Clone, Debug)]
pub struct Ship {
    /// Its own in the plan: names change and can repeat.
    pub id: u64,
    pub name: String,
    pub start: Spot,
    pub epoch: Epoch,
    pub steps: Vec<Step>,
}

/// A place as a plan names it.
#[derive(Clone, Debug)]
pub struct Spot {
    /// Its id, or a description of a place written out.
    pub label: String,
    pub kind: PlaceKind,
    /// As the plan wrote it: an id, or a table.
    pub source: toml::Value,
}

impl Spot {
    /// The place with id `id`.
    pub fn named(id: &str, places: &Places) -> Option<Spot> {
        let p = places.get(id)?;
        Some(Spot {
            label: id.to_string(),
            kind: p.kind.clone(),
            source: toml::Value::String(id.to_string()),
        })
    }
}

#[derive(Clone, Debug)]
pub enum Step {
    Go(Go),
    /// Days at the place, before the next leg's window opens.
    Wait(f64),
}

#[derive(Clone, Debug)]
pub struct Go {
    pub to: Spot,
    /// Durations offered, days: the cheapest design flies. Required within
    /// a system; between planets, without any the leg takes the cheapest
    /// date and duration.
    pub days: Vec<f64>,
    /// When the ship may leave; by default from when it is ready, for as
    /// long as [`Planner`] says.
    pub window: Option<(Epoch, Epoch)>,
    /// One of its options chosen: what picking one writes, over `days`
    /// and `window`, which stay as they were.
    pub pin: Option<Pin>,
    /// Arriving through the atmosphere: aerocapture into a low orbit,
    /// entry to the surface.
    pub aero: bool,
    pub note: Option<String>,
}

/// A leg's option chosen: when it leaves (a leg between planets, or an
/// event), and how long it takes (any leg). Either may be left to the
/// planner.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pin {
    pub depart: Option<Epoch>,
    pub days: Option<f64>,
}

impl Go {
    /// The durations the planner designs, days: the pinned one, else
    /// those offered.
    pub fn durations(&self) -> Vec<f64> {
        match self.pin.and_then(|p| p.days) {
            Some(d) => vec![d],
            None => self.days.clone(),
        }
    }

    /// The longest duration it may be designed at, days: of those offered
    /// and the pinned one, so that pinning an option leaves what is built
    /// around the leg (a station's visit, which its durations bound) as
    /// it was.
    pub fn longest(&self) -> f64 {
        self.days
            .iter()
            .copied()
            .chain(self.pin.and_then(|p| p.days))
            .fold(0.0, f64::max)
    }

    /// The pinned departure, if any.
    pub fn depart(&self) -> Option<Epoch> {
        self.pin.and_then(|p| p.depart)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlanFile {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    game: Option<toml::Table>,
    #[serde(default)]
    ship: Vec<ShipFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    replan: Vec<toml::Table>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ShipFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<u64>,
    name: String,
    start: StartFile,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    leg: Vec<LegFile>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StartFile {
    at: toml::Value,
    epoch: String,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LegFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    to: Option<toml::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wait: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    days: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    window: Option<[String; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pin: Option<PinFile>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    aero: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PinFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    depart: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    days: Option<f64>,
}

impl Plan {
    pub fn parse(src: &str, eph: &Ephemeris, places: &Places) -> Result<Plan, String> {
        let file: PlanFile = toml::from_str(src).map_err(|e| e.to_string())?;
        let mut ids: Vec<u64> = file.ship.iter().filter_map(|s| s.id).collect();
        ids.sort_unstable();
        if let Some(w) = ids.windows(2).find(|w| w[0] == w[1]) {
            return Err(format!("two ships have id {}", w[0]));
        }
        // Ships without an id get the next ones after the largest.
        let mut next = ids.last().map_or(0, |&id| id + 1);
        let ships = file
            .ship
            .into_iter()
            .map(|ship| {
                let name = ship.name;
                let fail = |e: String| format!("ship {name:?}: {e}");
                let start = spot(&ship.start.at, eph, places).map_err(fail)?;
                let epoch = ship.start.epoch.parse::<Epoch>().map_err(fail)?;
                let steps = steps(ship.leg, eph, places)
                    .map_err(|(i, e)| format!("ship {name:?}, leg {}: {e}", i + 1))?;
                let id = ship.id.unwrap_or_else(|| {
                    next += 1;
                    next - 1
                });
                Ok(Ship {
                    id,
                    name,
                    start,
                    epoch,
                    steps,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        // Its opening comments, blank lines among them.
        let opening: Vec<&str> = src
            .lines()
            .take_while(|l| l.trim().is_empty() || l.trim_start().starts_with('#'))
            .collect();
        let last = opening.iter().rposition(|l| !l.trim().is_empty());
        let header: String = opening[..last.map_or(0, |k| k + 1)]
            .iter()
            .skip_while(|l| l.trim().is_empty())
            .map(|l| format!("{}\n", l.trim_end_matches('\r')))
            .collect();
        Ok(Plan {
            name: file.name,
            ships,
            header,
            game: file.game,
            replans: file.replan,
        })
    }

    /// An id no ship of the plan has.
    pub fn next_id(&self) -> u64 {
        self.ships.iter().map(|s| s.id + 1).max().unwrap_or(0)
    }

    /// The plan as a file, which [`Plan::parse`] reads back as this plan
    /// (times to the minute).
    pub fn to_toml(&self) -> String {
        let file = PlanFile {
            name: self.name.clone(),
            game: self.game.clone(),
            ship: self
                .ships
                .iter()
                .map(|ship| ShipFile {
                    id: Some(ship.id),
                    name: ship.name.clone(),
                    start: StartFile {
                        at: ship.start.source.clone(),
                        epoch: ship.epoch.stamp(),
                    },
                    leg: legs(&ship.steps),
                })
                .collect(),
            replan: self.replans.clone(),
        };
        let body = toml::to_string(&file).expect("a plan serializes");
        match self.header.is_empty() {
            true => body,
            false => format!("{}\n{body}", self.header),
        }
    }
}

/// Steps as a plan's legs (`[[ship.leg]]`) are written: a TOML array
/// of tables, which [`parse_legs`] reads back.
pub fn legs_value(steps: &[Step]) -> toml::Value {
    toml::Value::try_from(legs(steps)).expect("legs serialize")
}

/// Steps from legs as a plan writes them (`[[ship.leg]]`): an array of
/// tables.
pub fn parse_legs(
    value: toml::Value,
    eph: &Ephemeris,
    places: &Places,
) -> Result<Vec<Step>, String> {
    let legs: Vec<LegFile> = value.try_into().map_err(|e| e.to_string())?;
    steps(legs, eph, places).map_err(|(i, e)| format!("leg {}: {e}", i + 1))
}

fn legs(steps: &[Step]) -> Vec<LegFile> {
    steps
        .iter()
        .map(|step| match step {
            Step::Wait(days) => LegFile {
                wait: Some(*days),
                ..LegFile::default()
            },
            Step::Go(go) => LegFile {
                to: Some(go.to.source.clone()),
                days: (!go.days.is_empty()).then(|| go.days.clone()),
                window: go.window.map(|(a, b)| [a.stamp(), b.stamp()]),
                pin: go.pin.map(|p| PinFile {
                    depart: p.depart.map(Epoch::stamp),
                    days: p.days,
                }),
                aero: go.aero,
                note: go.note.clone(),
                ..LegFile::default()
            },
        })
        .collect()
}

/// A ship's legs as steps; else the leg that is wrong (from 0), and why.
fn steps(
    legs: Vec<LegFile>,
    eph: &Ephemeris,
    places: &Places,
) -> Result<Vec<Step>, (usize, String)> {
    let date = |s: &str| s.parse::<Epoch>();
    legs.into_iter()
        .enumerate()
        .map(|(i, leg)| {
            let fail = |e: String| (i, e);
            match (leg.to, leg.wait) {
                (None, Some(days)) => {
                    if leg.days.is_some() || leg.window.is_some() || leg.pin.is_some() || leg.aero {
                        return Err(fail("a wait takes no other keys".into()));
                    }
                    if !days.is_finite() || days < 0.0 {
                        return Err(fail("a wait is a number of days, not negative".into()));
                    }
                    Ok(Step::Wait(days))
                }
                (Some(to), None) => Ok(Step::Go(Go {
                    to: spot(&to, eph, places).map_err(fail)?,
                    days: match leg.days {
                        Some(d) if d.iter().any(|&d| !d.is_finite() || d <= 0.0) => {
                            return Err(fail("durations are positive numbers of days".into()));
                        }
                        d => d.unwrap_or_default(),
                    },
                    window: leg
                        .window
                        .map(|[a, b]| {
                            let (a, b) = (date(&a)?, date(&b)?);
                            if a.0 > b.0 {
                                return Err(format!("the window opens {a}, after it closes"));
                            }
                            Ok((a, b))
                        })
                        .transpose()
                        .map_err(fail)?,
                    pin: leg
                        .pin
                        .map(|p| {
                            if p.depart.is_none() && p.days.is_none() {
                                return Err("a pin has `depart`, `days` or both".into());
                            }
                            if p.days.is_some_and(|d| !d.is_finite() || d <= 0.0) {
                                return Err("a pin's days are a positive number".into());
                            }
                            Ok(Pin {
                                depart: p.depart.as_deref().map(date).transpose()?,
                                days: p.days,
                            })
                        })
                        .transpose()
                        .map_err(fail)?,
                    aero: leg.aero,
                    note: leg.note,
                })),
                _ => Err(fail("a leg has `to` or `wait`".into())),
            }
        })
        .collect()
}

/// `YYYY-MM-DD HH:MM`, to the nearest minute.
pub fn when(t: Epoch) -> String {
    let (y, m, d) = Epoch((t.0 * 1_440.0).round() / 1_440.0).ymd();
    let minutes = ((d.fract() * 1_440.0).round() as u32).min(1_439);
    format!(
        "{y:04}-{m:02}-{:02} {:02}:{:02}",
        d.floor() as u32,
        minutes / 60,
        minutes % 60
    )
}

/// A place by id, or written out as a table.
fn spot(v: &toml::Value, eph: &Ephemeris, places: &Places) -> Result<Spot, String> {
    match v {
        toml::Value::String(id) => places
            .get(id)
            .map(|p| Spot {
                label: id.clone(),
                kind: p.kind.clone(),
                source: v.clone(),
            })
            .ok_or_else(|| format!("no place {id:?}")),
        toml::Value::Table(t) => {
            let kind = PlaceKind::from_table(t, eph, &[])?;
            Ok(Spot {
                label: describe(eph, &kind),
                kind,
                source: v.clone(),
            })
        }
        _ => Err("a place is an id or a table".into()),
    }
}

fn describe(eph: &Ephemeris, kind: &PlaceKind) -> String {
    match kind {
        PlaceKind::Low(l) => {
            let b = eph.body(l.body);
            let plane = l
                .inclination
                .map_or(String::new(), |i| format!(" at {:.0}°", i.to_degrees()));
            format!("{} r {:.0} km{plane}", b.id, l.radius)
        }
        PlaceKind::Station { family, period } => format!("{family} ({period} d)"),
        PlaceKind::Body(b) => eph.body(*b).id.clone(),
        PlaceKind::Surface(b) => format!("{} surface", eph.body(*b).id),
    }
}

/// The fields legs are designed in: one per planetary system with places
/// in it, and for legs between systems a barycentric field with fields
/// about the planets for arcs near them.
pub struct World {
    pub eph: &'static Ephemeris,
    cat: &'static Catalog,
    /// (planet, field, lighter field for seeds), centered on the planet.
    systems: Vec<(BodyId, Field<'static>, Field<'static>)>,
    global: Field<'static>,
    locals: Vec<Field<'static>>,
}

impl World {
    /// The fields M1a's gate used: about Earth, the Sun, Earth and the Moon
    /// (seeds without Earth's J2); about Mars, the Sun, Mars and Jupiter;
    /// between planets, every body, with Earth's and Mars's J2.
    pub fn standard() -> World {
        let eph = Ephemeris::standard();
        let id = |s: &str| eph.lookup(s).expect("a body in the ephemeris");
        let (earth, moon, mars) = (id("earth"), id("moon"), id("mars"));
        let cislunar = [eph.sun(), earth, moon];
        let mars_bodies = [eph.sun(), mars, id("jupiter")];
        World {
            eph,
            cat: Catalog::standard(),
            systems: vec![
                (
                    earth,
                    Field::new(eph, Some(earth), &cislunar).with_oblateness(),
                    Field::new(eph, Some(earth), &cislunar),
                ),
                (
                    mars,
                    Field::new(eph, Some(mars), &mars_bodies).with_oblateness(),
                    Field::new(eph, Some(mars), &mars_bodies).with_oblateness(),
                ),
            ],
            global: Field::all(eph, None).with_oblateness(),
            locals: vec![
                Field::all(eph, Some(earth)).with_oblateness(),
                Field::all(eph, Some(mars)).with_oblateness(),
            ],
        }
    }

    /// The fields about planet `system`.
    fn system(&self, system: BodyId) -> Result<(&Field<'static>, &Field<'static>), String> {
        self.systems
            .iter()
            .find(|s| s.0 == system)
            .map(|(_, f, l)| (f, l))
            .ok_or_else(|| format!("no field for {}'s system", self.eph.body(system).id))
    }

    /// `s` (relative to `from`, the barycenter if `None`) relative to `to`
    /// at `t`.
    fn rebase(&self, s: [f64; 6], from: Option<BodyId>, to: Option<BodyId>, t: Epoch) -> [f64; 6] {
        let at = |c: Option<BodyId>| {
            c.map_or([0.0; 6], |c| {
                let b = self.eph.barycentric(c, t);
                [b.r.x, b.r.y, b.r.z, b.v.x, b.v.y, b.v.z]
            })
        };
        let (a, b) = (at(from), at(to));
        std::array::from_fn(|d| s[d] + a[d] - b[d])
    }
}

impl World {
    /// The end a leg reaching `to` (through the atmosphere if `aero`)
    /// arrives at, other than a station; `between`, from another planet.
    fn end_to(&self, to: &PlaceKind, aero: bool, between: bool) -> Result<End<'static>, String> {
        let eph = self.eph;
        let air = |b: BodyId| {
            surface::rules(eph, b)
                .atmosphere
                .ok_or_else(|| format!("{} has no atmosphere", eph.body(b).id))
        };
        Ok(match to {
            PlaceKind::Low(orbit) if aero => {
                let a = air(orbit.body)?;
                let body = eph.body(orbit.body);
                let pass = body.radius + a.capture_altitude;
                End::Atmosphere {
                    body: orbit.body,
                    radius: pass,
                    after: surface::after_aerocapture(body.gm, pass, orbit.radius),
                    max_speed: a.max_speed,
                }
            }
            PlaceKind::Low(orbit) => End::Low(*orbit),
            PlaceKind::Body(b) => End::Body(*b),
            PlaceKind::Surface(b) if aero && between => {
                let a = air(*b)?;
                End::Atmosphere {
                    body: *b,
                    radius: eph.body(*b).radius + a.entry_altitude,
                    after: a.terminal_dv,
                    max_speed: a.max_speed,
                }
            }
            _ => return Err("unreachable end".into()),
        })
    }

    /// A leg between planets from `from` to `to` (through the atmosphere
    /// if `aero`), as its date search prices it.
    pub fn crossing(
        &self,
        from: &PlaceKind,
        to: &PlaceKind,
        aero: bool,
    ) -> Result<Crossing<'_>, String> {
        let eph = self.eph;
        between_ends(from, to)?;
        let (from_end, from_body) = match from {
            PlaceKind::Low(orbit) => (End::Low(*orbit), orbit.body),
            PlaceKind::Body(b) => (End::Body(*b), *b),
            _ => return Err("a leg between planets leaves a low orbit or a body".into()),
        };
        let to_system = to.system(eph);
        if eph.system(from_body) == to_system {
            return Err("both ends are in one planet's system".into());
        }
        Ok(Crossing {
            world: self,
            from: from_end,
            to: self.end_to(to, aero, true)?,
            from_body,
            to_system,
        })
    }
}

/// Why a leg between planets cannot leave `from` or reach `to`, if it
/// cannot.
fn between_ends(from: &PlaceKind, to: &PlaceKind) -> Result<(), String> {
    if matches!(from, PlaceKind::Station { .. }) {
        return Err(
            "leaving a station orbit for another planet is not designed yet (M1a step 9); fly through LEO"
                .into(),
        );
    }
    if matches!(to, PlaceKind::Station { .. }) {
        return Err(
            "arriving at a station orbit from another planet is not designed yet (M1a step 9)"
                .into(),
        );
    }
    Ok(())
}

/// How a leg is flown, by its ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LegKind {
    /// From a low orbit to its body's surface, and back.
    Landing,
    Ascent,
    /// Within a planet's system.
    Within(BodyId),
    Between,
}

/// How `go` is flown leaving `from`, by the planner's rules, or why it
/// cannot be (what the planner says when it reaches it).
pub fn leg_kind(eph: &Ephemeris, from: &PlaceKind, go: &Go) -> Result<LegKind, String> {
    let (from_system, to_system) = (from.system(eph), go.to.kind.system(eph));
    match (from, &go.to.kind) {
        (PlaceKind::Low(orbit), PlaceKind::Surface(b)) if orbit.body == *b => Ok(LegKind::Landing),
        (PlaceKind::Surface(b), PlaceKind::Low(orbit)) if orbit.body == *b => Ok(LegKind::Ascent),
        (PlaceKind::Surface(_), _) => {
            Err("a surface is left by ascent to a low orbit about its body".into())
        }
        (_, PlaceKind::Surface(_)) if !(go.aero && from_system != to_system) => Err(
            "a surface is reached by landing from a low orbit about its body, or by entry from another planet (`aero`)"
                .into(),
        ),
        _ if from_system == to_system => {
            if go.durations().is_empty() {
                Err("a leg within a system needs `days`".into())
            } else if go.depart().is_some() {
                Err("a pinned departure is for legs between planets and events; within a system, set a `window`".into())
            } else {
                Ok(LegKind::Within(from_system))
            }
        }
        _ => between_ends(from, &go.to.kind).map(|_| LegKind::Between),
    }
}

/// Each of `ship`'s legs that cannot be flown, by its step, and why: the
/// planner's rules, checked without designing.
pub fn check(eph: &Ephemeris, ship: &Ship) -> Vec<(usize, String)> {
    let mut at = &ship.start.kind;
    let mut out = Vec::new();
    for (i, step) in ship.steps.iter().enumerate() {
        if let Step::Go(go) = step {
            if let Err(why) = leg_kind(eph, at, go) {
                out.push((i, why));
            }
            at = &go.to.kind;
        }
    }
    out
}

/// A leg between planets, priced by patched conics (`World::crossing`).
pub struct Crossing<'w> {
    world: &'w World,
    pub from: End<'static>,
    pub to: End<'static>,
    from_body: BodyId,
    to_system: BodyId,
}

impl Crossing<'_> {
    /// Δv by patched conics, leaving at `depart` and taking `days`; `None`
    /// where the ephemeris does not know both ends then.
    pub fn dv(&self, depart: Epoch, days: f64) -> Option<f64> {
        let (eph, arrive) = (self.world.eph, depart.plus_days(days));
        if !eph.covers(self.from_body, depart) || !eph.covers(self.to_system, arrive) {
            return None;
        }
        interplanetary::patched(&self.world.global, &self.from, &self.to, depart, arrive)
    }

    /// `t` held within the span the ephemeris knows both ends over.
    fn known(&self, t: Epoch) -> Epoch {
        let eph = self.world.eph;
        let (a, b) = (eph.span_of(self.from_body), eph.span_of(self.to_system));
        Epoch(t.0.clamp(a.0.0.max(b.0.0), a.1.0.min(b.1.0)))
    }

    /// The durations the date search tries leaving at `t`, days: every
    /// [`TOF_STEP`] over the range the bodies' orbits bracket (then, or
    /// as near then as the ephemeris knows them).
    pub fn durations(&self, t: Epoch) -> Vec<f64> {
        let (lo, hi) = crate::transfer::default_tof_range(
            self.world.eph,
            self.from_body,
            self.to_system,
            self.known(t),
        );
        let n = ((hi - lo) / TOF_STEP).floor() as usize;
        (0..=n).map(|k| lo + k as f64 * TOF_STEP).collect()
    }

    /// The synodic period of its two ends about `t` (or as near it as
    /// the ephemeris knows them), days.
    pub fn synodic_days(&self, t: Epoch) -> f64 {
        self.world
            .eph
            .synodic_period_days(self.from_body, self.to_system, self.known(t))
    }
}

/// What a ship does over a span of time.
#[derive(Clone, Debug)]
pub struct Item {
    /// The ship's step that made it: the stay before a leg is the leg's.
    pub step: usize,
    pub kind: ItemKind,
    pub label: String,
    pub start: Epoch,
    pub end: Epoch,
    /// Total Δv, km/s.
    pub dv: f64,
    /// The path, where one is flown (or followed, in a stay).
    pub track: Option<Track>,
    pub markers: Vec<Marker>,
    /// The leg's options: its offered durations (or dates tried, between
    /// planets) and what each cost, km/s.
    pub options: Vec<Choice>,
    /// A leg's departure window as its search had it, without a pin.
    pub window: Option<(Epoch, Epoch)>,
    /// Designing it, s.
    pub seconds: f64,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    Leg,
    /// Coasting in a low orbit, following a station's orbit (and keeping
    /// it), or at a body or on its surface.
    Stay,
    /// Coasting from an aerocapture's pass up to the low orbit, and
    /// circularizing there.
    Capture,
    Landing,
    Ascent,
    /// The plan stops here: the leg could not be designed.
    Failed(String),
}

#[derive(Clone, Copy, Debug)]
pub struct Choice {
    pub depart: Epoch,
    pub days: f64,
    pub dv: Option<f64>,
}

#[derive(Clone, Copy, Debug)]
pub struct Marker {
    pub t: Epoch,
    pub kind: MarkerKind,
    /// km/s.
    pub dv: f64,
    /// The body passed, and the altitude above its mean radius, km.
    pub pass: Option<(BodyId, f64)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MarkerKind {
    Burn,
    /// A burn at a flyby's periapsis.
    Flyby,
    /// Through an atmosphere: an aerocapture, or an entry to the surface.
    Pass,
    Landing,
    Ascent,
    /// A leg leaving a low orbit that does not start from the ship's own
    /// orbit there (M1c.md, step 1 plan, item 4), at its departure: the
    /// gap, km, and the plane change it would take, as `dv`.
    Jump {
        km: f64,
        plane_deg: f64,
    },
}

/// Where a [`Planner`] is between steps: the step it goes on from, and
/// where the ship is then.
#[derive(Clone)]
pub struct Checkpoint {
    next: usize,
    place: PlaceKind,
    here: Here,
    label: String,
    since: Epoch,
    ready: Epoch,
    not_before: Epoch,
    stopped: bool,
}

impl Checkpoint {
    /// The step it goes on from.
    pub fn next(&self) -> usize {
        self.next
    }

    /// When the ship reached where it is.
    pub fn since(&self) -> Epoch {
        self.since
    }

    /// This checkpoint, with the ship staying where it is until at least
    /// `now` (M1b.md, step 1 plan, part 3): the steps from here leave no
    /// earlier, their waits counted from its arrival. In a low orbit whose
    /// plane was still free, `stay` (the stay designed from here, which
    /// the ship has flown until now) fixes the plane, and the ship leaves
    /// from that orbit.
    pub fn staying(&self, now: Epoch, stay: Option<&Item>) -> Checkpoint {
        let mut c = self.clone();
        c.not_before = Epoch(c.not_before.0.max(now.0));
        if let Here::Low { orbit, coast: None } = &c.here
            && let Some(track) = stay.and_then(|i| i.track.as_ref())
            && let (Some(&t), Some(&s)) = (track.times.first(), track.states.first())
        {
            c.here = Here::Low {
                orbit: *orbit,
                coast: Some((track.epoch.plus_days(t / DAY), s)),
            };
        }
        c
    }
}

/// The steps the planner reads designing the step from `next` (a
/// checkpoint's): from it through the leg the step designs, and through
/// the leg after that when the step reaches a station (its visit is built
/// long enough for them, [`Planner`]'s `visit_until`). From the start
/// (`None`), the steps a station start's visit is built for. Its end:
/// what designs are keyed by (M1b.md, step 1 plan, part 4).
pub fn reads(ship: &Ship, next: Option<usize>) -> usize {
    let after = |from: usize| -> usize {
        ship.steps[from.min(ship.steps.len())..]
            .iter()
            .position(|s| matches!(s, Step::Go(_)))
            .map_or(ship.steps.len(), |k| from + k + 1)
    };
    let at_station = |kind: &PlaceKind| matches!(kind, PlaceKind::Station { .. });
    match next {
        None if at_station(&ship.start.kind) => after(0),
        None => 0,
        Some(next) => {
            let end = after(next);
            match ship.steps.get(end.wrapping_sub(1)) {
                Some(Step::Go(go)) if end > next && at_station(&go.to.kind) => after(end),
                _ => end,
            }
        }
    }
}

/// Of `checkpoints` (by their steps, ascending), the one to go on from
/// after the ship's steps from `edited` on change: the last before the
/// edit; but if the ship is in a station's orbit there, the one before
/// (the visit was built long enough for the steps after it: its waits,
/// and the next leg's window and durations); and none where the plan had
/// stopped (a failure may have hung on the steps after it). `None`: from
/// the start.
pub fn resume_from(checkpoints: &[Checkpoint], edited: usize) -> Option<&Checkpoint> {
    let mut k = checkpoints.iter().rposition(|c| c.next <= edited)?;
    if matches!(checkpoints[k].here, Here::Station(_)) {
        k = k.checked_sub(1)?;
    }
    while checkpoints[k].stopped {
        k = k.checked_sub(1)?;
    }
    Some(&checkpoints[k])
}

/// A design a leg flies, and the reference of the ship's own low orbit it
/// leaves, if it was designed from it, with a note on how.
struct Flight {
    design: Design,
    own: Option<Reference>,
    note: Option<String>,
}

/// A station orbit for one visit: the reference the ship follows from the
/// leg that arrives until the leg that leaves.
struct Visit {
    reference: Reference,
    label: String,
}

/// Where the ship is between items.
#[derive(Clone)]
enum Here {
    /// In a low orbit, coasting from a state (relative to its system's
    /// field, s after the epoch) where a flown leg left it; `None` where its
    /// plane is still free (after a launch or an ascent).
    Low {
        orbit: LowOrbit,
        coast: Option<(Epoch, [f64; 6])>,
    },
    Station(Arc<Visit>),
    Body(BodyId),
    Surface(BodyId),
}

/// The end a ship leaves from.
fn end_from(here: &Here) -> End<'_> {
    match here {
        Here::Low { orbit, .. } => End::Low(*orbit),
        Here::Body(b) => End::Body(*b),
        Here::Station(v) => End::Orbit(&v.reference),
        Here::Surface(_) => unreachable!("surfaces are left by events"),
    }
}

/// Designs one ship's plan, an item at a time.
pub struct Planner<'w> {
    world: &'w World,
    ship: Ship,
    next: usize,
    /// The step being designed.
    at: usize,
    /// The place the ship is at.
    place: PlaceKind,
    here: Here,
    label: String,
    /// When the ship reached where it is, and when it may leave.
    since: Epoch,
    ready: Epoch,
    /// No step leaves before this (career's now when it was re-planned).
    not_before: Epoch,
    stopped: bool,
}

impl<'w> Planner<'w> {
    pub fn new(world: &'w World, ship: &Ship) -> Result<Planner<'w>, String> {
        let mut p = Planner {
            world,
            ship: ship.clone(),
            next: 0,
            at: 0,
            place: ship.start.kind.clone(),
            here: Here::Body(world.eph.sun()),
            label: ship.start.label.clone(),
            since: ship.epoch,
            ready: ship.epoch,
            not_before: Epoch(f64::NEG_INFINITY),
            stopped: false,
        };
        p.here = match &ship.start.kind {
            PlaceKind::Low(orbit) => Here::Low {
                orbit: *orbit,
                coast: None,
            },
            PlaceKind::Station { family, period } => {
                let until = p.visit_until(0, ship.epoch, p.member_days(family, *period)?);
                Here::Station(p.visit(&ship.start, ship.epoch, until)?)
            }
            PlaceKind::Body(b) => Here::Body(*b),
            PlaceKind::Surface(b) => Here::Surface(*b),
        };
        Ok(p)
    }

    /// Where it is between steps, to go on from (with the steps from
    /// there edited) by [`Planner::resume`].
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            next: self.next,
            place: self.place.clone(),
            here: self.here.clone(),
            label: self.label.clone(),
            since: self.since,
            ready: self.ready,
            not_before: self.not_before,
            stopped: self.stopped,
        }
    }

    /// Goes on from `checkpoint`, one of this ship's, with `ship`'s steps
    /// (from the checkpoint's on, edited or not).
    pub fn resume(world: &'w World, ship: &Ship, checkpoint: Checkpoint) -> Planner<'w> {
        Planner {
            world,
            ship: ship.clone(),
            next: checkpoint.next,
            at: checkpoint.next,
            place: checkpoint.place,
            here: checkpoint.here,
            label: checkpoint.label,
            since: checkpoint.since,
            ready: checkpoint.ready,
            not_before: checkpoint.not_before,
            stopped: checkpoint.stopped,
        }
    }

    /// Why the ship cannot go on from here as its steps now are, if it
    /// cannot: in a station's orbit, the steps need a longer visit than
    /// the orbit was built for. A planner going on from a checkpoint
    /// `resume_from` would have stepped back from meets this; M1b's step
    /// 3 makes each station one orbit.
    pub fn outstays(&self) -> Option<String> {
        let (Here::Station(visit), PlaceKind::Station { family, period }) =
            (&self.here, &self.place)
        else {
            return None;
        };
        let revolution = self.member_days(family, *period).ok()?;
        let until = self.visit_until(self.next, self.ready, revolution);
        let built = visit.reference.end().plus_days(-VISIT_MARGIN_DAYS);
        (until.0 > built.0).then(|| {
            format!(
                "{}'s orbit was built for a visit until {}, and the steps from here need one until {}",
                visit.label,
                when(built),
                when(until)
            )
        })
    }

    /// The next items: a stay and the leg or event after it, or a failure
    /// (after which there are none). `None` when the plan is done.
    pub fn step(&mut self) -> Option<Vec<Item>> {
        // Checked where the planner stands, before the waits.
        let mut refused = self.outstays();
        while !self.stopped {
            let i = self.next;
            self.next += 1;
            self.at = i;
            match self.ship.steps.get(i)?.clone() {
                Step::Wait(days) => self.ready = self.ready.plus_days(days),
                Step::Go(go) => {
                    let clock = Instant::now();
                    self.ready = Epoch(self.ready.0.max(self.not_before.0));
                    let gone = match refused.take() {
                        Some(why) => Err(why),
                        None => self.go(&go),
                    };
                    return Some(match gone {
                        Ok(mut items) => {
                            if let Some(i) = items.iter_mut().rfind(|i| i.kind != ItemKind::Capture)
                            {
                                i.seconds = clock.elapsed().as_secs_f64();
                            }
                            items
                        }
                        Err(why) => {
                            self.stopped = true;
                            vec![Item {
                                step: self.at,
                                kind: ItemKind::Failed(why),
                                label: format!("{} → {}", self.label, go.to.label),
                                start: self.ready,
                                end: self.ready,
                                dv: 0.0,
                                track: None,
                                markers: Vec::new(),
                                options: Vec::new(),
                                window: None,
                                seconds: clock.elapsed().as_secs_f64(),
                                note: go.note.clone(),
                            }]
                        }
                    });
                }
            }
        }
        None
    }

    /// The departure window for `go`, given its default length in days: an
    /// instant where its departure is pinned (held to when the ship is
    /// ready, if within half a minute of it: pins are kept to the minute).
    fn window(&self, go: &Go, default_days: f64) -> Result<(Epoch, Epoch), String> {
        if let Some(depart) = go.depart() {
            if depart.0 < self.ready.0 - PIN_SLACK_DAYS {
                return Err(format!(
                    "the leg is pinned to leave {}, before the ship is ready ({})",
                    when(depart),
                    when(self.ready)
                ));
            }
            let depart = Epoch(depart.0.max(self.ready.0));
            return Ok((depart, depart));
        }
        self.unpinned(go, default_days)
    }

    /// The departure window for `go` without its pin.
    fn unpinned(&self, go: &Go, default_days: f64) -> Result<(Epoch, Epoch), String> {
        match go.window {
            Some((open, close)) => {
                if close.0 < self.ready.0 {
                    return Err(format!(
                        "the window closes {close}, before the ship is ready ({})",
                        self.ready
                    ));
                }
                Ok((Epoch(open.0.max(self.ready.0)), close))
            }
            None => Ok((self.ready, self.ready.plus_days(default_days))),
        }
    }

    fn go(&mut self, go: &Go) -> Result<Vec<Item>, String> {
        let eph = self.world.eph;
        // A small body's orbit is known from 2040.
        let (open, _) = self.window(go, 0.0)?;
        let from = match self.here {
            Here::Body(b) => Some(b),
            _ => None,
        };
        let to = match go.to.kind {
            PlaceKind::Body(b) => Some(b),
            _ => None,
        };
        if let Some(b) = [from, to]
            .into_iter()
            .flatten()
            .find(|&b| !eph.covers(b, open))
        {
            let (first, last) = eph.span_of(b);
            return Err(format!(
                "{}'s orbit is known from {first} to {last}, not at {open}",
                eph.body(b).id
            ));
        }
        let kind = leg_kind(eph, &self.place, go)?;
        let items = match (kind, &self.place, &go.to.kind) {
            (LegKind::Landing, PlaceKind::Low(orbit), PlaceKind::Surface(b)) => {
                self.surface_event(go, *b, orbit.radius, true)
            }
            (LegKind::Ascent, PlaceKind::Surface(b), PlaceKind::Low(orbit)) => {
                let (b, radius) = (*b, orbit.radius);
                self.surface_event(go, b, radius, false)
            }
            (LegKind::Within(system), _, _) => self.within(go, system),
            (LegKind::Between, _, _) => self.between(go),
            _ => unreachable!("leg_kind's kinds match their ends"),
        }?;
        self.place = go.to.kind.clone();
        Ok(items)
    }

    /// Landing on (`down`) or ascending from a body's surface, from or to a
    /// low orbit of `radius`.
    fn surface_event(
        &mut self,
        go: &Go,
        body: BodyId,
        radius: f64,
        down: bool,
    ) -> Result<Vec<Item>, String> {
        let eph = self.world.eph;
        let (start, _) = self.window(go, 0.0)?;
        let (kind, marker, dv) = if down {
            let dv = if go.aero {
                surface::entry_from_orbit(eph, body, radius)
                    .ok_or_else(|| format!("{} has no atmosphere", eph.body(body).id))?
            } else {
                surface::descent(eph, body, radius)
            };
            (ItemKind::Landing, MarkerKind::Landing, dv)
        } else {
            (
                ItemKind::Ascent,
                MarkerKind::Ascent,
                surface::ascent(eph, body, radius),
            )
        };
        let end = start.plus_days(SURFACE_DAYS);
        let (mut items, _) = self.stay(start, None)?;
        items.push(Item {
            step: self.at,
            kind,
            label: format!("{} → {}", self.label, go.to.label),
            start,
            end,
            dv,
            track: None,
            markers: vec![Marker {
                t: start,
                kind: marker,
                dv,
                pass: None,
            }],
            options: Vec::new(),
            window: None,
            seconds: 0.0,
            note: go.note.clone(),
        });
        self.here = match &go.to.kind {
            PlaceKind::Low(orbit) => Here::Low {
                orbit: *orbit,
                coast: None,
            },
            _ => Here::Surface(body),
        };
        self.arrive(&go.to.label, end);
        Ok(items)
    }

    fn arrive(&mut self, label: &str, at: Epoch) {
        self.label = label.to_string();
        self.since = at;
        self.ready = at;
    }

    /// A leg within planet `system`'s field, by `legs::plan`.
    fn within(&mut self, go: &Go, system: BodyId) -> Result<Vec<Item>, String> {
        let world = self.world;
        let eph = world.eph;
        let (field, light) = world.system(system)?;
        let offered = go.durations();
        let longest = go.longest();
        // The window: a revolution of the station left or reached, else
        // the defaults.
        let default_days = match (&self.here, &go.to.kind) {
            (Here::Station(v), _) => v.reference.period_s() / DAY,
            (_, PlaceKind::Station { family, period }) => self.member_days(family, *period)?,
            _ if Some(system) == eph.lookup("earth") => EARTH_WINDOW_DAYS,
            _ => OTHER_WINDOW_DAYS,
        };
        let (open, close) = self.window(go, default_days)?;
        if close.0 <= open.0 {
            return Err("a leg within a system needs a window longer than an instant".into());
        }
        // Seeds sample the window at a few dozen times, and a few percent
        // of windows find none that converge (M1a's gate sweep: 98.2% for
        // low lunar orbit to the NRHO). A window that fails is tried again
        // opened a quarter of its length later (and closed later, unless
        // the plan set it).
        let length = close.0 - open.0;
        let windows: Vec<(Epoch, Epoch)> = (0..WITHIN_TRIES)
            .map(|k| {
                let shift = k as f64 * length / 4.0;
                let late = if go.window.is_some() { 0.0 } else { shift };
                (open.plus_days(shift), close.plus_days(late))
            })
            .enumerate()
            .filter(|(k, (o, c))| *k == 0 || o.0 < c.0)
            .map(|(_, w)| w)
            .collect();
        // Leaving the ship's own low orbit, the leg may wait for the planes
        // to align: up to a period of the moon its designs move with,
        // unless the plan sets the window.
        let wait = matches!(self.here, Here::Low { coast: Some(_), .. })
            .then(|| {
                let met = match go.to.kind {
                    PlaceKind::Body(b) => Some(b),
                    _ => None,
                };
                legs::wait_limit(field, met, open)
            })
            .flatten()
            .map(|limit| match go.window {
                Some(_) => close,
                None => Epoch(open.plus_days(limit / DAY).0.max(close.0)),
            });
        // A station reached is followed from before the earliest arrival
        // to after the leg that leaves it.
        let arriving = match &go.to.kind {
            PlaceKind::Station { family, period } => {
                let last = windows.last().unwrap().1;
                let latest = Epoch(wait.map_or(last.0, |w| w.0.max(last.0))).plus_days(longest);
                let until =
                    self.visit_until(self.at + 1, latest, self.member_days(family, *period)?);
                Some(self.visit(&go.to, open, until)?)
            }
            _ => None,
        };
        let here = self.here.clone();
        let from = end_from(&here);
        let to = match &arriving {
            Some(v) => End::Orbit(&v.reference),
            None => self.world.end_to(&go.to.kind, go.aero, false)?,
        };
        let durations: Vec<f64> = offered.iter().map(|d| d * DAY).collect();
        let leg = Leg {
            field,
            locals: &[],
            from,
            to,
            depart: None,
            arrive: None,
            duration: None,
            window: None,
        };
        for (k, &(o, c)) in windows.iter().enumerate() {
            // Each duration designed alone: designed in one call, durations
            // share seeds drawn over all of them, and a duration's design
            // depends on which others are asked for (from the NRHO to low
            // lunar orbit, 1.65–4 days cost 675–692 m/s alone and up to
            // 1,259 together). Alone, an option is what pinning it flies.
            let mut flights: Vec<Option<Flight>> = durations
                .par_iter()
                .map(|&d| {
                    let window = (0.0, (c.0 - o.0) * DAY);
                    let free = legs::plan(field, light, o, from, to, window, &[d])
                        .pop()
                        .flatten()?;
                    Some(self.leave(&leg, free, (o, c), wait))
                })
                .collect();
            let options = offered
                .iter()
                .zip(&flights)
                .map(|(&days, f)| Choice {
                    depart: f.as_ref().map_or(o, |f| f.design.departure_epoch()),
                    days,
                    dv: f.as_ref().map(|f| f.design.dv),
                })
                .collect();
            // The cheapest from the ship's own orbit; else the cheapest.
            let cheapest = |own: bool| {
                (0..flights.len())
                    .filter_map(|i| {
                        let f = flights[i].as_ref()?;
                        (f.own.is_some() == own).then_some((i, f.design.dv))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1))
            };
            let Some((i, _)) = cheapest(true).or_else(|| cheapest(false)) else {
                continue;
            };
            let Flight {
                design,
                own,
                note: own_note,
            } = flights[i].take().unwrap();
            let retried = (k > 0).then(|| {
                format!(
                    "no design converged leaving {}–{}; this window opens {}",
                    when(open),
                    when(close),
                    when(o)
                )
            });
            let note = [retried, own_note, go.note.clone()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
            let note = (!note.is_empty()).then(|| note.join("; "));
            let leg = match &own {
                Some(r) => Leg {
                    from: End::Orbit(r),
                    ..leg
                },
                None => leg,
            };
            let mut items = self.flown(go, &leg, design, options, arriving.clone(), note)?;
            set_window(&mut items, (open, close));
            return Ok(items);
        }
        Err(format!(
            "no design converged in {} windows from {}",
            windows.len(),
            when(open)
        ))
    }

    /// A leg between planetary systems: the cheapest dates by patched
    /// conics, then `interplanetary::plan`.
    fn between(&mut self, go: &Go) -> Result<Vec<Item>, String> {
        let world = self.world;
        let crossing = world.crossing(&self.place, &go.to.kind, go.aero)?;
        let default_days = crossing.synodic_days(self.ready).min(LONGEST_WINDOW_DAYS);
        let (open, close) = self.window(go, default_days)?;
        // The window without the pin (a pin overrides a window it is
        // outside of).
        let searched = self.unpinned(go, default_days).unwrap_or((open, close));
        let here = self.here.clone();
        let from = end_from(&here);
        let to = crossing.to;
        let tofs: Vec<f64> = match go.durations() {
            d if d.is_empty() => crossing.durations(open),
            d => d,
        };
        let departs = ((close.0 - open.0) / DEPART_STEP).floor() as usize;
        let global = &world.global;
        let crossing = &crossing;
        let mut cells: Vec<(f64, Epoch, f64)> = (0..=departs)
            .into_par_iter()
            .flat_map_iter(|k| {
                let d = open.plus_days(k as f64 * DEPART_STEP);
                tofs.iter()
                    .filter_map(move |&tof| Some((crossing.dv(d, tof)?, d, tof)))
            })
            .collect();
        cells.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut tried: Vec<Choice> = Vec::new();
        let mut flown = None;
        for &(_, depart, tof) in &cells {
            if tried.len() == TRIES || flown.is_some() {
                break;
            }
            // Designs tried leave days apart; at a pinned date, any.
            if go.depart().is_none()
                && tried
                    .iter()
                    .any(|c| (c.depart.0 - depart.0).abs() < TRIES_APART_DAYS)
            {
                continue;
            }
            let d = interplanetary::plan(
                global,
                &world.locals,
                from,
                to,
                depart,
                depart.plus_days(tof),
            );
            tried.push(Choice {
                depart,
                days: tof,
                dv: d.as_ref().map(|d| d.dv),
            });
            flown = d;
        }
        let Some(free) = flown else {
            return Err(if cells.is_empty() {
                "no dates in the window".into()
            } else {
                format!("no design converged on the {} cheapest dates", tried.len())
            });
        };
        let leg = Leg {
            field: global,
            locals: &world.locals,
            from,
            to,
            depart: None,
            arrive: None,
            duration: None,
            window: None,
        };
        // From the ship's own orbit, its phase picks the departure (within
        // half a revolution of the date); the date is not moved to wait
        // for the planes.
        let Flight {
            design,
            own,
            note: own_note,
        } = self.leave(&leg, free, (open, Epoch(f64::INFINITY)), None);
        if let Some(last) = tried.last_mut() {
            last.depart = design.departure_epoch();
            last.dv = Some(design.dv);
        }
        let leg = match &own {
            Some(r) => Leg {
                from: End::Orbit(r),
                ..leg
            },
            None => leg,
        };
        let note = [own_note, go.note.clone()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let note = (!note.is_empty()).then(|| note.join("; "));
        let mut items = self.flown(go, &leg, design, tried, None, note)?;
        set_window(&mut items, searched);
        Ok(items)
    }

    /// A station's member's period, days.
    fn member_days(&self, family: &str, period: f64) -> Result<f64, String> {
        let m = self
            .world
            .cat
            .nearest(family, period)
            .ok_or_else(|| format!("no family {family:?}"))?;
        Ok(self.world.cat.days(m.orbit))
    }

    /// When a visit to a station of `revolution` days, reached before step
    /// `next` (by `latest`), must last until: through the waits after it,
    /// and the next leg's windows (a revolution, and the later ones it may
    /// try) and longest duration.
    fn visit_until(&self, next: usize, latest: Epoch, revolution: f64) -> Epoch {
        let mut t = latest;
        for step in &self.ship.steps[next.min(self.ship.steps.len())..] {
            match step {
                Step::Wait(days) => t = t.plus_days(*days),
                Step::Go(go) => {
                    let t = Epoch(t.0.max(self.not_before.0));
                    let longest = go.longest();
                    let tries = 1.0 + (WITHIN_TRIES - 1) as f64 / 4.0;
                    let close = match go.window {
                        Some((_, close)) => Epoch(close.0.max(t.0)),
                        None => t.plus_days(tries * revolution),
                    };
                    return close.plus_days(longest);
                }
            }
        }
        t
    }

    /// The reference for a visit to station `spot` from `from` until
    /// `until`, with margins.
    fn visit(&self, spot: &Spot, from: Epoch, until: Epoch) -> Result<Arc<Visit>, String> {
        let PlaceKind::Station { family, period } = &spot.kind else {
            unreachable!("a visit is to a station")
        };
        let (eph, cat) = (self.world.eph, self.world.cat);
        let m = cat
            .nearest(family, *period)
            .ok_or_else(|| format!("no family {family:?}"))?;
        let days = cat.days(m.orbit);
        let (field, _) = self.world.system(eph.lookup("earth").unwrap())?;
        let span = until.0 - from.0 + 2.0 * VISIT_MARGIN_DAYS;
        // A start that does not converge is retried a day or two earlier.
        (0..3)
            .find_map(|early| {
                let start = from.plus_days(-VISIT_MARGIN_DAYS - early as f64);
                let revs = ((span + early as f64) / days).ceil() as usize + 1;
                continue_orbit(eph, field, cat, m.orbit, start, revs, 16).ok()
            })
            .map(|reference| {
                Arc::new(Visit {
                    reference,
                    label: spot.label.clone(),
                })
            })
            .ok_or_else(|| format!("{}'s orbit could not be continued from {from}", spot.label))
    }

    /// The items for a designed leg: the stay before it, the leg, and an
    /// aerocapture's coast after it; the ship is then where it arrived.
    fn flown(
        &mut self,
        go: &Go,
        leg: &Leg,
        design: Design,
        options: Vec<Choice>,
        arriving: Option<Arc<Visit>>,
        note: Option<String>,
    ) -> Result<Vec<Item>, String> {
        let eph = self.world.eph;
        let path = &design.path;
        let when = |t: f64| path.epoch.plus_days(t / DAY);
        let (t0, t1) = (when(path.times[0]), when(*path.times.last().unwrap()));
        let track = leg
            .track(&design)
            .ok_or("the design could not be integrated again")?;
        let center = leg.field.center();
        // The state leaving, before the departure burn.
        let leaving = {
            let s = path.states[0];
            let b = design.burns[0].dv;
            [s[0], s[1], s[2], s[3] - b.x, s[4] - b.y, s[5] - b.z]
        };
        // A design from the ship's own low orbit leaves where the ship is.
        let from_own = matches!((&self.here, leg.from), (Here::Low { .. }, End::Orbit(_)));
        let (mut items, jumped) = self.stay(t0, (!from_own).then_some((leaving, center)))?;
        // Burns and flybys.
        let burn_nodes: Vec<usize> = (0..path.times.len()).filter(|&k| path.burns[k]).collect();
        let mut markers: Vec<Marker> = design
            .burns
            .iter()
            .zip(&burn_nodes)
            .map(|(b, &k)| {
                let pass = path.passes.iter().find(|p| p.node == k).map(|p| {
                    let s = path.states[k];
                    let r =
                        DVec3::new(s[0], s[1], s[2]) - leg.field.body_position(p.body, when(b.t));
                    (p.body, r.length() - eph.body(p.body).radius)
                });
                Marker {
                    t: when(b.t),
                    kind: if pass.is_some() {
                        MarkerKind::Flyby
                    } else {
                        MarkerKind::Burn
                    },
                    dv: b.dv.length(),
                    pass,
                }
            })
            .collect();
        if let Some(j) = jumped {
            markers.insert(0, j);
        }
        let arrival = *track.states.last().unwrap();
        let end_burn = design.burns.last().unwrap().dv;
        let mut capture = None;
        if let End::Atmosphere { body, after, .. } = leg.to {
            let last = markers.last_mut().unwrap();
            last.kind = MarkerKind::Pass;
            let r =
                DVec3::new(arrival[0], arrival[1], arrival[2]) - leg.field.body_position(body, t1);
            last.pass = Some((body, r.length() - eph.body(body).radius));
            if let PlaceKind::Low(orbit) = &go.to.kind {
                let s = self
                    .world
                    .rebase(arrival, center, Some(eph.system(body)), t1);
                capture = Some((*orbit, s, after));
            }
        }
        items.push(Item {
            step: self.at,
            kind: ItemKind::Leg,
            label: format!("{} → {}", self.label, go.to.label),
            start: t0,
            end: t1,
            dv: design.dv,
            track: Some(track),
            markers,
            options,
            window: None,
            seconds: 0.0,
            note,
        });
        let mut end = t1;
        self.here = match (&go.to.kind, arriving) {
            (_, Some(visit)) => Here::Station(visit),
            (PlaceKind::Low(orbit), None) => match capture {
                Some((orbit, s, after)) => {
                    let (item, at, state) = self.capture(orbit, t1, s, after)?;
                    end = at;
                    items.push(item);
                    Here::Low {
                        orbit,
                        coast: Some((at, state)),
                    }
                }
                None => {
                    let system = Some(eph.system(orbit.body));
                    let s = [
                        arrival[0],
                        arrival[1],
                        arrival[2],
                        arrival[3] + end_burn.x,
                        arrival[4] + end_burn.y,
                        arrival[5] + end_burn.z,
                    ];
                    Here::Low {
                        orbit: *orbit,
                        coast: Some((t1, self.world.rebase(s, center, system, t1))),
                    }
                }
            },
            (PlaceKind::Body(b), None) => Here::Body(*b),
            (PlaceKind::Surface(b), None) => Here::Surface(*b),
            (PlaceKind::Station { .. }, None) => unreachable!("stations are visits"),
        };
        self.arrive(&go.to.label, end);
        Ok(items)
    }

    /// `free`, a design of `leg`, as it flies: from the ship's own low
    /// orbit if a flown leg left it there ([`Planner::own_orbit`]), else
    /// as it is.
    fn leave(
        &self,
        leg: &Leg,
        free: Design,
        window: (Epoch, Epoch),
        wait: Option<Epoch>,
    ) -> Flight {
        let Here::Low {
            orbit,
            coast: Some(coast),
        } = &self.here
        else {
            return Flight {
                design: free,
                own: None,
                note: None,
            };
        };
        match self.own_orbit(leg, (*orbit, *coast), &free, window, wait) {
            Ok((design, own, note)) => Flight {
                design,
                own: Some(own),
                note,
            },
            Err(why) => Flight {
                design: free,
                own: None,
                note: Some(why),
            },
        }
    }

    /// `free`, a design of `leg` leaving the ship's low orbit with its plane
    /// free, designed again from the ship's own orbit (`own`: the orbit,
    /// and where a flown leg left the ship in it), its duration kept: at
    /// once, from where the ship passes nearest its departure, leaving
    /// within `window`; within a system, also about the first times the
    /// planes align, by `wait` ([`legs::aligned`]). The first costing at
    /// most [`PLANE_FAIR`] more, else the cheapest if at most
    /// [`PLANE_MOST`] more: the design, the reference of the ship's orbit
    /// it leaves, and a note; `Err`, why it does not leave from there (the
    /// free design flies, and the jump is reported).
    fn own_orbit(
        &self,
        leg: &Leg,
        (orbit, coast): (LowOrbit, (Epoch, [f64; 6])),
        free: &Design,
        window: (Epoch, Epoch),
        wait: Option<Epoch>,
    ) -> Result<(Design, Reference, Option<String>), String> {
        let eph = self.world.eph;
        let (field, _) = self.world.system(eph.system(orbit.body))?;
        let period =
            2.0 * std::f64::consts::PI * (orbit.radius.powi(3) / eph.body(orbit.body).gm).sqrt();
        let left = free.departure_epoch();
        let departure = free.burns[0].dv.length();
        let more = |d: &Design| d.dv - free.dv;
        // `free` moved `dt` s later, leaving within `bounds` and a
        // revolution of where it was moved to. (The ship's orbit is coasted
        // from where it arrived each time, as its stay is: coasted from a
        // state found on the way, the two parted by 1.3 m over 18 days,
        // and it saved no measurable time.)
        let design = |dt: f64, bounds: (Epoch, Epoch)| -> Option<(Design, Reference)> {
            let around = left.plus_days(dt / DAY);
            let lo = Epoch(bounds.0.0.max(around.0 - period / DAY));
            let hi = Epoch(bounds.1.0.min(around.0 + period / DAY));
            if lo.0 > hi.0 {
                return None;
            }
            let (from, until) = (lo.plus_days(-period / DAY), hi.plus_days(period / DAY));
            let reference = own_reference(field, coast, from, until, period).ok()?;
            let from_own = Leg {
                from: End::Orbit(&reference),
                depart: None,
                arrive: None,
                duration: Some(free.duration()),
                window: None,
                ..*leg
            };
            let seed = legs::own_orbit_seed(&from_own, orbit.body, period, free, dt, (lo, hi))?;
            let after = |t: Epoch| (t.0 - seed.epoch.0) * DAY;
            let d = Leg {
                window: Some((after(lo), after(hi))),
                ..from_own
            }
            .design(&seed)
            .ok()?;
            Some((d, reference))
        };
        let costs = |d: &Design| match more(d) {
            m if m >= 0.0 => format!("{:.1} m/s more than with its plane free", m * 1e3),
            m => format!("{:.1} m/s less than with its plane free", -m * 1e3),
        };
        let now = design(0.0, window);
        if let Some((d, _)) = &now
            && more(d) <= PLANE_FAIR * departure
        {
            let note = (more(d).abs() > 1e-3).then(|| format!("from its own orbit, {}", costs(d)));
            let (d, r) = now.unwrap();
            return Ok((d, r, note));
        }
        // Waiting for the planes to align: about each of the first times
        // they do, moved a little either way (where it costs least is not
        // quite where they align: the moon's pull bends the path).
        let met = match leg.to {
            End::Body(b) => Some(b),
            _ => None,
        };
        // Designs about each time the planes align, in order.
        let mut later: Vec<Vec<(Design, Reference)>> = Vec::new();
        if let Some(latest) = wait.filter(|w| w.0 > left.0)
            && let Some(limit) = legs::wait_limit(field, met, left)
        {
            let times: Vec<Epoch> = (0..=ALIGN_SAMPLES)
                .map(|k| left.plus_days((latest.0 - left.0) * k as f64 / ALIGN_SAMPLES as f64))
                .collect();
            // A coast that fails leaves the leg to what it found at once.
            let states = coast_to(field, coast.0, coast.1, &times).unwrap_or_default();
            let samples: Vec<(Epoch, [f64; 6])> = times.into_iter().zip(states).collect();
            // Steps shorter than a revolution of the ship's orbit all land
            // on its same pass (about Phobos, whose period is hours): the
            // time the planes align alone.
            let steps: &[f64] = match limit * ALIGN_STEPS[3] < period {
                true => &[0.0],
                false => &ALIGN_STEPS,
            };
            later = legs::aligned(field, orbit.body, &leg.to, free, &samples, ALIGN_TIMES)
                .into_iter()
                .map(|(k, _)| {
                    let dt = (samples[k].0.0 - left.0) * DAY;
                    steps
                        .par_iter()
                        .map(|step| dt + step * limit)
                        .filter(|&dt| dt > 0.0)
                        .filter_map(|dt| design(dt, (window.0, latest)))
                        .collect()
                })
                .collect();
        }
        let (at_once, more_at_once) = match &now {
            Some((d, _)) => (
                format!("{:.1} m/s at once", d.dv * 1e3),
                format!("{:+.1} m/s at once", more(d) * 1e3),
            ),
            None => (
                "none converged at once".to_string(),
                "none converged at once".to_string(),
            ),
        };
        let cheapest = |designs: Vec<(Design, Reference)>| {
            designs.into_iter().min_by(|a, b| a.0.dv.total_cmp(&b.0.dv))
        };
        // The cheapest about the first time the planes align that has a
        // fair design; else the cheapest of all.
        let fair = later
            .iter()
            .position(|g| g.iter().any(|(d, _)| more(d) <= PLANE_FAIR * departure));
        let best = match fair {
            Some(k) => cheapest(later.swap_remove(k)),
            None => cheapest(now.into_iter().chain(later.into_iter().flatten()).collect()),
        };
        match best {
            Some((d, r)) if more(&d) <= PLANE_MOST * departure => {
                let days = d.departure_epoch().0 - left.0;
                let note = if days > period / DAY {
                    format!(
                        "from its own orbit, {days:.1} days later, when the planes align, \
                         {} ({more_at_once})",
                        costs(&d)
                    )
                } else {
                    format!("from its own orbit, {}", costs(&d))
                };
                Ok((d, r, Some(note)))
            }
            best => Err(format!(
                "from its own orbit: {at_once}{}, against {:.1} m/s with its plane free",
                best.map_or(String::new(), |(d, _)| {
                    format!(
                        ", at best {:.1} leaving {}",
                        d.dv * 1e3,
                        when(d.departure_epoch())
                    )
                }),
                free.dv * 1e3
            )),
        }
    }

    /// After an aerocapture's pass at `s` (relative to the system's planet)
    /// at `t`: the pass leaves the ship on an orbit whose apoapsis is the
    /// low orbit's radius (aimed in the field: with Mars's J2, half a
    /// Keplerian period missed it by 12 km), where it circularizes. `after`
    /// is the rule's burn there, which the leg counts.
    fn capture(
        &self,
        orbit: LowOrbit,
        t: Epoch,
        s: [f64; 6],
        after: f64,
    ) -> Result<(Item, Epoch, [f64; 6]), String> {
        let eph = self.world.eph;
        let (field, _) = self.world.system(eph.system(orbit.body))?;
        let body = eph.body(orbit.body);
        let b = field.body_state(orbit.body, t);
        let (r, v) = (
            DVec3::new(s[0], s[1], s[2]) - b.r,
            DVec3::new(s[3], s[4], s[5]) - b.v,
        );
        let leaving = |speed: f64| {
            let u = v.normalize() * speed + b.v;
            [s[0], s[1], s[2], u.x, u.y, u.z]
        };
        let a = (r.length() + orbit.radius) / 2.0;
        let period = 2.0 * std::f64::consts::PI * (a.powi(3) / body.gm).sqrt();
        let miss = |speed: f64| -> Result<(f64, f64, [f64; 6], Trajectory<6>), String> {
            let (t_apo, top, path) = apoapsis(field, orbit.body, t, leaving(speed), period)
                .ok_or("no apoapsis after the aerocapture")?;
            let rt = DVec3::new(top[0], top[1], top[2])
                - field.body_position(orbit.body, t.plus_days(t_apo / DAY));
            Ok((rt.length() - orbit.radius, t_apo, top, path))
        };
        // Secant steps on the speed leaving the pass, from the Keplerian
        // one, to within 10 m.
        let mut s0 = (body.gm * (2.0 / r.length() - 1.0 / a)).sqrt();
        let mut s1 = s0 + 1e-3;
        let mut f0 = miss(s0)?.0;
        let mut found = miss(s1)?;
        for _ in 0..10 {
            if found.0.abs() < 0.01 {
                break;
            }
            let next = s1 - found.0 * (s1 - s0) / (found.0 - f0);
            (s0, f0, s1) = (s1, found.0, next);
            found = miss(s1)?;
        }
        let (gap, t_apo, top, path) = found;
        if gap.abs() >= 0.01 {
            return Err(format!(
                "the aerocapture misses the low orbit by {gap:.2} km"
            ));
        }
        let mut track = Track::new(field.center(), t);
        track.extend(field, 0.0, t_apo, |t| path.at(t));
        let at = t.plus_days(t_apo / DAY);
        // Circularize: the circular speed, along the velocity.
        let bt = field.body_state(orbit.body, at);
        let (rt, vt) = (
            DVec3::new(top[0], top[1], top[2]) - bt.r,
            DVec3::new(top[3], top[4], top[5]) - bt.v,
        );
        let circular = vt.normalize() * (body.gm / rt.length()).sqrt() + bt.v;
        let dv = (circular - DVec3::new(top[3], top[4], top[5])).length();
        let state = [top[0], top[1], top[2], circular.x, circular.y, circular.z];
        Ok((
            Item {
                step: self.at,
                kind: ItemKind::Capture,
                label: format!("aerocapture into {}", describe(eph, &PlaceKind::Low(orbit))),
                start: t,
                end: at,
                dv: 0.0,
                track: Some(track),
                markers: vec![Marker {
                    t: at,
                    kind: MarkerKind::Burn,
                    dv,
                    pass: None,
                }],
                options: Vec::new(),
                window: None,
                seconds: 0.0,
                note: Some(format!(
                    "circularizing takes {:.1} m/s here; the leg counts the rule's {:.1}",
                    dv * 1e3,
                    after * 1e3
                )),
            },
            at,
            state,
        ))
    }

    /// The stay from when the ship arrived until `until`, when it leaves in
    /// state `leaving` (relative to the given center) if a flown leg leaves
    /// (none if no time passes), and the jump to that state if it is not
    /// on the ship's own orbit.
    fn stay(
        &self,
        until: Epoch,
        leaving: Option<([f64; 6], Option<BodyId>)>,
    ) -> Result<(Vec<Item>, Option<Marker>), String> {
        let eph = self.world.eph;
        let span = (until.0 - self.since.0) * DAY;
        // A leg held at its window's opening leaves when the ship arrives,
        // give or take rounding.
        if span < -1e-3 {
            return Err(format!(
                "the leg leaves {until}, before the ship arrives ({})",
                self.since
            ));
        }
        let span = span.max(0.0);
        let mut jumped = None;
        let mut item = Item {
            step: self.at,
            kind: ItemKind::Stay,
            label: format!("at {}", self.label),
            start: self.since,
            end: until,
            dv: 0.0,
            track: None,
            markers: Vec::new(),
            options: Vec::new(),
            window: None,
            seconds: 0.0,
            note: None,
        };
        let still = |b: BodyId| {
            let mut t = Track::new(Some(b), self.since);
            t.times = vec![0.0, span];
            t.states = vec![[0.0; 6]; 2];
            t
        };
        match &self.here {
            Here::Low { orbit, coast: c } => {
                let revolution = 2.0
                    * std::f64::consts::PI
                    * (orbit.radius.powi(3) / eph.body(orbit.body).gm).sqrt();
                let system = eph.system(orbit.body);
                let (field, _) = self.world.system(system)?;
                let leaving =
                    leaving.map(|(s, center)| self.world.rebase(s, center, Some(system), until));
                match (c, leaving) {
                    // From where the last leg left the ship; a leg leaving
                    // from another state jumps.
                    (Some((t, s)), leaving) => {
                        let (track, end) = coast(field, *t, *s, (until.0 - t.0) * DAY)?;
                        jumped = leaving.map(|l| jump(field, orbit.body, until, &end, &l));
                        item.track = Some(track);
                    }
                    // A free plane: the orbit the leg leaves from, coasted
                    // back to when the ship arrived.
                    (None, Some(l)) if span > 0.0 => {
                        let back = coast_back(field, until, l, span)?;
                        item.track = Some(coast(field, self.since, back, span)?.0);
                    }
                    (None, _) => {}
                }
                if let Some(track) = &mut item.track {
                    track.revolution = Some(revolution);
                }
            }
            Here::Station(visit) => {
                let r = &visit.reference;
                let (field, _) = self.world.system(eph.lookup("earth").unwrap())?;
                let mut track = Track::new(r.center, self.since);
                if span > 0.0 {
                    track.extend(field, 0.0, span, |t| r.state(self.since.plus_days(t / DAY)));
                }
                track.revolution = Some(r.period_s());
                item.track = Some(track);
                let revolutions = (r.path.span().1 - r.path.span().0) / r.period_s();
                if span > 0.0 {
                    if revolutions >= UPKEEP_REVOLUTIONS
                        && let Some(u) = upkeep(field, r, Errors::TYPICAL, UPKEEP_RUNS)
                    {
                        item.dv = u.dv_per_year_kms * span / (365.25 * DAY);
                        item.note = Some(format!(
                            "station-keeping {:.2} m/s per year",
                            u.dv_per_year_kms * 1e3
                        ));
                    } else {
                        item.note = Some("station-keeping not simulated (visit too short)".into());
                    }
                }
                item.label = format!("at {}", visit.label);
            }
            Here::Body(b) | Here::Surface(b) => item.track = Some(still(*b)),
        }
        let items = if span > 0.0 { vec![item] } else { Vec::new() };
        Ok((items, jumped))
    }
}

/// Records a leg's window, as its search had it, on its item.
fn set_window(items: &mut [Item], window: (Epoch, Epoch)) {
    if let Some(leg) = items.iter_mut().find(|i| i.kind == ItemKind::Leg) {
        leg.window = Some(window);
    }
}

/// `s` at `t` coasted `span` s in `field`: sampled, and the state at the
/// end.
fn coast(field: &Field, t: Epoch, s: [f64; 6], span: f64) -> Result<(Track, [f64; 6]), String> {
    let mut track = Track::new(field.center(), t);
    if span <= 0.0 {
        track.times.push(0.0);
        track.states.push(s);
        return Ok((track, s));
    }
    let ode = field.ode(t);
    let mut run = Dop853::new(&ode, 0.0, s, TOL);
    let mut steps = Vec::new();
    run.advance_dense(&ode, span, &mut steps)
        .map_err(|_| "a coast could not be integrated".to_string())?;
    let path = Trajectory::new(steps);
    track.extend(field, 0.0, span, |t| path.at(t));
    Ok((track, path.at(span)))
}

/// The first apoapsis about `body` of `s` at `t`, within `span` s: its
/// time (s after `t`), state, and the path there.
fn apoapsis(
    field: &Field,
    body: BodyId,
    t: Epoch,
    s: [f64; 6],
    span: f64,
) -> Option<(f64, [f64; 6], Trajectory<6>)> {
    let ode = field.ode(t);
    let rate = |dt: f64, y: &[f64; 6]| {
        let b = field.body_state(body, t.plus_days(dt / DAY));
        (DVec3::new(y[0], y[1], y[2]) - b.r).dot(DVec3::new(y[3], y[4], y[5]) - b.v)
    };
    let mut run = Dop853::new(&ode, 0.0, s, TOL);
    let mut steps = Vec::new();
    let mut rising = false;
    while run.t < span {
        let step = run.step(&ode, span).ok()?;
        let r = rate(run.t, &run.y);
        if rising && r <= 0.0 {
            let (mut lo, mut hi) = (step.t0, step.t0 + step.h);
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if rate(mid, &step.at(mid)) > 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let ta = 0.5 * (lo + hi);
            let top = step.at(ta);
            steps.push(step);
            return Some((ta, top, Trajectory::new(steps)));
        }
        rising = r > 0.0;
        steps.push(step);
    }
    None
}

/// `s` at `t` coasted backward `span` s: the state then.
fn coast_back(field: &Field, t: Epoch, s: [f64; 6], span: f64) -> Result<[f64; 6], String> {
    let ode = field.ode(t);
    let mut run = Dop853::new(&ode, 0.0, s, TOL);
    run.advance_to(&ode, -span)
        .map_err(|_| "a coast could not be integrated".to_string())?;
    Ok(run.y)
}

/// The state at each of `times` (in order) of `s` at `t`, coasting in
/// `field`.
fn coast_to(
    field: &Field,
    t: Epoch,
    s: [f64; 6],
    times: &[Epoch],
) -> Result<Vec<[f64; 6]>, String> {
    let failed = |_| "a coast could not be integrated".to_string();
    let Some(&first) = times.first() else {
        return Ok(Vec::new());
    };
    // To the first, either way in time; then on from there.
    let ode = field.ode(t);
    let mut run = Dop853::new(&ode, 0.0, s, TOL);
    run.advance_to(&ode, (first.0 - t.0) * DAY)
        .map_err(failed)?;
    let ode = field.ode(first);
    let mut run = Dop853::new(&ode, 0.0, run.y, TOL);
    times
        .iter()
        .map(|u| {
            run.advance_to(&ode, (u.0 - first.0) * DAY)
                .map_err(failed)?;
            Ok(run.y)
        })
        .collect()
}

/// The ship's own low orbit of `period` s, coasting in `field` from `s`
/// at `t`, from `from` until `until`: a reference a leg can leave.
fn own_reference(
    field: &Field,
    (t, s): (Epoch, [f64; 6]),
    from: Epoch,
    until: Epoch,
    period: f64,
) -> Result<Reference, String> {
    let start = coast_to(field, t, s, &[from])?[0];
    let span = (until.0 - from.0) * DAY;
    let ode = field.ode(from);
    let mut run = Dop853::new(&ode, 0.0, start, TOL);
    let mut steps = Vec::new();
    run.advance_dense(&ode, span, &mut steps)
        .map_err(|_| "a coast could not be integrated".to_string())?;
    Ok(Reference {
        epoch: from,
        center: field.center(),
        revolutions: ((span / period).round() as usize).max(1),
        patches: Vec::new(),
        path: Trajectory::new(steps),
    })
}

/// The jump between the ship's own state `own` in a low orbit about
/// `body` and the state `leaving` a leg starts from, at `t`.
fn jump(field: &Field, body: BodyId, t: Epoch, own: &[f64; 6], leaving: &[f64; 6]) -> Marker {
    let b = field.body_state(body, t);
    let rel = |s: &[f64; 6]| {
        (
            DVec3::new(s[0], s[1], s[2]) - b.r,
            DVec3::new(s[3], s[4], s[5]) - b.v,
        )
    };
    let ((r0, v0), (r1, v1)) = (rel(own), rel(leaving));
    let angle = r0.cross(v0).angle_between(r1.cross(v1));
    Marker {
        t,
        kind: MarkerKind::Jump {
            km: (r1 - r0).length(),
            plane_deg: angle.to_degrees(),
        },
        dv: 2.0 * v0.length() * (angle / 2.0).sin(),
        pass: None,
    }
}
