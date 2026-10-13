//! `orbit`: inspect the game's orbital mechanics from the command line.

mod porkchop_png;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use orbit::consts::AU_KM;
use orbit::ephem::{BodyId, BodyKind, Ephemeris};
use orbit::time::Epoch;
use orbit::transfer::{Grid, Porkchop, default_tof_range};
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
    /// List the bodies: planets and candidate asteroids.
    Bodies,
    /// Transfer windows between two bodies.
    Windows {
        from: String,
        to: String,
        #[command(flatten)]
        search: Search,
    },
    /// Windows from one body to every other body, by date.
    Calendar {
        #[arg(long, default_value = "earth")]
        from: String,
        #[command(flatten)]
        search: Search,
    },
    /// Porkchop grid between two bodies: best transfer, optional PNG.
    Porkchop {
        from: String,
        to: String,
        #[command(flatten)]
        search: Search,
        /// Range of total excess speed shaded above the best transfer, km/s.
        #[arg(long, default_value_t = 4.0)]
        span: f64,
        /// Write the grid as a PNG heatmap (darker = cheaper).
        #[arg(long)]
        png: Option<PathBuf>,
    },
}

#[derive(Args)]
struct Search {
    #[arg(long, default_value = "2040-01-01")]
    start: Epoch,
    #[arg(long, default_value = "2076-01-01")]
    end: Epoch,
    /// A window spans departures within this many km/s of its best total
    /// excess speed.
    #[arg(long, default_value_t = 0.5)]
    margin: f64,
    /// Omit windows whose best transfer needs more total excess speed than
    /// this, km/s.
    #[arg(long)]
    max_v_inf: Option<f64>,
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
}

impl World {
    fn body(&self, id: &str) -> Result<BodyId> {
        self.eph
            .lookup(id)
            .ok_or_else(|| anyhow!("unknown body {id:?} (see `orbit bodies`)"))
    }

    /// The two bodies of a transfer, which must differ.
    fn pair(&self, from: &str, to: &str) -> Result<(BodyId, BodyId)> {
        let (a, b) = (self.body(from)?, self.body(to)?);
        if a == b {
            bail!("a transfer needs two different bodies");
        }
        Ok((a, b))
    }

    fn porkchop(&self, from: BodyId, to: BodyId, s: &Search) -> Porkchop {
        let (lo, hi) = default_tof_range(&self.eph, from, to, s.start);
        let grid = Grid::covering(
            s.start,
            s.end,
            s.depart_step,
            s.tof_min.unwrap_or(lo),
            s.tof_max.unwrap_or(hi),
            s.tof_step,
        );
        Porkchop::compute(&self.eph, from, to, grid)
    }

    fn windows(&self, from: BodyId, to: BodyId, s: &Search) -> Vec<Window> {
        let pc = self.porkchop(from, to, s);
        let synodic = self.eph.synodic_period_days(from, to, s.start);
        let mut windows = find_windows(&pc, synodic, s.margin);
        windows.retain(|w| s.max_v_inf.is_none_or(|max| w.best.v_inf_total() <= max));
        windows
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let world = World {
        eph: Ephemeris::standard(),
    };
    match cli.cmd {
        Cmd::Ephem { body, date } => ephem(&world, &body, date),
        Cmd::Bodies => {
            bodies(&world);
            Ok(())
        }
        Cmd::Windows { from, to, search } => {
            let (from, to) = world.pair(&from, &to)?;
            let windows: Vec<_> = world
                .windows(from, to, &search)
                .into_iter()
                .map(|w| (to, w))
                .collect();
            print_windows(&world, &windows);
            Ok(())
        }
        Cmd::Calendar { from, search } => calendar(&world, &from, &search),
        Cmd::Porkchop {
            from,
            to,
            search,
            span,
            png,
        } => {
            let (from, to) = world.pair(&from, &to)?;
            porkchop(&world, from, to, &search, png, span)
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

fn bodies(w: &World) {
    println!("{:<12} {:<22} kind", "id", "name");
    for id in w.eph.ids() {
        let b = w.eph.body(id);
        let kind = match &b.kind {
            BodyKind::Planet => "planet".to_string(),
            BodyKind::SmallBody {
                spectral,
                diameter_km,
                ..
            } => format!(
                "small body, class {spectral}, {}",
                diameter_km.map_or("unknown size".to_string(), |d| format!("{d} km"))
            ),
        };
        println!("{:<12} {:<22} {kind}", b.id, b.name);
    }
}

fn calendar(w: &World, from: &str, s: &Search) -> Result<()> {
    let from = w.body(from)?;
    let mut all = Vec::new();
    for to in w.eph.ids().filter(|&to| to != from) {
        all.extend(w.windows(from, to, s).into_iter().map(|win| (to, win)));
    }
    all.sort_by(|a, b| a.1.open.0.total_cmp(&b.1.open.0));
    print_windows(w, &all);
    Ok(())
}

fn print_windows(w: &World, windows: &[(BodyId, Window)]) {
    if windows.is_empty() {
        println!("no windows in range");
        return;
    }
    println!("dates are heliocentric departures; v∞ is excess speed at each end, km/s");
    println!(
        "{:<12} {:<10} {:<10}  {:<10} {:>5} {:>4}  {:>6} {:>6} {:>6}  {:>6}",
        "to", "opens", "closes", "best", "days", "type", "v∞ dep", "v∞ arr", "total", "C3"
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
            "{:<12} {:<10} {:<10}  {:<10} {:>5.0} {:>4}  {:>6.2} {:>6.2} {:>6.2}  {:>6.2}  {notes}",
            w.eph.body(*to).id,
            win.open.to_string(),
            win.close.to_string(),
            b.depart.to_string(),
            b.tof_days,
            if b.is_type_ii() { "II" } else { "I" },
            b.v_inf_depart,
            b.v_inf_arrive,
            b.v_inf_total(),
            b.c3(),
        );
    }
}

fn porkchop(
    w: &World,
    from: BodyId,
    to: BodyId,
    s: &Search,
    png: Option<PathBuf>,
    span: f64,
) -> Result<()> {
    let pc = w.porkchop(from, to, s);
    let g = pc.grid;
    let best = pc
        .best_by_departure()
        .into_iter()
        .flatten()
        .min_by(|a, b| a.v_inf_total().total_cmp(&b.v_inf_total()))
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
        "best: depart {} arrive {} ({:.0} d, Type {}), v∞ {:.2} + {:.2} = {:.2} km/s, C3 {:.2} km²/s²",
        best.depart,
        best.arrive(),
        best.tof_days,
        if best.is_type_ii() { "II" } else { "I" },
        best.v_inf_depart,
        best.v_inf_arrive,
        best.v_inf_total(),
        best.c3(),
    );
    if let Some(path) = png {
        if span <= 0.0 {
            bail!("--span must be positive");
        }
        let (width, height) = porkchop_png::write(&pc, best.v_inf_total(), span, &path)?;
        println!(
            "wrote {} ({width}×{height}): x = departure (left→right), y = time of flight (bottom→top), \
             darker = cheaper, white = more than {span} km/s above best",
            path.display()
        );
    }
    Ok(())
}
