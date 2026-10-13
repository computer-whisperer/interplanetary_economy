//! Ephemerides, in the ecliptic and mean equinox of J2000, TDB.
//!
//! The Sun, the planets and the Moon come from JPL's DE440 for 2025–2080
//! (`data/de440.bin`, built by `tools/ephem/`): Chebyshev series for the
//! Sun, the planetary system barycenters, the Earth–Moon barycenter and the
//! Moon, as DE440 has them. Outer planets are their system barycenters.
//!
//! Small bodies start from JPL Horizons states at 2040-01-01
//! (`data/neas.toml`) and are integrated as particles in DE440's field
//! ([`crate::gravity`], with Earth's J2 and the Sun's relativistic term) to
//! the end of the tables, when the ephemeris is built.

use crate::gravity::Field;
use crate::integrate::{Dop853, Tolerance, Trajectory};
use crate::kepler::{Elements, State};
use crate::time::{Epoch, SECONDS_PER_DAY};
use glam::DVec3;
use rayon::prelude::*;
use serde::Deserialize;
use std::sync::OnceLock;

/// Small-body integration tolerance: steps hold ~1e-12 of the state, well
/// below the bodies' own uncertainty.
const SMALL_BODY_TOLERANCE: Tolerance = Tolerance {
    rtol: 1e-12,
    atol: 1e-6,
};

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
    /// Integrated barycentric `[r, v]`, time in seconds after `epoch`.
    Integrated { epoch: Epoch, path: Trajectory<6> },
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
    /// First and last times covered, TDB seconds past J2000.
    fn span(&self) -> (f64, f64) {
        let records = self.coef.len() / (3 * self.ncoef);
        (self.init, self.init + records as f64 * self.intlen)
    }

    /// The record covering `t` (the last one includes its end), and `t`
    /// scaled to [-1, 1] within it.
    fn record(&self, t: Epoch) -> (&[f64], f64) {
        let n = self.ncoef;
        let records = self.coef.len() / (3 * n);
        let sec = t.seconds() - self.init;
        assert!(
            sec >= 0.0 && sec <= records as f64 * self.intlen,
            "{t} is outside the DE440 tables (2025–2080)"
        );
        let k = ((sec / self.intlen).floor() as usize).min(records - 1);
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
    /// The DE440 tables' span.
    span: (Epoch, Epoch),
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
    /// Barycentric state at the file's epoch, km and km/s.
    r_km: [f64; 3],
    v_kms: [f64; 3],
}

impl Ephemeris {
    /// DE440 plus the small bodies in `data/neas.toml`, built once per
    /// process (integrating the small bodies takes tens of milliseconds).
    pub fn standard() -> &'static Ephemeris {
        static STANDARD: OnceLock<Ephemeris> = OnceLock::new();
        STANDARD.get_or_init(|| {
            Self::with_small_bodies(include_str!("../../../data/neas.toml"))
                .expect("data/neas.toml is valid")
        })
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

        let emb = de.series_index(3, 0);
        let moon_offset = de.series_index(301, 3);
        let series: Vec<Series> = de.series.into_iter().map(|(_, _, s)| s).collect();
        let span = (
            series.iter().map(|s| s.span().0).fold(f64::MIN, f64::max),
            series.iter().map(|s| s.span().1).fold(f64::MAX, f64::min),
        );
        let mut eph = Ephemeris {
            bodies,
            series,
            emb,
            moon_offset,
            emrat,
            sun: BodyId(0),
            span: (
                Epoch(span.0 / SECONDS_PER_DAY),
                Epoch(span.1 / SECONDS_PER_DAY),
            ),
        };

        let file: SmallBodyFile = toml::from_str(toml_src).map_err(|e| e.to_string())?;
        let epoch = Epoch::from_jd(file.epoch_jd_tdb);
        for (k, row) in file.body.iter().enumerate() {
            if eph.lookup(&row.id).is_some() || file.body[..k].iter().any(|b| b.id == row.id) {
                return Err(format!("duplicate body id {:?}", row.id));
            }
        }
        let paths: Vec<Trajectory<6>> = {
            let field = Field::all(&eph, None).with_oblateness().with_relativity();
            let ode = field.ode(epoch);
            let end = (eph.span.1.0 - epoch.0) * SECONDS_PER_DAY;
            file.body
                .par_iter()
                .map(|row| {
                    let ([x, y, z], [vx, vy, vz]) = (row.r_km, row.v_kms);
                    let mut run =
                        Dop853::new(&ode, 0.0, [x, y, z, vx, vy, vz], SMALL_BODY_TOLERANCE);
                    let mut steps = Vec::new();
                    run.advance_dense(&ode, end, &mut steps)
                        .unwrap_or_else(|e| panic!("{}: {e:?}", row.id));
                    Trajectory::new(steps)
                })
                .collect()
        };
        for (row, path) in file.body.into_iter().zip(paths) {
            eph.bodies.push(Body {
                id: row.id,
                name: row.name,
                kind: BodyKind::SmallBody {
                    spectral: row.spectral,
                    diameter_km: row.diameter_km,
                    h_mag: row.h_mag,
                },
                gm: 0.0,
                figure: None,
                motion: Motion::Integrated { epoch, path },
            });
        }
        Ok(eph)
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

    /// The span of the DE440 tables, which small bodies are integrated to.
    pub fn span(&self) -> (Epoch, Epoch) {
        self.span
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
            Motion::Integrated { .. } => self.barycentric(id, t).r,
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
            Motion::Integrated { epoch, path } => {
                let (t0, t1) = path.span();
                let sec = (t.0 - epoch.0) * SECONDS_PER_DAY;
                assert!(
                    sec >= t0 && sec <= t1,
                    "{t} is outside {}'s integrated span (from {epoch})",
                    self.bodies[id.0].id
                );
                let y = path.at(sec);
                State {
                    r: DVec3::new(y[0], y[1], y[2]),
                    v: DVec3::new(y[3], y[4], y[5]),
                }
            }
        }
    }

    /// State relative to the Sun.
    pub fn state(&self, id: BodyId, t: Epoch) -> State {
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
    /// [`Self::system_state`]): osculating, about the Sun and the system's
    /// mass. Panics for the Sun.
    pub fn elements(&self, id: BodyId, t: Epoch) -> Elements {
        let body = &self.bodies[id.0];
        let gm = match &body.motion {
            Motion::Earth | Motion::Moon => self
                .bodies
                .iter()
                .filter(|b| matches!(b.motion, Motion::Earth | Motion::Moon))
                .map(|b| b.gm)
                .sum(),
            Motion::Series(_) | Motion::Integrated { .. } => {
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
