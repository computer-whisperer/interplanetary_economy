//! A plan's paths and events as drawn (M1c.md, step 4 plan, items 4 and
//! 5): each item's track mapped into the view's frame at its samples' own
//! times, colored by leg; the station orbits' reference loops; and markers
//! for burns, passes and events where they happen. A stay, drawn a
//! revolution about the clock, is built from samples made about it, and
//! built again when the clock moves on (M1b.md, step 2 plan, part 6).
//!
//! Times on the paths' axis are seconds since J2000 (`Epoch` × 86,400).

use crate::frames::{Frame, loop_frame};
use crate::plan::{Ship, event_time, on_track, position};
use glam::DVec3;
use orbit::ephem::{BodyId, Ephemeris};
use orbit::mission::{Item, ItemKind, MarkerKind};
use orbit::time::{Epoch, SECONDS_PER_DAY};
use orbit::track::Track;
use rayon::prelude::*;
use render::{Path, Paths, Sample, Shape};
use std::sync::Arc;

/// The poses paths are drawn with (`render::Path::pose`): the view's frame,
/// and the frame reference loops are in; the bodies' orbits' follow
/// (`orbits`).
pub const VIEW_POSE: usize = 0;
pub const LOOP_POSE: usize = 1;

/// The scene's sets of paths (`render::Scene::paths`), each uploaded
/// apart: the reference loops, then each ship's (from [`SHIP_SETS`], so
/// that a ship's new items or edit build and upload its own alone), then
/// the bodies' orbits, a set each (`Shell::orbit_sets`).
pub const LOOP_SET: usize = 0;
pub const SHIP_SETS: usize = 1;

/// Legs' colors, in order (sRGB).
const LEG_COLORS: [u32; 8] = [
    0x5fa8ff, 0xffa94d, 0x69db7c, 0xff6b6b, 0xda77f2, 0xffd43b, 0x3bc9db, 0xf783ac,
];

/// A stay's color, from its leg's; before any leg, gray.
const STAY_DIM: f32 = 0.55;
const FIRST_STAY: [f32; 3] = [0.3; 3];

/// The past's color, from the future's.
pub const PAST_DIM: f32 = 0.35;

/// What a ship that ran dry does not fly after, from its color (M1b.md,
/// step 2 plan, part 4).
pub const NOT_FLOWN_DIM: f32 = 0.2;

/// Reference loops' color.
const LOOP_COLOR: [f32; 3] = [0.09; 3];

/// Events that are not a leg's own color: an atmospheric pass, and a
/// junction's jump.
const PASS_COLOR: [f32; 3] = [1.0, 0.45, 0.12];
const JUMP_COLOR: [f32; 3] = [1.0, 0.12, 0.08];

/// A leg that could not be designed, where it is drawn (the timeline);
/// the burn a ship ran dry at.
pub const FAILURE_COLOR: [f32; 3] = JUMP_COLOR;

/// A stay drawn a revolution about the clock is built from samples this
/// many revolutions each way of it, and again once the clock has moved a
/// revolution and a half (at a day a second, in LEO, ten times a second;
/// the ship's other lines kept, [`rebuilt`]).
const STAY_REVOLUTIONS: f64 = 2.0;

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
/// ascents) by their place in the plan, the stays after them dimmer; after
/// the item a ship ran dry in, dimmer still (not flown).
pub fn colors(ship: &Ship) -> Vec<[f32; 3]> {
    let mut leg = None;
    let flown = ship.dry().map_or(usize::MAX, |d| d.item);
    ship.items
        .iter()
        .enumerate()
        .map(|(k, item)| {
            (
                k,
                match item.kind {
                    ItemKind::Leg | ItemKind::Landing | ItemKind::Ascent => {
                        let k = leg.map_or(ship.first_leg, |k| k + 1);
                        leg = Some(k);
                        linear(LEG_COLORS[k % LEG_COLORS.len()])
                    }
                    _ => leg.map_or(FIRST_STAY, |k| {
                        scaled(linear(LEG_COLORS[k % LEG_COLORS.len()]), STAY_DIM)
                    }),
                },
            )
        })
        .map(|(k, c)| match k > flown {
            true => scaled(c, NOT_FLOWN_DIM),
            false => c,
        })
        .collect()
}

/// `path` split at `t` (s since J2000), at its last sample then or before
/// (on a leg, a burn's own): before it as it is, after it in `rest`, its
/// past dimmed as any path's (the halves share that sample, so they join).
fn split(path: Path, t: f64, rest: [f32; 3]) -> Vec<Path> {
    let k = path.samples.partition_point(|s| s.t <= t);
    let after = Path {
        samples: path.samples[k.saturating_sub(1)..].to_vec(),
        future: rest,
        past: scaled(rest, PAST_DIM),
        ..path.clone()
    };
    let before = Path {
        samples: path.samples[..k].to_vec(),
        ..path
    };
    [before, after]
        .into_iter()
        .filter(|p| p.samples.len() > 1)
        .collect()
}

