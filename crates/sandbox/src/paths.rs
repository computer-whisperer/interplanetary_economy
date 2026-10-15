//! A plan's paths and events as drawn (M1c.md, step 4 plan, items 4 and
//! 5): each item's track mapped into the view's frame at its samples' own
//! times, colored by leg; the station orbits' reference loops; and markers
//! for burns, passes and events where they happen.
//!
//! Times on the paths' axis are seconds since J2000 (`Epoch` × 86,400).

use crate::frames::{Frame, loop_frame};
use crate::plan::{Ship, event_time, on_track, position};
use glam::DVec3;
use orbit::ephem::{BodyId, Ephemeris};
use orbit::mission::{Item, ItemKind, MarkerKind};
use orbit::time::{Epoch, SECONDS_PER_DAY};
use rayon::prelude::*;
use render::{Path, Sample, Shape};

/// The poses paths are drawn with (`render::Path::pose`): the view's frame,
/// and the frame reference loops are in; the bodies' orbits' follow
/// (`orbits`).
pub const VIEW_POSE: usize = 0;
pub const LOOP_POSE: usize = 1;

/// The scene's sets of paths (`render::Scene::paths`): the plan's with the
/// reference loops, and the bodies' orbits.
pub const PLAN_SET: usize = 0;
pub const ORBIT_SET: usize = 1;

/// Legs' colors, in order (sRGB).
const LEG_COLORS: [u32; 8] = [
    0x5fa8ff, 0xffa94d, 0x69db7c, 0xff6b6b, 0xda77f2, 0xffd43b, 0x3bc9db, 0xf783ac,
];

/// A stay's color, from its leg's; before any leg, gray.
const STAY_DIM: f32 = 0.55;
const FIRST_STAY: [f32; 3] = [0.3; 3];

/// The past's color, from the future's.
pub const PAST_DIM: f32 = 0.35;

/// Reference loops' color.
const LOOP_COLOR: [f32; 3] = [0.09; 3];

/// Events that are not a leg's own color: an atmospheric pass, and a
/// junction's jump.
const PASS_COLOR: [f32; 3] = [1.0, 0.45, 0.12];
const JUMP_COLOR: [f32; 3] = [1.0, 0.12, 0.08];

/// A leg that could not be designed, where it is drawn (the timeline).
pub const FAILURE_COLOR: [f32; 3] = JUMP_COLOR;

/// Linear values of an sRGB color.
fn linear(rgb: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| {
        let c = ((rgb >> shift) & 0xff) as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    })
}

fn scaled(c: [f32; 3], k: f32) -> [f32; 3] {
    c.map(|x| x * k)
}

/// An event on a path: where it is in the view's frame, and how it is
/// drawn.
#[derive(Clone, Copy, Debug)]
pub struct Event {
    /// Its ship, the ship's item, and the item's marker, by index.
    pub ship: usize,
    pub item: usize,
    pub marker: usize,
    pub t: Epoch,
    pub position: DVec3,
    pub shape: Shape,
    pub color: [f32; 3],
    pub size_px: f32,
}

/// Each item's color (linear), in a ship's order: legs (and landings and
/// ascents) by their place in the plan, the stays after them dimmer.
pub fn colors(ship: &Ship) -> Vec<[f32; 3]> {
    let mut leg = None;
    ship.items
        .iter()
        .map(|item| match item.kind {
            ItemKind::Leg | ItemKind::Landing | ItemKind::Ascent => {
                let k = leg.map_or(ship.first_leg, |k| k + 1);
                leg = Some(k);
                linear(LEG_COLORS[k % LEG_COLORS.len()])
            }
            _ => leg.map_or(FIRST_STAY, |k| {
                scaled(linear(LEG_COLORS[k % LEG_COLORS.len()]), STAY_DIM)
            }),
        })
        .collect()
}

