//! What the renderer draws (M1c.md, step 3 plan), in 64-bit km, and its
//! reduction to 32-bit view-space instances for the GPU.
//!
//! Radiance is linear, in units of a white Lambertian surface under the
//! Sun at 1 au; [`Look::exposure`] maps it to the shown image. The
//! scene's pass writes it exposed, so that f16 holds the range shown.

use crate::atmosphere::AtmosphereGpu;
use crate::camera::{Camera, NEAR_KM};
use crate::maps::{Maps, Shading, SurfaceGpu};
use crate::paths::{MAX_POSES, Paths, Pose, split};
use crate::sky::Sky;
use bytemuck::{Pod, Zeroable};
use glam::{DMat3, DVec3};
use std::sync::Arc;

/// The astronomical unit, km.
pub const AU_KM: f64 = 149_597_870.7;

/// The width of a point of light's spot (a Gaussian, σ), pixels: about
/// a pixel across at half its peak.
pub const SPOT_SIGMA_PX: f32 = 0.6;

/// The largest share of a spot's flux one pixel takes (centered on it).
pub const SPOT_PEAK: f32 = 0.354;

/// The eye's resolution, radians (1′): a source it sees smaller than this
/// is a point to it, as bright as its flux (`Look::point_gain`).
pub const ACUITY: f64 = 2.909e-4;

/// A sphere from one pixel in radius to this fades from its point (a
/// spot carrying its flux times [`Look::point_gain`], lifted by
/// [`Look::body_lift`]) to its disk: the spot's share falling and the
/// disk's opacity rising, so that the flux shown runs on smoothly, and
/// the point's lift falling in stops with its share.
const FADE_PX: f64 = 3.0;

/// The most a spot's radiance per share reaches in any channel, exposed:
/// under f16's largest. A brighter spot is scaled down whole, its color
/// kept.
const SPOT_MAX: f64 = 60_000.0;

/// A sphere's quad is the whole screen when the cone of rays that hit it
/// is wider than this, or reaches behind the eye.
const BILLBOARD_MAX_HALF_ANGLE: f64 = 1.2;

#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub camera: Option<Camera>,
    /// Radiance where nothing is drawn.
    pub background: [f32; 3],
    pub sky: Option<Sky>,
    pub look: Look,
    /// Where the Sun's light comes from.
    pub sun: DVec3,
    /// The Sun's radius, km, for eclipses: other spheres hide part of its
    /// disk from a sphere's surface. 0: no eclipses.
    pub sun_radius: f64,
    pub spheres: Vec<Sphere>,
    pub markers: Vec<Marker>,
    /// Sets of paths, each uploaded when another is given in its place.
    pub paths: Vec<Arc<Paths>>,
    /// Where each frame the paths are in is now, by `Path::pose`.
    pub poses: Vec<Pose>,
    /// The moment drawn, s on the paths' time axis: before it they take
    /// their past colors.
    pub now: f64,
    /// Lines' width, pixels.
    pub line_px: f32,
}

/// How the scene's radiance is shown (M1c.md, step 6 plan, item 1).
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// Radiance to the shown image's units, where 1.0 is the SDR white.
    pub exposure: f32,
    /// The fraction of the image's energy that bloom spreads.
    pub bloom: f32,
    /// The output's peak over the SDR white; 1 on SDR, which is dithered.
    pub headroom: f32,
    /// The angle a pixel of the image subtends at the viewer's eye,
    /// radians, by which points of light are shown (`point_gain`); 0: the
    /// angle it subtends of the scene, as a camera records them.
    pub pixel: f32,
    /// The departures from what a camera would record, each 1 for none:
    /// what the sky's exposure (its stars' and the Milky Way's) is over
    /// the scene's;
    pub sky_lift: f32,
    /// what that of bodies drawn as points of light is;
    pub body_lift: f32,
    /// and what bodies' night lights are shown at over their radiance.
    pub night_lift: f32,
}

