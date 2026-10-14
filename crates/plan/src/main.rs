//! `plan`: solve the M2 kill-test LP for one draw, or sweep many.

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use orbit::ephem::Ephemeris;
use orbit::routes::RouteGraph;
use plan::classify::{Classification, SOURCES, Source, classify, programs_of, source_of};
use plan::edges::{self, Edge};
use plan::model::{Commodity, Lp, Plan, ProgramSetup};
use plan::params::{Draw, EconomyFile, Scenario};
use plan::search::{self, Outcome};
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
    #[arg(long, default_value_t = 6)]
    months: u32,
    /// Ignore programs: every plant may be built from the start at no fixed
    /// cost (the first sweep's model).
    #[arg(long)]
    free: bool,
    /// Also find the best plan using each in-space source alone.
    #[arg(long, conflicts_with = "free")]
    alternatives: bool,
    /// Interplanetary edges cost at most this, km/s.
    #[arg(long, default_value_t = 6.0)]
    max_dv: f64,
    /// The development path is read from propellant supply over these
    /// first years.
    #[arg(long, default_value_t = 20.0)]
    path_years: f64,
    /// Opening moves are the capital committed (programs and equipment) in
    /// these first years.
    #[arg(long, default_value_t = 5.0)]
    opening_years: f64,
    /// Multiplies the equipment per t/yr (α) of every plant at one source,
    /// after drawing: e.g. `asteroids=3`. Repeatable.
    #[arg(long, value_parser = parse_factor)]
    alpha: Vec<(Source, f64)>,
    /// Multiplies the launch price, after drawing.
    #[arg(long, default_value_t = 1.0)]
    launch_factor: f64,
    /// Keep one station orbit as the hub: the others, with their legs and
    /// gateways, leave the route graph (M1a step 11).
    #[arg(long, value_parser = STATIONS)]
    hub: Option<String>,
}

/// The route graph's station orbits.
const STATIONS: [&str; 4] = ["l1", "l2", "nrho", "dro"];

fn parse_factor(s: &str) -> Result<(Source, f64), String> {
    let (src, f) = s.split_once('=').ok_or("expected SOURCE=FACTOR")?;
    let src = SOURCES
        .into_iter()
        .find(|x| x.to_string().eq_ignore_ascii_case(src))
        .ok_or_else(|| format!("unknown source {src:?}"))?;
    Ok((src, f.parse().map_err(|e| format!("{e}"))?))
}

/// The best plan of one draw, and with `--alternatives` the objective of
/// the best plan using each in-space source alone.
struct Planned {
    best: Outcome,
    alone: Vec<(Source, Option<f64>)>,
}

struct World {
    eph: &'static Ephemeris,
    graph: RouteGraph,
    econ: EconomyFile,
}

