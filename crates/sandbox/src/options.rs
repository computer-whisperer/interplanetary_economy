//! The options panel (M1c.md, step 7 plan, item 5): for the leg selected,
//! its options along its frontier. Between planets, a porkchop over the
//! departures its search had (Δv by patched conics against departure and
//! duration), the dates the design tried and the one flown marked; a
//! click on a cell pins the leg there. Within a system, Δv against the
//! offered durations; a click on one pins it.

use crate::paths;
use crate::timeline;
use damascene_core::prelude::*;
use orbit::mission::{Crossing, Item};
use orbit::time::Epoch;
use rayon::prelude::*;
use std::sync::Arc;

/// The chart's key: a click on it picks what is under the pointer.
pub const CHART_KEY: &str = "options:chart";
/// The button that takes the leg's pin away.
pub const UNPIN_KEY: &str = "options:unpin";
/// The button that sweeps a leg's durations.
pub const SWEEP_KEY: &str = "options:sweep";

/// A sweep designs this many durations, from half the offered ones'
/// shortest to twice their longest.
const SWEPT: usize = 12;

/// The porkchop's most columns (departures) and rows (durations).
const MOST_COLUMNS: usize = 360;
const MOST_ROWS: usize = 180;
/// The chart's height, and the section's.
const CHART_PX: f32 = 170.0;
/// It is shaded from its least Δv, brightest, to this many times it, in
/// bands of a tenth of that.
const CEILING: f32 = 1.5;
const BANDS: f32 = 10.0;
const BRIGHT: [f32; 3] = [225.0, 225.0, 230.0];
const MARK_PX: f32 = 7.0;
const TRIED: Color = Color::srgb_u8(255, 255, 255);
const FAILED: Color = Color::srgb_u8(255, 80, 70);

/// A point's key, among a leg within a system's durations.
pub fn point_key(i: usize) -> String {
    format!("options:point:{i}")
}

/// The duration a point's key names, by its place among the options.
pub fn parse_point_key(key: &str) -> Option<usize> {
    key.strip_prefix("options:point:")?.parse().ok()
}

/// A swept duration's point's key, by its place in the sweep.
pub fn swept_key(i: usize) -> String {
    format!("options:swept:{i}")
}

pub fn parse_swept_key(key: &str) -> Option<usize> {
    key.strip_prefix("options:swept:")?.parse().ok()
}

/// The durations a sweep designs about `offered`, days: [`SWEPT`] of
/// them, evenly in their logarithm, to a twentieth of a day, none
/// offered already.
pub fn sweep_durations(offered: &[f64]) -> Vec<f64> {
    let (lo, hi) = offered
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &d| {
            (a.min(d), b.max(d))
        });
    if !lo.is_finite() {
        return Vec::new();
    }
    let (lo, hi) = ((0.5 * lo).max(0.05), 2.0 * hi);
    let mut out: Vec<f64> = (0..SWEPT)
        .map(|k| {
            let d = lo * (hi / lo).powf(k as f64 / (SWEPT - 1) as f64);
            (d * 20.0).round() / 20.0
        })
        .filter(|d| offered.iter().all(|o| (o - d).abs() > 1e-9))
        .collect();
    out.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    out
}

/// Δv by patched conics over a leg's departures and the durations its
/// date search tries.
#[derive(Clone, Debug)]
pub struct Porkchop {
    pub window: (Epoch, Epoch),
    /// Departures, evenly from the window's opening to its close.
    pub columns: usize,
    /// Durations, days, ascending.
    pub durations: Vec<f64>,
    /// Row by row from the shortest duration, km/s; NaN where none.
    pub dv: Vec<f32>,
    pub least: f32,
}

impl Porkchop {
    /// The porkchop of `crossing` over departures in `window`: a column a
    /// day, at most [`MOST_COLUMNS`]; rows the durations the date search
    /// tries at the window's opening, at most [`MOST_ROWS`].
    pub fn compute(crossing: &Crossing, window: (Epoch, Epoch)) -> Porkchop {
        let days = (window.1.0 - window.0.0).max(0.0);
        let columns = (days.floor() as usize + 1).clamp(1, MOST_COLUMNS);
        let all = crossing.durations(window.0);
        let every = all.len().div_ceil(MOST_ROWS).max(1);
        let durations: Vec<f64> = all.into_iter().step_by(every).collect();
        let mut p = Porkchop {
            window,
            columns,
            durations,
            dv: Vec::new(),
            least: f32::NAN,
        };
        let cells = p.columns * p.durations.len();
        p.dv = (0..cells)
            .into_par_iter()
            .map(|i| {
                let (row, column) = (i / p.columns, i % p.columns);
                crossing
                    .dv(p.depart(column), p.durations[row])
                    .map_or(f32::NAN, |v| v as f32)
            })
            .collect();
        p.least =
            p.dv.iter()
                .copied()
                .filter(|v| !v.is_nan())
                .fold(f32::NAN, f32::min);
        p
    }

