//! A GPU device with no window, texture readback, and timing, for tests
//! and screenshots (M1c.md, step 2 plan, item 7; step 4 plan, item 7).

/// A windowless adapter and device, with the renderer's features where
/// the adapter has them.
pub struct Offscreen {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Offscreen {
    pub fn new() -> Result<Offscreen, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("no GPU adapter for offscreen rendering: {e}"))?;
        let (device, queue) =
            crate::request_device(&adapter, "offscreen").map_err(|e| format!("GPU device: {e}"))?;
        Ok(Offscreen {
            adapter,
            device,
            queue,
        })
    }

    /// `texture`'s pixels (single-sampled, 2D, `COPY_SRC`), row by row from
    /// the top, each its raw bytes.
    pub fn read(&self, texture: &wgpu::Texture) -> Vec<u8> {
        let (width, height) = (texture.width(), texture.height());
        let pixel = u64::from(
            texture
                .format()
                .block_copy_size(None)
                .expect("a color format"),
        );
        let row = pixel * u64::from(width);
        let padded = row.div_ceil(u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT))
            * u64::from(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: padded * u64::from(height),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("readback"),
            });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded as u32),
                    rows_per_image: Some(height),
                },
            },
            texture.size(),
        );
        self.queue.submit(Some(encoder.finish()));
        buffer.slice(..).map_async(wgpu::MapMode::Read, |r| {
            r.expect("readback mapped");
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("readback finished");
        let data = buffer
            .slice(..)
            .get_mapped_range()
            .expect("readback mapped");
        data.chunks_exact(padded as usize)
            .flat_map(|r| r[..row as usize].iter().copied())
            .collect()
    }

    /// An `Rgba16Float` texture's pixels as linear floats.
    pub fn read_rgba16f(&self, texture: &wgpu::Texture) -> Vec<[f32; 4]> {
        assert_eq!(texture.format(), wgpu::TextureFormat::Rgba16Float);
        self.read(texture)
            .chunks_exact(8)
            .map(|p| {
                std::array::from_fn(|i| f16_to_f32(u16::from_le_bytes([p[2 * i], p[2 * i + 1]])))
            })
            .collect()
    }

    /// The GPU time of drawing `scene` at `size` pixels, ms: the median of
    /// 30 frames after 5. `None` without timestamps.
    pub fn time_scene(
        &self,
        renderer: &mut crate::Renderer,
        scene: &crate::Scene,
        size: (u32, u32),
    ) -> Option<f32> {
        let mut timer = crate::GpuTimer::new(&self.device, &self.queue)?;
        let target = crate::Target::new(&self.device, size.0, size.1);
        let mut times = Vec::new();
        for frame in 0..35 {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            let m = timer.begin(&mut encoder);
            let writes = timer.scene_pass(m);
            renderer.render(
                &self.device,
                &self.queue,
                &mut encoder,
                &target,
                scene,
                writes,
            );
            timer.after_scene(&mut encoder, m);
            timer.end(&mut encoder, m);
            self.queue.submit(Some(encoder.finish()));
            timer.submitted(m);
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("the GPU finishes");
            let done = timer.collect(&self.device);
            if frame >= 5 {
                times.extend(done.into_iter().map(|t| t.scene));
            }
        }
        times.sort_by(f32::total_cmp);
        times.get(times.len() / 2).copied()
    }
}

/// IEEE half precision to single.
pub fn f16_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((h >> 10) & 0x1f);
    let mantissa = f32::from(h & 0x3ff);
    sign * match exponent {
        0 => mantissa * 2f32.powi(-24),
        31 if mantissa == 0.0 => f32::INFINITY,
        31 => f32::NAN,
        e => (1.0 + mantissa / 1024.0) * 2f32.powi(e - 15),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Renderer, Scene, Target};

    #[test]
    fn halves() {
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0xc000), -2.0);
        assert_eq!(f16_to_f32(0x0001), 2f32.powi(-24));
        assert_eq!(f16_to_f32(0x7bff), 65504.0);
    }

    /// The empty scene is its background, in linear HDR: values above
    /// the SDR white survive. And where the device has timestamps, the
    /// frame is timed.
    #[test]
    fn empty_scene_reads_back_its_background() {
        let gpu = Offscreen::new().expect("a GPU");
        let target = Target::new(&gpu.device, 64, 32);
        let mut renderer = Renderer::new(&gpu.device);
        let mut timer = crate::GpuTimer::new(&gpu.device, &gpu.queue);
        let scene = Scene {
            background: [0.25, 1.5, 7.0],
            ..Default::default()
        };
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let m = timer.as_mut().and_then(|t| t.begin(&mut encoder));
        let writes = timer.as_ref().and_then(|t| t.scene_pass(m));
        renderer.render(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            &target,
            &scene,
            writes,
        );
        if let Some(t) = &timer {
            t.after_scene(&mut encoder, m);
            t.end(&mut encoder, m);
        }
        gpu.queue.submit(Some(encoder.finish()));
        if let Some(t) = &timer {
            t.submitted(m);
        }
        let pixels = gpu.read_rgba16f(&target.color);
        assert_eq!(pixels.len(), 64 * 32);
        assert!(
            pixels.iter().all(|p| *p == [0.25, 1.5, 7.0, 1.0]),
            "{:?}",
            pixels[0]
        );
        match timer.as_mut() {
            Some(t) => {
                let got = t.collect(&gpu.device);
                assert_eq!(got.len(), 1);
                let got = got[0];
                assert_eq!(t.last(), Some(got));
                println!("{:?} on {}", got, gpu.adapter.get_info().name);
                assert!(got.scene >= 0.0 && got.frame.is_none_or(|f| f >= got.scene));
            }
            None => println!("no timestamps on {}", gpu.adapter.get_info().name),
        }
    }

    /// Frames that finish between two collections come back oldest
    /// first, and the latest is the newest, even after the ring wraps
    /// (found in review: slots were read in index order).
    #[test]
    fn timer_reads_frames_oldest_first() {
        let gpu = Offscreen::new().expect("a GPU");
        let Some(mut timer) = crate::GpuTimer::new(&gpu.device, &gpu.queue) else {
            println!("no timestamps on {}", gpu.adapter.get_info().name);
            return;
        };
        let mut renderer = Renderer::new(&gpu.device);
        let (small, large) = (
            Target::new(&gpu.device, 1, 1),
            Target::new(&gpu.device, 4096, 4096),
        );
        let mut frame = |timer: &mut crate::GpuTimer, target: &Target| {
            let mut encoder = gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            let m = timer.begin(&mut encoder);
            assert!(m.is_some(), "a free slot");
            let scene = Scene::default();
            let writes = timer.scene_pass(m);
            renderer.render(
                &gpu.device,
                &gpu.queue,
                &mut encoder,
                target,
                &scene,
                writes,
            );
            timer.after_scene(&mut encoder, m);
            timer.end(&mut encoder, m);
            gpu.queue.submit(Some(encoder.finish()));
            timer.submitted(m);
            gpu.device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
        };
        // Slot 0, collected; then slots 1 and 2 small, and slot 0 again,
        // large, before the next collection.
        frame(&mut timer, &small);
        assert_eq!(timer.collect(&gpu.device).len(), 1);
        frame(&mut timer, &small);
        frame(&mut timer, &small);
        frame(&mut timer, &large);
        let got = timer.collect(&gpu.device);
        println!("{got:?}");
        assert_eq!(got.len(), 3);
        assert!(
            got[2].scene > got[0].scene && got[2].scene > got[1].scene,
            "{got:?}"
        );
        assert_eq!(timer.last(), Some(got[2]));
    }
}
