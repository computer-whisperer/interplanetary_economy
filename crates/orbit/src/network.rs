//! The Δv network: nodes (places a ship can be), fixed legs between nodes in
//! the same planetary system, and gateways that turn a heliocentric
//! hyperbolic excess speed into a burn at a node. Interplanetary legs are
//! computed on demand by [`crate::transfer`].

use crate::consts::{MU_EARTH, MU_MARS};
use crate::ephem::{BodyId, BodyKind, Ephemeris};
use serde::Deserialize;

/// Δv to raise periapsis out of the atmosphere after an aerocapture pass.
pub const AEROCAPTURE_TRIM_DV: f64 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(usize);

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub name: String,
    /// The heliocentric body whose system the node belongs to.
    pub system: BodyId,
    /// How the node connects to heliocentric space, if it does directly.
    pub gateway: Option<Gateway>,
}

#[derive(Clone, Copy, Debug)]
pub enum Gateway {
    /// An orbit about a planet; see [`OrbitGateway`].
    Orbit(OrbitGateway),
    /// A small body: matching its velocity costs exactly v∞, either way.
    Rendezvous,
    /// A surface reached by direct atmospheric entry, arrival only.
    Entry { dv: f64 },
}

/// An orbit about a planet. Burns to or from v∞ fire at `periapsis` for the
/// Oberth effect; a node above that periapsis first transfers down to it
/// (or, if cheaper, burns where it is).
#[derive(Clone, Copy, Debug)]
pub struct OrbitGateway {
    pub mu: f64,
    pub radius: f64,
    pub speed: f64,
    pub periapsis: f64,
    pub aerocapture: bool,
}

impl OrbitGateway {
    /// Δv between this node and hyperbolic excess speed `v_inf`. Two-body
    /// burns are time-symmetric, so departure and propulsive capture cost the
    /// same.
    pub fn burn_dv(&self, v_inf: f64) -> f64 {
        let at = |r: f64, v: f64| (v_inf * v_inf + 2.0 * self.mu / r).sqrt() - v;
        let direct = at(self.radius, self.speed);
        match self.periapsis_transfer() {
            Some((drop, v_peri)) => direct.min(drop + at(self.periapsis, v_peri)),
            None => direct,
        }
    }

    /// Δv to arrive by aerocapture: an atmospheric pass leaves the ship on an
    /// ellipse whose apoapsis is the node; it raises periapsis and matches the
    /// node's speed there.
    pub fn aerocapture_dv(&self) -> f64 {
        AEROCAPTURE_TRIM_DV + self.periapsis_transfer().map_or(0.0, |(drop, _)| drop)
    }

    /// For a node above its burn periapsis: the Δv between the node and an
    /// ellipse from the node down to periapsis, and the speed at periapsis.
    pub fn periapsis_transfer(&self) -> Option<(f64, f64)> {
        let (ra, rp) = (self.radius, self.periapsis);
        if rp >= ra {
            return None;
        }
        let v_apo = (2.0 * self.mu * rp / (ra * (ra + rp))).sqrt();
        let v_peri = (2.0 * self.mu * ra / (rp * (ra + rp))).sqrt();
        Some(((self.speed - v_apo).abs(), v_peri))
    }
}

impl Gateway {
    /// Δv to leave the node with excess speed `v_inf`, if departures are
    /// possible from it.
    pub fn departure_dv(&self, v_inf: f64) -> Option<f64> {
        match self {
            Gateway::Orbit(o) => Some(o.burn_dv(v_inf)),
            Gateway::Rendezvous => Some(v_inf),
            Gateway::Entry { .. } => None,
        }
    }

    /// Cheapest Δv to arrive at the node with excess speed `v_inf`, for a ship
    /// that can (`aero_capable`) or cannot brake in an atmosphere.
    pub fn arrival_dv(&self, v_inf: f64, aero_capable: bool) -> Option<f64> {
        match self {
            Gateway::Orbit(o) => {
                let propulsive = o.burn_dv(v_inf);
                Some(if aero_capable && o.aerocapture { propulsive.min(o.aerocapture_dv()) } else { propulsive })
            }
            Gateway::Rendezvous => Some(v_inf),
            Gateway::Entry { dv } => aero_capable.then_some(*dv),
        }
    }
}

/// A fixed leg between two nodes of one planetary system.
#[derive(Clone, Copy, Debug)]
pub struct Leg {
    pub from: NodeId,
    pub to: NodeId,
    pub dv: f64,
    pub days: f64,
    /// Brakes in an atmosphere; needs an aero-capable ship.
    pub aero: bool,
    /// A purchased launch service rather than a flown leg.
    pub launch: bool,
}

