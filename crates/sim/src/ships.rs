//! Ships' classes and loads (M1b.md, step 2 plan, part 2). A plan's ship
//! names its class (`data/economy.toml`) and what it carries at its start
//! (`orbit::mission::Ship` keeps them as written); one without a class is
//! massless, as in M1c. What its class does not allow of it is a
//! [`Breach`]: a warning in the sandbox, refused in career (`game`).

use crate::params::{Economy, ShipClass};
use orbit::ephem::{BodyId, Ephemeris};
use orbit::mission::{Ship, Spot, Step};
use orbit::places::{PlaceKind, Places};
use orbit::routes::RouteGraph;
use std::fmt;

/// What a hold carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Commodity {
    Lox,
    /// Methane.
    Ch4,
    /// Hydrogen.
    Lh2,
    Equipment,
    /// Third-party payload.
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
    /// As plans write it (`data/economy.toml`'s outputs write the first
    /// three so).
    pub fn id(self) -> &'static str {
        match self {
            Commodity::Lox => "lox",
            Commodity::Ch4 => "ch4",
            Commodity::Lh2 => "lh2",
            Commodity::Equipment => "equipment",
            Commodity::Payload => "payload",
            Commodity::Metal => "metal",
        }
    }
}

/// What a ship carries: its tanks and its hold, t.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Load {
    pub lox: f64,
    /// Its propellant's fuel.
    pub fuel: f64,
    /// By [`Commodity`].
    pub cargo: [f64; COMMODITIES.len()],
}

impl Load {
    pub fn propellant(&self) -> f64 {
        self.lox + self.fuel
    }

    pub fn cargo(&self) -> f64 {
        self.cargo.iter().sum()
    }
}

/// A ship's class, by its place in [`Economy::ships`], and its load at its
/// start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fitted {
    pub class: usize,
    pub load: Load,
}

/// `ship`'s class and its load at its start, in `economy`; `None` for a
/// ship without a class. Its propellant is a full tank unless it says,
/// LOX and fuel at the class's mixture ratio.
pub fn read(ship: &Ship, economy: &Economy) -> Result<Option<Fitted>, String> {
    let Some(id) = &ship.class else {
        if ship.propellant_t.is_some() || !ship.cargo.is_empty() {
            return Err("a load needs a class".into());
        }
        return Ok(None);
    };
    let class = economy
        .ships
        .iter()
        .position(|c| &c.id == id)
        .ok_or_else(|| {
            let ids: Vec<&str> = economy.ships.iter().map(|c| c.id.as_str()).collect();
            format!("no class {id:?}; the classes are {}", ids.join(", "))
        })?;
    let c = &economy.ships[class];
    let tonnes = |what: &str, t: f64| match t.is_finite() && t >= 0.0 {
        true => Ok(t),
        false => Err(format!("{what} is a number of tonnes, not negative")),
    };
    let propellant = tonnes("propellant_t", ship.propellant_t.unwrap_or(c.tank))?;
    let lox_share = economy.propellant(c.propellant).lox_share;
    let mut load = Load {
        lox: propellant * lox_share,
        fuel: propellant * (1.0 - lox_share),
        ..Load::default()
    };
    for (k, &t) in &ship.cargo {
        let commodity = COMMODITIES.iter().find(|c| c.id() == k).ok_or_else(|| {
            let ids: Vec<&str> = COMMODITIES.iter().map(|c| c.id()).collect();
            format!("no commodity {k:?}; the commodities are {}", ids.join(", "))
        })?;
        load.cargo[*commodity as usize] = tonnes(&format!("cargo's {k}"), t)?;
    }
    Ok(Some(Fitted { class, load }))
}

/// Something a ship's class does not allow of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Breach {
    /// The step it is at; `None` for its start.
    pub step: Option<usize>,
    pub why: String,
}

impl fmt::Display for Breach {
    /// As a plan counts its steps (`[[ship.leg]]`).
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.step {
            Some(s) => write!(f, "leg {}: {}", s + 1, self.why),
            None => write!(f, "{}", self.why),
        }
    }
}

/// Loads are held to their tanks and holds to the gram, t: amounts
/// written in decimal add up only to within rounding.
const GRAM: f64 = 1e-6;

