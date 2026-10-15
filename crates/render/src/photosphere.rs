//! A glowing sphere's surface (M1c.md, step 6, part 11 plan): each
//! channel's light across its disk, and on it granulation, spots and
//! faculae, each a temperature that a table turns into the channels'
//! light. The scene's pass draws it (scene.wgsl, `photosphere`).

use bytemuck::{Pod, Zeroable};
use glam::DVec3;

/// Each channel's light across an even disk: 1 at every μ.
pub const EVEN: [[f32; 6]; 3] = [[1.0, 0.0, 0.0, 0.0, 0.0, 0.0]; 3];

/// The most temperatures a table holds.
pub const MAX_TEMPERATURES: usize = 48;

/// The most glowing spheres a scene draws with their own limbs and
/// surfaces; past them, the last one's.
pub const MAX_GLOWS: usize = 4;

/// The most features drawn, over all glowing spheres.
pub const MAX_FEATURES: usize = 512;

/// `Glows` in scene.wgsl, bytes: [`MAX_GLOWS`] glows, then
/// [`MAX_FEATURES`] features.
pub const GLOWS_SIZE: u64 = (MAX_GLOWS * std::mem::size_of::<GlowGpu>()
    + MAX_FEATURES * std::mem::size_of::<FeatureGpu>()) as u64;

/// Writes `glows` and `features` into `buffer` (of [`GLOWS_SIZE`]).
pub(crate) fn write(
    queue: &wgpu::Queue,
    buffer: &wgpu::Buffer,
    glows: &[GlowGpu],
    features: &[FeatureGpu],
) {
    let glows = &glows[..glows.len().min(MAX_GLOWS)];
    let features = &features[..features.len().min(MAX_FEATURES)];
    if !glows.is_empty() {
        queue.write_buffer(buffer, 0, bytemuck::cast_slice(glows));
    }
    if !features.is_empty() {
        let at = (MAX_GLOWS * std::mem::size_of::<GlowGpu>()) as u64;
        queue.write_buffer(buffer, at, bytemuck::cast_slice(features));
    }
}

/// What a glowing sphere's surface shows besides its limb.
#[derive(Clone, Debug)]
pub struct Photosphere {
    /// The quiet surface's temperature, K: where `temperatures` is 1.
    pub quiet: f32,
    /// Each channel's light from the surface at a temperature, over the
    /// quiet surface's: from `first` K every `step` K, held past the
    /// ends; at most [`MAX_TEMPERATURES`].
    pub temperatures: Vec<[f32; 3]>,
    pub first: f32,
    pub step: f32,
    pub granulation: Option<Granulation>,
    /// The spots' umbrae's and penumbrae's temperatures, K.
    pub umbra: f32,
    pub penumbra: f32,
    /// The faculae's temperature over the quiet surface's, K, at μ 0.1,
    /// 0.2, … 1.0 (held under 0.1).
    pub facular_excess: [f32; 10],
    /// Spots and faculae: the first [`MAX_FEATURES`] drawn.
    pub features: Vec<Feature>,
}

/// Cells on the surface, each a temperature over the quiet one's.
#[derive(Clone, Copy, Debug)]
pub struct Granulation {
    /// The cells' grid's spacing, km: a jittered grid's cells, sliced by
    /// the surface, are 1/1.46 its square in area on average.
    pub spacing_km: f64,
    /// The sphere's radius, km.
    pub radius_km: f64,
    /// Each cell's life, s: born small, grown, shrunk away, and born
    /// again elsewhere in its grid cell.
    pub lifetime_s: f64,
    /// The temperature's rms at disk center, K.
    pub rms_k: f32,
}

/// A spot or a facula, in the sphere's body frame.
#[derive(Clone, Copy, Debug)]
pub enum Feature {
    /// Its center (a unit vector), its radius (radians on the sphere),
    /// and its umbra's share of that.
    Spot {
        center: DVec3,
        radius: f64,
        umbra: f64,
    },
    /// A plage of faculae: its center, its radius (the Gaussian's, radians
    /// on the sphere), and its strength (of `facular_excess`, at most 1).
    Facula {
        center: DVec3,
        radius: f64,
        strength: f64,
    },
}

