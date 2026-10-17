//! The timeline (M1c.md, step 7 plan, item 1): under the map and the
//! panel, a lane per ship with its items as bars on a time axis of its
//! own, a cursor at the clock's time, and an axis whose row seeks the
//! clock.

use crate::panel;
use crate::paths;
use crate::plan::{Ship, event_time};
use crate::windows::{self, Windows};
use damascene_core::prelude::*;
use orbit::ephem::Ephemeris;
use orbit::mission::ItemKind;
use orbit::time::Epoch;

/// The axis's row: a press or a drag on it seeks the clock.
pub const AXIS_KEY: &str = "timeline:axis";
/// The lanes, and the gutter of the ships' names beside them.
pub const LANES_KEY: &str = "timeline:lanes";
pub const GUTTER_KEY: &str = "timeline:gutter";
/// The lanes' and gutter's scrolling.
const SCROLL_KEY: &str = "timeline:scroll";

/// The shortest span shown, days: an hour.
const SHORTEST_DAYS: f64 = 1.0 / 24.0;
/// The span a plan is fitted to at least, days.
const FIT_DAYS: f64 = 10.0;
/// The ships' names' gutter's width, logical px.
const GUTTER_PX: f32 = 170.0;
const AXIS_PX: f32 = 22.0;
const LANE_PX: f32 = 18.0;
/// A window lane's height.
const WINDOW_PX: f32 = 12.0;
/// Bars' heights: a leg's or an event's, a stay's; and their least width.
const LEG_PX: f32 = 10.0;
const STAY_PX: f32 = 4.0;
const MIN_BAR_PX: f32 = 2.0;
/// The width of a marker's tick and of the cursor.
const TICK_PX: f32 = 2.0;
/// Lanes shown before they scroll.
const LANES_SHOWN: usize = 8;
/// A wheel line zooms the span by this factor.
const WHEEL_ZOOM: f64 = 1.25;
/// A press that moves less than this, px, is a click, not a pan.
const CLICK_PX: f32 = 3.0;
/// When the clock crosses out of the span shown, the span pages so that
/// the clock is this share of it in from the edge it crossed.
const PAGE_MARGIN: f64 = 0.05;
/// A plan is fitted with this share of the span spare at either end.
const FIT_MARGIN: f64 = 0.02;
const CURSOR: Color = Color::srgb_u8(230, 230, 230);
const SELECTED: Color = Color::srgb_u8(255, 255, 255);

/// A leg's bar that a drag moves (the selected leg between planets), and
/// how far it is dragged, days.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Movable {
    pub item: (usize, usize),
    pub shift_days: f64,
}

/// A bar's key: its ship and item.
pub fn bar_key(s: usize, k: usize) -> String {
    format!("bar:{s}:{k}")
}

/// The ship and item a bar's key names.
pub fn parse_bar_key(key: &str) -> Option<(usize, usize)> {
    let (s, k) = key.strip_prefix("bar:")?.split_once(':')?;
    Some((s.parse().ok()?, k.parse().ok()?))
}

/// The span of time the timeline shows, and a press on its lanes.
#[derive(Clone, Copy, Debug)]
pub struct Timeline {
    pub span: (Epoch, Epoch),
    /// The span it is held within: the ephemeris's.
    pub limits: (Epoch, Epoch),
    /// Whether the span follows the plan's as it is designed: until it
    /// is zoomed, panned or paged.
    pub fitted: bool,
    press: Option<Press>,
}

/// A press on the lanes: where the pointer went down, the span then, and
/// whether it has moved far enough to be no click.
#[derive(Clone, Copy, Debug)]
struct Press {
    at: (f32, f32),
    span: (Epoch, Epoch),
    moved: bool,
}

impl Timeline {
    /// A year about `t`, within `limits`, following the plan.
    pub fn new(t: Epoch, limits: (Epoch, Epoch)) -> Timeline {
        let mut tl = Timeline {
            span: (t, t),
            limits,
            fitted: true,
            press: None,
        };
        tl.show(t.0 - 182.625, 365.25);
        tl
    }

    /// The span's length, days.
    pub fn days(&self) -> f64 {
        self.span.1.0 - self.span.0.0
    }

    /// Where `t` falls across the span: 0 at its start, 1 at its end.
    pub fn fraction(&self, t: Epoch) -> f64 {
        (t.0 - self.span.0.0) / self.days()
    }

