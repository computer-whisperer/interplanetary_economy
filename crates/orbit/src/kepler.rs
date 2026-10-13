//! Two-body orbits: elements to and from state, and state propagation.

use glam::DVec3;
use std::f64::consts::{PI, TAU};

/// Position (km) and velocity (km/s).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State {
    pub r: DVec3,
    pub v: DVec3,
}

/// Classical elements of an elliptic orbit. Angles in radians, `a` in km,
/// `mean_motion` in rad/s.
///
/// The mean motion is carried explicitly instead of derived from a central
/// mass: fitted ephemerides (see [`crate::ephem`]) have rates that differ
/// slightly from √(μ/a³), and position and velocity must agree with them.
#[derive(Clone, Copy, Debug)]
pub struct Elements {
    pub a: f64,
    pub e: f64,
    pub i: f64,
    pub raan: f64,
    pub argp: f64,
    pub mean_anomaly: f64,
    pub mean_motion: f64,
}

impl Elements {
    pub fn state(&self) -> State {
        let Elements {
            a,
            e,
            i,
            raan,
            argp,
            mean_anomaly,
            mean_motion: n,
        } = *self;
        let ecc_anomaly = solve_kepler(mean_anomaly, e);
        let (sin_e, cos_e) = ecc_anomaly.sin_cos();
        let b = a * (1.0 - e * e).sqrt();
        let r_pf = DVec3::new(a * (cos_e - e), b * sin_e, 0.0);
        let edot = n / (1.0 - e * cos_e);
        let v_pf = DVec3::new(-a * sin_e * edot, b * cos_e * edot, 0.0);
        let rot = perifocal_rotation(i, raan, argp);
        State {
            r: rot(r_pf),
            v: rot(v_pf),
        }
    }

    /// The osculating elements of a state about a body with gravitational
    /// parameter `mu`; `None` unless the orbit is an ellipse. For a circular
    /// orbit the argument of periapsis is zero, and for an equatorial one
    /// the node is.
    pub fn from_state(s: State, mu: f64) -> Option<Elements> {
        let r = s.r.length();
        let h = s.r.cross(s.v);
        let a = 1.0 / (2.0 / r - s.v.length_squared() / mu);
        let e_vec = ((s.v.length_squared() - mu / r) * s.r - s.r.dot(s.v) * s.v) / mu;
        let e = e_vec.length();
        if !(a > 0.0 && e < 1.0) {
            return None;
        }
        let h_hat = h.normalize();
        let i = h_hat.z.clamp(-1.0, 1.0).acos();
        let node = DVec3::new(-h.y, h.x, 0.0);
        let raan = if node.length() > 1e-12 * h.length() {
            node.y.atan2(node.x).rem_euclid(TAU)
        } else {
            0.0
        };
        let n_hat = DVec3::new(raan.cos(), raan.sin(), 0.0);
        let m_hat = h_hat.cross(n_hat);
        let (argp, p_hat) = if e > 1e-12 {
            (
                e_vec.dot(m_hat).atan2(e_vec.dot(n_hat)).rem_euclid(TAU),
                e_vec / e,
            )
        } else {
            (0.0, n_hat)
        };
        let q_hat = h_hat.cross(p_hat);
        let nu = s.r.dot(q_hat).atan2(s.r.dot(p_hat));
        let ecc_anomaly = ((1.0 - e * e).sqrt() * nu.sin()).atan2(e + nu.cos());
        Some(Elements {
            a,
            e,
            i,
            raan,
            argp,
            mean_anomaly: (ecc_anomaly - e * ecc_anomaly.sin()).rem_euclid(TAU),
            mean_motion: (mu / a.powi(3)).sqrt(),
        })
    }

    /// The same orbit `dt` seconds later.
    pub fn advanced(&self, dt: f64) -> Elements {
        Elements {
            mean_anomaly: self.mean_anomaly + self.mean_motion * dt,
            ..*self
        }
    }

    pub fn period(&self) -> f64 {
        TAU / self.mean_motion
    }
}

/// Rotation from the perifocal frame into the reference frame.
fn perifocal_rotation(i: f64, raan: f64, argp: f64) -> impl Fn(DVec3) -> DVec3 {
    let (si, ci) = i.sin_cos();
    let (so, co) = raan.sin_cos();
    let (sw, cw) = argp.sin_cos();
    let px = DVec3::new(co * cw - so * sw * ci, so * cw + co * sw * ci, sw * si);
    let py = DVec3::new(-co * sw - so * cw * ci, -so * sw + co * cw * ci, cw * si);
    move |p: DVec3| px * p.x + py * p.y
}

