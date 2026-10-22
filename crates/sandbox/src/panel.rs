//! The plan's panel (M1c.md, step 5 plan, item 2): beside the map, each
//! ship and its items, with their times and Δv, the failures and why; its
//! tanks and hold, what each burn took, where it ran dry (M1b.md, step 2
//! plan, part 4); what it was bought for, and each stop's transfers
//! (M1b.md, step 3 plan, parts 5 and 7).

use crate::labels::{grouped, mps};
use crate::plan::{Ship, Tanks};
use damascene_core::prelude::*;
use orbit::ephem::Ephemeris;
use orbit::mission::{Item, ItemKind, MarkerKind, when};
use orbit::time::Epoch;
use sim::report::musd;
use sim::ships::{COMMODITIES, Commodity, Load};
use sim::state::{Bought, Kind, Purchase, Transfer, With};
use std::sync::Arc;

/// The panel's list's key.
pub const LIST_KEY: &str = "plan";

/// The button that adds a ship.
pub const ADD_SHIP_KEY: &str = "edit:add-ship";

/// A ship's row's key: a click on it edits the ship.
pub fn ship_key(s: usize) -> String {
    format!("ship:{s}")
}

/// The ship a ship's row's key names.
pub fn parse_ship_key(key: &str) -> Option<usize> {
    key.strip_prefix("ship:")?.parse().ok()
}

/// The panel's width, logical pixels.
pub const WIDTH: f32 = 380.0;

/// A row's height before it is measured, px.
const ROW_PX: f32 = 44.0;

/// Less Δv than this, km/s, is shown as none (a stay's station-keeping).
const NO_DV: f64 = 0.5e-3;

/// An item's row's key.
pub fn item_key(s: usize, k: usize) -> String {
    format!("item:{s}:{k}")
}

/// The ship and item an item's row's key names.
pub fn parse_item_key(key: &str) -> Option<(usize, usize)> {
    let (s, k) = key.strip_prefix("item:")?.split_once(':')?;
    Some((s.parse().ok()?, k.parse().ok()?))
}

/// How a line of a row reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Warning,
    Failure,
}

/// A row of the panel, as text: a ship's, or an item's.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub key: String,
    /// An item's: its kind; a ship's: empty.
    pub kind: String,
    /// The item's label, or the ship's name.
    pub label: String,
    /// Its Δv, or the ship's total; empty for none.
    pub dv: String,
    pub lines: Vec<(String, Tone)>,
    /// The item a ship is in at the clock's time.
    pub current: bool,
    pub selected: bool,
}

/// The item `ship` is in at `t`, if one spans it.
pub fn current(ship: &Ship, t: Epoch) -> Option<usize> {
    ship.items
        .iter()
        .rposition(|i| i.start.0 <= t.0 && t.0 <= i.end.0)
}

/// Days, to a precision that reads.
fn days(d: f64) -> String {
    if d < 10.0 {
        format!("{d:.2} d")
    } else {
        format!("{d:.1} d")
    }
}