impl World {
    fn scenario(&self, seed: Option<u64>, opts: &Opts) -> Scenario {
        let mut draw = seed.map_or(Draw::Central, Draw::seeded);
        let mut sc = self
            .econ
            .scenario(self.eph, &self.graph, &mut draw, opts.months);
        for &(src, factor) in &opts.alpha {
            for k in 0..sc.facilities.len() {
                if source_of(self.eph, &self.graph, &sc, k) == src {
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
        let mut all = edges::local_edges(&self.graph, &sc.cal);
        all.extend(edges::interplanetary_edges(
            self.eph,
            &self.graph,
            &sc.cal,
            &edges::pairs(self.eph, &self.graph),
            opts.max_dv,
        ));
        all
    }

    fn plan(&self, sc: &Scenario, edges: &[Edge], opts: &Opts, verbose: bool) -> Planned {
        let mut lp = Lp::new(sc, &self.graph, edges, verbose);
        if opts.free {
            return Planned {
                best: search::free(&mut lp, sc),
                alone: Vec::new(),
            };
        }
        let best = search::optimise(&mut lp, sc, &vec![true; sc.programs.len()]);
        let alone = if opts.alternatives {
            SOURCES[1..]
                .iter()
                .map(|&src| {
                    let allowed = programs_of(self.eph, &self.graph, sc, src);
                    let out = search::optimise(&mut lp, sc, &allowed);
                    let objective = (out.plan.status == highs::HighsModelStatus::Optimal)
                        .then_some(out.plan.objective);
                    (src, objective)
                })
                .collect()
        } else {
            Vec::new()
        };
        Planned { best, alone }
    }

    fn classify(&self, sc: &Scenario, plan: &Plan, opts: &Opts) -> Classification {
        classify(
            self.eph,
            &self.graph,
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
    let (Cmd::Solve { opts, .. } | Cmd::Sweep { opts, .. }) = &cli.cmd;
    let mut graph = edges::standard(eph);
    if let Some(hub) = &opts.hub {
        let others: Vec<&str> = STATIONS.into_iter().filter(|s| s != hub).collect();
        graph = graph.without(&others);
    }
    let world = World {
        eph,
        graph,
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
    let planned = w.plan(&sc, &edges, opts, true);
    let out = &planned.best;
    let plan = &out.plan;
    println!(
        "{:?}: objective {:.0} $M, {} solves in {:.1?}",
        plan.status,
        plan.objective,
        out.solves,
        t0.elapsed()
    );
    println!("{}", describe_draw(&sc));
    if let Some(v) = out.earth_only {
        println!("Earth-only: objective {v:.0} $M");
    }
    for &(j, v) in &out.drops {
        match v {
            Some(v) => println!("without {}: objective {v:.0} $M", sc.programs[j].id),
            None => println!("without {}: not optimal", sc.programs[j].id),
        }
    }
    for (src, v) in &planned.alone {
        match v {
            Some(v) => println!("{src} alone: objective {v:.0} $M"),
            None => println!("{src} alone: not optimal"),
        }
    }
    println!("programs funded (cost $M, plants installable from):");
    for (prog, s) in sc.programs.iter().zip(&out.setup) {
        if let ProgramSetup::Funded { ready } = s {
            println!(
                "  {:<34} {:>7.0}  {}",
                prog.id,
                prog.cost_musd,
                sc.cal.start_of(*ready)
            );
        }
    }
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
        Commodity::Lh2,
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
    for ((d, s), at) in sc.demand.iter().zip(&plan.served).zip(&plan.delivered) {
        println!(
            "  served {:<20} {:>9.0} {:>9.0} {:>9.0}",
            d.id,
            decade(s, 0),
            decade(s, 1),
            decade(s, 2)
        );
        if d.nodes.len() > 1 && d.nodes.len() < 10 {
            for (n, v) in d.nodes.iter().zip(at) {
                println!(
                    "    at {:<20} {:>9.0} {:>9.0} {:>9.0}",
                    w.graph.place(*n).id,
                    decade(v, 0),
                    decade(v, 1),
                    decade(v, 2)
                );
            }
        }
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
            w.graph.place(*from).id,
            w.graph.place(*to).id,
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
    let stations = STATIONS
        .into_iter()
        .flat_map(|s| [Commodity::Lox, Commodity::Ch4, Commodity::Lh2].map(|c| (s, c)));
    let others = [
        ("llo", Commodity::Lox),
        ("moon-south-pole", Commodity::Lox),
        ("moon-south-pole", Commodity::Payload),
        ("lmo", Commodity::Lox),
        ("mars-surface", Commodity::Payload),
    ];
    for (node, c) in [("leo", Commodity::Lox)]
        .into_iter()
        .chain(stations)
        .chain(others)
    {
        let Some(place) = w.graph.lookup(node) else {
            continue;
        };
        let v = &plan.price[&(place, c)];
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
    let c = w.classify(&sc, plan, opts);
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

/// Grade of the richest lunar ice deposit.
fn best_lunar_ice(sc: &Scenario) -> f64 {
    sc.facilities
        .iter()
        .filter(|f| f.id == "lunar-ice" || f.id.starts_with("lunar-ice:"))
        .map(|f| f.grade)
        .fold(0.0, f64::max)
}

/// The draw's headline hidden parameters.
fn describe_draw(sc: &Scenario) -> String {
    let ice = best_lunar_ice(sc);
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
        "launch ${:.0}→${:.0}/kg, equipment ${:.0}/kg, budget ${:.0}B/yr, richest lunar ice {:.1}%, \
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
    let rows: Vec<(u64, Scenario, Planned, Classification)> = pool.install(|| {
        (1..=draws)
            .into_par_iter()
            .map(|seed| {
                let sc = w.scenario(Some(seed), opts);
                let t0 = Instant::now();
                let planned = w.plan(&sc, &edges, opts, false);
                let c = w.classify(&sc, &planned.best.plan, opts);
                eprintln!(
                    "seed {seed:>4}: {:<24} opening {:<28} {:.0?}; {}",
                    c.path_label(),
                    c.opening_label(),
                    t0.elapsed(),
                    describe_draw(&sc)
                );
                (seed, sc, planned, c)
            })
            .collect()
    });
    println!("{draws} draws in {:.0?}", t0.elapsed());

    let failed = rows
        .iter()
        .filter(|(_, _, p, _)| p.best.plan.status != highs::HighsModelStatus::Optimal)
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
        "\nopening (sources with ≥ {:.0}% of the capital committed by {:.0}):",
        plan::classify::PATH_SHARE * 100.0,
        2040.0 + opts.opening_years
    );
    for (label, n) in tally(&|c| c.opening_label()) {
        println!("  {:>5.1}%  {label}", 100.0 * n as f64 / rows.len() as f64);
    }

    if let Some(path) = csv {
        let mut csv_text = String::from(
            "seed,status,objective_musd,launch_start,launch_end,equipment_usd_per_kg,\
             budget_musd,lunar_ice_best_grade,wet_asteroids,metal_asteroids,phobos_wet,\
             supply_earth_t,supply_moon_t,supply_mars_t,supply_asteroids_t,\
             opening_moon_musd,opening_mars_musd,opening_asteroids_musd,path,opening,\
             solves,program_musd,programs,earth_only_musd,drops,\
             moon_alone_musd,mars_alone_musd,asteroids_alone_musd\n",
        );
        for (seed, sc, planned, c) in &rows {
            let out = &planned.best;
            let plan = &out.plan;
            let alone = |src: Source| {
                planned
                    .alone
                    .iter()
                    .find(|(s, _)| *s == src)
                    .and_then(|(_, v)| *v)
                    .map_or(String::new(), |v| format!("{v:.0}"))
            };
            let funded: Vec<&str> = sc
                .programs
                .iter()
                .zip(&out.setup)
                .filter(|(_, s)| matches!(s, ProgramSetup::Funded { .. }))
                .map(|(p, _)| p.id.as_str())
                .collect();
            let ice = best_lunar_ice(sc);
            let count = |what: &str| {
                sc.asteroids
                    .iter()
                    .filter(|a| a.composition.as_deref() == Some(what))
                    .count()
            };
            let _ = writeln!(
                csv_text,
                "{seed},{:?},{:.0},{:.0},{:.0},{:.0},{:.0},{:.4},{},{},{},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{},{},{},{:.0},{},{},{},{},{},{}",
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
                c.opening_musd[1],
                c.opening_musd[2],
                c.opening_musd[3],
                c.path_label(),
                c.opening_label(),
                out.solves,
                plan.spend.iter().flatten().sum::<f64>(),
                funded.join(";"),
                out.earth_only.map_or(String::new(), |v| format!("{v:.0}")),
                out.drops
                    .iter()
                    .map(|&(j, v)| format!(
                        "{}={}",
                        sc.programs[j].id,
                        v.map_or("-".into(), |v| format!("{v:.0}"))
                    ))
                    .collect::<Vec<_>>()
                    .join(";"),
                alone(Source::Moon),
                alone(Source::Mars),
                alone(Source::Asteroids),
            );
        }
        std::fs::write(&path, csv_text)?;
        println!("\nwrote {}", path.display());
    }
    Ok(())
}
