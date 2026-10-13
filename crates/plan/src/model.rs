//! The linear program: one plan for the whole scenario, with perfect
//! foresight. See M2.md for the formulation.

use crate::edges::{Edge, EdgeKind};
use crate::params::{Demanded, Scenario};
use highs::{Col, HighsModelStatus, RowProblem, Sense};
use orbit::network::{Network, NodeId};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Commodity {
    Lox,
    Ch4,
    Equipment,
    Payload,
    Metal,
}

pub const COMMODITIES: [Commodity; 5] = [
    Commodity::Lox,
    Commodity::Ch4,
    Commodity::Equipment,
    Commodity::Payload,
    Commodity::Metal,
];

impl Commodity {
    fn index(self) -> usize {
        self as usize
    }

    fn is_propellant(self) -> bool {
        matches!(self, Commodity::Lox | Commodity::Ch4)
    }
}

/// $/kg × t → $M.
const MUSD_PER_USD_T: f64 = 1e-3;

/// Deposits larger than this are treated as unlimited.
const RESERVE_UNLIMITED_T: f64 = 1e7;

/// A solved plan, by period.
#[derive(Debug)]
pub struct Plan {
    pub status: HighsModelStatus,
    /// Discounted net value, $M.
    pub objective: f64,
    /// Capacity of each facility option (t/yr of rated output).
    pub capacity: Vec<Vec<f64>>,
    /// Equipment installed at each facility option, t.
    pub installed: Vec<Vec<f64>>,
    /// Tonnes bought at LEO, by commodity.
    pub bought: HashMap<Commodity, Vec<f64>>,
    /// Payload delivered, by demand category.
    pub served: Vec<Vec<f64>>,
    /// Metals sold, t.
    pub metal_sold: Vec<f64>,
    /// Ships bought, by class.
    pub ships_bought: Vec<Vec<f64>>,
    /// Cargo moved from node to node, by commodity, t per period of
    /// departure.
    pub shipped: HashMap<(NodeId, NodeId, Commodity), Vec<f64>>,
    /// Capital spending (equipment and ships), $M per period.
    pub capex: Vec<f64>,
    /// Marginal value of one more tonne at a node, by commodity, $/kg in the
    /// period's own money (undiscounted).
    pub price: HashMap<(NodeId, Commodity), Vec<f64>>,
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
    edge: usize,
    cargo: Vec<(Commodity, Col)>,
}

