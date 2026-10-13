//! `orbit`: inspect the game's orbital mechanics from the command line.

mod porkchop_png;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use orbit::consts::AU_KM;
use orbit::ephem::{BodyKind, Ephemeris};
use orbit::network::{Gateway, Network, NodeId};
use orbit::time::Epoch;
use orbit::transfer::{Grid, Porkchop, Route};
use orbit::windows::{Window, find_windows};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "orbit",
    about = "Orbital mechanics of the interplanetary economy game"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Heliocentric state of a body (ecliptic J2000).
    Ephem {
        body: String,
        #[arg(long, default_value = "2040-01-01")]
        date: Epoch,
    },
    /// List the network's nodes.
    Nodes,
    /// List the fixed legs between nodes of one system.
    Legs,
    /// Cheapest chain of fixed legs between two nodes of one system.
    Path {
        from: String,
        to: String,
        /// The ship can brake in atmospheres.
        #[arg(long)]
        aero: bool,
    },
    /// Transfer windows for one interplanetary route.
    Windows {
        from: String,
        to: String,
        #[command(flatten)]
        search: Search,
    },
    /// Windows from one node to every node in other systems, by date.
    Calendar {
        #[arg(long, default_value = "leo")]
        from: String,
        #[command(flatten)]
        search: Search,
    },
    /// Porkchop grid for one route: best transfer, optional PNG.
    Porkchop {
        from: String,
        to: String,
        #[command(flatten)]
        search: Search,
        /// Write the grid as a PNG heatmap (darker = cheaper).
        #[arg(long)]
        png: Option<PathBuf>,
        /// Δv range shaded above the best transfer, km/s.
        #[arg(long, default_value_t = 4.0)]
        span: f64,
    },
}

#[derive(Args)]
struct Search {
    #[arg(long, default_value = "2040-01-01")]
    start: Epoch,
    #[arg(long, default_value = "2076-01-01")]
    end: Epoch,
    /// The ship can brake in atmospheres (aerocapture, direct entry).
    #[arg(long)]
    aero: bool,
    /// A window spans departures within this many km/s of its best.
    #[arg(long, default_value_t = 0.5)]
    margin: f64,
    /// Omit windows whose best transfer costs more than this, km/s.
    #[arg(long)]
    max_dv: Option<f64>,
    /// Shortest time of flight, days (default: 0.3× Hohmann).
    #[arg(long)]
    tof_min: Option<f64>,
    /// Longest time of flight, days (default: 3× Hohmann).
    #[arg(long)]
    tof_max: Option<f64>,
    #[arg(long, default_value_t = 1.0)]
    depart_step: f64,
    #[arg(long, default_value_t = 2.0)]
    tof_step: f64,
}

struct World {
    eph: Ephemeris,
    net: Network,
}

impl World {
    fn node(&self, id: &str) -> Result<NodeId> {
        self.net
            .lookup(id)
            .ok_or_else(|| anyhow!("unknown node {id:?} (see `orbit nodes`)"))
    }

    fn porkchop(&self, route: Route, s: &Search) -> Result<Porkchop> {
        let (lo, hi) = route.default_tof_range(&self.eph, &self.net, s.start);
        let grid = Grid::covering(
            s.start,
            s.end,
            s.depart_step,
            s.tof_min.unwrap_or(lo),
            s.tof_max.unwrap_or(hi),
            s.tof_step,
        );
        Porkchop::compute(&self.eph, &self.net, route, grid).map_err(|e| anyhow!(e))
    }