impl Look {
    /// What a point of light (a spot carrying a source's flux) is
    /// multiplied by, its source `radius` radians across the sky and
    /// landing where a pixel subtends `pixel` radians of it (its side):
    /// so that it brings the viewer's eye, through a pixel of
    /// [`Look::pixel`], the flux the source would, while the eye could
    /// not resolve the source ([`ACUITY`]); past that, falling as the
    /// source's peak would, to 1 where its image could be resolved.
    pub fn point_gain(&self, radius: f64, pixel: f64) -> f64 {
        if self.pixel <= 0.0 {
            return 1.0;
        }
        let k = (pixel / f64::from(self.pixel)).powi(2);
        let gain = k * (ACUITY / radius.max(ACUITY)).powi(2);
        gain.clamp(k.min(1.0), k.max(1.0))
    }
}

impl Default for Look {
    /// Radiance shown as it is, unbloomed, on SDR.
    fn default() -> Look {
        Look {
            exposure: 1.0,
            bloom: 0.0,
            headroom: 1.0,
            pixel: 0.0,
            sky_lift: 1.0,
            body_lift: 1.0,
            night_lift: 1.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Sphere {
    /// km.
    pub center: DVec3,
    pub radius: f64,
    pub surface: Surface,
    /// The body's frame, as columns in the scene's: x through its prime
    /// meridian, y 90° east, z its north pole. Its maps lie on it.
    pub axes: DMat3,
    /// With them, a lit sphere's albedo multiplies its color map.
    pub maps: Option<Arc<Maps>>,
    /// What a lit sphere's surface reflects (and its point carries) is
    /// multiplied by, per channel: what brings its shading to its measured
    /// brightness and color. Not its air, rings, night lights or glint.
    pub gain: [f32; 3],
    /// What its point carries over its disk's light, per channel: what
    /// its disk does not show the way its point stands in for it
    /// (Saturn's rings, drawn only on its disk; Earth's color, which its
    /// disk's air gives it), or no longer scales.
    pub point_scale: [f32; 3],
}

impl Sphere {
    /// A sphere without maps, its frame the scene's.
    pub fn plain(center: DVec3, radius: f64, surface: Surface) -> Sphere {
        Sphere {
            center,
            radius,
            surface,
            axes: DMat3::IDENTITY,
            maps: None,
            gain: [1.0; 3],
            point_scale: [1.0; 3],
        }
    }

    /// A lit sphere's albedo as a whole (with maps, their mean and their
    /// clouds'), before its gain, and how it is shaded: what its point
    /// stands in for. None for a glowing one.
    pub fn reflectance(&self) -> Option<([f32; 3], Shading)> {
        let Surface::Lit(albedo) = self.surface else {
            return None;
        };
        Some(match &self.maps {
            Some(m) => (
                std::array::from_fn(|c| {
                    (1.0 - m.cover) * albedo[c] * m.mean[c] + m.cover * m.cloud_albedo
                }),
                m.shading,
            ),
            None => (albedo, Shading::Lambert),
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Surface {
    /// Lambertian with this albedo, lit by the Sun.
    Lit([f32; 3]),
    /// Glowing (the Sun): its disk's mean radiance, darkened toward the
    /// limb by the linear law, I(μ) = I₀ (1 − u (1 − μ)), with `darkening`
    /// u per channel (0: even).
    Emissive {
        radiance: [f32; 3],
        darkening: [f32; 3],
    },
}

/// A shape of a fixed size in pixels, with a dark rim: a ship, or an
/// event on a path.
#[derive(Clone, Copy, Debug)]
pub struct Marker {
    pub position: DVec3,
    pub color: [f32; 3],
    pub size_px: f32,
    pub shape: Shape,
}

/// Shapes of dots (`Dot::shape` in scene.wgsl).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Shape {
    /// A point of light: a Gaussian spot ([`SPOT_SIGMA_PX`]) whose pixels
    /// sum to the dot's color, additive.
    Spot = 0,
    Disc = 1,
    Ring = 2,
    Diamond = 3,
    Cross = 4,
    /// A square with a rim.
    Block = 5,
}

/// The GPU's frame constants (`Frame` in scene.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct FrameGpu {
    pub focal: f32,
    pub aspect: f32,
    pub near: f32,
    pub tan_half: f32,
    pub viewport: [f32; 2],
    pub line_px: f32,
    pub exposure: f32,
    pub slots: [SlotGpu; MAX_POSES],
}

/// A pose seen from the eye (`Slot` in scene.wgsl): the eye in the
/// frame's coordinates, high and low parts, and the map from a vector in
/// them to view space (columns).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct SlotGpu {
    pub eye_hi: [f32; 4],
    pub eye_lo: [f32; 4],
    pub to_view: [[f32; 4]; 3],
}

/// `Sphere` in scene.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct SphereGpu {
    pub center: [f32; 3],
    pub radius: f32,
    pub altitude: f32,
    pub emissive: f32,
    pub irradiance: f32,
    pub fullscreen: f32,
    pub color: [f32; 3],
    pub opacity: f32,
    pub light: [f32; 3],
    pub _pad: f32,
    pub gain: [f32; 3],
    pub _pad2: f32,
    pub corners: [[f32; 4]; 4],
    pub surface: SurfaceGpu,
    pub atmosphere: AtmosphereGpu,
    pub shadows: ShadowsGpu,
    /// The rings' inner and outer radii, km (0 for none).
    pub rings: [f32; 4],
}

/// What may hide the Sun from a sphere (`Shadows` in scene.wgsl): the Sun
/// and up to [`OCCLUDERS`] spheres, each its center in view space and its
/// radius (0 for none).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct ShadowsGpu {
    pub sun: [f32; 4],
    pub occluders: [[f32; 4]; OCCLUDERS],
}

/// The most spheres that may shade one.
pub const OCCLUDERS: usize = 4;

/// `Dot` in scene.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct DotGpu {
    pub center: [f32; 3],
    /// Pixels across; a spot's σ.
    pub size: f32,
    pub color: [f32; 3],
    pub shape: u32,
}

/// A frame's instances: spheres resolved on screen, points for those
/// under a pixel (additive light), and markers.
#[derive(Clone, Debug, Default)]
pub struct Instances {
    pub frame: Option<FrameGpu>,
    pub spheres: Vec<SphereGpu>,
    /// Each sphere's maps, by `spheres`.
    pub maps: Vec<Option<Arc<Maps>>>,
    pub points: Vec<DotGpu>,
    pub markers: Vec<DotGpu>,
}

fn f32x3(v: DVec3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

/// The view's side planes, through the eye: what a sphere must reach to
/// be seen.
pub(crate) struct Frustum {
    /// Unit normals, inward, in view space.
    sides: [DVec3; 4],
}

impl Frustum {
    pub(crate) fn new(camera: &Camera, size: (u32, u32)) -> Frustum {
        let (w, h) = (f64::from(size.0.max(1)), f64::from(size.1.max(1)));
        let tan_half = (camera.fov_y / 2.0).tan();
        let aspect_half = tan_half * w / h;
        Frustum {
            sides: [
                DVec3::new(-1.0, 0.0, -aspect_half),
                DVec3::new(1.0, 0.0, -aspect_half),
                DVec3::new(0.0, -1.0, -tan_half),
                DVec3::new(0.0, 1.0, -tan_half),
            ]
            .map(DVec3::normalize),
        }
    }

    /// Whether a sphere at `center` (view space) of `radius` reaches in
    /// front of the eye and within the sides.
    pub(crate) fn sees(&self, center: DVec3, radius: f64) -> bool {
        center.z <= radius && self.sides.iter().all(|n| n.dot(center) >= -radius)
    }
}

/// The Lambertian phase function, 1 at full phase and 0 at new.
pub fn lambert_phase(alpha: f64) -> f64 {
    (alpha.sin() + (std::f64::consts::PI - alpha) * alpha.cos()) / std::f64::consts::PI
}

/// The Lommel–Seeliger law's phase function, likewise (Fairbairn 2005):
/// 1 − sin(α/2) tan(α/2) ln cot(α/4).
pub fn lommel_seeliger_phase(alpha: f64) -> f64 {
    if alpha <= 0.0 {
        return 1.0;
    }
    if alpha >= std::f64::consts::PI {
        return 0.0;
    }
    let half = alpha / 2.0;
    1.0 - half.sin() * half.tan() * (1.0 / (alpha / 4.0).tan()).ln()
}

/// A distant sphere's mean radiance over its disk at phase angle `alpha`,
/// for unit albedo and irradiance, shaded as scene.wgsl's `lit` shades
/// it: Lambert's (2/3) Φ(α); lunar-Lambert's share L(α) of
/// Lommel–Seeliger (radiance 2 cos i / (cos i + cos e): 1 over the full
/// disk), the rest Lambert.
pub fn disk_mean(shading: Shading, alpha: f64) -> f64 {
    let lambert = 2.0 / 3.0 * lambert_phase(alpha);
    match shading {
        Shading::Lambert => lambert,
        Shading::LunarLambert => {
            let a = alpha.to_degrees();
            let l = (1.0 + a * (-0.019 + a * (2.42e-4 - a * 1.46e-6))).clamp(0.0, 1.0);
            l * lommel_seeliger_phase(alpha) + (1.0 - l) * lambert
        }
    }
}

impl Scene {
    /// The spheres that may hide part of the Sun from some point of `s`:
    /// nearer the Sun, and with their disks, as seen from `s`, reaching
    /// the Sun's (allowing for `s`'s own size), the largest first.
    fn shadows(&self, s: &Sphere, cam: &Camera) -> ShadowsGpu {
        let mut out = ShadowsGpu::default();
        if self.sun_radius <= 0.0 || !matches!(s.surface, Surface::Lit(_)) {
            return out;
        }
        let to_sun = self.sun - s.center;
        let sun_angle = (self.sun_radius / to_sun.length()).min(1.0).asin();
        let mut found: Vec<(f64, &Sphere)> = self
            .spheres
            .iter()
            .filter(|o| matches!(o.surface, Surface::Lit(_)) && o.center != s.center)
            .filter_map(|o| {
                let to = o.center - s.center;
                let d = to.length();
                if d >= to_sun.length() || to.dot(to_sun) <= 0.0 {
                    return None;
                }
                let reach = sun_angle + ((o.radius + s.radius) / d).min(1.0).asin();
                (to.angle_between(to_sun) < reach).then_some((o.radius / d, o))
            })
            .collect();
        found.sort_by(|a, b| b.0.total_cmp(&a.0));
        let at = |c: DVec3, r: f64| {
            let v = cam.to_view(c);
            [v.x as f32, v.y as f32, v.z as f32, r as f32]
        };
        out.sun = at(self.sun, self.sun_radius);
        for (slot, (_, o)) in out.occluders.iter_mut().zip(found) {
            *slot = at(o.center, o.radius);
        }
        out
    }

    /// The scene from its camera on a viewport of `size` pixels, in view
    /// space relative to the eye: subtracted and rotated in 64 bits.
    pub fn instances(&self, size: (u32, u32)) -> Instances {
        let Some(cam) = self.camera else {
            return Instances::default();
        };
        let (w, h) = (f64::from(size.0.max(1)), f64::from(size.1.max(1)));
        let tan_half = (cam.fov_y / 2.0).tan();
        let pixel = cam.pixel_angle(size.1.max(1));
        let mut slots = [SlotGpu::default(); MAX_POSES];
        for (slot, pose) in slots.iter_mut().zip(&self.poses) {
            let (hi, lo) = {
                let eye = pose.from_world(cam.eye);
                let [x, y, z] = [eye.x, eye.y, eye.z].map(split);
                ([x[0], y[0], z[0], 0.0], [x[1], y[1], z[1], 0.0])
            };
            let column = |c: DVec3| {
                let v = cam.rotate(c * pose.scale);
                [v.x as f32, v.y as f32, v.z as f32, 0.0]
            };
            *slot = SlotGpu {
                eye_hi: hi,
                eye_lo: lo,
                to_view: [
                    column(pose.axes.x_axis),
                    column(pose.axes.y_axis),
                    column(pose.axes.z_axis),
                ],
            };
        }
        let mut out = Instances {
            frame: Some(FrameGpu {
                focal: (1.0 / tan_half) as f32,
                aspect: (w / h) as f32,
                near: NEAR_KM as f32,
                tan_half: tan_half as f32,
                viewport: [w as f32, h as f32],
                line_px: self.line_px,
                exposure: self.look.exposure,
                slots,
            }),
            ..Default::default()
        };
        let frustum = Frustum::new(&cam, size);
        // Far to near: a nearer body's atmosphere blends over a farther
        // body.
        let mut order: Vec<usize> = (0..self.spheres.len()).collect();
        let distance = |k: &usize| cam.to_view(self.spheres[*k].center).length();
        order.sort_by(|a, b| distance(b).total_cmp(&distance(a)));
        for s in order.iter().map(|&k| &self.spheres[k]) {
            let v = cam.to_view(s.center);
            let d = v.length();
            let altitude = d - s.radius;
            // What the sphere reaches: its atmosphere's top, if any.
            let air = s.maps.as_ref().and_then(|m| m.air.as_ref());
            let rings = s.maps.as_ref().and_then(|m| m.rings.as_ref());
            let outer = air
                .map_or(s.radius, |a| a.atmosphere.top)
                .max(rings.map_or(0.0, |r| r.outer));
            // Inside it, or wholly behind the eye or off the screen.
            if altitude <= 0.0 || !frustum.sees(v, outer) {
                continue;
            }
            let half_angle = (s.radius / d).asin();
            // A pixel off the axis subtends less: pixel² cos³θ of sky.
            let cos = -v.z / d;
            let local_pixel = pixel * cos.max(0.0).powf(1.5);
            let lit = |albedo: [f32; 3]| {
                let to_sun = self.sun - s.center;
                let irradiance = (AU_KM / to_sun.length()).powi(2);
                (albedo, irradiance, to_sun.normalize(), to_sun)
            };
            // The sphere's flux as a spot: as the radiance of the pixel it
            // is spread over.
            let radiance = {
                let disk = std::f64::consts::PI * half_angle * half_angle;
                let spread = local_pixel * local_pixel;
                match s.surface {
                    Surface::Emissive { radiance, .. } => {
                        radiance.map(|c| f64::from(c) * disk / spread)
                    }
                    Surface::Lit(_) => {
                        let (albedo, shading) = s.reflectance().expect("lit");
                        let (albedo, irradiance, _, to_sun) = lit(albedo);
                        let alpha = to_sun.angle_between(cam.eye - s.center);
                        let mean = irradiance * disk_mean(shading, alpha);
                        std::array::from_fn(|c| {
                            let carried = f64::from(s.gain[c]) * f64::from(s.point_scale[c]);
                            f64::from(albedo[c]) * carried * mean * disk / spread
                        })
                    }
                }
            };
            // That spot's flux times `k`, shown at the frame's exposure;
            // in front of the sphere's disk (and its air and rings), which
            // it fades into: along its center's ray, nearer than anything
            // of it (every pixel of the spot takes the center's depth).
            let (exposure, lift) = (f64::from(self.look.exposure), f64::from(self.look.body_lift));
            let front = v * (1.0 - 1.01 * outer / -v.z).max(0.0);
            let mut spot = |k: f64| {
                let color = radiance.map(|x| x * k * exposure);
                let most = color.into_iter().fold(0.0, f64::max);
                let within = if most > SPOT_MAX { SPOT_MAX / most } else { 1.0 };
                out.points.push(DotGpu {
                    center: f32x3(front),
                    size: SPOT_SIGMA_PX,
                    color: color.map(|x| (x * within) as f32),
                    shape: Shape::Spot as u32,
                });
            };
            let gain = self.look.point_gain(half_angle, local_pixel);
            let radius_px = half_angle / local_pixel;
            if radius_px < 1.0 {
                // Under a pixel: a point of light, lifted.
                if front.z < -NEAR_KM {
                    spot(gain * lift);
                }
                continue;
            }
            // Its point's share, and the stops of its lift, while the disk
            // fades in.
            let fade = ((FADE_PX - radius_px) / (FADE_PX - 1.0)).max(0.0);
            if fade > 0.0 {
                spot(gain * fade * lift.powf(fade));
            }
            // The quad: across the cone of rays that hit the sphere (or its
            // atmosphere), in the plane through its center facing the eye;
            // the whole screen when that is too wide, reaches behind the
            // eye, or the eye is in the atmosphere.
            let outer_angle = (outer / d).min(1.0).asin();
            let reach = d * outer_angle.tan() * 1.02;
            let e1 = (v / d).any_orthonormal_vector();
            let e2 = (v / d).cross(e1);
            let corners: Vec<DVec3> = [-1.0, 1.0]
                .into_iter()
                .flat_map(|y| [-1.0, 1.0].map(|x| v + (e1 * x + e2 * y) * reach))
                .collect();
            let billboard = (outer < d
                && outer_angle <= BILLBOARD_MAX_HALF_ANGLE
                && corners.iter().all(|c| c.z < -NEAR_KM))
            .then(|| {
                std::array::from_fn(|k| {
                    let c = corners[k];
                    [c.x as f32, c.y as f32, c.z as f32, 1.0]
                })
            });
            let (emissive, color, irradiance, light) = match s.surface {
                Surface::Lit(albedo) => {
                    let (albedo, irradiance, light, _) = lit(albedo);
                    (0.0, albedo, irradiance as f32, f32x3(cam.rotate(light)))
                }
                // For a glowing sphere, `light` holds its darkening.
                Surface::Emissive {
                    radiance,
                    darkening,
                } => (1.0, radiance, 0.0, darkening),
            };
            out.spheres.push(SphereGpu {
                center: f32x3(v),
                radius: s.radius as f32,
                altitude: altitude as f32,
                emissive,
                irradiance,
                fullscreen: if billboard.is_some() { 0.0 } else { 1.0 },
                color,
                opacity: (1.0 - fade) as f32,
                light,
                _pad: 0.0,
                gain: s.gain,
                _pad2: 0.0,
                corners: billboard.unwrap_or_default(),
                surface: {
                    let column = |c: DVec3| {
                        let v = cam.rotate(c);
                        [v.x as f32, v.y as f32, v.z as f32, 0.0]
                    };
                    let m = s.maps.as_deref();
                    SurfaceGpu {
                        axes: [
                            column(s.axes.x_axis),
                            column(s.axes.y_axis),
                            column(s.axes.z_axis),
                        ],
                        night: m.map_or([0.0; 3], |m| {
                            m.night_radiance.map(|c| c * self.look.night_lift)
                        }),
                        flags: m.map_or(0, Maps::flags),
                        cloud_albedo: m.map_or(0.0, |m| m.cloud_albedo),
                        _pad: [0.0; 3],
                    }
                },
                atmosphere: air.map_or_else(AtmosphereGpu::default, |a| a.atmosphere.gpu()),
                shadows: self.shadows(s, &cam),
                rings: rings.map_or([0.0; 4], |r| [r.inner as f32, r.outer as f32, 0.0, 0.0]),
            });
            out.maps.push(s.maps.clone());
        }
        for m in &self.markers {
            let v = cam.to_view(m.position);
            if v.z < -NEAR_KM {
                out.markers.push(DotGpu {
                    center: f32x3(v),
                    size: m.size_px,
                    color: m.color,
                    shape: m.shape as u32,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point's gain, through a viewer's pixel a third the scene's (k =
    /// 9): k for a star and for a source under the eye's resolution,
    /// falling as 1 / radius² past it, to 1 at √k × it and beyond.
    /// Through a viewer's pixel twice the scene's (k = 1/4), k for any
    /// source; through a camera's (0), 1.
    #[test]
    fn points_bring_the_eye_their_flux() {
        let look = Look {
            pixel: 1e-4,
            ..Default::default()
        };
        let near = |a: f64, b: f64| (a / b - 1.0).abs() < 1e-6;
        let pixel = 3.0 * f64::from(look.pixel);
        assert!(near(look.point_gain(0.0, pixel), 9.0));
        assert!(near(look.point_gain(ACUITY, pixel), 9.0));
        assert!(near(look.point_gain(2.0 * ACUITY, pixel), 9.0 / 4.0));
        assert!(near(look.point_gain(3.0 * ACUITY, pixel), 1.0));
        assert_eq!(look.point_gain(10.0 * ACUITY, pixel), 1.0);
        let pixel = f64::from(look.pixel) / 2.0;
        assert!(near(look.point_gain(0.0, pixel), 0.25));
        assert!(near(look.point_gain(10.0 * ACUITY, pixel), 0.25));
        assert_eq!(Look::default().point_gain(0.0, pixel), 1.0);
    }

    /// `disk_mean` against the laws integrated in f64 over a distant
    /// sphere's disk (2000² samples), at phase angles from 0 to 170°: the
    /// closed forms, and their normalization.
    #[test]
    fn disk_mean_integrates_the_laws() {
        let n = 2000;
        for degrees in [0.0f64, 1.0, 30.0, 60.0, 90.0, 120.0, 170.0] {
            let alpha = degrees.to_radians();
            // Seen along -z; the Sun at `alpha` from the eye, toward +x.
            let light = DVec3::new(alpha.sin(), 0.0, alpha.cos());
            let a = degrees;
            let l = (1.0 + a * (-0.019 + a * (2.42e-4 - a * 1.46e-6))).clamp(0.0, 1.0);
            let (mut lambert, mut lunar) = (0.0, 0.0);
            for i in 0..n {
                for j in 0..n {
                    let x = (f64::from(i) + 0.5) / f64::from(n) * 2.0 - 1.0;
                    let y = (f64::from(j) + 0.5) / f64::from(n) * 2.0 - 1.0;
                    let zz = 1.0 - x * x - y * y;
                    if zz <= 0.0 {
                        continue;
                    }
                    let normal = DVec3::new(x, y, zz.sqrt());
                    let (cos_i, cos_e) = (normal.dot(light).max(0.0), normal.z);
                    lambert += cos_i;
                    lunar += 2.0 * l * cos_i / (cos_i + cos_e) + (1.0 - l) * cos_i;
                }
            }
            // Each sample's area over the disk's.
            let per = 4.0 / f64::from(n * n) / std::f64::consts::PI;
            for (shading, integral) in [(Shading::Lambert, lambert), (Shading::LunarLambert, lunar)]
            {
                let (got, want) = (disk_mean(shading, alpha), integral * per);
                assert!(
                    (got - want).abs() < 2e-3 * want.max(0.01),
                    "{shading:?} at {degrees}°: {got} vs {want}"
                );
            }
        }
    }
}
