//! The map view's renderer (M1c.md, The view): a scene drawn in linear
//! HDR into a texture that the app composites under its UI. It knows
//! nothing of orbits or missions.
//!
//! The target is `Rgba16Float`, linear, with 1.0 the SDR reference white
//! (scRGB's convention, which damascene composites in). Depth is reversed
//! (1 near, 0 at infinity) in a 32-bit float buffer, with no far plane.

pub mod camera;
pub mod offscreen;
pub mod scene;
pub mod timer;

use std::sync::Arc;

pub use camera::Camera;
pub use scene::{Marker, Scene, Sphere, Surface};
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

/// A storage buffer that grows to fit what is written to it.
struct Storage {
    buffer: wgpu::Buffer,
    label: &'static str,
}

impl Storage {
    fn new(device: &wgpu::Device, label: &'static str) -> Storage {
        Storage {
            buffer: Self::create(device, label, 4096),
            label,
        }
    }

    fn create(device: &wgpu::Device, label: &'static str, size: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Writes `data`; `true` if the buffer was replaced to fit it.
    fn write<T: bytemuck::Pod>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        data: &[T],
    ) -> bool {
        let bytes: &[u8] = bytemuck::cast_slice(data);
        let grew = bytes.len() as u64 > self.buffer.size();
        if grew {
            self.buffer =
                Self::create(device, self.label, (bytes.len() as u64).next_power_of_two());
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytes);
        }
        grew
    }
}

/// Draws scenes (scene.wgsl): ray-cast spheres, then points (additive),
/// then markers.
pub struct Renderer {
    frame: wgpu::Buffer,
    spheres: Storage,
    points: Storage,
    markers: Storage,
    layout: wgpu::BindGroupLayout,
    /// Over the point and marker buffers respectively (spheres in both).
    groups: Option<[wgpu::BindGroup; 2]>,
    sphere_pipeline: wgpu::RenderPipeline,
    point_pipeline: wgpu::RenderPipeline,
    marker_pipeline: wgpu::RenderPipeline,
}

impl Renderer {
    pub fn new(device: &wgpu::Device) -> Renderer {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty,
            count: None,
        };
        let storage = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene"),
            entries: &[
                entry(
                    0,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
                entry(1, storage),
                entry(2, storage),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label, vs, fs, blend: wgpu::BlendState, write_depth| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(write_depth),
                    // Reversed: nearer is larger.
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let additive = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        Renderer {
            frame: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scene frame"),
                size: std::mem::size_of::<scene::FrameGpu>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            spheres: Storage::new(device, "spheres"),
            points: Storage::new(device, "points"),
            markers: Storage::new(device, "markers"),
            groups: None,
            sphere_pipeline: pipeline(
                "spheres",
                "sphere_vs",
                "sphere_fs",
                wgpu::BlendState::REPLACE,
                true,
            ),
            point_pipeline: pipeline("points", "dot_vs", "dot_fs", additive, false),
            marker_pipeline: pipeline(
                "markers",
                "dot_vs",
                "dot_fs",
                wgpu::BlendState::REPLACE,
                true,
            ),
            layout,
        }
    }

    /// Records `scene` into `target`, with the pass's timestamps if
    /// given.
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &Target,
        scene: &Scene,
        timestamps: Option<wgpu::RenderPassTimestampWrites>,
    ) {
        let inst = scene.instances(target.size);
        if let Some(frame) = &inst.frame {
            queue.write_buffer(&self.frame, 0, bytemuck::bytes_of(frame));
        }
        let mut grew = self.spheres.write(device, queue, &inst.spheres);
        grew |= self.points.write(device, queue, &inst.points);
        grew |= self.markers.write(device, queue, &inst.markers);
        if grew || self.groups.is_none() {
            let group = |dots: &Storage| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("scene"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self.frame.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: self.spheres.buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: dots.buffer.as_entire_binding(),
                        },
                    ],
                })
            };
            self.groups = Some([group(&self.points), group(&self.markers)]);
        }
        let [r, g, b] = scene.background.map(f64::from);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
        if inst.frame.is_none() {
            return;
        }
        let [with_points, with_markers] = self.groups.as_ref().expect("bind groups");
        for (pipeline, group, count) in [
            (&self.sphere_pipeline, with_points, inst.spheres.len()),
            (&self.point_pipeline, with_points, inst.points.len()),
            (&self.marker_pipeline, with_markers, inst.markers.len()),
        ] {
            if count > 0 {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, group, &[]);
                pass.draw(0..6, 0..count as u32);
            }
        }
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
