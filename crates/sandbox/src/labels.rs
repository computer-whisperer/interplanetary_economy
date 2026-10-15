//! Labels on the map view (M1c.md, step 5 plan, item 1): bodies, ships,
//! markers and station loops, projected in f64 through the scene's
//! camera, left out where hidden, and decluttered greedily by priority.
//! Picking (`pick`) reads the same projections.
//!
//! Positions on the view are logical pixels from its top-left corner,
//! y down.

use crate::paths::Event;
use damascene_core::prelude::*;
use glam::{DVec2, DVec3, Vec2};
use orbit::ephem::{BodyId, BodyKind, Ephemeris};
use orbit::mission::{Marker, MarkerKind};
use orbit::time::Epoch;

/// Labels' text size, logical pixels (the caption's).
pub const TEXT_PX: f32 = tokens::TEXT_XS.size;

/// A label's backing reaches this far past its text, px.
pub const PAD: (f32, f32) = (4.0, 1.0);

/// Between a label and what it names, px.
const GAP_PX: f32 = 3.0;

/// What is kept clear around a placed label, px.
const MARGIN_PX: f32 = 1.0;

/// An anchor's box, kept clear of labels: its radius, within these, px.
const ANCHOR_PX: (f32, f32) = (2.0, 5.0);

/// A body whose disk is taller than this share of the view has no label.
const MAX_DISK_SHARE: f32 = 0.25;

/// Anchors this close name one spot, px: of their labels, only the first
/// placed is kept (a moon beside its planet from afar). A body drawn
/// smaller than this names the spot it is drawn at, to this far past its
/// radius.
const SAME_SPOT_PX: f32 = 6.0;

/// A body under this radius, px, is drawn as a point of light (render's
/// `Scene`), which hides nothing.
const POINT_BODY_PX: f64 = 1.0;

/// Labels' colors (sRGB): bodies, ships and station loops.
const BODY_COLOR: [u8; 3] = [214, 218, 226];
const SHIP_COLOR: [u8; 3] = [245, 245, 245];
const LOOP_COLOR: [u8; 3] = [120, 124, 132];

/// What a label names, and what a pick finds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Body(BodyId),
    Ship(usize),
    /// A marker: its ship, the ship's item, and its place among the item's
    /// markers.
    Marker {
        ship: usize,
        item: usize,
        marker: usize,
    },
    /// A station loop, by its place among the loops.
    Loop(usize),
}

/// Labels' priorities, the first placed first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    /// The selected item's markers.
    Selected,
    Ship,
    Focus,
    /// The Sun and the planets.
    Planet,
    Moon,
    /// Markers ahead of the clock.
    Ahead,
    SmallBody,
    /// Markers behind the clock.
    Behind,
    Loop,
}

/// A label, before it is placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub text: String,
    /// sRGB.
    pub color: [u8; 3],
    pub rank: Rank,
}

/// Something drawn on the map view, as projected.
#[derive(Clone, Debug, PartialEq)]
pub struct Anchor {
    pub target: Target,
    /// Its center, px. A body's may lie off the view while its disk is on
    /// it.
    pub at: Vec2,
    /// Its disk's or marker's radius, px.
    pub radius: f32,
    /// From the eye, km.
    pub depth: f64,
    pub label: Option<Label>,
}

/// A label placed on the view.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub target: Target,
    pub text: String,
    pub color: [u8; 3],
    /// Its backing's rect, px.
    pub rect: Rect,
    /// What it names, where it is.
    pub anchor: Vec2,
}

/// The map view as the scene's camera sees it: a target of `size`
/// physical pixels, `scale` of them to a logical one.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub camera: render::Camera,
    pub size: (u32, u32),
    pub scale: f32,
}

impl View {
    /// The view's size, px.
    pub fn extent(&self) -> Vec2 {
        Vec2::new(self.size.0 as f32, self.size.1 as f32) / self.scale
    }