/// Tonnes, to the gram.
fn t(x: f64) -> String {
    let s = format!("{x:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Whether `class` lands on `body`: on one of the surfaces it lists, the
/// economy drawn on the standard route graph (as `game` draws it).
fn lands_on(class: &ShipClass, body: BodyId) -> bool {
    let (graph, places) = (RouteGraph::standard(), Places::standard());
    class.lands.iter().any(|&p| {
        places
            .get(&graph.place(p).id)
            .is_some_and(|p| matches!(p.kind, PlaceKind::Surface(b) if b == body))
    })
}

/// What `ship`'s class (`fitted`, as [`read`] has it) does not allow of
/// it: a load beyond its tanks or its hold; being on a surface it does not
/// land on (`lands`), at its start or after a leg; arriving through an
/// atmosphere (`aero`); and, without `interplanetary`, being outside
/// Earth's system, where it is bought, or leaving the system it is in (a
/// planet's, or a small body alone).
pub fn breaches(ship: &Ship, fitted: &Fitted, economy: &Economy, eph: &Ephemeris) -> Vec<Breach> {
    let c = &economy.ships[fitted.class];
    let mut out = Vec::new();
    let mut breach = |step, why| out.push(Breach { step, why });
    let load = &fitted.load;
    if load.propellant() > c.tank + GRAM {
        breach(
            None,
            format!(
                "a {}'s tanks hold {} t, not {}",
                c.id,
                t(c.tank),
                t(load.propellant())
            ),
        );
    }
    if load.cargo() > c.cargo + GRAM {
        breach(
            None,
            format!(
                "a {}'s hold takes {} t, not {}",
                c.id,
                t(c.cargo),
                t(load.cargo())
            ),
        );
    }
    let no_landing = |at: &Spot| format!("a {} can't land at {}", c.id, at.label);
    if let PlaceKind::Surface(b) = ship.start.kind
        && !lands_on(c, b)
    {
        breach(None, no_landing(&ship.start));
    }
    let earth = eph.lookup("earth").expect("Earth is in the ephemeris");
    if ship.start.kind.system(eph) != earth && !c.interplanetary {
        breach(
            None,
            format!("a {} can't leave Earth's system, where it is bought", c.id),
        );
    }
    let mut from = &ship.start.kind;
    for (i, step) in ship.steps.iter().enumerate() {
        let Step::Go(go) = step else { continue };
        let to = &go.to.kind;
        let system = from.system(eph);
        if system != to.system(eph) && !c.interplanetary {
            breach(
                Some(i),
                format!("a {} can't leave {}'s system", c.id, eph.body(system).name),
            );
        }
        if go.aero && !c.aero {
            breach(
                Some(i),
                format!("a {} can't arrive through an atmosphere", c.id),
            );
        }
        if let PlaceKind::Surface(b) = *to
            && !lands_on(c, b)
        {
            breach(Some(i), no_landing(&go.to));
        }
        from = to;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{Draw, EconomyFile};
    use orbit::mission::Plan;

    fn economy() -> Economy {
        EconomyFile::standard().draw(
            Ephemeris::standard(),
            RouteGraph::standard(),
            &mut Draw::Central,
        )
    }

    fn ship(src: &str) -> Ship {
        let src = format!("name = \"x\"\n{src}");
        Plan::parse(&src, Ephemeris::standard(), Places::standard())
            .unwrap()
            .ships
            .remove(0)
    }

    fn breaches_of(src: &str) -> Vec<String> {
        let (e, ship) = (economy(), ship(src));
        let fitted = read(&ship, &e).unwrap().unwrap();
        breaches(&ship, &fitted, &e, Ephemeris::standard())
            .iter()
            .map(Breach::to_string)
            .collect()
    }

    #[test]
    fn a_full_tank_by_default_at_the_mixture_ratio() {
        let e = economy();
        let tug = ship(
            r#"
[[ship]]
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-01", cargo = { equipment = 20, lh2 = 5.5 } }
"#,
        );
        let fitted = read(&tug, &e).unwrap().unwrap();
        assert_eq!(e.ships[fitted.class].id, "tug-h");
        let load = fitted.load;
        // Hydrolox at 6:1.
        assert!((load.lox - 150.0 * 6.0 / 7.0).abs() < 1e-9, "{load:?}");
        assert!((load.fuel - 150.0 / 7.0).abs() < 1e-9, "{load:?}");
        assert_eq!(load.cargo[Commodity::Equipment as usize], 20.0);
        assert_eq!(load.cargo[Commodity::Lh2 as usize], 5.5);
        assert_eq!(load.cargo(), 25.5);
        let mut short = tug.clone();
        short.propellant_t = Some(70.0);
        let load = read(&short, &e).unwrap().unwrap().load;
        assert!((load.propellant() - 70.0).abs() < 1e-12);
        let mut massless = tug;
        massless.class = None;
        massless.cargo.clear();
        assert_eq!(read(&massless, &e), Ok(None));
    }

    #[test]
    fn what_a_load_is_not() {
        let e = economy();
        let base = ship(
            r#"
[[ship]]
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-01" }
"#,
        );
        let fails = |edit: &dyn Fn(&mut Ship)| {
            let mut s = base.clone();
            edit(&mut s);
            read(&s, &e).unwrap_err()
        };
        assert!(
            fails(&|s| s.class = Some("barge".into()))
                .starts_with("no class \"barge\"; the classes are hauler, lander")
        );
        assert_eq!(
            fails(&|s| {
                s.cargo.insert("water".into(), 1.0);
            }),
            "no commodity \"water\"; the commodities are lox, ch4, lh2, equipment, payload, metal"
        );
        assert_eq!(
            fails(&|s| s.propellant_t = Some(-1.0)),
            "propellant_t is a number of tonnes, not negative"
        );
        assert_eq!(
            fails(&|s| s.propellant_t = Some(f64::NAN)),
            "propellant_t is a number of tonnes, not negative"
        );
        assert_eq!(
            fails(&|s| {
                s.class = None;
                s.propellant_t = Some(1.0);
            }),
            "a load needs a class"
        );
    }

    #[test]
    fn what_each_class_allows() {
        // A lander: to the surface and back, but not through the air nor
        // to Mars.
        assert_eq!(
            breaches_of(
                r#"
[[ship]]
name = "Lander"
class = "lander"
start = { at = "llo", epoch = "2045-03-01" }

[[ship.leg]]
to = "moon-south-pole"

[[ship.leg]]
to = "llo"

[[ship.leg]]
to = "leo"
days = [3.5]
aero = true

[[ship.leg]]
wait = 2

[[ship.leg]]
to = "lmo"
"#
            ),
            [
                "leg 3: a lander can't arrive through an atmosphere",
                "leg 5: a lander can't leave Earth's system",
            ]
        );
        // A tug: through the air, but not down to a surface; nor its start
        // on one.
        assert_eq!(
            breaches_of(
                r#"
[[ship]]
name = "Tug"
class = "tug-h"
start = { at = "moon-mare", epoch = "2045-03-01" }

[[ship.leg]]
to = "llo"

[[ship.leg]]
to = "leo"
days = [3.5]
aero = true

[[ship.leg]]
to = "mars-surface"
"#
            ),
            [
                "a tug-h can't land at moon-mare",
                "leg 3: a tug-h can't leave Earth's system",
                "leg 3: a tug-h can't land at mars-surface",
            ]
        );
        // A hauler: anywhere it goes here, landing on Mars.
        assert!(
            breaches_of(
                r#"
[[ship]]
name = "Hauler"
class = "hauler"
start = { at = "leo", epoch = "2045-03-01" }

[[ship.leg]]
to = "mars-surface"
aero = true
"#
            )
            .is_empty()
        );
        // Loaded beyond its tanks and its hold.
        assert_eq!(
            breaches_of(
                r#"
[[ship]]
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-01", propellant_t = 150.5, cargo = { equipment = 40, payload = 10.5 } }
"#
            ),
            [
                "a tug-h's tanks hold 150 t, not 150.5",
                "a tug-h's hold takes 50 t, not 50.5",
            ]
        );
        // Full to the tonne, in amounts that add up only to within
        // rounding (found in review); over by 10 kg, said so.
        assert!(
            breaches_of(
                r#"
[[ship]]
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-01", cargo = { lox = 0.1, equipment = 42.2, payload = 7.7 } }
"#
            )
            .is_empty()
        );
        assert_eq!(
            breaches_of(
                r#"
[[ship]]
name = "Tug"
class = "tug-h"
start = { at = "leo", epoch = "2045-03-01", propellant_t = 150.01 }
"#
            ),
            ["a tug-h's tanks hold 150 t, not 150.01"]
        );
        // Outside Earth's system, a ship that cannot leave it: at its
        // start, but not for its legs within Mars's.
        assert_eq!(
            breaches_of(
                r#"
[[ship]]
name = "Tug"
class = "tug-h"
start = { at = "lmo", epoch = "2045-03-01" }

[[ship.leg]]
to = "phobos"
"#
            ),
            ["a tug-h can't leave Earth's system, where it is bought"]
        );
    }
}
