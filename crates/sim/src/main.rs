//! `sim`: the sim engine headless (M1b.md, step 1 plan, part 8).

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use orbit::mission::ItemKind;
use orbit::places::Places;
use orbit::time::Epoch;
use sim::game::{Game, world};
use sim::log::Log;
use sim::report;
use sim::ships::{self, Commodity};
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
    /// each ship's items, with what each burned of a ship with a class, and
    /// its tanks monthly; the state hashed on the first of each month; and
    /// a scenario's checks. A check that fails exits nonzero.
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
    let games =
        sim::run::run(&log, alone, &pool).map_err(|e| anyhow!("{}: {e}", file.display()))?;
    for game in &games {
        let ships = game.ships().iter().zip(&game.plan().ships);
        for ((ship, plan), state) in ships.zip(&game.state().ships) {
            let fuel = ships::read(plan, game.economy())
                .map_err(|e| anyhow!("{}: {e}", file.display()))?
                .map(|f| Commodity::fuel(game.economy().ships[f.class].propellant));
            let class = plan
                .class
                .as_ref()
                .map_or(String::new(), |c| format!("a {c}, "));
            println!(
                "\n{} ({class}from {}, {})",
                ship.name, plan.start.label, plan.epoch
            );
            for b in &ship.breaches {
                println!("  WARNING  {b}");
            }
            let items: Vec<_> = ship.items().cloned().collect();
            for (k, item) in items.iter().enumerate() {
                if let ItemKind::Docked(_) = item.kind {
                    print!("{}", report::docked(game.plan(), &items, k));
                    continue;
                }
                print!("{}", report::line(eph, item));
                if let Some(fuel) = fuel {
                    print!("{}", report::burned(state, fuel, item, k));
                }
            }
            if let Some(totals) = report::totals(&items) {
                println!("{totals}");
            }
            if let Some(tanks) = fuel.and_then(|f| report::tanks(state, f, plan.epoch, &items)) {
                print!("{tanks}");
            }
            if let Some(bought) = report::purchases(state) {
                print!("{bought}");
            }
            if let Some(transfers) = report::transfers(game.plan(), state) {
                print!("{transfers}");
            }
            // A failure no item says: a refused start, a panic.
            let said = items.iter().any(|i| matches!(i.kind, ItemKind::Failed(_)));
            if let Some(why) = ship.failure.as_ref().filter(|_| !said) {
                println!("  FAILED   {why}");
            }
            for (i, why) in &ship.refused {
                println!("  re-plan {} of this ship not flown: {why}", i + 1);
            }
            if let Some(dir) = &tracks {
                let out = dir.join(format!("ship-{}.csv", ship.id));
                std::fs::write(&out, report::csv(eph, &items))
                    .with_context(|| out.display().to_string())?;
            }
        }
    }
    // The programs are every game's: by career's now, or the loop's end.
    if let Some(game) = games.first() {
        let by = Epoch(game.now().0.max(game.state().until.0));
        if let Some(lines) = report::programs(game.economy(), game.state(), by) {
            print!("{lines}");
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
    report::months(start, Epoch(end))
        .into_iter()
        .map(|t| (t, game.day_hashes(t, 1)[0]))
        .collect()
}
