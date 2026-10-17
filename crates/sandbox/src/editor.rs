//! Editing a ship (M1c.md, step 7 plan, item 6): its name, its start,
//! and its steps (a leg's destination, durations, window and
//! aerocapture; a wait's days), steps added, removed and moved. Each
//! change makes a new ship, which the plan designs again from the first
//! step it changes; legs that cannot be flown say why at once, by the
//! planner's own rules (`mission::check`).

use damascene_core::prelude::*;
use orbit::mission::{Go, Ship, Spot, Step};
use orbit::places::Places;
use orbit::time::Epoch;
use std::collections::HashMap;

/// A control of the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Name,
    /// The ship's start: its place, and when.
    Start,
    Epoch,
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
            "add-leg" => Field::AddLeg,
            "add-wait" => Field::AddWait,
            "remove-ship" => Field::RemoveShip,
            "done" => Field::Done,
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
    /// The place menu open: the start's or a leg's.
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

/// Days as a field shows them: "5", "0.5", "190.25".
fn days(d: f64) -> String {
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
        Field::To(i) => go(i).map_or(String::new(), |g| g.to.label.clone()),
        Field::Days(i) => go(i).map_or(String::new(), |g| {
            g.days
                .iter()
                .map(|&d| days(d))
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
            Some(Step::Wait(d)) => days(*d),
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
    match field {
        Field::Name => {
            if text.is_empty() {
                return Err("a ship has a name".into());
            }
            ship.name = text.to_string();
        }
        Field::Epoch => ship.epoch = epoch(text)?,
        Field::Wait(i) => {
            let d: f64 = text
                .parse()
                .map_err(|_| format!("{text:?} is not a number"))?;
            if !d.is_finite() || d < 0.0 {
                return Err("a wait is a number of days, not negative".into());
            }
            ship.steps[i] = Step::Wait(d);
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

/// `ship` with its start's place, or step `i`'s destination, set to the
/// place `id`.
pub fn place(ship: &Ship, field: Field, id: &str, places: &Places) -> Result<Ship, String> {
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
/// (`problems`, by step) and the selection the fields share.
pub fn section(
    editor: &Editor,
    ship: &Ship,
    problems: &[(usize, String)],
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
    ];
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
                    let days = pin.days.map(|d| format!(" taking {} d", days(d)));
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

/// The place menu for `field`: every place by id, with its name.
pub fn place_menu(field: Field, places: &Places) -> El {
    select_menu(
        field.key(),
        places
            .iter()
            .map(|p| (p.id.clone(), format!("{} — {}", p.id, p.name)))
            .collect::<Vec<_>>(),
    )
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
}