    /// Column `column`'s departure.
    pub fn depart(&self, column: usize) -> Epoch {
        match self.columns {
            1 => self.window.0,
            n => Epoch(
                self.window.0.0
                    + (self.window.1.0 - self.window.0.0) * column as f64 / (n - 1) as f64,
            ),
        }
    }

    /// Where a departure and duration fall on the chart: across from the
    /// window's opening, and up from the shortest duration, 0 to 1 at the
    /// cells' centers' ends.
    pub fn place(&self, depart: Epoch, days: f64) -> (f64, f64) {
        let (c, r) = (self.columns as f64, self.durations.len() as f64);
        let across = match self.columns {
            1 => 0.0,
            n => {
                (depart.0 - self.window.0.0) / (self.window.1.0 - self.window.0.0) * (n - 1) as f64
            }
        };
        let (lo, hi) = (self.durations[0], *self.durations.last().unwrap());
        let up = match self.durations.len() {
            1 => 0.0,
            n => (days - lo) / (hi - lo) * (n - 1) as f64,
        };
        ((across + 0.5) / c, (up + 0.5) / r)
    }

    /// The cell under fractions `(x, y)` of the chart (`y` down from its
    /// top, the longest duration): its column and row.
    pub fn cell(&self, (x, y): (f64, f64)) -> (usize, usize) {
        let column = ((x * self.columns as f64).floor().max(0.0) as usize).min(self.columns - 1);
        let rows = self.durations.len();
        let up = ((1.0 - y) * rows as f64).floor().max(0.0) as usize;
        (column, up.min(rows - 1))
    }

    /// A cell's Δv, km/s (NaN for none).
    pub fn at(&self, column: usize, row: usize) -> f32 {
        self.dv[row * self.columns + column]
    }

    /// The chart as an image, the longest duration at the top.
    fn image(&self) -> Image {
        let [r, g, b, _] = tokens::MUTED.to_srgb_u8a();
        let dark = [f32::from(r), f32::from(g), f32::from(b)];
        let rows = self.durations.len();
        let mut pixels = Vec::with_capacity(4 * self.columns * rows);
        for row in (0..rows).rev() {
            for column in 0..self.columns {
                let v = self.at(column, row);
                let shade = 1.0 - (v - self.least) / (self.least * (CEILING - 1.0));
                let shade = if shade.is_nan() {
                    0.0
                } else {
                    (shade.clamp(0.0, 1.0) * BANDS).ceil() / BANDS
                };
                for c in 0..3 {
                    pixels.push((dark[c] + (BRIGHT[c] - dark[c]) * shade).round() as u8);
                }
                pixels.push(255);
            }
        }
        Image::from_rgba8(self.columns as u32, rows as u32, pixels)
    }
}

/// What the panel shows for the leg selected: its porkchop, kept while it
/// is the same leg and window.
#[derive(Default)]
pub struct Options {
    /// The leg (ship and step) and window the porkchop is for.
    key: Option<(usize, usize, (Epoch, Epoch))>,
    pub porkchop: Option<Arc<Porkchop>>,
}

impl Options {
    /// The porkchop of ship `s`'s step `step` over `window`, computed by
    /// `compute` unless kept.
    pub fn porkchop(
        &mut self,
        s: usize,
        step: usize,
        window: (Epoch, Epoch),
        compute: impl FnOnce() -> Option<Porkchop>,
    ) -> Option<Arc<Porkchop>> {
        if self.key != Some((s, step, window)) {
            self.key = Some((s, step, window));
            self.porkchop = compute().map(Arc::new);
        }
        self.porkchop.clone()
    }
}

/// What the section knows of the leg: its label, the item flown (none
/// while it is designed again), its color, whether it is pinned, and
/// what is under the pointer on the chart.
pub struct Leg<'a> {
    pub label: String,
    pub item: Option<&'a Item>,
    pub color: [f32; 3],
    pub pinned: bool,
    /// The pointer over the chart, as fractions of it.
    pub hover: Option<(f64, f64)>,
}

/// A mark placed by fractions of the chart (`y` down from the top).
fn mark(x: f64, y: f64, fill: Option<Color>, stroke: Color, size: f32) -> (El, (f64, f64, f32)) {
    let mut el = spacer().radius(size / 2.0).stroke(stroke).stroke_width(1.5);
    if let Some(f) = fill {
        el = el.fill(f);
    }
    (el, (x, y, size))
}

