//! The map view's renderer (M1c.md, The view): a scene drawn in linear
//! HDR, then exposed, bloomed and tonemapped into a texture that the app
//! composites under its UI, with lines and markers over it. It knows
//! nothing of orbits or missions.
//!
//! Textures are `Rgba16Float`, linear. The scene's is radiance (scene.rs
//! has its units); the shown one has 1.0 the SDR reference white
//! (scRGB's convention, which damascene composites in), up to the
//! output's headroom. Depth is reversed (1 near, 0 at infinity) in a
//! 32-bit float buffer, with no far plane.

pub mod atmosphere;
pub mod camera;
mod lines;
pub mod maps;
pub mod offscreen;
pub mod paths;
mod post;
pub mod scene;
pub mod shape;
mod sky;
pub mod timer;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub use camera::Camera;
pub use maps::{Image, Maps, Shading};
pub use paths::{Path, Paths, Pose, Sample};
pub use scene::{Look, Marker, Scene, Shape, Sphere, Surface};
pub use sky::{Sky, Star};
pub use timer::{GpuTimer, Timings};

/// The scene's and the shown image's color format.
pub const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The scene's depth format.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Where the scene is drawn: its radiance, the image the app composites
/// (and tests read back), depth, and bloom's levels.
pub struct Target {
    /// Tells targets apart, for what the renderer keeps per target.
    id: u64,
    /// The shown image. Shared, since the app's UI holds it too.
    pub color: Arc<wgpu::Texture>,
    pub color_view: wgpu::TextureView,
    /// The scene's radiance, exposed (before bloom and the tonemap).
    pub hdr: wgpu::Texture,
    pub hdr_view: wgpu::TextureView,
    pub depth_view: wgpu::TextureView,
    /// Half size, and halving.
    bloom: Vec<wgpu::TextureView>,
    /// Pixels.
    pub size: (u32, u32),
}

impl Target {
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Target {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let size = wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        };
        let texture = |label, size, format, usage| {
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
        let shown = U::RENDER_ATTACHMENT | U::TEXTURE_BINDING | U::COPY_SRC;
        let color = Arc::new(texture("shown color", size, COLOR_FORMAT, shown));
        let hdr = texture("scene radiance", size, COLOR_FORMAT, shown);
        let depth = texture("scene depth", size, DEPTH_FORMAT, U::RENDER_ATTACHMENT);
        let (w, h) = (size.width, size.height);
        let bloom = (1..=post::levels((w, h)))
            .map(|k| {
                let level = wgpu::Extent3d {
                    width: (w >> k).max(1),
                    height: (h >> k).max(1),
                    depth_or_array_layers: 1,
                };
                texture(
                    "bloom",
                    level,
                    COLOR_FORMAT,
                    U::RENDER_ATTACHMENT | U::TEXTURE_BINDING,
                )
                .create_view(&Default::default())
            })
            .collect();
        Target {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            color_view: color.create_view(&Default::default()),
            color,
            hdr_view: hdr.create_view(&Default::default()),
            hdr,
            depth_view: depth.create_view(&Default::default()),
            bloom,
            size: (w, h),
        }
    }
}

/// A storage buffer that grows to fit what is written to it.
pub(crate) struct Storage {
    pub(crate) buffer: wgpu::Buffer,
    label: &'static str,
}