/// Eccentric anomaly for mean anomaly `m` (any range) and `0 <= e < 1`.
pub fn solve_kepler(m: f64, e: f64) -> f64 {
    let m = (m + PI).rem_euclid(TAU) - PI;
    let mut ecc = if e < 0.8 { m } else { PI.copysign(m) };
    for _ in 0..50 {
        let step = (ecc - e * ecc.sin() - m) / (1.0 - e * ecc.cos());
        ecc -= step;
        if step.abs() < 1e-14 {
            break;
        }
    }
    ecc
}

/// Propagates a two-body state by `dt` seconds with universal variables
/// (Curtis, Algorithms 3.3–3.4), solved by Laguerre–Conway iteration, which
/// converges for any conic where Newton's method can diverge. Elliptic orbits
/// propagate by `dt` modulo the period; hyperbolic ones start from Vallado's
/// initial guess. Returns `None` if the iteration does not converge.
pub fn propagate(s: State, dt: f64, mu: f64) -> Option<State> {
    let sqrt_mu = mu.sqrt();
    let r0 = s.r.length();
    let vr0 = s.r.dot(s.v) / r0;
    let alpha = 2.0 / r0 - s.v.length_squared() / mu;
    let dt = if alpha > 0.0 {
        dt.rem_euclid(TAU / (mu * alpha.powi(3)).sqrt())
    } else {
        dt
    };

    let k1 = r0 * vr0 / sqrt_mu;
    let k2 = 1.0 - alpha * r0;
    let mut chi = if alpha > 1e-12 {
        sqrt_mu * alpha * dt
    } else if alpha < -1e-12 {
        // Vallado, Fundamentals of Astrodynamics, Algorithm 8.
        let a = 1.0 / alpha;
        let arg = -2.0 * mu * alpha * dt / (r0 * vr0 + dt.signum() * (-mu * a).sqrt() * k2);
        let guess = dt.signum() * (-a).sqrt() * arg.ln();
        if guess.is_finite() {
            guess
        } else {
            sqrt_mu * dt / r0
        }
    } else {
        sqrt_mu * dt / r0
    };
    const N: f64 = 5.0;
    let mut converged = false;
    for _ in 0..200 {
        let z = alpha * chi * chi;
        let (c, sz) = (stumpff_c(z), stumpff_s(z));
        let f = k1 * chi * chi * c + k2 * chi.powi(3) * sz + r0 * chi - sqrt_mu * dt;
        let df = k1 * chi * (1.0 - z * sz) + k2 * chi * chi * c + r0;
        let ddf = k1 * (1.0 - z * c) + k2 * chi * (1.0 - z * sz);
        let root = ((N - 1.0).powi(2) * df * df - N * (N - 1.0) * f * ddf)
            .abs()
            .sqrt();
        let step = N * f / (df + root.copysign(df));
        chi -= step;
        if !chi.is_finite() {
            return None;
        }
        if step.abs() < 1e-13 * chi.abs().max(1.0) {
            converged = true;
            break;
        }
    }
    if !converged {
        return None;
    }

    let z = alpha * chi * chi;
    let (c, sz) = (stumpff_c(z), stumpff_s(z));
    let f = 1.0 - chi * chi / r0 * c;
    let g = dt - chi.powi(3) / sqrt_mu * sz;
    let r = f * s.r + g * s.v;
    let rn = r.length();
    let fdot = sqrt_mu / (rn * r0) * (z * sz - 1.0) * chi;
    let gdot = 1.0 - chi * chi / rn * c;
    let out = State {
        r,
        v: fdot * s.r + gdot * s.v,
    };
    (out.r.is_finite() && out.v.is_finite()).then_some(out)
}

fn stumpff_c(z: f64) -> f64 {
    if z > 1e-8 {
        (1.0 - z.sqrt().cos()) / z
    } else if z < -1e-8 {
        ((-z).sqrt().cosh() - 1.0) / -z
    } else {
        0.5 - z / 24.0
    }
}

