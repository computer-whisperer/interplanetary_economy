//! Heliocentric ephemerides, in the ecliptic and mean equinox of J2000.
//!
//! Planets use JPL's approximate Keplerian elements (Standish & Williams,
//! Table 2a/2b, valid 3000 BC – 3000 AD). Small bodies use piecewise two-body
//! orbits fitted to JPL Horizons over the game's span (`data/neas.toml`).
//! Both are deliberately approximate: the game needs the real geometry of the
//! solar system, not navigation-grade positions.

use crate::consts::AU_KM;
use crate::kepler::{Elements, State};
use crate::time::{DAYS_PER_CENTURY, Epoch, SECONDS_PER_DAY};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Planet {
    Mercury,
    Venus,
    /// The Earth–Moon barycenter; the game treats it as Earth's position.
    Earth,
    Mars,
    Jupiter,
    Saturn,
    Uranus,
    Neptune,
}

/// One row of Standish Table 2a: value at J2000 and rate per Julian century.
/// a [au], e, I [deg], L [deg], long. perihelion [deg], long. node [deg].
struct Standish {
    a: [f64; 2],
    e: [f64; 2],
    i: [f64; 2],
    l: [f64; 2],
    varpi: [f64; 2],
    node: [f64; 2],
    /// Table 2b terms b, c, s, f for Jupiter–Neptune.
    bcsf: [f64; 4],
}

#[rustfmt::skip]
const STANDISH: [Standish; 8] = [
    Standish { a: [0.38709843, 0.0], e: [0.20563661, 0.00002123], i: [7.00559432, -0.00590158],
        l: [252.25166724, 149472.67486623], varpi: [77.45771895, 0.15940013],
        node: [48.33961819, -0.12214182], bcsf: [0.0; 4] },
    Standish { a: [0.72332102, -0.00000026], e: [0.00676399, -0.00005107], i: [3.39777545, 0.00043494],
        l: [181.97970850, 58517.81560260], varpi: [131.76755713, 0.05679648],
        node: [76.67261496, -0.27274174], bcsf: [0.0; 4] },
    Standish { a: [1.00000018, -0.00000003], e: [0.01673163, -0.00003661], i: [-0.00054346, -0.01337178],
        l: [100.46691572, 35999.37306329], varpi: [102.93005885, 0.31795260],
        node: [-5.11260389, -0.24123856], bcsf: [0.0; 4] },
    Standish { a: [1.52371243, 0.00000097], e: [0.09336511, 0.00009149], i: [1.85181869, -0.00724757],
        l: [-4.56813164, 19140.29934243], varpi: [-23.91744784, 0.45223625],
        node: [49.71320984, -0.26852431], bcsf: [0.0; 4] },
    Standish { a: [5.20248019, -0.00002864], e: [0.04853590, 0.00018026], i: [1.29861416, -0.00322699],
        l: [34.33479152, 3034.90371757], varpi: [14.27495244, 0.18199196],
        node: [100.29282654, 0.13024619], bcsf: [-0.00012452, 0.06064060, -0.35635438, 38.35125000] },
    Standish { a: [9.54149883, -0.00003065], e: [0.05550825, -0.00032044], i: [2.49424102, 0.00451969],
        l: [50.07571329, 1222.11494724], varpi: [92.86136063, 0.54179478],
        node: [113.63998702, -0.25015002], bcsf: [0.00025899, -0.13434469, 0.87320147, 38.35125000] },
    Standish { a: [19.18797948, -0.00020455], e: [0.04685740, -0.00001550], i: [0.77298127, -0.00180155],
        l: [314.20276625, 428.49512595], varpi: [172.43404441, 0.09266985],
        node: [73.96250215, 0.05739699], bcsf: [0.00058331, -0.97731848, 0.17689245, 7.67025000] },
    Standish { a: [30.06952752, 0.00006447], e: [0.00895439, 0.00000818], i: [1.77005520, 0.00022400],
        l: [304.22289287, 218.46515314], varpi: [46.68158724, 0.01009938],
        node: [131.78635853, -0.00606302], bcsf: [-0.00041348, 0.68346318, -0.10162547, 7.67025000] },
];

impl Planet {
    pub const ALL: [Planet; 8] = [
        Planet::Mercury,
        Planet::Venus,
        Planet::Earth,
        Planet::Mars,
        Planet::Jupiter,
        Planet::Saturn,
        Planet::Uranus,
        Planet::Neptune,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Planet::Mercury => "mercury",
            Planet::Venus => "venus",
            Planet::Earth => "earth",
            Planet::Mars => "mars",
            Planet::Jupiter => "jupiter",
            Planet::Saturn => "saturn",
            Planet::Uranus => "uranus",
            Planet::Neptune => "neptune",
        }
    }

    /// Osculating-equivalent elements at `t`.
    pub fn elements(self, t: Epoch) -> Elements {
        let p = &STANDISH[self as usize];
        let tc = t.centuries();
        let at = |x: [f64; 2]| x[0] + x[1] * tc;
        let [b, c, s, f] = p.bcsf;
        let (l, varpi, node) = (at(p.l), at(p.varpi), at(p.node));
        let m = l - varpi + b * tc * tc + c * (f * tc).to_radians().cos() + s * (f * tc).to_radians().sin();
        Elements {
            a: at(p.a) * AU_KM,
            e: at(p.e),
            i: at(p.i).to_radians(),
            raan: node.to_radians(),
            argp: (varpi - node).to_radians(),
            mean_anomaly: m.to_radians(),
            // The mean-longitude rate: it includes perihelion precession, so
            // velocities track the tabulated positions.
            mean_motion: p.l[1].to_radians() / (DAYS_PER_CENTURY * SECONDS_PER_DAY),
        }
    }
}

