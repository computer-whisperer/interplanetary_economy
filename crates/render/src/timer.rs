//! GPU time per frame from timestamp queries (M1c.md, step 2 plan, item
//! 6): at the frame's start, after the scene and after the UI where the
//! device can write timestamps between passes, otherwise around the
//! scene's pass alone. Read back through a ring of buffers, so measuring
//! never waits for the GPU; a frame whose slot is still in flight goes
//! unmeasured.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Frames in flight the ring measures.
const SLOTS: usize = 3;

/// Timestamps per frame: start, after the scene, end.
const STAMPS: u32 = 3;

/// `resolve_query_set` writes at offsets that are multiples of this.
const RESOLVE_ALIGN: u64 = wgpu::QUERY_RESOLVE_BUFFER_ALIGNMENT;

const IDLE: u8 = 0;
const PENDING: u8 = 1;
const READY: u8 = 2;
const FAILED: u8 = 3;

/// One frame's GPU times, ms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Timings {
    pub scene: f32,
    /// The whole frame, where the device timestamps between passes.
    pub frame: Option<f32>,
}

struct Slot {
    read: wgpu::Buffer,
    state: Arc<AtomicU8>,
}

pub struct GpuTimer {
    set: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    slots: Vec<Slot>,
    /// ns per tick.
    period: f32,
    between_passes: bool,
    next: usize,
    last: Option<Timings>,
}

/// A frame being measured: its slot.
#[derive(Clone, Copy, Debug)]
pub struct Measuring {
    slot: usize,
}

impl GpuTimer {
    /// `None` without timestamp queries on `device`.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<GpuTimer> {
        let features = device.features();
        if !features.contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let between_passes = features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS);
        let bytes = u64::from(STAMPS) * 8;
        use wgpu::BufferUsages as U;
        Some(GpuTimer {
            set: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("frame timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: STAMPS * SLOTS as u32,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame timestamps"),
                size: RESOLVE_ALIGN * SLOTS as u64,
                usage: U::QUERY_RESOLVE | U::COPY_SRC,
                mapped_at_creation: false,
            }),
            slots: (0..SLOTS)
                .map(|_| Slot {
                    read: device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("frame timestamps read"),
                        size: bytes,
                        usage: U::MAP_READ | U::COPY_DST,
                        mapped_at_creation: false,
                    }),
                    state: Arc::new(AtomicU8::new(IDLE)),
                })
                .collect(),
            period: queue.get_timestamp_period(),
            between_passes,
            next: 0,
            last: None,
        })
    }

    /// The latest frame read back.
    pub fn last(&self) -> Option<Timings> {
        self.last
    }

    /// Collects frames the GPU has finished, oldest first (call once a
    /// frame, before [`Self::begin`]), and returns their times.
    pub fn collect(&mut self, device: &wgpu::Device) -> Vec<Timings> {
        let _ = device.poll(wgpu::PollType::Poll);
        let mut out = Vec::new();
        // `next` is the slot begun longest ago.
        for k in 0..SLOTS {
            let slot = &self.slots[(self.next + k) % SLOTS];
            match slot.state.load(Ordering::Acquire) {
                READY => {
                    let ticks: Vec<u64> = {
                        let data = slot.read.slice(..).get_mapped_range().expect("mapped");
                        data.chunks_exact(8)
                            .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
                            .collect()
                    };
                    slot.read.unmap();
                    slot.state.store(IDLE, Ordering::Release);
                    let ms = |a: u64, b: u64| b.wrapping_sub(a) as f32 * self.period / 1e6;
                    out.push(Timings {
                        scene: ms(ticks[0], ticks[1]),
                        frame: self.between_passes.then(|| ms(ticks[0], ticks[2])),
                    });
                }
                FAILED => slot.state.store(IDLE, Ordering::Release),
                _ => {}
            }
        }
        if let Some(&t) = out.last() {
            self.last = Some(t);
        }
        out
    }

    /// Starts measuring a frame in `encoder`, if a slot is free. The frame
    /// must then record a scene pass: without timestamps between passes,
    /// its pass writes the first two.
    pub fn begin(&mut self, encoder: &mut wgpu::CommandEncoder) -> Option<Measuring> {
        let slot = self.next;
        if self.slots[slot].state.load(Ordering::Acquire) != IDLE {
            return None;
        }
        self.next = (self.next + 1) % SLOTS;
        if self.between_passes {
            encoder.write_timestamp(&self.set, self.query(slot, 0));
        }
        Some(Measuring { slot })
    }

    /// The scene pass's timestamps: none where they are written between
    /// passes instead.
    pub fn scene_pass(&self, m: Option<Measuring>) -> Option<wgpu::RenderPassTimestampWrites<'_>> {
        let m = m.filter(|_| !self.between_passes)?;
        Some(wgpu::RenderPassTimestampWrites {
            query_set: &self.set,
            beginning_of_pass_write_index: Some(self.query(m.slot, 0)),
            end_of_pass_write_index: Some(self.query(m.slot, 1)),
        })
    }

    /// Marks the end of the scene.
    pub fn after_scene(&self, encoder: &mut wgpu::CommandEncoder, m: Option<Measuring>) {
        if let Some(m) = m.filter(|_| self.between_passes) {
            encoder.write_timestamp(&self.set, self.query(m.slot, 1));
        }
    }

    /// Marks the end of the frame and records the readback.
    pub fn end(&self, encoder: &mut wgpu::CommandEncoder, m: Option<Measuring>) {
        let Some(m) = m else { return };
        let used = if self.between_passes {
            encoder.write_timestamp(&self.set, self.query(m.slot, 2));
            STAMPS
        } else {
            2
        };
        let first = self.query(m.slot, 0);
        let at = RESOLVE_ALIGN * m.slot as u64;
        encoder.resolve_query_set(&self.set, first..first + used, &self.resolve, at);
        encoder.copy_buffer_to_buffer(
            &self.resolve,
            at,
            &self.slots[m.slot].read,
            0,
            u64::from(used) * 8,
        );
    }

    /// After the frame's submission: starts reading it back.
    pub fn submitted(&self, m: Option<Measuring>) {
        let Some(m) = m else { return };
        let state = self.slots[m.slot].state.clone();
        state.store(PENDING, Ordering::Release);
        self.slots[m.slot]
            .read
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                state.store(
                    if result.is_ok() { READY } else { FAILED },
                    Ordering::Release,
                );
            });
    }

    fn query(&self, slot: usize, stamp: u32) -> u32 {
        slot as u32 * STAMPS + stamp
    }
}
