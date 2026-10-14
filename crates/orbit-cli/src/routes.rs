//! `orbit routes`: the route graph (M1a.md, step 11) from the gate sweep's
//! cold plans. Legs within a system take the median of their converged
//! designs; stations' and low lunar orbit's gateways are fitted to the
//! sweep's escapes and captures; the rest comes from patched conics and
//! `surface`'s rules.

use anyhow::{Context, Result, bail};
use orbit::ephem::{BodyId, BodyKind, Ephemeris};
use orbit::places::Places;
use orbit::routes::{Escape, Gateway, Leg, Pass, Place, PlaceId, RouteGraph};
use orbit::surface;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The stations, by their ids in `data/places.toml` and the sweep's leg
/// names.
const STATIONS: [&str; 4] = ["l1", "l2", "nrho", "dro"];

/// The altitude of the escapes' periapsis in the sweep, and Phobos's
/// orbit, km.
const ESCAPE_ALTITUDE: f64 = 200.0;
const PHOBOS_RADIUS: f64 = 9_376.0;

/// Landing and ascent take hours, days.
const SURFACE_DAYS: f64 = 0.1;

/// Converged designs of each leg: (Δv m/s, duration days).
type Designs = HashMap<String, Vec<(f64, f64)>>;

/// Rounded to 0.1 m/s (or 1e-4 of other units), for a readable file.
fn r4(x: f64) -> f64 {
    (x * 1e4).round() / 1e4
}

/// The sweeps' designs, and the first and last departure dates seen.
fn read(sweeps: &[PathBuf]) -> Result<(Designs, String, String)> {
    let mut out: Designs = HashMap::new();
    let (mut first, mut last) = (String::from("9999"), String::new());
    for dir in sweeps {
        let path = dir.join("cold.csv");
        let text = std::fs::read_to_string(&path).with_context(|| path.display().to_string())?;
        let mut lines = text.lines();
        let header: Vec<&str> = lines.next().context("an empty CSV")?.split(',').collect();
        let col = |name: &str| {
            header
                .iter()
                .position(|h| *h == name)
                .with_context(|| format!("{}: no column {name}", path.display()))
        };
        let (leg, date, conv, dv, days) = (
            col("leg")?,
            col("date")?,
            col("converged")?,
            col("dv_m_s")?,
            col("duration_days")?,
        );
        for line in lines {
            let f: Vec<&str> = line.split(',').collect();
            first = first.min(f[date].to_string());
            last = last.max(f[date].to_string());
            if f[conv] == "yes" {
                out.entry(f[leg].to_string())
                    .or_default()
                    .push((f[dv].parse()?, f[days].parse()?));
            }
        }
    }
    Ok((out, first, last))
}

fn quantile(mut v: Vec<f64>, q: f64) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    let x = q * (v.len() - 1) as f64;
    let (i, f) = (x.floor() as usize, x.fract());
    v[i] + f * (v.get(i + 1).unwrap_or(&v[i]) - v[i])
}

