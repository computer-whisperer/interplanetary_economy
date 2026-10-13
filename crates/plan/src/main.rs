//! `plan`: solve the M2 kill-test LP for one draw, or sweep many.

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use orbit::ephem::Ephemeris;
use orbit::network::Network;
use plan::classify::{Classification, SOURCES, Source, classify, source_of};
use plan::edges::{self, Edge};
use plan::model::{self, Commodity, Plan};
use plan::params::{Draw, EconomyFile, Scenario};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Parser)]
#[command(name = "plan", about = "M2 kill-test optimizer")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Solve one draw and print the plan.
    Solve {
        /// Random draw; omit for every parameter's central value.
        #[arg(long)]
        seed: Option<u64>,
        #[command(flatten)]
        opts: Opts,
    },
    /// Solve many random draws and tabulate their development paths.
    Sweep {
        /// Seeds 1..=draws.
        #[arg(long, default_value_t = 100)]
        draws: u64,
        /// Solves run at once.
        #[arg(long, default_value_t = 12)]
        jobs: usize,
        /// Write one row per draw.
        #[arg(long)]
        csv: Option<PathBuf>,
        #[command(flatten)]
        opts: Opts,
    },
}

#[derive(Args)]
struct Opts {
    /// Length of a time step.
    #[arg(long, default_value_t = 3)]
    months: u32,
    /// Interplanetary edges cost at most this, km/s.
    #[arg(long, default_value_t = 6.0)]
    max_dv: f64,
    /// The development path is read from propellant supply over these
    /// first years.
    #[arg(long, default_value_t = 15.0)]
    path_years: f64,
    /// Opening moves are installations in these first years.
    #[arg(long, default_value_t = 5.0)]
    opening_years: f64,
    /// Multiplies the equipment per t/yr (α) of every plant at one source,
    /// after drawing: e.g. `asteroids=3`. Repeatable.
    #[arg(long, value_parser = parse_factor)]
    alpha: Vec<(Source, f64)>,
    /// Multiplies the launch price, after drawing.
    #[arg(long, default_value_t = 1.0)]
    launch_factor: f64,
}

fn parse_factor(s: &str) -> Result<(Source, f64), String> {
    let (src, f) = s.split_once('=').ok_or("expected SOURCE=FACTOR")?;
    let src = SOURCES
        .into_iter()
        .find(|x| x.to_string().eq_ignore_ascii_case(src))
        .ok_or_else(|| format!("unknown source {src:?}"))?;
    Ok((src, f.parse().map_err(|e| format!("{e}"))?))
}

struct World {
    eph: Ephemeris,
    net: Network,
    econ: EconomyFile,
}

impl World {
    fn scenario(&self, seed: Option<u64>, opts: &Opts) -> Scenario {
        let mut draw = seed.map_or(Draw::Central, Draw::seeded);
        let mut sc = self
            .econ
            .scenario(&self.eph, &self.net, &mut draw, opts.months);
        for &(src, factor) in &opts.alpha {
            for k in 0..sc.facilities.len() {
                if source_of(&self.eph, &self.net, &sc, k) == src {
                    sc.facilities[k].alpha *= factor;
                }
            }
        }
        for p in sc.launch_usd_per_kg.iter_mut() {
            *p *= opts.launch_factor;
        }
        sc
    }

    /// The transport network is the same for every draw.
    fn edges(&self, sc: &Scenario, opts: &Opts) -> Vec<Edge> {
        let mut all = edges::local_edges(&self.net, &sc.cal);
        all.extend(edges::interplanetary_edges(
            &self.eph,
            &self.net,
            &sc.cal,
            &edges::routes(&self.eph, &self.net),
            opts.max_dv,
        ));
        all
    }

