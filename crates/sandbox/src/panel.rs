//! The plan's panel (M1c.md, step 5 plan, item 2): beside the map, each
//! ship and its items, with their times and Δv, the failures and why.

use crate::labels::{grouped, mps};
use crate::plan::Ship;
use damascene_core::prelude::*;
use orbit::ephem::Ephemeris;
use orbit::mission::{Item, ItemKind, MarkerKind, when};
use orbit::time::Epoch;
use std::sync::Arc;

/// The panel's list's key.
pub const LIST_KEY: &str = "plan";

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
pub fn rows(eph: &Ephemeris, ships: &[Ship], t: Epoch, selected: Option<(usize, usize)>) -> Vec<Row> {
    let mut rows = Vec::new();
    for (s, ship) in ships.iter().enumerate() {
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
            && !ship.items.iter().any(|i| matches!(i.kind, ItemKind::Failed(_)))
        {
            lines.push((why.clone(), Tone::Failure));
        }
        let dv: f64 = ship.items.iter().map(|i| i.dv).sum();
        rows.push(Row {
            key: format!("ship:{s}"),
            kind: String::new(),
            label: ship.name.clone(),
            dv: if ship.items.is_empty() { String::new() } else { mps(dv) },
            lines,
            current: false,
            selected: false,
        });
        let now = current(ship, t);
        for (k, item) in ship.items.iter().enumerate() {
            let mut row = item_row(eph, item);
            row.key = item_key(s, k);
            row.current = now == Some(k);
            row.selected = selected == Some((s, k));
            rows.push(row);
        }
    }
    rows
}

/// An item's row, unkeyed.
fn item_row(eph: &Ephemeris, item: &Item) -> Row {
    let kind = match &item.kind {
        ItemKind::Leg => "leg",
        ItemKind::Stay => "stay",
        ItemKind::Capture => "capture",
        ItemKind::Landing => "landing",
        ItemKind::Ascent => "ascent",
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
        dv: if item.dv >= NO_DV { mps(item.dv) } else { String::new() },
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
        head.push(text(r.kind.clone()).caption().muted().width(Size::Fixed(52.0)));
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
    } else {
        let el = el.key(r.key.clone()).cursor(Cursor::Pointer);
        if r.selected { el.fill(tokens::ACCENT) } else { el }
    }
}

/// The panel: `title` over the rows, in a list that scrolls.
pub fn panel(title: &str, rows: Vec<Row>) -> El {
    let rows = Arc::new(rows);
    let keys = rows.clone();
    column([
        text(title.to_string())
            .bold()
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
        assert_eq!(parse_item_key("item:3"), None);
    }
}
