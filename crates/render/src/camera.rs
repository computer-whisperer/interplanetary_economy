//! The eye: a 64-bit position and orientation (M1c.md, step 3 plan,
//! item 1). Everything is moved into view space in 64 bits, relative to
//! the eye, before the GPU sees it.

use glam::{DVec2, DVec3};

/// The near plane, km: depth is reversed with no far plane, so a small
/// near costs no precision far away.
pub const NEAR_KM: f64 = 1e-3;

/// A perspective eye. View space is right-handed: x right, y up, looking
/// down -z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// km, in the scene's frame.
    pub eye: DVec3,
    /// Unit and orthogonal.
    pub right: DVec3,
    pub up: DVec3,
    pub forward: DVec3,
    /// Vertical field of view, radians.
    pub fov_y: f64,
}

impl Camera {
    /// An eye at `eye` looking at `target`, with `up` (not parallel to
    /// the view) toward the top of the screen.
    pub fn looking_at(eye: DVec3, target: DVec3, up: DVec3, fov_y: f64) -> Camera {
        let forward = (target - eye).normalize();
        let right = forward.cross(up).normalize();
        Camera {
            eye,
            right,
            up: right.cross(forward),
            forward,
            fov_y,
        }
    }

    /// A direction in view space.
    pub fn rotate(&self, d: DVec3) -> DVec3 {
        DVec3::new(d.dot(self.right), d.dot(self.up), -d.dot(self.forward))
    }

    /// A point in view space.
    pub fn to_view(&self, p: DVec3) -> DVec3 {
        self.rotate(p - self.eye)
    }

    /// Radians per pixel on a viewport `height` pixels tall, at its center.
    pub fn pixel_angle(&self, height: u32) -> f64 {
        2.0 * (self.fov_y / 2.0).tan() / f64::from(height)
    }

    /// Where `p` lands on a viewport of `size` pixels (x right, y down,
    /// from the top-left corner), or `None` behind the eye.
    pub fn project(&self, p: DVec3, size: (u32, u32)) -> Option<DVec2> {
        let v = self.to_view(p);
        if v.z >= 0.0 {
            return None;
        }
        let (w, h) = (f64::from(size.0), f64::from(size.1));
        let f = 1.0 / (self.fov_y / 2.0).tan();
        let ndc = DVec2::new(v.x * f * h / w / -v.z, v.y * f / -v.z);
        Some(DVec2::new((ndc.x + 1.0) * w / 2.0, (1.0 - ndc.y) * h / 2.0))
    }
}