    fn classify(&self, sc: &Scenario, plan: &Plan, opts: &Opts) -> Classification {
        classify(
            &self.eph,
            &self.net,
            sc,
            plan,
            opts.path_years,
            opts.opening_years,
        )
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let eph = Ephemeris::standard();
    let net = Network::standard(&eph);
    let world = World {
        eph,
        net,
        econ: EconomyFile::standard(),
    };
    match cli.cmd {
        Cmd::Solve { seed, opts } => solve(&world, seed, &opts),
        Cmd::Sweep {
            draws,
            jobs,
            csv,
            opts,
        } => sweep(&world, draws, jobs, csv, &opts),
    }
}

fn solve(w: &World, seed: Option<u64>, opts: &Opts) -> Result<()> {
    let sc = w.scenario(seed, opts);
    let t0 = Instant::now();
    let edges = w.edges(&sc, opts);
    println!("{} edges in {:.1?}", edges.len(), t0.elapsed());
    let t0 = Instant::now();
    let plan = model::solve(&sc, &w.net, &edges, true);
    println!(
        "{:?}: objective {:.0} $M in {:.1?}",
        plan.status,
        plan.objective,
        t0.elapsed()
    );
    println!("{}", describe_draw(&sc));
    let per_year = (1.0 / sc.cal.period_years()).round() as usize;
    let at = |y: usize| (y * per_year).min(sc.cal.periods - 1);
    let decade = |v: &[f64], d: usize| -> f64 { v[at(10 * d)..at(10 * d + 10)].iter().sum() };
    println!("plants (capacity, t/yr of output):");
    for (f, caps) in sc.facilities.iter().zip(&plan.capacity) {
        if let Some(first) = caps.iter().position(|&c| c > 1.0) {
            println!(
                "  {:<28} α {:.3}  from {}  2045 {:>7.0}  2055 {:>7.0}  2075 {:>7.0}",
                f.id,
                f.alpha,
                sc.cal.start_of(first),
                caps[at(5)],
                caps[at(15)],
                caps[at(35)],
            );
        }
    }
    println!("by decade (2040s, 2050s, 2060s), t:");
    for c in [
        Commodity::Lox,
        Commodity::Ch4,
        Commodity::Equipment,
        Commodity::Payload,
    ] {
        let b = &plan.bought[&c];
        println!(
            "  bought {:<20} {:>9.0} {:>9.0} {:>9.0}",
            format!("{c:?}"),
            decade(b, 0),
            decade(b, 1),
            decade(b, 2)
        );
    }
    for (d, s) in sc.demand.iter().zip(&plan.served) {
        println!(
            "  served {:<20} {:>9.0} {:>9.0} {:>9.0}",
            d.id,
            decade(s, 0),
            decade(s, 1),
            decade(s, 2)
        );
    }
    println!(
        "  metal sold           {:>9.0} {:>9.0} {:>9.0}",
        decade(&plan.metal_sold, 0),
        decade(&plan.metal_sold, 1),
        decade(&plan.metal_sold, 2)
    );
    println!("largest flows, t (2040s, 2050s, 2060s):");
    let mut flows: Vec<_> = plan.shipped.iter().collect();
    flows.sort_by(|a, b| b.1.iter().sum::<f64>().total_cmp(&a.1.iter().sum::<f64>()));
    for ((from, to, c), v) in flows.into_iter().take(20) {
        println!(
            "  {:<16} → {:<16} {:<10} {:>9.0} {:>9.0} {:>9.0}",
            w.net.node(*from).id,
            w.net.node(*to).id,
            format!("{c:?}"),
            decade(v, 0),
            decade(v, 1),
            decade(v, 2)
        );
    }
    for (k, s) in sc.ships.iter().zip(&plan.ships_bought) {
        println!("ships bought: {} {:.1}", k.id, s.iter().sum::<f64>());
    }
    println!(
        "capital spending by decade, $M: {:.0} {:.0} {:.0} (budget {:.0} per decade)",
        decade(&plan.capex, 0),
        decade(&plan.capex, 1),
        decade(&plan.capex, 2),
        sc.budget_musd_per_year * 10.0
    );
    let mut cumulative = 0.0;
    let binding: Vec<String> = (0..sc.cal.periods / per_year)
        .filter(|&y| {
            cumulative += plan.capex[y * per_year..(y + 1) * per_year]
                .iter()
                .sum::<f64>();
            cumulative > 0.99 * sc.budget_musd_per_year * (y + 1) as f64
        })
        .map(|y| (sc.cal.start.ymd().0 + y as i32).to_string())
        .collect();
    println!("budget binds through: {}", binding.join(" "));
    println!("marginal value, $/kg (2040, 2045, 2055, 2070):");
    for (node, c) in [
        ("leo", Commodity::Lox),
        ("eml1", Commodity::Lox),
        ("eml1", Commodity::Ch4),
        ("llo", Commodity::Lox),
        ("moon-south-pole", Commodity::Lox),
        ("moon-south-pole", Commodity::Payload),
        ("lmo", Commodity::Lox),
        ("mars-surface", Commodity::Payload),
    ] {
        let v = &plan.price[&(w.net.lookup(node).unwrap(), c)];
        println!(
            "  {:<16} {:<8} {:>8.0} {:>8.0} {:>8.0} {:>8.0}",
            node,
            format!("{c:?}"),
            v[at(0)],
            v[at(5)],
            v[at(15)],
            v[at(30)]
        );
    }
    let c = w.classify(&sc, &plan, opts);
    println!(
        "path: {}   opening: {}   supply to {:.0}: {}",
        c.path_label(),
        c.opening_label(),
        2040.0 + opts.path_years,
        supply_shares(&c)
    );
    Ok(())
}

fn supply_shares(c: &Classification) -> String {
    let total: f64 = c.supply.iter().sum();
    let mut s = String::new();
    for src in SOURCES {
        let share = (c.supply[src as usize] / total).max(0.0);
        let _ = write!(s, "{src} {:.0}% ", 100.0 * share);
    }
    s.trim_end().to_string()
}

/// The draw's headline hidden parameters.
fn describe_draw(sc: &Scenario) -> String {
    let ice = sc
        .facilities
        .iter()
        .find(|f| f.id == "lunar-ice")
        .map_or(0.0, |f| f.grade);
    let wet = sc
        .asteroids
        .iter()
        .filter(|a| a.composition.as_deref() == Some("volatiles"))
        .count();
    let metal = sc
        .asteroids
        .iter()
        .filter(|a| a.composition.as_deref() == Some("pgm"))
        .count();
    let phobos = sc.facilities.iter().any(|f| f.id == "phobos-volatiles");
    format!(
        "launch ${:.0}→${:.0}/kg, equipment ${:.0}/kg, budget ${:.0}B/yr, lunar ice {:.1}%, \
         {wet} wet and {metal} metal asteroids, Phobos {}",
        sc.launch_usd_per_kg[0],
        sc.launch_usd_per_kg[sc.cal.periods - 1],
        sc.equipment_usd_per_kg,
        sc.budget_musd_per_year / 1000.0,
        ice * 100.0,
        if phobos { "wet" } else { "dry" },
    )
}

fn sweep(w: &World, draws: u64, jobs: usize, csv: Option<PathBuf>, opts: &Opts) -> Result<()> {
    let t0 = Instant::now();
    let edges = w.edges(&w.scenario(None, opts), opts);
    let pool = rayon::ThreadPoolBuilder::new().num_threads(jobs).build()?;
    let rows: Vec<(u64, Scenario, Plan, Classification)> = pool.install(|| {
        (1..=draws)
            .into_par_iter()
            .map(|seed| {
                let sc = w.scenario(Some(seed), opts);
                let plan = model::solve(&sc, &w.net, &edges, false);
                let c = w.classify(&sc, &plan, opts);
                eprintln!(
                    "seed {seed:>4}: {:<24} opening {:<28} {}",
                    c.path_label(),
                    c.opening_label(),
                    describe_draw(&sc)
                );
                (seed, sc, plan, c)
            })
            .collect()
    });
    println!("{draws} draws in {:.0?}", t0.elapsed());

    let failed = rows
        .iter()
        .filter(|(_, _, p, _)| p.status != highs::HighsModelStatus::Optimal)
        .count();
    if failed > 0 {
        println!("{failed} draws did not solve to optimality");
    }
    let tally = |label: &dyn Fn(&Classification) -> String| {
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for (_, _, _, c) in &rows {
            *counts.entry(label(c)).or_default() += 1;
        }
        let mut v: Vec<_> = counts.into_iter().collect();
        v.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
        v
    };
    println!(
        "\ndevelopment path (sources ≥ {:.0}% of in-space propellant to {:.0}):",
        plan::classify::PATH_SHARE * 100.0,
        2040.0 + opts.path_years
    );
    for (label, n) in tally(&|c| c.path_label()) {
        println!("  {:>5.1}%  {label}", 100.0 * n as f64 / rows.len() as f64);
    }
    println!(
        "\nopening (sources with ≥ {:.0}% of the equipment installed by {:.0}):",
        plan::classify::PATH_SHARE * 100.0,
        2040.0 + opts.opening_years
    );
    for (label, n) in tally(&|c| c.opening_label()) {
        println!("  {:>5.1}%  {label}", 100.0 * n as f64 / rows.len() as f64);
    }

    if let Some(path) = csv {
        let mut out = String::from(
            "seed,status,objective_musd,launch_start,launch_end,equipment_usd_per_kg,\
             budget_musd,lunar_ice_grade,wet_asteroids,metal_asteroids,phobos_wet,\
             supply_earth_t,supply_moon_t,supply_mars_t,supply_asteroids_t,\
             opening_moon_t,opening_mars_t,opening_asteroids_t,path,opening\n",
        );
        for (seed, sc, plan, c) in &rows {
            let ice = sc
                .facilities
                .iter()
                .find(|f| f.id == "lunar-ice")
                .map_or(0.0, |f| f.grade);
            let count = |what: &str| {
                sc.asteroids
                    .iter()
                    .filter(|a| a.composition.as_deref() == Some(what))
                    .count()
            };
            let _ = writeln!(
                out,
                "{seed},{:?},{:.0},{:.0},{:.0},{:.0},{:.0},{:.4},{},{},{},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{},{}",
                plan.status,
                plan.objective,
                sc.launch_usd_per_kg[0],
                sc.launch_usd_per_kg[sc.cal.periods - 1],
                sc.equipment_usd_per_kg,
                sc.budget_musd_per_year,
                ice,
                count("volatiles"),
                count("pgm"),
                sc.facilities.iter().any(|f| f.id == "phobos-volatiles"),
                c.supply[0],
                c.supply[1],
                c.supply[2],
                c.supply[3],
                c.opening_equipment[1],
                c.opening_equipment[2],
                c.opening_equipment[3],
                c.path_label(),
                c.opening_label(),
            );
        }
        std::fs::write(&path, out)?;
        println!("\nwrote {}", path.display());
    }
    Ok(())
}