/// What of a track is drawn with the clock at `t`: from `from` to `to` (s
/// after its epoch), as built while the clock is within `valid` (s since
/// J2000).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub from: f64,
    pub to: f64,
    pub valid: (f64, f64),
}

impl Window {
    pub fn holds(&self, t: Epoch) -> bool {
        (self.valid.0..=self.valid.1).contains(&t.seconds())
    }
}

/// What of `track` is drawn with the clock at `t`: of a stay drawn a
/// revolution about the clock, samples made [`STAY_REVOLUTIONS`] each way
/// of it, clamped to the stay, for as long as the revolution drawn about
/// the clock (the renderer's, `render::Path::drawn`) is among them; of
/// anything else, all of it, for all time.
pub fn window(track: &Track, t: Epoch) -> Option<Window> {
    let (first, last) = track.span()?;
    let all = Window {
        from: first,
        to: last,
        valid: (f64::NEG_INFINITY, f64::INFINITY),
    };
    let Some(revolution) = track.revolution else {
        return Some(all);
    };
    let (reach, half) = (STAY_REVOLUTIONS * revolution, 0.5 * revolution);
    if last - first <= 2.0 * reach {
        return Some(all);
    }
    let now = (t.0 - track.epoch.0) * SECONDS_PER_DAY;
    let at = |s: f64| track.epoch.seconds() + s;
    // Before the stay or after it, its first or last revolution is drawn.
    Some(if now <= first + reach {
        let to = first + 2.0 * reach;
        Window {
            from: first,
            to,
            valid: (f64::NEG_INFINITY, at(to - half)),
        }
    } else if now >= last - reach {
        let from = last - 2.0 * reach;
        Window {
            from,
            to: last,
            valid: (at(from + half), f64::INFINITY),
        }
    } else {
        Window {
            from: now - reach,
            to: now + reach,
            valid: (at(now - reach + half), at(now + reach - half)),
        }
    })
}

/// The clock's span over which `item` is drawn as built with the clock at
/// `t` ([`window`]): all time for an item without a track.
fn valid(item: &Item, t: Epoch) -> (f64, f64) {
    item.track
        .as_ref()
        .and_then(|track| window(track, t))
        .map_or((f64::NEG_INFINITY, f64::INFINITY), |w| w.valid)
}

/// An item's path in `frame` (with the camera on `focus`), the clock at
/// `t` ([`window`]). None for an item without a track, or resting on or at
/// a body.
fn item_path(
    eph: &Ephemeris,
    frame: &Frame,
    focus: BodyId,
    item: &Item,
    color: [f32; 3],
    t: Epoch,
) -> Option<Path> {
    let track = item.track.as_ref().filter(|t| !t.rests())?;
    let w = window(track, t)?;
    let made = track.samples(w.from, w.to);
    if made.len() < 2 {
        return None;
    }
    // Where the frame is known (a small body as the focus, from 2040).
    let samples: Vec<Sample> = made
        .par_iter()
        .filter_map(|&(s, state)| {
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
        window: track.revolution,
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
            window: None,
        })
        .collect()
}

/// What is drawn of a ship in a frame.
#[derive(Clone, Debug, Default)]
pub struct Built {
    pub set: Arc<Paths>,
    /// Each path's item, by its index in the ship's.
    pub items: Vec<usize>,
    /// Each of the ship's items' span of the clock over which it is drawn
    /// as built, s since J2000 ([`window`]), drawn or not.
    pub valid: Vec<(f64, f64)>,
    /// Its events, as ship `s`'s.
    pub events: Vec<Event>,
}

impl Built {
    /// Whether all of it is drawn as built with the clock at `t`.
    pub fn holds(&self, t: Epoch) -> bool {
        let now = t.seconds();
        self.valid.iter().all(|v| (v.0..=v.1).contains(&now))
    }
}

/// How marker `i` of `ship`'s item `k` is drawn, `colors` its items' (as
/// [`colors`] gives them): the burn the ship ran dry at, a red block; what
/// it did not fly after, dimmed (its items' own colors are, by `colors`;
/// a pass's or a jump's, and its later markers on the item it ran dry in,
/// here).
pub fn event_style(ship: &Ship, colors: &[[f32; 3]], k: usize, i: usize) -> (Shape, [f32; 3], f32) {
    let m = &ship.items[k].markers[i];
    let (shape, color, size) = marker_style(m.kind, colors[k]);
    let own = matches!(m.kind, MarkerKind::Pass | MarkerKind::Jump { .. });
    match ship.dry() {
        Some(d) if (d.item, d.marker) == (k, Some(i)) => (Shape::Block, FAILURE_COLOR, 13.0),
        Some(d) if (k > d.item && own) || (k == d.item && m.t.0 > d.t.0) => {
            (shape, scaled(color, NOT_FLOWN_DIM), size)
        }
        _ => (shape, color, size),
    }
}

