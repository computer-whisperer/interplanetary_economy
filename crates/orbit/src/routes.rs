//! The route graph (M1a.md, step 11): the places ships can be, the legs
//! between places in one planetary system, priced by the autopilot's sweep
//! (`orbit gate`), and each place's gateway to the legs between systems,
//! which patched conics price ([`Gateway`]). `orbit routes` writes it to
//! `data/routes.toml`; M2's planner reads it.

use crate::ephem::{BodyId, Ephemeris};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlaceId(usize);

#[derive(Clone, Debug)]
pub struct Place {
    pub id: String,
    pub name: String,
    /// The planetary system: its planet, or a small body alone.
    pub system: BodyId,
    /// How ships leave for and arrive from other systems; none at places
    /// they pass through another to reach.
    pub gateway: Option<Gateway>,
}

/// A leg between two places in one system, at its median cost.
#[derive(Clone, Debug)]
pub struct Leg {
    pub from: PlaceId,
    pub to: PlaceId,
    /// km/s.
    pub dv: f64,
    pub days: f64,
    /// Through an atmosphere: aero-capable ships only.
    pub aero: bool,
    /// Where the numbers come from.
    pub source: String,
}

/// A place's departures to and arrivals from other systems, as functions of
/// the heliocentric arc's excess speed there.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Gateway {
    /// Leaving.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depart: Option<Escape>,
    /// Arriving by burning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arrive: Option<Escape>,
    /// Arriving through the atmosphere instead (aero-capable ships).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pass: Option<Pass>,
    /// Days between the place and the heliocentric arc, each way.
    pub days: f64,
}

/// What it takes to reach a hyperbola's excess speed v∞ from a place (or
/// to stop from one there; the same).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Escape {
    /// Reaching a periapsis at `radius` (km) with `speed` (km/s) costs
    /// `extra` (km/s), then a burn there along the velocity: Δv = extra +
    /// √(v∞² + 2μ/radius) − speed. From a low orbit, no extra and the
    /// circular speed.
    Periapsis { extra: f64, radius: f64, speed: f64 },
    /// Matching a small body's velocity: Δv = v∞.
    Rendezvous,
}

/// An aerocapture or entry: a pass at periapsis `radius` (km), survived up
/// to `max_speed` (km/s) there, then `after` (km/s) of burns.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Pass {
    pub radius: f64,
    pub max_speed: f64,
    pub after: f64,
}

impl Escape {
    /// km/s, about a body of `gm`.
    pub fn dv(&self, gm: f64, v_inf: f64) -> f64 {
        match *self {
            Escape::Periapsis {
                extra,
                radius,
                speed,
            } => extra + (v_inf * v_inf + 2.0 * gm / radius).sqrt() - speed,
            Escape::Rendezvous => v_inf,
        }
    }
}

impl Pass {
    /// km/s, or `None` if the pass is too fast.
    pub fn dv(&self, gm: f64, v_inf: f64) -> Option<f64> {
        ((v_inf * v_inf + 2.0 * gm / self.radius).sqrt() <= self.max_speed).then_some(self.after)
    }
}

impl Gateway {
    /// Leaving at excess speed `v_inf`, km/s.
    pub fn departure(&self, gm: f64, v_inf: f64) -> Option<f64> {
        self.depart.map(|e| e.dv(gm, v_inf))
    }

    /// Arriving at excess speed `v_inf` by burning, km/s.
    pub fn arrival(&self, gm: f64, v_inf: f64) -> Option<f64> {
        self.arrive.map(|e| e.dv(gm, v_inf))
    }

    /// Arriving at excess speed `v_inf` through the atmosphere, km/s.
    pub fn aerocapture(&self, gm: f64, v_inf: f64) -> Option<f64> {
        self.pass.and_then(|p| p.dv(gm, v_inf))
    }
}

