//! The linear program: one plan for the whole scenario, with perfect
//! foresight. See M2.md for the formulation.

use crate::edges::{Edge, EdgeKind};
use crate::params::{PROPELLANTS, Propellant, Scenario, ShipClass};
use highs::{Col, HighsModelStatus, Model, RowProblem, Sense, SolvedModel};
use orbit::routes::{PlaceId, RouteGraph};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Commodity {
    Lox,
    Ch4,
    Lh2,
    Equipment,
    Payload,
    Metal,
}

pub const COMMODITIES: [Commodity; 6] = [
    Commodity::Lox,
    Commodity::Ch4,
    Commodity::Lh2,
    Commodity::Equipment,
    Commodity::Payload,
    Commodity::Metal,
];

impl Commodity {
    fn index(self) -> usize {
        self as usize
    }

    /// The fuel burned with LOX in `p`.
    pub fn fuel(p: Propellant) -> Commodity {
        match p {
            Propellant::Methalox => Commodity::Ch4,
            Propellant::Hydrolox => Commodity::Lh2,
        }
    }

    /// Whether a ship burning `p` carries this in its tanks rather than its
    /// hold.
    fn tanked(self, p: Propellant) -> bool {
        self == Commodity::Lox || self == Commodity::fuel(p)
    }
}

/// Fraction of a stock of `c` left after `days`.
fn kept(sc: &Scenario, c: Commodity, days: f64) -> f64 {
    PROPELLANTS
        .iter()
        .find(|&&p| Commodity::fuel(p) == c)
        .map_or(1.0, |&p| {
            (1.0 - sc.propellant(p).fuel_boiloff_per_day).powf(days)
        })
}

/// $/kg × t → $M.
pub const MUSD_PER_USD_T: f64 = 1e-3;

/// Deposits larger than this are treated as unlimited: ~30× the largest
/// plant over the whole scenario.
const RESERVE_UNLIMITED_T: f64 = 1e9;

/// How a solve treats one program (see [`Scenario::programs`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProgramSetup {
    /// Not available: plants that need it are not built.
    Closed,
    /// Paid in full before period `ready`, from which its plants may be
    /// installed.
    Funded { ready: usize },
    /// Not paid. Its plants may be installed from period `ready`, at
    /// `musd_per_t` per tonne of equipment installed (discounted like any
    /// other cost).
    Charged { ready: usize, musd_per_t: f64 },
}

impl ProgramSetup {
    /// The first period its plants may be installed, unless closed.
    pub fn ready(self) -> Option<usize> {
        match self {
            ProgramSetup::Closed => None,
            ProgramSetup::Funded { ready } | ProgramSetup::Charged { ready, .. } => Some(ready),
        }
    }
}

/// A solved plan, by period.
#[derive(Debug)]
pub struct Plan {
    pub status: HighsModelStatus,
    /// Discounted net value, $M, including any per-tonne program charges.
    pub objective: f64,
    /// Capacity of each facility option (t/yr of rated output).
    pub capacity: Vec<Vec<f64>>,
    /// Equipment installed at each facility option, t.
    pub installed: Vec<Vec<f64>>,
    /// Tonnes bought at LEO, by commodity.
    pub bought: HashMap<Commodity, Vec<f64>>,
    /// Payload delivered, by demand category.
    pub served: Vec<Vec<f64>>,
    /// The same at each of the category's nodes, [category][node][period].
    pub delivered: Vec<Vec<Vec<f64>>>,
    /// Metals sold, t.
    pub metal_sold: Vec<f64>,
    /// Ships bought, by class.
    pub ships_bought: Vec<Vec<f64>>,
    /// Cargo moved from node to node, by commodity, t per period of
    /// departure.
    pub shipped: HashMap<(PlaceId, PlaceId, Commodity), Vec<f64>>,
    /// Capital spending (equipment, ships and programs), $M per period.
    pub capex: Vec<f64>,
    /// Spending on each program, $M per period.
    pub spend: Vec<Vec<f64>>,
    /// Marginal value of one more tonne at a node, by commodity, $/kg in the
    /// period's own money (undiscounted).
    pub price: HashMap<(PlaceId, Commodity), Vec<f64>>,
}

