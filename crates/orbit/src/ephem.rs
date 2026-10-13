//! Ephemerides, in the ecliptic and mean equinox of J2000, TDB.
//!
//! The Sun, the planets and the Moon come from JPL's DE440 for 2025–2080
//! (`data/de440.bin`, built by `tools/ephem/`): Chebyshev series for the
//! Sun, the planetary system barycenters, the Earth–Moon barycenter and the
//! Moon, as DE440 has them. Outer planets are their system barycenters.
//! Small bodies use piecewise two-body orbits fitted to JPL Horizons over the
//! game's span (`data/neas.toml`), until M1a integrates them.

use crate::consts::AU_KM;
use crate::kepler::{Elements, State};
use crate::time::{Epoch, SECONDS_PER_DAY};
use glam::DVec3;
use serde::Deserialize;

/// Obliquity of the ecliptic of J2000 (IAU 1976, as JPL Horizons uses): the
/// angle between this module's frame and the ICRF equator.
pub const OBLIQUITY_J2000: f64 = 84_381.448 / 3_600.0 * std::f64::consts::PI / 180.0;

/// A body's identity in an [`Ephemeris`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyId(usize);

#[derive(Clone, Debug)]
pub struct Body {
    pub id: String,
    pub name: String,
    pub kind: BodyKind,
    /// Gravitational parameter, km³/s²; zero for small bodies.
    pub gm: f64,
    /// Oblateness, where the game models it.
    pub figure: Option<Figure>,
    motion: Motion,
}

/// A body's oblateness: the J2 term of its gravity field.
#[derive(Clone, Copy, Debug)]
pub struct Figure {
    pub j2: f64,
    /// Reference radius of `j2`, km.
    pub radius: f64,
    /// Unit vector along the rotation axis.
    pub pole: DVec3,
}

#[derive(Clone, Debug)]
pub enum BodyKind {
    Star,
    Planet,
    Moon,
    SmallBody {
        /// Spectral class (SMASSII/Tholen), or "unknown".
        spectral: String,
        diameter_km: Option<f64>,
        h_mag: Option<f64>,
    },
}

impl BodyKind {
    /// On its own orbit about the Sun: what heliocentric transfers connect.
    pub fn orbits_sun(&self) -> bool {
        matches!(self, BodyKind::Planet | BodyKind::SmallBody { .. })
    }
}

#[derive(Clone, Debug)]
enum Motion {
    /// A DE440 series, relative to the solar-system barycenter.
    Series(usize),
    /// The Earth–Moon barycenter less the Moon's offset from it / EMRAT.
    Earth,
    /// The Earth–Moon barycenter plus the Moon's offset from it.
    Moon,
    /// Heliocentric two-body orbits, sorted by start; the first and last
    /// extend before and after the fitted span.
    Segments {
        epoch: Epoch,
        segments: Vec<Segment>,
    },
}

#[derive(Clone, Debug)]
struct Segment {
    start: Epoch,
    /// Elements at the body's common epoch.
    elements: Elements,
}

/// One body's DE440 Chebyshev series: records of `intlen` seconds from
/// `init` (TDB seconds past J2000), each `ncoef` coefficients for x, y, z.
#[derive(Clone, Debug)]
struct Series {
    init: f64,
    intlen: f64,
    ncoef: usize,
    coef: Vec<f64>,
}

impl Series {
    /// The record covering `t`, and `t` scaled to [-1, 1] within it.
    fn record(&self, t: Epoch) -> (&[f64], f64) {
        let n = self.ncoef;
        let sec = t.seconds() - self.init;
        let k = (sec / self.intlen).floor();
        assert!(
            k >= 0.0 && (k as usize) < self.coef.len() / (3 * n),
            "{t} is outside the DE440 tables (2025–2080)"
        );
        let k = k as usize;
        let s = 2.0 * (sec - k as f64 * self.intlen) / self.intlen - 1.0;
        (&self.coef[3 * n * k..3 * n * (k + 1)], s)
    }