/// The panel's rows for a plan's `ships` at `t`, `selected` marked.
pub fn rows(
    eph: &Ephemeris,
    ships: &[Ship],
    t: Epoch,
    selected: Option<(usize, usize)>,
) -> Vec<Row> {
    let mut rows = Vec::new();
    for (s, ship) in ships.iter().enumerate() {
        let base = rows.len();
        let state = match (&ship.failure, ship.done) {
            (Some(_), _) => "failed",
            (None, true) => "designed",
            (None, false) => "designing",
        };
        let mut lines = vec![(
            match (ship.items.first(), ship.items.last()) {
                (Some(first), Some(last)) => {
                    format!("{state} · {}", days(last.end.0 - first.start.0))
                }
                _ => state.to_string(),
            },
            Tone::Muted,
        )];
        // Why it stopped, where no failed item says (a ship refused, at
        // its start or when designing it panicked).
        if let Some(why) = &ship.failure
            && !ship
                .items
                .iter()
                .any(|i| matches!(i.kind, ItemKind::Failed(_)))
        {
            lines.push((why.clone(), Tone::Failure));
        }
        // What its class does not allow: on the step's first item past
        // its stay (a leg, or a landing), once it is designed; on the ship,
        // its start's and those of steps not yet.
        let past_stay = |step: usize| {
            ship.items
                .iter()
                .position(|i| i.step == step && i.kind != ItemKind::Stay)
        };
        for w in &ship.warnings {
            if w.step.and_then(past_stay).is_none() {
                lines.push((w.to_string(), Tone::Warning));
            }
        }
        // Its class, its tanks and hold at the clock, and where it ran
        // dry (M1b.md, step 2 plan, part 4).
        if let Some(tanks) = &ship.tanks {
            let now = match tanks.load(t) {
                _ if tanks.pending(t) => " · tanks pending".to_string(),
                Some(load) => format!(" · {}", carried(&load, tanks.fuel)),
                None => String::new(),
            };
            lines.push((format!("{}{now}", tanks.class), Tone::Muted));
            if !tanks.state.purchases.is_empty() {
                lines.push((bought(&tanks.state.purchases), Tone::Muted));
            }
            if let Some(dry) = tanks.dry() {
                let at = ship
                    .items
                    .get(dry.item)
                    .map_or(String::new(), |i| dry.place(i));
                let line = match dry.stranded() {
                    true => format!("Stranded {} {at}, not flown after", when(dry.t)),
                    false => format!(
                        "Ran dry {} {at}: {} short, not flown after",
                        when(dry.t),
                        short(dry.short)
                    ),
                };
                lines.push((line, Tone::Failure));
            }
        }
        let dv: f64 = ship.items.iter().map(|i| i.dv).sum();
        rows.push(Row {
            key: ship_key(s),
            kind: String::new(),
            label: ship.name.clone(),
            dv: if ship.items.is_empty() {
                String::new()
            } else {
                mps(dv)
            },
            lines,
            current: false,
            selected: false,
        });
        let now = current(ship, t);
        for (k, item) in ship.items.iter().enumerate() {
            let mut row = item_row(eph, item);
            if past_stay(item.step) == Some(k) {
                for w in ship.warnings.iter().filter(|w| w.step == Some(item.step)) {
                    row.lines.push((w.why.clone(), Tone::Warning));
                }
            }
            if let Some(tanks) = &ship.tanks {
                row.lines.extend(burned(tanks, item, k));
            }
            row.key = item_key(s, k);
            row.current = now == Some(k);
            row.selected = selected == Some((s, k));
            rows.push(row);
        }
        // Its transfers, on the stop each was made at (the ship's own row
        // where no item is the stop's: not expected, and not dropped).
        if let Some(tanks) = &ship.tanks {
            let mut stops: Vec<(Option<usize>, Vec<&Transfer>)> = Vec::new();
            for x in &tanks.state.transfers {
                let at = stop_item(&ship.items, x.step);
                match stops.iter_mut().find(|(k, _)| *k == at) {
                    Some((_, made)) => made.push(x),
                    None => stops.push((at, vec![x])),
                }
            }
            for (at, made) in stops {
                let r = at.map_or(base, |k| base + 1 + k);
                rows[r].lines.extend(transferred(ships, &made));
            }
        }
    }
    rows
}

/// The item a stop's transfers are shown on, for the stop the leg at
/// `step` arrives at: the stay after it, or the arrival itself where the
/// ship does not stay (its plan ends there, or its next leg leaves at
/// once).
fn stop_item(items: &[Item], step: usize) -> Option<usize> {
    let arrival = items.iter().rposition(|i| i.step == step)?;
    match items.get(arrival + 1) {
        Some(next) if next.kind == ItemKind::Stay => Some(arrival + 1),
        _ => Some(arrival),
    }
}

/// Whom a transfer was with: the vessel by its name among `ships`.
fn whom(ships: &[Ship], with: With) -> String {
    match with {
        With::Vessel { id, .. } => ships
            .iter()
            .find(|s| s.id == id)
            .map_or_else(|| format!("ship {id}"), |s| s.name.clone()),
        With::Earth => "Earth".into(),
        With::NoOne => "no one".into(),
    }
}

/// A stop's transfers, `made` in time order: those made in full as one
/// line each way ("gave LOX 60.0 t, LH2 8.6 t to Depot"; at LEO, "bought
/// LOX 85.7 t from Earth for $12.3M"), and each that fell short with why.
fn transferred(ships: &[Ship], made: &[&Transfer]) -> Vec<(String, Tone)> {
    let mut lines = Vec::new();
    for kind in [Kind::Give, Kind::Take] {
        let full: Vec<&&Transfer> = made
            .iter()
            .filter(|x| x.kind == kind && x.short.is_none())
            .collect();
        let Some(first) = full.first() else {
            continue;
        };
        let amounts: Vec<String> = full
            .iter()
            .map(|x| format!("{} {}", x.commodity.name(), mass(x.got)))
            .collect();
        let with = whom(ships, first.with);
        let cost: f64 = full.iter().filter_map(|x| x.cost_musd).sum();
        let line = match (kind, first.with) {
            (Kind::Give, _) => format!("gave {} to {with}", amounts.join(", ")),
            (Kind::Take, With::Earth) => {
                format!(
                    "bought {} from {with} for {}",
                    amounts.join(", "),
                    musd(cost)
                )
            }
            (Kind::Take, _) => format!("took {} from {with}", amounts.join(", ")),
        };
        lines.push((line, Tone::Muted));
    }
    for x in made.iter().filter(|x| x.short.is_some()) {
        let kind = match x.kind {
            Kind::Give => "give",
            Kind::Take => "take",
        };
        lines.push((
            format!(
                "{kind} of {} of {} fell short: {} ({})",
                mass(x.asked),
                x.commodity.name(),
                mass(x.got),
                x.short.expect("filtered")
            ),
            Tone::Warning,
        ));
    }
    lines
}