/// Linear terms of one row, merged by column (HiGHS rejects duplicates).
#[derive(Default)]
struct Terms(HashMap<usize, (Col, f64)>);

impl Terms {
    fn add(&mut self, col: Col, coef: f64) {
        self.0
            .entry(col.index())
            .and_modify(|(_, c)| *c += coef)
            .or_insert((col, coef));
    }

    fn row(&self) -> Vec<(Col, f64)> {
        self.0.values().copied().collect()
    }
}

/// The cargo variables of one ship class on one edge.
struct EdgeVars {
    edge: Edge,
    cargo: Vec<(Commodity, Col)>,
}

/// One scenario's LP, kept between solves that change only which programs
/// are open and what they cost.
pub struct Lp {
    /// `None` only while solving.
    model: Option<Model>,
    verbose: bool,
    t_n: usize,
    /// Discount factor of each period.
    disc: Vec<f64>,
    nodes: Vec<PlaceId>,
    first_balance_row: usize,
    buy_cols: Vec<(Commodity, usize, Col)>,
    metal_cols: Vec<(usize, Col)>,
    /// [demand][period][node]
    served_cols: Vec<Vec<Vec<Col>>>,
    cap_cols: Vec<Vec<Col>>,
    install_cols: Vec<Vec<Col>>,
    ship_buy: Vec<Vec<Col>>,
    edge_vars: Vec<EdgeVars>,
    /// Capital spending of each period, as (column, $M per unit).
    capex_terms: Vec<Vec<(Col, f64)>>,
    /// Per program: whether it is paid for (fixed to 0 or 1 per solve).
    select_cols: Vec<Col>,
    /// Per program and period: spending, $M.
    spend_cols: Vec<Vec<Col>>,
}

