//! Gravity on a ship, a massless particle: the massive bodies as point
//! masses at their ephemeris positions, plus the J2 oblateness of those that
//! have a [`Figure`](crate::ephem::Figure).
//!
//! Positions are measured from a center: a body (geocentric for cislunar
//! space keeps positions small and precise) or the solar-system barycenter.
//! A body center brings the indirect term, the center's own acceleration
//! toward the other bodies.

use crate::ephem::{BodyId, Ephemeris};
use crate::time::{Epoch, SECONDS_PER_DAY};
use glam::DVec3;

/// The gravity of chosen bodies, in a frame centered on one of them or on
/// the solar-system barycenter.
#[derive(Clone, Debug)]
pub struct Field<'a> {
    eph: &'a Ephemeris,
    center: Option<BodyId>,
    /// Bodies other than the center, with their GMs.
    others: Vec<(BodyId, f64)>,
    center_gm: f64,
    oblate: bool,
}

impl<'a> Field<'a> {
    /// The pull of `bodies`, with positions measured from `center` (the
    /// solar-system barycenter if `None`). A body center must be one of
    /// `bodies`.
    pub fn new(eph: &'a Ephemeris, center: Option<BodyId>, bodies: &[BodyId]) -> Self {
        if let Some(c) = center {
            assert!(bodies.contains(&c), "the center must be one of the bodies");
        }
        Field {
            eph,
            center,
            others: bodies
                .iter()
                .filter(|&&b| Some(b) != center)
                .map(|&b| (b, eph.body(b).gm))
                .collect(),
            center_gm: center.map_or(0.0, |c| eph.body(c).gm),
            oblate: false,
        }
    }

    /// Every body with mass.
    pub fn all(eph: &'a Ephemeris, center: Option<BodyId>) -> Self {
        let massive: Vec<BodyId> = eph.ids().filter(|&b| eph.body(b).gm > 0.0).collect();
        Self::new(eph, center, &massive)
    }

    /// Includes the J2 oblateness of the bodies that have a figure.
    pub fn with_oblateness(mut self) -> Self {
        self.oblate = true;
        self
    }

    /// Acceleration of a ship at `r` (from the center) at `t`, km/s².
    pub fn acceleration(&self, t: Epoch, r: DVec3) -> DVec3 {
        let origin = self.center.map(|c| self.eph.barycentric_position(c, t));
        let mut a = DVec3::ZERO;
        if let Some(c) = self.center {
            a += self.center_gm * point_mass(-r);
            a += self.oblateness(c, r);
        }
        for &(b, gm) in &self.others {
            let d = self.eph.barycentric_position(b, t) - origin.unwrap_or(DVec3::ZERO);
            a += gm * point_mass(d - r);
            if origin.is_some() {
                a -= gm * point_mass(d);
            }
            a += self.oblateness(b, r - d);
        }
        a
    }

    /// The J2 acceleration toward body `b` of a ship at `rho` from its
    /// center, if enabled and the body has a figure.
    fn oblateness(&self, b: BodyId, rho: DVec3) -> DVec3 {
        let Some(fig) = self.eph.body(b).figure.filter(|_| self.oblate) else {
            return DVec3::ZERO;
        };
        let r2 = rho.length_squared();
        let z = rho.dot(fig.pole);
        let k =
            -1.5 * fig.j2 * self.eph.body(b).gm * fig.radius * fig.radius / (r2 * r2 * r2.sqrt());
        k * ((1.0 - 5.0 * z * z / r2) * rho + 2.0 * z * fig.pole)
    }

    /// The equations of motion of a ship, `[r, v]` in km and km/s, with time
    /// in seconds after `epoch`: the right-hand side for
    /// [`Dop853`](crate::integrate::Dop853).
    pub fn ode(&self, epoch: Epoch) -> impl Fn(f64, &[f64; 6]) -> [f64; 6] + '_ {
        move |t, y| {
            let a = self.acceleration(
                Epoch(epoch.0 + t / SECONDS_PER_DAY),
                DVec3::new(y[0], y[1], y[2]),
            );
            [y[3], y[4], y[5], a.x, a.y, a.z]
        }
    }
}

