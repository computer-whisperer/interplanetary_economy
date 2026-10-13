//! Which development path a plan follows.

use crate::model::{Commodity, Plan};
use crate::params::Scenario;
use orbit::ephem::Ephemeris;
use orbit::network::Network;
use std::fmt;

/// Where propellant comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Earth,
    Moon,
    Mars,
    Asteroids,
}

pub const SOURCES: [Source; 4] = [Source::Earth, Source::Moon, Source::Mars, Source::Asteroids];

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Source::Earth => "Earth",
            Source::Moon => "Moon",
            Source::Mars => "Mars",
            Source::Asteroids => "Asteroids",
        })
    }
}

/// A source counts as part of a path when it supplies at least this share
/// of the in-space propellant.
pub const PATH_SHARE: f64 = 0.2;

/// In-space production below this share of all propellant is Earth-only.
pub const EARTH_ONLY_SHARE: f64 = 0.25;

#[derive(Clone, Debug)]
pub struct Classification {
    /// Propellant supplied over the classification years, t, by source.
    pub supply: [f64; 4],
    /// In-space sources supplying ≥ PATH_SHARE of in-space propellant,
    /// largest first; empty for an Earth-only plan.
    pub path: Vec<Source>,
    /// Equipment installed over the opening years, t, by source.
    pub opening_equipment: [f64; 4],
    /// In-space sources receiving ≥ PATH_SHARE of the equipment installed
    /// in the opening years, largest first.
    pub opening: Vec<Source>,
}

impl Classification {
    pub fn path_label(&self) -> String {
        if self.path.is_empty() {
            return "Earth-only".into();
        }
        let names: Vec<String> = self.path.iter().map(|s| s.to_string()).collect();
        names.join("+")
    }

    pub fn opening_label(&self) -> String {
        if self.opening.is_empty() {
            return "none".into();
        }
        let names: Vec<String> = self.opening.iter().map(|s| s.to_string()).collect();
        names.join("+")
    }
}

/// The in-space source a facility's site belongs to.
pub fn source_of(eph: &Ephemeris, net: &Network, sc: &Scenario, facility: usize) -> Source {
    let system = eph.body(net.node(sc.facilities[facility].node).system);
    match system.id.as_str() {
        "earth" => Source::Moon,
        "mars" => Source::Mars,
        _ => Source::Asteroids,
    }
}

/// Classifies `plan` by its propellant supply over the first
/// `path_years`, and the equipment it installs over the first
/// `opening_years`.
pub fn classify(
    eph: &Ephemeris,
    net: &Network,
    sc: &Scenario,
    plan: &Plan,
    path_years: f64,
    opening_years: f64,
) -> Classification {
    let periods =
        |years: f64| ((years / sc.cal.period_years()).round() as usize).min(sc.cal.periods);
    let (path_end, opening_end) = (periods(path_years), periods(opening_years));
    let dt = sc.cal.period_years();

    let mut supply = [0.0; 4];
    supply[Source::Earth as usize] = [Commodity::Lox, Commodity::Ch4]
        .iter()
        .map(|c| plan.bought[c][..path_end].iter().sum::<f64>())
        .sum();
    let mut opening_equipment = [0.0; 4];
    for (k, f) in sc.facilities.iter().enumerate() {
        let src = source_of(eph, net, sc, k) as usize;
        let propellant = f.output.lox + f.output.ch4;
        supply[src] += plan.capacity[k][..path_end].iter().sum::<f64>() * dt * propellant;
        opening_equipment[src] += plan.installed[k][..opening_end].iter().sum::<f64>();
    }
    let in_space: f64 = supply[1..].iter().sum();
    let total = in_space + supply[0];
    let path = if total > 0.0 && in_space / total >= EARTH_ONLY_SHARE {
        leading(&supply)
    } else {
        Vec::new()
    };
    let opening = leading(&opening_equipment);
    Classification {
        supply,
        opening_equipment,
        path,
        opening,
    }
}

/// The in-space sources with ≥ PATH_SHARE of the in-space total, largest
/// first.
fn leading(by_source: &[f64; 4]) -> Vec<Source> {
    let in_space: f64 = by_source[1..].iter().sum();
    if in_space <= 0.0 {
        return Vec::new();
    }
    let mut v: Vec<Source> = SOURCES[1..]
        .iter()
        .copied()
        .filter(|&s| by_source[s as usize] / in_space >= PATH_SHARE)
        .collect();
    v.sort_by(|a, b| by_source[*b as usize].total_cmp(&by_source[*a as usize]));
    v
}