/// Elements placed at fractions of their container, centered.
fn marks(els: Vec<(El, (f64, f64, f32))>) -> El {
    let places: Vec<(f64, f64, f32)> = els.iter().map(|(_, p)| *p).collect();
    stack(els.into_iter().map(|(e, _)| e))
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0))
        .layout(move |ctx| {
            let c = ctx.container;
            places
                .iter()
                .map(|&(x, y, size)| {
                    Rect::new(
                        c.x + x as f32 * c.w - size / 2.0,
                        c.y + y as f32 * c.h - size / 2.0,
                        size,
                        size,
                    )
                })
                .collect()
        })
}

/// The section for a leg between planets: its porkchop (`width` px wide
/// when last laid out), and a line saying what is under the pointer.
pub fn porkchop_section(p: &Porkchop, leg: &Leg, width: f32) -> El {
    let mut els = Vec::new();
    if let Some(item) = leg.item {
        for c in &item.options {
            let (x, y) = p.place(c.depart, c.days);
            let (fill, stroke) = match c.dv {
                Some(_) => (None, TRIED),
                None => (None, FAILED),
            };
            els.push(mark(x, 1.0 - y, fill, stroke, MARK_PX));
        }
        let [r, g, b] = leg.color;
        let (x, y) = p.place(item.start, item.end.0 - item.start.0);
        els.push(mark(
            x,
            1.0 - y,
            Some(Color::srgb_linear(r, g, b, 1.0)),
            TRIED,
            MARK_PX + 3.0,
        ));
    }
    // The cell under the pointer.
    if let Some(f) = leg.hover {
        let (column, row) = p.cell(f);
        let (x, y) = p.place(p.depart(column), p.durations[row]);
        els.push(mark(x, 1.0 - y, None, TRIED, MARK_PX - 2.0));
    }
    let chart = stack([
        image(p.image())
            .image_fit(ImageFit::Fill)
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0)),
        marks(els),
        // The durations' range, at the chart's corners.
        column([
            text(format!("{:.0} d", p.durations.last().unwrap()))
                .caption()
                .muted(),
            spacer(),
            text(format!("{:.0} d", p.durations[0])).caption().muted(),
        ])
        .padding(Sides::xy(4.0, 2.0))
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0)),
    ])
    .key(CHART_KEY)
    .cursor(Cursor::Pointer)
    .width(Size::Fill(1.0))
    .height(Size::Fixed(CHART_PX))
    .clip();
    // Departure dates along the bottom, each at its date (the cells'
    // centers span the chart's width less a cell).
    let ticks = timeline::ticks(p.window, width);
    let at: Vec<f64> = ticks
        .iter()
        .map(|t| p.place(t.t, p.durations[0]).0)
        .collect();
    let room = at
        .windows(2)
        .map(|w| ((w[1] - w[0]) as f32 * width - 4.0).max(0.0))
        .fold(width, f32::min);
    let dates = stack(
        ticks
            .iter()
            .map(|t| {
                text(t.label.clone())
                    .caption()
                    .muted()
                    .tabular_numerals()
                    .nowrap_text()
                    .ellipsis()
            })
            .collect::<Vec<_>>(),
    )
    .width(Size::Fill(1.0))
    .height(Size::Fixed(16.0))
    .clip()
    .layout(move |ctx| {
        let c = ctx.container;
        at.iter()
            .map(|&f| Rect::new(c.x + f as f32 * c.w, c.y, room, c.h))
            .collect()
    });
    let readout = match leg.hover {
        Some(f) => {
            let (column, row) = p.cell(f);
            let v = p.at(column, row);
            let dv = if v.is_nan() {
                "none".to_string()
            } else {
                format!("{v:.2} km/s")
            };
            format!(
                "leave {} · {:.0} d · {dv} by patched conics: click to fly it",
                orbit::mission::when(p.depart(column)),
                p.durations[row]
            )
        }
        None => format!(
            "least {:.2} km/s; bright to {:.2}. ○ tried, ● flown{}",
            p.least,
            p.least * CEILING,
            if leg.pinned { " (pinned)" } else { "" }
        ),
    };
    column([chart, dates, text(readout).caption().muted().wrap_text()])
        .gap(tokens::SPACE_1)
        .width(Size::Fill(1.0))
}

