//! The places ships go (`data/places.toml`), each defined physically: a low
//! circular orbit, a station orbit from the catalog's families, a small
//! body or moon to meet, or a body's surface. Mission plans and the route
//! graph name places by the ids here.

use crate::ephem::{BodyId, BodyKind, Ephemeris};
use crate::shooting::LowOrbit;
use std::sync::OnceLock;

/// Where a ship can be.
#[derive(Clone, Debug)]
pub enum PlaceKind {
    /// A circular orbit (see [`LowOrbit`]).
    Low(LowOrbit),
    /// The member of an Earth–Moon family (`data/orbits.toml`) whose period
    /// is nearest `period`, days.
    Station { family: String, period: f64 },
    /// Meeting a small body or a moon.
    Body(BodyId),
    /// Resting on a body's surface, reached from a low orbit about it.
    Surface(BodyId),
}

impl PlaceKind {
    /// The planet whose system the place is in (a small body is its own).
    pub fn system(&self, eph: &Ephemeris) -> BodyId {
        match self {
            PlaceKind::Low(low) => eph.system(low.body),
            PlaceKind::Station { .. } => eph.lookup("earth").expect("Earth is in the ephemeris"),
            PlaceKind::Body(b) | PlaceKind::Surface(b) => eph.system(*b),
        }
    }

    /// The place written as a TOML table (see `data/places.toml`), which
    /// may also hold the keys `extra`.
    pub fn from_table(
        table: &toml::Table,
        eph: &Ephemeris,
        extra: &[&str],
    ) -> Result<PlaceKind, String> {
        let kinds = ["low", "station", "body", "surface"];
        let kind = match kinds
            .iter()
            .filter(|k| table.contains_key(**k))
            .collect::<Vec<_>>()[..]
        {
            [k] => *k,
            _ => return Err(format!("a place needs one of {kinds:?}")),
        };
        let allowed: &[&str] = match kind {
            "low" => &["low", "altitude", "radius", "inclination"],
            "station" => &["station", "period"],
            _ => &[kind],
        };
        if let Some(k) = table
            .keys()
            .find(|k| !allowed.contains(&k.as_str()) && !extra.contains(&k.as_str()))
        {
            return Err(format!("unknown key {k:?} for a {kind} place"));
        }
        let text = |key: &str| -> Result<&str, String> {
            table
                .get(key)
                .and_then(|v| v.as_str())
                .ok_or_else(|| format!("{key} must be a string"))
        };
        let number = |key: &str| -> Result<Option<f64>, String> {
            table
                .get(key)
                .map(|v| {
                    v.as_float()
                        .or_else(|| v.as_integer().map(|i| i as f64))
                        .ok_or_else(|| format!("{key} must be a number"))
                })
                .transpose()
        };
        let body = |key: &str| -> Result<BodyId, String> {
            let id = text(key)?;
            eph.lookup(id).ok_or_else(|| format!("no body {id:?}"))
        };
        Ok(match kind {
            "low" => {
                let b = body("low")?;
                let radius = match (number("altitude")?, number("radius")?) {
                    (Some(altitude), None) => eph.body(b).radius + altitude,
                    (None, Some(radius)) => radius,
                    _ => return Err("a low orbit needs an altitude or a radius".into()),
                };
                PlaceKind::Low(LowOrbit {
                    body: b,
                    radius,
                    inclination: number("inclination")?.map(f64::to_radians),
                })
            }
            "station" => PlaceKind::Station {
                family: text("station")?.to_string(),
                period: number("period")?.ok_or("a station needs a period")?,
            },
            "body" => PlaceKind::Body(body("body")?),
            _ => PlaceKind::Surface(body("surface")?),
        })
    }
}

#[derive(Clone, Debug)]
pub struct NamedPlace {
    pub id: String,
    pub name: String,
    pub kind: PlaceKind,
}

/// The places with ids.
#[derive(Clone, Debug)]
pub struct Places {
    list: Vec<NamedPlace>,
}