/// What a ship was bought for at LEO: "Bought at LEO for $1,150M: the ship
/// $900M, LOX 85.7 t $200M, LH2 12.2 t $50M".
fn bought(purchases: &[Purchase]) -> String {
    let total: f64 = purchases.iter().map(|p| p.musd).sum();
    let parts: Vec<String> = purchases
        .iter()
        .map(|p| match p.what {
            Bought::Ship => format!("the ship {}", musd(p.musd)),
            Bought::Commodity(c) => format!("{} {} {}", c.name(), mass(p.tonnes), musd(p.musd)),
        })
        .collect();
    format!("Bought at LEO for {}: {}", musd(total), parts.join(", "))
}

/// A shortfall, km/s: in m/s, to a tenth under 100; under a tenth, in
/// mm/s (a day's upkeep).
fn short(dv: f64) -> String {
    let m = dv * 1e3;
    if m >= 100.0 {
        mps(dv)
    } else if m >= 0.1 {
        format!("{m:.1} m/s")
    } else {
        format!("{:.1} mm/s", m * 1e3)
    }
}

/// Tonnes, to a precision that reads, down to grams.
fn mass(t: f64) -> String {
    if t >= 10.0 {
        format!("{t:.1} t")
    } else if t >= 1.0 {
        format!("{t:.2} t")
    } else if t >= 1e-3 {
        format!("{:.1} kg", t * 1e3)
    } else {
        format!("{:.0} g", t * 1e6)
    }
}

/// What a ship carries: its tanks' LOX and fuel, and its hold by
/// commodity, if it holds anything.
fn carried(load: &Load, fuel: Commodity) -> String {
    let mut s = format!(
        "{} {}, {} {}",
        Commodity::Lox.name(),
        mass(load.lox),
        fuel.name(),
        mass(load.fuel)
    );
    let held: Vec<String> = COMMODITIES
        .iter()
        .filter(|&&c| load.cargo[c as usize] > 0.0)
        .map(|&c| format!("{} {}", c.name(), mass(load.cargo[c as usize])))
        .collect();
    if !held.is_empty() {
        s.push_str(&format!(" · hold {}", held.join(", ")));
    }
    s
}

/// What item `k` of a ship with `tanks` burned, and what it left; where
/// the ship ran dry, and that what follows is not flown; pending where the
/// loop has not run to its end.
fn burned(tanks: &Tanks, item: &Item, k: usize) -> Vec<(String, Tone)> {
    if tanks.pending(item.end) {
        return vec![("tanks pending".into(), Tone::Muted)];
    }
    let state = &tanks.state;
    let mut lines = Vec::new();
    let used = state.used.get(k).copied().unwrap_or(0.0);
    if used > 0.0 {
        let what = match item.kind {
            ItemKind::Stay => "upkeep",
            _ => "propellant",
        };
        let left = state
            .after
            .get(k)
            .copied()
            .flatten()
            .map_or(String::new(), |load| {
                format!(" · left {}", carried(&load, tanks.fuel))
            });
        lines.push((format!("{what} {}{left}", mass(used)), Tone::Muted));
    }
    match tanks.dry() {
        Some(dry) if dry.item == k => lines.push((
            match (dry.with, dry.marker) {
                (Some(host), _) => format!("Stranded with ship {host}, which ran dry"),
                (None, Some(_)) => format!("Ran dry: the burn is {} short", short(dry.short)),
                (None, None) => format!("Ran dry keeping station: {} short", short(dry.short)),
            },
            Tone::Failure,
        )),
        Some(dry) if k > dry.item => lines.push((
            match dry.stranded() {
                true => "Not flown: the ship is stranded".into(),
                false => "Not flown: the ship ran dry".into(),
            },
            Tone::Muted,
        )),
        _ => {}
    }
    lines
}

