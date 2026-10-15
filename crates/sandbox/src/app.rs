//! The sandbox's UI (M1c.md, step 2 plan, item 5; step 3 plan, items 6
//! and 7; step 4 plan, items 2, 4 and 6; step 6 plan, item 1): a top bar
//! with the focus, the frame and the clock; the map view; a scrubber; and
//! a status line.

use crate::bodies;
use crate::camera::MapCamera;
use crate::clock::{self, Clock};
use crate::frames::{self, Frame};
use crate::look::{self, Exposure};
use crate::paths::{self, Event, PAST_DIM};
use crate::plan::{Loaded, on_track, position};
use crate::sky;
use damascene_core::prelude::*;
use damascene_core::surface::AppTexture;
use damascene_core::widgets::slider;
use glam::DVec3;
use orbit::ephem::{BodyId, Ephemeris};
use orbit::families::Catalog;
use orbit::places::Places;
use orbit::time::Epoch;
use render::Timings;
use std::collections::HashMap;
use std::sync::Arc;

/// The map view's key: input over it goes to the camera.
pub const VIEWPORT_KEY: &str = "viewport";

/// The selects' and the scrubber's keys.
const FOCUS_KEY: &str = "focus";
const FRAME_KEY: &str = "frame";
const SCRUBBER_KEY: &str = "scrubber";

/// The longest step of wall time a frame advances, s.
const MAX_STEP_S: f64 = 0.25;

/// Stops of exposure set by hand, at most either way.
const MAX_BIAS: i32 = 12;

/// Lines' width, and a ship's marker's, logical pixels.
const LINE_PX: f32 = 1.5;
const SHIP_PX: f32 = 9.0;
const SHIP_COLOR: [f32; 3] = [0.9; 3];

/// What the status line reports.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    /// Wall time between frames, ms.
    pub frame_ms: Option<f32>,
    pub gpu: Option<Timings>,
    /// Whether the device has GPU timestamps at all.
    pub timestamps: bool,
    /// The output's headroom over SDR white, and that white in nits,
    /// where the compositor says.
    pub output: Option<(f32, f32)>,
    /// Whether the output is HDR.
    pub hdr: bool,
}

impl Stats {
    /// The output's headroom over SDR white: 1 on SDR, or where the
    /// compositor does not say.
    pub fn headroom(&self) -> f32 {
        match (self.hdr, self.output) {
            (true, Some((headroom, _))) => headroom.max(1.0),
            _ => 1.0,
        }
    }
}

/// What the paths were last built for: rebuilt when it changes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BuiltFor {
    frame: Frame,
    /// The focus, where the frame is centered on it.
    focus: Option<BodyId>,
    generation: u64,
}

pub struct Shell {
    pub eph: &'static Ephemeris,
    /// The scene's target, once its size is known.
    pub viewport: Option<AppTexture>,
    pub camera: MapCamera,
    pub clock: Clock,
    pub stats: Stats,
    pub frame: Frame,
    pub plan: Option<Loaded>,
    pub exposure: Exposure,
    /// The bodies the focus select offers.
    bodies: Vec<BodyId>,
    /// Station orbits' reference loops.
    loops: Vec<render::Path>,
    built: Option<BuiltFor>,
    paths: Arc<render::Paths>,
    /// Each path's ship and item, where it is one's.
    owners: HashMap<(usize, usize), usize>,
    events: Vec<Event>,
    /// Sorted marker times, for jumps.
    event_times: Vec<Epoch>,
    focus_open: bool,
    frame_open: bool,
}

impl Shell {
    /// Focused on Earth at `t`, in the frame centered on the focus.
    pub fn new(t: Epoch) -> Shell {
        let eph = Ephemeris::standard();
        let earth = eph.lookup("earth").expect("Earth is in the ephemeris");
        sky::start();
        Shell {
            eph,
            viewport: None,
            camera: MapCamera::new(eph, earth),
            clock: Clock::new(t, eph.span()),
            stats: Stats::default(),
            frame: Frame::Focus,
            plan: None,
            exposure: Exposure::default(),
            bodies: bodies::drawn(eph),
            loops: paths::loop_paths(&frames::station_loops(
                Places::standard(),
                Catalog::standard(),
            )),
            built: None,
            paths: Arc::default(),
            owners: HashMap::new(),
            events: Vec::new(),
            event_times: Vec::new(),
            focus_open: false,
            frame_open: false,
        }
    }

