//! Paths drawn as lines (M1c.md, step 4 plan, item 1): samples in some
//! frame's coordinates, placed in the world by that frame's pose at the
//! moment drawn.
//!
//! Positions reach the GPU as two f32s per coordinate, high and low, and
//! so does the eye in the frame's coordinates: the vertex shader subtracts
//! the eye first, so a sample's error is ~6e-8 of its distance from the
//! eye, as for bodies. Between samples the line follows the cubic Hermite
//! curve through their positions and velocities ([`Path::at`]).
//!
//! Paths are cut into chunks of [`CHUNK_SEGMENTS`] segments, the units of
//! culling: each frame the CPU culls them in f64. Where a chunk's straight
//! segments would bow visibly from the curve, the GPU cuts each into
//! pieces along it, a power of two up to [`MAX_PIECES`]; where a chunk
//! needs many, its parts of [`PART_SEGMENTS`] are culled and cut each as
//! it needs. Where that is not enough, or f32 is not (the curve's terms
//! are as large as its segment, 650,000 km in cruise), the CPU cuts the
//! part's segments and places the pieces relative to the eye in f64.
//!
//! Each frame the CPU also finds where each path is at the scene's moment
//! (a segment and a fraction along it), which splits its past color from
//! its future one; and, for a path that leaves a gap there (a body's
//! orbit, so that the body shows through it), where that is on the
//! screen.

use crate::camera::Camera;
use crate::scene::Frustum;
use bytemuck::{Pod, Zeroable};
use glam::{DMat3, DVec2, DVec3};
use std::ops::Range;

/// Segments per chunk, and per part of one.
pub const CHUNK_SEGMENTS: usize = 64;
pub const PART_SEGMENTS: usize = 8;

/// The most pieces the GPU cuts a segment into.
pub const MAX_PIECES: u32 = 32;

/// A chunk whose segments need this many pieces or more is cut part by
/// part; under it, all its segments are cut alike.
const PARTS_FROM: u32 = 8;

/// Segments are cut until a piece's chord bows from the curve by at most
/// this, pixels; and are cut on the CPU where f32 would err by more.
const MAX_BOW_PX: f64 = 0.25;

/// What the GPU's f32 Hermite errs by, relative to its terms' size (a few
/// units in the last place).
const F32_ERROR: f64 = 2.4e-7;

/// The most poses a scene places paths with.
pub const MAX_POSES: usize = 8;

/// Where a frame's coordinates are in the world at one moment: a point `p`
/// is at `origin + axes · (scale · p)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub origin: DVec3,
    /// Orthonormal columns: the frame's x, y and z in the world.
    pub axes: DMat3,
    pub scale: f64,
}

impl Default for Pose {
    fn default() -> Pose {
        Pose {
            origin: DVec3::ZERO,
            axes: DMat3::IDENTITY,
            scale: 1.0,
        }
    }
}

impl Pose {
    pub fn to_world(&self, p: DVec3) -> DVec3 {
        self.origin + self.axes * (p * self.scale)
    }

    pub fn from_world(&self, w: DVec3) -> DVec3 {
        self.axes.transpose() * (w - self.origin) / self.scale
    }
}

/// A moment of a path: s on the scene's time axis, and the position and
/// velocity (per s) in the path's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub t: f64,
    pub position: DVec3,
    pub velocity: DVec3,
}

/// A line through samples, in order of time. Two samples at one time (a
/// burn, or two motions meeting) are not joined.
#[derive(Clone, Debug)]
pub struct Path {
    /// The scene's pose (`Scene::poses`) that places it.
    pub pose: usize,
    pub samples: Vec<Sample>,
    /// Linear colors after the scene's moment and before it.
    pub future: [f32; 3],
    pub past: [f32; 3],
    /// The radius of the gap left where the path is at the scene's moment,
    /// in line widths; 0 for none.
    pub gap: f32,
}

/// The cubic Hermite curve at `u` between samples `a` and `b`, relative to
/// `origin`.
fn hermite(a: &Sample, b: &Sample, u: f64, origin: DVec3) -> DVec3 {
    let h = b.t - a.t;
    let (u2, u3) = (u * u, u * u * u);
    (2.0 * u3 - 3.0 * u2 + 1.0) * (a.position - origin)
        + (-2.0 * u3 + 3.0 * u2) * (b.position - origin)
        + h * ((u3 - 2.0 * u2 + u) * a.velocity + (u3 - u2) * b.velocity)
}

impl Path {
    /// The position on the line at `t` (s on the scene's time axis), in
    /// the path's frame: the cubic Hermite curve the line is drawn along,
    /// in f64. `None` outside the samples' span; where two samples share a
    /// time, the later's.
    pub fn at(&self, t: f64) -> Option<DVec3> {
        let s = &self.samples;
        if s.is_empty() || t < s[0].t || t > s[s.len() - 1].t {
            return None;
        }
        let b = s.partition_point(|x| x.t <= t);
        if b == s.len() {
            return Some(s[b - 1].position);
        }
        let (a, b) = (&s[b - 1], &s[b]);
        Some(hermite(a, b, (t - a.t) / (b.t - a.t), DVec3::ZERO))
    }

