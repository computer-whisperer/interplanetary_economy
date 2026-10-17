//! Items as text and as CSV, for `sim run` (M1b.md, step 1 plan, part 8;
//! M1c.md's `orbit mission`, which it replaces); a ship's propellant and
//! tanks (step 2 plan, part 5).

use crate::ships::{Commodity, Load};
use crate::state::ShipState;
use orbit::ephem::Ephemeris;
use orbit::mission::{Item, ItemKind, MarkerKind, when};
use orbit::time::{Epoch, SECONDS_PER_DAY};
use std::fmt::Write as _;

/// An item: what, when, how long and its Δv, and the time designing it
/// took; then its burns, options, a jump and its note, where it has them.
pub fn line(eph: &Ephemeris, item: &Item) -> String {
    let kind = match &item.kind {
        ItemKind::Leg => "leg",
        ItemKind::Stay => "stay",
        ItemKind::Capture => "capture",
        ItemKind::Landing => "landing",
        ItemKind::Ascent => "ascent",
        ItemKind::Failed(_) => "FAILED",
    };
    let mut s = format!(
        "  {kind:<8} {:<34} {} → {}  {:>7.2} d  {:>8.1} m/s",
        item.label,
        when(item.start),
        when(item.end),
        item.end.0 - item.start.0,
        item.dv * 1e3
    );
    if item.seconds >= 0.05 {
        write!(s, "  ({:.1} s)", item.seconds).unwrap();
    }
    s.push('\n');
    if let ItemKind::Failed(why) = &item.kind {
        writeln!(s, "           {why}").unwrap();
    }
    let burns: Vec<String> = item
        .markers
        .iter()
        .filter_map(|m| {
            let at = |what: &str| {
                m.pass.map_or(String::new(), |(b, alt)| {
                    format!(" ({what} {} at {alt:.0} km)", eph.body(b).id)
                })
            };
            match m.kind {
                MarkerKind::Burn => Some(format!("{:.1}", m.dv * 1e3)),
                MarkerKind::Flyby => Some(format!("{:.1}{}", m.dv * 1e3, at("flyby"))),
                MarkerKind::Pass => Some(format!("{:.1}{}", m.dv * 1e3, at("pass"))),
                _ => None,
            }
        })
        .collect();
    if item.kind == ItemKind::Leg && !burns.is_empty() {
        writeln!(s, "           burns, m/s: {}", burns.join(" + ")).unwrap();
    }
    if item.options.len() > 1 || item.kind != ItemKind::Leg && !item.options.is_empty() {
        let options: Vec<String> = item
            .options
            .iter()
            .map(|c| {
                format!(
                    "{} {:.1} d: {}",
                    c.depart,
                    c.days,
                    c.dv.map_or("failed".into(), |dv| format!("{:.1}", dv * 1e3))
                )
            })
            .collect();
        writeln!(s, "           options, m/s: {}", options.join("; ")).unwrap();
    }
    for m in &item.markers {
        if let MarkerKind::Jump { km, plane_deg } = m.kind {
            writeln!(
                s,
                "           JUMP at departure: the leg leaves {km:.0} km from the ship, \
                 in a plane {plane_deg:.1}° off its orbit (a plane change: {:.0} m/s), not flown",
                m.dv * 1e3
            )
            .unwrap();
        }
    }
    if let Some(note) = &item.note {
        writeln!(s, "           {note}").unwrap();
    }
    s
}

/// What item `k` of a ship whose state is `state` burned, and what it left;
/// where it ran dry, and that what follows is not flown.
pub fn burned(state: &ShipState, fuel: Commodity, item: &Item, k: usize) -> String {
    let mut s = String::new();
    let used = state.used.get(k).copied().unwrap_or(0.0);
    if used > 0.0 {
        let what = match item.kind {
            ItemKind::Stay => "upkeep",
            _ => "propellant",
        };
        write!(s, "           {what} {}", tonnes(used)).unwrap();
        if let Some(load) = state.after.get(k).copied().flatten() {
            write!(s, ", leaving {}", carried(&load, fuel)).unwrap();
        }
        s.push('\n');
    }
    match state.dry {
        Some(dry) if dry.item == k => {
            let what = match dry.marker {
                Some(_) => format!("the burn of {:.1} m/s", dry.dv * 1e3),
                None => "keeping station".into(),
            };
            writeln!(
                s,
                "           RAN DRY  {} {what}: {} short, not flown after",
                when(dry.t),
                short(dry.short)
            )
            .unwrap();
        }
        Some(dry) if k > dry.item => writeln!(s, "           not flown: the ship ran dry").unwrap(),
        _ => {}
    }
    s
}