    /// Holds the clock within the span the focus is known over (a small
    /// body's, from 2040).
    pub fn follow_focus(&mut self) {
        self.clock.span = self.eph.span_of(self.camera.focus);
        self.clock.seek(self.clock.t);
    }

    /// Changes the frame, keeping the eye where it is.
    pub fn set_frame(&mut self, frame: Frame) {
        let (eph, t, focus) = (self.eph, self.clock.t, self.camera.focus);
        let (from, to) = (self.frame.pose(eph, focus, t), frame.pose(eph, focus, t));
        self.camera.reorient(from.axes, to.axes);
        self.frame = frame;
    }

    /// Advances the clock and the camera by `dt` seconds of wall time,
    /// at most [`MAX_STEP_S`]: after a stall or a hidden window, time
    /// resumes rather than leaping. Takes what the plan's designer has
    /// sent, and builds the paths again if they changed.
    pub fn tick(&mut self, dt: f64) {
        let dt = dt.min(MAX_STEP_S);
        self.follow_focus();
        self.clock.tick(dt);
        self.camera.tick(dt);
        self.exposure.tick(self.target_exposure(), dt);
        if let Some(plan) = self.plan.as_mut() {
            plan.poll();
        }
        self.update_paths();
    }

    /// Builds the paths and events, if the frame, its center or the plan
    /// changed since they were.
    pub fn update_paths(&mut self) {
        let key = BuiltFor {
            frame: self.frame,
            focus: (self.frame == Frame::Focus).then_some(self.camera.focus),
            generation: self.plan.as_ref().map_or(0, |p| p.generation),
        };
        if self.built == Some(key) {
            return;
        }
        let ships = self.plan.as_ref().map_or(&[][..], |p| &p.ships);
        let built = paths::build(self.eph, &self.frame, self.camera.focus, &self.loops, ships);
        self.owners = built
            .owners
            .iter()
            .enumerate()
            .filter_map(|(p, o)| Some(((*o)?, p)))
            .collect();
        self.paths = Arc::new(render::Paths { paths: built.paths });
        self.events = built.events;
        self.event_times = self.plan.as_ref().map_or_else(Vec::new, Loaded::events);
        self.built = Some(key);
    }

    /// The span the scrubber covers: the plan's, or the ephemeris's.
    fn timeline(&self) -> (Epoch, Epoch) {
        match &self.plan {
            Some(p) => p.span(),
            None => self.eph.span(),
        }
    }

    /// The exposure the focus calls for now.
    fn target_exposure(&self) -> f64 {
        look::exposure_for(self.eph, self.camera.focus, self.clock.t)
    }

    /// What the map view shows now, with `px` physical pixels to a
    /// logical one.
    pub fn scene(&self, px: f32) -> render::Scene {
        let (eph, t, focus) = (self.eph, self.clock.t, self.camera.focus);
        let poses = paths::poses(eph, &self.frame, focus, t);
        let view = poses[paths::VIEW_POSE];
        let mut markers: Vec<render::Marker> = self
            .events
            .iter()
            .map(|e| render::Marker {
                position: view.to_world(e.position),
                color: if e.t.0 < t.0 {
                    e.color.map(|c| c * PAST_DIM)
                } else {
                    e.color
                },
                size_px: e.size_px * px,
                shape: e.shape,
            })
            .collect();
        if let Some(plan) = &self.plan {
            markers.extend((0..plan.ships.len()).filter_map(|s| {
                Some(render::Marker {
                    position: self.ship_position(s, t, &view)?,
                    color: SHIP_COLOR,
                    size_px: SHIP_PX * px,
                    shape: render::Shape::Disc,
                })
            }));
        }
        render::Scene {
            camera: Some(self.camera.camera(eph, t, view.axes)),
            sun: eph.barycentric_position(eph.sun(), t),
            spheres: self
                .bodies
                .iter()
                .filter_map(|&b| bodies::sphere(eph, b, t))
                .collect(),
            markers,
            sky: sky::loaded().map(|d| sky::sky(d, t)),
            paths: Some(self.paths.clone()),
            poses,
            now: paths::seconds(t),
            line_px: LINE_PX * px,
            look: render::Look {
                exposure: self.exposure.value(self.target_exposure()),
                bloom: look::BLOOM,
                headroom: self.stats.headroom(),
                sky: look::SKY,
            },
            ..Default::default()
        }
    }

