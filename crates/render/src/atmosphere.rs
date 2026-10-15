//! Atmospheres (M1c.md, step 6 plan, item 5), by Hillaire's method
//! ("A Scalable and Production Ready Sky and Atmosphere Rendering
//! Technique", 2020): a shell of Rayleigh and Mie scattering and ozone's
//! absorption over a sphere, its transmittance and multiple scattering
//! tabulated here on the CPU (in f64, so tests can check them), and a ray
//! march per pixel on the GPU (scene.wgsl) reading them.
//!
//! Lengths are km; coefficients per km.

use crate::maps::{Image, f32_to_f16};
use bytemuck::{Pod, Zeroable};
use glam::DVec3;

/// An atmosphere's composition.
#[derive(Clone, Debug, PartialEq)]
pub struct Atmosphere {
    /// The ground's radius and the top's.
    pub bottom: f64,
    pub top: f64,
    /// Rayleigh scattering at the ground, per channel, and its scale
    /// height.
    pub rayleigh: [f64; 3],
    pub rayleigh_height: f64,
    /// Mie scattering and absorption at the ground, their scale height,
    /// and the phase function's asymmetry (Cornette–Shanks).
    pub mie_scattering: [f64; 3],
    pub mie_absorption: [f64; 3],
    pub mie_height: f64,
    pub mie_g: f64,
    /// Absorption at its densest (ozone's), in a layer whose density
    /// falls linearly from `absorption_center` to zero `absorption_width`
    /// above and below.
    pub absorption: [f64; 3],
    pub absorption_center: f64,
    pub absorption_width: f64,
    /// The ground's albedo, for light it sends back up.
    pub ground_albedo: f64,
}

/// The isotropic phase function, per sr.
const ISOTROPIC: f64 = 1.0 / (4.0 * std::f64::consts::PI);

/// The transmittance table's size: by the view direction, then the
/// altitude.
pub const TRANSMITTANCE: (u32, u32) = (256, 64);

/// The multiple-scattering table's size: by the Sun's direction, then the
/// altitude.
pub const MULTIPLE: (u32, u32) = (32, 32);

impl Atmosphere {
    fn densities(&self, h: f64) -> (f64, f64, f64) {
        let ozone = (1.0 - (h - self.absorption_center).abs() / self.absorption_width).max(0.0);
        (
            (-h / self.rayleigh_height).exp(),
            (-h / self.mie_height).exp(),
            ozone,
        )
    }

    /// Scattering and extinction at altitude `h`, per channel.
    fn coefficients(&self, h: f64) -> ([f64; 3], [f64; 3]) {
        let (r, m, o) = self.densities(h.max(0.0));
        let scattering = std::array::from_fn(|c| self.rayleigh[c] * r + self.mie_scattering[c] * m);
        let extinction = std::array::from_fn(|c| {
            scattering[c] + self.mie_absorption[c] * m + self.absorption[c] * o
        });
        (scattering, extinction)
    }

    /// The distance from radius `r` along direction cosine `mu` (from the
    /// local zenith) to the top.
    fn to_top(&self, r: f64, mu: f64) -> f64 {
        (-r * mu
            + (r * r * (mu * mu - 1.0) + self.top * self.top)
                .max(0.0)
                .sqrt())
        .max(0.0)
    }

    /// Whether a ray from radius `r` at cosine `mu` meets the ground.
    fn meets_ground(&self, r: f64, mu: f64) -> bool {
        mu < 0.0 && r * r * (mu * mu - 1.0) + self.bottom * self.bottom >= 0.0
    }

    /// Transmittance from radius `r` along cosine `mu` to the top,
    /// integrated (a ray meeting the ground is integrated through it, as
    /// the table stores it; lookups test the ground).
    pub fn transmittance(&self, r: f64, mu: f64) -> [f64; 3] {
        const STEPS: usize = 500;
        let d = self.to_top(r, mu);
        let dt = d / STEPS as f64;
        let mut depth = [0.0; 3];
        for i in 0..=STEPS {
            let t = i as f64 * dt;
            let h = (r * r + t * t + 2.0 * r * mu * t).sqrt() - self.bottom;
            let (_, e) = self.coefficients(h);
            // Simpson's rule (the trapezoid's error was 2e-5 at the
            // zenith, Mie's scale height being 1.2 km).
            let w = if i == 0 || i == STEPS {
                1.0
            } else if i % 2 == 1 {
                4.0
            } else {
                2.0
            };
            for c in 0..3 {
                depth[c] += e[c] * w * dt / 3.0;
            }
        }
        depth.map(|x| (-x).exp())
    }