/// A body's identity in an [`Ephemeris`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyId(usize);

#[derive(Clone, Debug)]
pub struct Body {
    pub id: String,
    pub name: String,
    pub kind: BodyKind,
    motion: Motion,
}

#[derive(Clone, Debug)]
pub enum BodyKind {
    Planet,
    SmallBody {
        /// Spectral class (SMASSII/Tholen), or "unknown".
        spectral: String,
        diameter_km: Option<f64>,
        h_mag: Option<f64>,
    },
}

#[derive(Clone, Debug)]
enum Motion {
    Planet(Planet),
    /// Consecutive two-body orbits, sorted by start; the first and last
    /// extend before and after the fitted span.
    Segments { epoch: Epoch, segments: Vec<Segment> },
}

#[derive(Clone, Debug)]
struct Segment {
    start: Epoch,
    /// Elements at the body's common epoch.
    elements: Elements,
}

impl Body {
    pub fn state(&self, t: Epoch) -> State {
        self.elements(t).state()
    }

    pub fn elements(&self, t: Epoch) -> Elements {
        match &self.motion {
            Motion::Planet(p) => p.elements(t),
            Motion::Segments { epoch, segments } => {
                let k = segments.partition_point(|s| s.start <= t).saturating_sub(1);
                segments[k].elements.advanced((t.0 - epoch.0) * SECONDS_PER_DAY)
            }
        }
    }
}

/// Every heliocentric body the game knows about.
#[derive(Clone, Debug)]
pub struct Ephemeris {
    bodies: Vec<Body>,
}

#[derive(Deserialize)]
struct SmallBodyFile {
    epoch_jd_tdb: f64,
    body: Vec<SmallBodyRow>,
}

#[derive(Deserialize)]
struct SmallBodyRow {
    id: String,
    name: String,
    spectral: String,
    diameter_km: Option<f64>,
    h_mag: Option<f64>,
    segment: Vec<SegmentRow>,
}

#[derive(Deserialize)]
struct SegmentRow {
    start_jd: f64,
    a_au: f64,
    e: f64,
    i_deg: f64,
    node_deg: f64,
    peri_deg: f64,
    mean_anomaly_deg: f64,
    mean_motion_deg_per_day: f64,
}

impl Ephemeris {
    /// The planets plus the small bodies in `data/neas.toml`.
    pub fn standard() -> Self {
        Self::with_small_bodies(include_str!("../../../data/neas.toml"))
            .expect("data/neas.toml is valid")
    }

    /// The planets plus small bodies from a TOML document shaped like
    /// `data/neas.toml`.
    pub fn with_small_bodies(toml_src: &str) -> Result<Self, String> {
        let file: SmallBodyFile = toml::from_str(toml_src).map_err(|e| e.to_string())?;
        let epoch = Epoch::from_jd(file.epoch_jd_tdb);
        let mut bodies: Vec<Body> = Planet::ALL
            .iter()
            .map(|&p| Body {
                id: p.id().to_string(),
                name: capitalize(p.id()),
                kind: BodyKind::Planet,
                motion: Motion::Planet(p),
            })
            .collect();
        for row in file.body {
            if bodies.iter().any(|b| b.id == row.id) {
                return Err(format!("duplicate body id {:?}", row.id));
            }
            if row.segment.is_empty() {
                return Err(format!("body {:?} has no orbit segments", row.id));
            }
            let mut segments: Vec<Segment> = row
                .segment
                .iter()
                .map(|s| Segment {
                    start: Epoch::from_jd(s.start_jd),
                    elements: Elements {
                        a: s.a_au * AU_KM,
                        e: s.e,
                        i: s.i_deg.to_radians(),
                        raan: s.node_deg.to_radians(),
                        argp: s.peri_deg.to_radians(),
                        mean_anomaly: s.mean_anomaly_deg.to_radians(),
                        mean_motion: s.mean_motion_deg_per_day.to_radians() / SECONDS_PER_DAY,
                    },
                })
                .collect();
            segments.sort_by(|a, b| a.start.0.total_cmp(&b.start.0));
            bodies.push(Body {
                id: row.id,
                name: row.name,
                kind: BodyKind::SmallBody {
                    spectral: row.spectral,
                    diameter_km: row.diameter_km,
                    h_mag: row.h_mag,
                },
                motion: Motion::Segments { epoch, segments },
            });
        }
        Ok(Ephemeris { bodies })
    }

    pub fn lookup(&self, id: &str) -> Option<BodyId> {
        self.bodies.iter().position(|b| b.id == id).map(BodyId)
    }

    pub fn body(&self, id: BodyId) -> &Body {
        &self.bodies[id.0]
    }

    pub fn state(&self, id: BodyId, t: Epoch) -> State {
        self.bodies[id.0].state(t)
    }

    pub fn ids(&self) -> impl Iterator<Item = BodyId> {
        (0..self.bodies.len()).map(BodyId)
    }

    /// Synodic period of two bodies, in days (infinite for equal periods).
    pub fn synodic_period_days(&self, a: BodyId, b: BodyId, t: Epoch) -> f64 {
        let n = |id: BodyId| self.body(id).elements(t).mean_motion * SECONDS_PER_DAY;
        std::f64::consts::TAU / (n(a) - n(b)).abs()
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}