    /// The moment at `f` across the span.
    pub fn at(&self, f: f64) -> Epoch {
        Epoch(self.span.0.0 + f * self.days())
    }

    /// Shows `days` from `from`: at least an hour, at most the limits,
    /// and within them.
    fn show(&mut self, from: f64, days: f64) {
        let (lo, hi) = (self.limits.0.0, self.limits.1.0);
        let days = days.clamp(SHORTEST_DAYS, hi - lo);
        let from = from.clamp(lo, hi - days);
        self.span = (Epoch(from), Epoch(from + days));
    }

    /// Zooms out by `factor` (in, under 1) about the moment at `f`
    /// across the span, which stays where it is.
    pub fn zoom(&mut self, f: f64, factor: f64) {
        let at = self.at(f).0;
        let days = self.days() * factor;
        self.show(
            at - f * days.clamp(SHORTEST_DAYS, self.limits.1.0 - self.limits.0.0),
            days,
        );
        self.fitted = false;
    }

    /// Zooms for `lines` of the wheel (up, positive, zooms in) about the
    /// pointer at `f` across the span.
    pub fn wheel(&mut self, f: f64, lines: f32) {
        self.zoom(f, WHEEL_ZOOM.powf(-f64::from(lines)));
    }

    /// Shows a plan's `span`, if the timeline follows the plan.
    pub fn fit(&mut self, span: (Epoch, Epoch)) {
        if !self.fitted {
            return;
        }
        let days = (span.1.0 - span.0.0).max(FIT_DAYS);
        self.show(
            span.0.0 - FIT_MARGIN * days,
            days * (1.0 + 2.0 * FIT_MARGIN),
        );
    }

    /// Pages the span to the clock, now at `t`, where it has crossed out of
    /// it since it was at `before`: not where the span was moved off it.
    pub fn follow(&mut self, before: Epoch, t: Epoch) {
        let inside = |t: Epoch| t.0 >= self.span.0.0 && t.0 <= self.span.1.0;
        if inside(t) || !inside(before) {
            return;
        }
        let days = self.days();
        if t.0 > self.span.1.0 {
            self.show(t.0 - PAGE_MARGIN * days, days);
        } else if t.0 < self.span.0.0 {
            self.show(t.0 - (1.0 - PAGE_MARGIN) * days, days);
        } else {
            return;
        }
        self.fitted = false;
    }

    /// A press on the lanes at `at`.
    pub fn press(&mut self, at: (f32, f32)) {
        self.press = Some(Press {
            at,
            span: self.span,
            moved: false,
        });
    }

    /// The pointer pressed on the lanes is at `at`, on lanes `width` px
    /// wide: past a click's slop, the span pans with it across.
    pub fn drag(&mut self, at: (f32, f32), width: f32) {
        let Some(mut p) = self.press else {
            return;
        };
        p.moved |= (at.0 - p.at.0).hypot(at.1 - p.at.1) >= CLICK_PX;
        self.press = Some(p);
        if p.moved && width > 0.0 {
            let days = p.span.1.0 - p.span.0.0;
            let shift = f64::from(p.at.0 - at.0) / f64::from(width) * days;
            self.show(p.span.0.0 + shift, days);
            self.fitted = false;
        }
    }

    /// The press ends: whether it was a click (not a pan).
    pub fn release(&mut self) -> bool {
        self.press.take().is_some_and(|p| !p.moved)
    }

    /// Follows the plan again.
    pub fn refit(&mut self) {
        self.fitted = true;
    }
}

/// A calendar step between ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Hours(u32),
    Days(u32),
    Months(u32),
    Years(i32),
}

/// The ladder of steps, finest first.
const STEPS: [Step; 15] = [
    Step::Hours(1),
    Step::Hours(3),
    Step::Hours(6),
    Step::Hours(12),
    Step::Days(1),
    Step::Days(2),
    Step::Days(5),
    Step::Days(10),
    Step::Months(1),
    Step::Months(3),
    Step::Months(6),
    Step::Years(1),
    Step::Years(2),
    Step::Years(5),
    Step::Years(10),
];

impl Step {
    /// About how long it is, days.
    fn days(self) -> f64 {
        match self {
            Step::Hours(h) => f64::from(h) / 24.0,
            Step::Days(d) => f64::from(d),
            Step::Months(m) => f64::from(m) * 30.44,
            Step::Years(y) => f64::from(y) * 365.25,
        }
    }