    /// Position by Clenshaw's recurrence, all three coordinates together.
    fn position(&self, t: Epoch) -> DVec3 {
        let n = self.ncoef;
        let (c, s) = self.record(t);
        let coef = |j: usize| DVec3::new(c[j], c[n + j], c[2 * n + j]);
        let (mut b1, mut b2) = (DVec3::ZERO, DVec3::ZERO);
        for j in (1..n).rev() {
            (b1, b2) = (2.0 * s * b1 - b2 + coef(j), b1);
        }
        s * b1 - b2 + coef(0)
    }

    fn state(&self, t: Epoch) -> State {
        let n = self.ncoef;
        let (c, s) = self.record(t);
        let coef = |j: usize| DVec3::new(c[j], c[n + j], c[2 * n + j]);
        // Chebyshev polynomials T_j(s) and their derivatives.
        let mut r = coef(0);
        let mut v = DVec3::ZERO;
        let (mut t_prev, mut t_cur) = (1.0, s);
        let (mut d_prev, mut d_cur) = (0.0, 1.0);
        for j in 1..n {
            r += coef(j) * t_cur;
            v += coef(j) * d_cur;
            let t_next = 2.0 * s * t_cur - t_prev;
            let d_next = 2.0 * t_cur + 2.0 * s * d_cur - d_prev;
            (t_prev, t_cur) = (t_cur, t_next);
            (d_prev, d_cur) = (d_cur, d_next);
        }
        State {
            r,
            v: v * (2.0 / self.intlen),
        }
    }
}

/// `data/de440.bin`: the constants and series `tools/ephem/build.py` wrote.
struct De440 {
    constants: Vec<(String, f64)>,
    /// (NAIF target, NAIF center, series).
    series: Vec<(i32, i32, Series)>,
}

impl De440 {
    fn parse(bytes: &[u8]) -> De440 {
        let mut at = 0;
        let mut take = |n: usize| {
            let s = &bytes[at..at + n];
            at += n;
            s
        };
        assert_eq!(take(8), b"DE440ECL", "not a DE440 table file");
        let u32_ = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap());
        let f64_ = |b: &[u8]| f64::from_le_bytes(b.try_into().unwrap());
        assert_eq!(u32_(take(4)), 1, "unknown DE440 table version");
        let constants = (0..u32_(take(4)))
            .map(|_| {
                let name = take(16);
                let name = String::from_utf8_lossy(name)
                    .trim_end_matches('\0')
                    .to_string();
                (name, f64_(take(8)))
            })
            .collect();
        let series = (0..u32_(take(4)))
            .map(|_| {
                let target = u32_(take(4)) as i32;
                let center = u32_(take(4)) as i32;
                let init = f64_(take(8));
                let intlen = f64_(take(8));
                let ncoef = u32_(take(4)) as usize;
                let nrec = u32_(take(4)) as usize;
                let coef = take(8 * 3 * ncoef * nrec)
                    .chunks_exact(8)
                    .map(f64_)
                    .collect();
                let s = Series {
                    init,
                    intlen,
                    ncoef,
                    coef,
                };
                (target, center, s)
            })
            .collect();
        De440 { constants, series }
    }

    fn constant(&self, name: &str) -> f64 {
        self.constants
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("DE440 constant {name} missing"))
            .1
    }

    /// Index of the series for `target` relative to `center`.
    fn series_index(&self, target: i32, center: i32) -> usize {
        self.series
            .iter()
            .position(|(t, c, _)| (*t, *c) == (target, center))
            .unwrap_or_else(|| panic!("DE440 series {target} wrt {center} missing"))
    }
}