impl Lp {
    pub fn new(sc: &Scenario, graph: &RouteGraph, edges: &[Edge], verbose: bool) -> Lp {
        let t_n = sc.cal.periods;
        let nodes: Vec<PlaceId> = graph.places().map(|(id, _)| id).collect();
        let leo = graph.lookup("leo").expect("LEO is a place");
        let earth_system = graph.place(leo).system;
        let disc: Vec<f64> = (0..t_n).map(|p| sc.discount(p)).collect();
        let dt = sc.cal.period_years();

        let mut pb = RowProblem::new();
        // balance[node][commodity][period]: inflow − outflow − Δstock = 0.
        let mut balance: Vec<Vec<Vec<Terms>>> = (0..nodes.len())
            .map(|_| {
                (0..COMMODITIES.len())
                    .map(|_| (0..t_n).map(|_| Terms::default()).collect())
                    .collect()
            })
            .collect();
        let node_index: HashMap<PlaceId, usize> =
            nodes.iter().enumerate().map(|(k, &id)| (id, k)).collect();
        macro_rules! bal {
            ($n:expr, $c:expr, $p:expr) => {
                balance[node_index[&$n]][$c.index()][$p]
            };
        }

        // Stocks, less boil-off over each period.
        for node_rows in balance.iter_mut() {
            for (c, c_rows) in COMMODITIES.iter().zip(node_rows.iter_mut()) {
                let keep = kept(sc, *c, sc.cal.period_days);
                let mut prev: Option<Col> = None;
                for row in c_rows.iter_mut() {
                    let s = pb.add_column(0.0, 0.0..);
                    row.add(s, -1.0);
                    if let Some(prev) = prev {
                        row.add(prev, keep);
                    }
                    prev = Some(s);
                }
            }
        }

        // Purchases at LEO.
        let mut capex_terms: Vec<Terms> = (0..t_n).map(|_| Terms::default()).collect();
        let mut buy_cols: Vec<(Commodity, usize, Col)> = Vec::new();
        for p in 0..t_n {
            let launch = sc.launch_usd_per_kg[p];
            for (c, goods) in [
                (Commodity::Lox, sc.propellant_usd_per_kg),
                (Commodity::Ch4, sc.propellant_usd_per_kg),
                (Commodity::Lh2, sc.propellant_usd_per_kg),
                (Commodity::Equipment, sc.equipment_usd_per_kg),
                (Commodity::Payload, 0.0),
            ] {
                let usd = (goods + launch) * MUSD_PER_USD_T;
                let b = pb.add_column(-disc[p] * usd, 0.0..);
                bal!(leo, c, p).add(b, 1.0);
                if c == Commodity::Equipment {
                    capex_terms[p].add(b, usd);
                }
                buy_cols.push((c, p, b));
            }
        }

        // Metals sold at LEO, in price steps by cumulative tonnes.
        let mut metal_cols: Vec<(usize, Col)> = Vec::new();
        for &(tonnes, usd_per_kg) in &sc.metal_steps {
            let usd = usd_per_kg * MUSD_PER_USD_T;
            let mut step = Terms::default();
            for p in 0..t_n {
                let s = pb.add_column(disc[p] * usd, 0.0..);
                bal!(leo, Commodity::Metal, p).add(s, -1.0);
                capex_terms[p].add(s, -usd);
                step.add(s, 1.0);
                metal_cols.push((p, s));
            }
            pb.add_row(..=tonnes, step.row());
        }

        // Demand.
        let mut served_cols: Vec<Vec<Vec<Col>>> = Vec::new();
        for d in &sc.demand {
            let mut by_period = Vec::new();
            for p in 0..t_n {
                let growth = (1.0 + d.growth).powf(sc.cal.years_at(p));
                let mut serve = Terms::default();
                for seg in &d.segments {
                    let s = pb.add_column(
                        disc[p] * seg.usd_per_kg * MUSD_PER_USD_T,
                        0.0..=seg.tpy * growth * dt,
                    );
                    serve.add(s, 1.0);
                }
                let mut delivered = Vec::new();
                for &n in &d.nodes {
                    let x = pb.add_column(0.0, 0.0..);
                    match d.what.propellant() {
                        None => bal!(n, Commodity::Payload, p).add(x, -1.0),
                        Some(prop) => {
                            let lox = sc.propellant(prop).lox_share;
                            bal!(n, Commodity::Lox, p).add(x, -lox);
                            bal!(n, Commodity::fuel(prop), p).add(x, lox - 1.0);
                        }
                    }
                    serve.add(x, -1.0);
                    delivered.push(x);
                }
                pb.add_row(0.0..=0.0, serve.row());
                by_period.push(delivered);
            }
            served_cols.push(by_period);
        }

        // Facilities. Installs are opened per solve, by the programs.
        let mut cap_cols: Vec<Vec<Col>> = Vec::new();
        let mut install_cols: Vec<Vec<Col>> = Vec::new();
        for f in &sc.facilities {
            let installs: Vec<Col> = (0..t_n)
                .map(|p| {
                    let x = pb.add_column(0.0, 0.0..);
                    bal!(f.node, Commodity::Equipment, p).add(x, -1.0);
                    x
                })
                .collect();
            let mut caps = Vec::new();
            let mut reserve = Terms::default();
            for p in 0..t_n {
                let cap = pb.add_column(0.0, 0.0..);
                let mut grow = Terms::default();
                grow.add(cap, 1.0);
                if let Some(&prev) = caps.last() {
                    grow.add(prev, -1.0);
                }
                if p >= f.build_periods {
                    grow.add(installs[p - f.build_periods], -1.0 / f.alpha);
                }
                pb.add_row(0.0..=0.0, grow.row());
                for (c, y) in [
                    (Commodity::Lox, f.output.lox),
                    (Commodity::Ch4, f.output.ch4),
                    (Commodity::Lh2, f.output.lh2),
                    (Commodity::Metal, f.output.metal),
                ] {
                    if y > 0.0 {
                        bal!(f.node, c, p).add(cap, y * dt);
                    }
                }
                reserve.add(cap, dt);
                caps.push(cap);
            }
            // A deposit that outlasts any plausible plant needs no row (and a
            // huge bound would hurt the solver's scaling).
            if let Some(r) = f.reserve.filter(|&r| r < RESERVE_UNLIMITED_T) {
                pb.add_row(..=r, reserve.row());
            }
            cap_cols.push(caps);
            install_cols.push(installs);
        }

        // Programs: spending sums to the cost if selected. Selection and the
        // spending window are set per solve.
        let mut select_cols = Vec::new();
        let mut spend_cols = Vec::new();
        for prog in &sc.programs {
            let y = pb.add_column(0.0, 0.0..=0.0);
            let mut total = Terms::default();
            total.add(y, -prog.cost_musd);
            let spends = (0..t_n)
                .map(|p| {
                    let g = pb.add_column(-disc[p], 0.0..=0.0);
                    total.add(g, 1.0);
                    capex_terms[p].add(g, 1.0);
                    g
                })
                .collect();
            pb.add_row(0.0..=0.0, total.row());
            select_cols.push(y);
            spend_cols.push(spends);
        }

        // Fleet: ships at each node at the end of each period.
        let mut systems = Vec::new();
        for (_, n) in graph.places() {
            if !systems.contains(&n.system) {
                systems.push(n.system);
            }
        }
        let mut fleet: Vec<Vec<Vec<Col>>> = Vec::new(); // [class][node][period]
        let mut fleet_bal: Vec<Vec<Vec<Terms>>> = Vec::new();
        let mut ship_buy: Vec<Vec<Col>> = Vec::new();
        for ship in &sc.ships {
            let mut by_node = Vec::new();
            let mut bal_node = Vec::new();
            for _ in &nodes {
                let mut cols = Vec::new();
                let mut rows = Vec::new();
                for _ in 0..t_n {
                    let f = pb.add_column(0.0, 0.0..);
                    let mut r = Terms::default();
                    r.add(f, -1.0);
                    if let Some(&prev) = cols.last() {
                        r.add(prev, 1.0);
                    }
                    cols.push(f);
                    rows.push(r);
                }
                by_node.push(cols);
                bal_node.push(rows);
            }
            let buys: Vec<Col> = (0..t_n)
                .map(|p| {
                    let b = pb.add_column(-disc[p] * ship.price_musd, 0.0..);
                    bal_node[node_index[&leo]][p].add(b, 1.0);
                    capex_terms[p].add(b, ship.price_musd);
                    b
                })
                .collect();
            fleet.push(by_node);
            fleet_bal.push(bal_node);
            ship_buy.push(buys);
        }
        // Ship-days per class, system and period: local trips × (days +
        // turnaround) ≤ period days × ships in the system at the period's
        // start (including ships bought in the period).
        let mut ship_days: Vec<HashMap<(usize, usize), Terms>> =
            sc.ships.iter().map(|_| HashMap::new()).collect();
        let sys_index = |n: PlaceId| {
            systems
                .iter()
                .position(|&s| s == graph.place(n).system)
                .unwrap()
        };

        // Edges.
        let can_fly = |ship: &ShipClass, e: &Edge| -> bool {
            (!e.aero || ship.aero)
                && match e.kind {
                    EdgeKind::Interplanetary => ship.interplanetary,
                    EdgeKind::Local => [e.from, e.to]
                        .iter()
                        .all(|n| !sc.surfaces.contains(n) || ship.lands.contains(n)),
                }
        };
        let mut edge_vars: Vec<EdgeVars> = Vec::new();
        for e in edges {
            let system = |n: PlaceId| graph.place(n).system;
            let inbound = system(e.from) != earth_system && system(e.to) == earth_system;
            let outbound = system(e.from) == earth_system && system(e.to) != earth_system;
            for (k, ship) in sc.ships.iter().enumerate() {
                if !can_fly(ship, e) {
                    continue;
                }
                let prop = sc.propellant(ship.propellant);
                let r = (e.dv / prop.ve).exp() - 1.0;
                if !ship.interplanetary && system(e.from) != earth_system {
                    continue;
                }
                if r * ship.dry >= ship.tank {
                    continue;
                }
                let trips = pb.add_column(-disc[e.leave] * ship.trip_musd, 0.0..);
                let mut cargo = Vec::new();
                for c in COMMODITIES {
                    let allowed = match c {
                        Commodity::Lox | Commodity::Ch4 | Commodity::Lh2 => true,
                        Commodity::Equipment | Commodity::Payload => !inbound,
                        Commodity::Metal => !outbound,
                    };
                    if allowed {
                        cargo.push((c, pb.add_column(0.0, 0.0..)));
                    }
                }
                // Cargo hold and tanks.
                let mut hold = Terms::default();
                hold.add(trips, -ship.cargo);
                let mut tank = Terms::default();
                tank.add(trips, r * ship.dry - ship.tank);
                for &(c, x) in &cargo {
                    if c.tanked(ship.propellant) {
                        tank.add(x, 1.0 + r);
                    } else {
                        hold.add(x, 1.0);
                        tank.add(x, r);
                    }
                }
                pb.add_row(..=0.0, hold.row());
                pb.add_row(..=0.0, tank.row());
                // Burn at the origin, cargo out of the origin and into the
                // destination.
                let burn =
                    |b: &mut Terms, share: f64, x: Col, mass: f64| b.add(x, -share * r * mass);
                for (c, share) in [
                    (Commodity::Lox, prop.lox_share),
                    (Commodity::fuel(ship.propellant), 1.0 - prop.lox_share),
                ] {
                    burn(&mut bal!(e.from, c, e.leave), share, trips, ship.dry);
                    for &(_, x) in &cargo {
                        burn(&mut bal!(e.from, c, e.leave), share, x, 1.0);
                    }
                }
                for &(c, x) in &cargo {
                    bal!(e.from, c, e.leave).add(x, -1.0);
                    bal!(e.to, c, e.arrive).add(x, kept(sc, c, e.days));
                }
                // Ships.
                fleet_bal[k][node_index[&e.from]][e.leave].add(trips, -1.0);
                fleet_bal[k][node_index[&e.to]][e.arrive].add(trips, 1.0);
                if e.kind == EdgeKind::Local {
                    ship_days[k]
                        .entry((sys_index(e.from), e.leave))
                        .or_default()
                        .add(trips, e.days + ship.turnaround_days);
                }
                edge_vars.push(EdgeVars { edge: *e, cargo });
            }
        }
        for (k, rows) in ship_days.iter_mut().enumerate() {
            for (&(s, p), terms) in rows.iter_mut() {
                if p > 0 {
                    for (n, _) in graph.places().filter(|&(n, _)| sys_index(n) == s) {
                        terms.add(fleet[k][node_index[&n]][p - 1], -sc.cal.period_days);
                    }
                }
                if sys_index(leo) == s {
                    terms.add(ship_buy[k][p], -sc.cal.period_days);
                }
                pb.add_row(..=0.0, terms.row());
            }
        }
        for rows in &fleet_bal {
            for node_rows in rows {
                for r in node_rows {
                    pb.add_row(0.0..=0.0, r.row());
                }
            }
        }

        // Budget: cumulative capital spending ≤ cumulative appropriation, at
        // the end of every year.
        let per_year = (1.0 / dt).round() as usize;
        let mut cumulative = Terms::default();
        for (p, terms) in capex_terms.iter().enumerate() {
            for &(col, coef) in terms.0.values() {
                cumulative.add(col, coef);
            }
            if (p + 1) % per_year == 0 || p + 1 == t_n {
                let years = (p + 1) as f64 * dt;
                pb.add_row(..=sc.budget_musd_per_year * years, cumulative.row());
            }
        }

        let first_balance_row = pb.num_rows();
        for node_rows in &balance {
            for c_rows in node_rows {
                for r in c_rows {
                    pb.add_row(0.0..=0.0, r.row());
                }
            }
        }

        if verbose {
            eprintln!("LP: {} columns, {} rows", pb.num_cols(), pb.num_rows());
        }
        let mut model = pb.optimise(Sense::Maximise);
        if verbose {
            model.set_option("output_flag", true);
            model.set_option("log_to_console", true);
        } else {
            // Quiet solves run side by side in sweeps.
            model.make_quiet();
            model.set_option("threads", 1);
        }
        // Interior point with crossover, from scratch every solve: ~9 s on
        // the six-monthly LP. Dual simplex from scratch had not finished
        // after 5 minutes, and simplex warm-started from the previous
        // solve's basis took 7–45 s (primal simplex up to 160 s).
        model.set_option("solver", "ipm");
        Lp {
            model: Some(model),
            verbose,
            t_n,
            disc,
            nodes,
            first_balance_row,
            buy_cols,
            metal_cols,
            served_cols,
            cap_cols,
            install_cols,
            ship_buy,
            edge_vars,
            capex_terms: capex_terms
                .into_iter()
                .map(|t| t.0.into_values().filter(|&(_, coef)| coef > 0.0).collect())
                .collect(),
            select_cols,
            spend_cols,
        }
    }

