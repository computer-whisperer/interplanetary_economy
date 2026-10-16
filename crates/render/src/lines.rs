//! Sets of paths on the GPU (M1c.md, step 4 plan, item 1; step 6 plan,
//! item 3; step 8): each set uploaded when the scene holds another, apart
//! from the others (a ship's paths are large and change when its design
//! does, the bodies' orbits small and often), and culled and cut each
//! frame.

use crate::Storage;
use crate::paths::{self, Paths, Pose};
use std::sync::Arc;

pub(crate) struct PathSet {
    vertices: Storage,
    infos: Storage,
    runs: Storage,
    splits: Storage,
    pieces: Storage,
    group: Option<wgpu::BindGroup>,
    /// The paths uploaded, and their chunks.
    uploaded: Option<(Arc<Paths>, paths::Prepared)>,
    /// This frame's.
    visible: paths::Visible,
}

impl PathSet {
    pub(crate) fn new(device: &wgpu::Device) -> PathSet {
        PathSet {
            vertices: Storage::new(device, "path vertices"),
            infos: Storage::new(device, "paths"),
            runs: Storage::new(device, "path runs"),
            splits: Storage::new(device, "path splits"),
            pieces: Storage::new(device, "path pieces"),
            group: None,
            uploaded: None,
            visible: paths::Visible::default(),
        }
    }

    /// Uploads `set` if it is not the set uploaded, and this frame's
    /// visible runs, splits and pieces.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        set: &Arc<Paths>,
        camera: &crate::Camera,
        poses: &[Pose],
        size: (u32, u32),
        now: f64,
    ) {
        let mut grew = false;
        if self
            .uploaded
            .as_ref()
            .is_none_or(|(p, _)| !Arc::ptr_eq(p, set))
        {
            let prepared = paths::Prepared::new(set);
            grew |= self.vertices.write(device, queue, &prepared.vertices);
            grew |= self.infos.write(device, queue, &prepared.paths);
            self.uploaded = Some((set.clone(), prepared));
        }
        let (paths, prepared) = self.uploaded.as_ref().expect("uploaded above");
        self.visible = prepared.visible(paths, camera, poses, size, now);
        grew |= self.runs.write(device, queue, &self.visible.instances);
        grew |= self.splits.write(device, queue, &self.visible.splits);
        grew |= self.pieces.write(device, queue, &self.visible.pieces);
        if grew || self.group.is_none() {
            let entries: Vec<wgpu::BindGroupEntry> = [
                &self.vertices,
                &self.infos,
                &self.runs,
                &self.splits,
                &self.pieces,
            ]
            .into_iter()
            .enumerate()
            .map(|(binding, s)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: s.buffer.as_entire_binding(),
            })
            .collect();
            self.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("paths"),
                layout,
                entries: &entries,
            }));
        }
    }

    /// Draws this frame's lines: runs cut on the GPU, then pieces placed
    /// by the CPU. Group 0 is bound.
    pub(crate) fn draw(
        &self,
        pass: &mut wgpu::RenderPass,
        lines: &wgpu::RenderPipeline,
        pieces: &wgpu::RenderPipeline,
    ) {
        let (visible, Some(group)) = (&self.visible, &self.group) else {
            return;
        };
        if visible.draws.is_empty() && visible.pieces.is_empty() {
            return;
        }
        pass.set_bind_group(1, group, &[]);
        pass.set_pipeline(lines);
        for (segments, cut, instances) in &visible.draws {
            pass.draw(0..6 * segments * cut, instances.clone());
        }
        if !visible.pieces.is_empty() {
            pass.set_pipeline(pieces);
            pass.draw(0..6, 0..visible.pieces.len() as u32);
        }
    }
}