    /// The transmittance table's coordinates (0 to 1, texel centers
    /// included) of radius `r` and cosine `mu` (Bruneton 2017's mapping:
    /// fine near the horizon). As scene.wgsl's `through_air` reads it.
    pub fn transmittance_uv(&self, r: f64, mu: f64) -> (f64, f64) {
        let h = (self.top * self.top - self.bottom * self.bottom).sqrt();
        let rho = (r * r - self.bottom * self.bottom).max(0.0).sqrt();
        let d = self.to_top(r, mu);
        let (d_min, d_max) = (self.top - r, rho + h);
        let x_mu = (d - d_min) / (d_max - d_min);
        let x_r = rho / h;
        let center = |x: f64, n: u32| 0.5 / f64::from(n) + x * (1.0 - 1.0 / f64::from(n));
        (center(x_mu, TRANSMITTANCE.0), center(x_r, TRANSMITTANCE.1))
    }

    /// The radius and cosine of the transmittance table's texel `(x, y)`.
    fn transmittance_at(&self, x: u32, y: u32) -> (f64, f64) {
        let unit = |i: u32, n: u32| f64::from(i) / f64::from(n - 1);
        let (x_mu, x_r) = (unit(x, TRANSMITTANCE.0), unit(y, TRANSMITTANCE.1));
        let h = (self.top * self.top - self.bottom * self.bottom).sqrt();
        let rho = h * x_r;
        let r = (rho * rho + self.bottom * self.bottom).sqrt();
        let (d_min, d_max) = (self.top - r, rho + h);
        let d = d_min + x_mu * (d_max - d_min);
        let mu = if d == 0.0 {
            1.0
        } else {
            ((h * h - rho * rho - d * d) / (2.0 * r * d)).clamp(-1.0, 1.0)
        };
        (r, mu)
    }

    /// The tables, as the GPU reads them (RGBA half floats).
    pub fn tables(&self) -> (Image, Image) {
        let (tw, th) = TRANSMITTANCE;
        let table: Vec<[f64; 3]> = (0..th)
            .flat_map(|y| (0..tw).map(move |x| (x, y)))
            .map(|(x, y)| {
                let (r, mu) = self.transmittance_at(x, y);
                self.transmittance(r, mu)
            })
            .collect();
        let lookup = |r: f64, mu: f64| -> [f64; 3] {
            if self.meets_ground(r, mu) {
                return [0.0; 3];
            }
            let (u, v) = self.transmittance_uv(r, mu);
            bilinear(&table, tw, th, u, v)
        };
        let (mw, mh) = MULTIPLE;
        let multiple: Vec<[f64; 3]> = (0..mh)
            .flat_map(|y| (0..mw).map(move |x| (x, y)))
            .map(|(x, y)| {
                let mu_s = -1.0 + 2.0 * (f64::from(x) + 0.5) / f64::from(mw);
                let r =
                    self.bottom + (self.top - self.bottom) * (f64::from(y) + 0.5) / f64::from(mh);
                self.multiple_scattering(r, mu_s, &lookup)
            })
            .collect();
        (half_image(tw, th, &table), half_image(mw, mh, &multiple))
    }