/// Ship `s`'s paths and events in `frame`, the clock at `t`: where it ran
/// dry, its path after then not flown (a stay, drawn a revolution about
/// the clock, is not split), and the burn marked.
pub fn build(
    eph: &Ephemeris,
    frame: &Frame,
    focus: BodyId,
    s: usize,
    ship: &Ship,
    t: Epoch,
) -> Built {
    let colors = colors(ship);
    let dry = ship.dry();
    let (paths, items): (Vec<Path>, Vec<usize>) = colors
        .par_iter()
        .enumerate()
        .filter_map(|(k, &color)| {
            item_path(eph, frame, focus, &ship.items[k], color, t).map(|p| (p, k))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .flat_map(
            |(p, k)| match dry.filter(|d| d.item == k && p.window.is_none()) {
                Some(d) => split(p, d.t.seconds(), scaled(colors[k], NOT_FLOWN_DIM))
                    .into_iter()
                    .map(|p| (p, k))
                    .collect(),
                None => vec![(p, k)],
            },
        )
        .unzip();
    let mut out = Built {
        set: Arc::new(Paths { paths }),
        items,
        valid: ship.items.iter().map(|i| valid(i, t)).collect(),
        events: Vec::new(),
    };
    for (k, item) in ship.items.iter().enumerate() {
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
            let (shape, color, size_px) = event_style(ship, &colors, k, i);
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

/// Ship `ship`'s lines `old`, built with the clock elsewhere, with the
/// clock at `t`: its items' that no longer hold made again about it (a
/// stay's, never split where it ran dry), the rest and the events kept.
pub fn rebuilt(
    old: &Built,
    eph: &Ephemeris,
    frame: &Frame,
    focus: BodyId,
    ship: &Ship,
    t: Epoch,
) -> Built {
    let colors = colors(ship);
    let now = t.seconds();
    let made: Vec<(Vec<Path>, (f64, f64))> = (0..ship.items.len())
        .into_par_iter()
        .map(|k| {
            let v = old.valid[k];
            match (v.0..=v.1).contains(&now) {
                true => {
                    let kept = old.items.iter().zip(&old.set.paths);
                    let kept = kept.filter(|(i, _)| **i == k).map(|(_, p)| p.clone());
                    (kept.collect(), v)
                }
                false => {
                    let item = &ship.items[k];
                    let path = item_path(eph, frame, focus, item, colors[k], t);
                    (path.into_iter().collect(), valid(item, t))
                }
            }
        })
        .collect();
    let mut out = Built {
        events: old.events.clone(),
        ..Built::default()
    };
    let mut paths = Vec::new();
    for (k, (made, v)) in made.into_iter().enumerate() {
        out.items.extend(made.iter().map(|_| k));
        paths.extend(made);
        out.valid.push(v);
    }
    out.set = Arc::new(Paths { paths });
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Split at a time, a path's two halves meet at the sample before it,
    /// the second in the color given; a half of one sample is not drawn.
    #[test]
    fn a_path_splits_at_a_time() {
        let path = Path {
            pose: VIEW_POSE,
            samples: (0..5)
                .map(|k| Sample {
                    t: k as f64,
                    position: DVec3::splat(k as f64),
                    velocity: DVec3::ZERO,
                })
                .collect(),
            future: [1.0; 3],
            past: [0.5; 3],
            gap: 0.0,
            window: None,
        };
        let halves = split(path.clone(), 2.5, [0.1; 3]);
        let times = |p: &Path| p.samples.iter().map(|s| s.t).collect::<Vec<_>>();
        assert_eq!(times(&halves[0]), [0.0, 1.0, 2.0]);
        assert_eq!(times(&halves[1]), [2.0, 3.0, 4.0]);
        assert_eq!((halves[0].future, halves[0].past), ([1.0; 3], [0.5; 3]));
        assert_eq!(
            (halves[1].future, halves[1].past),
            ([0.1; 3], scaled([0.1; 3], PAST_DIM))
        );
        // At its first sample, all of it is after; past its last, before.
        let after = split(path.clone(), 0.0, [0.1; 3]);
        assert_eq!((after.len(), after[0].future), (1, [0.1; 3]));
        let before = split(path, 9.0, [0.1; 3]);
        assert_eq!((before.len(), before[0].future), (1, [1.0; 3]));
        assert_eq!(times(&before[0]), [0.0, 1.0, 2.0, 3.0, 4.0]);
    }
}