    fn windows(&self, route: Route, s: &Search) -> Result<Vec<Window>> {
        let pc = self.porkchop(route, s)?;
        let (a, b) = (
            self.net.node(route.from).system,
            self.net.node(route.to).system,
        );
        let mut windows = find_windows(&pc, self.eph.synodic_period_days(a, b, s.start), s.margin);
        windows.retain(|w| s.max_dv.is_none_or(|max| w.best.dv() <= max));
        Ok(windows)
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let eph = Ephemeris::standard();
    let net = Network::standard(&eph);
    let world = World { eph, net };
    match cli.cmd {
        Cmd::Ephem { body, date } => ephem(&world, &body, date),
        Cmd::Nodes => {
            nodes(&world);
            Ok(())
        }
        Cmd::Legs => {
            legs(&world);
            Ok(())
        }
        Cmd::Path { from, to, aero } => path(&world, &from, &to, aero),
        Cmd::Windows { from, to, search } => {
            let route = Route {
                from: world.node(&from)?,
                to: world.node(&to)?,
                aero_capable: search.aero,
            };
            print_windows(
                &world,
                &world
                    .windows(route, &search)?
                    .iter()
                    .map(|w| (route.to, *w))
                    .collect::<Vec<_>>(),
            );
            Ok(())
        }
        Cmd::Calendar { from, search } => calendar(&world, &from, &search),
        Cmd::Porkchop {
            from,
            to,
            search,
            png,
            span,
        } => {
            let route = Route {
                from: world.node(&from)?,
                to: world.node(&to)?,
                aero_capable: search.aero,
            };
            porkchop(&world, route, &search, png, span)
        }
    }
}

fn ephem(w: &World, body: &str, date: Epoch) -> Result<()> {
    let id = w
        .eph
        .lookup(body)
        .ok_or_else(|| anyhow!("unknown body {body:?}"))?;
    let b = w.eph.body(id);
    let s = b.state(date);
    let el = b.elements(date);
    println!("{} on {date}", b.name);
    if let BodyKind::SmallBody {
        spectral,
        diameter_km,
        ..
    } = &b.kind
    {
        let size = diameter_km.map_or("unknown size".to_string(), |d| format!("{d} km"));
        println!("  spectral class {spectral}, {size}");
    }
    let r = s.r / AU_KM;
    println!(
        "  r = ({:+.6}, {:+.6}, {:+.6}) au   |r| = {:.6} au",
        r.x,
        r.y,
        r.z,
        r.length()
    );
    println!(
        "  v = ({:+.4}, {:+.4}, {:+.4}) km/s |v| = {:.4} km/s",
        s.v.x,
        s.v.y,
        s.v.z,
        s.v.length()
    );
    println!(
        "  a = {:.6} au  e = {:.5}  i = {:.4}°",
        el.a / AU_KM,
        el.e,
        el.i.to_degrees()
    );
    Ok(())
}

fn nodes(w: &World) {
    println!("{:<16} {:<10} {:<34} gateway", "id", "system", "name");
    for (_, n) in w.net.nodes() {
        let gateway = match n.gateway {
            None => "-".to_string(),
            Some(Gateway::Orbit(o)) => format!(
                "orbit r={:.0} km{}{}",
                o.radius,
                if o.periapsis < o.radius {
                    format!(", burns at r={:.0} km", o.periapsis)
                } else {
                    String::new()
                },
                if o.aerocapture { ", aerocapture" } else { "" }
            ),
            Some(Gateway::Rendezvous) => "rendezvous".to_string(),
            Some(Gateway::Entry { dv }) => format!("direct entry, {dv} km/s"),
        };
        println!(
            "{:<16} {:<10} {:<34} {gateway}",
            n.id,
            w.eph.body(n.system).id,
            n.name
        );
    }
}

fn legs(w: &World) {
    println!(
        "{:<16} {:<16} {:>7} {:>6}  notes",
        "from", "to", "Δv", "days"
    );
    for l in w.net.legs() {
        let notes = [(l.aero, "aerobraking"), (l.launch, "launch service")]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, s)| *s)
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "{:<16} {:<16} {:>7.2} {:>6.1}  {notes}",
            w.net.node(l.from).id,
            w.net.node(l.to).id,
            l.dv,
            l.days
        );
    }
}