    /// Hillaire's multiple-scattering term Ψ at radius `r`, the Sun at
    /// cosine `mu_s`, for a Sun of unit irradiance: the second-order light
    /// L₂ arriving from every direction (64, over the sphere), scattered
    /// once more isotropically, and the share f of light scattered there
    /// that comes back, summed as a series: Ψ = L₂ / (1 − f). A point
    /// scattering σ_s adds σ_s Ψ.
    fn multiple_scattering(
        &self,
        r: f64,
        mu_s: f64,
        transmittance: &dyn Fn(f64, f64) -> [f64; 3],
    ) -> [f64; 3] {
        const SQRT: usize = 8;
        const STEPS: usize = 20;
        let origin = DVec3::new(0.0, 0.0, r);
        let sun = DVec3::new((1.0 - mu_s * mu_s).max(0.0).sqrt(), 0.0, mu_s);
        let mut light = [0.0; 3];
        let mut transfer = [0.0; 3];
        for i in 0..SQRT {
            for j in 0..SQRT {
                let theta = std::f64::consts::TAU * (i as f64 + 0.5) / SQRT as f64;
                let phi = (1.0 - 2.0 * (j as f64 + 0.5) / SQRT as f64).acos();
                let dir = DVec3::new(phi.sin() * theta.cos(), phi.sin() * theta.sin(), phi.cos());
                let mu = dir.z;
                let ground = self.meets_ground(r, mu);
                let length = if ground {
                    -r * mu
                        - (r * r * (mu * mu - 1.0) + self.bottom * self.bottom)
                            .max(0.0)
                            .sqrt()
                } else {
                    self.to_top(r, mu)
                };
                let dt = length / STEPS as f64;
                let mut seen = [1.0; 3];
                let (mut l, mut f) = ([0.0; 3], [0.0; 3]);
                for k in 0..STEPS {
                    let p = origin + dir * ((k as f64 + 0.5) * dt);
                    let pr = p.length();
                    let (s, e) = self.coefficients(pr - self.bottom);
                    let sun_t = transmittance(pr, p.dot(sun) / pr);
                    for c in 0..3 {
                        let step = (-e[c] * dt).exp();
                        // The step's light, integrated against its own
                        // extinction (Hillaire's energy-conserving form).
                        let integral = if e[c] > 0.0 { (1.0 - step) / e[c] } else { dt };
                        l[c] += seen[c] * s[c] * sun_t[c] * ISOTROPIC * integral;
                        f[c] += seen[c] * s[c] * integral;
                        seen[c] *= step;
                    }
                }
                if ground {
                    let p = origin + dir * length;
                    let n = p.normalize();
                    let sun_t = transmittance(self.bottom, n.dot(sun));
                    let lit = n.dot(sun).max(0.0);
                    for c in 0..3 {
                        l[c] +=
                            seen[c] * sun_t[c] * lit * self.ground_albedo / std::f64::consts::PI;
                    }
                }
                // Each direction 4π / 64 of the sphere, times the isotropic
                // phase: a mean.
                let weight = 1.0 / (SQRT * SQRT) as f64;
                for c in 0..3 {
                    light[c] += l[c] * weight;
                    transfer[c] += f[c] * weight;
                }
            }
        }
        std::array::from_fn(|c| light[c] / (1.0 - transfer[c]).max(1e-6))
    }

    /// The GPU's constants (part of `SphereGpu`).
    pub(crate) fn gpu(&self) -> AtmosphereGpu {
        let f = |v: [f64; 3]| v.map(|x| x as f32);
        AtmosphereGpu {
            rayleigh: f(self.rayleigh),
            rayleigh_height: self.rayleigh_height as f32,
            mie_scattering: f(self.mie_scattering),
            mie_height: self.mie_height as f32,
            mie_absorption: f(self.mie_absorption),
            mie_g: self.mie_g as f32,
            absorption: f(self.absorption),
            absorption_center: self.absorption_center as f32,
            top: self.top as f32,
            absorption_width: self.absorption_width as f32,
            _pad: [0.0; 2],
        }
    }
}

/// An atmosphere with its tables, as a body's maps carry it.
#[derive(Clone, Debug)]
pub struct Air {
    pub atmosphere: Atmosphere,
    pub transmittance: Image,
    pub multiple: Image,
}

impl Air {
    /// `atmosphere`, its tables computed (a fraction of a second).
    pub fn new(atmosphere: Atmosphere) -> Air {
        let (transmittance, multiple) = atmosphere.tables();
        Air {
            atmosphere,
            transmittance,
            multiple,
        }
    }
}

/// `table` (rows of `w`) at `(u, v)`, with texel centers at `(i + 0.5) /
/// n`, clamped at the edges.
fn bilinear(table: &[[f64; 3]], w: u32, h: u32, u: f64, v: f64) -> [f64; 3] {
    let x = (u * f64::from(w) - 0.5).clamp(0.0, f64::from(w - 1));
    let y = (v * f64::from(h) - 0.5).clamp(0.0, f64::from(h - 1));
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - f64::from(x0), y - f64::from(y0));
    let at = |x: u32, y: u32| table[(y * w + x) as usize];
    std::array::from_fn(|c| {
        let top = at(x0, y0)[c] * (1.0 - fx) + at(x1, y0)[c] * fx;
        let bottom = at(x0, y1)[c] * (1.0 - fx) + at(x1, y1)[c] * fx;
        top * (1.0 - fy) + bottom * fy
    })
}