/// A chain of fixed legs.
#[derive(Clone, Debug)]
pub struct LocalPath {
    pub nodes: Vec<NodeId>,
    pub dv: f64,
    pub days: f64,
}

#[derive(Clone, Debug)]
pub struct Network {
    nodes: Vec<Node>,
    legs: Vec<Leg>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NetworkFile {
    node: Vec<NodeRow>,
    leg: Vec<LegRow>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeRow {
    id: String,
    name: String,
    system: String,
    orbit: Option<OrbitRow>,
    #[serde(default)]
    aerocapture: bool,
    entry_dv: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrbitRow {
    radius_km: f64,
    speed_kms: Option<f64>,
    periapsis_radius_km: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegRow {
    from: String,
    to: String,
    dv: f64,
    days: f64,
    #[serde(default)]
    aero: bool,
    #[serde(default)]
    launch: bool,
}

/// Gravitational parameter of a planet that can host orbit gateways.
fn planet_mu(system: &str) -> Option<f64> {
    match system {
        "earth" => Some(MU_EARTH),
        "mars" => Some(MU_MARS),
        _ => None,
    }
}

impl Network {
    /// `data/network.toml`, plus a rendezvous node for every small body.
    pub fn standard(eph: &Ephemeris) -> Self {
        Self::from_toml(include_str!("../../../data/network.toml"), eph).expect("data/network.toml is valid")
    }

    pub fn from_toml(src: &str, eph: &Ephemeris) -> Result<Self, String> {
        let file: NetworkFile = toml::from_str(src).map_err(|e| e.to_string())?;
        let mut nodes = Vec::new();
        for row in file.node {
            let system = eph.lookup(&row.system).ok_or_else(|| format!("{}: unknown system {:?}", row.id, row.system))?;
            let gateway = match (row.orbit, row.entry_dv) {
                (Some(_), Some(_)) => return Err(format!("{}: both orbit and entry_dv", row.id)),
                (Some(o), None) => {
                    let mu = planet_mu(&row.system)
                        .ok_or_else(|| format!("{}: no gravitational parameter for {:?}", row.id, row.system))?;
                    Some(Gateway::Orbit(OrbitGateway {
                        mu,
                        radius: o.radius_km,
                        speed: o.speed_kms.unwrap_or_else(|| (mu / o.radius_km).sqrt()),
                        periapsis: o.periapsis_radius_km.unwrap_or(o.radius_km),
                        aerocapture: row.aerocapture,
                    }))
                }
                (None, Some(dv)) => Some(Gateway::Entry { dv }),
                (None, None) => None,
            };
            if row.aerocapture && !matches!(gateway, Some(Gateway::Orbit(_))) {
                return Err(format!("{}: aerocapture needs an orbit gateway", row.id));
            }
            nodes.push(Node { id: row.id, name: row.name, system, gateway });
        }
        for id in eph.ids() {
            let body = eph.body(id);
            if matches!(body.kind, BodyKind::SmallBody { .. }) {
                nodes.push(Node {
                    id: body.id.clone(),
                    name: body.name.clone(),
                    system: id,
                    gateway: Some(Gateway::Rendezvous),
                });
            }
        }
        for (k, n) in nodes.iter().enumerate() {
            if nodes[..k].iter().any(|m| m.id == n.id) {
                return Err(format!("duplicate node id {:?}", n.id));
            }
        }

        let mut net = Network { nodes, legs: Vec::new() };
        for row in file.leg {
            let find = |id: &str| net.lookup(id).ok_or_else(|| format!("leg references unknown node {id:?}"));
            let (from, to) = (find(&row.from)?, find(&row.to)?);
            if net.node(from).system != net.node(to).system {
                return Err(format!("leg {} -> {} crosses systems", row.from, row.to));
            }
            net.legs.push(Leg { from, to, dv: row.dv, days: row.days, aero: row.aero, launch: row.launch });
        }
        Ok(net)
    }

    pub fn lookup(&self, id: &str) -> Option<NodeId> {
        self.nodes.iter().position(|n| n.id == id).map(NodeId)
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0]
    }

    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter().enumerate().map(|(k, n)| (NodeId(k), n))
    }

    pub fn legs(&self) -> &[Leg] {
        &self.legs
    }

    /// The lowest-Δv chain of fixed legs from `from` to `to`, excluding launch
    /// services, and aero legs unless `aero_capable`.
    pub fn cheapest_local_path(&self, from: NodeId, to: NodeId, aero_capable: bool) -> Option<LocalPath> {
        // Dijkstra over a graph of a few dozen nodes; a linear scan is plenty.
        let n = self.nodes.len();
        let mut best: Vec<Option<(f64, f64, Option<NodeId>)>> = vec![None; n];
        let mut done = vec![false; n];
        best[from.0] = Some((0.0, 0.0, None));
        while let Some(u) = (0..n).filter(|&k| !done[k] && best[k].is_some()).min_by(|&a, &b| {
            best[a].unwrap().0.total_cmp(&best[b].unwrap().0)
        }) {
            done[u] = true;
            if u == to.0 {
                break;
            }
            let (dv_u, days_u, _) = best[u].unwrap();
            for leg in self.legs.iter().filter(|l| l.from.0 == u && !l.launch && (aero_capable || !l.aero)) {
                let dv = dv_u + leg.dv;
                if best[leg.to.0].is_none_or(|(d, _, _)| dv < d) {
                    best[leg.to.0] = Some((dv, days_u + leg.days, Some(NodeId(u))));
                }
            }
        }
        let (dv, days, _) = best[to.0]?;
        let mut nodes = vec![to];
        while let Some((_, _, Some(prev))) = best[nodes.last().unwrap().0] {
            nodes.push(prev);
        }
        nodes.reverse();
        Some(LocalPath { nodes, dv, days })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standard() -> (Ephemeris, Network) {
        let eph = Ephemeris::standard();
        let net = Network::standard(&eph);
        (eph, net)
    }

    fn orbit(net: &Network, id: &str) -> OrbitGateway {
        match net.node(net.lookup(id).unwrap()).gateway {
            Some(Gateway::Orbit(o)) => o,
            other => panic!("{id}: {other:?}"),
        }
    }

    fn leg_dv(net: &Network, from: &str, to: &str) -> f64 {
        let (f, t) = (net.lookup(from).unwrap(), net.lookup(to).unwrap());
        net.legs().iter().find(|l| l.from == f && l.to == t && !l.aero).unwrap().dv
    }

    /// The tabulated Δv values agree with two-impulse Hohmann estimates
    /// computed from the gateway geometry.
    #[test]
    fn table_matches_hohmann_estimates() {
        let (_, net) = standard();
        let hohmann = |low: OrbitGateway, high: OrbitGateway| {
            let (drop, v_peri) = high.periapsis_transfer().unwrap();
            (v_peri - low.speed) + drop
        };
        let leo_eml1 = hohmann(orbit(&net, "leo"), orbit(&net, "eml1"));
        assert!((leo_eml1 - leg_dv(&net, "leo", "eml1")).abs() < 0.1, "{leo_eml1}");
        let lmo_phobos = hohmann(orbit(&net, "lmo"), orbit(&net, "phobos"));
        assert!((lmo_phobos - leg_dv(&net, "lmo", "phobos")).abs() < 0.02, "{lmo_phobos}");
    }

    #[test]
    fn departure_burns() {
        let (_, net) = standard();
        // Trans-Mars injection from LEO at C3 = 9 km²/s²: ~3.6 km/s.
        let leo = orbit(&net, "leo").burn_dv(3.0);
        assert!((leo - 3.6).abs() < 0.05, "{leo}");
        // From EML1 the perigee drop plus Oberth burn costs about a third.
        let eml1 = orbit(&net, "eml1").burn_dv(3.0);
        assert!((1.0..1.3).contains(&eml1), "{eml1}");
        // Mars orbit insertion at v∞ = 2.65 km/s into a 300 km orbit: ~2.1.
        let lmo = orbit(&net, "lmo").burn_dv(2.65);
        assert!((lmo - 2.09).abs() < 0.03, "{lmo}");
    }

    #[test]
    fn cheapest_paths() {
        let (_, net) = standard();
        let id = |s: &str| net.lookup(s).unwrap();
        let out = net.cheapest_local_path(id("leo"), id("moon-south-pole"), false).unwrap();
        assert_eq!(out.nodes, vec![id("leo"), id("llo"), id("moon-south-pole")]);
        assert!((out.dv - 5.91).abs() < 1e-9);
        let back = net.cheapest_local_path(id("moon-south-pole"), id("leo"), true).unwrap();
        assert!((back.dv - 2.77).abs() < 1e-9, "{}", back.dv);
        let back_no_aero = net.cheapest_local_path(id("moon-south-pole"), id("leo"), false).unwrap();
        assert!((back_no_aero.dv - 5.91).abs() < 1e-9, "{}", back_no_aero.dv);
        // The launch service is not a flown leg.
        assert!(net.cheapest_local_path(id("earth-surface"), id("leo"), true).is_none());
        // Systems are not connected by fixed legs.
        assert!(net.cheapest_local_path(id("leo"), id("lmo"), true).is_none());
    }

    #[test]
    fn small_bodies_are_rendezvous_nodes() {
        let (_, net) = standard();
        let ryugu = net.node(net.lookup("ryugu").unwrap());
        assert!(matches!(ryugu.gateway, Some(Gateway::Rendezvous)));
    }
}