/// What a ship carries, to the kilogram: its tanks' LOX and fuel, and its
/// hold, if it holds anything.
pub fn carried(load: &Load, fuel: Commodity) -> String {
    let mut s = format!(
        "{} {:.3} t, {} {:.3} t",
        Commodity::Lox.name(),
        load.lox,
        fuel.name(),
        load.fuel
    );
    if load.cargo() > 0.0 {
        write!(s, ", hold {:.3} t", load.cargo()).unwrap();
    }
    s
}

/// Tonnes to the kilogram; under half a kilogram, grams (a stay's upkeep).
fn tonnes(t: f64) -> String {
    match t {
        t if t >= 5e-4 => format!("{t:.3} t"),
        t => format!("{:.0} g", t * 1e6),
    }
}

/// The first of each month from `start` to `end`, both included.
pub fn months(start: Epoch, end: Epoch) -> Vec<Epoch> {
    let (mut y, mut m, _) = start.ymd();
    let mut out = Vec::new();
    loop {
        let t = Epoch::from_ymd(y, m, 1);
        if t.0 > end.0 {
            return out;
        }
        if t.0 >= start.0 {
            out.push(t);
        }
        (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    }
}

/// A shortfall, km/s: in m/s, to a tenth; under a tenth, in mm/s (a day's
/// upkeep).
pub fn short(dv: f64) -> String {
    match dv * 1e3 {
        m if m >= 0.1 => format!("{m:.1} m/s"),
        m => format!("{:.1} mm/s", m * 1e3),
    }
}

/// A ship's tanks and hold at its start, on the first of each month, and
/// at its last item's end; `None` for a ship without a class.
pub fn tanks(state: &ShipState, fuel: Commodity, start: Epoch, items: &[Item]) -> Option<String> {
    state.load(start)?;
    let end = items.last().map_or(start, |i| i.end);
    let mut times = vec![(start, Some("start"))];
    let inside = months(start, end)
        .into_iter()
        .filter(|t| start.0 < t.0 && t.0 < end.0);
    times.extend(inside.map(|t| (t, None)));
    if end.0 > start.0 {
        times.push((end, Some("end")));
    }
    let mut s = String::from("  tanks and hold, t:\n");
    for (t, what) in times {
        let load = state
            .load(t)
            .expect("a ship with a load at its start has one after");
        let notes: Vec<&str> = what
            .into_iter()
            .chain(state.dry_by(t).map(|_| "dry"))
            .collect();
        let note = match notes.is_empty() {
            true => String::new(),
            false => format!("  ({})", notes.join(", ")),
        };
        writeln!(s, "    {}  {}{note}", when(t), carried(&load, fuel)).unwrap();
    }
    Some(s)
}

/// The ship's totals: Δv, days, junctions not flown, and a failure.
pub fn totals(items: &[Item]) -> Option<String> {
    let (first, last) = (items.first()?, items.last()?);
    let dv: f64 = items.iter().map(|i| i.dv).sum();
    let jumps = items
        .iter()
        .flat_map(|i| &i.markers)
        .filter(|m| matches!(m.kind, MarkerKind::Jump { .. }))
        .count();
    let failed = items.iter().any(|i| matches!(i.kind, ItemKind::Failed(_)));
    Some(format!(
        "  total {:.1} m/s over {:.1} days{}{}",
        dv * 1e3,
        last.end.0 - first.start.0,
        if jumps > 0 {
            format!(", {jumps} junction(s) not flown")
        } else {
            String::new()
        },
        if failed { ", stopped by a failure" } else { "" }
    ))
}

/// The items' samples (a stay's made over it, `Track::samples`): item,
/// kind, center, Julian date (TDB), state km and km/s relative to the
/// center, and the Moon's (for the Earth–Moon frame).
pub fn csv(eph: &Ephemeris, items: &[Item]) -> String {
    let moon = eph.lookup("moon").expect("the Moon is in the ephemeris");
    let mut out = String::from("item,kind,center,jd,x,y,z,vx,vy,vz,mx,my,mz,mvx,mvy,mvz\n");
    for (i, item) in items.iter().enumerate() {
        let Some(track) = &item.track else { continue };
        let center = track
            .center
            .map_or("ssb".to_string(), |c| eph.body(c).id.clone());
        let kind = format!("{:?}", item.kind).to_lowercase();
        let kind = kind.split('(').next().unwrap_or_default();
        let Some((first, last)) = track.span() else {
            continue;
        };
        for (t, st) in track.samples(first, last) {
            let at = track.epoch.plus_days(t / SECONDS_PER_DAY);
            let m = eph.barycentric(moon, at);
            let c = track.center.map(|c| eph.barycentric(c, at));
            let (mr, mv) = c.map_or((m.r, m.v), |c| (m.r - c.r, m.v - c.v));
            write!(out, "{},{kind},{center},{:.9}", i + 1, at.jd()).unwrap();
            for x in st.iter().chain(&[mr.x, mr.y, mr.z, mv.x, mv.y, mv.z]) {
                write!(out, ",{x:.9}").unwrap();
            }
            out.push('\n');
        }
    }
    out
}
