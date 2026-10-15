//! Atmospheres (M1c.md, step 6 plan, item 5), by Hillaire's method
//! ("A Scalable and Production Ready Sky and Atmosphere Rendering
//! Technique", 2020): a shell of Rayleigh and Mie scattering and ozone's
//! absorption over a sphere, its transmittance, multiple scattering and
//! skylight on the ground tabulated here on the CPU (in f64, so tests can
//! check them), and a ray march per pixel on the GPU (scene.wgsl) reading
//! them.
//!
//! Against a Monte Carlo of Mars's dust (step 6, part 8 plan;
//! `tools/atmosphere/`), three departures from Hillaire:
//! - his multiple scattering is spread over directions as two
//!   scatterings would spread it (isotropic, it was 10× too faint at 40°
//!   from the Sun's direction, 1.2–1.8× too bright at 140°), and leaves
//!   out light from the ground;
//! - the ground is lit through the delta-scaled medium (Mie's forward
//!   peak taken as unscattered) by its direct beam and its sky (it had no
//!   sky: near the terminator 2.5× its direct light);
//! - the ground is seen through T(μ) + E(μ)/μ, the skylight a beam from
//!   the eye would give it (reciprocity): light the air scatters out of
//!   the view, it mostly scatters back in (the beam alone was 0.70 where
//!   the Monte Carlo's ground was seen by 0.89).
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
}

/// The isotropic phase function, per sr.
const ISOTROPIC: f64 = 1.0 / (4.0 * std::f64::consts::PI);

/// Rayleigh's phase function, per sr, at scattering cosine `c`.
fn rayleigh_phase(c: f64) -> f64 {
    3.0 / (16.0 * std::f64::consts::PI) * (1.0 + c * c)
}

/// Henyey and Greenstein's phase function, per sr, at scattering cosine
/// `c`.
fn henyey_greenstein(g: f64, c: f64) -> f64 {
    (1.0 - g * g) / (4.0 * std::f64::consts::PI * (1.0 + g * g - 2.0 * g * c).powf(1.5))
}

/// Cornette and Shanks's phase function (scene.wgsl's `mie_phase`), per
/// sr, at scattering cosine `c`.
pub fn cornette_shanks(g: f64, c: f64) -> f64 {
    3.0 / (8.0 * std::f64::consts::PI) * (1.0 - g * g) * (1.0 + c * c)
        / ((2.0 + g * g) * (1.0 + g * g - 2.0 * g * c).powf(1.5))
}

/// Cornette and Shanks's asymmetry (mean cosine) for parameter `g`.
pub fn asymmetry(g: f64) -> f64 {
    3.0 * g * (4.0 + g * g) / (5.0 * (2.0 + g * g))
}

/// The transmittance table's size: by the view direction, then the
/// altitude.
pub const TRANSMITTANCE: (u32, u32) = (256, 64);

/// The multiple-scattering table's size: by the Sun's direction, then the
/// altitude.
pub const MULTIPLE: (u32, u32) = (32, 32);

