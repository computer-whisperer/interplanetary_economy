//! The sky (M1c.md, step 6 plan, item 2; scene.wgsl): the Milky Way
//! behind everything, then the catalog's stars as spots, in the sky's own
//! coordinates (equatorial, ICRS) turned into the scene's.

use crate::maps::Image;
use crate::{COLOR_FORMAT, DEPTH_FORMAT, Storage};
use bytemuck::{Pod, Zeroable};
use glam::DMat3;
use std::sync::Arc;

/// A catalog star (`Star` in scene.wgsl), as `data/sky/stars.bin` stores
/// it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Star {
    /// Unit, in the sky's coordinates, at J2000.
    pub direction: [f32; 3],
    /// Radiance × sr in the largest channel (radiance in scene.rs's
    /// units).
    pub flux: f32,
    /// Proper motion, radians per Julian year: a vector tangent to the
    /// sky at `direction`.
    pub motion: [f32; 3],
    /// Linear r, g, b in the low three bytes, the largest 255.
    pub color: u32,
}

/// What the sky shows.
#[derive(Clone, Debug)]
pub struct Sky {
    /// Uploaded when another list is given.
    pub stars: Arc<Vec<Star>>,
    /// Equirectangular: right ascension 0h at the center, increasing to
    /// the left; north up. Uploaded when another is given.
    pub milky_way: Option<Arc<Image>>,
    /// Radiance of one unit of `milky_way`, linear.
    pub milky_way_radiance: f32,
    /// From the sky's coordinates to the scene's.
    pub orientation: DMat3,
    /// Julian years since J2000, for proper motion.
    pub years: f64,
}

/// `Sky` in scene.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct SkyGpu {
    to_view: [[f32; 4]; 3],
    years: f32,
    exposure: f32,
    milky_way: f32,
    faintest: f32,
    viewer: f32,
    _pad: [f32; 3],
}

/// The least a pixel shows (after exposure) for a star to be drawn: a
/// tenth of an 8-bit step near black.
const FAINTEST_SHOWN: f32 = 3e-5;

pub(crate) struct SkyPass {
    layout: wgpu::BindGroupLayout,
    background: wgpu::RenderPipeline,
    stars_pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    stars: Storage,
    star_count: u32,
    sampler: wgpu::Sampler,
    /// Shown where there is no Milky Way: one black texel.
    black: wgpu::TextureView,
    uploaded_stars: Option<Arc<Vec<Star>>>,
    uploaded_image: Option<(Arc<Image>, wgpu::TextureView)>,
    group: Option<wgpu::BindGroup>,
}

impl SkyPass {
    pub(crate) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        shader: &wgpu::ShaderModule,
        scene_layout: &wgpu::BindGroupLayout,
    ) -> SkyPass {
        let visible = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: visible,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: visible,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: visible,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: visible,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky"),
            bind_group_layouts: &[Some(scene_layout), None, Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label, vs, fs, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                // Behind everything: drawn first, tested against nothing.
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend,
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
        let black = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("no milky way"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            black.as_image_copy(),
            &[0, 0, 0, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: None,
            },
            black.size(),
        );
        SkyPass {
            background: pipeline("milky way", "sky_vs", "sky_fs", None),
            stars_pipeline: pipeline("stars", "star_vs", "dot_fs", Some(additive)),
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sky"),
                size: std::mem::size_of::<SkyGpu>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            stars: Storage::new(device, "stars"),
            star_count: 0,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("sky"),
                address_mode_u: wgpu::AddressMode::Repeat,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
            black: black.create_view(&Default::default()),
            uploaded_stars: None,
            uploaded_image: None,
            group: None,
            layout,
        }
    }

    /// Uploads what changed and this frame's constants; `false` with no
    /// sky to draw.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        post: &crate::post::Post,
        scene: &crate::Scene,
    ) -> bool {
        let (Some(sky), Some(camera)) = (&scene.sky, &scene.camera) else {
            return false;
        };
        let look = &scene.look;
        let mut changed = self.group.is_none();
        if self
            .uploaded_stars
            .as_ref()
            .is_none_or(|s| !Arc::ptr_eq(s, &sky.stars))
        {
            changed |= self.stars.write(device, queue, &sky.stars);
            self.star_count = sky.stars.len() as u32;
            self.uploaded_stars = Some(sky.stars.clone());
        }
        let image_changed = match (&sky.milky_way, &self.uploaded_image) {
            (Some(a), Some((b, _))) => !Arc::ptr_eq(a, b),
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        };
        if image_changed {
            self.uploaded_image = sky.milky_way.as_ref().map(|image| {
                (
                    image.clone(),
                    post.upload_with_mips(device, queue, encoder, image),
                )
            });
            changed = true;
        }
        if changed {
            let image = self.uploaded_image.as_ref().map_or(&self.black, |(_, v)| v);
            self.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("sky"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.stars.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(image),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            }));
        }
        let column = |c: glam::DVec3| {
            let v = camera.rotate(sky.orientation * c);
            [v.x as f32, v.y as f32, v.z as f32, 0.0]
        };
        let uniform = SkyGpu {
            to_view: [
                column(glam::DVec3::X),
                column(glam::DVec3::Y),
                column(glam::DVec3::Z),
            ],
            years: sky.years as f32,
            exposure: look.exposure,
            milky_way: sky.milky_way_radiance * look.exposure,
            faintest: FAINTEST_SHOWN,
            viewer: look.pixel * look.pixel,
            _pad: [0.0; 3],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        true
    }

    /// Draws the Milky Way, then the stars, into the scene's pass.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass, scene_group: &wgpu::BindGroup) {
        let Some(group) = &self.group else {
            return;
        };
        pass.set_bind_group(0, scene_group, &[]);
        pass.set_bind_group(2, group, &[]);
        pass.set_pipeline(&self.background);
        pass.draw(0..3, 0..1);
        if self.star_count > 0 {
            pass.set_pipeline(&self.stars_pipeline);
            pass.draw(0..6, 0..self.star_count);
        }
    }
}
