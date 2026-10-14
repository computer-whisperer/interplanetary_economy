//! Small scenarios for tests.

use crate::edges::{self, Edge};
use crate::params::{Draw, EconomyFile, Scenario};
use orbit::ephem::Ephemeris;
use orbit::routes::RouteGraph;

/// The central draw, cut to its first `years` (quarterly), and its
/// transport network.
pub fn short(years: usize) -> (RouteGraph, Scenario, Vec<Edge>) {
    let eph = Ephemeris::standard();
    let graph = edges::standard(eph);
    let mut sc = EconomyFile::standard().scenario(eph, &graph, &mut Draw::Central, 3);
    sc.cal.periods = years * 4;
    sc.launch_usd_per_kg.truncate(sc.cal.periods);
    let mut all = edges::local_edges(&graph, &sc.cal);
    all.extend(edges::interplanetary_edges(
        eph,
        &graph,
        &sc.cal,
        &edges::pairs(eph, &graph),
        6.0,
    ));
    (graph, sc, all)
}

/// The richest lunar ice deposit's plant option.
pub fn richest_lunar_ice(sc: &Scenario) -> usize {
    (0..sc.facilities.len())
        .filter(|&k| sc.facilities[k].id.starts_with("lunar-ice"))
        .max_by(|&a, &b| sc.facilities[a].grade.total_cmp(&sc.facilities[b].grade))
        .expect("the scenario has lunar ice")
}