/// `d / |d|³`: the pull per unit GM toward a mass at offset `d`.
fn point_mass(d: DVec3) -> DVec3 {
    let r2 = d.length_squared();
    d / (r2 * r2.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrate::{Dop853, Tolerance};
    use crate::kepler::Elements;
    use std::f64::consts::PI;

    const TIGHT: Tolerance = Tolerance {
        rtol: 1e-12,
        atol: 1e-9,
    };

    fn fly(f: impl Fn(f64, &[f64; 6]) -> [f64; 6], y0: [f64; 6], seconds: f64) -> [f64; 6] {
        let mut ode = Dop853::new(&f, 0.0, y0, TIGHT);
        ode.advance_to(&f, seconds).unwrap();
        ode.y
    }

    fn split(y: &[f64; 6]) -> (DVec3, DVec3) {
        (DVec3::new(y[0], y[1], y[2]), DVec3::new(y[3], y[4], y[5]))
    }

    /// Earth's J2 turns a 400 km, 51.6° orbit's node at the secular rate
    /// -3/2 n J2 (R/p)² cos i.
    #[test]
    fn earth_j2_regresses_the_node() {
        let eph = Ephemeris::standard();
        let earth = eph.lookup("earth").unwrap();
        let field = Field::new(&eph, Some(earth), &[earth]).with_oblateness();
        let fig = eph.body(earth).figure.unwrap();
        let gm = eph.body(earth).gm;
        // Equatorial axes: the equinox, the pole, and their cross product.
        let (ex, ez) = (DVec3::X, fig.pole);
        let ey = ez.cross(ex);
        let to_eq = |v: DVec3| DVec3::new(v.dot(ex), v.dot(ey), v.dot(ez));
        let (a, inc) = (6_778.137, 51.6f64.to_radians());
        let v = (gm / a).sqrt();
        let r0 = a * ex;
        let v0 = v * (inc.cos() * ey + inc.sin() * ez);
        let days = 10.0;
        let y = fly(
            field.ode(Epoch(16_000.0)),
            [r0.x, r0.y, r0.z, v0.x, v0.y, v0.z],
            days * 86_400.0,
        );
        let (r, v) = split(&y);
        let el = Elements::from_state(
            crate::kepler::State {
                r: to_eq(r),
                v: to_eq(v),
            },
            gm,
        )
        .unwrap();
        let drift = (el.raan + PI).rem_euclid(2.0 * PI) - PI;
        let n = (gm / a.powi(3)).sqrt();
        let expected = -1.5 * n * fig.j2 * (fig.radius / a).powi(2) * inc.cos() * days * 86_400.0;
        println!(
            "node moved {:.3}°, secular theory {:.3}°",
            drift.to_degrees(),
            expected.to_degrees()
        );
        assert!(
            (drift / expected - 1.0).abs() < 0.01,
            "{drift} vs {expected}"
        );
    }

    /// A cislunar trajectory integrated about Earth (with indirect terms)
    /// and about the solar-system barycenter agrees to within what point
    /// masses leave out of Earth's own motion: DE440's Earth accelerates
    /// ~1.9e-13 km/s² differently from the point-mass pull of the other
    /// bodies (mostly relativity; measured 2026-10-13), ~100 m over 10 days.
    /// Near Earth the geocentric frame is the better model, since those
    /// forces act almost equally on Earth and the ship.
    #[test]
    fn geocentric_and_barycentric_frames_agree() {
        let eph = Ephemeris::standard();
        let earth = eph.lookup("earth").unwrap();
        let epoch = Epoch::from_ymd(2045, 3, 1);
        let geo = Field::all(&eph, Some(earth));
        let bary = Field::all(&eph, None);
        // Starting near EML1 with a slow drift.
        let (r0, v0) = (
            DVec3::new(320_000.0, 50_000.0, 10_000.0),
            DVec3::new(-0.1, 0.8, 0.05),
        );
        let e0 = eph.barycentric(earth, epoch);
        let seconds = 10.0 * 86_400.0;
        let g = fly(
            geo.ode(epoch),
            [r0.x, r0.y, r0.z, v0.x, v0.y, v0.z],
            seconds,
        );
        let (rb, vb) = (r0 + e0.r, v0 + e0.v);
        let b = fly(
            bary.ode(epoch),
            [rb.x, rb.y, rb.z, vb.x, vb.y, vb.z],
            seconds,
        );
        let e1 = eph.barycentric_position(earth, Epoch(epoch.0 + 10.0));
        let diff = (split(&b).0 - e1 - split(&g).0).length();
        println!("frames differ by {:.3} m after 10 days", diff * 1e3);
        assert!(diff < 0.3, "{diff} km");
    }

    /// The Moon, flown as a particle from its DE440 state about Earth (with
    /// its own mass added to Earth's pull, the two-body reduced mass), stays
    /// with DE440's Moon: 1.0 km after 30 days (measured 2026-10-13; DE440
    /// also models relativity, tides and the Moon's own figure).
    #[test]
    fn moon_as_particle_follows_de440() {
        let eph = Ephemeris::standard();
        let (earth, moon) = (eph.lookup("earth").unwrap(), eph.lookup("moon").unwrap());
        let others: Vec<BodyId> = eph
            .ids()
            .filter(|&b| eph.body(b).gm > 0.0 && b != moon)
            .collect();
        let field = Field::new(&eph, Some(earth), &others).with_oblateness();
        let gm_moon = eph.body(moon).gm;
        let epoch = Epoch::from_ymd(2045, 3, 1);
        let rhs = |t: f64, y: &[f64; 6]| {
            let mut d = field.ode(epoch)(t, y);
            let r = DVec3::new(y[0], y[1], y[2]);
            let extra = gm_moon * point_mass(-r);
            d[3] += extra.x;
            d[4] += extra.y;
            d[5] += extra.z;
            d
        };
        let geocentric = |t: Epoch| {
            let (m, e) = (eph.barycentric(moon, t), eph.barycentric(earth, t));
            (m.r - e.r, m.v - e.v)
        };
        let (r0, v0) = geocentric(epoch);
        let days = 30.0;
        let y = fly(rhs, [r0.x, r0.y, r0.z, v0.x, v0.y, v0.z], days * 86_400.0);
        let err = (split(&y).0 - geocentric(Epoch(epoch.0 + days)).0).length();
        println!("Moon after {days} days: {err:.3} km from DE440");
        assert!(err < 3.0, "{err} km");
    }
}
