//! Editing a ship (M1c.md, step 7 plan, item 6): its name, its start,
//! its class and load (M1b.md, step 2 plan, part 4), and its steps (a
//! leg's destination, a place or a vessel of the plan met where it is
//! after one of its legs; durations, window and aerocapture; its manifest,
//! what to carry after the stop; a wait's days), steps added, removed and
//! moved. Each change makes a new ship, which the plan designs again from
//! the first step it changes (a new class, load or manifest designs
//! nothing); legs that cannot be flown say why at once, by the planner's
//! own rules (`mission::check`).

use damascene_core::prelude::*;
use orbit::mission::{Go, Manifest, Meeting, Meetings, Ship, Spot, Step};
use orbit::places::Places;
use orbit::time::Epoch;
use sim::params::ShipClass;
use sim::ships::{COMMODITIES, Commodity};
use std::collections::HashMap;

/// A control of the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Name,
    /// The ship's start: its place, and when.
    Start,
    Epoch,
    /// The ship's class (a menu; "none": massless), and what it carries
    /// at its start: propellant, t (empty: a full tank), and cargo, t by
    /// commodity.
    Class,
    Propellant,
    Cargo(Commodity),
    /// Step `i`: a leg's destination, durations (days, by commas), window
    /// (its opening and close) and aerocapture; a wait's days.
    To(usize),
    Days(usize),
    Open(usize),
    Close(usize),
    Aero(usize),
    Wait(usize),
    /// Leg `i`'s manifest (M1b.md, step 3 plan, part 4): the propellant,
    /// t, and cargo, t by commodity, to carry after its stop; empty, left
    /// as it is.
    LegPropellant(usize),
    LegCargo(usize, Commodity),
    /// Step `i` moved up or down, or removed.
    Up(usize),
    Down(usize),
    Remove(usize),
    AddLeg,
    AddWait,
    RemoveShip,
    Done,
}

impl Field {
    pub fn key(self) -> String {
        match self {
            Field::Name => "edit:name".into(),
            Field::Start => "edit:start".into(),
            Field::Epoch => "edit:epoch".into(),
            Field::Class => "edit:class".into(),
            Field::Propellant => "edit:propellant".into(),
            Field::Cargo(c) => format!("edit:cargo:{}", c.id()),
            Field::To(i) => format!("edit:{i}:to"),
            Field::Days(i) => format!("edit:{i}:days"),
            Field::Open(i) => format!("edit:{i}:open"),
            Field::Close(i) => format!("edit:{i}:close"),
            Field::Aero(i) => format!("edit:{i}:aero"),
            Field::Wait(i) => format!("edit:{i}:wait"),
            Field::LegPropellant(i) => format!("edit:{i}:propellant"),
            Field::LegCargo(i, c) => format!("edit:{i}:cargo:{}", c.id()),
            Field::Up(i) => format!("edit:{i}:up"),
            Field::Down(i) => format!("edit:{i}:down"),
            Field::Remove(i) => format!("edit:{i}:remove"),
            Field::AddLeg => "edit:add-leg".into(),
            Field::AddWait => "edit:add-wait".into(),
            Field::RemoveShip => "edit:remove-ship".into(),
            Field::Done => "edit:done".into(),
        }
    }

    /// The field a key names (a menu's option keys are its own key with
    /// `:option:ID` after).
    pub fn parse(key: &str) -> Option<Field> {
        let rest = key.strip_prefix("edit:")?;
        Some(match rest {
            "name" => Field::Name,
            "start" => Field::Start,
            "epoch" => Field::Epoch,
            "class" => Field::Class,
            "propellant" => Field::Propellant,
            "add-leg" => Field::AddLeg,
            "add-wait" => Field::AddWait,
            "remove-ship" => Field::RemoveShip,
            "done" => Field::Done,
            _ if rest.starts_with("cargo:") => Field::Cargo(commodity(rest)?),
            _ => {
                let (i, what) = rest.split_once(':')?;
                let i: usize = i.parse().ok()?;
                match what {
                    "to" => Field::To(i),
                    "days" => Field::Days(i),
                    "open" => Field::Open(i),
                    "close" => Field::Close(i),
                    "aero" => Field::Aero(i),
                    "wait" => Field::Wait(i),
                    "propellant" => Field::LegPropellant(i),
                    _ if what.starts_with("cargo:") => Field::LegCargo(i, commodity(what)?),
                    "up" => Field::Up(i),
                    "down" => Field::Down(i),
                    "remove" => Field::Remove(i),
                    _ => return None,
                }
            }
        })
    }

    /// Whether it is typed into.
    pub fn typed(self) -> bool {
        matches!(
            self,
            Field::Name
                | Field::Epoch
                | Field::Propellant
                | Field::Cargo(_)
                | Field::Days(_)
                | Field::Open(_)
                | Field::Close(_)
                | Field::Wait(_)
                | Field::LegPropellant(_)
                | Field::LegCargo(..)
        )
    }