    /// Solves with the programs set up as given, one entry per program.
    pub fn solve(&mut self, sc: &Scenario, setup: &[ProgramSetup]) -> Plan {
        assert_eq!(setup.len(), sc.programs.len());
        let mut model = self
            .model
            .take()
            .expect("the model is back after each solve");
        for (j, (prog, s)) in sc.programs.iter().zip(setup).enumerate() {
            let (selected, window) = match *s {
                ProgramSetup::Funded { ready } => {
                    assert!(ready >= prog.lead_periods, "{} ready too early", prog.id);
                    (1.0, ready)
                }
                _ => (0.0, 0),
            };
            model.change_column_bounds(self.select_cols[j], selected..=selected);
            let rate = prog.cost_musd / prog.lead_periods as f64;
            for (p, &g) in self.spend_cols[j].iter().enumerate() {
                model.change_column_bounds(g, 0.0..=if p < window { rate } else { 0.0 });
            }
        }
        for (k, f) in sc.facilities.iter().enumerate() {
            // Installable from the latest program's ready period; never if
            // a program is closed.
            let ready = f
                .programs
                .iter()
                .try_fold(0, |acc, &j| setup[j].ready().map(|r| acc.max(r)));
            let charge: f64 = f
                .programs
                .iter()
                .map(|&j| match setup[j] {
                    ProgramSetup::Charged { musd_per_t, .. } => musd_per_t,
                    _ => 0.0,
                })
                .sum();
            for (p, &x) in self.install_cols[k].iter().enumerate() {
                let open = ready.is_some_and(|r| p >= r);
                model.change_column_bounds(x, 0.0..=if open { f64::INFINITY } else { 0.0 });
                model.change_column_cost(x, -self.disc[p] * charge);
            }
        }
        let t0 = std::time::Instant::now();
        let solved = model.solve();
        let plan = self.extract(&solved);
        if self.verbose {
            eprintln!(
                "solve: {:?}, objective {:.0}, {} simplex iterations, {:.1?}",
                plan.status,
                plan.objective,
                solved.simplex_iteration_count(),
                t0.elapsed()
            );
        }
        self.model = Some(solved.into());
        plan
    }