/// An item's path in `frame` (with the camera on `focus`): none for an
/// item without a track, or resting on or at a body.
fn item_path(
    eph: &Ephemeris,
    frame: &Frame,
    focus: BodyId,
    item: &Item,
    color: [f32; 3],
) -> Option<Path> {
    let track = item
        .track
        .as_ref()
        .filter(|t| !t.rests() && t.times.len() > 1)?;
    // Where the frame is known (a small body as the focus, from 2040).
    let samples: Vec<Sample> = track
        .times
        .par_iter()
        .zip(&track.states)
        .filter_map(|(&s, state)| {
            let t = Epoch(track.epoch.0 + s / SECONDS_PER_DAY);
            if !frame.covers(eph, focus, t) {
                return None;
            }
            let (mut r, mut v) = (
                DVec3::new(state[0], state[1], state[2]),
                DVec3::new(state[3], state[4], state[5]),
            );
            if let Some(c) = track.center {
                let c = eph.barycentric(c, t);
                r += c.r;
                v += c.v;
            }
            Some(frame.sample(eph, focus, t, track.epoch.seconds() + s, r, v))
        })
        .collect();
    if samples.len() < 2 {
        return None;
    }
    Some(Path {
        pose: VIEW_POSE,
        samples,
        future: color,
        past: scaled(color, PAST_DIM),
        gap: 0.0,
    })
}

/// The reference loops, in the loop frame.
pub fn loop_paths(loops: Vec<Vec<Sample>>) -> Vec<Path> {
    loops
        .into_iter()
        .map(|samples| Path {
            pose: LOOP_POSE,
            samples,
            future: LOOP_COLOR,
            past: LOOP_COLOR,
            gap: 0.0,
        })
        .collect()
}

/// What is drawn of a plan in a frame.
#[derive(Clone, Debug, Default)]
pub struct Built {
    pub paths: Vec<Path>,
    /// Each path's ship and item, by their indices, where it is one's.
    pub owners: Vec<Option<(usize, usize)>>,
    pub events: Vec<Event>,
}

/// The ships' paths and events in `frame`, after `loops`.
pub fn build(
    eph: &Ephemeris,
    frame: &Frame,
    focus: BodyId,
    loops: &[Path],
    ships: &[Ship],
) -> Built {
    let colored: Vec<(usize, usize, [f32; 3])> = ships
        .iter()
        .enumerate()
        .flat_map(|(s, ship)| {
            colors(ship)
                .into_iter()
                .enumerate()
                .map(move |(k, c)| (s, k, c))
        })
        .collect();
    let drawn: Vec<(Path, (usize, usize))> = colored
        .par_iter()
        .filter_map(|&(s, k, color)| {
            item_path(eph, frame, focus, &ships[s].items[k], color).map(|p| (p, (s, k)))
        })
        .collect();
    let mut out = Built {
        paths: loops.to_vec(),
        owners: vec![None; loops.len()],
        events: Vec::new(),
    };
    for (path, owner) in drawn {
        out.paths.push(path);
        out.owners.push(Some(owner));
    }
    for &(s, k, color) in &colored {
        let (ship, item) = (&ships[s], &ships[s].items[k]);
        for (i, m) in item.markers.iter().enumerate() {
            // A landing on the track it leaves, an ascent on the one it
            // reaches; the rest on the item's own.
            let t = event_time(item, m);
            let r = match m.kind {
                MarkerKind::Landing => position(eph, &ship.items, t, false),
                MarkerKind::Ascent => position(eph, &ship.items, t, true),
                _ => item.track.as_ref().and_then(|tr| on_track(eph, tr, t)),
            };
            let Some(r) = r.filter(|_| frame.covers(eph, focus, t)) else {
                continue;
            };
            let (shape, color, size_px) = marker_style(m.kind, color);
            out.events.push(Event {
                ship: s,
                item: k,
                marker: i,
                t,
                position: frame.pose(eph, focus, t).from_world(r),
                shape,
                color,
                size_px,
            });
        }
    }
    out
}

/// How a marker is drawn on an item of `color` (linear): its shape, its
/// color and its size, logical px.
pub fn marker_style(kind: MarkerKind, color: [f32; 3]) -> (Shape, [f32; 3], f32) {
    match kind {
        MarkerKind::Burn => (Shape::Diamond, color, 9.0),
        MarkerKind::Flyby => (Shape::Ring, color, 11.0),
        MarkerKind::Pass => (Shape::Ring, PASS_COLOR, 11.0),
        MarkerKind::Landing | MarkerKind::Ascent => (Shape::Block, color, 9.0),
        MarkerKind::Jump { .. } => (Shape::Cross, JUMP_COLOR, 11.0),
    }
}

/// The poses paths are placed with at `t`, by [`VIEW_POSE`] and
/// [`LOOP_POSE`].
pub fn poses(eph: &Ephemeris, frame: &Frame, focus: BodyId, t: Epoch) -> Vec<render::Pose> {
    vec![
        frame.pose(eph, focus, t),
        loop_frame(eph).pose(eph, focus, t),
    ]
}