    /// Where `p` (km, in the scene) lands on the view, px; `None` behind
    /// the eye.
    pub fn project(&self, p: DVec3) -> Option<DVec2> {
        self.camera
            .project(p, self.size)
            .map(|q| q / f64::from(self.scale))
    }

    /// Px per radian at the view's center.
    fn px_per_radian(&self) -> f64 {
        1.0 / (self.camera.pixel_angle(self.size.1) * f64::from(self.scale))
    }

    /// Whether `q` (px) is on the view, with `margin` px to spare.
    fn shows(&self, q: DVec2, margin: f64) -> bool {
        let e = self.extent().as_dvec2();
        q.x >= -margin && q.y >= -margin && q.x <= e.x + margin && q.y <= e.y + margin
    }
}

/// A sphere that hides what lies behind it.
#[derive(Clone, Copy, Debug)]
pub struct Ball {
    pub center: DVec3,
    pub radius: f64,
}

/// Whether the sight line from `eye` to `p` enters one of `balls`, other
/// than `own`, before it reaches `p`.
pub fn hidden(eye: DVec3, p: DVec3, balls: &[Ball], own: Option<usize>) -> bool {
    let d = p - eye;
    let length = d.length();
    if length == 0.0 {
        return false;
    }
    let u = d / length;
    balls.iter().enumerate().any(|(i, b)| {
        if Some(i) == own {
            return false;
        }
        let to = b.center - eye;
        let along = to.dot(u);
        let miss2 = to.length_squared() - along * along;
        let r2 = b.radius * b.radius;
        if miss2 >= r2 {
            return false;
        }
        let enter = along - (r2 - miss2).sqrt();
        enter > 0.0 && enter < length * (1.0 - 1e-9)
    })
}

/// sRGB bytes of a linear color.
pub fn srgb(c: [f32; 3]) -> [u8; 3] {
    c.map(|x| {
        let x = x.clamp(0.0, 1.0);
        let v = if x <= 0.003_130_8 {
            12.92 * x
        } else {
            1.055 * x.powf(1.0 / 2.4) - 0.055
        };
        (v * 255.0).round() as u8
    })
}

/// km/s as m/s with thousands separated: "1,205 m/s".
pub fn mps(dv: f64) -> String {
    format!("{} m/s", grouped(dv * 1e3))
}