    /// Where ship `s` is at `t` (barycentric km): on its line, where its
    /// item there is drawn (the view's frame is at `view` then), so that
    /// the marker rides it in any frame; else on its track (resting at a
    /// body, or within rounding of the line's ends). `None` where no item
    /// with a track spans `t`.
    pub fn ship_position(&self, s: usize, t: Epoch, view: &render::Pose) -> Option<DVec3> {
        let ship = &self.plan.as_ref()?.ships[s];
        let k = ship.items.iter().rposition(|i| {
            i.track
                .as_ref()
                .is_some_and(|track| on_track(self.eph, track, t).is_some())
        })?;
        self.owners
            .get(&(s, k))
            .and_then(|&p| self.paths.paths[p].at(paths::seconds(t)))
            .map(|p| view.to_world(p))
            .or_else(|| position(self.eph, &ship.items[k..=k], t, true))
    }

    fn status(&self) -> String {
        let ms = |x: Option<f32>| x.map_or("–".to_string(), |x| format!("{x:.2} ms"));
        let frame = ms(self.stats.frame_ms);
        let gpu = match (self.stats.timestamps, self.stats.gpu) {
            (false, _) => "no GPU timestamps".to_string(),
            (true, None) => "GPU –".to_string(),
            (true, Some(t)) => format!("GPU scene {}, frame {}", ms(Some(t.scene)), ms(t.frame)),
        };
        let mut s = format!("frame {frame} · {gpu}");
        if let Some(plan) = &self.plan {
            s.push_str(&format!(" · {}", plan.progress()));
        }
        s
    }

    fn camera_text(&self) -> String {
        let altitude = self.camera.altitude(self.eph);
        let distance = if altitude < 1e6 {
            format!("{altitude:.0} km")
        } else if altitude < 3e7 {
            format!("{:.2} million km", altitude / 1e6)
        } else {
            format!("{:.3} au", altitude / render::scene::AU_KM)
        };
        let mut s = format!("{distance} above {}", self.eph.body(self.camera.focus).name);
        if self.exposure.bias != 0 {
            s.push_str(&format!(" · exposure {:+} stops", self.exposure.bias));
        }
        s
    }

    fn frame_name(&self) -> &'static str {
        Frame::offered(self.eph)
            .into_iter()
            .find(|(_, _, f)| *f == self.frame)
            .map_or("", |(_, name, _)| name)
    }
}

