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

    /// The Jacobi constant, conserved along any trajectory.
    pub fn jacobi(&self, s: &[f64; 6]) -> f64 {
        let [x, y, z, vx, vy, vz] = *s;
        let mu = self.mu;
        let r1 = ((x + mu).powi(2) + y * y + z * z).sqrt();
        let r2 = ((x - 1.0 + mu).powi(2) + y * y + z * z).sqrt();
        x * x + y * y + 2.0 * (1.0 - mu) / r1 + 2.0 * mu / r2 - (vx * vx + vy * vy + vz * vz)
    }

    /// `r³` for the offset (x, y, z).
    fn cube(&self, x: f64, y: f64, z: f64) -> f64 {
        let r2 = x * x + y * y + z * z;
        r2 * r2.sqrt()
    }
}