    fn extract(&self, solved: &SolvedModel) -> Plan {
        let t_n = self.t_n;
        let status = solved.status();
        let sol = solved.get_solution();
        let v = sol.columns();
        let val = |c: Col| v[c.index()];
        let series = |cs: &[Col]| -> Vec<f64> { cs.iter().map(|&c| val(c)).collect() };
        // A balance row's dual is the discounted value of one tonne less
        // there.
        let duals = sol.dual_rows();
        let mut price = HashMap::new();
        for (k, &n) in self.nodes.iter().enumerate() {
            for c in COMMODITIES {
                let row = self.first_balance_row + (k * COMMODITIES.len() + c.index()) * t_n;
                let usd_per_kg = (0..t_n)
                    .map(|p| -duals[row + p] / self.disc[p] / MUSD_PER_USD_T)
                    .collect();
                price.insert((n, c), usd_per_kg);
            }
        }

        let mut bought: HashMap<Commodity, Vec<f64>> = HashMap::new();
        for &(c, p, col) in &self.buy_cols {
            bought.entry(c).or_insert_with(|| vec![0.0; t_n])[p] += val(col);
        }
        let mut metal_sold = vec![0.0; t_n];
        for &(p, col) in &self.metal_cols {
            metal_sold[p] += val(col);
        }
        let capex = self
            .capex_terms
            .iter()
            .map(|terms| terms.iter().map(|&(c, coef)| coef * val(c)).sum())
            .collect();
        let mut shipped: HashMap<(PlaceId, PlaceId, Commodity), Vec<f64>> = HashMap::new();
        for ev in &self.edge_vars {
            let e = &ev.edge;
            for &(c, x) in &ev.cargo {
                shipped
                    .entry((e.from, e.to, c))
                    .or_insert_with(|| vec![0.0; t_n])[e.leave] += val(x);
            }
        }

        Plan {
            status,
            objective: solved.objective_value(),
            capacity: self.cap_cols.iter().map(|cs| series(cs)).collect(),
            installed: self.install_cols.iter().map(|cs| series(cs)).collect(),
            bought,
            served: self
                .served_cols
                .iter()
                .map(|by_p| {
                    by_p.iter()
                        .map(|xs| xs.iter().map(|&x| val(x)).sum())
                        .collect()
                })
                .collect(),
            delivered: self
                .served_cols
                .iter()
                .map(|by_p| {
                    (0..by_p[0].len())
                        .map(|k| by_p.iter().map(|xs| val(xs[k])).collect())
                        .collect()
                })
                .collect(),
            metal_sold,
            ships_bought: self.ship_buy.iter().map(|bs| series(bs)).collect(),
            shipped,
            capex,
            spend: self.spend_cols.iter().map(|gs| series(gs)).collect(),
            price,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::ProgramKind;
    use crate::testing::{richest_lunar_ice, short};

    fn free(sc: &Scenario) -> Vec<ProgramSetup> {
        vec![
            ProgramSetup::Charged {
                ready: 0,
                musd_per_t: 0.0
            };
            sc.programs.len()
        ]
    }

    #[test]
    fn earth_propellant_costs_launch_plus_the_rocket_equation() {
        let (graph, mut sc, edges) = short(4);
        sc.facilities.clear();
        let closed = vec![ProgramSetup::Closed; sc.programs.len()];
        let plan = Lp::new(&sc, &graph, &edges, false).solve(&sc, &closed);
        assert_eq!(plan.status, HighsModelStatus::Optimal);
        let price =
            |node: &str, p: usize| plan.price[&(graph.lookup(node).unwrap(), Commodity::Lox)][p];
        let at_leo = sc.launch_usd_per_kg[4] + sc.propellant_usd_per_kg;
        assert!(
            (price("leo", 4) - at_leo).abs() < 1e-3 * at_leo,
            "{}",
            price("leo", 4)
        );
        // One tonne at the L1 halo costs at least the propellant that pushed
        // it there with the better propellant (the ship's dry mass and
        // return add more), and not much more than with methalox.
        let (leo, l1) = (graph.lookup("leo").unwrap(), graph.lookup("l1").unwrap());
        let dv = graph
            .legs()
            .iter()
            .find(|l| (l.from, l.to) == (leo, l1))
            .expect("a leg from LEO to the L1 halo")
            .dv;
        let r = |p: Propellant| (dv / sc.propellant(p).ve).exp();
        let l1 = price("l1", 4);
        assert!(
            l1 > r(Propellant::Hydrolox) * at_leo && l1 < 1.5 * r(Propellant::Methalox) * at_leo,
            "{l1} vs {} and {}",
            r(Propellant::Hydrolox) * at_leo,
            r(Propellant::Methalox) * at_leo
        );
    }

    #[test]
    fn hydrogen_boils_off_in_storage() {
        let (graph, mut sc, edges) = short(4);
        sc.facilities.clear();
        let closed = vec![ProgramSetup::Closed; sc.programs.len()];
        // Launch gets 10× dearer halfway: worth stockpiling hydrogen, unless
        // it boils off.
        let half = sc.cal.periods / 2;
        for p in &mut sc.launch_usd_per_kg[half..] {
            *p *= 10.0;
        }
        let mut bought_late = |boiloff: f64| {
            sc.propellants[Propellant::Hydrolox as usize].fuel_boiloff_per_day = boiloff;
            let plan = Lp::new(&sc, &graph, &edges, false).solve(&sc, &closed);
            assert_eq!(plan.status, HighsModelStatus::Optimal);
            plan.bought[&Commodity::Lh2][half..].iter().sum::<f64>()
        };
        let kept = bought_late(0.0);
        let lost = bought_late(0.05);
        assert!(
            kept < 1e-3,
            "without boil-off, all hydrogen is bought early: {kept}"
        );
        assert!(lost > 1.0, "with boil-off, some is bought late: {lost}");
    }

    #[test]
    fn plants_produce_only_after_building_and_within_their_deposit() {
        let (graph, mut sc, edges) = short(6);
        let ice = richest_lunar_ice(&sc);
        sc.facilities[ice].reserve = Some(5000.0);
        let plan = Lp::new(&sc, &graph, &edges, false).solve(&sc, &free(&sc));
        assert_eq!(plan.status, HighsModelStatus::Optimal);
        for (f, (caps, installs)) in sc
            .facilities
            .iter()
            .zip(plan.capacity.iter().zip(&plan.installed))
        {
            let first_install = installs.iter().position(|&x| x > 1e-6);
            let first_capacity = caps.iter().position(|&c| c > 1e-6);
            if let Some(c) = first_capacity {
                assert!(c >= first_install.unwrap() + f.build_periods, "{}", f.id);
            }
        }
        let produced: f64 = plan.capacity[ice].iter().sum::<f64>() * sc.cal.period_years();
        assert!(
            produced > 1000.0,
            "the test needs lunar ice to be used: {produced}"
        );
        assert!(produced <= 5000.0 * (1.0 + 1e-6), "{produced}");
    }

    #[test]
    fn funded_programs_are_paid_in_full_before_their_plants() {
        let (graph, mut sc, edges) = short(8);
        let ice = richest_lunar_ice(&sc);
        let needs = sc.facilities[ice].programs.clone();
        assert_eq!(needs.len(), 2, "lunar ice needs a technology and a survey");
        for &j in &needs {
            sc.programs[j].lead_periods = 4;
        }
        let ready = 8;
        let setup: Vec<ProgramSetup> = (0..sc.programs.len())
            .map(|j| {
                if needs.contains(&j) {
                    ProgramSetup::Funded { ready }
                } else {
                    ProgramSetup::Closed
                }
            })
            .collect();
        let mut lp = Lp::new(&sc, &graph, &edges, false);
        let plan = lp.solve(&sc, &setup);
        assert_eq!(plan.status, HighsModelStatus::Optimal);
        for (k, installs) in plan.installed.iter().enumerate() {
            let first = installs.iter().position(|&x| x > 1e-6);
            let f = &sc.facilities[k];
            if k == ice {
                assert!(first.is_some_and(|p| p >= ready), "{first:?}");
            } else if f.programs.iter().all(|j| needs.contains(j)) {
                assert!(first.is_none_or(|p| p >= ready), "{} {first:?}", f.id);
            } else {
                assert_eq!(first, None, "{}", f.id);
            }
        }
        for (j, prog) in sc.programs.iter().enumerate() {
            let spent: f64 = plan.spend[j].iter().sum();
            if needs.contains(&j) {
                assert!(
                    (spent - prog.cost_musd).abs() < 1e-6 * prog.cost_musd,
                    "{spent}"
                );
                let rate = prog.cost_musd / prog.lead_periods as f64;
                assert!(plan.spend[j].iter().all(|&g| g <= rate * (1.0 + 1e-9)));
                assert!(plan.spend[j][ready..].iter().all(|&g| g.abs() < 1e-9));
            } else {
                assert!(spent.abs() < 1e-9, "{}", prog.id);
            }
        }
        assert!(sc.programs[needs[1]].kind == ProgramKind::Survey);

        // Re-solving the same LP with another setup agrees with a fresh LP.
        let free_setup = free(&sc);
        let again = lp.solve(&sc, &free_setup);
        let fresh = Lp::new(&sc, &graph, &edges, false).solve(&sc, &free_setup);
        assert!(
            (again.objective - fresh.objective).abs() < 1e-6 * fresh.objective.abs(),
            "{} vs {}",
            again.objective,
            fresh.objective
        );
    }
}