    /// The room its labels need between ticks, px: their width at the
    /// caption's size with tabular numerals (`2045-03` 54.5 px,
    /// `2045-03-05` 77.8), 10 px about them, and for months, room in
    /// February (0.92 of a mean month).
    fn room(self) -> f32 {
        match self {
            Step::Years(_) => 46.0,
            Step::Months(_) => 72.0,
            Step::Days(_) => 90.0,
            Step::Hours(_) => 96.0,
        }
    }
}

/// A tick on the axis: its moment and its label.
#[derive(Clone, Debug, PartialEq)]
pub struct Tick {
    pub t: Epoch,
    pub label: String,
}

fn days_in_month(y: i32, m: u32) -> u32 {
    let next = if m == 12 {
        Epoch::from_ymd(y + 1, 1, 1)
    } else {
        Epoch::from_ymd(y, m + 1, 1)
    };
    (next.0 - Epoch::from_ymd(y, m, 1).0).round() as u32
}

/// The axis's ticks over `span` across `width` px: on the finest step of
/// the calendar whose labels have room.
pub fn ticks(span: (Epoch, Epoch), width: f32) -> Vec<Tick> {
    let days = span.1.0 - span.0.0;
    if days <= 0.0 || width <= 0.0 {
        return Vec::new();
    }
    let step = STEPS
        .into_iter()
        .find(|s| s.days() / days * f64::from(width) >= f64::from(s.room()))
        .unwrap_or(Step::Years(10));
    let (y0, m0, d0) = span.0.ymd();
    let mut out = Vec::new();
    let mut push = |t: Epoch, label: String| {
        if t.0 >= span.0.0 && t.0 <= span.1.0 {
            out.push(Tick { t, label });
        }
        t.0 <= span.1.0
    };
    match step {
        Step::Years(k) => {
            let mut y = y0.div_euclid(k) * k;
            while push(Epoch::from_ymd(y, 1, 1), format!("{y}")) {
                y += k;
            }
        }
        Step::Months(k) => {
            let mut i = (y0 * 12 + m0 as i32 - 1).div_euclid(k as i32) * k as i32;
            loop {
                let (y, m) = (i.div_euclid(12), i.rem_euclid(12) as u32 + 1);
                if !push(Epoch::from_ymd(y, m, 1), format!("{y}-{m:02}")) {
                    break;
                }
                i += k as i32;
            }
        }
        Step::Days(k) => {
            let (mut y, mut m) = (y0, m0);
            'months: loop {
                // Days 1, 1 + k, …: each month starts again at its first.
                let n = days_in_month(y, m);
                let mut d = 1;
                while d <= n && (k == 1 || d + k <= n + 1) {
                    if !push(Epoch::from_ymd(y, m, d), format!("{y}-{m:02}-{d:02}")) {
                        break 'months;
                    }
                    d += k;
                }
                (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            }
        }
        Step::Hours(k) => {
            let day = Epoch::from_ymd(y0, m0, d0.floor() as u32);
            let mut h = 0;
            loop {
                // The date from its midnight, exact in TDB days.
                let (_, m, d) = day.plus_days(f64::from(h / 24)).ymd();
                let label = format!("{m:02}-{:02} {:02}:00", d as u32, h % 24);
                if !push(day.plus_days(f64::from(h) / 24.0), label) {
                    break;
                }
                h += k;
            }
        }
    }
    out
}

/// A mark placed on a row: where it spans across the span (0 to 1, held
/// just past the ends), its top and height, and whether it is a bar (at
/// least [`MIN_BAR_PX`] wide, about its middle) or a tick.
#[derive(Clone, Copy, Debug)]
struct Place {
    from: f64,
    to: f64,
    top: f32,
    height: f32,
    bar: bool,
}

impl Place {
    fn rect(&self, c: Rect) -> Rect {
        let (x0, x1) = (c.x + self.from as f32 * c.w, c.x + self.to as f32 * c.w);
        let (x, w) = if !self.bar {
            (x0 - TICK_PX / 2.0, TICK_PX)
        } else if x1 - x0 < MIN_BAR_PX {
            ((x0 + x1 - MIN_BAR_PX) / 2.0, MIN_BAR_PX)
        } else {
            (x0, x1 - x0)
        };
        Rect::new(x, c.y + self.top, w, self.height)
    }
}