    /// How near the line comes to `at` (pixels from the top-left corner,
    /// y down) on a viewport of `size` pixels seen by `camera`, the path
    /// placed by `pose`: along the Hermite curve it is drawn along, in f64,
    /// within a quarter pixel, counting only parts whose middle (in the
    /// world) is `shown`. `None` where it comes no nearer than `within`
    /// pixels, or is behind the eye.
    ///
    /// Each segment is halved until its halves are straight to a quarter
    /// pixel on the screen, or certainly too far from `at`, or behind the
    /// eye. Over part of a segment the curve is again a cubic Hermite, so
    /// [`bow`] bounds it in the world; a point moved by δ at view depth z,
    /// |(x, y)| / z off the view's axis, moves δ (1 + |(x, y)| / z) / z on
    /// the screen, and the curve may come nearer the eye than the chord by
    /// its bow (along the chord, |(x, y)| / z is largest at an end).
    pub fn near(
        &self,
        pose: &Pose,
        camera: &Camera,
        size: (u32, u32),
        at: DVec2,
        within: f64,
        shown: impl Fn(DVec3) -> bool,
    ) -> Option<f64> {
        let px_per_radian = 1.0 / camera.pixel_angle(size.1);
        let mut best = f64::INFINITY;
        let mut parts: Vec<(f64, f64)> = Vec::new();
        for i in 1..self.samples.len() {
            let (a, b) = (&self.samples[i - 1], &self.samples[i]);
            if b.t <= a.t {
                continue;
            }
            // The part from u0 to u1 as a Hermite segment of its own, in
            // the world: its ends, and its tangents over its span.
            let part = |u: f64, span: f64| {
                let p = pose.to_world(hermite(a, b, u, DVec3::ZERO));
                (p, pose.axes * hermite_du(a, b, u) * (pose.scale * span))
            };
            parts.push((0.0, 1.0));
            while let Some((u0, u1)) = parts.pop() {
                let ((p0, t0), (p1, t1)) = (part(u0, u1 - u0), part(u1, u1 - u0));
                let chord = p1 - p0;
                let bow_km = 4.0 / 27.0 * ((t0 - chord).length() + (t1 - chord).length());
                let (v0, v1) = (camera.to_view(p0), camera.to_view(p1));
                let nearest = (-v0.z).min(-v1.z) - bow_km;
                let split = u1 - u0 > 1.0 / 65_536.0;
                if nearest <= 0.0 {
                    // Behind the eye, or reaching past it.
                    if v0.z.min(v1.z) <= bow_km && split {
                        parts.extend([(u0, 0.5 * (u0 + u1)), (0.5 * (u0 + u1), u1)]);
                    }
                    continue;
                }
                let (Some(q0), Some(q1)) =
                    (camera.project_view(v0, size), camera.project_view(v1, size))
                else {
                    continue;
                };
                let off_axis = [v0, v1]
                    .map(|v| v.truncate().length() / -v.z)
                    .into_iter()
                    .fold(0.0, f64::max);
                let stray_px = bow_km * (1.0 + off_axis) / nearest * px_per_radian;
                let d = segment_distance(at, q0, q1);
                if d - stray_px > within.min(best) {
                    continue;
                }
                if stray_px > 0.25 && split {
                    parts.extend([(u0, 0.5 * (u0 + u1)), (0.5 * (u0 + u1), u1)]);
                } else if d < best && shown(part(0.5 * (u0 + u1), 0.0).0) {
                    best = d;
                }
            }
        }
        (best <= within).then_some(best)
    }

    /// Where the path is at `t`: the segment (by its first sample) and the
    /// fraction along it. Before the path, its first segment's start;
    /// after it, its last sample.
    fn split(&self, t: f64) -> (usize, f64) {
        let b = self.samples.partition_point(|x| x.t <= t);
        if b == 0 {
            return (0, 0.0);
        }
        if b == self.samples.len() {
            return (b - 1, 0.0);
        }
        let (a, c) = (&self.samples[b - 1], &self.samples[b]);
        (b - 1, (t - a.t) / (c.t - a.t))
    }
}

/// The paths of a scene, uploaded together: the renderer uploads them again
/// when the scene holds another `Arc`.
#[derive(Clone, Debug, Default)]
pub struct Paths {
    pub paths: Vec<Path>,
}

/// `PathVertex` in scene.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct VertexGpu {
    pub hi: [f32; 3],
    /// s to the next sample; 0 where the two are not joined.
    pub h: f32,
    pub lo: [f32; 3],
    pub path: u32,
    pub velocity: [f32; 3],
    pub _pad: u32,
}

/// `PathInfo` in scene.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct PathGpu {
    pub pose: u32,
    /// Line widths.
    pub gap: f32,
    pub _pad: [u32; 2],
    pub future: [f32; 4],
    pub past: [f32; 4],
}

/// `Piece` in scene.wgsl: a piece of a segment placed by the CPU, in view
/// space, and the fractions of the segment at its ends.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct PieceGpu {
    pub a: [f32; 3],
    /// The segment's first vertex.
    pub segment: u32,
    pub b: [f32; 3],
    pub path: u32,
    pub fractions: [f32; 2],
    pub _pad: [u32; 2],
}