impl Storage {
    pub(crate) fn new(device: &wgpu::Device, label: &'static str) -> Storage {
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
    pub(crate) fn write<T: bytemuck::Pod>(
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

/// Draws scenes (scene.wgsl): the scene's pass (the sky, then ray-cast
/// spheres, each with its maps, shaped ones marched, then points,
/// additive) into its radiance; bloom and the tonemap into the shown image
/// (post.wgsl); and over that, depth-tested against the scene, the overlay
/// (each set of paths' lines, then markers), which exposure does not
/// touch.
pub struct Renderer {
    frame: wgpu::Buffer,
    spheres: Storage,
    points: Storage,
    markers: Storage,
    /// By the scene's sets of paths.
    path_sets: Vec<lines::PathSet>,
    layout: wgpu::BindGroupLayout,
    path_layout: wgpu::BindGroupLayout,
    /// Over the point and marker buffers respectively (spheres in both).
    groups: Option<[wgpu::BindGroup; 2]>,
    sphere_pipeline: wgpu::RenderPipeline,
    shape_pipeline: wgpu::RenderPipeline,
    point_pipeline: wgpu::RenderPipeline,
    marker_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    piece_pipeline: wgpu::RenderPipeline,
    post: post::Post,
    sky: sky::SkyPass,
    maps: maps::MapsPass,
}

impl Renderer {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Renderer {
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
        let path_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("paths"),
            entries: &[
                entry(0, storage),
                entry(1, storage),
                entry(2, storage),
                entry(3, storage),
                entry(4, storage),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let maps = maps::MapsPass::new(device, queue);
        let sphere_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("spheres"),
            bind_group_layouts: &[Some(&layout), None, None, Some(&maps.layout)],
            immediate_size: 0,
        });
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lines"),
            bind_group_layouts: &[Some(&layout), Some(&path_layout)],
            immediate_size: 0,
        });
        let pipeline = |label, layout, vs, fs, blend: wgpu::BlendState, write_depth| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
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
        let over = wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING;
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
            path_sets: Vec::new(),
            groups: None,
            sphere_pipeline: pipeline(
                "spheres",
                &sphere_layout,
                "sphere_vs",
                "sphere_fs",
                over,
                true,
            ),
            shape_pipeline: pipeline(
                "shapes",
                &sphere_layout,
                "sphere_vs",
                "shape_fs",
                over,
                true,
            ),
            point_pipeline: pipeline(
                "points",
                &pipeline_layout,
                "dot_vs",
                "dot_fs",
                additive,
                false,
            ),
            marker_pipeline: pipeline("markers", &pipeline_layout, "dot_vs", "dot_fs", over, false),
            line_pipeline: pipeline("lines", &line_layout, "line_vs", "line_fs", over, false),
            piece_pipeline: pipeline("pieces", &line_layout, "piece_vs", "line_fs", over, false),
            post: post::Post::new(device),
            sky: sky::SkyPass::new(device, queue, &shader, &layout),
            maps,
            layout,
            path_layout,
        }
    }

    /// Records `scene` into `target`: its passes timed by `timestamps` if
    /// given, from the scene's start to the overlay's end.
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
        let mut grew = false;
        while self.path_sets.len() < scene.paths.len() {
            self.path_sets.push(lines::PathSet::new(device));
        }
        if let Some(camera) = &scene.camera {
            for (set, paths) in self.path_sets.iter_mut().zip(&scene.paths) {
                set.prepare(
                    device,
                    queue,
                    &self.path_layout,
                    paths,
                    camera,
                    &scene.poses,
                    target.size,
                    scene.now,
                );
            }
        }
        if let Some(frame) = &inst.frame {
            queue.write_buffer(&self.frame, 0, bytemuck::bytes_of(frame));
        }
        grew |= self.spheres.write(device, queue, &inst.spheres);
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
        let (first, last) = match &timestamps {
            Some(t) => (
                Some(wgpu::RenderPassTimestampWrites {
                    query_set: t.query_set,
                    beginning_of_pass_write_index: t.beginning_of_pass_write_index,
                    end_of_pass_write_index: None,
                }),
                Some(wgpu::RenderPassTimestampWrites {
                    query_set: t.query_set,
                    beginning_of_pass_write_index: None,
                    end_of_pass_write_index: t.end_of_pass_write_index,
                }),
            ),
            None => (None, None),
        };
        let sky = self.sky.prepare(device, queue, encoder, &self.post, scene);
        self.maps
            .prepare(device, queue, encoder, &self.post, &inst.maps);
        let [r, g, b] = scene
            .background
            .map(|c| f64::from((c * scene.look.exposure).min(60_000.0)));
        let [with_points, with_markers] = self.groups.as_ref().expect("bind groups");
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.hdr_view,
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
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: first,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if inst.frame.is_some() {
                if sky {
                    self.sky.draw(&mut pass, with_points);
                }
                pass.set_bind_group(0, with_points, &[]);
                // Each sphere with its maps; a shaped one marched.
                for (k, maps) in inst.maps.iter().enumerate() {
                    let shaped = maps.as_ref().is_some_and(|m| m.shape.is_some());
                    pass.set_pipeline(if shaped {
                        &self.shape_pipeline
                    } else {
                        &self.sphere_pipeline
                    });
                    pass.set_bind_group(3, self.maps.group(maps.as_ref()), &[]);
                    pass.draw(0..6, k as u32..k as u32 + 1);
                }
                if !inst.points.is_empty() {
                    pass.set_pipeline(&self.point_pipeline);
                    pass.draw(0..6, 0..inst.points.len() as u32);
                }
            }
        }
        self.post
            .record(device, queue, encoder, target, &scene.look);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: last,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if inst.frame.is_none() {
            return;
        }
        pass.set_bind_group(0, with_markers, &[]);
        for set in &self.path_sets[..scene.paths.len()] {
            set.draw(&mut pass, &self.line_pipeline, &self.piece_pipeline);
        }
        if !inst.markers.is_empty() {
            pass.set_pipeline(&self.marker_pipeline);
            pass.set_bind_group(0, with_markers, &[]);
            pass.draw(0..6, 0..inst.markers.len() as u32);
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
