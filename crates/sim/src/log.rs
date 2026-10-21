//! A game's log (M1b.md, Modes; step 1 plan, part 2): its setup and its
//! actions. Its file is a mission plan's (`orbit::mission::Plan`, whose
//! ships are the first actions) with a `[game]` table and `[[replan]]`
//! entries; a plan without them is a sandbox log, so the plans in
//! `data/missions/` read as they are.

use crate::designs::apply;
use crate::ships::{Commodity, commodity};
use crate::state::Kind;
use orbit::ephem::Ephemeris;
use orbit::mission::{Manifest, Meeting, Meetings, Plan, Step, legs_value, parse_legs};
use orbit::places::Places;
use orbit::time::Epoch;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Any action may change at any time.
    #[default]
    Sandbox,
    /// Actions are added at now, and take effect from it.
    Career,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Setup {
    /// The world's draw: from this seed, else every parameter's central
    /// value.
    pub seed: Option<u64>,
    pub mode: Mode,
    /// When the game starts; by default, its first ship's epoch.
    pub start: Option<Epoch>,
    /// In career, the time the game has reached.
    pub now: Option<Epoch>,
}

/// A ship's steps from one of them on, replaced.
#[derive(Clone, Debug)]
pub struct Replan {
    pub ship: u64,
    /// When it was made (career's now then), to the minute ([`minute`]).
    pub at: Epoch,
    /// The first of the ship's steps it replaces; those before stay.
    pub from: usize,
    pub steps: Vec<Step>,
    /// The manifest of the stop the ship is at, set anew (M1b.md, step 3
    /// plan, part 4): that of the last leg before `from`, whose takes are
    /// still to come (what was given on its arrival was by the manifest
    /// standing then; the loop reads re-plans by their dates).
    pub here: Option<Manifest>,
}

/// A scenario's check (M1b.md, step 1 plan, part 8; step 2 plan, part 5).
#[derive(Clone, Debug, PartialEq)]
pub enum Check {
    /// Ship `ship` is staying at `place` (as the plan names it) at `at`:
    /// as designed, or stranded there by running dry.
    At { ship: u64, at: Epoch, place: String },
    /// The leg of ship `ship`'s step `step` costs from `dv[0]` to `dv[1]`
    /// m/s.
    Dv {
        ship: u64,
        step: usize,
        dv: [f64; 2],
    },
    /// Ship `ship` carries from `t[0]` to `t[1]` t of propellant at `at`.
    Propellant { ship: u64, at: Epoch, t: [f64; 2] },
    /// Ship `ship` runs dry at step `step`; with none, it does not.
    Dry { ship: u64, step: Option<usize> },
    /// Ship `ship`'s hold at `at` has, of each commodity, from the first
    /// bound to the second, t (M1b.md, step 3 plan, part 8).
    Cargo {
        ship: u64,
        at: Epoch,
        cargo: Vec<(Commodity, [f64; 2])>,
    },
    /// The give or take of `commodity` that step `step`'s manifest asked
    /// of ship `ship` fell short, or was made in full.
    Transfer {
        ship: u64,
        step: usize,
        kind: Kind,
        commodity: Commodity,
        short: bool,
    },
}

