//! What the renderer draws (M1c.md, step 3 plan), in 64-bit km, and its
//! reduction to 32-bit view-space instances for the GPU.
//!
//! Radiance is linear, in units of a white Lambertian surface under the
//! Sun at 1 au: 1.0 is SDR white until step 6's exposure.

use crate::camera::{Camera, NEAR_KM};
use crate::paths::{MAX_POSES, Paths, Pose, split};
use bytemuck::{Pod, Zeroable};
use glam::DVec3;
use std::sync::Arc;

/// The astronomical unit, km.
pub const AU_KM: f64 = 149_597_870.7;

/// Pixels across a point standing in for a sphere under a pixel.
pub const POINT_PX: f64 = 2.0;

/// A sphere's quad is the whole screen when the cone of rays that hit it
/// is wider than this, or reaches behind the eye.
const BILLBOARD_MAX_HALF_ANGLE: f64 = 1.2;

#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub camera: Option<Camera>,
    pub background: [f32; 3],
    /// Where the Sun's light comes from.
    pub sun: DVec3,
    pub spheres: Vec<Sphere>,
    pub markers: Vec<Marker>,
    pub paths: Option<Arc<Paths>>,
    /// Where each frame the paths are in is now, by `Path::pose`.
    pub poses: Vec<Pose>,
    /// The moment drawn, s on the paths' time axis: before it they take
    /// their past colors.
    pub now: f64,
    /// Lines' width, pixels.
    pub line_px: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Sphere {
    /// km.
    pub center: DVec3,
    pub radius: f64,
    pub surface: Surface,
}

#[derive(Clone, Copy, Debug)]
pub enum Surface {
    /// Lambertian with this albedo, lit by the Sun.
    Lit([f32; 3]),
    /// Glowing with this radiance (the Sun).
    Emissive([f32; 3]),
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
    /// Hard-edged, for points of light.
    Square = 0,
    Disc = 1,
    Ring = 2,
    Diamond = 3,
    Cross = 4,
    /// A square with a rim and antialiased, unlike [`Shape::Square`].
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
    pub _pad: u32,
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
    pub _pad0: f32,
    pub light: [f32; 3],
    pub _pad1: f32,
    pub corners: [[f32; 4]; 4],
}

/// `Dot` in scene.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct DotGpu {
    pub center: [f32; 3],
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

impl Scene {
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
                _pad: 0,
                slots,
            }),
            ..Default::default()
        };
        let frustum = Frustum::new(&cam, size);
        for s in &self.spheres {
            let v = cam.to_view(s.center);
            let d = v.length();
            let altitude = d - s.radius;
            // Inside it, or wholly behind the eye or off the screen.
            if altitude <= 0.0 || !frustum.sees(v, s.radius) {
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
            if half_angle < local_pixel {
                // Under a pixel: a point with the sphere's flux.
                if v.z >= -NEAR_KM {
                    continue;
                }
                let disk = std::f64::consts::PI * half_angle * half_angle;
                let spread = (POINT_PX * local_pixel).powi(2);
                let radiance = match s.surface {
                    Surface::Emissive(c) => c.map(|c| f64::from(c) * disk / spread),
                    Surface::Lit(albedo) => {
                        let (albedo, irradiance, _, to_sun) = lit(albedo);
                        let alpha = to_sun.angle_between(cam.eye - s.center);
                        let mean = 2.0 / 3.0 * irradiance * lambert_phase(alpha);
                        albedo.map(|a| f64::from(a) * mean * disk / spread)
                    }
                };
                out.points.push(DotGpu {
                    center: f32x3(v),
                    size: POINT_PX as f32,
                    color: radiance.map(|x| x as f32),
                    shape: Shape::Square as u32,
                });
                continue;
            }
            // The quad: across the cone of rays that hit the sphere, in the
            // plane through its center facing the eye; the whole screen
            // when that is too wide or reaches behind the eye.
            let reach = d * half_angle.tan() * 1.02;
            let e1 = (v / d).any_orthonormal_vector();
            let e2 = (v / d).cross(e1);
            let corners: Vec<DVec3> = [-1.0, 1.0]
                .into_iter()
                .flat_map(|y| [-1.0, 1.0].map(|x| v + (e1 * x + e2 * y) * reach))
                .collect();
            let billboard = (half_angle <= BILLBOARD_MAX_HALF_ANGLE
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
                Surface::Emissive(c) => (1.0, c, 0.0, [0.0; 3]),
            };
            out.spheres.push(SphereGpu {
                center: f32x3(v),
                radius: s.radius as f32,
                altitude: altitude as f32,
                emissive,
                irradiance,
                fullscreen: if billboard.is_some() { 0.0 } else { 1.0 },
                color,
                _pad0: 0.0,
                light,
                _pad1: 0.0,
                corners: billboard.unwrap_or_default(),
            });
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