/// An item's row, unkeyed.
pub fn item_row(eph: &Ephemeris, item: &Item) -> Row {
    let kind = match &item.kind {
        ItemKind::Leg => "leg",
        ItemKind::Stay => "stay",
        ItemKind::Capture => "capture",
        ItemKind::Landing => "landing",
        ItemKind::Ascent => "ascent",
        ItemKind::Docked(_) => "docked",
        ItemKind::Failed(_) => "failed",
    };
    let mut lines = vec![(
        format!("{} · {}", when(item.start), days(item.end.0 - item.start.0)),
        Tone::Muted,
    )];
    // A leg's burns, where there is more to say than its Δv.
    let burns: Vec<String> = item
        .markers
        .iter()
        .filter_map(|m| {
            let at = |what: &str| {
                m.pass.map_or(String::new(), |(b, km)| {
                    format!(" ({what} {}, {km:.0} km)", eph.body(b).name)
                })
            };
            let dv = grouped(m.dv * 1e3);
            match m.kind {
                MarkerKind::Burn => Some(dv),
                MarkerKind::Flyby => Some(format!("{dv}{}", at("flyby"))),
                MarkerKind::Pass => Some(format!("{dv}{}", at("pass"))),
                _ => None,
            }
        })
        .collect();
    if item.kind == ItemKind::Leg && (burns.len() > 1 || burns.iter().any(|b| b.contains('('))) {
        lines.push((format!("burns {} m/s", burns.join(" + ")), Tone::Muted));
    }
    if let ItemKind::Failed(why) = &item.kind {
        lines.push((why.clone(), Tone::Failure));
    }
    for m in &item.markers {
        if let MarkerKind::Jump { km, plane_deg } = m.kind {
            lines.push((
                format!(
                    "Jump at departure: the leg leaves {km:.0} km from the ship, in a plane \
                     {plane_deg:.1}° off its orbit ({}), not flown",
                    mps(m.dv)
                ),
                Tone::Warning,
            ));
        }
    }
    if let Some(note) = &item.note {
        lines.push((note.clone(), Tone::Muted));
    }
    Row {
        key: String::new(),
        kind: kind.to_string(),
        label: item.label.clone(),
        dv: if item.dv >= NO_DV {
            mps(item.dv)
        } else {
            String::new()
        },
        lines,
        current: false,
        selected: false,
    }
}

/// A row as drawn.
fn drawn(r: &Row) -> El {
    let ship = r.kind.is_empty();
    let label = if ship {
        text(r.label.clone()).bold()
    } else {
        text(r.label.clone()).caption()
    };
    let mut head = vec![];
    if !ship {
        head.push(
            text(if r.current { "▸" } else { "" })
                .caption()
                .width(Size::Fixed(10.0)),
        );
        head.push(
            text(r.kind.clone())
                .caption()
                .muted()
                .width(Size::Fixed(52.0)),
        );
    }
    head.extend([
        label,
        spacer(),
        text(r.dv.clone()).caption().tabular_numerals(),
    ]);
    let mut parts = vec![row(head).gap(tokens::SPACE_1).align(Align::Center)];
    let indent = if ship { 0.0 } else { 62.0 };
    parts.extend(r.lines.iter().map(|(s, tone)| {
        let line = text(s.clone()).caption().wrap_text().width(Size::Fill(1.0));
        let line = match tone {
            Tone::Muted => line.muted(),
            Tone::Warning => line.text_color(tokens::WARNING_TINT_FOREGROUND),
            Tone::Failure => line.text_color(tokens::DESTRUCTIVE_TINT_FOREGROUND),
        };
        row([line]).padding(Sides {
            left: indent,
            ..Sides::zero()
        })
    }));
    let el = column(parts)
        .gap(2.0)
        .width(Size::Fill(1.0))
        .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_1));
    if ship {
        el.padding(Sides {
            top: tokens::SPACE_3,
            bottom: tokens::SPACE_1,
            left: tokens::SPACE_3,
            right: tokens::SPACE_3,
        })
        .key(r.key.clone())
        .cursor(Cursor::Pointer)
        .tooltip("Click to edit")
    } else {
        let el = el.key(r.key.clone()).cursor(Cursor::Pointer);
        if r.selected {
            el.fill(tokens::ACCENT)
        } else {
            el
        }
    }
}

