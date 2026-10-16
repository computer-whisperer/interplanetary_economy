//! `sim`: the sim engine headless (M1b.md, step 1 plan, part 8).

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use orbit::places::Places;
use orbit::time::Epoch;
use sim::game::{Game, world};
use sim::log::Log;
use sim::report;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Parser)]
#[command(about = "The sim engine, headless (M1b.md)")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Design a game's log (a mission plan, or a career game) to its end:
    /// each ship's items, the state hashed on the first of each month, and
    /// a scenario's checks; a check that fails exits nonzero.
    Run {
        log: PathBuf,
        /// Design each ship alone, one after another, so that each item's
        /// time is its own (no hashes: they are the whole game's).
        #[arg(long)]
        alone: bool,
        /// Write each ship's sampled path as CSV here.
        #[arg(long)]
        tracks: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Run { log, alone, tracks } => run(&log, alone, tracks),
    }
}

fn run(file: &PathBuf, alone: bool, tracks: Option<PathBuf>) -> Result<()> {
    let eph = world().eph;
    let src = std::fs::read_to_string(file).with_context(|| file.display().to_string())?;
    let log = Log::parse(&src, eph, Places::standard())
        .map_err(|e| anyhow!("{}: {e}", file.display()))?;
    if let Some(dir) = &tracks {
        std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    }
    println!("{}", log.plan.name);
    let pool = Arc::new(rayon::ThreadPoolBuilder::new().build()?);
    let games = sim::run::run(&log, alone, &pool);
    for game in &games {
        for (ship, plan) in game.ships().iter().zip(&game.plan().ships) {
            println!(
                "\n{} (from {}, {})",
                ship.name, plan.start.label, plan.epoch
            );
            let items: Vec<_> = ship.items().cloned().collect();
            for item in &items {
                print!("{}", report::line(eph, item));
            }
            if let Some(totals) = report::totals(&items) {
                println!("{totals}");
            }
            if let Some(why) = ship.failure.as_ref().filter(|_| items.is_empty()) {
                println!("  FAILED   {why}");
            }
            if let Some(dir) = &tracks {
                let out = dir.join(format!("ship-{}.csv", ship.id));
                std::fs::write(&out, report::csv(eph, &items))
                    .with_context(|| out.display().to_string())?;
            }
        }
    }
    if let [game] = games.as_slice() {
        println!("\nthe state, hashed, on the first of each month:");
        for (t, h) in monthly(game) {
            println!("  {}  {h:016x}", t.stamp());
        }
    }
    let checks = sim::run::checks(&log, &games);
    if !checks.is_empty() {
        println!("\nchecks:");
    }
    for c in &checks {
        match c {
            Ok(held) => println!("  held    {held}"),
            Err(failed) => println!("  FAILED  {failed}"),
        }
    }
    let failed = checks.iter().filter(|c| c.is_err()).count();
    match failed {
        0 => Ok(()),
        n => Err(anyhow!("{n} of {} checks failed", checks.len())),
    }
}

/// The first of each month from the first ship's start to the last item's
/// end, and the state then.
fn monthly(game: &Game) -> Vec<(Epoch, u64)> {
    let ships = &game.plan().ships;
    let Some(start) = ships
        .iter()
        .map(|s| s.epoch)
        .min_by(|a, b| a.0.total_cmp(&b.0))
    else {
        return Vec::new();
    };
    let end = game
        .ships()
        .iter()
        .flat_map(|s| s.items().last())
        .map(|i| i.end.0)
        .fold(start.0, f64::max);
    let (mut y, mut m, _) = start.ymd();
    let mut out = Vec::new();
    loop {
        let t = Epoch::from_ymd(y, m, 1);
        if t.0 > end {
            return out;
        }
        if t.0 >= start.0 {
            out.push((t, game.day_hashes(t, 1)[0]));
        }
        (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    }
}
