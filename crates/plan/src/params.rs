//! The economy's parameters (`data/economy.toml`) and draws of them: a
//! [`Scenario`] is one game's worth of concrete numbers.

use crate::calendar::{Calendar, DAYS_PER_YEAR};
use orbit::ephem::{BodyKind, Ephemeris};
use orbit::network::{Network, NodeId};
use orbit::time::Epoch;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Normal};
use serde::Deserialize;
use std::f64::consts::PI;

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

    fn sample(&self, rng: &mut Draw) -> f64 {
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

    /// True with probability `p`; the central draw takes the likelier side.
    fn chance(&mut self, p: f64) -> bool {
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

/// A fixed cost paid before a plant: spent at no more than `cost_musd /
/// years` per year, after which the plant may be installed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProgramRow {
    cost_musd: Dist,
    years: Dist,
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
    pub lands: Vec<NodeId>,
    pub turnaround_days: f64,
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

/// A kind of plant at one site, as drawn.
#[derive(Clone, Debug)]
pub struct FacilityOption {
    pub id: String,
    pub node: NodeId,
    /// Per t/yr of rated output.
    pub output: Output,
    /// Equipment, t per t/yr of output.
    pub alpha: f64,
    pub grade: f64,
    pub build_periods: usize,
    /// Cumulative output the deposit can yield, t.
    pub reserve: Option<f64>,
    /// Indices into [`Scenario::programs`] that must be paid before the
    /// plant is installed.
    pub programs: Vec<usize>,
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
    /// $M. A survey's is its mission cost divided by the prior probability
    /// that the deposit has the resource: the expected cost of finding one.
    pub cost_musd: f64,
    /// Spending is at most `cost_musd / lead_periods` per period.
    pub lead_periods: usize,
}

#[derive(Clone, Debug)]
pub struct DemandSegment {
    pub usd_per_kg: f64,
    /// t/yr at the start; grows with the category.
    pub tpy: f64,
}

#[derive(Clone, Debug)]
pub struct DemandCategory {
    pub id: String,
    pub what: Demanded,
    pub nodes: Vec<NodeId>,
    pub segments: Vec<DemandSegment>,
    pub growth: f64,
}

/// What a draw decided about one asteroid.
#[derive(Clone, Debug)]
pub struct AsteroidDraw {
    pub id: String,
    pub composition: Option<String>,
    pub mass_t: f64,
}

#[derive(Clone, Debug)]
pub struct Scenario {
    pub cal: Calendar,
    pub discount_rate: f64,
    /// By [`Propellant`].
    pub propellants: [PropellantSpec; 2],
    /// $/kg to LEO, per period.
    pub launch_usd_per_kg: Vec<f64>,
    pub equipment_usd_per_kg: f64,
    pub propellant_usd_per_kg: f64,
    pub budget_musd_per_year: f64,
    /// `(tonnes, usd_per_kg)` by cumulative sales.
    pub metal_steps: Vec<(f64, f64)>,
    pub surfaces: Vec<NodeId>,
    pub ships: Vec<ShipClass>,
    pub facilities: Vec<FacilityOption>,
    pub programs: Vec<Program>,
    pub demand: Vec<DemandCategory>,
    pub asteroids: Vec<AsteroidDraw>,
}

impl Scenario {
    pub fn propellant(&self, p: Propellant) -> &PropellantSpec {
        &self.propellants[p as usize]
    }

    /// Discount factor of period `p`, at its middle.
    pub fn discount(&self, p: usize) -> f64 {
        (1.0 + self.discount_rate).powf(-self.cal.years_at(p))
    }
}

impl EconomyFile {
    pub fn standard() -> Self {
        toml::from_str(include_str!("../../../data/economy.toml"))
            .expect("data/economy.toml is valid")
    }

    /// One draw of the economy, on a calendar of `period_months` steps.
    pub fn scenario(
        &self,
        eph: &Ephemeris,
        net: &Network,
        draw: &mut Draw,
        period_months: u32,
    ) -> Scenario {
        let node = |id: &str| {
            net.lookup(id)
                .unwrap_or_else(|| panic!("economy.toml: unknown node {id:?}"))
        };
        let start: Epoch = self.start.parse().expect("economy.toml: start");
        let end: Epoch = self.end.parse().expect("economy.toml: end");
        let cal = Calendar::with_period(start, end, period_months as f64 * DAYS_PER_YEAR / 12.0);

        let (l0, l1) = (
            self.earth.launch_usd_per_kg_start.sample(draw),
            self.earth.launch_usd_per_kg_end.sample(draw),
        );
        let launch_usd_per_kg = (0..cal.periods)
            .map(|p| l0 * (l1 / l0).powf(p as f64 / (cal.periods - 1) as f64))
            .collect();

        let ships = self
            .ship
            .iter()
            .map(|s| ShipClass {
                id: s.id.clone(),
                dry: s.dry_t,
                tank: s.propellant_t,
                cargo: s.cargo_t,
                price_musd: s.price_musd.sample(draw),
                propellant: s.propellant,
                aero: s.aero,
                interplanetary: s.interplanetary,
                lands: s.lands.iter().map(|n| node(n)).collect(),
                turnaround_days: s.turnaround_days,
            })
            .collect();

        let build_periods = |months: f64| (months / 12.0 / cal.period_years()).round() as usize;
        let alpha = |e: &EquipmentRow, grade: f64, draw: &mut Draw| {
            e.plant.sample(draw) + e.ore.sample(draw) / grade
        };
        // Each plant's technology and survey, and each deposit's size, are
        // sampled after everything else so that adding them left the other
        // parameters of each seed as they were.
        let mut needs: Vec<Need> = Vec::new();
        let mut reserves: Vec<(usize, &Dist)> = Vec::new();
        let mut facilities = Vec::new();
        for f in &self.facility {
            let present = draw.chance(f.present);
            let grade = f.grade.sample(draw);
            let plant = f.equipment_t_per_tpy.plant.sample(draw);
            let ore = f.equipment_t_per_tpy.ore.sample(draw);
            if !present {
                continue;
            }
            let option = |id: String, grade: f64| FacilityOption {
                id,
                node: node(&f.node),
                output: f.output.into(),
                alpha: plant + ore / grade,
                grade,
                build_periods: build_periods(f.build_months),
                reserve: None,
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
                facilities.push(option(f.id.clone(), grade));
                needs.push(need());
            }
            for d in &f.deposits {
                reserves.push((facilities.len(), &d.reserve_t));
                facilities.push(option(format!("{}:{}", f.id, d.id), grade * d.grade_factor));
                needs.push(need());
            }
        }

        let mut asteroids = Vec::new();
        for (_, n) in net.nodes() {
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
            let composition = plants.get(k).map(|p| {
                let grade = p.grade.sample(draw);
                facilities.push(FacilityOption {
                    id: format!("{}@{}", p.id, n.id),
                    node: net.lookup(&n.id).unwrap(),
                    output: p.output.into(),
                    alpha: alpha(&p.equipment_t_per_tpy, grade, draw),
                    grade,
                    build_periods: build_periods(p.build_months),
                    reserve: Some(mass_t * p.minable_fraction * grade),
                    programs: Vec::new(),
                });
                needs.push(Need {
                    technology: &p.technology,
                    survey: Some((&p.survey, weights[k], format!("survey:{}@{}", p.id, n.id))),
                });
                p.id.clone()
            });
            asteroids.push(AsteroidDraw {
                id: n.id.clone(),
                composition,
                mass_t,
            });
        }

        let mut demand = Vec::new();
        for d in &self.demand {
            let ref_tpy = d.ref_tpy.sample(draw);
            let q = |p: f64| ref_tpy * (p / d.ref_usd_per_kg).powf(-d.elasticity);
            let mut segments = Vec::new();
            let mut hi = d.max_usd_per_kg;
            while hi > d.min_usd_per_kg * 1.000_001 {
                let lo = hi / 2.0;
                segments.push(DemandSegment {
                    usd_per_kg: (hi * lo).sqrt(),
                    tpy: q(lo) - q(hi),
                });
                hi = lo;
            }
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
                demand.push(DemandCategory {
                    id,
                    what,
                    nodes: d.nodes.iter().map(|n| node(n)).collect(),
                    segments: segments
                        .iter()
                        .map(|s| DemandSegment {
                            usd_per_kg: s.usd_per_kg,
                            tpy: s.tpy * share,
                        })
                        .collect(),
                    growth: d.growth,
                });
            }
        }

        let price_factor = self.metals.price_factor.sample(draw);
        let equipment_usd_per_kg = self.earth.equipment_usd_per_kg.sample(draw);
        let budget_musd_per_year = self.budget.musd_per_year.sample(draw);

        let lead_periods =
            |years: f64| (years / cal.period_years() - 1e-9).ceil().max(1.0) as usize;
        let mut programs: Vec<Program> = self
            .technology
            .iter()
            .map(|t| Program {
                id: t.id.clone(),
                kind: ProgramKind::Technology,
                cost_musd: t.cost_musd.sample(draw),
                lead_periods: lead_periods(t.years.sample(draw)),
            })
            .collect();
        for (f, need) in facilities.iter_mut().zip(needs) {
            let find = |programs: &[Program], kind, id: &str| {
                programs.iter().position(|p| p.kind == kind && p.id == id)
            };
            let tech =
                find(&programs, ProgramKind::Technology, need.technology).unwrap_or_else(|| {
                    panic!("economy.toml: unknown technology {:?}", need.technology)
                });
            f.programs.push(tech);
            if let Some((row, p, id)) = need.survey {
                let survey = find(&programs, ProgramKind::Survey, &id).unwrap_or_else(|| {
                    programs.push(Program {
                        id,
                        kind: ProgramKind::Survey,
                        cost_musd: row.cost_musd.sample(draw) / p,
                        lead_periods: lead_periods(row.years.sample(draw)),
                    });
                    programs.len() - 1
                });
                f.programs.push(survey);
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

        Scenario {
            cal,
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

    #[test]
    fn deposits_share_one_survey_and_leaner_ones_need_more_equipment() {
        let eph = Ephemeris::standard();
        let net = Network::standard(&eph);
        let sc = EconomyFile::standard().scenario(&eph, &net, &mut Draw::Central, 6);
        let mut ice: Vec<&FacilityOption> = sc
            .facilities
            .iter()
            .filter(|f| f.id.starts_with("lunar-ice:"))
            .collect();
        assert!(ice.len() >= 2, "the test needs lunar ice deposits");
        ice.sort_by(|a, b| b.grade.total_cmp(&a.grade));
        for pair in ice.windows(2) {
            assert!(
                pair[1].alpha > pair[0].alpha,
                "{} {}",
                pair[0].id,
                pair[1].id
            );
        }
        assert!(ice.iter().all(|f| f.programs == ice[0].programs));
        assert!(ice.iter().all(|f| f.reserve.is_some()));
        let surveys = sc.programs.iter().filter(|p| p.id == "survey:lunar-ice");
        assert_eq!(surveys.count(), 1);
    }
}