/// Consecutive segments of a path, kept on the CPU for culling and detail:
/// their bounds in their frame's coordinates.
#[derive(Clone, Copy, Debug)]
struct Run {
    /// The first vertex, and the first sample in its path.
    first: u32,
    local: u32,
    segments: u32,
    center: DVec3,
    /// Enclosing the curve, not only the samples.
    radius: f64,
    /// The most a segment's curve strays from its chord.
    bow: f64,
    /// The largest of a segment's Hermite terms: its chord, and its
    /// velocities times its duration.
    terms: f64,
}

#[derive(Clone, Debug)]
struct Chunk {
    run: Run,
    path: usize,
    /// Its parts, in `Prepared::parts`.
    parts: Range<usize>,
}

/// Paths ready to upload: vertices and per-path records for the GPU, and the
/// chunks for culling.
#[derive(Clone, Debug, Default)]
pub struct Prepared {
    pub vertices: Vec<VertexGpu>,
    pub paths: Vec<PathGpu>,
    /// Each path's first vertex.
    bases: Vec<u32>,
    chunks: Vec<Chunk>,
    parts: Vec<Run>,
}

/// What to draw of the paths in a frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Visible {
    /// An instance per run of segments the GPU cuts:
    /// `[first vertex, segments | pieces << 16]`.
    pub instances: Vec<[u32; 2]>,
    /// The draws over them: the most segments an instance holds, the
    /// pieces per segment, and its instances.
    pub draws: Vec<(u32, u32, Range<u32>)>,
    /// Pieces the CPU placed.
    pub pieces: Vec<PieceGpu>,
    /// Per path, where it is at the scene's moment: the first vertex of
    /// the segment it is on, and the fraction along it (bits of an f32);
    /// segments before are past, after are future. Then, for a path with
    /// a gap, where that point is on the screen (pixels, bits of f32s;
    /// far off it for none).
    pub splits: Vec<[u32; 4]>,
}

/// A gap's center where there is none: pixels, far off the screen.
const NO_GAP: f32 = -1e9;

/// High and low f32 parts of `x`.
pub fn split(x: f64) -> [f32; 2] {
    let hi = x as f32;
    [hi, (x - f64::from(hi)) as f32]
}

fn split3(v: DVec3) -> ([f32; 3], [f32; 3]) {
    let [x, y, z] = [v.x, v.y, v.z].map(split);
    ([x[0], y[0], z[0]], [x[1], y[1], z[1]])
}

fn f32x3(v: DVec3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

/// The cubic Hermite curve's derivative in `u` between `a` and `b`.
fn hermite_du(a: &Sample, b: &Sample, u: f64) -> DVec3 {
    let h = b.t - a.t;
    let u2 = u * u;
    (6.0 * u2 - 6.0 * u) * (a.position - b.position)
        + h * ((3.0 * u2 - 4.0 * u + 1.0) * a.velocity + (3.0 * u2 - 2.0 * u) * b.velocity)
}

/// How far the Hermite curve between `a` and `b` strays from its chord:
/// it is the chord plus h₁₀(u)·(h·v₀ − Δp) + h₁₁(u)·(h·v₁ − Δp), and
/// |h₁₀|, |h₁₁| ≤ 4/27.
fn bow(a: &Sample, b: &Sample) -> f64 {
    let (h, chord) = (b.t - a.t, b.position - a.position);
    4.0 / 27.0 * ((h * a.velocity - chord).length() + (h * b.velocity - chord).length())
}

/// The distance from `p` to the segment from `a` to `b`.
fn segment_distance(p: DVec2, a: DVec2, b: DVec2) -> f64 {
    let ab = b - a;
    let u = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * u - p).length()
}

/// The largest of the Hermite's terms between `a` and `b`.
fn terms(a: &Sample, b: &Sample) -> f64 {
    let h = b.t - a.t;
    (b.position - a.position)
        .length()
        .max(h * a.velocity.length())
        .max(h * b.velocity.length())
}

/// The bounds of samples `s` (their segments), the first `local` in its
/// path, at vertex `first`.
fn run(s: &[Sample], first: usize, local: usize) -> Run {
    let (lo, hi) = s.iter().fold(
        (DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)),
        |(lo, hi), p| (lo.min(p.position), hi.max(p.position)),
    );
    let center = (lo + hi) / 2.0;
    let joined = || s.windows(2).filter(|w| w[1].t > w[0].t);
    let bow = joined().map(|w| bow(&w[0], &w[1])).fold(0.0, f64::max);
    let terms = joined().map(|w| terms(&w[0], &w[1])).fold(0.0, f64::max);
    let reach = s
        .iter()
        .map(|p| (p.position - center).length())
        .fold(0.0, f64::max);
    Run {
        first: first as u32,
        local: local as u32,
        segments: (s.len() - 1) as u32,
        center,
        radius: reach + bow,
        bow,
        terms,
    }
}

/// How a run is drawn from the eye.
enum Cut {
    Hidden,
    /// By the GPU, its segments in this many pieces.
    Gpu(u32),
    /// By the CPU.
    Cpu,
}