/// Elements placed on a row by their [`Place`]s.
fn placed(els: Vec<El>, places: Vec<Place>) -> El {
    stack(els).layout(move |ctx| places.iter().map(|p| p.rect(ctx.container)).collect())
}

/// `a` (linear sRGB) as a color.
fn color(a: [f32; 3]) -> Color {
    Color::srgb_linear(a[0], a[1], a[2], 1.0)
}

/// An item's tooltip: its kind, label and Δv, and its row's lines.
fn tooltip(eph: &Ephemeris, item: &orbit::mission::Item) -> String {
    let row = panel::item_row(eph, item);
    let mut tip = format!("{} {}", row.kind, row.label);
    if !row.dv.is_empty() {
        tip.push_str(&format!(" · {}", row.dv));
    }
    for (line, _) in &row.lines {
        tip.push('\n');
        tip.push_str(line);
    }
    tip
}

/// The timeline under the map: the axis (`axis_px` wide when last laid
/// out); below it the plan's window lanes, then a lane per ship
/// (scrolling past [`LANES_SHOWN`]), the item `selected` outlined, the
/// clock at `t`.
#[allow(clippy::too_many_arguments)]
pub fn strip(
    eph: &Ephemeris,
    tl: &Timeline,
    axis_px: f32,
    ships: &[Ship],
    windows: Option<&Windows>,
    t: Epoch,
    selected: Option<(usize, usize)>,
    movable: Option<Movable>,
) -> El {
    let held = |f: f64| f.clamp(-0.01, 1.01);
    let cursor = tl.fraction(t);
    let shown = (0.0..=1.0).contains(&cursor);
    let mut places = Vec::new();
    let mut els = Vec::new();
    for (s, ship) in ships.iter().enumerate() {
        let colors = paths::colors(ship);
        let lane = s as f32 * LANE_PX;
        for (k, item) in ship.items.iter().enumerate() {
            let moved = movable.filter(|m| m.item == (s, k));
            let shift = moved.map_or(0.0, |m| m.shift_days);
            let (from, to) = (
                tl.fraction(item.start.plus_days(shift)),
                tl.fraction(item.end.plus_days(shift)),
            );
            if to < 0.0 || from > 1.0 {
                continue;
            }
            let (height, c) = match item.kind {
                ItemKind::Stay | ItemKind::Capture => (STAY_PX, colors[k]),
                ItemKind::Failed(_) => (LEG_PX, paths::FAILURE_COLOR),
                _ => (LEG_PX, colors[k]),
            };
            let mut tip = tooltip(eph, item);
            if moved.is_some() {
                tip.push_str(
                    "\nDrag to move its departure: it lands on the cheapest leg leaving then.",
                );
            }
            let mut el = spacer()
                .fill(color(c))
                .radius(2.0)
                .key(bar_key(s, k))
                .tooltip(tip);
            if moved.is_some() {
                el = el.cursor(Cursor::Grab);
            }
            if selected == Some((s, k)) {
                el = el.stroke(SELECTED).stroke_width(1.5);
            }
            places.push(Place {
                from: held(from),
                to: held(to),
                top: lane + (LANE_PX - height) / 2.0,
                height,
                bar: true,
            });
            els.push(el);
        }
        // Markers over the bars.
        for (k, item) in ship.items.iter().enumerate() {
            for (i, m) in item.markers.iter().enumerate() {
                let f = tl.fraction(event_time(item, m));
                if (0.0..=1.0).contains(&f) {
                    let (_, c, _) = paths::event_style(ship, &colors, k, i);
                    places.push(Place {
                        from: f,
                        to: f,
                        top: lane + 2.0,
                        height: LANE_PX - 4.0,
                        bar: false,
                    });
                    els.push(spacer().fill(color(c)));
                }
            }
        }
    }
    let height = ships.len() as f32 * LANE_PX;
    if shown {
        places.push(Place {
            from: cursor,
            to: cursor,
            top: 0.0,
            height,
            bar: false,
        });
        els.push(spacer().fill(CURSOR));
    }
    let lanes = placed(els, places)
        .key(LANES_KEY)
        .width(Size::Fill(1.0))
        .height(Size::Fixed(height))
        .clip();
    let names = column(ships.iter().enumerate().map(|(s, ship)| {
        let name = text(ship.name.clone())
            .caption()
            .ellipsis()
            .width(Size::Fill(1.0))
            .height(Size::Fixed(LANE_PX));
        if selected.is_some_and(|(chosen, _)| chosen == s) {
            name
        } else {
            name.muted()
        }
    }))
    .key(GUTTER_KEY)
    .width(Size::Fixed(GUTTER_PX));
    let mut parts = vec![axis_row(tl, axis_px, shown.then_some(cursor))];
    if let Some(w) = windows {
        for (i, lane) in w.lanes.iter().enumerate() {
            // Each ship's departure on it, in its leg's color.
            let departures: Vec<(f64, Color)> = w
                .legs
                .iter()
                .filter(|&(_, &l)| l == i)
                .flat_map(|(&(s, step), _)| {
                    let colors = paths::colors(&ships[s]);
                    ships[s]
                        .items
                        .iter()
                        .enumerate()
                        .filter(move |(_, item)| item.step == step && item.kind == ItemKind::Leg)
                        .map(move |(k, item)| (tl.fraction(item.start), color(colors[k])))
                })
                .collect();
            parts.push(window_row(
                lane,
                tl,
                axis_px,
                departures,
                shown.then_some(cursor),
            ));
        }
    }
    if !ships.is_empty() {
        parts.push(
            scroll([row([names, lanes]).height(Size::Fixed(height))])
                .key(SCROLL_KEY)
                .height(Size::Fixed(ships.len().min(LANES_SHOWN) as f32 * LANE_PX)),
        );
    }
    column(parts)
        .width(Size::Fill(1.0))
        .padding(Sides::xy(tokens::SPACE_4, tokens::SPACE_1))
}

