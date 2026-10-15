//! From the scene's radiance to what the app shows (M1c.md, step 6 plan,
//! item 1; post.wgsl): bloom down and up a chain of half-size levels,
//! then exposure, the tonemap and the dither.

use crate::{COLOR_FORMAT, Target};
use bytemuck::{Pod, Zeroable};

/// `Post` in post.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PostGpu {
    exposure: f32,
    bloom: f32,
    headroom: f32,
    levels: f32,
}

/// Bloom's levels at most, each half the last: the widest blurs a
/// 1080p image over ~a third of its height.
pub const MAX_LEVELS: usize = 8;

/// Levels for a target `size` pixels: halving while the smaller side is
/// at least 8.
pub fn levels(size: (u32, u32)) -> usize {
    let mut n = 0;
    let mut side = size.0.min(size.1) / 2;
    while n < MAX_LEVELS && side >= 8 {
        n += 1;
        side /= 2;
    }
    n.max(1)
}

pub(crate) struct Post {
    chain_layout: wgpu::BindGroupLayout,
    tonemap_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    tonemap: wgpu::RenderPipeline,
    /// The target the groups below were made for, by its id.
    groups: Option<(u64, Groups)>,
}

struct Groups {
    /// Each downsample's source: the scene, then each level but the last.
    down: Vec<wgpu::BindGroup>,
    /// Each upsample's source: each level but the first, by level.
    up: Vec<wgpu::BindGroup>,
    tonemap: wgpu::BindGroup,
}

impl Post {
    pub(crate) fn new(device: &wgpu::Device) -> Post {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("post"),
            source: wgpu::ShaderSource::Wgsl(include_str!("post.wgsl").into()),
        });
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let chain_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom chain"),
            entries: &[texture(0), sampler],
        });
        let tonemap_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tonemap"),
            entries: &[
                texture(0),
                sampler,
                texture(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline = |label, layout: &wgpu::BindGroupLayout, fs, blend| {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("full_vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
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
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        Post {
            down: pipeline("bloom down", &chain_layout, "down_fs", None),
            up: pipeline("bloom up", &chain_layout, "up_fs", Some(add)),
            tonemap: pipeline("tonemap", &tonemap_layout, "tonemap_fs", None),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("post"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("post"),
                size: std::mem::size_of::<PostGpu>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            chain_layout,
            tonemap_layout,
            groups: None,
        }
    }

    /// Makes the bind groups for `target`, unless they were last made for
    /// it.
    fn make_groups(&mut self, device: &wgpu::Device, target: &Target) {
        if self.groups.as_ref().is_none_or(|(id, _)| *id != target.id) {
            let chain = |view: &wgpu::TextureView| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("bloom chain"),
                    layout: &self.chain_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                })
            };
            let bloom = &target.bloom;
            let groups = Groups {
                down: std::iter::once(&target.hdr_view)
                    .chain(&bloom[..bloom.len() - 1])
                    .map(chain)
                    .collect(),
                up: bloom.iter().skip(1).map(chain).collect(),
                tonemap: device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("tonemap"),
                    layout: &self.tonemap_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&target.hdr_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&bloom[0]),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: self.uniform.as_entire_binding(),
                        },
                    ],
                }),
            };
            self.groups = Some((target.id, groups));
        }
    }

    /// Records bloom and the tonemap: from `target`'s scene radiance into
    /// its color.
    pub(crate) fn record(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &Target,
        look: &crate::scene::Look,
    ) {
        let levels = target.bloom.len();
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&PostGpu {
                exposure: look.exposure,
                bloom: look.bloom,
                headroom: look.headroom.max(1.0),
                levels: levels as f32,
            }),
        );
        self.make_groups(device, target);
        let (down, up, tonemap) = (&self.down, &self.up, &self.tonemap);
        let groups = &self.groups.as_ref().expect("made above").1;
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    view: &wgpu::TextureView,
                    pipeline: &wgpu::RenderPipeline,
                    group: &wgpu::BindGroup,
                    load: wgpu::LoadOp<wgpu::Color>| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        };
        let clear = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        for (level, group) in groups.down.iter().enumerate() {
            pass(encoder, &target.bloom[level], down, group, clear);
        }
        for level in (1..levels).rev() {
            pass(
                encoder,
                &target.bloom[level - 1],
                up,
                &groups.up[level - 1],
                wgpu::LoadOp::Load,
            );
        }
        pass(encoder, &target.color_view, tonemap, &groups.tonemap, clear);
    }
}