/// The skylight table's size: by the Sun's direction.
pub const SKYLIGHT: u32 = 64;

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

    /// The share of Mie scattering in its phase function's forward peak
    /// that delta-scaling takes as unscattered: g², δ-Eddington's for
    /// Cornette–Shanks's Henyey–Greenstein factor (its second Legendre
    /// moment would take the factor 1 + cos²θ's symmetric 0.1 too, which
    /// is no peak).
    pub fn truncation(&self) -> f64 {
        self.mie_g * self.mie_g
    }

    /// The delta-scaled medium (Joseph, Wiscombe and Weinman 1976): Mie
    /// scattering less its forward peak, the rest scattering by
    /// Cornette–Shanks with the rest's asymmetry, (χ₁ − f) / (1 − f).
    pub fn scaled(&self) -> Atmosphere {
        let f = self.truncation();
        let rest = (asymmetry(self.mie_g) - f) / (1.0 - f);
        // The asymmetry rises with g: bisection.
        let (mut lo, mut hi) = (0.0, 0.999);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if asymmetry(mid) < rest {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Atmosphere {
            mie_scattering: self.mie_scattering.map(|x| x * (1.0 - f)),
            mie_g: 0.5 * (lo + hi),
            ..self.clone()
        }
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
        self.optical(r, mu).0.map(|x| (-x).exp())
    }

    /// From radius `r` along cosine `mu` to the top: the optical depth,
    /// and the Mie column (km at the ground's density), by which the
    /// scaled medium's depth is less by f σₘ.
    fn optical(&self, r: f64, mu: f64) -> ([f64; 3], f64) {
        const STEPS: usize = 500;
        let d = self.to_top(r, mu);
        let dt = d / STEPS as f64;
        let (mut depth, mut column) = ([0.0; 3], 0.0);
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
            } * dt
                / 3.0;
            for c in 0..3 {
                depth[c] += e[c] * w;
            }
            column += self.densities(h.max(0.0)).1 * w;
        }
        (depth, column)
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

    /// The tables, as the GPU reads them (RGBA half floats): the
    /// transmittance (the Mie column in alpha) and multiple scattering,
    /// and the scaled medium's skylight.
    pub fn tables(&self) -> Tables {
        let (tw, th) = TRANSMITTANCE;
        let table: Vec<[f64; 4]> = (0..th)
            .flat_map(|y| (0..tw).map(move |x| (x, y)))
            .map(|(x, y)| {
                let (r, mu) = self.transmittance_at(x, y);
                let (depth, column) = self.optical(r, mu);
                [
                    (-depth[0]).exp(),
                    (-depth[1]).exp(),
                    (-depth[2]).exp(),
                    column,
                ]
            })
            .collect();
        // The true medium's and the scaled one's (as scene.wgsl's
        // `scaled` reads it).
        let truncated = self.mie_scattering.map(|x| x * self.truncation());
        let lookup = |r: f64, mu: f64, scaled: bool| -> [f64; 3] {
            if self.meets_ground(r, mu) {
                return [0.0; 3];
            }
            let (u, v) = self.transmittance_uv(r, mu);
            let t = bilinear(&table, tw, th, u, v);
            std::array::from_fn(|c| {
                t[c] * if scaled {
                    (truncated[c] * t[3]).exp()
                } else {
                    1.0
                }
            })
        };
        let (mw, mh) = MULTIPLE;
        let multiple_of = |a: &Atmosphere, scaled: bool| -> Vec<[f64; 3]> {
            (0..mh)
                .flat_map(|y| (0..mw).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let mu_s = -1.0 + 2.0 * (f64::from(x) + 0.5) / f64::from(mw);
                    let r = self.bottom
                        + (self.top - self.bottom) * (f64::from(y) + 0.5) / f64::from(mh);
                    a.multiple_scattering(r, mu_s, &|r, mu| lookup(r, mu, scaled))
                })
                .collect()
        };
        let multiple = multiple_of(self, false);
        let scaled = self.scaled();
        let scaled_multiple = multiple_of(&scaled, true);
        let psi = |r: f64, mu_s: f64| {
            let u = ((mu_s + 1.0) / 2.0).clamp(0.0, 1.0);
            let v = ((r - self.bottom) / (self.top - self.bottom)).clamp(0.0, 1.0);
            bilinear(&scaled_multiple, mw, mh, u, v)
        };
        let skylight: Vec<[f64; 3]> = (0..SKYLIGHT)
            .map(|x| {
                let mu_s = -1.0 + 2.0 * (f64::from(x) + 0.5) / f64::from(SKYLIGHT);
                scaled.skylight(mu_s, &|r, mu| lookup(r, mu, true), &psi)
            })
            .collect();
        Tables {
            transmittance: half_image(tw, th, &table),
            multiple: half_image(
                mw,
                mh,
                &multiple
                    .iter()
                    .map(|v| [v[0], v[1], v[2], 1.0])
                    .collect::<Vec<_>>(),
            ),
            skylight: half_image(
                SKYLIGHT,
                1,
                &skylight
                    .iter()
                    .map(|v| [v[0], v[1], v[2], 1.0])
                    .collect::<Vec<_>>(),
            ),
        }
    }

    /// The sky's irradiance on level ground, the Sun at cosine `mu_s`,
    /// for a Sun of unit irradiance: its radiance (light scattered once,
    /// and `psi`'s multiple scattering spread as scene.wgsl spreads it,
    /// along each ray up to the top, seen through `transmittance`) over
    /// the upper hemisphere (16 cosines by 32 azimuths), times each
    /// direction's cosine.
    fn skylight(
        &self,
        mu_s: f64,
        transmittance: &dyn Fn(f64, f64) -> [f64; 3],
        psi: &dyn Fn(f64, f64) -> [f64; 3],
    ) -> [f64; 3] {
        const RINGS: usize = 16;
        const AZIMUTHS: usize = 32;
        const STEPS: usize = 32;
        let r = self.bottom;
        let origin = DVec3::new(0.0, 0.0, r);
        let sun = DVec3::new((1.0 - mu_s * mu_s).max(0.0).sqrt(), 0.0, mu_s);
        let mut out = [0.0; 3];
        for i in 0..RINGS {
            let mu = (i as f64 + 0.5) / RINGS as f64;
            for j in 0..AZIMUTHS {
                let phi = std::f64::consts::TAU * (j as f64 + 0.5) / AZIMUTHS as f64;
                let s = (1.0 - mu * mu).sqrt();
                let dir = DVec3::new(s * phi.cos(), s * phi.sin(), mu);
                let c = dir.dot(sun);
                let (pr, pm) = (rayleigh_phase(c), cornette_shanks(self.mie_g, c));
                let g = asymmetry(self.mie_g);
                let spread_m = henyey_greenstein(g * g, c) * 4.0 * std::f64::consts::PI;
                let length = self.to_top(r, mu);
                let dt = length / STEPS as f64;
                let mut seen = [1.0; 3];
                for k in 0..STEPS {
                    let p = origin + dir * ((k as f64 + 0.5) * dt);
                    let radius = p.length();
                    let h = radius - self.bottom;
                    let (d_r, d_m, _) = self.densities(h.max(0.0));
                    let (s_all, e) = self.coefficients(h);
                    let up = p / radius;
                    let sun_t = transmittance(radius, up.dot(sun));
                    let ms = psi(radius, up.dot(sun));
                    let mie: f64 = self.mie_scattering.iter().sum::<f64>() * d_m;
                    let share = mie / s_all.iter().sum::<f64>().max(1e-30);
                    let spread = 1.0 + share * (spread_m - 1.0);
                    for c in 0..3 {
                        let single =
                            self.rayleigh[c] * d_r * pr + self.mie_scattering[c] * d_m * pm;
                        let source = single * sun_t[c] + s_all[c] * ms[c] * spread;
                        let step = (-e[c] * dt).exp();
                        let integral = if e[c] > 0.0 { (1.0 - step) / e[c] } else { dt };
                        // dω = dμ dφ, weighed by μ.
                        out[c] += seen[c]
                            * source
                            * integral
                            * mu
                            * (1.0 / RINGS as f64)
                            * (std::f64::consts::TAU / AZIMUTHS as f64);
                        seen[c] *= step;
                    }
                }
            }
        }
        out
    }

    /// Hillaire's multiple-scattering term Ψ at radius `r`, the Sun at
    /// cosine `mu_s`, for a Sun of unit irradiance: the second-order light
    /// L₂ arriving from every direction (64, over the sphere), scattered
    /// once more isotropically, and the share f of light scattered there
    /// that comes back, summed as a series: Ψ = L₂ / (1 − f). A point
    /// scattering σ_s adds σ_s Ψ. Light from the ground is left out: the
    /// ground's own term carries it (scene.wgsl's `sphere_fs`).
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
                let length = if self.meets_ground(r, mu) {
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
            truncation: self.truncation() as f32,
            mie_asymmetry: asymmetry(self.mie_g) as f32,
        }
    }
}

