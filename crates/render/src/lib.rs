//! The map view's renderer (M1c.md, The view): a scene drawn in linear
//! HDR into a texture that the app composites under its UI. It knows
//! nothing of orbits or missions.
//!
//! The target is `Rgba16Float`, linear, with 1.0 the SDR reference white
//! (scRGB's convention, which damascene composites in). Depth is reversed
//! (1 near, 0 at infinity) in a 32-bit float buffer, with no far plane.

pub mod offscreen;
pub mod timer;

use std::sync::Arc;

pub use timer::{GpuTimer, Timings};

/// The scene's color format.
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The scene's depth format.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Where the scene is drawn: color the app composites (and tests read
/// back), and depth.
pub struct Target {
    /// Shared, since the app's UI holds it too.
    pub color: Arc<wgpu::Texture>,
    pub color_view: wgpu::TextureView,
    pub depth_view: wgpu::TextureView,
    /// Pixels.
    pub size: (u32, u32),
}

impl Target {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Target {
        let size = wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        };
        let texture = |label, format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        use wgpu::TextureUsages as U;
        let color = Arc::new(texture(
            "scene color",
            COLOR_FORMAT,
            U::RENDER_ATTACHMENT | U::TEXTURE_BINDING | U::COPY_SRC,
        ));
        let depth = texture("scene depth", DEPTH_FORMAT, U::RENDER_ATTACHMENT);
        Target {
            color_view: color.create_view(&Default::default()),
            depth_view: depth.create_view(&Default::default()),
            color,
            size: (size.width, size.height),
        }
    }
}

/// What to draw. Empty so far: space, in a linear color.
#[derive(Clone, Copy, Debug)]
pub struct Scene {
    pub background: [f32; 3],
}

impl Default for Scene {
    fn default() -> Scene {
        Scene {
            background: [0.0; 3],
        }
    }
}

/// Draws scenes. Holds the pipelines (none yet).
pub struct Renderer {}

impl Renderer {
    pub fn new(_device: &wgpu::Device) -> Renderer {
        Renderer {}
    }

    /// Records `scene` into `target`, with the pass's timestamps if
    /// given.
    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target: &Target,
        scene: &Scene,
        timestamps: Option<wgpu::RenderPassTimestampWrites>,
    ) {
        let [r, g, b] = scene.background.map(f64::from);
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r, g, b, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth_view,
                // Reversed: infinitely far is 0.
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    }
}

/// The optional features the renderer uses where the adapter has them:
/// GPU timestamps, in passes and between them.
pub fn wanted_features(adapter: &wgpu::Adapter) -> wgpu::Features {
    adapter.features()
        & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS)
}

/// A device on `adapter` with [`wanted_features`] and the adapter's
/// limits where they fall below the defaults.
pub fn request_device(
    adapter: &wgpu::Adapter,
    label: &str,
) -> Result<(wgpu::Device, wgpu::Queue), wgpu::RequestDeviceError> {
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some(label),
        required_features: wanted_features(adapter),
        required_limits: wgpu::Limits::default().or_worse_values_from(&adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
}