impl Places {
    /// `data/places.toml` and the ephemeris's small bodies, loaded once per
    /// process.
    pub fn standard() -> &'static Places {
        static STANDARD: OnceLock<Places> = OnceLock::new();
        STANDARD.get_or_init(|| {
            Self::parse(
                include_str!("../../../data/places.toml"),
                Ephemeris::standard(),
            )
            .expect("data/places.toml is valid")
        })
    }

    pub fn parse(src: &str, eph: &Ephemeris) -> Result<Places, String> {
        let file: toml::Table = toml::from_str(src).map_err(|e| e.to_string())?;
        let mut list: Vec<NamedPlace> = Vec::new();
        let entries = file
            .get("place")
            .and_then(|v| v.as_array())
            .ok_or("no [[place]] entries")?;
        for entry in entries {
            let table = entry.as_table().ok_or("a place is a table")?;
            let field = |key: &str| {
                table
                    .get(key)
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .ok_or_else(|| format!("a place needs a string {key}"))
            };
            let id = field("id")?;
            if list.iter().any(|p| p.id == id) {
                return Err(format!("place {id} listed twice"));
            }
            let kind = PlaceKind::from_table(table, eph, &["id", "name"])
                .map_err(|e| format!("place {id}: {e}"))?;
            list.push(NamedPlace {
                name: field("name")?,
                id,
                kind,
            });
        }
        for b in eph.ids() {
            let body = eph.body(b);
            if matches!(body.kind, BodyKind::SmallBody { .. })
                && !list.iter().any(|p| p.id == body.id)
            {
                list.push(NamedPlace {
                    id: body.id.clone(),
                    name: body.name.clone(),
                    kind: PlaceKind::Body(b),
                });
            }
        }
        Ok(Places { list })
    }

    pub fn get(&self, id: &str) -> Option<&NamedPlace> {
        self.list.iter().find(|p| p.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &NamedPlace> {
        self.list.iter()
    }

    /// The low orbit with id `id`, if it is one.
    pub fn low(&self, id: &str) -> Option<LowOrbit> {
        match self.get(id)?.kind {
            PlaceKind::Low(orbit) => Some(orbit),
            _ => None,
        }
    }

    /// The station orbit with id `id`, if it is one: its family and the
    /// period wanted, days.
    pub fn station(&self, id: &str) -> Option<(&str, f64)> {
        match &self.get(id)?.kind {
            PlaceKind::Station { family, period } => Some((family, *period)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_places() {
        let eph = Ephemeris::standard();
        let places = Places::standard();
        let PlaceKind::Low(llo) = &places.get("llo").unwrap().kind else {
            panic!("llo is a low orbit")
        };
        assert_eq!(
            llo.radius,
            eph.body(eph.lookup("moon").unwrap()).radius + 100.0
        );
        assert_eq!(llo.inclination, Some(90f64.to_radians()));
        let earth = eph.lookup("earth").unwrap();
        assert_eq!(places.get("llo").unwrap().kind.system(eph), earth);
        let mars = eph.lookup("mars").unwrap();
        assert_eq!(places.get("phobos").unwrap().kind.system(eph), mars);
        assert!(matches!(
            places.get("2000-sg344").unwrap().kind,
            PlaceKind::Body(_)
        ));
    }

    #[test]
    fn misspelled_keys_are_errors() {
        let eph = Ephemeris::standard();
        let table = |s: &str| -> toml::Table { toml::from_str(s).unwrap() };
        let bad = PlaceKind::from_table(
            &table("low = \"moon\"\naltitude = 100\ninclinaton = 90"),
            eph,
            &[],
        );
        assert!(bad.unwrap_err().contains("inclinaton"));
        assert!(PlaceKind::from_table(&table("low = \"moon\""), eph, &[]).is_err());
        assert!(
            PlaceKind::from_table(&table("low = \"moon\"\naltitude = 1\nradius = 2"), eph, &[])
                .is_err()
        );
        assert!(PlaceKind::from_table(&table("body = \"vulcan\""), eph, &[]).is_err());
        assert!(
            PlaceKind::from_table(&table("low = \"moon\"\nbody = \"moon\""), eph, &[]).is_err()
        );
    }
}