impl Prepared {
    pub fn new(paths: &Paths) -> Prepared {
        let mut out = Prepared::default();
        for (index, path) in paths.paths.iter().enumerate() {
            assert!(path.pose < MAX_POSES, "a path's pose is one of the scene's");
            out.paths.push(PathGpu {
                pose: path.pose as u32,
                gap: path.gap,
                _pad: [0; 2],
                future: [path.future[0], path.future[1], path.future[2], 1.0],
                past: [path.past[0], path.past[1], path.past[2], 1.0],
            });
            let base = out.vertices.len();
            out.bases.push(base as u32);
            let s = &path.samples;
            for (k, sample) in s.iter().enumerate() {
                let (hi, lo) = split3(sample.position);
                let h = s.get(k + 1).map_or(0.0, |n| (n.t - sample.t).max(0.0));
                out.vertices.push(VertexGpu {
                    hi,
                    h: h as f32,
                    lo,
                    path: index as u32,
                    velocity: f32x3(sample.velocity),
                    _pad: 0,
                });
            }
            for start in (0..s.len().saturating_sub(1)).step_by(CHUNK_SEGMENTS) {
                let end = (start + CHUNK_SEGMENTS).min(s.len() - 1);
                let first_part = out.parts.len();
                for from in (start..end).step_by(PART_SEGMENTS) {
                    let to = (from + PART_SEGMENTS).min(end);
                    out.parts.push(run(&s[from..=to], base + from, from));
                }
                out.chunks.push(Chunk {
                    run: run(&s[start..=end], base + start, start),
                    path: index,
                    parts: first_part..out.parts.len(),
                });
            }
        }
        out
    }

    /// What to draw of `paths` (those this was prepared from) from
    /// `camera` on a viewport of `size` pixels, at `now` on the paths'
    /// time axis: the chunks in view, their segments cut until each piece
    /// is within [`MAX_BOW_PX`] of the curve; from [`PARTS_FROM`] pieces,
    /// their parts in view instead, each cut as it needs; and where the
    /// GPU would need more than [`MAX_PIECES`], or err in f32, the part's
    /// segments cut and placed by the CPU.
    pub fn visible(
        &self,
        paths: &Paths,
        camera: &Camera,
        poses: &[Pose],
        size: (u32, u32),
        now: f64,
    ) -> Visible {
        let frustum = Frustum::new(camera, size);
        let pixel = camera.pixel_angle(size.1.max(1));
        // How a run is drawn, given the nearest its curve comes to the eye.
        let by_distance = |r: &Run, pose: &Pose, near: f64| {
            if near <= 0.0 || F32_ERROR * r.terms * pose.scale / near / pixel > MAX_BOW_PX {
                return Cut::Cpu;
            }
            match pieces(r.bow * pose.scale / near / pixel) {
                n if n > MAX_PIECES => Cut::Cpu,
                n => Cut::Gpu(n),
            }
        };
        // A run in view, and its distance from the eye by its bounds; for
        // a part around the eye, by its segments' (`path`'s).
        let cut = |r: &Run, pose: &Pose, path: Option<&Path>| {
            let center = camera.to_view(pose.to_world(r.center));
            let radius = r.radius * pose.scale;
            if !frustum.sees(center, radius) {
                return Cut::Hidden;
            }
            let mut near = center.length() - radius;
            if near <= 0.0
                && let Some(path) = path
            {
                let eye = pose.from_world(camera.eye);
                let s = &path.samples[r.local as usize..=(r.local + r.segments) as usize];
                near = s
                    .windows(2)
                    .filter(|w| w[1].t > w[0].t)
                    .map(|w| nearest(&w[0], &w[1], eye))
                    .fold(f64::INFINITY, f64::min)
                    * pose.scale;
            }
            by_distance(r, pose, near)
        };
        let level = |n: u32| n.trailing_zeros() as usize;
        let levels = level(MAX_PIECES) + 1;
        let (mut chunks, mut parts) = (vec![Vec::new(); levels], vec![Vec::new(); levels]);
        let mut out = Visible::default();
        for c in &self.chunks {
            let path = &paths.paths[c.path];
            let Some(pose) = poses.get(path.pose) else {
                continue;
            };
            match cut(&c.run, pose, None) {
                Cut::Hidden => {}
                Cut::Gpu(n) if n < PARTS_FROM => {
                    chunks[level(n)].push([c.run.first, c.run.segments | n << 16]);
                }
                _ => {
                    for p in &self.parts[c.parts.clone()] {
                        match cut(p, pose, Some(path)) {
                            Cut::Hidden => {}
                            Cut::Gpu(n) => parts[level(n)].push([p.first, p.segments | n << 16]),
                            Cut::Cpu => {
                                place(&mut out.pieces, path, c.path, p, pose, camera, pixel)
                            }
                        }
                    }
                }
            }
        }
        let batch = |out: &mut Visible, list: Vec<[u32; 2]>, segments: usize, pieces: u32| {
            if !list.is_empty() {
                let start = out.instances.len() as u32;
                out.instances.extend(list);
                out.draws
                    .push((segments as u32, pieces, start..out.instances.len() as u32));
            }
        };
        for (level, list) in chunks.into_iter().enumerate() {
            batch(&mut out, list, CHUNK_SEGMENTS, 1 << level);
        }
        for (level, list) in parts.into_iter().enumerate() {
            batch(&mut out, list, PART_SEGMENTS, 1 << level);
        }
        out.splits = paths
            .paths
            .iter()
            .zip(&self.bases)
            .map(|(path, &base)| {
                let (k, u) = path.split(now);
                let gap = (path.gap > 0.0)
                    .then(|| path.at(now))
                    .flatten()
                    .and_then(|p| camera.project(poses[path.pose].to_world(p), size))
                    .map_or([NO_GAP; 2], |g| [g.x as f32, g.y as f32]);
                [
                    base + k as u32,
                    (u as f32).to_bits(),
                    gap[0].to_bits(),
                    gap[1].to_bits(),
                ]
            })
            .collect();
        out
    }
}

