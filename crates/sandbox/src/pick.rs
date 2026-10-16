//! Picking on the map view (M1c.md, step 5 plan, item 3): what a click
//! lands on, from the projections the labels made (`labels`), and the
//! paths' lines, on the CPU in f64. A click picks, in turn: a label; a
//! ship or a marker ([`marked`]); a line ([`line`]); a body ([`body`]).
//! Lines come before bodies so that a line over a body's disk (a low
//! orbit, seen from close) can be picked.

use crate::labels::{Anchor, Ball, Placed, Target, View, hidden};
use glam::Vec2;
use orbit::ephem::BodyId;
use rayon::prelude::*;

/// How near a click must come to a ship, a marker or a body's point, and
/// to a line, logical px.
pub const POINT_PX: f32 = 8.0;
pub const LINE_PX: f64 = 4.0;

/// What a click picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Body(BodyId),
    Ship(usize),
    /// A ship's item.
    Item(usize, usize),
}

fn of(target: Target) -> Option<Pick> {
    match target {
        Target::Body(b) => Some(Pick::Body(b)),
        Target::Ship(s) => Some(Pick::Ship(s)),
        Target::Marker { ship, item, .. } => Some(Pick::Item(ship, item)),
        Target::Loop(_) => None,
    }
}

/// What a click at `at` (px on the view) picks among the labels placed
/// and the ships' and markers' anchors: a label, for what it names; else
/// the nearest ship or marker within [`POINT_PX`].
pub fn marked(at: Vec2, placed: &[Placed], anchors: &[Anchor]) -> Option<Pick> {
    let inside = |p: &&Placed| {
        let r = p.rect;
        at.x >= r.x && at.x <= r.x + r.w && at.y >= r.y && at.y <= r.y + r.h
    };
    if let Some(p) = placed.iter().filter(inside).find_map(|p| of(p.target)) {
        return Some(p);
    }
    let nearest = anchors
        .iter()
        .filter(|a| matches!(a.target, Target::Ship(_) | Target::Marker { .. }))
        .map(|a| (a.at.distance(at), a))
        .filter(|(d, _)| *d <= POINT_PX)
        .min_by(|a, b| a.0.total_cmp(&b.0));
    nearest.and_then(|(_, a)| of(a.target))
}

/// The body a click at `at` picks: the nearest to the eye whose disk is
/// under it, or whose point is within [`POINT_PX`].
pub fn body(at: Vec2, anchors: &[Anchor]) -> Option<Pick> {
    anchors
        .iter()
        .filter(|a| {
            matches!(a.target, Target::Body(_)) && a.at.distance(at) <= a.radius.max(POINT_PX)
        })
        .min_by(|a, b| a.depth.total_cmp(&b.depth))
        .and_then(|a| of(a.target))
}

/// The ship and item whose line passes nearest a click at `at` (px on
/// `view`), within [`LINE_PX`], where `balls` do not hide it: `lines` are
/// the items' (ship and item, by index), placed by `poses`, as drawn at
/// `now` (s on the paths' time axis).
pub fn line(
    at: Vec2,
    view: &View,
    lines: &[((usize, usize), &render::Path)],
    poses: &[render::Pose],
    now: f64,
    balls: &[Ball],
) -> Option<(usize, usize)> {
    let scale = f64::from(view.scale);
    let at = at.as_dvec2() * scale;
    lines
        .par_iter()
        .filter_map(|&(owner, path)| {
            let pose = poses.get(path.pose)?;
            let shown = |p| !hidden(view.camera.eye, p, balls, None);
            let d = path.near(
                pose,
                &view.camera,
                view.size,
                now,
                at,
                LINE_PX * scale,
                shown,
            )?;
            Some((d, owner))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, owner)| owner)
}