    /// Whether it is an amount of the start's load or a leg's manifest,
    /// which needs a class.
    fn carried(self) -> bool {
        matches!(
            self,
            Field::Propellant | Field::Cargo(_) | Field::LegPropellant(_) | Field::LegCargo(..)
        )
    }
}

/// The commodity a `cargo:ID` key names.
fn commodity(key: &str) -> Option<Commodity> {
    let id = key.strip_prefix("cargo:")?;
    COMMODITIES.iter().find(|c| c.id() == id).copied()
}

/// A menu's option for meeting a vessel: `meet:ID` where it starts,
/// `meet:ID:LEG` after its leg.
fn meet_option(m: &Meeting) -> String {
    match m.leg {
        Some(leg) => format!("meet:{}:{leg}", m.ship),
        None => format!("meet:{}", m.ship),
    }
}

/// The meeting a menu's option names, if it is one.
fn parse_meet_option(id: &str) -> Option<Meeting> {
    let rest = id.strip_prefix("meet:")?;
    let (ship, leg) = match rest.split_once(':') {
        Some((ship, leg)) => (ship, Some(leg.parse().ok()?)),
        None => (rest, None),
    };
    Some(Meeting {
        ship: ship.parse().ok()?,
        leg,
    })
}

/// The key of the editor's scroll view, under its header.
pub const SCROLL_KEY: &str = "edit:scroll";

/// What the editor holds between frames.
#[derive(Clone, Debug, Default)]
pub struct Editor {
    /// The ship edited, by its place in the plan.
    pub ship: usize,
    /// Text typed into fields and not yet committed, by field.
    pub drafts: HashMap<Field, String>,
    /// The menu open: the start's or a leg's place, or the class.
    pub menu: Option<Field>,
    /// Why the last change was refused.
    pub refused: Option<String>,
    /// Whether a text field has the keyboard (pressed last).
    pub typing: bool,
}

/// A date as a field shows it: `YYYY-MM-DD`, or with `THH:MM`.
fn date(t: Epoch) -> String {
    t.stamp()
}