pub fn solve(sc: &Scenario, net: &Network, edges: &[Edge], verbose: bool) -> Plan {
    let t_n = sc.cal.periods;
    let n_nodes = net.nodes().count();
    let leo = net.lookup("leo").expect("LEO is a node");
    let earth_system = net.node(leo).system;
    let disc: Vec<f64> = (0..t_n)
        .map(|p| (1.0 + sc.discount_rate).powf(-sc.cal.years_at(p)))
        .collect();
    let dt = sc.cal.period_years();

    let mut pb = RowProblem::new();
    // balance[node][commodity][period]: inflow − outflow − Δstock = 0.
    let mut balance: Vec<Vec<Vec<Terms>>> = (0..n_nodes)
        .map(|_| {
            (0..COMMODITIES.len())
                .map(|_| (0..t_n).map(|_| Terms::default()).collect())
                .collect()
        })
        .collect();
    let node_index: HashMap<NodeId, usize> = net
        .nodes()
        .enumerate()
        .map(|(k, (id, _))| (id, k))
        .collect();
    macro_rules! bal {
        ($n:expr, $c:expr, $p:expr) => {
            balance[node_index[&$n]][$c.index()][$p]
        };
    }

    // Stocks.
    for node_rows in balance.iter_mut() {
        for c_rows in node_rows.iter_mut() {
            let mut prev: Option<Col> = None;
            for row in c_rows.iter_mut() {
                let s = pb.add_column(0.0, 0.0..);
                row.add(s, -1.0);
                if let Some(prev) = prev {
                    row.add(prev, 1.0);
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
    let mut metal_cols: Vec<(usize, Col, f64)> = Vec::new();
    for &(tonnes, usd_per_kg) in &sc.metal_steps {
        let usd = usd_per_kg * MUSD_PER_USD_T;
        let mut step = Terms::default();
        for p in 0..t_n {
            let s = pb.add_column(disc[p] * usd, 0.0..);
            bal!(leo, Commodity::Metal, p).add(s, -1.0);
            capex_terms[p].add(s, -usd);
            step.add(s, 1.0);
            metal_cols.push((p, s, usd));
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
                match d.what {
                    Demanded::Payload => bal!(n, Commodity::Payload, p).add(x, -1.0),
                    Demanded::Propellant => {
                        bal!(n, Commodity::Lox, p).add(x, -sc.lox_share);
                        bal!(n, Commodity::Ch4, p).add(x, sc.lox_share - 1.0);
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

    // Facilities.
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

    // Fleet: ships at each node at the end of each period.
    let mut systems = Vec::new();
    for (_, n) in net.nodes() {
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
        for _ in net.nodes() {
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
    // turnaround) ≤ period days × ships in the system at the period's start
    // (including ships bought in the period).
    let mut ship_days: Vec<HashMap<(usize, usize), Terms>> =
        sc.ships.iter().map(|_| HashMap::new()).collect();
    let sys_index = |n: NodeId| {
        systems
            .iter()
            .position(|&s| s == net.node(n).system)
            .unwrap()
    };

    // Edges.
    let can_fly = |ship: &crate::params::ShipClass, e: &Edge| -> bool {
        match e.kind {
            EdgeKind::Interplanetary => ship.interplanetary,
            EdgeKind::Local { aero } => {
                (!aero || ship.aero)
                    && [e.from, e.to]
                        .iter()
                        .all(|n| !sc.surfaces.contains(n) || ship.lands.contains(n))
            }
        }
    };
    let mut edge_vars: Vec<EdgeVars> = Vec::new();
    for (i, e) in edges.iter().enumerate() {
        let inbound =
            net.node(e.from).system != earth_system && net.node(e.to).system == earth_system;
        let outbound =
            net.node(e.from).system == earth_system && net.node(e.to).system != earth_system;
        let r = (e.dv / sc.ve).exp() - 1.0;
        for (k, ship) in sc.ships.iter().enumerate() {
            if !can_fly(ship, e) {
                continue;
            }
            if !ship.interplanetary && net.node(e.from).system != earth_system {
                continue;
            }
            if r * ship.dry >= ship.tank {
                continue;
            }
            let trips = pb.add_column(0.0, 0.0..);
            let mut cargo = Vec::new();
            for c in COMMODITIES {
                let allowed = match c {
                    Commodity::Lox | Commodity::Ch4 => true,
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
                if c.is_propellant() {
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
            let burn = |b: &mut Terms, share: f64, x: Col, mass: f64| b.add(x, -share * r * mass);
            for (c, share) in [
                (Commodity::Lox, sc.lox_share),
                (Commodity::Ch4, 1.0 - sc.lox_share),
            ] {
                burn(&mut bal!(e.from, c, e.leave), share, trips, ship.dry);
                for &(_, x) in &cargo {
                    burn(&mut bal!(e.from, c, e.leave), share, x, 1.0);
                }
            }
            for &(c, x) in &cargo {
                bal!(e.from, c, e.leave).add(x, -1.0);
                bal!(e.to, c, e.arrive).add(x, 1.0);
            }
            // Ships.
            fleet_bal[k][node_index[&e.from]][e.leave].add(trips, -1.0);
            fleet_bal[k][node_index[&e.to]][e.arrive].add(trips, 1.0);
            if let EdgeKind::Local { .. } = e.kind {
                ship_days[k]
                    .entry((sys_index(e.from), e.leave))
                    .or_default()
                    .add(trips, e.days + ship.turnaround_days);
            }
            edge_vars.push(EdgeVars { edge: i, cargo });
        }
    }
    for (k, rows) in ship_days.iter_mut().enumerate() {
        for (&(s, p), terms) in rows.iter_mut() {
            if p > 0 {
                for (n, _) in net.nodes().filter(|&(n, _)| sys_index(n) == s) {
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

    // Budget: cumulative capital spending ≤ cumulative appropriation, at the
    // end of every year.
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
    // Interior point with crossover: ~30 s on the quarterly LP, where dual
    // simplex had not finished after 5 minutes.
    model.set_option("solver", "ipm");
    let solved = model.solve();
    let status = solved.status();
    let sol = solved.get_solution();
    let v = sol.columns();
    let val = |c: Col| v[c.index()];
    // A balance row's dual is the discounted value of one tonne less there.
    let duals = sol.dual_rows();
    let mut price = HashMap::new();
    for (n, _) in net.nodes() {
        for c in COMMODITIES {
            let row = first_balance_row + (node_index[&n] * COMMODITIES.len() + c.index()) * t_n;
            let usd_per_kg = (0..t_n)
                .map(|p| -duals[row + p] / disc[p] / MUSD_PER_USD_T)
                .collect();
            price.insert((n, c), usd_per_kg);
        }
    }

    let mut bought: HashMap<Commodity, Vec<f64>> = HashMap::new();
    for &(c, p, col) in &buy_cols {
        bought.entry(c).or_insert_with(|| vec![0.0; t_n])[p] += val(col);
    }
    let mut metal_sold = vec![0.0; t_n];
    let mut capex = vec![0.0; t_n];
    for &(p, col, _) in &metal_cols {
        metal_sold[p] += val(col);
    }
    for p in 0..t_n {
        capex[p] = capex_terms[p]
            .0
            .values()
            .filter(|(_, coef)| *coef > 0.0)
            .map(|&(c, coef)| coef * val(c))
            .sum();
    }
    let mut shipped: HashMap<(NodeId, NodeId, Commodity), Vec<f64>> = HashMap::new();
    for ev in &edge_vars {
        let e = &edges[ev.edge];
        for &(c, x) in &ev.cargo {
            shipped
                .entry((e.from, e.to, c))
                .or_insert_with(|| vec![0.0; t_n])[e.leave] += val(x);
        }
    }

    Plan {
        status,
        objective: solved.objective_value(),
        capacity: cap_cols
            .iter()
            .map(|cs| cs.iter().map(|&c| val(c)).collect())
            .collect(),
        installed: install_cols
            .iter()
            .map(|cs| cs.iter().map(|&c| val(c)).collect())
            .collect(),
        bought,
        served: served_cols
            .iter()
            .map(|by_p| {
                by_p.iter()
                    .map(|xs| xs.iter().map(|&x| val(x)).sum())
                    .collect()
            })
            .collect(),
        metal_sold,
        ships_bought: ship_buy
            .iter()
            .map(|bs| bs.iter().map(|&b| val(b)).collect())
            .collect(),
        shipped,
        capex,
        price,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edges;
    use crate::params::{Draw, EconomyFile};
    use orbit::ephem::Ephemeris;

    /// The central draw, cut to its first `years` (quarterly), and its
    /// transport network.
    fn short(years: usize) -> (Network, Scenario, Vec<Edge>) {
        let eph = Ephemeris::standard();
        let net = Network::standard(&eph);
        let mut sc = EconomyFile::standard().scenario(&eph, &net, &mut Draw::Central, 3);
        sc.cal.periods = years * 4;
        sc.launch_usd_per_kg.truncate(sc.cal.periods);
        let mut all = edges::local_edges(&net, &sc.cal);
        all.extend(edges::interplanetary_edges(
            &eph,
            &net,
            &sc.cal,
            &edges::routes(&eph, &net),
            6.0,
        ));
        (net, sc, all)
    }

    #[test]
    fn earth_propellant_costs_launch_plus_the_rocket_equation() {
        let (net, mut sc, edges) = short(4);
        sc.facilities.clear();
        let plan = solve(&sc, &net, &edges, false);
        assert_eq!(plan.status, HighsModelStatus::Optimal);
        let price =
            |node: &str, p: usize| plan.price[&(net.lookup(node).unwrap(), Commodity::Lox)][p];
        let at_leo = sc.launch_usd_per_kg[4] + sc.propellant_usd_per_kg;
        assert!(
            (price("leo", 4) - at_leo).abs() < 1e-3 * at_leo,
            "{}",
            price("leo", 4)
        );
        // One tonne at EML1 costs at least the propellant that pushed it
        // there (the ship's dry mass and return add more).
        let r = (3.77 / sc.ve).exp();
        let l1 = price("eml1", 4);
        assert!(
            l1 > r * at_leo && l1 < 1.5 * r * at_leo,
            "{l1} vs {}",
            r * at_leo
        );
    }

    #[test]
    fn plants_produce_only_after_building_and_within_their_deposit() {
        let (net, mut sc, edges) = short(6);
        let ice = sc
            .facilities
            .iter()
            .position(|f| f.id == "lunar-ice")
            .unwrap();
        sc.facilities[ice].reserve = Some(5000.0);
        let plan = solve(&sc, &net, &edges, false);
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
}