impl App for Shell {
    fn build(&self, cx: &BuildCx) -> El {
        let hdr = cx.diagnostics().is_some_and(|d| d.hdr_active());
        let output = match (hdr, self.stats.output) {
            (true, Some((headroom, nits))) => {
                format!("HDR: headroom {headroom:.2}× over {nits:.0} nits")
            }
            (true, None) => "HDR".to_string(),
            (false, _) => "SDR".to_string(),
        };
        let map = match &self.viewport {
            Some(texture) => surface(texture.clone())
                .surface_alpha(SurfaceAlpha::Opaque)
                .surface_fit(ImageFit::Fill),
            None => spacer(),
        }
        .key(VIEWPORT_KEY)
        .fill_size()
        .clip();
        let focus_name = self.eph.body(self.camera.focus).name.as_str();
        let timeline = self.timeline();
        let main = column([
            row([
                text("Interplanetary sandbox").bold(),
                text("Focus").muted().caption(),
                select_trigger(FOCUS_KEY, focus_name).width(Size::Fixed(180.0)),
                text("Frame").muted().caption(),
                select_trigger(FRAME_KEY, self.frame_name()).width(Size::Fixed(270.0)),
                spacer(),
                text(format!(
                    "{} · {}",
                    orbit::mission::when(self.clock.t),
                    self.clock.rate_text()
                ))
                .caption()
                .tabular_numerals(),
                text("space pause · , . warp · [ ] events · - = exposure")
                    .muted()
                    .caption(),
                text(output).muted().caption(),
            ])
            .gap(tokens::SPACE_3)
            .padding(Sides::xy(tokens::SPACE_4, tokens::SPACE_2))
            .align(Align::Center),
            map,
            row([
                text(orbit::mission::when(timeline.0))
                    .muted()
                    .caption()
                    .tabular_numerals(),
                slider(SCRUBBER_KEY, clock::fraction(timeline, self.clock.t) as f32)
                    .width(Size::Fill(1.0)),
                text(orbit::mission::when(timeline.1))
                    .muted()
                    .caption()
                    .tabular_numerals(),
            ])
            .gap(tokens::SPACE_3)
            .padding(Sides::xy(tokens::SPACE_4, tokens::SPACE_1))
            .align(Align::Center),
            row([
                text(self.status()).muted().caption().tabular_numerals(),
                spacer(),
                text(self.camera_text())
                    .muted()
                    .caption()
                    .tabular_numerals(),
            ])
            .padding(Sides::xy(tokens::SPACE_4, tokens::SPACE_1))
            .align(Align::Center),
        ])
        .width(Size::Fill(1.0))
        .height(Size::Fill(1.0));
        let mut layers = vec![main];
        if self.focus_open {
            // Small bodies exist from 2040, when their integration starts.
            let options: Vec<(String, String)> = self
                .bodies
                .iter()
                .filter(|&&b| self.eph.covers(b, self.clock.t))
                .map(|&b| (self.eph.body(b).id.clone(), self.eph.body(b).name.clone()))
                .collect();
            layers.push(select_menu(FOCUS_KEY, options));
        }
        if self.frame_open {
            let options: Vec<(String, String)> = Frame::offered(self.eph)
                .into_iter()
                .map(|(id, name, _)| (id.to_string(), name.to_string()))
                .collect();
            layers.push(select_menu(FRAME_KEY, options));
        }
        stack(layers).width(Size::Fill(1.0)).height(Size::Fill(1.0))
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        let clicked = matches!(event.kind, UiEventKind::Click | UiEventKind::Activate);
        if event.is_hotkey("pause") {
            self.clock.paused = !self.clock.paused;
        } else if event.is_hotkey("faster") {
            self.clock.faster();
        } else if event.is_hotkey("slower") {
            self.clock.slower();
        } else if event.is_hotkey("next") {
            self.clock.next(&self.event_times);
        } else if event.is_hotkey("previous") {
            self.clock.previous(&self.event_times);
        } else if event.is_hotkey("brighter") {
            self.exposure.bias = (self.exposure.bias + 1).min(MAX_BIAS);
        } else if event.is_hotkey("darker") {
            self.exposure.bias = (self.exposure.bias - 1).max(-MAX_BIAS);
        } else if matches!(event.kind, UiEventKind::PointerDown | UiEventKind::Drag)
            && event.route() == Some(SCRUBBER_KEY)
            && let (Some(rect), Some(x)) = (event.target_rect(), event.pointer_x())
        {
            let f = slider::normalized_from_event(rect, x);
            self.clock
                .seek(clock::at_fraction(self.timeline(), f64::from(f)));
        } else if event.is_click_or_activate(FOCUS_KEY) {
            self.focus_open = !self.focus_open;
            self.frame_open = false;
        } else if event.is_click_or_activate(FRAME_KEY) {
            self.frame_open = !self.frame_open;
            self.focus_open = false;
        } else if event.is_click_or_activate(&format!("{FOCUS_KEY}:dismiss")) {
            self.focus_open = false;
        } else if event.is_click_or_activate(&format!("{FRAME_KEY}:dismiss")) {
            self.frame_open = false;
        } else if clicked
            && let Some(id) = event.route_suffix(&format!("{FOCUS_KEY}:option"))
            && let Some(b) = self.eph.lookup(id)
        {
            self.camera.focus_on(self.eph, b, self.clock.t);
            self.follow_focus();
            self.focus_open = false;
        } else if clicked
            && let Some(id) = event.route_suffix(&format!("{FRAME_KEY}:option"))
            && let Some((_, _, f)) = Frame::offered(self.eph)
                .into_iter()
                .find(|(i, _, _)| *i == id)
        {
            self.set_frame(f);
            self.frame_open = false;
        }
    }

    fn hotkeys(&self) -> Vec<(KeyChord, String)> {
        vec![
            (
                KeyChord::named(LogicalKey::Named(NamedKey::Space)),
                "pause".into(),
            ),
            (KeyChord::vim('.'), "faster".into()),
            (KeyChord::vim(','), "slower".into()),
            (KeyChord::vim(']'), "next".into()),
            (KeyChord::vim('['), "previous".into()),
            (KeyChord::vim('='), "brighter".into()),
            (KeyChord::vim('-'), "darker".into()),
        ]
    }

    fn theme(&self) -> Theme {
        Theme::damascene_dark()
    }
}