/// A number as a field shows it, days or tonnes: "5", "0.5", "190.25".
fn figure(d: f64) -> String {
    let s = format!("{d:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// What `field` of `ship` reads, as typed.
pub fn value(ship: &Ship, field: Field) -> String {
    let go = |i: usize| match ship.steps.get(i) {
        Some(Step::Go(go)) => Some(go),
        _ => None,
    };
    match field {
        Field::Name => ship.name.clone(),
        Field::Start => ship.start.label.clone(),
        Field::Epoch => date(ship.epoch),
        Field::Class => ship.class.clone().unwrap_or_else(|| "none".into()),
        Field::Propellant => ship.propellant_t.map_or(String::new(), figure),
        Field::Cargo(c) => ship.cargo.get(c.id()).map_or(String::new(), |&t| figure(t)),
        Field::To(i) => go(i).map_or(String::new(), |g| g.to.label.clone()),
        Field::LegPropellant(i) => ship
            .steps
            .get(i)
            .and_then(Step::manifest)
            .and_then(|m| m.propellant_t)
            .map_or(String::new(), figure),
        Field::LegCargo(i, c) => ship
            .steps
            .get(i)
            .and_then(Step::manifest)
            .and_then(|m| m.cargo.get(c.id()))
            .map_or(String::new(), |&t| figure(t)),
        Field::Days(i) => go(i).map_or(String::new(), |g| {
            g.days
                .iter()
                .map(|&d| figure(d))
                .collect::<Vec<_>>()
                .join(", ")
        }),
        Field::Open(i) => go(i)
            .and_then(|g| g.window)
            .map_or(String::new(), |w| date(w.0)),
        Field::Close(i) => go(i)
            .and_then(|g| g.window)
            .map_or(String::new(), |w| date(w.1)),
        Field::Wait(i) => match ship.steps.get(i) {
            Some(Step::Wait(d)) => figure(*d),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// `ship` with typed `field` set to `text`, or why not.
pub fn set(ship: &Ship, field: Field, text: &str) -> Result<Ship, String> {
    let mut ship = ship.clone();
    let text = text.trim();
    let number = |s: &str| -> Result<f64, String> {
        let d: f64 = s
            .trim()
            .parse()
            .map_err(|_| format!("{s:?} is not a number"))?;
        if !d.is_finite() || d <= 0.0 {
            return Err("days are positive".into());
        }
        Ok(d)
    };
    let epoch = |s: &str| s.parse::<Epoch>();
    // Tonnes, as the engine reads them (`sim::ships::read`); empty, none.
    let tonnes = |s: &str| -> Result<Option<f64>, String> {
        if s.is_empty() {
            return Ok(None);
        }
        let t: f64 = s.parse().map_err(|_| format!("{s:?} is not a number"))?;
        if !t.is_finite() || t < 0.0 {
            return Err("a load is a number of tonnes, not negative".into());
        }
        // Not "-0".
        Ok(Some(if t == 0.0 { 0.0 } else { t }))
    };
    if field.carried() && ship.class.is_none() {
        return Err(match field {
            Field::Propellant | Field::Cargo(_) => "a load needs a class: choose one first",
            _ => "a manifest needs a class: choose one first",
        }
        .into());
    }
    match field {
        Field::Name => {
            if text.is_empty() {
                return Err("a ship has a name".into());
            }
            ship.name = text.to_string();
        }
        Field::Epoch => ship.epoch = epoch(text)?,
        Field::Propellant => ship.propellant_t = tonnes(text)?,
        Field::Cargo(c) => match tonnes(text)?.filter(|&t| t > 0.0) {
            Some(t) => {
                ship.cargo.insert(c.id().to_string(), t);
            }
            None => {
                ship.cargo.remove(c.id());
            }
        },
        Field::Wait(i) => {
            let d: f64 = text
                .parse()
                .map_err(|_| format!("{text:?} is not a number"))?;
            if !d.is_finite() || d < 0.0 {
                return Err("a wait is a number of days, not negative".into());
            }
            // A draft outliving its step (an undo) sets nothing.
            let Some(Step::Wait(wait)) = ship.steps.get_mut(i) else {
                return Err("not a wait".into());
            };
            *wait = d;
        }
        Field::LegPropellant(i) | Field::LegCargo(i, _) => {
            let Some(m) = ship.steps.get_mut(i).and_then(Step::manifest_mut) else {
                return Err("not a leg or a dock".into());
            };
            match field {
                // A manifest's cargo at 0 means to give it all; empty
                // leaves it as it is.
                Field::LegPropellant(_) => m.propellant_t = tonnes(text)?,
                Field::LegCargo(_, c) => match tonnes(text)? {
                    Some(t) => {
                        m.cargo.insert(c.id().to_string(), t);
                    }
                    None => {
                        m.cargo.remove(c.id());
                    }
                },
                _ => unreachable!("matched above"),
            }
        }
        Field::Days(i) | Field::Open(i) | Field::Close(i) => {
            let Some(Step::Go(go)) = ship.steps.get_mut(i) else {
                return Err("not a leg".into());
            };
            match field {
                Field::Days(_) => {
                    go.days = text
                        .split(',')
                        .filter(|s| !s.trim().is_empty())
                        .map(number)
                        .collect::<Result<_, _>>()?;
                }
                _ => {
                    // A window needs both ends; one cleared clears it.
                    let (open, close) = go.window.unzip();
                    let mut ends = [open, close];
                    let k = if matches!(field, Field::Open(_)) {
                        0
                    } else {
                        1
                    };
                    ends[k] = if text.is_empty() {
                        None
                    } else {
                        Some(epoch(text)?)
                    };
                    go.window = match ends {
                        [Some(a), Some(b)] if a.0 > b.0 => {
                            return Err(format!("the window opens {a}, after it closes"));
                        }
                        [Some(a), Some(b)] => Some((a, b)),
                        [Some(a), None] => Some((a, a)),
                        [None, Some(b)] => Some((b, b)),
                        _ => None,
                    };
                }
            }
        }
        _ => return Err("not typed".into()),
    }
    Ok(ship)
}

/// `ship` with a menu's option `id` chosen: its start's place or step
/// `i`'s destination, from `places` or a vessel among the plan's `ships`;
/// its class, from `classes` ("none": massless, carrying nothing).
pub fn choose(
    ship: &Ship,
    field: Field,
    id: &str,
    places: &Places,
    ships: &[Ship],
    classes: &[ShipClass],
) -> Result<Ship, String> {
    if field != Field::Class {
        return place(ship, field, id, places, ships);
    }
    let mut ship = ship.clone();
    if id == "none" {
        ship.class = None;
        ship.propellant_t = None;
        ship.cargo.clear();
    } else if classes.iter().any(|c| c.id == id) {
        ship.class = Some(id.to_string());
    } else {
        return Err(format!("no class {id:?}"));
    }
    Ok(ship)
}

/// `ship` with its start's place, or step `i`'s destination, set to the
/// place `id`; or its destination to meeting a vessel (`meet:ID:LEG`),
/// where the vessel is after that leg as the plan's `ships` stand (the
/// game's check refuses what cannot be designed: a ship meeting itself,
/// or meetings waiting on each other).
fn place(
    ship: &Ship,
    field: Field,
    id: &str,
    places: &Places,
    ships: &[Ship],
) -> Result<Ship, String> {
    let spot = match parse_meet_option(id) {
        Some(m) => {
            if field == Field::Start {
                return Err("a start is a place".into());
            }
            let kind = Meetings::new(ships)?.place(&m)?;
            Spot::meeting(m, kind)
        }
        None => Spot::named(id, places).ok_or_else(|| format!("no place {id:?}"))?,
    };
    let mut ship = ship.clone();
    match field {
        Field::Start => ship.start = spot,
        Field::To(i) => match ship.steps.get_mut(i) {
            Some(Step::Go(go)) => go.to = spot,
            _ => return Err("not a leg".into()),
        },
        _ => return Err("not a place".into()),
    }
    Ok(ship)
}

/// Where leg `i` of `ship` goes, as the editor shows it: a vessel of the
/// plan's `ships` by its name ("Depot, leg 2"; "Depot, start"), a place by
/// its label.
pub fn destination(ship: &Ship, ships: &[Ship], i: usize) -> String {
    let Some(to) = ship.steps.get(i).and_then(Step::to) else {
        return String::new();
    };
    let Some(m) = &to.meeting else {
        return to.label.clone();
    };
    let Some(vessel) = ships.iter().find(|s| s.id == m.ship) else {
        return to.label.clone();
    };
    match m.leg {
        Some(leg) => format!("{}, leg {leg}", vessel.name),
        None => format!("{}, start", vessel.name),
    }
}

/// `ship` after pressing `field`: aerocapture toggled, a step moved,
/// removed or added (after the last; a leg to the NRHO in 5 days, a wait
/// of a day). `None` where it changes nothing.
pub fn press(ship: &Ship, field: Field, places: &Places) -> Option<Ship> {
    let mut ship = ship.clone();
    let n = ship.steps.len();
    match field {
        Field::Aero(i) => match ship.steps.get_mut(i) {
            Some(Step::Go(go)) => go.aero = !go.aero,
            _ => return None,
        },
        Field::Up(i) if i > 0 && i < n => ship.steps.swap(i - 1, i),
        Field::Down(i) if i + 1 < n => ship.steps.swap(i, i + 1),
        Field::Remove(i) if i < n => {
            ship.steps.remove(i);
        }
        Field::AddLeg => ship.steps.push(Step::Go(Go {
            to: Spot::named("nrho", places)?,
            days: vec![5.0],
            window: None,
            pin: None,
            aero: false,
            dock: false,
            note: None,
            manifest: Manifest::default(),
        })),
        Field::AddWait => ship.steps.push(Step::Wait(1.0)),
        _ => return None,
    }
    Some(ship)
}

/// A new ship, `id`: leaving LEO at `t`, to the midnight before, with no
/// legs.
pub fn new_ship(id: u64, name: String, t: Epoch, places: &Places) -> Ship {
    let (y, m, d) = t.ymd();
    Ship {
        id,
        name,
        start: Spot::named("leo", places).expect("LEO is a place"),
        epoch: Epoch::from_ymd(y, m, d.floor() as u32),
        steps: Vec::new(),
        class: None,
        propellant_t: None,
        cargo: Default::default(),
    }
}

/// A field's text box.
fn input(editor: &Editor, ship: &Ship, field: Field, selection: &Selection, width: f32) -> El {
    let key = field.key();
    let shown = editor
        .drafts
        .get(&field)
        .cloned()
        .unwrap_or_else(|| value(ship, field));
    text_input(&key, &shown, selection).width(Size::Fixed(width))
}

/// A small button.
fn small(label: &str, field: Field) -> El {
    button(label).key(field.key())
}

/// The editor's section for `ship`, one of the plan's `ships`, with the
/// legs that cannot be flown (`problems`, by step), the classes a ship may
/// have, and the selection the fields share.
pub fn section(
    editor: &Editor,
    ship: &Ship,
    ships: &[Ship],
    problems: &[(usize, String)],
    classes: &[ShipClass],
    selection: &Selection,
) -> El {
    let caption = |s: &str| text(s.to_string()).caption().muted();
    let menu = |field: Field| {
        let shown = match field {
            Field::To(i) => destination(ship, ships, i),
            _ => value(ship, field),
        };
        select_trigger(field.key(), shown).width(Size::Fixed(150.0))
    };
    let indented = |els: Vec<El>, left: f32| {
        row(els)
            .gap(tokens::SPACE_1)
            .align(Align::Center)
            .padding(Sides {
                left,
                ..Sides::zero()
            })
    };
    let class = classes.iter().find(|c| ship.class.as_ref() == Some(&c.id));
    // The header stays in view; the steps scroll under it.
    let head = row([
        text(format!("Edit · {}", ship.name))
            .caption()
            .bold()
            .ellipsis()
            .width(Size::Fill(1.0)),
        small("+ Leg", Field::AddLeg),
        small("+ Wait", Field::AddWait),
        small("Done", Field::Done),
    ])
    .gap(tokens::SPACE_1)
    .align(Align::Center)
    .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2));
    let mut rows = vec![
        row([
            caption("Name"),
            input(editor, ship, Field::Name, selection, 200.0),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center),
        row([
            caption("Start"),
            menu(Field::Start),
            input(editor, ship, Field::Epoch, selection, 130.0),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center),
        row([caption("Class"), menu(Field::Class)])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
    ];
    // What it carries at its start: a classless ship, nothing.
    if let Some(class) = class {
        let full = if ship.propellant_t.is_none() {
            ", full"
        } else {
            ""
        };
        rows.push(
            row([
                caption("propellant"),
                input(editor, ship, Field::Propellant, selection, 70.0),
                caption(&format!("t of {}{full}", figure(class.tank))),
            ])
            .gap(tokens::SPACE_1)
            .align(Align::Center),
        );
        rows.push(caption(&format!(
            "cargo, t (a hold of {})",
            figure(class.cargo)
        )));
        for three in COMMODITIES.chunks(3) {
            rows.push(indented(
                three
                    .iter()
                    .flat_map(|&c| {
                        [
                            caption(c.name()),
                            input(editor, ship, Field::Cargo(c), selection, 50.0),
                        ]
                    })
                    .collect(),
                16.0,
            ));
        }
    }
    // A leg's or a dock's manifest, where the ship has tanks and a hold to
    // carry one.
    let manifest_rows = |i: usize| -> Vec<El> {
        if class.is_none() {
            return Vec::new();
        }
        let mut rows = vec![indented(
            vec![
                caption("manifest").tooltip(
                    "What to carry after the stop, t: more than this is given on arrival, less \
                     is taken on leaving, and what is not named is left as it is. A cargo at 0 \
                     gives it all.",
                ),
                caption("propellant"),
                input(editor, ship, Field::LegPropellant(i), selection, 70.0),
                caption("t"),
            ],
            16.0,
        )];
        for three in COMMODITIES.chunks(3) {
            rows.push(indented(
                three
                    .iter()
                    .flat_map(|&c| {
                        [
                            caption(c.name()),
                            input(editor, ship, Field::LegCargo(i, c), selection, 50.0),
                        ]
                    })
                    .collect(),
                32.0,
            ));
        }
        rows
    };
    for (i, step) in ship.steps.iter().enumerate() {
        let moves = [
            small("↑", Field::Up(i)),
            small("↓", Field::Down(i)),
            small("✕", Field::Remove(i)),
        ];
        match step {
            Step::Wait(_) => rows.push(
                row([
                    caption(&format!("{}. wait", i + 1)),
                    input(editor, ship, Field::Wait(i), selection, 70.0),
                    caption("d"),
                    spacer(),
                ]
                .into_iter()
                .chain(moves))
                .gap(tokens::SPACE_1)
                .align(Align::Center),
            ),
            Step::Go(go) => {
                rows.push(
                    row([
                        caption(&format!("{}. to", i + 1)),
                        menu(Field::To(i)),
                        spacer(),
                    ]
                    .into_iter()
                    .chain(moves))
                    .gap(tokens::SPACE_1)
                    .align(Align::Center),
                );
                let aero = switch(Field::Aero(i).key(), go.aero);
                rows.push(
                    row([
                        caption("days"),
                        input(editor, ship, Field::Days(i), selection, 80.0),
                        caption("aero"),
                        aero,
                    ])
                    .gap(tokens::SPACE_1)
                    .align(Align::Center)
                    .padding(Sides {
                        left: 16.0,
                        ..Sides::zero()
                    }),
                );
                rows.push(
                    row([
                        caption("window"),
                        input(editor, ship, Field::Open(i), selection, 110.0),
                        caption("–"),
                        input(editor, ship, Field::Close(i), selection, 110.0),
                    ])
                    .gap(tokens::SPACE_1)
                    .align(Align::Center)
                    .padding(Sides {
                        left: 16.0,
                        ..Sides::zero()
                    }),
                );
                if let Some(pin) = go.pin {
                    let depart = pin
                        .depart
                        .map(|d| format!(" leaving {}", orbit::mission::when(d)));
                    let days = pin.days.map(|d| format!(" taking {} d", figure(d)));
                    rows.push(caption(&format!(
                        "    pinned{}{} (the options' Unpin takes it off)",
                        depart.unwrap_or_default(),
                        days.unwrap_or_default()
                    )));
                }
                rows.extend(manifest_rows(i));
            }
            // A dock step: its vessel (the menu to choose one is step 4,
            // part 6's) and its manifest.
            Step::Dock(_) => {
                rows.push(
                    row([
                        caption(&format!("{}. dock to", i + 1)),
                        caption(&destination(ship, ships, i)),
                        spacer(),
                    ]
                    .into_iter()
                    .chain(moves))
                    .gap(tokens::SPACE_1)
                    .align(Align::Center),
                );
                rows.extend(manifest_rows(i));
            }
        }
        for (_, why) in problems.iter().filter(|p| p.0 == i) {
            rows.push(
                text(why.clone())
                    .caption()
                    .wrap_text()
                    .text_color(tokens::DESTRUCTIVE_TINT_FOREGROUND)
                    .width(Size::Fill(1.0)),
            );
        }
    }
    if let Some(why) = &editor.refused {
        rows.push(
            text(why.clone())
                .caption()
                .wrap_text()
                .text_color(tokens::WARNING_TINT_FOREGROUND)
                .width(Size::Fill(1.0)),
        );
    }
    rows.push(
        row([spacer(), small("Remove ship", Field::RemoveShip)])
            .gap(tokens::SPACE_2)
            .align(Align::Center),
    );
    column([
        head,
        scroll([column(rows)
            .gap(tokens::SPACE_1)
            .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_1))
            .width(Size::Fill(1.0))])
        .key(SCROLL_KEY)
        .height(Size::Fixed(320.0)),
    ])
    .width(Size::Fill(1.0))
}

/// The menu for `field` of `ship`: for a leg's destination, every other
/// ship of the plan's `ships` by name, where it starts and after each of
/// its legs ("Depot, leg 2 — nrho"), then every place by id, with its
/// name (a start's menu: the places); or, for the class, "none" and every
/// class, with its propellant, tanks and hold.
pub fn menu(
    field: Field,
    ship: &Ship,
    ships: &[Ship],
    places: &Places,
    classes: &[ShipClass],
) -> El {
    let vessels = || {
        ships
            .iter()
            .filter(|v| v.id != ship.id)
            .flat_map(|v| {
                let start = Meeting {
                    ship: v.id,
                    leg: None,
                };
                let legs = v.steps.iter().enumerate().filter_map(move |(k, s)| {
                    let m = Meeting {
                        ship: v.id,
                        leg: Some(k),
                    };
                    match s {
                        Step::Go(go) => Some((m, format!("{}, leg {k} — {}", v.name, go.to.label))),
                        Step::Dock(d) => {
                            Some((m, format!("{}, leg {k} — docked to {}", v.name, d.to.label)))
                        }
                        Step::Wait(_) => None,
                    }
                });
                std::iter::once((start, format!("{}, start — {}", v.name, v.start.label)))
                    .chain(legs)
            })
            .map(|(m, label)| (meet_option(&m), label))
    };
    let options: Vec<(String, String)> = match field {
        Field::Class => std::iter::once(("none".to_string(), "none — massless".to_string()))
            .chain(classes.iter().map(|c| {
                (
                    c.id.clone(),
                    format!(
                        "{} — {:?}, tanks {} t, hold {} t",
                        c.id,
                        c.propellant,
                        figure(c.tank),
                        figure(c.cargo)
                    )
                    .to_lowercase(),
                )
            }))
            .collect(),
        Field::To(_) => vessels()
            .chain(
                places
                    .iter()
                    .map(|p| (p.id.clone(), format!("{} — {}", p.id, p.name))),
            )
            .collect(),
        _ => places
            .iter()
            .map(|p| (p.id.clone(), format!("{} — {}", p.id, p.name)))
            .collect(),
    };
    select_menu(field.key(), options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit::mission::Plan;

    fn ship() -> Ship {
        let src = r#"
name = "x"
[[ship]]
name = "Lander"
start = { at = "nrho", epoch = "2045-03-05" }
[[ship.leg]]
to = "llo"
days = [0.5]
[[ship.leg]]
wait = 2
[[ship.leg]]
to = "nrho"
days = [0.5, 1]
window = ["2045-03-08", "2045-03-10"]
"#;
        Plan::parse(src, orbit::ephem::Ephemeris::standard(), Places::standard())
            .unwrap()
            .ships
            .remove(0)
    }

    #[test]
    fn keys_parse() {
        for f in [
            Field::Name,
            Field::Start,
            Field::Epoch,
            Field::Class,
            Field::Propellant,
            Field::Cargo(Commodity::Lh2),
            Field::Cargo(Commodity::Equipment),
            Field::To(3),
            Field::Days(0),
            Field::Open(1),
            Field::Close(1),
            Field::Aero(2),
            Field::Wait(4),
            Field::LegPropellant(2),
            Field::LegCargo(0, Commodity::Lox),
            Field::LegCargo(3, Commodity::Metal),
            Field::Up(1),
            Field::Down(2),
            Field::Remove(0),
            Field::AddLeg,
            Field::AddWait,
            Field::RemoveShip,
            Field::Done,
        ] {
            assert_eq!(Field::parse(&f.key()), Some(f));
        }
        assert_eq!(Field::parse("edit:2:to:option:llo"), None);
        assert_eq!(Field::parse("edit:2:to:option:meet:0:1"), None);
        assert_eq!(Field::parse("edit:cargo:water"), None);
        assert_eq!(Field::parse("edit:2:cargo:water"), None);
        assert_eq!(Field::parse("plan"), None);
        for m in [
            Meeting { ship: 3, leg: None },
            Meeting {
                ship: 0,
                leg: Some(2),
            },
        ] {
            assert_eq!(parse_meet_option(&meet_option(&m)), Some(m));
        }
        assert_eq!(parse_meet_option("meet:x"), None);
        assert_eq!(parse_meet_option("meet:1:y"), None);
        assert_eq!(parse_meet_option("llo"), None);
    }

    /// Where `t` first differs from `s` as the planner reads them: `None`
    /// if nowhere, `Some(None)` at its start, else at the step.
    fn first_change(s: &Ship, t: &Ship) -> Option<Option<usize>> {
        if format!("{:?}", (&s.start, s.epoch)) != format!("{:?}", (&t.start, t.epoch)) {
            return Some(None);
        }
        (0..s.steps.len().max(t.steps.len()))
            .find(|&i| format!("{:?}", s.steps.get(i)) != format!("{:?}", t.steps.get(i)))
            .map(Some)
    }

    /// Fields read what they set, and refuse what the plan's file would.
    #[test]
    fn fields_set_and_read() {
        let s = ship();
        assert_eq!(value(&s, Field::Days(2)), "0.5, 1");
        assert_eq!(value(&s, Field::Open(2)), "2045-03-08");
        let t = set(&s, Field::Days(2), "1, 2.25").unwrap();
        assert_eq!(value(&t, Field::Days(2)), "1, 2.25");
        assert_eq!(first_change(&s, &t), Some(Some(2)));
        let t = set(&s, Field::Wait(1), "3.5").unwrap();
        assert_eq!(first_change(&s, &t), Some(Some(1)));
        let t = set(&s, Field::Close(2), "").unwrap();
        assert_eq!(value(&t, Field::Open(2)), "2045-03-08");
        assert_eq!(value(&t, Field::Close(2)), "2045-03-08");
        let t = set(&s, Field::Epoch, "2045-03-06T12:00").unwrap();
        assert_eq!(first_change(&s, &t), Some(None));
        let t = set(&s, Field::Name, "Other").unwrap();
        assert_eq!(first_change(&s, &t), None);
        assert!(set(&s, Field::Days(2), "0").is_err());
        assert!(set(&s, Field::Days(2), "x").is_err());
        assert!(set(&s, Field::Wait(1), "-1").is_err());
        assert!(set(&s, Field::Wait(1), "NaN").is_err());
        assert!(set(&s, Field::Days(2), "inf").is_err());
        assert!(set(&s, Field::Open(2), "2045-03-11").is_err());
        assert!(set(&s, Field::Epoch, "2045-02-30").is_err());
        assert!(set(&s, Field::Name, "  ").is_err());
        // A wait's draft set into what is not a wait, or no step at all (an
        // undo under it: found in review, it panicked).
        assert!(set(&s, Field::Wait(0), "2").is_err());
        assert!(set(&s, Field::Wait(9), "2").is_err());
    }

    /// Steps move, go and come; places change.
    #[test]
    fn steps_move_and_places_change() {
        let (s, places) = (ship(), Places::standard());
        let t = press(&s, Field::Down(0), places).unwrap();
        assert!(matches!(t.steps[0], Step::Wait(_)));
        assert_eq!(first_change(&s, &t), Some(Some(0)));
        assert!(press(&s, Field::Up(0), places).is_none());
        assert!(press(&s, Field::Down(2), places).is_none());
        let t = press(&s, Field::Remove(1), places).unwrap();
        assert_eq!(t.steps.len(), 2);
        assert_eq!(first_change(&s, &t), Some(Some(1)));
        let t = press(&s, Field::AddWait, places).unwrap();
        assert_eq!(first_change(&s, &t), Some(Some(3)));
        let t = press(&s, Field::Aero(0), places).unwrap();
        assert!(matches!(&t.steps[0], Step::Go(g) if g.aero));
        let t = place(&s, Field::To(0), "moon-south-pole", places, &[]).unwrap();
        assert_eq!(value(&t, Field::To(0)), "moon-south-pole");
        assert_eq!(first_change(&s, &t), Some(Some(0)));
        assert!(
            place(&s, Field::To(1), "llo", places, &[]).is_err(),
            "a wait has no place"
        );
        assert!(place(&s, Field::Start, "nowhere", places, &[]).is_err());
    }

    /// A leg's destination chosen among the plan's vessels (M1b.md, step 3
    /// plan, part 3): the meeting, where the vessel is then, as the plan
    /// writes it and as the editor names it; a vessel the plan has not, a
    /// wait of its, or a start, refused.
    #[test]
    fn a_leg_meets_a_vessel() {
        let places = Places::standard();
        let mut depot = ship();
        depot.id = 7;
        depot.name = "Depot".into();
        let s = ship();
        let ships = [depot.clone(), s.clone()];
        let t = place(&s, Field::To(0), "meet:7:2", places, &ships).unwrap();
        let Step::Go(go) = &t.steps[0] else {
            unreachable!()
        };
        assert_eq!(
            go.to.meeting,
            Some(Meeting {
                ship: 7,
                leg: Some(2)
            })
        );
        assert_eq!(go.to.label, "ship 7, leg 2");
        let Step::Go(met) = &depot.steps[2] else {
            unreachable!()
        };
        assert_eq!(format!("{:?}", go.to.kind), format!("{:?}", met.to.kind));
        assert_eq!(go.to.source.to_string(), "{ leg = 2, ship = 7 }");
        assert_eq!(destination(&t, &ships, 0), "Depot, leg 2");
        assert_eq!(destination(&t, &[], 0), "ship 7, leg 2");
        assert_eq!(first_change(&s, &t), Some(Some(0)));
        let t = place(&s, Field::To(0), "meet:7", places, &ships).unwrap();
        assert_eq!(destination(&t, &ships, 0), "Depot, start");
        assert_eq!(destination(&t, &ships, 1), "", "a wait goes nowhere");
        assert_eq!(destination(&s, &ships, 0), "llo");
        let refused = |id: &str| place(&s, Field::To(0), id, places, &ships).unwrap_err();
        assert!(
            refused("meet:9").contains("no ship"),
            "{}",
            refused("meet:9")
        );
        assert!(
            refused("meet:7:1").contains("wait"),
            "{}",
            refused("meet:7:1")
        );
        assert!(
            refused("meet:7:5").contains("no leg 5"),
            "{}",
            refused("meet:7:5")
        );
        assert_eq!(
            place(&s, Field::Start, "meet:7", places, &ships).unwrap_err(),
            "a start is a place"
        );
    }

    /// A leg's manifest typed as the engine reads it (M1b.md, step 3 plan,
    /// part 4): a class first; empty leaves a commodity as it is, 0 gives
    /// it all; a negative amount refused; a wait has none.
    #[test]
    fn a_manifest_is_set_and_read() {
        let s = ship();
        assert_eq!(
            set(&s, Field::LegPropellant(0), "20").unwrap_err(),
            "a manifest needs a class: choose one first"
        );
        let mut s = s;
        s.class = Some("lander".into());
        assert_eq!(value(&s, Field::LegPropellant(0)), "");
        let t = set(&s, Field::LegPropellant(0), "20.5").unwrap();
        let t = set(&t, Field::LegCargo(0, Commodity::Equipment), "0").unwrap();
        let t = set(&t, Field::LegCargo(2, Commodity::Lh2), "1.25").unwrap();
        assert_eq!(value(&t, Field::LegPropellant(0)), "20.5");
        assert_eq!(value(&t, Field::LegCargo(0, Commodity::Equipment)), "0");
        assert_eq!(value(&t, Field::LegCargo(2, Commodity::Lh2)), "1.25");
        assert_eq!(value(&t, Field::LegCargo(0, Commodity::Lh2)), "");
        let Step::Go(go) = &t.steps[0] else {
            unreachable!()
        };
        assert_eq!(go.manifest.propellant_t, Some(20.5));
        assert_eq!(go.manifest.cargo.get("equipment"), Some(&0.0));
        assert_eq!(
            first_change(&s, &t),
            Some(Some(0)),
            "a manifest is the leg's (the game designs nothing for it)"
        );
        let u = set(&t, Field::LegCargo(0, Commodity::Equipment), "").unwrap();
        let Step::Go(go) = &u.steps[0] else {
            unreachable!()
        };
        assert!(go.manifest.cargo.is_empty());
        let u = set(&t, Field::LegPropellant(0), "").unwrap();
        assert_eq!(value(&u, Field::LegPropellant(0)), "");
        assert!(set(&t, Field::LegPropellant(0), "-1").is_err());
        assert!(set(&t, Field::LegCargo(0, Commodity::Lox), "NaN").is_err());
        assert_eq!(
            set(&t, Field::LegPropellant(1), "1").unwrap_err(),
            "not a leg or a dock"
        );
    }

    /// A class from its menu ("none" carries nothing), and a load typed as
    /// the engine reads it: empty propellant, a full tank; empty or no
    /// cargo, none.
    #[test]
    fn class_and_load_set_and_read() {
        let s = ship();
        let classes = sim::params::EconomyFile::standard()
            .draw(
                orbit::ephem::Ephemeris::standard(),
                orbit::routes::RouteGraph::standard(),
                &mut sim::params::Draw::Central,
            )
            .ships;
        let places = Places::standard();
        assert_eq!(value(&s, Field::Class), "none");
        assert!(
            set(&s, Field::Propellant, "100").is_err(),
            "a load needs a class"
        );
        let t = choose(&s, Field::Class, "tug-h", places, &[], &classes).unwrap();
        assert_eq!(value(&t, Field::Class), "tug-h");
        let t = set(&t, Field::Propellant, "120.5").unwrap();
        let t = set(&t, Field::Cargo(Commodity::Equipment), "20").unwrap();
        assert_eq!(value(&t, Field::Propellant), "120.5");
        assert_eq!(value(&t, Field::Cargo(Commodity::Equipment)), "20");
        assert_eq!(t.cargo.get("equipment"), Some(&20.0));
        let u = set(&t, Field::Cargo(Commodity::Equipment), "0").unwrap();
        assert!(u.cargo.is_empty());
        let u = set(&t, Field::Propellant, "").unwrap();
        assert_eq!(u.propellant_t, None);
        assert!(set(&t, Field::Propellant, "-1").is_err());
        assert!(set(&t, Field::Cargo(Commodity::Lh2), "NaN").is_err());
        assert!(choose(&s, Field::Class, "barge", places, &[], &classes).is_err());
        let none = choose(&t, Field::Class, "none", places, &[], &classes).unwrap();
        assert_eq!(none.class, None);
        assert_eq!(none.propellant_t, None);
        assert!(none.cargo.is_empty());
        assert_eq!(
            choose(&s, Field::To(0), "nrho", places, &[], &classes)
                .map(|t| value(&t, Field::To(0))),
            Ok("nrho".into())
        );
    }
}