fn half_image(w: u32, h: u32, table: &[[f64; 3]]) -> Image {
    let data = table
        .iter()
        .flat_map(|v| [v[0], v[1], v[2], 1.0])
        .flat_map(|x| f32_to_f16(x as f32).to_le_bytes())
        .collect();
    Image {
        width: w,
        height: h,
        format: wgpu::TextureFormat::Rgba16Float,
        data,
    }
}

/// An atmosphere's constants in `SphereGpu` (`Atmosphere` in scene.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct AtmosphereGpu {
    pub rayleigh: [f32; 3],
    pub rayleigh_height: f32,
    pub mie_scattering: [f32; 3],
    pub mie_height: f32,
    pub mie_absorption: [f32; 3],
    pub mie_g: f32,
    pub absorption: [f32; 3],
    pub absorption_center: f32,
    /// The top's radius; 0 for none.
    pub top: f32,
    pub absorption_width: f32,
    pub _pad: [f32; 2],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn earth() -> Atmosphere {
        Atmosphere {
            bottom: 6360.0,
            top: 6460.0,
            rayleigh: [5.802e-3, 13.558e-3, 33.1e-3],
            rayleigh_height: 8.0,
            mie_scattering: [3.996e-3; 3],
            mie_absorption: [4.40e-3; 3],
            mie_height: 1.2,
            mie_g: 0.8,
            absorption: [0.650e-3, 1.881e-3, 0.085e-3],
            absorption_center: 25.0,
            absorption_width: 15.0,
            ground_albedo: 0.3,
        }
    }

    /// Straight up from the ground, the optical depth is each
    /// constituent's coefficient times its scale height (less what lies
    /// above the top), plus ozone's tent: its peak times its width.
    #[test]
    fn zenith_transmittance_is_analytic() {
        let a = earth();
        let got = a.transmittance(a.bottom, 1.0);
        let top = a.top - a.bottom;
        for (c, got) in got.into_iter().enumerate() {
            let depth =
                a.rayleigh[c] * a.rayleigh_height * (1.0 - (-top / a.rayleigh_height).exp())
                    + (a.mie_scattering[c] + a.mie_absorption[c])
                        * a.mie_height
                        * (1.0 - (-top / a.mie_height).exp())
                    + a.absorption[c] * a.absorption_width;
            let want = (-depth).exp();
            assert!((got - want).abs() < 1e-5, "{c}: {got} vs {want}");
        }
    }

    /// The table's coordinates and its texels' radii and cosines invert.
    #[test]
    fn transmittance_mapping_inverts() {
        let a = earth();
        for (x, y) in [(0, 0), (17, 3), (128, 40), (255, 63), (200, 1)] {
            let (r, mu) = a.transmittance_at(x, y);
            let (u, v) = a.transmittance_uv(r, mu);
            let ux = u * f64::from(TRANSMITTANCE.0) - 0.5;
            let vy = v * f64::from(TRANSMITTANCE.1) - 0.5;
            let want = |i: u32, n: u32| f64::from(i) / f64::from(n - 1) * f64::from(n - 1);
            assert!((ux - want(x, TRANSMITTANCE.0)).abs() < 1e-6, "{x}: {ux}");
            assert!((vy - want(y, TRANSMITTANCE.1)).abs() < 1e-6, "{y}: {vy}");
        }
    }

    /// Multiple scattering adds light, more in blue (Rayleigh) and more
    /// with the Sun high.
    #[test]
    fn multiple_scattering_is_bluer_with_the_sun_up() {
        let a = earth();
        let (_, table) = a.tables();
        let at = |x: u32, y: u32| {
            let k = ((y * MULTIPLE.0 + x) * 8) as usize;
            [0, 2, 4].map(|c| {
                crate::offscreen::f16_to_f32(u16::from_le_bytes([
                    table.data[k + c],
                    table.data[k + c + 1],
                ]))
            })
        };
        let (noon, dusk) = (at(MULTIPLE.0 - 1, 0), at(MULTIPLE.0 / 2, 0));
        println!("ground, Sun overhead {noon:?}, at the horizon {dusk:?}");
        assert!(noon[2] > noon[0] && noon[0] > 0.0);
        assert!(noon[2] > dusk[2]);
    }
}