pub fn run(sweeps: &[PathBuf], out: &Path) -> Result<()> {
    let eph = Ephemeris::standard();
    let (designs, first, last) = read(sweeps)?;
    let body = |id: &str| eph.lookup(id).with_context(|| format!("no body {id}"));
    let (earth, moon, mars) = (body("earth")?, body("moon")?, body("mars")?);
    let source = format!(
        "medians of `orbit gate` cold plans departing {first} to {last} (sweeps {})",
        sweeps
            .iter()
            .map(|p| p.file_name().unwrap_or_default().to_string_lossy())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut g = RouteGraph::new(source);
    let places = Places::standard();
    let named = |id: &str| places.get(id).with_context(|| format!("no place {id}"));
    let place = |g: &mut RouteGraph, id: &str, system: BodyId, gw: Option<Gateway>| {
        Ok::<_, anyhow::Error>(g.add_place(Place {
            id: id.into(),
            name: named(id)?.name.clone(),
            system,
            gateway: gw,
        }))
    };
    let radius = |id: &str| {
        places
            .low(id)
            .map(|l| l.radius)
            .with_context(|| format!("{id} is not a low orbit"))
    };

    // A low orbit's gateway: a burn from it, and an aerocapture whose
    // apoapsis reaches it.
    let low = |b: BodyId, radius: f64| {
        let gm = eph.body(b).gm;
        let burn = Escape::Periapsis {
            extra: 0.0,
            radius,
            speed: r4((gm / radius).sqrt()),
        };
        let pass = surface::rules(eph, b).atmosphere.map(|air| {
            let r = eph.body(b).radius + air.capture_altitude;
            Pass {
                radius: r,
                max_speed: air.max_speed,
                after: r4(surface::after_aerocapture(gm, r, radius)),
            }
        });
        Gateway {
            depart: Some(burn),
            arrive: Some(burn),
            pass,
            days: 0.0,
        }
    };
    let leo = place(&mut g, "leo", earth, Some(low(earth, radius("leo")?)))?;

    // Stations and low lunar orbit: a perigee burn 200 km above Earth,
    // after `extra` to get there (fitted per design as Δv less the burn).
    // Under step 9's stand-in (`End::Hyperbola`) the way down does not
    // depend on v∞, so escapes at 1 and 3 km/s give the same extra.
    let gm_e = eph.body(earth).gm;
    let rp = eph.body(earth).radius + ESCAPE_ALTITUDE;
    let parabolic = (2.0 * gm_e / rp).sqrt();
    let burn = |v: f64| (v * v + 2.0 * gm_e / rp).sqrt() - parabolic;
    let fit = |name: &str, outward: bool| -> Result<(f64, f64)> {
        let mut extra = Vec::new();
        let mut days = Vec::new();
        for v in [1.0, 3.0] {
            let leg = if outward {
                format!("{name}-esc{v}")
            } else {
                format!("esc{v}-{name}")
            };
            let d = designs
                .get(&leg)
                .with_context(|| format!("no designs of {leg}"))?;
            let mine: Vec<f64> = d.iter().map(|&(dv, _)| dv / 1e3 - burn(v)).collect();
            println!(
                "{leg:>10}: {} designs, extra {:.0} m/s (10–90%: {:.0}–{:.0})",
                d.len(),
                quantile(mine.clone(), 0.5) * 1e3,
                quantile(mine.clone(), 0.1) * 1e3,
                quantile(mine.clone(), 0.9) * 1e3,
            );
            extra.extend(mine);
            days.extend(d.iter().map(|&(_, t)| t));
        }
        Ok((quantile(extra, 0.5), r4(quantile(days, 0.5))))
    };
    let fitted = |name: &str| -> Result<Gateway> {
        let (out, days) = fit(name, true)?;
        let (back, _) = fit(name, false)?;
        let at = |extra: f64| Escape::Periapsis {
            extra: r4(extra),
            radius: rp,
            speed: r4(parabolic),
        };
        Ok(Gateway {
            depart: Some(at(out)),
            arrive: Some(at(back)),
            pass: None,
            days,
        })
    };
    let mut station = HashMap::new();
    for id in STATIONS {
        let gw = fitted(id)?;
        station.insert(id, place(&mut g, id, earth, Some(gw))?);
    }
    let llo_gw = fitted("llo")?;
    let llo = place(&mut g, "llo", earth, Some(llo_gw))?;
    let pole = place(&mut g, "moon-south-pole", earth, None)?;
    let mare = place(&mut g, "moon-mare", earth, None)?;

    // Mars: low orbit; the surface, reached from another system by entry;
    // Phobos, left by dropping to a periapsis at low orbit's radius.
    let (gm_m, r_m) = (eph.body(mars).gm, eph.body(mars).radius);
    let lmo_r = radius("lmo")?;
    let lmo = place(&mut g, "lmo", mars, Some(low(mars, lmo_r)))?;
    let air = surface::rules(eph, mars)
        .atmosphere
        .context("Mars's atmosphere")?;
    let entry = Gateway {
        depart: None,
        arrive: None,
        pass: Some(Pass {
            radius: r_m + air.entry_altitude,
            max_speed: air.max_speed,
            after: air.terminal_dv,
        }),
        days: 0.0,
    };
    let ground = place(&mut g, "mars-surface", mars, Some(entry))?;
    let a = (PHOBOS_RADIUS + lmo_r) / 2.0;
    let vis = |r: f64| (gm_m * (2.0 / r - 1.0 / a)).sqrt();
    let drop = Escape::Periapsis {
        extra: r4((gm_m / PHOBOS_RADIUS).sqrt() - vis(PHOBOS_RADIUS)),
        radius: lmo_r,
        speed: r4(vis(lmo_r)),
    };
    let phobos_gw = Gateway {
        depart: Some(drop),
        arrive: Some(drop),
        pass: None,
        days: r4(std::f64::consts::PI * (a.powi(3) / gm_m).sqrt() / 86_400.0),
    };
    let phobos = place(&mut g, "phobos", mars, Some(phobos_gw))?;

    // The small bodies, each its own system.
    let meet = Gateway {
        depart: Some(Escape::Rendezvous),
        arrive: Some(Escape::Rendezvous),
        pass: None,
        days: 0.0,
    };
    for b in eph.ids() {
        if matches!(eph.body(b).kind, BodyKind::SmallBody { .. }) {
            place(&mut g, &eph.body(b).id, b, Some(meet))?;
        }
    }

    // Legs from the sweep, by its names.
    let mut swept: Vec<(String, PlaceId, PlaceId, bool)> = vec![
        ("leo400-l1".into(), leo, station["l1"], false),
        ("l1-leo400".into(), station["l1"], leo, false),
        ("leo400-llo".into(), leo, llo, false),
        ("llo-leo400".into(), llo, leo, false),
        ("lmo-phobos".into(), lmo, phobos, false),
        ("phobos-lmo".into(), phobos, lmo, false),
    ];
    for s in STATIONS {
        let at = station[s];
        if s != "l1" {
            swept.push((format!("leo-{s}"), leo, at, false));
            swept.push((format!("{s}-leo"), at, leo, false));
        }
        swept.push((format!("{s}-llo"), at, llo, false));
        swept.push((format!("llo-{s}"), llo, at, false));
        swept.push((format!("{s}-leo-aero"), at, leo, true));
        for o in STATIONS {
            if o != s {
                swept.push((format!("{s}-{o}"), at, station[o], false));
            }
        }
    }
    swept.push(("llo-leo-aero".into(), llo, leo, true));
    for (name, from, to, aero) in swept {
        let Some(d) = designs.get(&name) else {
            bail!("no converged designs of {name}");
        };
        let dv: Vec<f64> = d.iter().map(|&(dv, _)| dv).collect();
        g.add_leg(Leg {
            from,
            to,
            dv: r4(quantile(dv.clone(), 0.5) / 1e3),
            days: r4(quantile(d.iter().map(|&(_, t)| t).collect(), 0.5)),
            aero,
            source: format!(
                "sweep ({name}): median of {} designs, 10–90% {:.0}–{:.0} m/s",
                d.len(),
                quantile(dv.clone(), 0.1),
                quantile(dv, 0.9)
            ),
        });
    }

    // Landing and ascent.
    let llo_r = radius("llo")?;
    for site in [pole, mare] {
        for (from, to, dv) in [
            (llo, site, surface::descent(eph, moon, llo_r)),
            (site, llo, surface::ascent(eph, moon, llo_r)),
        ] {
            g.add_leg(Leg {
                from,
                to,
                dv: r4(dv),
                days: SURFACE_DAYS,
                aero: false,
                source: "surface rules (Apollo 11)".into(),
            });
        }
    }
    let rule = "surface rules (Palaszewski)";
    for (from, to, dv, aero) in [
        (lmo, ground, surface::descent(eph, mars, lmo_r), false),
        (
            lmo,
            ground,
            surface::entry_from_orbit(eph, mars, lmo_r).context("Mars's atmosphere")?,
            true,
        ),
        (ground, lmo, surface::ascent(eph, mars, lmo_r), false),
    ] {
        g.add_leg(Leg {
            from,
            to,
            dv: r4(dv),
            days: SURFACE_DAYS,
            aero,
            source: rule.into(),
        });
    }

    let text = format!(
        "# The route graph (M1a.md, step 11), generated by `orbit routes`: do\n\
         # not edit. Δv in km/s, durations in days, radii in km, speeds in\n\
         # km/s. A gateway prices departures to and arrivals from other\n\
         # systems from the excess speed v∞ of a patched-conic arc (see\n\
         # orbit::routes).\n\n{}",
        g.to_toml(eph)
    );
    std::fs::write(out, text).with_context(|| out.display().to_string())?;
    println!(
        "{} places, {} legs: {}",
        g.places().count(),
        g.legs().len(),
        out.display()
    );
    Ok(())
}
