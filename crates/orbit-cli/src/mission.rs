//! `orbit mission FILE`: designs each ship of a mission plan (M1c.md, step
//! 1), printing each item as it is designed, and optionally writing the
//! sampled paths as CSV.

use anyhow::{Context, Result, anyhow};
use orbit::ephem::Ephemeris;
use orbit::mission::{Item, ItemKind, MarkerKind, Plan, Planner, World, when};
use orbit::places::Places;
use orbit::time::SECONDS_PER_DAY;
use std::fmt::Write as _;
use std::path::Path;

pub fn run(file: &Path, tracks: Option<&Path>) -> Result<()> {
    let eph = Ephemeris::standard();
    let src = std::fs::read_to_string(file).with_context(|| file.display().to_string())?;
    let plan = Plan::parse(&src, eph, Places::standard())
        .map_err(|e| anyhow!("{}: {e}", file.display()))?;
    let world = World::standard();
    if let Some(dir) = tracks {
        std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    }
    println!("{}", plan.name);
    for (s, ship) in plan.ships.iter().enumerate() {
        println!(
            "\n{} (from {}, {})",
            ship.name, ship.start.label, ship.epoch
        );
        let mut planner = match Planner::new(&world, ship) {
            Ok(p) => p,
            Err(e) => {
                println!("  FAILED   {e}");
                continue;
            }
        };
        let mut items: Vec<Item> = Vec::new();
        while let Some(batch) = planner.step() {
            for item in batch {
                print!("{}", line(eph, &item));
                items.push(item);
            }
        }
        let dv: f64 = items.iter().map(|i| i.dv).sum();
        let jumps = items
            .iter()
            .flat_map(|i| &i.markers)
            .filter(|m| matches!(m.kind, MarkerKind::Jump { .. }))
            .count();
        let failed = items.iter().any(|i| matches!(i.kind, ItemKind::Failed(_)));
        if let (Some(first), Some(last)) = (items.first(), items.last()) {
            println!(
                "  total {:.1} m/s over {:.1} days{}{}",
                dv * 1e3,
                last.end.0 - first.start.0,
                if jumps > 0 {
                    format!(", {jumps} junction(s) not flown")
                } else {
                    String::new()
                },
                if failed { ", stopped by a failure" } else { "" }
            );
        }
        if let Some(dir) = tracks {
            let out = dir.join(format!("ship-{}.csv", s + 1));
            std::fs::write(&out, csv(eph, &items)).with_context(|| out.display().to_string())?;
        }
    }
    Ok(())
}

fn line(eph: &Ephemeris, item: &Item) -> String {
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

/// The items' samples: item, kind, center, Julian date (TDB), state km and
/// km/s relative to the center, and the Moon's (for the Earth–Moon frame).
fn csv(eph: &Ephemeris, items: &[Item]) -> String {
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