/// Part `p` of `path` (the `index`th), its segments cut where each needs
/// it from the eye and placed in view space in f64: a piece is halved
/// until it is within [`MAX_BOW_PX`] of the curve seen from the eye, at
/// most [`MAX_HALVINGS`] times.
fn place(
    out: &mut Vec<PieceGpu>,
    path: &Path,
    index: usize,
    p: &Run,
    pose: &Pose,
    camera: &Camera,
    pixel: f64,
) {
    let eye = pose.from_world(camera.eye);
    for k in 0..p.segments as usize {
        let local = p.local as usize + k;
        let (a, b) = (&path.samples[local], &path.samples[local + 1]);
        if b.t <= a.t {
            continue;
        }
        let piece = Piece {
            a,
            b,
            eye,
            pose,
            camera,
            pixel,
            segment: p.first + k as u32,
            path: index as u32,
        };
        let (start, end) = (piece.at(0.0), piece.at(1.0));
        piece.halve(out, (0.0, start), (1.0, end), 0);
    }
}

/// Pieces are halved at most this many times (1,024 pieces).
const MAX_HALVINGS: u32 = 10;

/// A segment the CPU cuts, from `a` to `b`, seen from `eye` (in the path's
/// frame).
struct Piece<'a> {
    a: &'a Sample,
    b: &'a Sample,
    eye: DVec3,
    pose: &'a Pose,
    camera: &'a Camera,
    pixel: f64,
    segment: u32,
    path: u32,
}

impl Piece<'_> {
    /// The curve at fraction `u`, relative to the eye: position, and
    /// velocity per unit of `u`.
    fn at(&self, u: f64) -> (DVec3, DVec3) {
        let (a, b, h) = (self.a, self.b, self.b.t - self.a.t);
        let u2 = u * u;
        let position = hermite(a, b, u, self.eye);
        let rate = (6.0 * u2 - 6.0 * u) * (a.position - b.position)
            + h * ((3.0 * u2 - 4.0 * u + 1.0) * a.velocity + (3.0 * u2 - 2.0 * u) * b.velocity);
        (position, rate)
    }

    /// The piece between fractions `from` and `to` (with the curve's
    /// states there), placed if it is within [`MAX_BOW_PX`] of the curve
    /// from the eye, else halved.
    fn halve(
        &self,
        out: &mut Vec<PieceGpu>,
        from: (f64, (DVec3, DVec3)),
        to: (f64, (DVec3, DVec3)),
        depth: u32,
    ) {
        let du = to.0 - from.0;
        // The piece as a segment of its own: per unit of its fraction.
        let (sa, sb) = (
            Sample {
                t: 0.0,
                position: from.1.0,
                velocity: from.1.1 * du,
            },
            Sample {
                t: 1.0,
                position: to.1.0,
                velocity: to.1.1 * du,
            },
        );
        let scale = self.pose.scale;
        let near = nearest(&sa, &sb, DVec3::ZERO) * scale;
        if depth >= MAX_HALVINGS
            || (near > 0.0 && bow(&sa, &sb) * scale / near / self.pixel <= MAX_BOW_PX)
        {
            let to_view = |v: DVec3| f32x3(self.camera.rotate(self.pose.axes * (v * scale)));
            out.push(PieceGpu {
                a: to_view(sa.position),
                segment: self.segment,
                b: to_view(sb.position),
                path: self.path,
                fractions: [from.0 as f32, to.0 as f32],
                _pad: [0; 2],
            });
            return;
        }
        let middle = (from.0 + to.0) / 2.0;
        let state = (middle, self.at(middle));
        self.halve(out, from, state, depth + 1);
        self.halve(out, state, to, depth + 1);
    }
}

/// The nearest the curve between `a` and `b` comes to `eye`, at least: each
/// of its points is within [`bow`] of the chord's point at the same
/// fraction, so no nearer than the chord less that.
fn nearest(a: &Sample, b: &Sample, eye: DVec3) -> f64 {
    let chord = b.position - a.position;
    let along = (eye - a.position).dot(chord) / chord.length_squared().max(f64::MIN_POSITIVE);
    let closest = a.position + chord * along.clamp(0.0, 1.0);
    (eye - closest).length() - bow(a, b)
}

