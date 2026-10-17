//! Editing a ship (M1c.md, step 7 plan, item 6): its name, its start,
//! its class and load (M1b.md, step 2 plan, part 4), and its steps (a
//! leg's destination, durations, window and aerocapture; a wait's days),
//! steps added, removed and moved. Each change makes a new ship, which the
//! plan designs again from the first step it changes (a new class or load
//! designs nothing); legs that cannot be flown say why at once, by the
//! planner's own rules (`mission::check`).

use damascene_core::prelude::*;
use orbit::mission::{Go, Ship, Spot, Step};
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
            _ if rest.starts_with("cargo:") => {
                let id = rest.strip_prefix("cargo:")?;
                Field::Cargo(*COMMODITIES.iter().find(|c| c.id() == id)?)
            }
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
        )
    }
}

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
    if matches!(field, Field::Propellant | Field::Cargo(_)) && ship.class.is_none() {
        return Err("a load needs a class: choose one first".into());
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
/// `i`'s destination, from `places`; its class, from `classes` ("none":
/// massless, carrying nothing).
pub fn choose(
    ship: &Ship,
    field: Field,
    id: &str,
    places: &Places,
    classes: &[ShipClass],
) -> Result<Ship, String> {
    if field != Field::Class {
        return place(ship, field, id, places);
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
/// place `id`.
fn place(ship: &Ship, field: Field, id: &str, places: &Places) -> Result<Ship, String> {
    let spot = Spot::named(id, places).ok_or_else(|| format!("no place {id:?}"))?;
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
            note: None,
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

/// The editor's section for `ship`, with the legs that cannot be flown
/// (`problems`, by step), the classes a ship may have, and the selection
/// the fields share.
pub fn section(
    editor: &Editor,
    ship: &Ship,
    problems: &[(usize, String)],
    classes: &[ShipClass],
    selection: &Selection,
) -> El {
    let caption = |s: &str| text(s.to_string()).caption().muted();
    let menu =
        |field: Field| select_trigger(field.key(), value(ship, field)).width(Size::Fixed(150.0));
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
    if let Some(class) = classes.iter().find(|c| ship.class.as_ref() == Some(&c.id)) {
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
            rows.push(
                row(three.iter().flat_map(|&c| {
                    [
                        caption(c.name()),
                        input(editor, ship, Field::Cargo(c), selection, 50.0),
                    ]
                }))
                .gap(tokens::SPACE_1)
                .align(Align::Center)
                .padding(Sides {
                    left: 16.0,
                    ..Sides::zero()
                }),
            );
        }
    }
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
        .key("edit:scroll")
        .height(Size::Fixed(320.0)),
    ])
    .width(Size::Fill(1.0))
}

/// The menu for `field`: every place by id, with its name; or, for the
/// class, "none" and every class, with its propellant, tanks and hold.
pub fn menu(field: Field, places: &Places, classes: &[ShipClass]) -> El {
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
        assert_eq!(Field::parse("edit:cargo:water"), None);
        assert_eq!(Field::parse("plan"), None);
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
        let t = place(&s, Field::To(0), "moon-south-pole", places).unwrap();
        assert_eq!(value(&t, Field::To(0)), "moon-south-pole");
        assert_eq!(first_change(&s, &t), Some(Some(0)));
        assert!(
            place(&s, Field::To(1), "llo", places).is_err(),
            "a wait has no place"
        );
        assert!(place(&s, Field::Start, "nowhere", places).is_err());
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
        let t = choose(&s, Field::Class, "tug-h", places, &classes).unwrap();
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
        assert!(choose(&s, Field::Class, "barge", places, &classes).is_err());
        let none = choose(&t, Field::Class, "none", places, &classes).unwrap();
        assert_eq!(none.class, None);
        assert_eq!(none.propellant_t, None);
        assert!(none.cargo.is_empty());
        assert_eq!(
            choose(&s, Field::To(0), "nrho", places, &classes).map(|t| value(&t, Field::To(0))),
            Ok("nrho".into())
        );
    }
}