/// The section for a leg within a system: Δv against its offered
/// durations and those of a sweep (`sweep`), each clickable.
pub fn durations_section(leg: &Leg, sweep: Option<&crate::plan::Sweep>) -> El {
    let Some(item) = leg.item else {
        return text("designing").caption().muted();
    };
    // (key, duration, Δv, offered or swept)
    let mut points: Vec<(String, f64, f64, bool)> = item
        .options
        .iter()
        .enumerate()
        .filter_map(|(i, c)| Some((point_key(i), c.days, c.dv?, true)))
        .collect();
    let mut status = String::new();
    match sweep.map(|s| (s.durations.len(), &s.options)) {
        Some((n, None)) => status = format!(" Sweeping {n} durations…"),
        Some((_, Some(Err(why)))) => status = format!(" The sweep failed: {why}"),
        Some((_, Some(Ok(swept)))) => {
            let failed = swept.iter().filter(|c| c.dv.is_none()).count();
            points.extend(
                swept
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| Some((swept_key(i), c.days, c.dv?, false))),
            );
            if failed > 0 {
                status = format!(" {failed} of the sweep's durations did not converge.");
            }
        }
        None => {}
    }
    if points.is_empty() {
        return text("no options").caption().muted();
    }
    let (d0, d1) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
            (a.min(p.1), b.max(p.1))
        });
    let (v0, v1) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
            (a.min(p.2), b.max(p.2))
        });
    let sweep_button = button(if sweep.is_some() {
        "Sweep again"
    } else {
        "Sweep durations"
    })
    .key(SWEEP_KEY);
    let spread = |x: f64, lo: f64, hi: f64| {
        if hi > lo {
            0.1 + 0.8 * (x - lo) / (hi - lo)
        } else {
            0.5
        }
    };
    let flown = item.end.0 - item.start.0;
    let [r, g, b] = leg.color;
    let els = points
        .iter()
        .map(|(key, days, dv, offered)| {
            let (days, dv) = (*days, *dv);
            let chosen = *offered && ((days - flown).abs() < 1e-6 || (dv - item.dv).abs() < 1e-9);
            let (el, place) = mark(
                spread(days, d0, d1),
                1.0 - spread(dv, v0, v1),
                chosen.then(|| Color::srgb_linear(r, g, b, 1.0)),
                if *offered {
                    TRIED
                } else {
                    tokens::MUTED_FOREGROUND
                },
                if *offered { MARK_PX + 3.0 } else { MARK_PX },
            );
            (
                el.key(key.clone())
                    .cursor(Cursor::Pointer)
                    .tooltip(format!("{days} d: {:.0} m/s — click to fly it", dv * 1e3)),
                place,
            )
        })
        .collect();
    column([
        marks(els)
            .fill(tokens::MUTED)
            .radius(3.0)
            .width(Size::Fill(1.0))
            .height(Size::Fixed(CHART_PX / 2.0)),
        row([
            text(format!(
                "Δv against duration: {d0}–{d1} d, {:.0}–{:.0} m/s. ● flown, ○ offered, small ○ swept.{status}",
                v0 * 1e3,
                v1 * 1e3,
            ))
            .caption()
            .muted()
            .wrap_text()
            .width(Size::Fill(1.0)),
            sweep_button,
        ])
        .gap(tokens::SPACE_2)
        .align(Align::Center),
    ])
    .gap(tokens::SPACE_1)
    .width(Size::Fill(1.0))
}

/// The options section under the plan's list: the leg's label, its
/// chart, and the button that takes a pin away.
pub fn section(leg: &Leg, chart: El) -> El {
    let mut head = vec![
        text(format!("Options · {}", leg.label))
            .caption()
            .bold()
            .ellipsis()
            .width(Size::Fill(1.0)),
    ];
    if leg.pinned {
        head.push(button("Unpin").key(UNPIN_KEY));
    }
    if leg.item.is_none() {
        head.push(text("designing").caption().muted());
    }
    column([row(head).gap(tokens::SPACE_2).align(Align::Center), chart])
        .gap(tokens::SPACE_2)
        .padding(Sides::xy(tokens::SPACE_3, tokens::SPACE_2))
        .width(Size::Fill(1.0))
}

/// The color a ship's item is drawn in (linear).
pub fn color(ship: &crate::plan::Ship, k: usize) -> [f32; 3] {
    paths::colors(ship).get(k).copied().unwrap_or([0.5; 3])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sweep spans half the shortest offered to twice the longest, in
    /// even ratios, offered ones left out.
    #[test]
    fn sweeps_span_the_offered() {
        let d = sweep_durations(&[0.5, 1.0, 2.0]);
        assert_eq!(d.first(), Some(&0.25));
        assert_eq!(d.last(), Some(&4.0));
        assert!(d.len() >= SWEPT - 2 && d.windows(2).all(|w| w[1] > w[0]));
        assert!(d.iter().all(|x| ![0.5, 1.0, 2.0].contains(x)));
        assert!(sweep_durations(&[]).is_empty());
    }
}