/// A window lane's row, `width` px wide: each pixel's days' least Δv,
/// brightest at the lane's least and dark from [`windows::CEILING`] times
/// it; ticks at `departures` (across the span, in their legs' colors), and
/// the cursor.
fn window_row(
    lane: &windows::Lane,
    tl: &Timeline,
    width: f32,
    departures: Vec<(f64, Color)>,
    cursor: Option<f64>,
) -> El {
    let n = width.round().clamp(1.0, 4096.0) as usize;
    let mut pixels = Vec::with_capacity(4 * n);
    for i in 0..n {
        let f = |i: usize| i as f64 / n as f64;
        let v = lane.least_between(tl.at(f(i)), tl.at(f(i + 1)));
        pixels.extend(windows::shaded(windows::shade(v, lane.least)));
    }
    let strip = image(Image::from_rgba8(n as u32, 1, pixels))
        .image_fit(ImageFit::Fill)
        .width(Size::Fill(1.0))
        .height(Size::Fixed(WINDOW_PX));
    let mut places = Vec::new();
    let mut els = Vec::new();
    for (f, c) in departures.into_iter().chain(cursor.map(|f| (f, CURSOR))) {
        if (0.0..=1.0).contains(&f) {
            places.push(Place {
                from: f,
                to: f,
                top: -1.0,
                height: WINDOW_PX + 2.0,
                bar: false,
            });
            els.push(spacer().fill(c));
        }
    }
    let legend = match lane.cheapest {
        Some(_) => format!(
            "{}: each day's least Δv leaving then, by patched conics over the durations \
             the planner tries; brightest at {:.2} km/s, dark from {:.2}",
            lane.label,
            lane.least,
            lane.least * windows::CEILING
        ),
        None => format!("{}: computing", lane.label),
    };
    row([
        text(lane.label.clone())
            .caption()
            .muted()
            .ellipsis()
            .tooltip(legend)
            .width(Size::Fixed(GUTTER_PX))
            .height(Size::Fixed(WINDOW_PX + 2.0)),
        stack([
            strip,
            placed(els, places)
                .width(Size::Fill(1.0))
                .height(Size::Fixed(WINDOW_PX)),
        ])
        .width(Size::Fill(1.0))
        .height(Size::Fixed(WINDOW_PX)),
    ])
    .height(Size::Fixed(WINDOW_PX + 2.0))
    .align(Align::Center)
}