/// The Sun, the planets, the Moon, and the game's small bodies.
#[derive(Clone, Debug)]
pub struct Ephemeris {
    bodies: Vec<Body>,
    series: Vec<Series>,
    emb: usize,
    moon_offset: usize,
    emrat: f64,
    sun: BodyId,
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
    /// DE440 plus the small bodies in `data/neas.toml`.
    pub fn standard() -> Self {
        Self::with_small_bodies(include_str!("../../../data/neas.toml"))
            .expect("data/neas.toml is valid")
    }

    /// DE440 plus small bodies from a TOML document shaped like
    /// `data/neas.toml`.
    pub fn with_small_bodies(toml_src: &str) -> Result<Self, String> {
        let de = De440::parse(include_bytes!("../../../data/de440.bin"));
        let emrat = de.constant("EMRAT");
        let gmb = de.constant("GMB");
        let mut bodies = Vec::new();
        let mut massive = |id: &str, kind: BodyKind, gm: f64, motion: Motion| {
            let mut name = id.to_string();
            name[..1].make_ascii_uppercase();
            bodies.push(Body {
                id: id.to_string(),
                name,
                kind,
                gm,
                figure: None,
                motion,
            });
        };
        let series = |naif: i32| Motion::Series(de.series_index(naif, 0));
        massive("sun", BodyKind::Star, de.constant("GMS"), series(10));
        massive("mercury", BodyKind::Planet, de.constant("GM1"), series(1));
        massive("venus", BodyKind::Planet, de.constant("GM2"), series(2));
        massive(
            "earth",
            BodyKind::Planet,
            gmb * emrat / (1.0 + emrat),
            Motion::Earth,
        );
        massive("moon", BodyKind::Moon, gmb / (1.0 + emrat), Motion::Moon);
        for (id, naif) in [
            ("mars", 4),
            ("jupiter", 5),
            ("saturn", 6),
            ("uranus", 7),
            ("neptune", 8),
        ] {
            massive(
                id,
                BodyKind::Planet,
                de.constant(&format!("GM{naif}")),
                series(naif),
            );
        }
        // Earth's mean pole of J2000 is the ICRF z axis; precession moves it
        // ~0.7° over 2025–2080, which this ignores.
        let earth = bodies.iter_mut().find(|b| b.id == "earth").unwrap();
        earth.figure = Some(Figure {
            j2: de.constant("J2E"),
            radius: de.constant("RE"),
            pole: DVec3::new(0.0, -OBLIQUITY_J2000.sin(), OBLIQUITY_J2000.cos()),
        });

        let file: SmallBodyFile = toml::from_str(toml_src).map_err(|e| e.to_string())?;
        let epoch = Epoch::from_jd(file.epoch_jd_tdb);
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
                gm: 0.0,
                figure: None,
                motion: Motion::Segments { epoch, segments },
            });
        }
        let emb = de.series_index(3, 0);
        let moon_offset = de.series_index(301, 3);
        Ok(Ephemeris {
            bodies,
            series: de.series.into_iter().map(|(_, _, s)| s).collect(),
            emb,
            moon_offset,
            emrat,
            sun: BodyId(0),
        })
    }

    pub fn lookup(&self, id: &str) -> Option<BodyId> {
        self.bodies.iter().position(|b| b.id == id).map(BodyId)
    }

    pub fn body(&self, id: BodyId) -> &Body {
        &self.bodies[id.0]
    }

    pub fn ids(&self) -> impl Iterator<Item = BodyId> {
        (0..self.bodies.len()).map(BodyId)
    }

    pub fn sun(&self) -> BodyId {
        self.sun
    }

    /// Position relative to the solar-system barycenter: [`Self::barycentric`]
    /// without the velocity, for evaluating forces.
    pub fn barycentric_position(&self, id: BodyId, t: Epoch) -> DVec3 {
        match &self.bodies[id.0].motion {
            Motion::Series(k) => self.series[*k].position(t),
            Motion::Earth | Motion::Moon => {
                let emb = self.series[self.emb].position(t);
                let off = self.series[self.moon_offset].position(t);
                match self.bodies[id.0].motion {
                    Motion::Earth => emb - off / self.emrat,
                    _ => emb + off,
                }
            }
            Motion::Segments { .. } => self.state(id, t).r + self.barycentric_position(self.sun, t),
        }
    }

    /// State relative to the solar-system barycenter.
    pub fn barycentric(&self, id: BodyId, t: Epoch) -> State {
        match &self.bodies[id.0].motion {
            Motion::Series(k) => self.series[*k].state(t),
            Motion::Earth | Motion::Moon => {
                let emb = self.series[self.emb].state(t);
                let off = self.series[self.moon_offset].state(t);
                let f = match self.bodies[id.0].motion {
                    Motion::Earth => -1.0 / self.emrat,
                    _ => 1.0,
                };
                State {
                    r: emb.r + f * off.r,
                    v: emb.v + f * off.v,
                }
            }
            Motion::Segments { .. } => {
                let (h, sun) = (self.state(id, t), self.barycentric(self.sun, t));
                State {
                    r: h.r + sun.r,
                    v: h.v + sun.v,
                }
            }
        }
    }

    /// State relative to the Sun.
    pub fn state(&self, id: BodyId, t: Epoch) -> State {
        if let Motion::Segments { epoch, segments } = &self.bodies[id.0].motion {
            return segment_elements(*epoch, segments, t).state();
        }
        let (b, sun) = (self.barycentric(id, t), self.barycentric(self.sun, t));
        State {
            r: b.r - sun.r,
            v: b.v - sun.v,
        }
    }

    /// Heliocentric state of the body's planetary system: the Earth–Moon
    /// barycenter for Earth and the Moon, the body itself otherwise (outer
    /// planets are their system barycenters already). A ship that escapes a
    /// system leaves with excess speed relative to this, and heliocentric
    /// transfers connect these states.
    pub fn system_state(&self, id: BodyId, t: Epoch) -> State {
        match self.bodies[id.0].motion {
            Motion::Earth | Motion::Moon => {
                let (emb, sun) = (
                    self.series[self.emb].state(t),
                    self.barycentric(self.sun, t),
                );
                State {
                    r: emb.r - sun.r,
                    v: emb.v - sun.v,
                }
            }
            _ => self.state(id, t),
        }
    }

    /// Heliocentric orbital elements of the body's planetary system (see
    /// [`Self::system_state`]): the fitted ones for small bodies, osculating
    /// ones about the Sun and the system's mass otherwise. Panics for the
    /// Sun.
    pub fn elements(&self, id: BodyId, t: Epoch) -> Elements {
        let body = &self.bodies[id.0];
        let gm = match &body.motion {
            Motion::Segments { epoch, segments } => return segment_elements(*epoch, segments, t),
            Motion::Earth | Motion::Moon => self
                .bodies
                .iter()
                .filter(|b| matches!(b.motion, Motion::Earth | Motion::Moon))
                .map(|b| b.gm)
                .sum(),
            Motion::Series(_) => {
                assert_ne!(id, self.sun, "the Sun has no heliocentric orbit");
                body.gm
            }
        };
        Elements::from_state(self.system_state(id, t), self.bodies[self.sun.0].gm + gm)
            .unwrap_or_else(|| panic!("{} is not on a heliocentric ellipse", body.id))
    }

    /// Synodic period of two bodies, in days (infinite for equal periods).
    pub fn synodic_period_days(&self, a: BodyId, b: BodyId, t: Epoch) -> f64 {
        let n = |id: BodyId| self.elements(id, t).mean_motion * SECONDS_PER_DAY;
        std::f64::consts::TAU / (n(a) - n(b)).abs()
    }
}

fn segment_elements(epoch: Epoch, segments: &[Segment], t: Epoch) -> Elements {
    let k = segments.partition_point(|s| s.start <= t).saturating_sub(1);
    segments[k]
        .elements
        .advanced((t.0 - epoch.0) * SECONDS_PER_DAY)
}