/// The panel: `title` over the rows, in a list that scrolls.
pub fn panel(title: &str, rows: Vec<Row>) -> El {
    let rows = Arc::new(rows);
    let keys = rows.clone();
    column([
        row([
            text(title.to_string())
                .bold()
                .ellipsis()
                .width(Size::Fill(1.0)),
            button("+ Ship").key(ADD_SHIP_KEY),
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center)
        .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2)),
        virtual_list_dyn(
            rows.len(),
            ROW_PX,
            move |i| keys[i].key.clone(),
            move |i| drawn(&rows[i]),
        )
        .key(LIST_KEY),
    ])
    .width(Size::Fixed(WIDTH))
    .height(Size::Fill(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_keys_parse() {
        assert_eq!(parse_item_key(&item_key(3, 14)), Some((3, 14)));
        assert_eq!(parse_item_key("ship:3"), None);
        assert_eq!(parse_ship_key(&ship_key(3)), Some(3));
        assert_eq!(parse_item_key("item:3"), None);
    }

    #[test]
    fn mass_reads() {
        assert_eq!(mass(85.66), "85.7 t");
        assert_eq!(mass(1.234), "1.23 t");
        assert_eq!(mass(0.308), "308.0 kg");
        assert_eq!(mass(0.0), "0 g");
    }

    /// Transfers and purchases as lines: in full each way, and each that
    /// fell short with why; a vessel by its name.
    #[test]
    fn transfers_and_purchases_read() {
        use orbit::time::Epoch;
        let t = Epoch::from_ymd(2045, 3, 11);
        let x = |kind, with, commodity, asked, got, short, cost_musd| Transfer {
            t,
            step: 0,
            with,
            kind,
            commodity,
            asked,
            got,
            short,
            cost_musd,
        };
        let depot = With::Vessel {
            id: 7,
            leg: Some(0),
        };
        let ships = [Ship {
            id: 7,
            name: "Depot".into(),
            items: Vec::new(),
            done: true,
            failure: None,
            first_leg: 0,
            revision: 0,
            warnings: Vec::new(),
            tanks: None,
        }];
        let made = [
            x(Kind::Give, depot, Commodity::Lox, 60.0, 60.0, None, None),
            x(Kind::Give, depot, Commodity::Lh2, 8.6, 8.6, None, None),
            x(
                Kind::Take,
                depot,
                Commodity::Lh2,
                0.308,
                0.0,
                Some(sim::state::Short::Stock),
                None,
            ),
            x(
                Kind::Take,
                depot,
                Commodity::Lox,
                0.0231,
                0.0231,
                None,
                None,
            ),
        ];
        let lines = transferred(&ships, &made.iter().collect::<Vec<_>>());
        assert_eq!(
            lines,
            vec![
                (
                    "gave LOX 60.0 t, LH2 8.60 t to Depot".to_string(),
                    Tone::Muted
                ),
                ("took LOX 23.1 kg from Depot".to_string(), Tone::Muted),
                (
                    "take of 308.0 kg of LH2 fell short: 0 g (the vessel has less)".to_string(),
                    Tone::Warning
                ),
            ]
        );
        let earth = [
            x(
                Kind::Take,
                With::Earth,
                Commodity::Lox,
                85.7,
                85.7,
                None,
                Some(10.0),
            ),
            x(
                Kind::Take,
                With::Earth,
                Commodity::Lh2,
                12.2,
                12.2,
                None,
                Some(2.3),
            ),
            x(
                Kind::Give,
                With::NoOne,
                Commodity::Equipment,
                5.0,
                0.0,
                Some(sim::state::Short::NoOne),
                None,
            ),
        ];
        let lines = transferred(&[], &earth.iter().collect::<Vec<_>>());
        assert_eq!(
            lines,
            vec![
                (
                    "bought LOX 85.7 t, LH2 12.2 t from Earth for $12.3M".to_string(),
                    Tone::Muted
                ),
                (
                    "give of 5.00 t of equipment fell short: 0 g (no one to transfer with)"
                        .to_string(),
                    Tone::Warning
                ),
            ]
        );
        assert_eq!(whom(&[], depot), "ship 7");
        let purchases = [
            Purchase {
                t,
                what: Bought::Ship,
                tonnes: 0.0,
                musd: 900.0,
            },
            Purchase {
                t,
                what: Bought::Commodity(Commodity::Lox),
                tonnes: 85.7,
                musd: 200.0,
            },
            Purchase {
                t,
                what: Bought::Commodity(Commodity::Lh2),
                tonnes: 12.2,
                musd: 50.0,
            },
        ];
        assert_eq!(
            bought(&purchases),
            "Bought at LEO for $1,150M: the ship $900M, LOX 85.7 t $200M, LH2 12.2 t $50.0M"
        );
    }
}
