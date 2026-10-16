//! Items as text and as CSV, for `sim run` (M1b.md, step 1 plan, part 8;
//! M1c.md's `orbit mission`, which it replaces).

use orbit::ephem::Ephemeris;
use orbit::mission::{Item, ItemKind, MarkerKind, when};
use orbit::time::SECONDS_PER_DAY;
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

/// The items' samples: item, kind, center, Julian date (TDB), state km and
/// km/s relative to the center, and the Moon's (for the Earth–Moon frame).
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
        for (t, st) in track.times.iter().zip(&track.states) {
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
