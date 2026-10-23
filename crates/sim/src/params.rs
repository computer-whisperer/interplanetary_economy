//! The economy's parameters (`data/economy.toml`) and the world's draw of
//! them (M1b.md, step 2 plan, part 1): an [`Economy`] is one game's worth
//! of concrete numbers, in their own units. M2's LP builds its periods
//! from one (`plan::scenario`). The draw keeps M2's order, so M2's seeds
//! give M2's worlds. Keyed draws (step 5 plan, part 1) are streams named
//! by a key, for what is drawn as the game runs.

use crate::hash::Fixed;
use orbit::ephem::{BodyKind, Ephemeris};
use orbit::routes::{PlaceId, RouteGraph};
use orbit::time::Epoch;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Normal};
use serde::Deserialize;
use std::f64::consts::PI;
use std::hash::Hasher;

/// Standard gravity, km/s².
const G0: f64 = 9.806_65e-3;

/// An uncertain parameter: fixed, lognormal or uniform.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Dist {
    Fixed(f64),
    LogNormal { median: f64, sigma: f64 },
    Uniform { low: f64, high: f64 },
}

impl Dist {
    pub fn central(&self) -> f64 {
        match *self {
            Dist::Fixed(x) => x,
            Dist::LogNormal { median, .. } => median,
            Dist::Uniform { low, high } => (low + high) / 2.0,
        }
    }

    pub fn sample(&self, rng: &mut Draw) -> f64 {
        match (*self, rng) {
            (d, Draw::Central) => d.central(),
            (Dist::Fixed(x), _) => x,
            (Dist::LogNormal { median, sigma }, Draw::Random(r)) => {
                median * (sigma * Normal::new(0.0, 1.0).unwrap().sample(r)).exp()
            }
            (Dist::Uniform { low, high }, Draw::Random(r)) => r.random_range(low..=high),
        }
    }
}

/// Where parameter values come from: every distribution's central value, or
/// a seeded random draw.
pub enum Draw {
    Central,
    Random(Box<ChaCha8Rng>),
}

impl Draw {
    pub fn seeded(seed: u64) -> Self {
        Draw::Random(Box::new(ChaCha8Rng::seed_from_u64(seed)))
    }

    /// The stream named `key` in the world of `seed` (0 for the central
    /// draw): the same in every build and on every thread, and another
    /// for another seed or key. Always random: what it draws (a survey's
    /// estimate) is state, in the central world too.
    pub fn keyed(seed: u64, key: &str) -> Self {
        Draw::seeded(Fixed::default().u64(seed).str(key).finish())
    }

    /// True with probability `p`; the central draw takes the likelier side.
    pub fn chance(&mut self, p: f64) -> bool {
        match self {
            Draw::Central => p >= 0.5,
            Draw::Random(r) => r.random_bool(p.clamp(0.0, 1.0)),
        }
    }

    /// An index chosen with the given weights; the central draw takes the
    /// heaviest.
    fn pick(&mut self, weights: &[f64]) -> usize {
        match self {
            Draw::Central => (0..weights.len())
                .max_by(|&a, &b| weights[a].total_cmp(&weights[b]))
                .unwrap(),
            Draw::Random(r) => {
                let mut u = r.random_range(0.0..weights.iter().sum::<f64>());
                for (k, w) in weights.iter().enumerate() {
                    if u < *w {
                        return k;
                    }
                    u -= w;
                }
                weights.len() - 1
            }
        }
    }
}

