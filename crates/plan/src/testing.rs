//! Small scenarios for tests.

use crate::edges::{self, Edge};
use crate::params::{Draw, EconomyFile, Scenario};
use orbit::ephem::Ephemeris;
use orbit::network::Network;

/// The central draw, cut to its first `years` (quarterly), and its
/// transport network.
pub fn short(years: usize) -> (Network, Scenario, Vec<Edge>) {
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

/// The richest lunar ice deposit's plant option.
pub fn richest_lunar_ice(sc: &Scenario) -> usize {
    (0..sc.facilities.len())
        .filter(|&k| sc.facilities[k].id.starts_with("lunar-ice"))
        .max_by(|&a, &b| sc.facilities[a].grade.total_cmp(&sc.facilities[b].grade))
        .expect("the scenario has lunar ice")
}