/// `x` rounded, its thousands separated: "1,205".
pub fn grouped(x: f64) -> String {
    let n = x.round() as i64;
    let digits = n.abs().to_string();
    let mut out = String::from(if n < 0 { "−" } else { "" });
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A body's name as its label gives it: a numbered small body's proper
/// name ("25143 Itokawa (1998 SF36)": "Itokawa"), else its provisional
/// designation ("175706 (1996 FG3)": "1996 FG3").
pub fn short_name(name: &str) -> &str {
    let Some((head, designation)) = name.split_once('(') else {
        return name;
    };
    let head = head.trim();
    let proper = match head.split_once(' ') {
        Some((number, rest)) if number.chars().all(|c| c.is_ascii_digit()) => rest,
        _ if head.chars().all(|c| c.is_ascii_digit()) => "",
        _ => head,
    };
    if proper.is_empty() {
        designation.trim_end_matches(')')
    } else {
        proper
    }
}

/// What a marker's label says.
pub fn marker_text(eph: &Ephemeris, m: &Marker) -> String {
    let at = || {
        m.pass
            .map(|(b, km)| format!("{} {km:.0} km", eph.body(b).name))
            .unwrap_or_default()
    };
    match m.kind {
        MarkerKind::Burn => format!("Δv {}", mps(m.dv)),
        MarkerKind::Flyby if m.dv * 1e3 >= 0.5 => format!("flyby {}, Δv {}", at(), mps(m.dv)),
        MarkerKind::Flyby => format!("flyby {}", at()),
        MarkerKind::Pass => format!("pass {}", at()),
        MarkerKind::Landing => "landing".into(),
        MarkerKind::Ascent => "ascent".into(),
        MarkerKind::Jump { km, plane_deg } => format!("jump {km:.0} km, {plane_deg:.1}°"),
    }
}

/// The bodies covering `t` drawn as disks, as balls (those drawn as
/// points hide nothing), and the anchors of those whose center is not
/// hidden and whose disk is on the view. A body's label is left out where
/// its disk is large.
pub fn bodies(
    eph: &Ephemeris,
    t: Epoch,
    bodies: &[BodyId],
    focus: BodyId,
    view: &View,
) -> (Vec<Ball>, Vec<Anchor>) {
    let eye = view.camera.eye;
    let seen: Vec<(BodyId, Ball, f64)> = bodies
        .iter()
        .copied()
        .filter(|&b| eph.covers(b, t))
        .map(|b| {
            let ball = Ball {
                center: eph.barycentric_position(b, t),
                radius: eph.body(b).radius,
            };
            let radius =
                (ball.radius / (ball.center - eye).length()).min(1.0).asin() * view.px_per_radian();
            (b, ball, radius)
        })
        .collect();
    let balls: Vec<Ball> = seen
        .iter()
        .filter(|(_, _, radius)| *radius >= POINT_BODY_PX)
        .map(|(_, ball, _)| *ball)
        .collect();
    let max_radius = MAX_DISK_SHARE * view.extent().y / 2.0;
    let anchors = seen
        .iter()
        .filter_map(|&(b, ball, radius)| {
            let at = view.project(ball.center)?;
            let own = balls.iter().position(|o| o.center == ball.center);
            if !view.shows(at, radius) || hidden(eye, ball.center, &balls, own) {
                return None;
            }
            let body = eph.body(b);
            let rank = match body.kind {
                _ if b == focus => Rank::Focus,
                BodyKind::Star | BodyKind::Planet => Rank::Planet,
                BodyKind::Moon => Rank::Moon,
                BodyKind::SmallBody { .. } => Rank::SmallBody,
            };
            let radius = radius as f32;
            Some(Anchor {
                target: Target::Body(b),
                at: at.as_vec2(),
                radius,
                depth: (ball.center - eye).length(),
                label: (radius <= max_radius).then(|| Label {
                    text: short_name(&body.name).to_string(),
                    color: BODY_COLOR,
                    rank,
                }),
            })
        })
        .collect();
    (balls, anchors)
}

/// An anchor for a point at `p` (km, in the scene) drawn `radius` px
/// across, if it is on the view and not hidden.
fn point(
    view: &View,
    balls: &[Ball],
    target: Target,
    p: DVec3,
    radius: f32,
    label: Label,
) -> Option<Anchor> {
    let at = view.project(p)?;
    if !view.shows(at, 0.0) || hidden(view.camera.eye, p, balls, None) {
        return None;
    }
    Some(Anchor {
        target,
        at: at.as_vec2(),
        radius,
        depth: (p - view.camera.eye).length(),
        label: Some(label),
    })
}

/// A ship's anchor: `p` where its marker is, `px` across.
pub fn ship(
    view: &View,
    balls: &[Ball],
    s: usize,
    name: &str,
    p: DVec3,
    px: f32,
) -> Option<Anchor> {
    let label = Label {
        text: name.to_string(),
        color: SHIP_COLOR,
        rank: Rank::Ship,
    };
    point(view, balls, Target::Ship(s), p, px / 2.0, label)
}

/// A marker's anchor, the event drawn where `view_pose` places it; its
/// label from `marker`, first if its item is `selected`.
#[allow(clippy::too_many_arguments)]
pub fn marker(
    eph: &Ephemeris,
    view: &View,
    balls: &[Ball],
    e: &Event,
    marker: &Marker,
    view_pose: &render::Pose,
    t: Epoch,
    selected: Option<(usize, usize)>,
) -> Option<Anchor> {
    let past = e.t.0 < t.0;
    let rank = if selected == Some((e.ship, e.item)) {
        Rank::Selected
    } else if past {
        Rank::Behind
    } else {
        Rank::Ahead
    };
    let color = if past {
        e.color.map(|c| c * crate::paths::PAST_DIM)
    } else {
        e.color
    };
    let label = Label {
        text: marker_text(eph, marker),
        color: srgb(color.map(|c| c.max(0.25))),
        rank,
    };
    let target = Target::Marker {
        ship: e.ship,
        item: e.item,
        marker: e.marker,
    };
    point(
        view,
        balls,
        target,
        view_pose.to_world(e.position),
        e.size_px / 2.0,
        label,
    )
}

/// A station loop's anchor, named `name`, at `p`.
pub fn station(view: &View, balls: &[Ball], i: usize, name: &str, p: DVec3) -> Option<Anchor> {
    let label = Label {
        text: name.to_uppercase(),
        color: LOOP_COLOR,
        rank: Rank::Loop,
    };
    point(view, balls, Target::Loop(i), p, 0.0, label)
}

/// A label's backing's size for text measured `text` px.
pub fn backing(text: Vec2) -> Vec2 {
    text + 2.0 * Vec2::new(PAD.0, PAD.1)
}

/// The size of `text`'s backing, measured as damascene draws it.
pub fn measure(text: &str) -> Vec2 {
    let m = measure_text(
        text,
        TEXT_PX,
        FontWeight::Regular,
        false,
        TextWrap::NoWrap,
        None,
    );
    backing(Vec2::new(m.width, m.height))
}

/// An anchor's box, kept clear of other labels.
fn anchor_box(a: &Anchor) -> Rect {
    let r = a.radius.clamp(ANCHOR_PX.0, ANCHOR_PX.1);
    Rect::new(a.at.x - r, a.at.y - r, 2.0 * r, 2.0 * r)
}

fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

/// How far an anchor's spot reaches, px: a body drawn small names the
/// spot it is drawn at, to [`SAME_SPOT_PX`] past its radius.
fn reach(a: &Anchor) -> f32 {
    if matches!(a.target, Target::Body(_)) && a.radius < SAME_SPOT_PX {
        a.radius + SAME_SPOT_PX
    } else {
        SAME_SPOT_PX
    }
}

/// Places the anchors' labels on a view `extent` px, `size` giving a
/// label's backing's size: by rank, then nearer the eye first, each
/// right of, left of, above or below its anchor, at the first place on
/// the view that overlaps no label placed and no anchor's box (other than
/// those at its spot); failing that, at the first that overlaps only
/// boxes of anchors ranked lower; else it is left out.
///
/// A label is also left out where its anchor shares a spot: where it is
/// within [`SAME_SPOT_PX`] of a placed label's anchor, or (unless it
/// names a body) where a body is drawn small (a ship resting at
/// an asteroid, or in low orbit seen from afar): the body's label names
/// the spot.
pub fn declutter(
    anchors: &[Anchor],
    extent: Vec2,
    mut size: impl FnMut(&str) -> Vec2,
) -> Vec<Placed> {
    let view = Rect::new(0.0, 0.0, extent.x, extent.y);
    let rank = |a: &Anchor| a.label.as_ref().map_or(Rank::Selected, |l| l.rank);
    let shown: Vec<&Anchor> = anchors
        .iter()
        .filter(|a| a.at.x >= 0.0 && a.at.y >= 0.0 && a.at.x <= extent.x && a.at.y <= extent.y)
        .collect();
    let small_bodies: Vec<&Anchor> = shown
        .iter()
        .copied()
        .filter(|a| matches!(a.target, Target::Body(_)) && a.radius < SAME_SPOT_PX)
        .collect();
    let mut order: Vec<&Anchor> = anchors
        .iter()
        .filter(|a| a.label.is_some())
        .filter(|a| {
            matches!(a.target, Target::Body(_))
                || !small_bodies.iter().any(|b| b.at.distance(a.at) < reach(b))
        })
        .collect();
    order.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.depth.total_cmp(&b.depth)));
    let mut placed: Vec<Placed> = Vec::new();
    let mut spots_taken: Vec<Vec2> = Vec::new();
    for a in order {
        let Some(label) = &a.label else { continue };
        if spots_taken.iter().any(|&p| p.distance(a.at) < SAME_SPOT_PX) {
            continue;
        }
        let s = size(&label.text);
        let off = a.radius + GAP_PX;
        let (x, y) = (a.at.x, a.at.y);
        let spots = [
            (x + off, y - s.y / 2.0),
            (x - off - s.x, y - s.y / 2.0),
            (x - s.x / 2.0, y - off - s.y),
            (x - s.x / 2.0, y + off),
        ];
        let others: Vec<&Anchor> = shown
            .iter()
            .copied()
            .filter(|o| o.at.distance(a.at) >= reach(a))
            .collect();
        let fits = |r: &Rect, lower_too: bool| {
            let grown = Rect::new(
                r.x - MARGIN_PX,
                r.y - MARGIN_PX,
                r.w + 2.0 * MARGIN_PX,
                r.h + 2.0 * MARGIN_PX,
            );
            r.x >= view.x
                && r.y >= view.y
                && r.x + r.w <= view.w
                && r.y + r.h <= view.h
                && !placed.iter().any(|p| overlaps(&grown, &p.rect))
                && !others
                    .iter()
                    .any(|o| (lower_too || rank(o) <= label.rank) && overlaps(r, &anchor_box(o)))
        };
        let rects = spots.map(|(x, y)| Rect::new(x, y, s.x, s.y));
        if let Some(rect) = [true, false]
            .into_iter()
            .find_map(|lower_too| rects.into_iter().find(|r| fits(r, lower_too)))
        {
            placed.push(Placed {
                target: a.target,
                text: label.text.clone(),
                color: label.color,
                rect,
                anchor: a.at,
            });
            spots_taken.push(a.at);
        }
    }
    placed
}