/// The axis's row, `width` px wide: its ticks and their labels, and the
/// cursor at `cursor` across the span.
fn axis_row(tl: &Timeline, width: f32, cursor: Option<f64>) -> El {
    let ticks = ticks(tl.span, width);
    let room = ticks
        .windows(2)
        .map(|w| (tl.fraction(w[1].t) - tl.fraction(w[0].t)) as f32 * width)
        .fold(f32::INFINITY, f32::min);
    let mut places = Vec::new();
    let mut els = Vec::new();
    for tick in &ticks {
        let f = tl.fraction(tick.t);
        places.push(Place {
            from: f,
            to: f,
            top: AXIS_PX / 2.0,
            height: AXIS_PX / 2.0,
            bar: false,
        });
        els.push(spacer().fill(tokens::MUTED_FOREGROUND.with_alpha_u8(120)));
        // The label right of its tick, as wide as the room to the next.
        let w = if room.is_finite() { room - 6.0 } else { 120.0 };
        places.push(Place {
            from: f + f64::from(4.0 / width.max(1.0)),
            to: f + f64::from((4.0 + w.max(0.0)) / width.max(1.0)),
            top: 0.0,
            height: AXIS_PX / 2.0 + 2.0,
            bar: true,
        });
        els.push(
            text(tick.label.clone())
                .caption()
                .muted()
                .tabular_numerals()
                .nowrap_text()
                .ellipsis(),
        );
    }
    if let Some(f) = cursor {
        places.push(Place {
            from: f,
            to: f,
            top: 0.0,
            height: AXIS_PX,
            bar: false,
        });
        els.push(spacer().fill(CURSOR));
    }
    let marks = placed(els, places)
        .key(AXIS_KEY)
        .width(Size::Fill(1.0))
        .height(Size::Fixed(AXIS_PX))
        .fill(tokens::MUTED)
        .radius(3.0)
        .clip();
    row([
        spacer()
            .width(Size::Fixed(GUTTER_PX))
            .height(Size::Fixed(AXIS_PX)),
        marks,
    ])
    .height(Size::Fixed(AXIS_PX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> (Epoch, Epoch) {
        (Epoch::from_ymd(2025, 1, 1), Epoch::from_ymd(2080, 1, 1))
    }

    /// Zooming keeps the moment under the pointer, down to an hour across
    /// and out to the limits, within them.
    #[test]
    fn zoom_keeps_the_moment_under_the_pointer() {
        let mut tl = Timeline::new(Epoch::from_ymd(2045, 7, 1), limits());
        let at = tl.at(0.3);
        tl.zoom(0.3, 0.5);
        assert!((tl.at(0.3).0 - at.0).abs() < 1e-9);
        assert!((tl.days() - 365.25 / 2.0).abs() < 1e-9);
        assert!(!tl.fitted, "a zoom stops following the plan");
        for _ in 0..100 {
            tl.wheel(0.3, 1.0);
        }
        assert!((tl.days() - SHORTEST_DAYS).abs() < 1e-9);
        assert!((tl.at(0.3).0 - at.0).abs() < 1e-6);
        for _ in 0..100 {
            tl.wheel(0.9, -1.0);
        }
        assert_eq!(tl.span, limits());
    }

    /// Ticks fall on the calendar's steps, read as their dates, with room
    /// for their labels; the step grows with the span.
    #[test]
    fn ticks_fall_on_the_calendar() {
        let d = |y, m, d| Epoch::from_ymd(y, m, d);
        let cases = [
            ((d(2031, 1, 28), d(2068, 3, 1)), Step::Years(2)),
            ((d(2045, 1, 1), d(2046, 1, 1)), Step::Months(1)),
            ((d(2045, 2, 20), d(2045, 3, 20)), Step::Days(5)),
            (
                (d(2045, 3, 1), d(2045, 3, 1).plus_days(0.25)),
                Step::Hours(1),
            ),
        ];
        for (span, want) in cases {
            let width = 1000.0;
            let ticks = ticks(span, width);
            assert!(ticks.len() >= 2, "{span:?}: {ticks:?}");
            let step = STEPS
                .into_iter()
                .find(|s| s.days() / (span.1.0 - span.0.0) * 1000.0 >= f64::from(s.room()))
                .unwrap();
            assert_eq!(step, want, "{span:?}");
            for t in &ticks {
                assert!(t.t.0 >= span.0.0 && t.t.0 <= span.1.0);
                let (y, m, day) = t.t.ymd();
                let read = match step {
                    Step::Years(k) => {
                        assert_eq!((m, day, y % k), (1, 1.0, 0));
                        format!("{y}")
                    }
                    Step::Months(_) => {
                        assert_eq!(day, 1.0);
                        format!("{y}-{m:02}")
                    }
                    Step::Days(_) => format!("{y}-{m:02}-{:02}", day as u32),
                    Step::Hours(_) => {
                        let hour = (day.fract() * 24.0).round() as u32;
                        format!("{m:02}-{:02} {hour:02}:00", day.floor() as u32)
                    }
                };
                assert_eq!(t.label, read);
            }
            for w in ticks.windows(2) {
                let px = (w[1].t.0 - w[0].t.0) / (span.1.0 - span.0.0) * f64::from(width);
                assert!(px >= f64::from(step.room()) * 0.9, "{span:?}: {px} px");
            }
        }
        // Five-day steps start each month again at its first.
        let t = ticks((d(2045, 1, 20), d(2045, 3, 11)), 1000.0);
        let days: Vec<u32> = t.iter().map(|t| t.t.ymd().2 as u32).collect();
        assert!(
            days.windows(2).all(|w| w[1] == w[0] + 5 || w[1] == 1),
            "{days:?}"
        );
    }

    /// The span follows the plan until it is zoomed; the clock leaving
    /// it pages it.
    #[test]
    fn fits_the_plan_and_follows_the_clock() {
        let start = Epoch::from_ymd(2045, 3, 1);
        let mut tl = Timeline::new(start, limits());
        tl.fit((start, start.plus_days(100.0)));
        assert!((tl.fraction(start) - FIT_MARGIN / (1.0 + 2.0 * FIT_MARGIN)).abs() < 1e-9);
        assert!((tl.days() - 100.0 * (1.0 + 2.0 * FIT_MARGIN)).abs() < 1e-9);
        tl.fit((start, start.plus_days(1.0)));
        assert!((tl.days() - FIT_DAYS * (1.0 + 2.0 * FIT_MARGIN)).abs() < 1e-9);
        let span = tl.span;
        tl.follow(start, start.plus_days(2.0));
        assert_eq!(tl.span, span, "the clock within the span");
        let later = tl.span.1.plus_days(0.1);
        tl.follow(start.plus_days(2.0), later);
        assert!((tl.fraction(later) - PAGE_MARGIN).abs() < 1e-9);
        assert!(!tl.fitted);
        tl.fit((start, start.plus_days(100.0)));
        assert!(
            (tl.fraction(later) - PAGE_MARGIN).abs() < 1e-9,
            "no longer fitted"
        );
        let earlier = tl.span.0.plus_days(-3.0);
        tl.follow(later, earlier);
        assert!((tl.fraction(earlier) - (1.0 - PAGE_MARGIN)).abs() < 1e-9);
        // The span moved off the clock stays where it was put, the clock
        // still or moving outside it.
        let clock = tl.at(0.5);
        tl.zoom(0.95, 0.1);
        assert!(tl.fraction(clock) < 0.0);
        let span = tl.span;
        tl.follow(clock, clock);
        tl.follow(clock, clock.plus_days(0.01));
        assert_eq!(tl.span, span);
        tl.refit();
        tl.fit((start, start.plus_days(100.0)));
        assert!((tl.days() - 100.0 * (1.0 + 2.0 * FIT_MARGIN)).abs() < 1e-9);
    }

    /// A press that moves less than a click's slop is a click; past it,
    /// the span pans with the pointer.
    #[test]
    fn a_press_that_moves_pans() {
        let mut tl = Timeline::new(Epoch::from_ymd(2045, 3, 1), limits());
        let span = tl.span;
        tl.press((100.0, 10.0));
        tl.drag((102.0, 11.0), 1000.0);
        assert_eq!(tl.span, span);
        assert!(tl.release(), "a click");
        tl.press((100.0, 10.0));
        tl.drag((100.0, 200.0), 1000.0);
        assert_eq!(tl.span, span);
        assert!(!tl.release(), "moved down: no click, no pan");
        tl.press((100.0, 10.0));
        tl.drag((300.0, 10.0), 1000.0);
        assert!((tl.span.0.0 - (span.0.0 - 0.2 * 365.25)).abs() < 1e-9);
        tl.drag((102.0, 10.0), 1000.0);
        assert!(
            (tl.span.0.0 - (span.0.0 - 0.002 * 365.25)).abs() < 1e-9,
            "back within the slop, still a pan"
        );
        assert!(!tl.release());
        assert!(!tl.release(), "no press");
    }
}
