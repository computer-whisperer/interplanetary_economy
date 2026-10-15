//! From the scene's radiance, exposed, to what the app shows (M1c.md, step
//! 6 plan, item 1, and part 10; post.wgsl): the viewer's eye's glare
//! (`glare`) down and up a chain of half-size levels, then the tonemap
//! and the dither. And, on the same machinery, images' mipmaps.

use crate::{COLOR_FORMAT, Target};
use bytemuck::{Pod, Zeroable};

/// `Post` in post.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PostGpu {
    glare: f32,
    headroom: f32,
}

/// The glare's levels at most, each half the last.
pub const MAX_LEVELS: usize = 12;

/// Levels for a target `size` pixels: halving while the smaller side is
/// at least 2, so that the last spreads light across the image (the
/// eye's glare reaches past it).
pub fn levels(size: (u32, u32)) -> usize {
    let mut n = 0;
    let mut side = size.0.min(size.1) / 2;
    while n < MAX_LEVELS && side >= 2 {
        n += 1;
        side /= 2;
    }
    n.max(1)
}

pub(crate) struct Post {
    chain_layout: wgpu::BindGroupLayout,
    excess_layout: wgpu::BindGroupLayout,
    tonemap_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    excess: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    tonemap: wgpu::RenderPipeline,
    /// An sRGB image's next mip level from the last.
    /// By `Image::FORMATS`.
    blit: Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    /// The target the groups below were made for, by its id.
    groups: Option<(u64, Groups)>,
    /// The glare's levels' weights, for the eye, `Look::pixel`'s bits and
    /// the levels they were found for.
    weights: Option<((crate::glare::Eye, u32, usize), Vec<f64>)>,
}

struct Groups {
    /// The first downsample's source: the scene.
    excess: wgpu::BindGroup,
    /// Each later downsample's source: each level but the last.
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
        let uniform = wgpu::BindGroupLayoutEntry {
            binding: 3,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let chain_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glare chain"),
            entries: &[texture(0), sampler],
        });
        let excess_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glare excess"),
            entries: &[texture(0), sampler, uniform],
        });
        let tonemap_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tonemap"),
            entries: &[texture(0), sampler, texture(2), uniform],
        });
        let pipeline = |label, layout: &wgpu::BindGroupLayout, fs, format, blend| {
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
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // The level below's share by the blend constant, the rest kept.
        let weigh = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Constant,
                dst_factor: wgpu::BlendFactor::OneMinusConstant,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        Post {
            excess: pipeline(
                "glare excess",
                &excess_layout,
                "excess_fs",
                COLOR_FORMAT,
                None,
            ),
            down: pipeline("glare down", &chain_layout, "down_fs", COLOR_FORMAT, None),
            up: pipeline(
                "glare up",
                &chain_layout,
                "up_fs",
                COLOR_FORMAT,
                Some(weigh),
            ),
            tonemap: pipeline("tonemap", &tonemap_layout, "tonemap_fs", COLOR_FORMAT, None),
            blit: crate::maps::Image::FORMATS
                .iter()
                .map(|&f| (f, pipeline("mip", &chain_layout, "blit_fs", f, None)))
                .collect(),
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
            excess_layout,
            tonemap_layout,
            groups: None,
            weights: None,
        }
    }

    /// Makes the bind groups for `target`, unless they were last made for
    /// it.
    fn make_groups(&mut self, device: &wgpu::Device, target: &Target) {
        if self.groups.as_ref().is_none_or(|(id, _)| *id != target.id) {
            let chain = |view: &wgpu::TextureView| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("glare chain"),
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
            let glare = &target.glare;
            let groups = Groups {
                excess: device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("glare excess"),
                    layout: &self.excess_layout,
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
                            binding: 3,
                            resource: self.uniform.as_entire_binding(),
                        },
                    ],
                }),
                down: glare[..glare.len() - 1].iter().map(chain).collect(),
                up: glare.iter().skip(1).map(chain).collect(),
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
                            resource: wgpu::BindingResource::TextureView(&glare[0]),
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

    /// Records the glare and the tonemap: from `target`'s scene radiance
    /// into its color.
    pub(crate) fn record(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &Target,
        look: &crate::scene::Look,
    ) {
        let levels = target.glare.len();
        let weights = match look.eye {
            Some(eye) if look.pixel > 0.0 => {
                let key = (eye, look.pixel.to_bits(), levels);
                if self.weights.as_ref().is_none_or(|(k, _)| *k != key) {
                    let w = crate::glare::weights(&eye, f64::from(look.pixel), levels);
                    self.weights = Some((key, w));
                }
                self.weights.as_ref().expect("found above").1.clone()
            }
            _ => vec![0.0; levels],
        };
        // What each level and those past it carry.
        let below: Vec<f64> = (0..=levels).map(|k| weights[k..].iter().sum()).collect();
        queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&PostGpu {
                glare: below[0] as f32,
                headroom: look.headroom.max(1.0),
            }),
        );
        self.make_groups(device, target);
        let (down, up, tonemap) = (&self.down, &self.up, &self.tonemap);
        let groups = &self.groups.as_ref().expect("made above").1;
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    view: &wgpu::TextureView,
                    pipeline: &wgpu::RenderPipeline,
                    group: &wgpu::BindGroup,
                    load: wgpu::LoadOp<wgpu::Color>,
                    weight: f64| {
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
            pass.set_blend_constant(wgpu::Color {
                r: weight,
                g: weight,
                b: weight,
                a: weight,
            });
            pass.draw(0..3, 0..1);
        };
        let clear = wgpu::LoadOp::Clear(wgpu::Color::BLACK);
        if below[0] > 0.0 {
            pass(
                encoder,
                &target.glare[0],
                &self.excess,
                &groups.excess,
                clear,
                0.0,
            );
            for (level, group) in groups.down.iter().enumerate() {
                pass(encoder, &target.glare[level + 1], down, group, clear, 0.0);
            }
            // Each level becomes the weighted mean of itself and those past
            // it: the level past it holds theirs, weighing their share.
            for level in (1..levels).rev() {
                let share = if below[level - 1] > 0.0 {
                    below[level] / below[level - 1]
                } else {
                    0.0
                };
                pass(
                    encoder,
                    &target.glare[level - 1],
                    up,
                    &groups.up[level - 1],
                    wgpu::LoadOp::Load,
                    share,
                );
            }
        }
        pass(
            encoder,
            &target.color_view,
            tonemap,
            &groups.tonemap,
            clear,
            0.0,
        );
    }

    /// `image` on the GPU with its full chain of mip levels, each made from
    /// the last in `encoder`.
    pub(crate) fn upload_with_mips(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        image: &crate::maps::Image,
    ) -> wgpu::TextureView {
        let blit = &self
            .blit
            .iter()
            .find(|(f, _)| *f == image.format)
            .unwrap_or_else(|| panic!("no mips for {:?}", image.format))
            .1;
        let levels = 32 - image.width.max(image.height).max(1).leading_zeros();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size: wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: image.format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &image.data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.pixel() * image.width),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: image.width,
                height: image.height,
                depth_or_array_layers: 1,
            },
        );
        let level = |k: u32| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                base_mip_level: k,
                mip_level_count: Some(1),
                ..Default::default()
            })
        };
        for k in 1..levels {
            let source = level(k - 1);
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mip"),
                layout: &self.chain_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            let target = level(k);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mip"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(blit);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        texture.create_view(&Default::default())
    }
}