/// The placed labels, drawn over the map view where they were placed on
/// a view `extent` px. Where the map view is another size (in the frame
/// it changes size, which still shows the scene drawn at the old one,
/// stretched), their anchors stretch with it, each label keeping its
/// offset from its anchor.
pub fn layer(placed: &[Placed], extent: Vec2) -> El {
    let rects: Vec<(Rect, Vec2)> = placed.iter().map(|p| (p.rect, p.anchor)).collect();
    stack(placed.iter().map(|p| {
        let [r, g, b] = p.color;
        text(p.text.clone())
            .font_size(TEXT_PX)
            .nowrap_text()
            .text_color(Color::srgb_u8(r, g, b))
            .fill(tokens::BACKGROUND.with_alpha_u8(170))
            .radius(3.0)
            .padding(Sides::xy(PAD.0, PAD.1))
            .width(Size::Fixed(p.rect.w))
            .height(Size::Fixed(p.rect.h))
    }))
    .width(Size::Fill(1.0))
    .height(Size::Fill(1.0))
    .layout(move |ctx| {
        let c = ctx.container;
        let (kx, ky) = (c.w / extent.x, c.h / extent.y);
        rects
            .iter()
            .map(|(r, a)| {
                let (x, y) = (a.x * kx + r.x - a.x, a.y * ky + r.y - a.y);
                Rect::new(c.x + x, c.y + y, r.w, r.h)
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(target: Target, at: (f32, f32), rank: Rank, depth: f64) -> Anchor {
        Anchor {
            target,
            at: Vec2::new(at.0, at.1),
            radius: 2.0,
            depth,
            label: Some(Label {
                text: "Label".into(),
                color: [255; 3],
                rank,
            }),
        }
    }

    fn size(_: &str) -> Vec2 {
        Vec2::new(40.0, 14.0)
    }

    /// Placed labels overlap neither each other nor any anchor's box, and
    /// stay on the view; the same anchors place the same labels.
    #[test]
    fn labels_keep_apart() {
        let anchors: Vec<Anchor> = (0..400)
            .map(|i| {
                let a = i as f32 * 2.399;
                let r = (i as f32 / 400.0).sqrt() * 300.0;
                anchor(
                    Target::Ship(i),
                    (400.0 + r * a.cos(), 300.0 + r * a.sin()),
                    Rank::Ship,
                    i as f64,
                )
            })
            .collect();
        let extent = Vec2::new(800.0, 600.0);
        let placed = declutter(&anchors, extent, size);
        println!("{} of {} placed", placed.len(), anchors.len());
        assert!(placed.len() > 20 && placed.len() < anchors.len());
        for (i, p) in placed.iter().enumerate() {
            assert!(p.rect.x >= 0.0 && p.rect.y >= 0.0);
            assert!(p.rect.x + p.rect.w <= extent.x && p.rect.y + p.rect.h <= extent.y);
            for q in &placed[i + 1..] {
                assert!(!overlaps(&p.rect, &q.rect), "{p:?} {q:?}");
            }
            for a in &anchors {
                assert!(!overlaps(&p.rect, &anchor_box(a)), "{p:?} over {a:?}");
            }
        }
        assert_eq!(placed, declutter(&anchors, extent, size));
    }

    /// Of two labels in conflict, the higher rank stays, whatever the
    /// order given; within a rank, the nearer.
    #[test]
    fn rank_then_nearness_wins() {
        // Hemmed in: the only room is to the right, which both want.
        let wall = |x: f32| -> Vec<Anchor> {
            (0..12)
                .flat_map(|k| {
                    let y = k as f32 * 9.0;
                    [(x - 9.0, y), (x + 60.0, y)]
                })
                .map(|at| Anchor {
                    label: None,
                    ..anchor(Target::Loop(0), at, Rank::Loop, 0.0)
                })
                .collect()
        };
        let a = anchor(Target::Ship(1), (100.0, 50.0), Rank::Moon, 5.0);
        let b = anchor(Target::Ship(2), (100.0, 50.0), Rank::Planet, 9.0);
        let c = anchor(Target::Ship(3), (100.0, 50.0), Rank::Planet, 7.0);
        for anchors in [
            [a.clone(), b.clone(), c.clone()],
            [c.clone(), b.clone(), a.clone()],
        ] {
            let mut all = wall(100.0);
            all.extend(anchors);
            let placed = declutter(&all, Vec2::new(400.0, 108.0), size);
            let names: Vec<Target> = placed.iter().map(|p| p.target).collect();
            assert_eq!(names, vec![Target::Ship(3)], "{placed:?}");
        }
    }

    /// The sight line to a point behind a sphere is hidden; beside it, in
    /// front of it, or on its near surface, not.
    #[test]
    fn spheres_hide_what_is_behind() {
        let eye = DVec3::ZERO;
        let balls = [Ball {
            center: DVec3::new(0.0, 0.0, -100.0),
            radius: 10.0,
        }];
        assert!(hidden(eye, DVec3::new(0.0, 0.0, -200.0), &balls, None));
        assert!(!hidden(eye, DVec3::new(0.0, 0.0, -200.0), &balls, Some(0)));
        assert!(!hidden(eye, DVec3::new(30.0, 0.0, -200.0), &balls, None));
        assert!(!hidden(eye, DVec3::new(0.0, 0.0, -50.0), &balls, None));
        assert!(!hidden(eye, DVec3::new(0.0, 0.0, -90.0), &balls, None));
    }

    #[test]
    fn small_bodies_go_by_short_names() {
        assert_eq!(short_name("25143 Itokawa (1998 SF36)"), "Itokawa");
        assert_eq!(short_name("175706 (1996 FG3)"), "1996 FG3");
        assert_eq!(short_name("(2009 UY19)"), "2009 UY19");
        assert_eq!(short_name("Phobos"), "Phobos");
    }

    /// A label whose anchor is at a placed label's spot is left out.
    #[test]
    fn one_label_to_a_spot() {
        let anchors = [
            anchor(Target::Ship(1), (100.0, 50.0), Rank::Planet, 9.0),
            anchor(Target::Ship(2), (103.0, 52.0), Rank::Moon, 5.0),
            anchor(Target::Ship(3), (110.0, 50.0), Rank::Moon, 5.0),
        ];
        let placed = declutter(&anchors, Vec2::new(400.0, 200.0), size);
        let names: Vec<Target> = placed.iter().map(|p| p.target).collect();
        assert_eq!(names, vec![Target::Ship(1), Target::Ship(3)]);
    }

    /// A body drawn small names its spot: a ship and a marker there (a
    /// low orbit seen from afar) are not labeled, nor do their boxes keep
    /// the body's label out (they once took all four places around it).
    /// A ship farther off is labeled.
    #[test]
    fn a_small_body_names_its_spot() {
        let earth = Ephemeris::standard().lookup("earth").unwrap();
        let mut body = anchor(Target::Body(earth), (100.0, 50.0), Rank::Planet, 9.0);
        body.radius = 1.5;
        let anchors = [
            anchor(Target::Ship(0), (103.0, 51.0), Rank::Ship, 9.0),
            body,
            anchor(Target::Ship(1), (98.6, 52.0), Rank::Ahead, 9.0),
            anchor(Target::Ship(2), (101.4, 48.0), Rank::Ahead, 9.0),
            anchor(Target::Ship(3), (160.0, 50.0), Rank::Ship, 9.0),
        ];
        let placed = declutter(&anchors, Vec2::new(400.0, 200.0), size);
        let names: Vec<Target> = placed.iter().map(|p| p.target).collect();
        assert_eq!(names, vec![Target::Ship(3), Target::Body(earth)]);
    }

    /// Hemmed in on all four sides by lower-ranked anchors' boxes, a
    /// label is placed over one; hemmed in by higher-ranked ones, it is
    /// not. Where a side is clear of every box, it goes there.
    #[test]
    fn lower_anchors_give_way() {
        let around = |rank: Rank| -> Vec<Anchor> {
            [(9.0, 0.0), (-9.0, 0.0), (0.0, -9.0), (0.0, 9.0)]
                .into_iter()
                .map(|(dx, dy)| Anchor {
                    label: Some(Label {
                        text: String::new(),
                        color: [0; 3],
                        rank,
                    }),
                    ..anchor(Target::Loop(0), (100.0 + dx, 50.0 + dy), rank, 0.0)
                })
                .collect()
        };
        let focus = anchor(Target::Ship(1), (100.0, 50.0), Rank::Focus, 1.0);
        let placed_for = |rank: Rank| {
            let mut all = around(rank);
            all.push(focus.clone());
            // Their own labels never fit.
            declutter(&all, Vec2::new(400.0, 200.0), |t| {
                if t.is_empty() {
                    Vec2::splat(1e4)
                } else {
                    size(t)
                }
            })
            .into_iter()
            .any(|p| p.target == Target::Ship(1))
        };
        assert!(placed_for(Rank::Ahead));
        assert!(!placed_for(Rank::Ship));
        // One side clear: taken, over no box.
        let mut all = around(Rank::Ahead);
        all.remove(1);
        all.push(focus.clone());
        let placed = declutter(&all, Vec2::new(400.0, 200.0), size);
        let label = placed.iter().find(|p| p.target == Target::Ship(1)).unwrap();
        assert!(label.rect.x + label.rect.w < 100.0, "{label:?}");
    }

    #[test]
    fn speeds_read_in_meters() {
        assert_eq!(mps(1.2054), "1,205 m/s");
        assert_eq!(mps(3.5761), "3,576 m/s");
        assert_eq!(mps(0.684), "684 m/s");
        assert_eq!(mps(12.3456), "12,346 m/s");
    }
}