fn stumpff_s(z: f64) -> f64 {
    if z > 1e-8 {
        let s = z.sqrt();
        (s - s.sin()) / s.powi(3)
    } else if z < -1e-8 {
        let s = (-z).sqrt();
        (s.sinh() - s) / s.powi(3)
    } else {
        1.0 / 6.0 - z / 120.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::MU_EARTH;

    #[test]
    fn kepler_equation_residual() {
        for &e in &[0.0, 0.1, 0.5, 0.9, 0.99] {
            for k in -20..=20 {
                let m = k as f64 * 0.37;
                let ecc = solve_kepler(m, e);
                let resid = ecc - e * ecc.sin() - m;
                let wrapped = (resid + PI).rem_euclid(TAU) - PI;
                assert!(wrapped.abs() < 1e-12, "e={e} m={m} resid={wrapped}");
            }
        }
    }

    fn sample_orbit() -> Elements {
        let a: f64 = 26_600.0;
        Elements {
            a,
            e: 0.74,
            i: 1.1,
            raan: 0.4,
            argp: 4.5,
            mean_anomaly: 0.3,
            mean_motion: (MU_EARTH / a.powi(3)).sqrt(),
        }
    }

    #[test]
    fn elements_state_obeys_vis_viva_and_momentum() {
        let el = sample_orbit();
        let h_expected = (MU_EARTH * el.a * (1.0 - el.e * el.e)).sqrt();
        for k in 0..12 {
            let s = el.advanced(k as f64 * 3_000.0).state();
            let vis_viva = MU_EARTH * (2.0 / s.r.length() - 1.0 / el.a);
            assert!((s.v.length_squared() - vis_viva).abs() < 1e-9 * vis_viva);
            assert!((s.r.cross(s.v).length() - h_expected).abs() < 1e-9 * h_expected);
        }
    }

    #[test]
    fn elements_round_trip_through_state() {
        let el = sample_orbit();
        let angle = |x: f64, y: f64| (x - y + PI).rem_euclid(TAU) - PI;
        for k in 0..12 {
            let el = el.advanced(k as f64 * 3_000.0);
            let back = Elements::from_state(el.state(), MU_EARTH).unwrap();
            assert!((back.a - el.a).abs() < 1e-6 * el.a, "{back:?}");
            assert!((back.e - el.e).abs() < 1e-10);
            for (x, y) in [
                (back.i, el.i),
                (back.raan, el.raan),
                (back.argp, el.argp),
                (back.mean_anomaly, el.mean_anomaly),
            ] {
                assert!(angle(x, y).abs() < 1e-9, "{back:?} vs {el:?}");
            }
        }
        let escape = State {
            r: DVec3::new(7_000.0, 0.0, 0.0),
            v: DVec3::new(0.0, 12.0, 0.0),
        };
        assert!(Elements::from_state(escape, MU_EARTH).is_none());
    }

    #[test]
    fn propagate_matches_elements() {
        let el = sample_orbit();
        let s0 = el.state();
        for &dt in &[60.0, 5_000.0, 20_000.0, 2.5 * el.period()] {
            let a = propagate(s0, dt, MU_EARTH).unwrap();
            let b = el.advanced(dt).state();
            assert!(
                (a.r - b.r).length() < 1e-5,
                "dt={dt} dr={}",
                (a.r - b.r).length()
            );
            assert!((a.v - b.v).length() < 1e-8, "dt={dt}");
        }
    }

    #[test]
    fn propagate_hyperbolic_conserves_energy() {
        let s0 = State {
            r: DVec3::new(7_000.0, 0.0, 0.0),
            v: DVec3::new(0.0, 12.0, 1.0),
        };
        let energy = |s: State| s.v.length_squared() / 2.0 - MU_EARTH / s.r.length();
        let s1 = propagate(s0, 40_000.0, MU_EARTH).unwrap();
        assert!((energy(s1) - energy(s0)).abs() < 1e-9);
        // And back again.
        let s2 = propagate(s1, -40_000.0, MU_EARTH).unwrap();
        assert!((s2.r - s0.r).length() < 1e-4);
    }

    /// Long hyperbolic propagations (once NaN from an overflowing initial
    /// guess) agree with many short steps.
    #[test]
    fn propagate_long_hyperbolic() {
        let s0 = State {
            r: DVec3::new(7_000.0, 0.0, 0.0),
            v: DVec3::new(0.0, 12.0, 1.0),
        };
        for dt in [1e6, -1e6, 1e7] {
            let once = propagate(s0, dt, MU_EARTH).unwrap();
            let mut stepped = s0;
            for _ in 0..1000 {
                stepped = propagate(stepped, dt / 1000.0, MU_EARTH).unwrap();
            }
            let rel = (once.r - stepped.r).length() / stepped.r.length();
            assert!(rel < 1e-9, "dt={dt} rel={rel}");
        }
    }
}