impl Check {
    /// The ship it checks, by id.
    pub fn ship(&self) -> u64 {
        match self {
            Check::At { ship, .. }
            | Check::Dv { ship, .. }
            | Check::Propellant { ship, .. }
            | Check::Dry { ship, .. }
            | Check::Cargo { ship, .. }
            | Check::Transfer { ship, .. } => *ship,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Log {
    /// Its name, header and ships as first given. Its `game` and
    /// `replans` tables are read into [`Log::setup`] and
    /// [`Log::replans`], and written from them.
    pub plan: Plan,
    pub setup: Setup,
    /// In the order they were made.
    pub replans: Vec<Replan>,
    /// What a scenario expects of the game.
    pub checks: Vec<Check>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct GameFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<u64>,
    #[serde(default)]
    mode: Mode,
    #[serde(skip_serializing_if = "Option::is_none")]
    start: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    now: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CheckFile {
    ship: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    place: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    step: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dv: Option<[f64; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    propellant_t: Option<[f64; 2]>,
    /// True, with the step it runs dry at; false, it does not.
    #[serde(skip_serializing_if = "Option::is_none")]
    dry: Option<bool>,
    /// The hold's bounds by commodity, t.
    #[serde(skip_serializing_if = "Option::is_none")]
    cargo: Option<BTreeMap<String, [f64; 2]>>,
    /// A transfer at `step`: the commodity given or taken, and whether it
    /// fell short.
    #[serde(skip_serializing_if = "Option::is_none")]
    give: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    take: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    short: Option<bool>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReplanFile {
    ship: u64,
    at: String,
    from: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    here: Option<HereFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    leg: Option<toml::Value>,
}

/// A re-plan's `here`: a manifest, written as a leg writes its own.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HereFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    propellant_t: Option<f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    cargo: BTreeMap<String, f64>,
}

/// Why a re-plan from step `from` cannot set the manifest of the stop the
/// ship is at: no leg before `from` arrives at one.
pub fn here_without_a_stop(from: usize) -> String {
    format!(
        "`here` sets the manifest of the stop the ship is at, but no leg before step {from} \
         arrives at one"
    )
}

/// `t` rounded up to the minute, as a log writes it: actions are made at
/// whole minutes, so that a log read back is the game it was.
pub fn minute(t: Epoch) -> Epoch {
    // A minute's time read back lands within rounding of a whole minute,
    // either side (past 2044, f64 days hold a minute to ~4e-9 of one).
    let m = t.0 * 1_440.0;
    let up = if (m - m.round()).abs() < 1e-6 {
        m.round()
    } else {
        m.ceil()
    };
    Epoch(up / 1_440.0)
        .stamp()
        .parse()
        .expect("a stamp reads back")
}

fn date(s: &str) -> Result<Epoch, String> {
    s.parse()
}

impl Log {
    /// A sandbox log of `plan`'s ships.
    pub fn sandbox(mut plan: Plan) -> Log {
        plan.game = None;
        plan.replans.clear();
        plan.checks.clear();
        Log {
            plan,
            setup: Setup::default(),
            replans: Vec::new(),
            checks: Vec::new(),
        }
    }

    pub fn parse(src: &str, eph: &Ephemeris, places: &Places) -> Result<Log, String> {
        let mut plan = Plan::parse(src, eph, places)?;
        let setup = match plan.game.take() {
            None => Setup::default(),
            Some(table) => {
                let fail = |e: String| format!("[game]: {e}");
                let g: GameFile = table.try_into().map_err(|e| fail(format!("{e}")))?;
                Setup {
                    seed: g.seed,
                    mode: g.mode,
                    start: g.start.as_deref().map(date).transpose().map_err(fail)?,
                    now: g.now.as_deref().map(date).transpose().map_err(fail)?,
                }
            }
        };
        let replans = std::mem::take(&mut plan.replans)
            .into_iter()
            .enumerate()
            .map(|(i, table)| {
                let fail = |e: String| format!("replan {}: {e}", i + 1);
                let r: ReplanFile = table.try_into().map_err(|e| fail(format!("{e}")))?;
                Ok(Replan {
                    ship: r.ship,
                    at: date(&r.at).map_err(fail)?,
                    from: r.from,
                    steps: match r.leg {
                        Some(legs) => parse_legs(legs, eph, places).map_err(fail)?,
                        None => Vec::new(),
                    },
                    here: r.here.map(|h| Manifest {
                        propellant_t: h.propellant_t,
                        cargo: h.cargo,
                    }),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let checks = std::mem::take(&mut plan.checks)
            .into_iter()
            .enumerate()
            .map(|(i, table)| {
                let fail = |e: String| format!("check {}: {e}", i + 1);
                let c: CheckFile = table.try_into().map_err(|e| fail(format!("{e}")))?;
                match c {
                    CheckFile {
                        ship,
                        at: Some(at),
                        place: Some(place),
                        step: None,
                        dv: None,
                        propellant_t: None,
                        dry: None,
                        cargo: None,
                        give: None,
                        take: None,
                        short: None,
                    } => Ok(Check::At {
                        ship,
                        at: date(&at).map_err(fail)?,
                        place,
                    }),
                    CheckFile {
                        ship,
                        at: None,
                        place: None,
                        step: Some(step),
                        dv: Some(dv),
                        propellant_t: None,
                        dry: None,
                        cargo: None,
                        give: None,
                        take: None,
                        short: None,
                    } => Ok(Check::Dv { ship, step, dv }),
                    CheckFile {
                        ship,
                        at: Some(at),
                        place: None,
                        step: None,
                        dv: None,
                        propellant_t: Some(t),
                        dry: None,
                        cargo: None,
                        give: None,
                        take: None,
                        short: None,
                    } => Ok(Check::Propellant {
                        ship,
                        at: date(&at).map_err(fail)?,
                        t,
                    }),
                    CheckFile {
                        ship,
                        at: None,
                        place: None,
                        step,
                        dv: None,
                        propellant_t: None,
                        dry: Some(dry),
                        cargo: None,
                        give: None,
                        take: None,
                        short: None,
                    } if dry == step.is_some() => Ok(Check::Dry { ship, step }),
                    CheckFile {
                        ship,
                        at: Some(at),
                        place: None,
                        step: None,
                        dv: None,
                        propellant_t: None,
                        dry: None,
                        cargo: Some(cargo),
                        give: None,
                        take: None,
                        short: None,
                    } => Ok(Check::Cargo {
                        ship,
                        at: date(&at).map_err(fail)?,
                        cargo: cargo
                            .iter()
                            .map(|(k, &t)| Ok((commodity(k)?, t)))
                            .collect::<Result<_, String>>()
                            .map_err(fail)?,
                    }),
                    CheckFile {
                        ship,
                        at: None,
                        place: None,
                        step: Some(step),
                        dv: None,
                        propellant_t: None,
                        dry: None,
                        cargo: None,
                        give,
                        take,
                        short: Some(short),
                    } if give.is_some() != take.is_some() => {
                        let (kind, c) = match (give, take) {
                            (Some(c), None) => (Kind::Give, c),
                            (None, Some(c)) => (Kind::Take, c),
                            _ => unreachable!("one of the two"),
                        };
                        Ok(Check::Transfer {
                            ship,
                            step,
                            kind,
                            commodity: commodity(&c).map_err(fail)?,
                            short,
                        })
                    }
                    _ => Err(fail(
                        "a check has `at` and `place`; `step` and `dv`; `at` and \
                         `propellant_t`; `step` and `dry = true`; `dry = false`; `at` and \
                         `cargo`; or `step`, `give` or `take`, and `short`"
                            .into(),
                    )),
                }
            })
            .collect::<Result<Vec<_>, String>>()?;
        let mut log = Log {
            plan,
            setup,
            replans,
            checks,
        };
        log.check()?;
        Ok(log)
    }

    /// Why the log is not a game, if it is not: career has a now, and its
    /// re-plans were made by then; re-plans are in time order, each of a
    /// ship the log has, from one of the steps it had then, and one that
    /// sets the manifest of the stop the ship is at (`here`) has a leg
    /// before them that arrives at one; and its meetings can be designed
    /// (M1b.md, step 3 plan, part 3:
    /// [`Meetings`]), as the plan stands with its re-plans, which is how
    /// they are resolved here, into each meeting's place. A re-plan does
    /// not replace a leg another ship's plan meets its ship after, where
    /// that meeting was made by an earlier re-plan; one in a ship's first
    /// steps the log does not date (the game refuses both when they are
    /// made).
    pub fn check(&mut self) -> Result<(), String> {
        match (self.setup.mode, self.setup.now) {
            (Mode::Career, None) => return Err("[game]: a career game has a now".into()),
            (Mode::Sandbox, Some(_)) => return Err("[game]: now is career's".into()),
            _ => {}
        }
        let mut steps: Vec<(u64, usize)> = self
            .plan
            .ships
            .iter()
            .map(|s| (s.id, s.steps.len()))
            .collect();
        let mut last = f64::NEG_INFINITY;
        for (i, r) in self.replans.iter().enumerate() {
            let fail = |e: String| Err(format!("replan {}: {e}", i + 1));
            if r.at.0 < last {
                return fail("made before the re-plan above it".into());
            }
            last = r.at.0;
            if let Some(now) = self.setup.now
                && r.at.0 > now.0
            {
                return fail(format!(
                    "made {}, after now ({})",
                    r.at.stamp(),
                    now.stamp()
                ));
            }
            let Some((_, n)) = steps.iter_mut().find(|(id, _)| *id == r.ship) else {
                return fail(format!("no ship has id {}", r.ship));
            };
            if r.from > *n {
                return fail(format!("from step {}, but the ship has {n}", r.from));
            }
            *n = r.from + r.steps.len();
        }
        for (i, c) in self.checks.iter().enumerate() {
            let ship = c.ship();
            if !steps.iter().any(|(id, _)| *id == ship) {
                return Err(format!("check {}: no ship has id {ship}", i + 1));
            }
        }
        // The plan as it stands, and the meetings re-plans made that still
        // stand: (ship, its step, the meeting).
        let mut standing = self.plan.ships.clone();
        let mut made: Vec<(u64, usize, Meeting)> = Vec::new();
        for (i, r) in self.replans.iter().enumerate() {
            if let Some((ship, step, m)) = made
                .iter()
                .find(|(_, _, m)| m.ship == r.ship && m.leg.is_some_and(|l| l >= r.from))
            {
                return Err(format!(
                    "replan {}: replaces leg {} of ship {}, which ship {ship} meets it after \
                     (step {step} of its plan)",
                    i + 1,
                    m.leg.unwrap(),
                    r.ship
                ));
            }
            made.retain(|(ship, step, _)| *ship != r.ship || *step < r.from);
            for (k, step) in r.steps.iter().enumerate() {
                if let Step::Go(go) = step
                    && let Some(m) = go.to.meeting
                {
                    made.push((r.ship, r.from + k, m));
                }
            }
            let s = standing
                .iter_mut()
                .find(|s| s.id == r.ship)
                .expect("a re-plan's ship is in the plan (checked above)");
            if r.here.is_some() && !s.steps[..r.from].iter().any(|s| matches!(s, Step::Go(_))) {
                return Err(format!("replan {}: {}", i + 1, here_without_a_stop(r.from)));
            }
            apply(&mut s.steps, r);
        }
        let meetings = Meetings::new(&standing)?;
        for ship in &mut self.plan.ships {
            meetings.resolve(&mut ship.steps);
        }
        for r in &mut self.replans {
            meetings.resolve(&mut r.steps);
        }
        Ok(())
    }

    /// The log as a file, which [`Log::parse`] reads back as this log.
    pub fn to_toml(&self) -> String {
        let table = |v: toml::Value| match v {
            toml::Value::Table(t) => t,
            _ => unreachable!("a struct serializes as a table"),
        };
        let mut plan = self.plan.clone();
        plan.game = (self.setup != Setup::default()).then(|| {
            let s = &self.setup;
            table(
                toml::Value::try_from(GameFile {
                    seed: s.seed,
                    mode: s.mode,
                    start: s.start.map(Epoch::stamp),
                    now: s.now.map(Epoch::stamp),
                })
                .expect("a setup serializes"),
            )
        });
        plan.replans = self
            .replans
            .iter()
            .map(|r| {
                table(
                    toml::Value::try_from(ReplanFile {
                        ship: r.ship,
                        at: r.at.stamp(),
                        from: r.from,
                        here: r.here.as_ref().map(|m| HereFile {
                            propellant_t: m.propellant_t,
                            cargo: m.cargo.clone(),
                        }),
                        leg: (!r.steps.is_empty()).then(|| legs_value(&r.steps)),
                    })
                    .expect("a re-plan serializes"),
                )
            })
            .collect();
        plan.checks = self
            .checks
            .iter()
            .map(|c| {
                table(
                    toml::Value::try_from(match c {
                        Check::At { ship, at, place } => CheckFile {
                            ship: *ship,
                            at: Some(at.stamp()),
                            place: Some(place.clone()),
                            ..CheckFile::default()
                        },
                        Check::Dv { ship, step, dv } => CheckFile {
                            ship: *ship,
                            step: Some(*step),
                            dv: Some(*dv),
                            ..CheckFile::default()
                        },
                        Check::Propellant { ship, at, t } => CheckFile {
                            ship: *ship,
                            at: Some(at.stamp()),
                            propellant_t: Some(*t),
                            ..CheckFile::default()
                        },
                        Check::Dry { ship, step } => CheckFile {
                            ship: *ship,
                            step: *step,
                            dry: Some(step.is_some()),
                            ..CheckFile::default()
                        },
                        Check::Cargo { ship, at, cargo } => CheckFile {
                            ship: *ship,
                            at: Some(at.stamp()),
                            cargo: Some(
                                cargo
                                    .iter()
                                    .map(|(c, t)| (c.id().to_string(), *t))
                                    .collect(),
                            ),
                            ..CheckFile::default()
                        },
                        Check::Transfer {
                            ship,
                            step,
                            kind,
                            commodity,
                            short,
                        } => CheckFile {
                            ship: *ship,
                            step: Some(*step),
                            give: (*kind == Kind::Give).then(|| commodity.id().to_string()),
                            take: (*kind == Kind::Take).then(|| commodity.id().to_string()),
                            short: Some(*short),
                            ..CheckFile::default()
                        },
                    })
                    .expect("a check serializes"),
                )
            })
            .collect();
        plan.to_toml()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAREER: &str = r#"
# A career game.

name = "career"

[game]
seed = 3
mode = "career"
start = "2045-01-01"
now = "2045-03-04T06:30"

[[ship]]
id = 2
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-01", propellant_t = 120.0, cargo = { equipment = 20.0 } }

[[ship.leg]]
to = "nrho"
days = [5.0]
propellant_t = 100.0
cargo = { equipment = 0.0 }

[[replan]]
ship = 2
at = "2045-03-02T12:00"
from = 1
here = { propellant_t = 120.0, cargo = { equipment = 5.0 } }

[[replan.leg]]
wait = 3

[[replan.leg]]
to = "llo"
days = [0.5]

[[check]]
ship = 2
at = "2045-03-03"
place = "leo"

[[check]]
ship = 2
step = 2
dv = [600.0, 750.0]

[[check]]
ship = 2
at = "2045-03-03"
propellant_t = [100.0, 120.0]

[[check]]
ship = 2
step = 2
dry = true

[[check]]
ship = 2
dry = false

[[check]]
ship = 2
at = "2045-03-03"
cargo = { equipment = [19.0, 20.0], lh2 = [0.0, 0.0] }

[[check]]
ship = 2
step = 0
give = "equipment"
short = false

[[check]]
ship = 2
step = 1
take = "lox"
short = true
"#;

    fn parse(src: &str) -> Result<Log, String> {
        Log::parse(src, Ephemeris::standard(), Places::standard())
    }

    #[test]
    fn a_career_log_reads_back_as_written() {
        let log = parse(CAREER).unwrap();
        assert_eq!(log.setup.seed, Some(3));
        assert_eq!(log.setup.mode, Mode::Career);
        assert_eq!(log.replans.len(), 1);
        assert_eq!(log.replans[0].steps.len(), 2);
        let tug = &log.plan.ships[0];
        assert_eq!(tug.class.as_deref(), Some("tug-h"));
        assert_eq!(tug.propellant_t, Some(120.0));
        assert_eq!(tug.cargo.get("equipment"), Some(&20.0));
        let Step::Go(out) = &tug.steps[0] else {
            panic!("a leg")
        };
        assert_eq!(out.manifest.propellant_t, Some(100.0));
        assert_eq!(out.manifest.cargo.get("equipment"), Some(&0.0));
        let here = log.replans[0].here.as_ref().unwrap();
        assert_eq!(here.propellant_t, Some(120.0));
        assert_eq!(here.cargo.get("equipment"), Some(&5.0));
        let text = log.to_toml();
        assert!(text.starts_with("# A career game.\n"), "{text}");
        let back = parse(&text).unwrap();
        assert_eq!(back.to_toml(), text);
        assert_eq!(
            format!("{:?}", back.plan.ships),
            format!("{:?}", log.plan.ships)
        );
        assert_eq!(back.setup, log.setup);
        assert_eq!(back.replans[0].at, log.replans[0].at);
        assert_eq!(back.replans[0].here, log.replans[0].here);
        assert_eq!(back.checks, log.checks);
        assert_eq!(
            log.checks[1..],
            [
                Check::Dv {
                    ship: 2,
                    step: 2,
                    dv: [600.0, 750.0]
                },
                Check::Propellant {
                    ship: 2,
                    at: Epoch::from_ymd(2045, 3, 3),
                    t: [100.0, 120.0]
                },
                Check::Dry {
                    ship: 2,
                    step: Some(2)
                },
                Check::Dry {
                    ship: 2,
                    step: None
                },
                Check::Cargo {
                    ship: 2,
                    at: Epoch::from_ymd(2045, 3, 3),
                    cargo: vec![
                        (Commodity::Equipment, [19.0, 20.0]),
                        (Commodity::Lh2, [0.0, 0.0])
                    ]
                },
                Check::Transfer {
                    ship: 2,
                    step: 0,
                    kind: Kind::Give,
                    commodity: Commodity::Equipment,
                    short: false
                },
                Check::Transfer {
                    ship: 2,
                    step: 1,
                    kind: Kind::Take,
                    commodity: Commodity::Lox,
                    short: true
                },
            ]
        );
    }

    #[test]
    fn a_plan_is_a_sandbox_log() {
        let path = format!(
            "{}/../../data/missions/gateway-rotation.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let log = parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(log.setup, Setup::default());
        assert!(log.replans.is_empty());
        assert!(!log.to_toml().contains("[game]"));
    }

    #[test]
    fn what_is_not_a_game() {
        let without_now = CAREER.replace("now = \"2045-03-04T06:30\"\n", "");
        assert_eq!(
            parse(&without_now).unwrap_err(),
            "[game]: a career game has a now"
        );
        let after_now = CAREER.replace("now = \"2045-03-04T06:30\"", "now = \"2045-03-02\"");
        assert_eq!(
            parse(&after_now).unwrap_err(),
            "replan 1: made 2045-03-02T12:00, after now (2045-03-02)"
        );
        let no_ship = CAREER.replace("ship = 2\nat", "ship = 5\nat");
        assert_eq!(parse(&no_ship).unwrap_err(), "replan 1: no ship has id 5");
        let half = CAREER.replace("step = 2\ndv", "dv");
        let neither = "check 2: a check has `at` and `place`; `step` and `dv`; `at` and \
                       `propellant_t`; `step` and `dry = true`; `dry = false`; `at` and \
                       `cargo`; or `step`, `give` or `take`, and `short`";
        assert_eq!(parse(&half).unwrap_err(), neither);
        let dry_without_step = CAREER.replace("step = 2\ndry = true", "dry = true");
        assert_eq!(
            parse(&dry_without_step).unwrap_err(),
            neither.replace("check 2", "check 4")
        );
        let not_dry_at_a_step = CAREER.replace("dry = false", "step = 1\ndry = false");
        assert_eq!(
            parse(&not_dry_at_a_step).unwrap_err(),
            neither.replace("check 2", "check 5")
        );
        let both = CAREER.replace(
            "give = \"equipment\"",
            "give = \"equipment\"\ntake = \"lox\"",
        );
        assert_eq!(
            parse(&both).unwrap_err(),
            neither.replace("check 2", "check 7")
        );
        let unsold = CAREER.replace("take = \"lox\"", "take = \"water\"");
        assert!(
            parse(&unsold).unwrap_err().starts_with("check 8: "),
            "{}",
            parse(&unsold).unwrap_err()
        );
        let past_the_end = CAREER.replace("from = 1", "from = 2");
        assert_eq!(
            parse(&past_the_end).unwrap_err(),
            "replan 1: from step 2, but the ship has 1"
        );
        let here_at_the_start = CAREER.replace("from = 1", "from = 0");
        assert_eq!(
            parse(&here_at_the_start).unwrap_err(),
            "replan 1: `here` sets the manifest of the stop the ship is at, but no leg before \
             step 0 arrives at one"
        );
    }

    /// A time rounded up to the minute is a minute's time, and stays one:
    /// every minute of a day in 2045 and in 2075, read from its stamp.
    #[test]
    fn minutes_read_back() {
        for day in [Epoch::from_ymd(2045, 6, 1), Epoch::from_ymd(2075, 12, 30)] {
            for k in 0..1_440 {
                let m = Epoch(day.0 + k as f64 / 1_440.0).stamp().parse().unwrap();
                assert_eq!(minute(m), m, "{}", m.stamp());
            }
        }
        for t in [
            9_000.0,
            9_000.000_1,
            9_000.5 + 1e-9,
            12_345.678_9,
            20_000.123_4,
        ] {
            let m = minute(Epoch(t));
            assert!(m.0 >= t - 1e-9, "{t}: {}", m.0);
            assert!(m.0 - t < 1.0 / 1_440.0 + 1e-9);
            assert_eq!(minute(m), m);
            assert_eq!(m.stamp().parse::<Epoch>().unwrap(), m);
        }
    }
}