/// The pieces a segment bowing `bow_px` from its chord needs so that each
/// bows at most [`MAX_BOW_PX`] (a piece of 1/n bows 1/n² as much): the
/// least power of two; past [`MAX_PIECES`], twice that (for the CPU).
fn pieces(bow_px: f64) -> u32 {
    let mut n: u32 = 1;
    while n <= MAX_PIECES && bow_px / f64::from(n * n) > MAX_BOW_PX {
        n *= 2;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    /// High and low parts carry an f64 to ~1e-14 of itself.
    #[test]
    fn split_keeps_f64_precision() {
        for x in [1.496e8 + 0.123_456_789, -7.78e8 - 1e-3, 6_778.0, 0.0] {
            let [hi, lo] = split(x);
            let back = f64::from(hi) + f64::from(lo);
            assert!((back - x).abs() <= x.abs() * 1e-14, "{x} {back}");
        }
    }

    /// A pose maps frame coordinates into the world and back.
    #[test]
    fn pose_inverts() {
        let axes = DMat3::from_rotation_z(0.7) * DMat3::from_rotation_x(-0.3);
        let pose = Pose {
            origin: DVec3::new(1.4e8, -3e7, 2e4),
            axes,
            scale: 384_400.0,
        };
        let p = DVec3::new(0.98, -0.02, 0.15);
        let back = pose.from_world(pose.to_world(p));
        assert!((back - p).length() < 1e-14, "{back}");
    }

    /// A circle sampled four times a turn, seen face on: `near` follows
    /// the Hermite curve (1.5% inside the circle at 45°), ~0.3 of the
    /// radius off its chords.
    #[test]
    fn near_follows_the_curve() {
        let (r, w) = (1e4, 1e-3);
        let path = Path {
            pose: 0,
            samples: (0..=4)
                .map(|i| {
                    let (t, a) = (
                        f64::from(i) * 1_570.796_326_794_896_6,
                        f64::from(i) * 0.5 * std::f64::consts::PI,
                    );
                    let (s, c) = a.sin_cos();
                    Sample {
                        t,
                        position: DVec3::new(r * c, r * s, 0.0),
                        velocity: DVec3::new(-r * w * s, r * w * c, 0.0),
                    }
                })
                .collect(),
            future: [1.0; 3],
            past: [1.0; 3],
            gap: 0.0,
        };
        let pose = Pose {
            origin: DVec3::new(1.5e8, 0.0, 0.0),
            axes: DMat3::IDENTITY,
            scale: 1.0,
        };
        let camera = Camera::looking_at(
            pose.origin + DVec3::new(0.0, 0.0, 3e4),
            pose.origin,
            DVec3::Y,
            50f64.to_radians(),
        );
        let size = (800, 800);
        let on = |a: f64, k: f64| {
            let p = pose.origin + DVec3::new(k * r * a.cos(), k * r * a.sin(), 0.0);
            camera.project(p, size).unwrap()
        };
        let a = 0.25 * std::f64::consts::PI;
        let mid = camera
            .project(pose.to_world(path.at(785.398_163_397_448_3).unwrap()), size)
            .unwrap();
        let curve = path.near(&pose, &camera, size, mid, 4.0, |_| true).unwrap();
        let chord = path
            .near(&pose, &camera, size, on(a, 0.5f64.sqrt()), 400.0, |_| true)
            .unwrap();
        let px = (mid - on(a, 0.5f64.sqrt())).length();
        println!("on the curve {curve:.3} px; at the chord {chord:.1} px ({px:.1} px in)");
        assert!(curve < 0.25, "{curve}");
        assert!((chord - px).abs() < 1.0, "{chord} {px}");
        assert!(
            path.near(&pose, &camera, size, on(a, 0.5f64.sqrt()), 4.0, |_| true)
                .is_none()
        );
        // Behind the eye, nothing.
        let behind = Camera::looking_at(
            pose.origin + DVec3::new(0.0, 0.0, 3e4),
            pose.origin + DVec3::new(0.0, 0.0, 6e4),
            DVec3::Y,
            0.8,
        );
        assert!(
            path.near(&pose, &behind, size, DVec2::new(400.0, 400.0), 1e6, |_| {
                true
            })
            .is_none()
        );
    }

    /// Against the curve sampled densely, over random one-sided segments
    /// seen from near and far, off the view's axis and bowing toward the
    /// eye: a click within 3.5 px of the curve is never culled, and
    /// `near` is within 0.3 px of the truth. Hidden pieces are not
    /// counted.
    #[test]
    fn near_never_misses_the_curve() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut f = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let size = (1920, 1080);
        let pose = Pose {
            origin: DVec3::new(1.5e8, 0.0, 0.0),
            axes: DMat3::IDENTITY,
            scale: 1.0,
        };
        let (mut tried, mut worst) = (0, 0.0f64);
        for trial in 0..1500 {
            let mut v = || DVec3::new(2.0 * f() - 1.0, 2.0 * f() - 1.0, 2.0 * f() - 1.0);
            let (r, h) = (1e4, 1000.0);
            let (p0, p1) = (v() * r, v() * r);
            let chord = p1 - p0;
            let off = v().normalize() * chord.length();
            let eye = if trial % 2 == 0 {
                p0 + chord * 0.5 + off * 0.4 + v() * 0.3 * r
            } else {
                v() * 2.0 * r
            };
            let target = v() * r;
            let off = off * (0.5 + 2.0 * f());
            let path = Path {
                pose: 0,
                samples: vec![
                    Sample {
                        t: 0.0,
                        position: p0,
                        velocity: (chord + off) / h,
                    },
                    Sample {
                        t: h,
                        position: p1,
                        velocity: chord / h,
                    },
                ],
                future: [1.0; 3],
                past: [1.0; 3],
                gap: 0.0,
            };
            let camera = Camera::looking_at(
                pose.origin + eye,
                pose.origin + target,
                DVec3::Z,
                50f64.to_radians(),
            );
            let project = |t: f64| camera.project(pose.to_world(path.at(t).unwrap()), size);
            let Some(on) = project(h * (0.1 + 0.8 * f())) else {
                continue;
            };
            if on.x < 0.0 || on.y < 0.0 || on.x > 1920.0 || on.y > 1080.0 {
                continue;
            }
            let at = on + DVec2::new(2.0 * f() - 1.0, 2.0 * f() - 1.0) * 3.0;
            let mut truth = f64::INFINITY;
            let mut last = project(0.0);
            for k in 1..=20_000 {
                let p = project(h * f64::from(k) / 20_000.0);
                if let (Some(p0), Some(p1)) = (last, p) {
                    truth = truth.min(segment_distance(at, p0, p1));
                }
                last = p;
            }
            if truth > 3.5 {
                continue;
            }
            tried += 1;
            let got = path.near(&pose, &camera, size, at, 4.0, |_| true);
            let Some(got) = got else {
                panic!("trial {trial}: culled at {truth:.3} px from the curve")
            };
            worst = worst.max((got - truth).abs());
            assert!(
                path.near(&pose, &camera, size, at, 4.0, |_| false)
                    .is_none()
            );
        }
        println!("{tried} clicks; worst {worst:.3} px from the dense curve");
        assert!(tried > 300 && worst < 0.3, "{tried} {worst}");
    }

    /// Near the screen's edge (0.6 of the depth off the axis), a segment
    /// bowing out and toward the eye strays 142 px from its chord where
    /// bow / depth gives 116 px: a click on the curve there is found (with
    /// bow / depth for its bound, it was culled; found in review).
    #[test]
    fn near_bounds_a_curve_off_the_axis() {
        let (z, bow, h) = (1e4, 1e3, 1e3);
        let (p0, p1) = (DVec3::new(0.6 * z, -1e3, -z), DVec3::new(0.6 * z, 1e3, -z));
        let chord = p1 - p0;
        // One-sided: the curve is the chord plus h₁₀(u)·off, |h₁₀| ≤ 4/27
        // at u = 1/3.
        let off = DVec3::new(1.0, 0.0, 0.6).normalize() * bow * 27.0 / 4.0;
        let path = Path {
            pose: 0,
            samples: vec![
                Sample {
                    t: 0.0,
                    position: p0,
                    velocity: (chord + off) / h,
                },
                Sample {
                    t: h,
                    position: p1,
                    velocity: chord / h,
                },
            ],
            future: [1.0; 3],
            past: [1.0; 3],
            gap: 0.0,
        };
        let pose = Pose {
            origin: DVec3::ZERO,
            axes: DMat3::IDENTITY,
            scale: 1.0,
        };
        let camera = Camera::looking_at(DVec3::ZERO, -DVec3::Z, DVec3::Y, 50f64.to_radians());
        let size = (1920, 1080);
        let apex = camera.project(path.at(h / 3.0).unwrap(), size).unwrap();
        let on_chord = camera.project(p0 + chord / 3.0, size).unwrap();
        let estimate = bow / z / camera.pixel_angle(size.1);
        println!(
            "the curve {:.0} px from its chord, bow / depth {estimate:.0} px",
            (apex - on_chord).length()
        );
        assert!((apex - on_chord).length() > estimate + 20.0);
        let d = path.near(&pose, &camera, size, apex, 4.0, |_| true);
        assert!(d.is_some_and(|d| d < 0.25), "{d:?}");
    }

    /// A low orbit sampled every 2°, `turns` times, from t = 1.4e9 s.
    fn leo(turns: usize) -> Path {
        let r = 6_778.0;
        let w = 2.0 * std::f64::consts::PI / 5_550.0;
        Path {
            pose: 0,
            samples: (0..=180 * turns)
                .map(|k| {
                    let a = k as f64 * 2f64.to_radians();
                    let (s, c) = a.sin_cos();
                    Sample {
                        t: 1.4e9 + a / w,
                        position: DVec3::new(r * c, r * s, 0.0),
                        velocity: DVec3::new(-r * w * s, r * w * c, 0.0),
                    }
                })
                .collect(),
            future: [1.0; 3],
            past: [0.5; 3],
            gap: 0.0,
        }
    }

    /// A circle seen from far and from close: chunks off the screen are
    /// culled, and the parts near the eye cut finer the nearer they are;
    /// from 1 km, beyond the GPU's 32 pieces, by the CPU.
    #[test]
    fn chunks_culled_and_cut_by_distance() {
        let paths = Paths {
            paths: vec![leo(4)],
        };
        let prepared = Prepared::new(&paths);
        assert_eq!((prepared.chunks.len(), prepared.parts.len()), (12, 90));
        // 2° of LEO bows ~1 km from its chord; the bound is within 25%.
        let (bow, sagitta) = (
            prepared.chunks[0].run.bow,
            6_778.0 * (1.0 - 1f64.to_radians().cos()),
        );
        assert!(bow > sagitta && bow < 1.25 * sagitta, "{bow} {sagitta}");
        let poses = [Pose::default()];
        let fov = 50f64.to_radians();
        let far = Camera::looking_at(DVec3::new(0.0, -1e5, 3e4), DVec3::ZERO, DVec3::Z, fov);
        let v = prepared.visible(&paths, &far, &poses, (800, 600), 0.0);
        assert_eq!(v.draws, vec![(64, 1, 0..12)]);
        assert!(v.pieces.is_empty());
        // 10 km above the circle, looking along it: the GPU cuts.
        let eye = DVec3::new(6_778.0, 0.0, 10.0);
        let close = Camera::looking_at(eye, eye + DVec3::Y, DVec3::Z, fov);
        let v = prepared.visible(&paths, &close, &poses, (800, 600), 0.0);
        let pieces: Vec<u32> = v.draws.iter().map(|d| d.1).collect();
        println!("10 km: {:?}, {} pieces placed", v.draws, v.pieces.len());
        assert!(pieces.contains(&MAX_PIECES) && pieces.iter().any(|&n| n > 1 && n < MAX_PIECES));
        assert!(v.pieces.is_empty());
        // 1 km above it: the CPU, for the segments nearest.
        let eye = DVec3::new(6_778.0, 0.0, 1.0);
        let closer = Camera::looking_at(eye, eye + DVec3::Y, DVec3::Z, fov);
        let v = prepared.visible(&paths, &closer, &poses, (800, 600), 0.0);
        println!("1 km: {:?}, {} pieces placed", v.draws, v.pieces.len());
        assert!(!v.pieces.is_empty() && v.pieces.len() < 500);
        assert!(
            v.instances
                .iter()
                .all(|i| (i[1] & 0xffff) as usize <= CHUNK_SEGMENTS)
        );
    }

    /// An S-shaped segment (both velocities across its chord) strays from
    /// its chord though its middle does not: culling still sees it (found
    /// in review: the midpoint's offset alone was 0).
    #[test]
    fn s_curve_is_not_culled() {
        let (a, b) = (
            Sample {
                t: 0.0,
                position: DVec3::new(-100.0, 0.0, 0.0),
                velocity: DVec3::new(0.0, 0.0, 10.0),
            },
            Sample {
                t: 100.0,
                position: DVec3::new(100.0, 0.0, 0.0),
                velocity: DVec3::new(0.0, 0.0, 10.0),
            },
        );
        let path = Path {
            pose: 0,
            samples: vec![a, b],
            future: [1.0; 3],
            past: [1.0; 3],
            gap: 0.0,
        };
        // The curve's highest point, 96 km above the chord at u ≈ 0.21
        // (1,000 km · u(2u − 1)(u − 1)).
        let top = (0..=100)
            .map(|k| path.at(f64::from(k)).unwrap())
            .max_by(|p, q| p.z.total_cmp(&q.z))
            .unwrap();
        assert!(top.z > 90.0 && top.z < bow(&a, &b), "{top} {}", bow(&a, &b));
        let paths = Paths { paths: vec![path] };
        let prepared = Prepared::new(&paths);
        // Looking straight at the top from 50 km, the chord out of view.
        let camera = Camera::looking_at(
            top + DVec3::new(0.0, -50.0, 0.0),
            top,
            DVec3::Z,
            20f64.to_radians(),
        );
        let v = prepared.visible(&paths, &camera, &[Pose::default()], (400, 300), 0.0);
        assert!(!v.instances.is_empty() || !v.pieces.is_empty());
    }

    /// The line's curve passes through the samples, and between them
    /// stays within r·θ⁴/384 of a circle sampled every θ (2.6 cm in LEO).
    #[test]
    fn path_at_follows_the_circle() {
        let path = leo(1);
        assert_eq!(path.at(path.samples[3].t), Some(path.samples[3].position));
        let (t0, t1) = (path.samples[0].t, path.samples[10].t);
        let worst = (0..1_000)
            .map(|k| {
                let t = t0 + (t1 - t0) * f64::from(k) / 1_000.0;
                (path.at(t).unwrap().length() - 6_778.0).abs()
            })
            .fold(0.0, f64::max);
        assert!(worst < 3e-5, "{worst} km");
        assert!(path.at(t0 - 1.0).is_none());
    }

    /// Splits: before the path, nothing is past; after it, everything;
    /// between, the segment and fraction, in f64 however long the path.
    #[test]
    fn splits_at_the_moment() {
        // 100 days of LEO.
        let path = leo(1_560);
        let n = path.samples.len();
        assert_eq!(path.split(0.0), (0, 0.0));
        assert_eq!(path.split(2e9), (n - 1, 0.0));
        let k = n - 10;
        let t = path.samples[k].t + 0.25 * (path.samples[k + 1].t - path.samples[k].t);
        let (got, u) = path.split(t);
        assert_eq!(got, k);
        // Times near 1.4e9 s carry ~1e-7 s.
        assert!((u - 0.25).abs() < 1e-7, "{u}");
    }
}