/// An atmosphere's tables (`Atmosphere::tables`).
#[derive(Clone, Debug)]
pub struct Tables {
    pub transmittance: Image,
    pub multiple: Image,
    pub skylight: Image,
}

/// An atmosphere with its tables, as a body's maps carry it.
#[derive(Clone, Debug)]
pub struct Air {
    pub atmosphere: Atmosphere,
    pub tables: Tables,
}

impl Air {
    /// `atmosphere`, its tables computed (a fraction of a second).
    pub fn new(atmosphere: Atmosphere) -> Air {
        Air {
            tables: atmosphere.tables(),
            atmosphere,
        }
    }
}

/// `table` (rows of `w`) at `(u, v)`, with texel centers at `(i + 0.5) /
/// n`, clamped at the edges.
fn bilinear<const N: usize>(table: &[[f64; N]], w: u32, h: u32, u: f64, v: f64) -> [f64; N] {
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

fn half_image(w: u32, h: u32, table: &[[f64; 4]]) -> Image {
    let data = table
        .iter()
        .flatten()
        .flat_map(|&x| f32_to_f16(x as f32).to_le_bytes())
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
    /// Mie scattering's share in its forward peak, delta-scaled.
    pub truncation: f32,
    /// Mie scattering's mean cosine.
    pub mie_asymmetry: f32,
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

    /// Under a thin Rayleigh sky (optical depth 0.01) on a nearly flat
    /// planet, the skylight on the ground is half the light scattered,
    /// whatever the Sun's height (Rayleigh's phase function sends as much
    /// down as up from any beam), less what the air takes on the way in
    /// and out: τ/2 (1 − τ/(2μ₀) − τ), within 1.5%. On Earth's sphere,
    /// rays near the horizon cross less air, and it was 3.8% under.
    #[test]
    fn thin_rayleigh_skylight_is_half_the_scattering() {
        let tau = 0.01;
        let a = Atmosphere {
            bottom: 1e6,
            top: 1e6 + 100.0,
            rayleigh: [tau / 8.0; 3],
            mie_scattering: [0.0; 3],
            mie_absorption: [0.0; 3],
            absorption: [0.0; 3],
            ..earth()
        };
        let table = a.tables().skylight;
        for mu_s in [1.0, 0.7, 0.5] {
            let x = (((mu_s + 1.0) / 2.0) * f64::from(SKYLIGHT) - 0.5).floor() as usize;
            let at = |x: usize| {
                f64::from(crate::offscreen::f16_to_f32(u16::from_le_bytes([
                    table.data[x * 8],
                    table.data[x * 8 + 1],
                ])))
            };
            // Between the two texels about mu_s.
            let u = ((mu_s + 1.0) / 2.0) * f64::from(SKYLIGHT) - 0.5 - x as f64;
            let got = at(x) * (1.0 - u) + at((x + 1).min(SKYLIGHT as usize - 1)) * u;
            let want = 0.5 * tau * (1.0 - tau / (2.0 * mu_s) - tau);
            println!("Sun at {mu_s}: {got:.5} (want {want:.5})");
            assert!((got / want - 1.0).abs() < 0.015, "{mu_s}: {got} vs {want}");
        }
    }

    /// Multiple scattering adds light, more in blue (Rayleigh) and more
    /// with the Sun high.
    #[test]
    fn multiple_scattering_is_bluer_with_the_sun_up() {
        let a = earth();
        let table = a.tables().multiple;
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