// --- data/economy.toml ------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EconomyFile {
    start: String,
    end: String,
    discount_rate: f64,
    propellant: PropellantsRow,
    earth: EarthRow,
    budget: BudgetRow,
    metals: MetalsRow,
    /// Nodes on a planetary surface: ships land there only if they may.
    surfaces: Vec<String>,
    ship: Vec<ShipRow>,
    facility: Vec<FacilityRow>,
    asteroid_class: Vec<AsteroidClassRow>,
    asteroid_plant: Vec<AsteroidPlantRow>,
    technology: Vec<TechnologyRow>,
    demand: Vec<DemandRow>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PropellantsRow {
    methalox: PropellantRow,
    hydrolox: PropellantRow,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PropellantRow {
    isp_s: f64,
    /// Oxidizer to fuel, by mass.
    mixture: f64,
    /// Fraction of the stored fuel lost per day; none if absent.
    fuel_boiloff_per_day: Option<Dist>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EarthRow {
    launch_usd_per_kg_start: Dist,
    launch_usd_per_kg_end: Dist,
    equipment_usd_per_kg: Dist,
    propellant_usd_per_kg: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetRow {
    musd_per_year: Dist,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MetalsRow {
    /// Price steps by cumulative tonnes sold: `[tonnes, usd_per_kg]`, the
    /// first step's price holding for its tonnes, and so on.
    steps: Vec<[f64; 2]>,
    /// Scales every step's price.
    price_factor: Dist,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShipRow {
    id: String,
    dry_t: f64,
    propellant_t: f64,
    cargo_t: f64,
    price_musd: Dist,
    propellant: Propellant,
    #[serde(default)]
    aero: bool,
    #[serde(default)]
    interplanetary: bool,
    #[serde(default)]
    lands: Vec<String>,
    turnaround_days: f64,
    /// Flights before the ship is worn out.
    life_flights: f64,
    ops_musd_per_trip: Dist,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(deny_unknown_fields)]
struct OutputRow {
    #[serde(default)]
    lox: f64,
    #[serde(default)]
    ch4: f64,
    #[serde(default)]
    lh2: f64,
    #[serde(default)]
    metal: f64,
}

/// Equipment per t/yr of output: `plant` for the plant and its power, plus
/// `ore` per t/yr of ore handled, which is output / grade.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EquipmentRow {
    plant: Dist,
    ore: Dist,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FacilityRow {
    id: String,
    node: String,
    output: OutputRow,
    equipment_t_per_tpy: EquipmentRow,
    /// Product mass per mass of ore.
    grade: Dist,
    /// Probability that the site has the resource at all.
    #[serde(default = "certain")]
    present: f64,
    build_months: f64,
    /// The `technology` program the plant needs.
    technology: String,
    /// The survey of the site the plant needs, if any; one survey covers
    /// all of the site's deposits.
    survey: Option<ProgramRow>,
    /// Finite deposits of the resource, each its own plant option; one
    /// unlimited deposit if empty.
    #[serde(default)]
    deposits: Vec<DepositRow>,
}

fn certain() -> f64 {
    1.0
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DepositRow {
    id: String,
    /// The deposit's grade over the site's drawn grade.
    grade_factor: f64,
    /// Recoverable product, t.
    reserve_t: Dist,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AsteroidClassRow {
    /// Spectral classes this prior covers.
    spectral: Vec<String>,
    albedo: f64,
    density_t_per_m3: f64,
    /// Probability weights of each composition, by `asteroid_plant` id.
    /// The remainder is barren.
    composition: toml::Table,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AsteroidPlantRow {
    id: String,
    output: OutputRow,
    equipment_t_per_tpy: EquipmentRow,
    grade: Dist,
    /// Fraction of the body's mass that can be mined.
    minable_fraction: f64,
    build_months: f64,
    technology: String,
    /// The survey of each body, before its plant.
    survey: ProgramRow,
}

/// What a plant option needs: its technology's id, and its site's survey
/// (row, prior probability that the site has the resource, program id).
struct Need<'a> {
    technology: &'a str,
    survey: Option<(&'a ProgramRow, f64, String)>,
}

/// A survey, paid before a plant: spent at no more than `cost_musd /
/// years` per year, after which the plant may be installed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramRow {
    cost_musd: Dist,
    years: Dist,
    /// The estimate's lognormal σ: the survey reports each deposit's grade
    /// and reserve as the truth times exp(σz).
    noise: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TechnologyRow {
    id: String,
    cost_musd: Dist,
    years: Dist,
}

/// A propellant: liquid oxygen and one fuel.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Propellant {
    /// Oxygen and methane.
    Methalox,
    /// Oxygen and hydrogen.
    Hydrolox,
}

pub const PROPELLANTS: [Propellant; 2] = [Propellant::Methalox, Propellant::Hydrolox];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Demanded {
    /// Third-party payload, launched from Earth.
    Payload,
    /// Propellant sold to third-party ships, at its mixture ratio.
    Methalox,
    Hydrolox,
}

impl Demanded {
    pub fn propellant(self) -> Option<Propellant> {
        match self {
            Demanded::Payload => None,
            Demanded::Methalox => Some(Propellant::Methalox),
            Demanded::Hydrolox => Some(Propellant::Hydrolox),
        }
    }
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum WhatRow {
    Payload,
    Methalox,
    Hydrolox,
    /// Either propellant, by the row's `split`.
    Propellant,
}

/// Shares of a propellant demand by propellant.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SplitRow {
    methalox: f64,
    hydrolox: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DemandRow {
    id: String,
    what: WhatRow,
    split: Option<SplitRow>,
    /// Delivered to any of these nodes.
    nodes: Vec<String>,
    /// Quantity demanded at the reference price in the start year.
    ref_tpy: Dist,
    ref_usd_per_kg: f64,
    elasticity: f64,
    /// Annual growth of the quantity demanded at any price.
    growth: f64,
    /// The demand curve is stepped between these prices, halving each step.
    max_usd_per_kg: f64,
    min_usd_per_kg: f64,
}

// --- one draw -----------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct ShipClass {
    pub id: String,
    pub dry: f64,
    pub tank: f64,
    pub cargo: f64,
    pub price_musd: f64,
    pub propellant: Propellant,
    pub aero: bool,
    pub interplanetary: bool,
    pub lands: Vec<PlaceId>,
    pub turnaround_days: f64,
    /// What each trip costs, $M: the wear (the price over the ship's life
    /// in flights) and flight operations.
    pub trip_musd: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Output {
    pub lox: f64,
    pub ch4: f64,
    pub lh2: f64,
    pub metal: f64,
}

impl Output {
    /// Oxygen and fuels, t per t/yr of rated output.
    pub fn propellant(&self) -> f64 {
        self.lox + self.ch4 + self.lh2
    }
}

/// What one propellant does, as drawn.
#[derive(Clone, Copy, Debug)]
pub struct PropellantSpec {
    /// Exhaust velocity, km/s.
    pub ve: f64,
    /// LOX share of propellant mass.
    pub lox_share: f64,
    /// Fraction of the stored fuel lost per day.
    pub fuel_boiloff_per_day: f64,
}

/// A kind of plant at one site, as drawn: a plant option. Every option
/// the file describes is in every world, `present` or not, so that a plan
/// naming one reads in every world (step 5 plan, decision 5).
#[derive(Clone, Debug)]
pub struct Facility {
    pub id: String,
    pub node: PlaceId,
    /// Per t/yr of rated output.
    pub output: Output,
    /// Equipment, t per t/yr of output, for the plant and its power.
    pub plant: f64,
    /// Equipment, t per t/yr of ore handled (output over grade).
    pub ore: f64,
    /// Product mass per mass of ore.
    pub grade: f64,
    /// Whether the resource is there at all; a plant on an absent deposit
    /// yields nothing.
    pub present: bool,
    /// From installation to output.
    pub build_months: f64,
    /// Cumulative output the deposit can yield, t.
    pub reserve: Option<f64>,
    /// What is known of the deposit before a survey.
    pub prior: Prior,
    /// Indices into [`Economy::programs`] that must be finished before
    /// the plant is installed.
    pub programs: Vec<usize>,
}

impl Facility {
    /// Equipment, t per t/yr of output, were the grade `grade`.
    pub fn alpha_at(&self, grade: f64) -> f64 {
        self.plant + self.ore / grade
    }

    /// Equipment, t per t/yr of output, at the true grade.
    pub fn alpha(&self) -> f64 {
        self.alpha_at(self.grade)
    }
}

/// What the file says of a deposit before anything is learned: its central
/// grade and reserve, and the probability that the resource is there.
#[derive(Clone, Copy, Debug)]
pub struct Prior {
    pub grade: f64,
    pub reserve: Option<f64>,
    pub present: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramKind {
    /// Develops a plant technology.
    Technology,
    /// Characterizes one deposit.
    Survey,
}

/// A fixed cost that must be paid before some plants are installed.
#[derive(Clone, Debug)]
pub struct Program {
    pub id: String,
    pub kind: ProgramKind,
    /// $M.
    pub cost_musd: f64,
    /// From its start to the first plant it allows; spending is at most
    /// `cost_musd / years` a year.
    pub years: f64,
    /// The probability that what a survey characterizes has the resource
    /// at all; 1 for a technology.
    pub prior: f64,
    /// A survey's estimate's lognormal σ; 0 for a technology.
    pub noise: f64,
}

/// Third-party demand for one thing: Q = `ref_tpy` × `share` × (price /
/// `ref_usd_per_kg`)^−`elasticity` in the start year, growing by `growth`
/// a year, between the prices given.
#[derive(Clone, Debug)]
pub struct Demand {
    pub id: String,
    pub what: Demanded,
    pub nodes: Vec<PlaceId>,
    pub ref_tpy: f64,
    /// Of the row's demand, by its `split` (1 without one).
    pub share: f64,
    pub ref_usd_per_kg: f64,
    pub elasticity: f64,
    pub growth: f64,
    pub max_usd_per_kg: f64,
    pub min_usd_per_kg: f64,
}

/// What a draw decided about one asteroid.
#[derive(Clone, Debug)]
pub struct AsteroidDraw {
    pub id: String,
    pub composition: Option<String>,
    pub mass_t: f64,
}

/// One game's economy, as drawn.
#[derive(Clone, Debug)]
pub struct Economy {
    pub start: Epoch,
    pub end: Epoch,
    pub discount_rate: f64,
    /// By [`Propellant`].
    pub propellants: [PropellantSpec; 2],
    /// $/kg to LEO at the start and at the end, declining geometrically
    /// between them.
    pub launch_usd_per_kg: [f64; 2],
    pub equipment_usd_per_kg: f64,
    pub propellant_usd_per_kg: f64,
    pub budget_musd_per_year: f64,
    /// `(tonnes, usd_per_kg)` by cumulative sales.
    pub metal_steps: Vec<(f64, f64)>,
    pub surfaces: Vec<PlaceId>,
    pub ships: Vec<ShipClass>,
    pub facilities: Vec<Facility>,
    pub programs: Vec<Program>,
    pub demand: Vec<Demand>,
    pub asteroids: Vec<AsteroidDraw>,
}

impl Economy {
    pub fn propellant(&self, p: Propellant) -> &PropellantSpec {
        &self.propellants[p as usize]
    }

    /// $/kg to LEO at `t`: geometric between the start's and the end's
    /// prices, held outside them.
    pub fn launch_usd_per_kg_at(&self, t: Epoch) -> f64 {
        let [l0, l1] = self.launch_usd_per_kg;
        let x = ((t.0 - self.start.0) / (self.end.0 - self.start.0)).clamp(0.0, 1.0);
        l0 * (l1 / l0).powf(x)
    }
}

impl EconomyFile {
    pub fn standard() -> Self {
        toml::from_str(include_str!("../../../data/economy.toml"))
            .expect("data/economy.toml is valid")
    }

    /// One draw of the economy, its places those of `graph`.
    pub fn draw(&self, eph: &Ephemeris, graph: &RouteGraph, draw: &mut Draw) -> Economy {
        let node = |id: &str| {
            graph
                .lookup(id)
                .unwrap_or_else(|| panic!("economy.toml: unknown node {id:?}"))
        };
        let start: Epoch = self.start.parse().expect("economy.toml: start");
        let end: Epoch = self.end.parse().expect("economy.toml: end");

        let launch_usd_per_kg = [
            self.earth.launch_usd_per_kg_start.sample(draw),
            self.earth.launch_usd_per_kg_end.sample(draw),
        ];

        let mut ships: Vec<ShipClass> = self
            .ship
            .iter()
            .map(|s| {
                let price_musd = s.price_musd.sample(draw);
                ShipClass {
                    id: s.id.clone(),
                    dry: s.dry_t,
                    tank: s.propellant_t,
                    cargo: s.cargo_t,
                    price_musd,
                    propellant: s.propellant,
                    aero: s.aero,
                    interplanetary: s.interplanetary,
                    lands: s.lands.iter().map(|n| node(n)).collect(),
                    turnaround_days: s.turnaround_days,
                    trip_musd: price_musd / s.life_flights,
                }
            })
            .collect();

        // Each plant's technology and survey, and each deposit's size, are
        // sampled after everything else so that adding them left the other
        // parameters of each seed as they were; and the surveys of absent
        // options after those, since absent options were once left out.
        // An absent option draws nothing more than it did then.
        let mut needs: Vec<Need> = Vec::new();
        let mut reserves: Vec<(usize, &Dist)> = Vec::new();
        let mut facilities = Vec::new();
        for f in &self.facility {
            let present = draw.chance(f.present);
            let grade = f.grade.sample(draw);
            let plant = f.equipment_t_per_tpy.plant.sample(draw);
            let ore = f.equipment_t_per_tpy.ore.sample(draw);
            let option = |id: String, factor: f64, reserve: Option<&Dist>| Facility {
                id,
                node: node(&f.node),
                output: f.output.into(),
                plant,
                ore,
                grade: grade * factor,
                present,
                build_months: f.build_months,
                reserve: if present {
                    None
                } else {
                    reserve.map(Dist::central)
                },
                prior: Prior {
                    grade: f.grade.central() * factor,
                    reserve: reserve.map(Dist::central),
                    present: f.present,
                },
                programs: Vec::new(),
            };
            let need = || Need {
                technology: &f.technology,
                survey: f
                    .survey
                    .as_ref()
                    .map(|s| (s, f.present, format!("survey:{}", f.id))),
            };
            if f.deposits.is_empty() {
                facilities.push(option(f.id.clone(), 1.0, None));
                needs.push(need());
            }
            for d in &f.deposits {
                if present {
                    reserves.push((facilities.len(), &d.reserve_t));
                }
                facilities.push(option(
                    format!("{}:{}", f.id, d.id),
                    d.grade_factor,
                    Some(&d.reserve_t),
                ));
                needs.push(need());
            }
        }

        let mut asteroids = Vec::new();
        for (place, n) in graph.places() {
            let body = eph.body(n.system);
            let BodyKind::SmallBody {
                spectral,
                diameter_km,
                h_mag,
            } = &body.kind
            else {
                continue;
            };
            let class = self
                .asteroid_class
                .iter()
                .find(|c| c.spectral.iter().any(|s| s == spectral))
                .or_else(|| {
                    self.asteroid_class
                        .iter()
                        .find(|c| c.spectral.iter().any(|s| s == "unknown"))
                })
                .expect("economy.toml: an asteroid_class covers \"unknown\"");
            let d_km = diameter_km.unwrap_or_else(|| {
                let h = h_mag.expect("a small body has a diameter or an H magnitude");
                1329.0 / class.albedo.sqrt() * 10f64.powf(-h / 5.0)
            });
            let mass_t = class.density_t_per_m3 * PI / 6.0 * (d_km * 1e3).powi(3);
            let plants: Vec<&AsteroidPlantRow> = self.asteroid_plant.iter().collect();
            let mut weights: Vec<f64> = plants
                .iter()
                .map(|p| {
                    class
                        .composition
                        .get(&p.id)
                        .and_then(|v| v.as_float())
                        .unwrap_or(0.0)
                })
                .collect();
            weights.push((1.0 - weights.iter().sum::<f64>()).max(0.0));
            let k = draw.pick(&weights);
            for (j, p) in plants.iter().enumerate() {
                let present = j == k;
                let e = &p.equipment_t_per_tpy;
                let (grade, plant, ore) = if present {
                    (
                        p.grade.sample(draw),
                        e.plant.sample(draw),
                        e.ore.sample(draw),
                    )
                } else {
                    (p.grade.central(), e.plant.central(), e.ore.central())
                };
                let reserve = |grade: f64| Some(mass_t * p.minable_fraction * grade);
                facilities.push(Facility {
                    id: format!("{}@{}", p.id, n.id),
                    node: place,
                    output: p.output.into(),
                    plant,
                    ore,
                    grade,
                    present,
                    build_months: p.build_months,
                    reserve: reserve(grade),
                    prior: Prior {
                        grade: p.grade.central(),
                        reserve: reserve(p.grade.central()),
                        present: weights[j],
                    },
                    programs: Vec::new(),
                });
                needs.push(Need {
                    technology: &p.technology,
                    survey: Some((&p.survey, weights[j], format!("survey:{}@{}", p.id, n.id))),
                });
            }
            asteroids.push(AsteroidDraw {
                id: n.id.clone(),
                composition: plants.get(k).map(|p| p.id.clone()),
                mass_t,
            });
        }

        let mut demand = Vec::new();
        for d in &self.demand {
            let ref_tpy = d.ref_tpy.sample(draw);
            let parts = match (d.what, &d.split) {
                (WhatRow::Payload, None) => vec![(d.id.clone(), Demanded::Payload, 1.0)],
                (WhatRow::Methalox, None) => vec![(d.id.clone(), Demanded::Methalox, 1.0)],
                (WhatRow::Hydrolox, None) => vec![(d.id.clone(), Demanded::Hydrolox, 1.0)],
                (WhatRow::Propellant, Some(split)) => vec![
                    (
                        format!("{}:methalox", d.id),
                        Demanded::Methalox,
                        split.methalox,
                    ),
                    (
                        format!("{}:hydrolox", d.id),
                        Demanded::Hydrolox,
                        split.hydrolox,
                    ),
                ],
                _ => panic!(
                    "economy.toml: demand {}: `split` goes with what = \"propellant\"",
                    d.id
                ),
            };
            for (id, what, share) in parts {
                demand.push(Demand {
                    id,
                    what,
                    // Places the graph leaves out (`plan --hub`) are not
                    // served; the test below checks the names.
                    nodes: d.nodes.iter().filter_map(|n| graph.lookup(n)).collect(),
                    ref_tpy,
                    share,
                    ref_usd_per_kg: d.ref_usd_per_kg,
                    elasticity: d.elasticity,
                    growth: d.growth,
                    max_usd_per_kg: d.max_usd_per_kg,
                    min_usd_per_kg: d.min_usd_per_kg,
                });
            }
        }

        let price_factor = self.metals.price_factor.sample(draw);
        let equipment_usd_per_kg = self.earth.equipment_usd_per_kg.sample(draw);
        let budget_musd_per_year = self.budget.musd_per_year.sample(draw);

        let mut programs: Vec<Program> = self
            .technology
            .iter()
            .map(|t| Program {
                id: t.id.clone(),
                kind: ProgramKind::Technology,
                cost_musd: t.cost_musd.sample(draw),
                years: t.years.sample(draw),
                prior: 1.0,
                noise: 0.0,
            })
            .collect();
        let mut later: Vec<(usize, &ProgramRow, f64, String)> = Vec::new();
        for (k, (f, need)) in facilities.iter_mut().zip(needs).enumerate() {
            let tech =
                find(&programs, ProgramKind::Technology, need.technology).unwrap_or_else(|| {
                    panic!("economy.toml: unknown technology {:?}", need.technology)
                });
            f.programs.push(tech);
            if let Some((row, prior, id)) = need.survey {
                if f.present {
                    f.programs.push(survey(&mut programs, row, prior, id, draw));
                } else {
                    later.push((k, row, prior, id));
                }
            }
        }

        // Sampled last, after the programs, for the same reason.
        let mut spec = |row: &PropellantRow| PropellantSpec {
            ve: row.isp_s * G0,
            lox_share: row.mixture / (1.0 + row.mixture),
            fuel_boiloff_per_day: row.fuel_boiloff_per_day.map_or(0.0, |d| d.sample(draw)),
        };
        let propellants = [
            spec(&self.propellant.methalox),
            spec(&self.propellant.hydrolox),
        ];
        for (k, reserve) in reserves {
            facilities[k].reserve = Some(reserve.sample(draw));
        }
        for (ship, row) in ships.iter_mut().zip(&self.ship) {
            ship.trip_musd += row.ops_musd_per_trip.sample(draw);
        }
        for (k, row, prior, id) in later {
            let j = survey(&mut programs, row, prior, id, draw);
            facilities[k].programs.push(j);
        }

        Economy {
            start,
            end,
            discount_rate: self.discount_rate,
            propellants,
            launch_usd_per_kg,
            equipment_usd_per_kg,
            propellant_usd_per_kg: self.earth.propellant_usd_per_kg,
            budget_musd_per_year,
            metal_steps: self
                .metals
                .steps
                .iter()
                .map(|[t, p]| (*t, p * price_factor))
                .collect(),
            surfaces: self.surfaces.iter().map(|n| node(n)).collect(),
            ships,
            facilities,
            programs,
            demand,
            asteroids,
        }
    }
}

fn find(programs: &[Program], kind: ProgramKind, id: &str) -> Option<usize> {
    programs.iter().position(|p| p.kind == kind && p.id == id)
}

/// The survey `id`, made from `row` if it is not yet among `programs`.
fn survey(
    programs: &mut Vec<Program>,
    row: &ProgramRow,
    prior: f64,
    id: String,
    draw: &mut Draw,
) -> usize {
    find(programs, ProgramKind::Survey, &id).unwrap_or_else(|| {
        programs.push(Program {
            id,
            kind: ProgramKind::Survey,
            cost_musd: row.cost_musd.sample(draw),
            years: row.years.sample(draw),
            prior,
            noise: row.noise,
        });
        programs.len() - 1
    })
}

impl From<OutputRow> for Output {
    fn from(o: OutputRow) -> Self {
        Output {
            lox: o.lox,
            ch4: o.ch4,
            lh2: o.lh2,
            metal: o.metal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Demand categories name places of the route graph (a draw skips
    /// absent ones, for graphs restricted by `plan --hub`).
    #[test]
    fn demand_nodes_are_places() {
        let graph = RouteGraph::standard();
        for d in &EconomyFile::standard().demand {
            for n in &d.nodes {
                assert!(graph.lookup(n).is_some(), "{}: no place {n}", d.id);
            }
        }
    }

    #[test]
    fn deposits_share_one_survey_and_leaner_ones_need_more_equipment() {
        let eph = Ephemeris::standard();
        let e = EconomyFile::standard().draw(eph, RouteGraph::standard(), &mut Draw::Central);
        let mut ice: Vec<&Facility> = e
            .facilities
            .iter()
            .filter(|f| f.id.starts_with("lunar-ice:"))
            .collect();
        assert!(ice.len() >= 2, "the test needs lunar ice deposits");
        ice.sort_by(|a, b| b.grade.total_cmp(&a.grade));
        for pair in ice.windows(2) {
            assert!(
                pair[1].alpha() > pair[0].alpha(),
                "{} {}",
                pair[0].id,
                pair[1].id
            );
        }
        assert!(ice.iter().all(|f| f.programs == ice[0].programs));
        assert!(ice.iter().all(|f| f.reserve.is_some()));
        let surveys = e.programs.iter().filter(|p| p.id == "survey:lunar-ice");
        assert_eq!(surveys.count(), 1);
    }

    /// The same in every build: a keyed stream's first values are part of
    /// the format (captured from this build, where `hash::it_is_fixed`'s
    /// were computed apart), the same for the same seed and key, and
    /// others for another seed or key.
    #[test]
    fn keyed_draws_are_fixed() {
        let unit = Dist::Uniform {
            low: 0.0,
            high: 1.0,
        };
        let mut a = Draw::keyed(0, "survey:lunar-ice");
        let first = unit.sample(&mut a);
        let second = unit.sample(&mut a);
        assert_eq!(first.to_bits(), KEYED_FIRST);
        assert_eq!(second.to_bits(), KEYED_SECOND);
        let mut b = Draw::keyed(0, "survey:lunar-ice");
        assert_eq!(unit.sample(&mut b), first);
        assert_ne!(unit.sample(&mut Draw::keyed(0, "survey:mars-ice")), first);
        assert_ne!(unit.sample(&mut Draw::keyed(1, "survey:lunar-ice")), first);
        assert!(Draw::keyed(0, "x").chance(1.0));
        assert!(!Draw::keyed(0, "x").chance(0.0));
    }

    const KEYED_FIRST: u64 = 4_599_573_686_303_640_636;
    const KEYED_SECOND: u64 = 4_600_259_522_190_956_308;

    /// A world where the draw left a resource out keeps its options and
    /// their surveys, so that a plan naming one reads in every world; the
    /// surveys of absent options come after the others, sampled last.
    #[test]
    fn absent_options_are_kept() {
        let eph = Ephemeris::standard();
        // Phobos is present with probability 0.4: dry in the central world.
        let e = EconomyFile::standard().draw(eph, RouteGraph::standard(), &mut Draw::Central);
        let phobos = e
            .facilities
            .iter()
            .find(|f| f.id == "phobos-volatiles")
            .expect("the option is there");
        assert!(!phobos.present);
        assert_eq!(phobos.prior.present, 0.4);
        assert_eq!(phobos.prior.grade, 0.06);
        assert_eq!(phobos.grade, 0.06, "an absent option takes the central");
        let survey = &e.programs[phobos.programs[1]];
        assert_eq!(survey.id, "survey:phobos-volatiles");
        assert_eq!(survey.kind, ProgramKind::Survey);
        assert_eq!(survey.noise, 0.4);
        let of_present: Vec<bool> = e
            .programs
            .iter()
            .enumerate()
            .filter(|(_, p)| p.kind == ProgramKind::Survey)
            .map(|(j, _)| {
                e.facilities
                    .iter()
                    .any(|f| f.present && f.programs.contains(&j))
            })
            .collect();
        assert!(of_present.contains(&true) && of_present.contains(&false));
        assert!(of_present.iter().skip_while(|p| **p).all(|p| !*p));
        // Every asteroid has an option of each kind, present as drawn.
        assert!(!e.asteroids.is_empty());
        for a in &e.asteroids {
            for kind in ["volatiles", "pgm"] {
                let id = format!("{kind}@{}", a.id);
                let f = e
                    .facilities
                    .iter()
                    .find(|f| f.id == id)
                    .expect("both kinds");
                assert_eq!(f.present, a.composition.as_deref() == Some(kind), "{id}");
                assert_eq!(f.programs.len(), 2);
            }
        }
    }
}
