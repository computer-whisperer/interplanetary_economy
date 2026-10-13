//! The circular restricted three-body problem: a massless ship moving with
//! two bodies on circular orbits about their barycenter, in the rotating
//! frame where they stand still on the x axis. Units are nondimensional:
//! their separation, their combined mass, and 1/(mean motion), so the
//! smaller body sits at x = 1 - mu. The Earth–Moon case is the model JPL's
//! periodic-orbit catalog uses (M1a.md, Orbits).

/// A CR3BP with mass ratio `mu` = m2 / (m1 + m2).
#[derive(Clone, Copy, Debug)]
pub struct Cr3bp {
    pub mu: f64,
}

impl Cr3bp {
    /// The equations of motion of `[x, y, z, vx, vy, vz]`.
    pub fn ode(&self) -> impl Fn(f64, &[f64; 6]) -> [f64; 6] + '_ {
        move |_t, s| {
            let [x, y, z, vx, vy, vz] = *s;
            let mu = self.mu;
            let (d1, d2) = (self.cube(x + mu, y, z), self.cube(x - 1.0 + mu, y, z));
            let ax = 2.0 * vy + x - (1.0 - mu) * (x + mu) / d1 - mu * (x - 1.0 + mu) / d2;
            let ay = -2.0 * vx + y - (1.0 - mu) * y / d1 - mu * y / d2;
            let az = -(1.0 - mu) * z / d1 - mu * z / d2;
            [vx, vy, vz, ax, ay, az]
        }
    }

    /// The equations of motion with the state transition matrix: `[x, y, z,
    /// vx, vy, vz, Φ]`, Φ the 6×6 sensitivity of the state to the initial
    /// state, row-major.
    pub fn ode_stm(&self) -> impl Fn(f64, &[f64; 42]) -> [f64; 42] + '_ {
        let ode = self.ode();
        move |t, s| {
            let state: [f64; 6] = s[..6].try_into().unwrap();
            let mut out = [0.0; 42];
            out[..6].copy_from_slice(&ode(t, &state));
            let u = self.hessian(&state);
            let phi = &s[6..];
            for c in 0..6 {
                for r in 0..3 {
                    out[6 + r * 6 + c] = phi[(r + 3) * 6 + c];
                    let (vx, vy) = (phi[3 * 6 + c], phi[4 * 6 + c]);
                    let coriolis = match r {
                        0 => 2.0 * vy,
                        1 => -2.0 * vx,
                        _ => 0.0,
                    };
                    out[6 + (r + 3) * 6 + c] =
                        (0..3).map(|k| u[r][k] * phi[k * 6 + c]).sum::<f64>() + coriolis;
                }
            }
            out
        }
    }

    /// Second derivatives of the effective potential at a position.
    fn hessian(&self, s: &[f64; 6]) -> [[f64; 3]; 3] {
        let [x, y, z, ..] = *s;
        let mu = self.mu;
        let mut u = [[0.0; 3]; 3];
        u[0][0] = 1.0;
        u[1][1] = 1.0;
        for (m, d) in [(1.0 - mu, [x + mu, y, z]), (mu, [x - 1.0 + mu, y, z])] {
            let r2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            let r3 = r2 * r2.sqrt();
            for (i, row) in u.iter_mut().enumerate() {
                for (k, v) in row.iter_mut().enumerate() {
                    *v += m * (3.0 * d[i] * d[k] / (r3 * r2) - if i == k { 1.0 / r3 } else { 0.0 });
                }
            }
        }
        u
    }

    /// The Jacobi constant, conserved along any trajectory.
    pub fn jacobi(&self, s: &[f64; 6]) -> f64 {
        let [x, y, z, vx, vy, vz] = *s;
        let mu = self.mu;
        let r1 = ((x + mu).powi(2) + y * y + z * z).sqrt();
        let r2 = ((x - 1.0 + mu).powi(2) + y * y + z * z).sqrt();
        x * x + y * y + 2.0 * (1.0 - mu) / r1 + 2.0 * mu / r2 - (vx * vx + vy * vy + vz * vz)
    }

    /// The stability index (|λ| + 1/|λ|)/2 of the largest eigenvalue λ of a
    /// periodic orbit's monodromy matrix (its state transition matrix over
    /// one period), as JPL's catalog reports it: 1 for a stable orbit. Uses
    /// the eigenvalues' reciprocal pairs, so only traces of `m` and `m²`.
    pub fn stability_index(monodromy: &[f64; 36]) -> f64 {
        let m2 = crate::linalg::mul(6, 6, 6, monodromy, monodromy);
        let (t1, t2) = (
            (0..6).map(|i| monodromy[i * 7]).sum::<f64>(),
            (0..6).map(|i| m2[i * 7]).sum::<f64>(),
        );
        // Two eigenvalues are 1 (the orbit's own direction and the Jacobi
        // constant); the other pairs have λ + 1/λ = p, the roots of
        // p² + a p + b - 2 = 0.
        let a = 2.0 - t1;
        let b = (a * a + 2.0 - t2) / 2.0;
        let disc = (a * a - 4.0 * (b - 2.0)).max(0.0).sqrt();
        let p = (-a + disc).abs().max((-a - disc).abs()) / 2.0;
        (p / 2.0).max(1.0)
    }

    /// `r³` for the offset (x, y, z).
    fn cube(&self, x: f64, y: f64, z: f64) -> f64 {
        let r2 = x * x + y * y + z * z;
        r2 * r2.sqrt()
    }
}