/// `Glow` in scene.wgsl: a glowing sphere's limb and surface.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct GlowGpu {
    /// The k-th coefficient of each channel's polynomial in μ.
    pub limb: [[f32; 4]; 6],
    pub quiet: f32,
    pub first: f32,
    pub step: f32,
    /// Entries in `temperatures`; 0 for no surface.
    pub count: u32,
    pub umbra: f32,
    pub penumbra: f32,
    pub features_start: u32,
    pub features_count: u32,
    /// The granulation's rms, K, with its fade over the frame's span
    /// (0 for none); its grid's spacing and the sphere's radius, km.
    pub granulation_k: f32,
    pub spacing_km: f32,
    pub radius_km: f32,
    /// The cells' lives: how many have passed (wrapping), and the share
    /// of the current one.
    pub cycle: u32,
    pub phase: f32,
    pub _pad: [f32; 3],
    pub facular: [[f32; 4]; 3],
    pub temperatures: [[f32; 4]; MAX_TEMPERATURES],
}

/// `Feature` in scene.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct FeatureGpu {
    pub center: [f32; 3],
    pub radius: f32,
    /// 0 a spot, 1 a facula.
    pub kind: u32,
    /// A spot's umbra's share; a facula's strength.
    pub share: f32,
    pub _pad: [f32; 2],
}

/// The share of a granule's life a frame may span before the granulation
/// begins to fade, and where it is gone: past them it would flicker.
const FRAME_FADE: (f64, f64) = (0.03, 0.15);

/// A glowing sphere's limb, and its `photosphere` if any, at `now` s with
/// a frame spanning `frame_s` s; its features appended to `features`,
/// but those below the horizon of an eye `eye` away from its center (in
/// its body frame, in its radii).
pub fn glow(
    limb: &[[f32; 6]; 3],
    photosphere: Option<&Photosphere>,
    (now, frame_s): (f64, f64),
    eye: DVec3,
    features: &mut Vec<FeatureGpu>,
) -> GlowGpu {
    let mut g = GlowGpu::zeroed();
    for (k, row) in g.limb.iter_mut().enumerate() {
        *row = [limb[0][k], limb[1][k], limb[2][k], 0.0];
    }
    let Some(p) = photosphere else {
        return g;
    };
    debug_assert!(!p.temperatures.is_empty() && p.temperatures.len() <= MAX_TEMPERATURES);
    g.quiet = p.quiet;
    g.first = p.first;
    g.step = p.step;
    g.count = p.temperatures.len().min(MAX_TEMPERATURES) as u32;
    for (slot, t) in g.temperatures.iter_mut().zip(&p.temperatures) {
        *slot = [t[0], t[1], t[2], 0.0];
    }
    g.umbra = p.umbra;
    g.penumbra = p.penumbra;
    for (k, x) in p.facular_excess.iter().enumerate() {
        g.facular[k / 4][k % 4] = *x;
    }
    if let Some(gr) = p.granulation {
        let span = frame_s / gr.lifetime_s;
        let (from, to) = FRAME_FADE;
        let fade = ((to - span) / (to - from)).clamp(0.0, 1.0);
        g.granulation_k = gr.rms_k * fade as f32;
        g.spacing_km = gr.spacing_km as f32;
        g.radius_km = gr.radius_km as f32;
        let lives = now / gr.lifetime_s;
        g.cycle = lives.floor().rem_euclid(u32::MAX as f64 + 1.0) as u32;
        g.phase = lives.rem_euclid(1.0) as f32;
    }
    g.features_start = features.len() as u32;
    // The eye's horizon on the sphere: the cosine from the sub-eye point.
    let (toward, horizon) = (eye.normalize_or_zero(), 1.0 / eye.length().max(1.0));
    let room = MAX_FEATURES.saturating_sub(features.len());
    for f in p.features.iter() {
        if features.len() - g.features_start as usize >= room {
            break;
        }
        let (center, radius, kind, share) = match *f {
            Feature::Spot {
                center,
                radius,
                umbra,
            } => (center, radius, 0, umbra),
            Feature::Facula {
                center,
                radius,
                strength,
            } => (center, radius, 1, strength),
        };
        // A facula's Gaussian reaches 3 of its radii.
        let reach = if kind == 1 { 3.0 * radius } else { radius };
        if center.dot(toward) < horizon - reach.sin() {
            continue;
        }
        features.push(FeatureGpu {
            center: center.as_vec3().to_array(),
            radius: radius as f32,
            kind,
            share: share as f32,
            _pad: [0.0; 2],
        });
    }
    g.features_count = features.len() as u32 - g.features_start;
    g
}