fn path(w: &World, from: &str, to: &str, aero: bool) -> Result<()> {
    let p = w
        .net
        .cheapest_local_path(w.node(from)?, w.node(to)?, aero)
        .context(
            "no chain of fixed legs connects them (different systems, or aerobraking needed)",
        )?;
    let ids: Vec<_> = p.nodes.iter().map(|&n| w.net.node(n).id.as_str()).collect();
    println!("{}: {:.2} km/s, {:.1} days", ids.join(" → "), p.dv, p.days);
    Ok(())
}

fn calendar(w: &World, from: &str, s: &Search) -> Result<()> {
    let from = w.node(from)?;
    let mut all = Vec::new();
    let mut skipped = Vec::new();
    for (to, n) in w.net.nodes() {
        if n.system == w.net.node(from).system || n.gateway.is_none() {
            continue;
        }
        let route = Route {
            from,
            to,
            aero_capable: s.aero,
        };
        if let Err(e) = route.validate(&w.net) {
            skipped.push(e);
            continue;
        }
        all.extend(w.windows(route, s)?.into_iter().map(|win| (to, win)));
    }
    all.sort_by(|a, b| a.1.open.0.total_cmp(&b.1.open.0));
    print_windows(w, &all);
    for e in skipped {
        println!("skipped: {e}");
    }
    Ok(())
}

fn print_windows(w: &World, windows: &[(NodeId, Window)]) {
    if windows.is_empty() {
        println!("no windows in range");
        return;
    }
    println!(
        "{:<16} {:<10} {:<10}  {:<10} {:>5} {:>4}  {:>6} {:>6} {:>6}  {:>6}",
        "to", "opens", "closes", "best", "days", "type", "Δv dep", "Δv arr", "total", "C3"
    );
    for (to, win) in windows {
        let b = &win.best;
        let notes = [
            (win.truncated, "runs past departure range"),
            (win.tof_limited, "time of flight at search limit"),
        ]
        .iter()
        .filter(|(on, _)| *on)
        .map(|(_, s)| *s)
        .collect::<Vec<_>>()
        .join("; ");
        println!(
            "{:<16} {:<10} {:<10}  {:<10} {:>5.0} {:>4}  {:>6.2} {:>6.2} {:>6.2}  {:>6.2}  {notes}",
            w.net.node(*to).id,
            win.open.to_string(),
            win.close.to_string(),
            b.depart.to_string(),
            b.tof_days,
            if b.is_type_ii() { "II" } else { "I" },
            b.dv_depart,
            b.dv_arrive,
            b.dv(),
            b.c3(),
        );
    }
}

fn porkchop(w: &World, route: Route, s: &Search, png: Option<PathBuf>, span: f64) -> Result<()> {
    let pc = w.porkchop(route, s)?;
    let g = pc.grid;
    let best = pc
        .best_by_departure()
        .into_iter()
        .flatten()
        .min_by(|a, b| a.dv().total_cmp(&b.dv()))
        .context("no transfer solutions in the grid")?;
    println!(
        "grid: departures {} to {} every {} d; time of flight {:.0}–{:.0} d every {} d",
        g.depart(0),
        g.depart(g.departs - 1),
        g.depart_step,
        g.tof(0),
        g.tof(g.tofs - 1),
        g.tof_step
    );
    println!(
        "best: depart {} arrive {} ({:.0} d, Type {}), Δv {:.2} + {:.2} = {:.2} km/s, C3 {:.2} km²/s²",
        best.depart,
        best.arrive(),
        best.tof_days,
        if best.is_type_ii() { "II" } else { "I" },
        best.dv_depart,
        best.dv_arrive,
        best.dv(),
        best.c3()
    );
    if let Some(path) = png {
        if span <= 0.0 {
            bail!("--span must be positive");
        }
        let (width, height) = porkchop_png::write(&pc, best.dv(), span, &path)?;
        println!(
            "wrote {} ({width}×{height}): x = departure (left→right), y = time of flight (bottom→top), \
             darker = cheaper, white = more than {span} km/s above best",
            path.display()
        );
    }
    Ok(())
}