/// Places and legs.
#[derive(Clone, Debug)]
pub struct RouteGraph {
    /// Where the graph's numbers come from, as a whole.
    pub source: String,
    places: Vec<Place>,
    legs: Vec<Leg>,
    index: HashMap<String, PlaceId>,
}

/// `data/routes.toml`'s shape: places and legs by id.
#[derive(Serialize, Deserialize)]
struct File {
    source: String,
    place: Vec<PlaceEntry>,
    leg: Vec<LegEntry>,
}

#[derive(Serialize, Deserialize)]
struct PlaceEntry {
    id: String,
    name: String,
    /// The system's body, by ephemeris id.
    system: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gateway: Option<Gateway>,
}

#[derive(Serialize, Deserialize)]
struct LegEntry {
    from: String,
    to: String,
    dv: f64,
    days: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    aero: bool,
    source: String,
}

impl RouteGraph {
    pub fn new(source: String) -> RouteGraph {
        RouteGraph {
            source,
            places: Vec::new(),
            legs: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn add_place(&mut self, place: Place) -> PlaceId {
        let id = PlaceId(self.places.len());
        self.index.insert(place.id.clone(), id);
        self.places.push(place);
        id
    }

    /// Panics unless both ends are in one system.
    pub fn add_leg(&mut self, leg: Leg) {
        assert_eq!(
            self.place(leg.from).system,
            self.place(leg.to).system,
            "a leg stays in one system"
        );
        self.legs.push(leg);
    }

    pub fn lookup(&self, id: &str) -> Option<PlaceId> {
        self.index.get(id).copied()
    }

    pub fn place(&self, id: PlaceId) -> &Place {
        &self.places[id.0]
    }

    pub fn places(&self) -> impl Iterator<Item = (PlaceId, &Place)> {
        self.places.iter().enumerate().map(|(k, p)| (PlaceId(k), p))
    }

    pub fn legs(&self) -> &[Leg] {
        &self.legs
    }

    /// The graph without the places `ids` and their legs.
    pub fn without(&self, ids: &[&str]) -> RouteGraph {
        let mut g = RouteGraph::new(self.source.clone());
        let mut kept = HashMap::new();
        for (id, p) in self.places() {
            if !ids.contains(&p.id.as_str()) {
                kept.insert(id, g.add_place(p.clone()));
            }
        }
        for l in &self.legs {
            if let (Some(&from), Some(&to)) = (kept.get(&l.from), kept.get(&l.to)) {
                g.legs.push(Leg {
                    from,
                    to,
                    ..l.clone()
                });
            }
        }
        g
    }

    /// `data/routes.toml`, loaded once per process.
    pub fn standard() -> &'static RouteGraph {
        static STANDARD: OnceLock<RouteGraph> = OnceLock::new();
        STANDARD.get_or_init(|| {
            RouteGraph::from_toml(
                include_str!("../../../data/routes.toml"),
                Ephemeris::standard(),
            )
            .expect("data/routes.toml is valid")
        })
    }

    pub fn from_toml(src: &str, eph: &Ephemeris) -> Result<RouteGraph, String> {
        let file: File = toml::from_str(src).map_err(|e| e.to_string())?;
        let mut graph = RouteGraph::new(file.source);
        for p in file.place {
            if graph.lookup(&p.id).is_some() {
                return Err(format!("place {} listed twice", p.id));
            }
            let system = eph
                .lookup(&p.system)
                .ok_or_else(|| format!("place {}: no body {}", p.id, p.system))?;
            graph.add_place(Place {
                id: p.id,
                name: p.name,
                system,
                gateway: p.gateway,
            });
        }
        for l in file.leg {
            let end = |id: &str| graph.lookup(id).ok_or_else(|| format!("no place {id}"));
            let (from, to) = (end(&l.from)?, end(&l.to)?);
            if graph.place(from).system != graph.place(to).system {
                return Err(format!("leg {} to {} leaves its system", l.from, l.to));
            }
            graph.legs.push(Leg {
                from,
                to,
                dv: l.dv,
                days: l.days,
                aero: l.aero,
                source: l.source,
            });
        }
        Ok(graph)
    }

    pub fn to_toml(&self, eph: &Ephemeris) -> String {
        let file = File {
            source: self.source.clone(),
            place: self
                .places
                .iter()
                .map(|p| PlaceEntry {
                    id: p.id.clone(),
                    name: p.name.clone(),
                    system: eph.body(p.system).id.clone(),
                    gateway: p.gateway,
                })
                .collect(),
            leg: self
                .legs
                .iter()
                .map(|l| LegEntry {
                    from: self.place(l.from).id.clone(),
                    to: self.place(l.to).id.clone(),
                    dv: l.dv,
                    days: l.days,
                    aero: l.aero,
                    source: l.source.clone(),
                })
                .collect(),
        };
        toml::to_string(&file).expect("the graph serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// From a low orbit, a gateway's departure is the hyperbola's burn; a
    /// pass too fast for the atmosphere is refused.
    #[test]
    fn gateways_price_excess_speeds() {
        let gm = 398_600.4;
        let r = 6_778.0;
        let low = Escape::Periapsis {
            extra: 0.0,
            radius: r,
            speed: (gm / r).sqrt(),
        };
        let v_inf: f64 = 3.0;
        let hyperbola = (v_inf * v_inf + 2.0 * gm / r).sqrt() - (gm / r).sqrt();
        assert!((low.dv(gm, v_inf) - hyperbola).abs() < 1e-12);
        assert_eq!(Escape::Rendezvous.dv(gm, v_inf), v_inf);
        let pass = Pass {
            radius: 6_453.0,
            max_speed: 12.5,
            after: 0.1,
        };
        assert_eq!(pass.dv(gm, 3.0), Some(0.1));
        assert_eq!(pass.dv(gm, 8.0), None);
    }

    /// The TOML form keeps every place, gateway and leg.
    #[test]
    fn round_trip() {
        let eph = Ephemeris::standard();
        let earth = eph.lookup("earth").unwrap();
        let mut g = RouteGraph::new("test".into());
        let gateway = Gateway {
            depart: Some(Escape::Periapsis {
                extra: 0.5,
                radius: 6_578.0,
                speed: 11.0,
            }),
            arrive: Some(Escape::Rendezvous),
            pass: Some(Pass {
                radius: 6_453.0,
                max_speed: 12.5,
                after: 0.1,
            }),
            days: 5.0,
        };
        let a = g.add_place(Place {
            id: "a".into(),
            name: "A".into(),
            system: earth,
            gateway: Some(gateway),
        });
        let b = g.add_place(Place {
            id: "b".into(),
            name: "B".into(),
            system: earth,
            gateway: None,
        });
        g.add_leg(Leg {
            from: a,
            to: b,
            dv: 0.7,
            days: 3.0,
            aero: true,
            source: "made up".into(),
        });
        let text = g.to_toml(eph);
        let back = RouteGraph::from_toml(&text, eph).unwrap();
        assert_eq!(back.to_toml(eph), text);
        assert_eq!(back.source, "test");
        let l = &back.legs()[0];
        assert_eq!(
            (back.place(l.from).id.as_str(), l.aero, l.dv),
            ("a", true, 0.7)
        );
        assert!(back.place(b).gateway.is_none());
        let (gm, v_inf) = (398_600.4, 1.0);
        let kept = back.place(a).gateway.unwrap();
        for (x, y) in [
            (kept.departure(gm, v_inf), gateway.departure(gm, v_inf)),
            (kept.arrival(gm, v_inf), gateway.arrival(gm, v_inf)),
            (kept.aerocapture(gm, v_inf), gateway.aerocapture(gm, v_inf)),
        ] {
            assert_eq!(x, y);
        }
        assert_eq!(kept.days, 5.0);
    }
}
