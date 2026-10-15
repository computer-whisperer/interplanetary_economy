//! The sandbox's UI (M1c.md, step 2 plan, item 5; step 3 plan, items 6
//! and 7; step 4 plan, items 2, 4 and 6; step 5 plan; step 6 plan, item
//! 1; step 7 plan, item 1): a top bar with the focus, the frame, the
//! clock and the settings; the map view, labeled and picked by clicks,
//! with the plan's panel beside it; the timeline; and a status line.

use crate::bodies;
use crate::camera::MapCamera;
use crate::clock::Clock;
use crate::editor::{self, Editor, Field};
use crate::frames::{self, Frame};
use crate::labels::{self, Anchor, Placed};
use crate::look::{self, Exposure};
use crate::options::{self, Options, Porkchop};
use crate::orbits::Orbits;
use crate::panel;
use crate::paths::{self, Event, PAST_DIM};
use crate::pick::{self, Pick};
use crate::plan::{Loaded, on_track, position};
use crate::settings::{self, Settings};
use crate::sky;
use crate::timeline::{self, Timeline};
use damascene_core::prelude::*;
use damascene_core::scroll::{ScrollAlignment, ScrollRequest};
use damascene_core::surface::AppTexture;
use damascene_core::toast::ToastSpec;
use damascene_core::widgets::text_input;
use glam::{DVec3, Vec2};
use orbit::ephem::{BodyId, Ephemeris};
use orbit::families::Catalog;
use orbit::mission::{Item, ItemKind, Plan, Step};
use orbit::places::Places;
use orbit::time::Epoch;
use render::Timings;
use std::collections::HashMap;
use std::sync::Arc;

/// The map view's key: input over it goes to the camera.
pub const VIEWPORT_KEY: &str = "viewport";

/// The selects' keys.
const FOCUS_KEY: &str = "focus";
const FRAME_KEY: &str = "frame";
const PLAN_KEY: &str = "plan-file";

/// The plan menu's option for a new plan.
const NEW_PLAN: &str = "new";

/// The longest step of wall time a frame advances, s.
const MAX_STEP_S: f64 = 0.25;

/// Stops of exposure set by hand, at most either way: enough to bring up
/// the sky with the Sun in focus (~35 stops).
pub const MAX_BIAS: i32 = 40;

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

/// What the map view showed in the last frame: its projection, the
/// poses its paths were placed by, what was drawn on it, and the labels
/// placed.
struct Seen {
    view: labels::View,
    poses: Vec<render::Pose>,
    /// The bodies drawn as disks.
    balls: Vec<labels::Ball>,
    anchors: Vec<Anchor>,
    placed: Vec<Placed>,
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
    /// Whether the ships' paths and the station orbits are shown.
    ships: bool,
    stations: bool,
}

pub struct Shell {
    pub eph: &'static Ephemeris,
    /// The scene's target, once its size is known.
    pub viewport: Option<AppTexture>,
    pub camera: MapCamera,
    pub clock: Clock,
    pub timeline: Timeline,
    /// The clock's time when the timeline last followed it.
    followed: Epoch,
    pub stats: Stats,
    pub frame: Frame,
    pub plan: Option<Loaded>,
    pub exposure: Exposure,
    pub settings: Settings,
    /// The bodies the focus select offers.
    bodies: Vec<BodyId>,
    /// Station orbits' reference loops, and their places' ids.
    loops: Vec<render::Path>,
    loop_names: Vec<String>,
    orbits: Orbits,
    built: Option<BuiltFor>,
    paths: Arc<render::Paths>,
    /// Each path's ship and item, where it is one's.
    owners: HashMap<(usize, usize), usize>,
    events: Vec<Event>,
    /// Sorted marker times, for jumps.
    event_times: Vec<Epoch>,
    focus_open: bool,
    frame_open: bool,
    plan_open: bool,
    toasts: Vec<ToastSpec>,
    pub settings_open: bool,
    /// The item selected, by its ship and place: in the panel, its
    /// markers' labels first.
    pub selected: Option<(usize, usize)>,
    /// The leg (ship and step) whose options the panel shows: that of the
    /// item selected, kept while it is designed again.
    pub leg: Option<(usize, usize)>,
    options: Options,
    /// Where the pointer is, logical px, if over the window.
    pub pointer: Option<(f32, f32)>,
    /// The ship being edited, and the text fields' shared selection.
    pub editor: Option<Editor>,
    selection: Selection,
    /// The plan before each change, for undo.
    undo: Vec<Plan>,
    pub panel_open: bool,
    seen: Option<Seen>,
    /// Labels' sizes by their text.
    sizes: HashMap<String, Vec2>,
    scrolls: Vec<ScrollRequest>,
}

impl Shell {
    /// Focused on Earth at `t`, in the frame centered on the focus.
    pub fn new(t: Epoch) -> Shell {
        let eph = Ephemeris::standard();
        let earth = eph.lookup("earth").expect("Earth is in the ephemeris");
        sky::start();
        crate::maps::start();
        let (loop_names, loops): (Vec<String>, Vec<_>) =
            frames::station_loops(Places::standard(), Catalog::standard())
                .into_iter()
                .unzip();
        Shell {
            eph,
            viewport: None,
            camera: MapCamera::new(eph, earth),
            clock: Clock::new(t, eph.span()),
            timeline: Timeline::new(t, eph.span()),
            followed: t,
            stats: Stats::default(),
            frame: Frame::Focus,
            plan: None,
            exposure: Exposure::default(),
            settings: Settings::default(),
            bodies: bodies::drawn(eph),
            orbits: Orbits::new(eph, &bodies::drawn(eph), t),
            loops: paths::loop_paths(loops),
            loop_names,
            built: None,
            paths: Arc::default(),
            owners: HashMap::new(),
            events: Vec::new(),
            event_times: Vec::new(),
            focus_open: false,
            frame_open: false,
            plan_open: false,
            toasts: Vec::new(),
            settings_open: false,
            selected: None,
            leg: None,
            options: Options::default(),
            pointer: None,
            editor: None,
            selection: Selection::default(),
            undo: Vec::new(),
            panel_open: true,
            seen: None,
            sizes: HashMap::new(),
            scrolls: Vec::new(),
        }
    }

    /// The leg's item, if designed: ship `s`'s leg made by `step`.
    fn leg_item(&self, (s, step): (usize, usize)) -> Option<(usize, &Item)> {
        self.plan
            .as_ref()?
            .ships
            .get(s)?
            .items
            .iter()
            .enumerate()
            .find(|(_, i)| i.step == step && i.kind == ItemKind::Leg)
    }

    /// Computes the porkchop of the leg whose options are shown, if it is
    /// one between planets and is not kept.
    fn update_options(&mut self) {
        let (Some(plan), Some((s, step))) = (&self.plan, self.leg) else {
            return;
        };
        if !plan.windows.legs.contains_key(&(s, step)) {
            return;
        }
        let Some(window) = plan.ships[s]
            .items
            .iter()
            .find(|i| i.step == step && i.kind == ItemKind::Leg)
            .and_then(|i| i.window)
        else {
            return;
        };
        let Some((from, to, aero)) = crate::windows::ends(&plan.plan, s, step) else {
            return;
        };
        self.options.porkchop(s, step, window, || {
            let c = crate::plan::world()
                .crossing(&from.kind, &to.kind, aero)
                .ok()?;
            Some(Porkchop::compute(&c, window))
        });
    }

    /// Pins ship `s`'s leg at `step` to leave at `depart` (none: as its
    /// window allows) taking `days` (none: as the planner picks), and
    /// designs it again from there.
    fn pin(&mut self, (s, step): (usize, usize), depart: Option<Epoch>, days: Vec<f64>) {
        let Some(plan) = self.plan.as_mut() else {
            return;
        };
        let mut ship = plan.plan.ships[s].clone();
        let Some(Step::Go(go)) = ship.steps.get_mut(step) else {
            return;
        };
        go.depart = depart;
        go.days = days;
        plan.edit(s, ship, Some(step));
        // The items after the edit are gone; the panel keeps the leg.
        self.selected = None;
    }

    /// A click on the options panel's chart at `at`: the porkchop's cell
    /// there pinned.
    fn clicked_chart(&mut self, at: (f32, f32), chart: Rect) {
        let (Some(leg), Some(p)) = (self.leg, self.options.porkchop.clone()) else {
            return;
        };
        let f = (
            f64::from((at.0 - chart.x) / chart.w),
            f64::from((at.1 - chart.y) / chart.h),
        );
        let (column, row) = p.cell(f);
        if p.at(column, row).is_nan() {
            return;
        }
        // To the minute and a hundredth of a day, as the plan's file
        // keeps them.
        let depart = Epoch((p.depart(column).0 * 1_440.0).round() / 1_440.0);
        let days = (p.durations[row] * 100.0).round() / 100.0;
        self.pin(leg, Some(depart), vec![days]);
    }

    /// The options section for the leg shown, if any.
    fn options_section(&self, cx: &BuildCx) -> Option<El> {
        let (s, step) = self.leg?;
        let plan = self.plan.as_ref()?;
        let (from, to, aero) = crate::windows::ends(&plan.plan, s, step)?;
        let item = self.leg_item((s, step));
        let pinned = matches!(
            plan.plan.ships[s].steps.get(step),
            Some(Step::Go(go)) if go.depart.is_some()
        );
        let chart = cx.rect_of_key(options::CHART_KEY);
        let hover = match (self.pointer, chart) {
            (Some((x, y)), Some(r)) if r.contains(x, y) => {
                Some((f64::from((x - r.x) / r.w), f64::from((y - r.y) / r.h)))
            }
            _ => None,
        };
        let leg = options::Leg {
            label: format!(
                "{} → {}{}",
                from.label,
                to.label,
                if aero { " (aero)" } else { "" }
            ),
            item: item.map(|(_, i)| i),
            color: item.map_or([0.5; 3], |(k, _)| options::color(&plan.ships[s], k)),
            pinned,
            hover,
        };
        let body = if plan.windows.legs.contains_key(&(s, step)) {
            match &self.options.porkchop {
                Some(p) => options::porkchop_section(p, &leg, chart.map_or(350.0, |r| r.w)),
                None => text("computing").caption().muted(),
            }
        } else {
            options::durations_section(&leg, plan.sweeps.get(&(s, step)))
        };
        Some(options::section(&leg, body))
    }

    /// Replaces ship `s` with `ship`, designing it again from where it
    /// first changed (a new name alone is only a rename); the plan before
    /// is kept for undo.
    pub fn change(&mut self, s: usize, ship: orbit::mission::Ship) {
        let Some(plan) = self.plan.as_mut() else {
            return;
        };
        let Some(changed) = editor::first_change(&plan.plan.ships[s], &ship) else {
            if ship.name != plan.plan.ships[s].name {
                self.undo.push(plan.plan.clone());
                plan.rename(s, &ship.name);
            }
            return;
        };
        self.undo.push(plan.plan.clone());
        plan.edit(s, ship, changed);
        self.selected = None;
    }

    /// Back to the plan before the last change: the ships changed
    /// designed again from where they differ, or, if ships were added or
    /// removed, the whole plan.
    pub fn undo(&mut self) {
        let (Some(plan), Some(before)) = (self.plan.as_mut(), self.undo.pop()) else {
            return;
        };
        self.selected = None;
        if before.ships.len() != plan.plan.ships.len() {
            let path = plan.path.clone();
            let mut restarted = Loaded::start(before);
            restarted.path = path;
            self.plan = Some(restarted);
            self.leg = None;
            self.editor = None;
            self.built = None;
            return;
        }
        for (s, ship) in before.ships.into_iter().enumerate() {
            match editor::first_change(&plan.plan.ships[s], &ship) {
                Some(changed) => plan.edit(s, ship, changed),
                None if ship.name != plan.plan.ships[s].name => plan.rename(s, &ship.name),
                None => {}
            }
        }
    }

    /// Commits the editor's drafts: each that reads as a value changes
    /// the ship; the first that does not says why.
    fn commit_drafts(&mut self) {
        let Some(ed) = self.editor.as_mut() else {
            return;
        };
        let drafts: Vec<(Field, String)> = ed.drafts.drain().collect();
        for (field, text) in drafts {
            self.commit(field, &text);
        }
    }

    /// Commits `text` typed into `field` of the ship edited.
    fn commit(&mut self, field: Field, text: &str) {
        let (Some(ed), Some(plan)) = (self.editor.as_ref(), self.plan.as_ref()) else {
            return;
        };
        let s = ed.ship;
        let result = editor::set(&plan.plan.ships[s], field, text);
        match result {
            Ok(ship) => {
                self.change(s, ship);
                if let Some(ed) = self.editor.as_mut() {
                    ed.refused = None;
                }
            }
            Err(why) => {
                if let Some(ed) = self.editor.as_mut() {
                    ed.refused = Some(why);
                }
            }
        }
    }

    /// An event for the editor: `true` if it took it.
    fn editor_event(&mut self, event: &UiEvent) -> bool {
        let Some(ed) = self.editor.as_ref() else {
            return false;
        };
        let s = ed.ship;
        let routed = event.route().and_then(Field::parse);
        let target = event.target_key().and_then(Field::parse);
        // A press on a field starts typing into it; anywhere else, ends
        // it.
        if event.kind == UiEventKind::PointerDown
            && let Some(ed) = self.editor.as_mut()
        {
            ed.typing = routed.is_some_and(|f| f.typed());
        }
        let ed = self.editor.as_ref().unwrap();
        // A press anywhere but on a draft's field commits the drafts.
        if event.kind == UiEventKind::PointerDown && ed.drafts.keys().any(|f| Some(*f) != routed) {
            let keep = routed.and_then(|f| self.editor.as_mut()?.drafts.remove_entry(&f));
            self.commit_drafts();
            if let (Some((f, t)), Some(ed)) = (keep, self.editor.as_mut()) {
                ed.drafts.insert(f, t);
            }
        }
        let Some(plan) = self.plan.as_ref() else {
            return false;
        };
        let ship = plan.plan.ships[s].clone();
        // Typing into a field.
        if let Some(f) = target.or(routed).filter(|f| f.typed()) {
            let ed = self.editor.as_mut().unwrap();
            // Enter: an Activate, or, in a field capturing keys, a raw
            // key down.
            let enter = event.kind == UiEventKind::Activate
                || (event.kind == UiEventKind::KeyDown
                    && event
                        .key_press
                        .as_ref()
                        .is_some_and(|k| k.logical == LogicalKey::Named(NamedKey::Enter)));
            match event.kind {
                _ if enter => {
                    let text = ed
                        .drafts
                        .remove(&f)
                        .unwrap_or_else(|| editor::value(&ship, f));
                    self.commit(f, &text);
                    return true;
                }
                UiEventKind::Escape => {
                    // Damascene takes the focus away after it.
                    ed.drafts.remove(&f);
                    ed.typing = false;
                    return true;
                }
                _ => {}
            }
            let mut text = ed
                .drafts
                .get(&f)
                .cloned()
                .unwrap_or_else(|| editor::value(&ship, f));
            if text_input::apply_event(&mut text, &mut self.selection, event, &f.key()) {
                ed.drafts.insert(f, text);
                return true;
            }
            if matches!(event.kind, UiEventKind::KeyDown | UiEventKind::TextInput) {
                return true;
            }
        }
        let clicked = matches!(event.kind, UiEventKind::Click | UiEventKind::Activate);
        // A place menu's option, or its trigger.
        let ed = self.editor.as_mut().unwrap();
        if let Some(f) = ed.menu {
            if clicked && let Some(id) = event.route_suffix(&format!("{}:option", f.key())) {
                ed.menu = None;
                let changed = editor::place(&ship, f, id, Places::standard());
                match changed {
                    Ok(ship) => self.change(s, ship),
                    Err(why) => self.editor.as_mut().unwrap().refused = Some(why),
                }
                return true;
            }
            if event.is_click_or_activate(&format!("{}:dismiss", f.key()))
                || event.kind == UiEventKind::Escape
            {
                ed.menu = None;
                return true;
            }
        }
        if !clicked {
            return false;
        }
        match routed {
            Some(f @ (Field::Start | Field::To(_))) => {
                ed.menu = if ed.menu == Some(f) { None } else { Some(f) };
            }
            Some(Field::Done) => {
                self.commit_drafts();
                self.editor = None;
            }
            Some(Field::RemoveShip) => {
                if let Some(plan) = self.plan.as_mut() {
                    self.undo.push(plan.plan.clone());
                    plan.remove_ship(s);
                }
                self.editor = None;
                self.selected = None;
                self.leg = None;
            }
            Some(f) if !f.typed() => {
                if let Some(ship) = editor::press(&ship, f, Places::standard()) {
                    self.change(s, ship);
                }
            }
            _ => return false,
        }
        true
    }

    fn close_menus(&mut self) {
        self.focus_open = false;
        self.frame_open = false;
        self.plan_open = false;
        self.settings_open = false;
    }

    /// Shows `plan` from its first ship's start, the timeline fitted to
    /// it.
    pub fn load(&mut self, plan: Loaded) {
        self.clock.seek(plan.start);
        self.plan = Some(plan);
        self.selected = None;
        self.leg = None;
        self.editor = None;
        self.undo.clear();
        self.built = None;
        self.timeline.refit();
        self.poll();
    }

    /// Saves the plan to its file, saying so.
    fn save(&mut self) {
        let Some(plan) = &self.plan else {
            return;
        };
        self.toasts.push(match plan.save() {
            Ok(path) => ToastSpec::success(format!("Saved {}", path.display())),
            Err(why) => ToastSpec::error(format!("Not saved: {why}")),
        });
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
    /// resumes rather than leaping. Then [`Self::poll`], and builds the
    /// paths again if they changed.
    pub fn tick(&mut self, dt: f64) {
        let dt = dt.min(MAX_STEP_S);
        self.follow_focus();
        self.clock.tick(dt);
        self.camera.tick(dt);
        self.exposure.tick(self.target_exposure(), dt);
        self.poll();
        self.update_paths();
    }

    /// Takes what the plan's designer has sent; the timeline follows the
    /// plan and the clock; the leg's porkchop is computed if it is new.
    pub fn poll(&mut self) {
        if let Some(plan) = self.plan.as_mut() {
            plan.poll();
            self.timeline.fit(plan.span());
        }
        self.update_options();
        self.timeline.follow(self.followed, self.clock.t);
        self.followed = self.clock.t;
    }

    /// Builds the paths and events, if the frame, its center, the plan or
    /// the classes of lines shown changed since they were, and the bodies'
    /// orbits whose windows moved.
    pub fn update_paths(&mut self) {
        let lines = self.settings.lines;
        self.orbits
            .update(self.eph, self.clock.t, self.camera.focus, lines);
        let key = BuiltFor {
            frame: self.frame,
            focus: (self.frame == Frame::Focus).then_some(self.camera.focus),
            generation: self.plan.as_ref().map_or(0, |p| p.generation),
            ships: lines.ships,
            stations: lines.stations,
        };
        if self.built == Some(key) {
            return;
        }
        let ships = match &self.plan {
            Some(p) if lines.ships => &p.ships[..],
            _ => &[],
        };
        let loops = if lines.stations { &self.loops[..] } else { &[] };
        let built = paths::build(self.eph, &self.frame, self.camera.focus, loops, ships);
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

    /// The exposure the focus calls for now.
    fn target_exposure(&self) -> f64 {
        let sun = self.settings.lifts()[3];
        look::exposure_for(self.eph, self.camera.focus, self.clock.t, f64::from(sun))
    }

    /// What the map view shows now, with `px` physical pixels to a
    /// logical one.
    pub fn scene(&self, px: f32) -> render::Scene {
        let (eph, t, focus) = (self.eph, self.clock.t, self.camera.focus);
        let mut poses = paths::poses(eph, &self.frame, focus, t);
        poses.extend(self.orbits.poses(eph, t));
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
        let [sky_lift, body_lift, night_lift, sun_lift] = self.settings.lifts();
        let camera = self.camera.camera(eph, t, view.axes);
        render::Scene {
            camera: Some(camera),
            sun: eph.barycentric_position(eph.sun(), t),
            sun_radius: eph.body(eph.sun()).radius,
            spheres: bodies::spheres(eph, &self.bodies, t, camera.eye),
            markers,
            sky: sky::loaded().map(|d| sky::sky(d, t)),
            // By `paths::PLAN_SET` and `ORBIT_SET`.
            paths: vec![self.paths.clone(), self.orbits.paths()],
            poses,
            now: t.seconds(),
            frame_s: self.clock.stepped_s,
            line_px: LINE_PX * px,
            look: render::Look {
                exposure: self.exposure.value(self.target_exposure()),
                headroom: self.stats.headroom(),
                pixel: (look::PIXEL / f64::from(px)) as f32,
                eye: Some(render::glare::Eye::STANDARD),
                sky_lift,
                body_lift,
                night_lift,
                sun_lift,
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
            .and_then(|&p| self.paths.paths[p].at(t.seconds()))
            .map(|p| view.to_world(p))
            .or_else(|| position(self.eph, &ship.items[k..=k], t, true))
    }

    /// Projects what the map view shows in `scene` onto its target of
    /// `size` pixels, `scale` to a logical one, and places their labels
    /// (none without a target yet, or with labels hidden: what is drawn
    /// can still be picked).
    pub fn place_labels(&mut self, scene: &render::Scene, size: Option<(u32, u32)>, scale: f32) {
        let (Some(camera), Some(size)) = (scene.camera, size) else {
            self.seen = None;
            return;
        };
        let view = labels::View {
            camera,
            size,
            scale,
        };
        let (eph, t) = (self.eph, self.clock.t);
        let (balls, mut anchors) = labels::bodies(eph, t, &self.bodies, self.camera.focus, &view);
        let pose = scene.poses[paths::VIEW_POSE];
        if let Some(plan) = &self.plan {
            for (s, ship) in plan.ships.iter().enumerate() {
                if let Some(p) = self.ship_position(s, t, &pose) {
                    anchors.extend(labels::ship(&view, &balls, s, &ship.name, p, SHIP_PX));
                }
            }
            for e in &self.events {
                let m = &plan.ships[e.ship].items[e.item].markers[e.marker];
                anchors.extend(labels::marker(
                    eph,
                    &view,
                    &balls,
                    e,
                    m,
                    &pose,
                    t,
                    self.selected,
                ));
            }
        }
        let loop_pose = scene.poses[paths::LOOP_POSE];
        let loops = if self.settings.lines.stations {
            &self.loops[..]
        } else {
            &[]
        };
        for (i, (name, path)) in self.loop_names.iter().zip(loops).enumerate() {
            if let Some(first) = path.samples.first() {
                let p = loop_pose.to_world(first.position);
                anchors.extend(labels::station(&view, &balls, i, name, p));
            }
        }
        let sizes = &mut self.sizes;
        let placed = match self.settings.labels {
            true => labels::declutter(&anchors, view.extent(), |text| {
                *sizes
                    .entry(text.to_string())
                    .or_insert_with(|| labels::measure(text))
            }),
            false => Vec::new(),
        };
        self.seen = Some(Seen {
            view,
            poses: scene.poses.clone(),
            balls,
            anchors,
            placed,
        });
    }

    /// What a click at `at` (logical px on the map view) picks, in what
    /// the view showed last.
    pub fn pick(&self, at: Vec2) -> Option<Pick> {
        let seen = self.seen.as_ref()?;
        pick::marked(at, &seen.placed, &seen.anchors)
            .or_else(|| {
                let (s, k) = pick::line(
                    at,
                    &seen.view,
                    &self.paths,
                    &seen.poses,
                    &self.owners,
                    &seen.balls,
                )?;
                Some(Pick::Item(s, k))
            })
            .or_else(|| pick::body(at, &seen.anchors))
    }

    /// A click on the map view at `at`: a body is focused; a ship's item
    /// (the one it is in), a marker's or a line's is selected; empty
    /// space clears the selection.
    pub fn clicked_map(&mut self, at: Vec2) {
        match self.pick(at) {
            Some(Pick::Body(b)) => {
                self.camera.focus_on(self.eph, b, self.clock.t);
                self.follow_focus();
            }
            Some(Pick::Ship(s)) => {
                let k = self
                    .plan
                    .as_ref()
                    .and_then(|p| panel::current(&p.ships[s], self.clock.t));
                self.select(k.map(|k| (s, k)));
            }
            Some(Pick::Item(s, k)) => self.select(Some((s, k))),
            None => self.select(None),
        }
    }

    /// Selects `item`, scrolling the panel to it.
    pub fn select(&mut self, item: Option<(usize, usize)>) {
        self.selected = item;
        // The options of the leg the item is part of (a stay's, the leg
        // after it).
        self.leg = item.and_then(|(s, k)| {
            let ship = &self.plan.as_ref()?.ships[s];
            let step = ship.items.get(k)?.step;
            ship.items
                .iter()
                .any(|i| i.step == step && i.kind == ItemKind::Leg)
                .then_some((s, step))
        });
        if let Some((s, k)) = item {
            self.scrolls.push(ScrollRequest::to_row_key(
                panel::LIST_KEY,
                panel::item_key(s, k),
                ScrollAlignment::Visible,
            ));
        }
    }

    /// The labels placed in the last frame.
    pub fn placed(&self) -> &[Placed] {
        self.seen.as_ref().map_or(&[], |s| &s.placed)
    }

    /// The map view's projection in the last frame.
    pub fn view(&self) -> Option<labels::View> {
        self.seen.as_ref().map(|s| s.view)
    }

    /// What the map view showed in the last frame.
    pub fn anchors(&self) -> &[Anchor] {
        self.seen.as_ref().map_or(&[], |s| &s.anchors)
    }

    /// Presses, drags and releases on the timeline: on the axis, seeking
    /// the clock (held to the span shown); on the lanes, panning, a click
    /// on a bar selecting its item (on empty space, nothing) and seeking
    /// to where it landed, and a double click on empty space fitting the
    /// plan again. `false` if the event is not the timeline's.
    fn timeline_event(&mut self, event: &UiEvent, cx: &EventCx) -> bool {
        let Some(route) = event.route() else {
            return false;
        };
        let bar = timeline::parse_bar_key(route);
        let on_axis = route == timeline::AXIS_KEY;
        if !on_axis && route != timeline::LANES_KEY && bar.is_none() {
            return false;
        }
        let (Some(axis), Some((x, y))) = (cx.rect_of_key(timeline::AXIS_KEY), event.pointer) else {
            return true;
        };
        let f = f64::from((x - axis.x) / axis.w.max(1.0));
        match event.kind {
            UiEventKind::PointerDown | UiEventKind::Drag if on_axis => {
                self.clock.seek(self.timeline.at(f.clamp(0.0, 1.0)));
            }
            UiEventKind::PointerDown if !on_axis => self.timeline.press((x, y)),
            UiEventKind::Drag => self.timeline.drag((x, y), axis.w),
            UiEventKind::PointerUp if !on_axis => {
                // Released within a click's slop of where it pressed.
                let clicked = self.timeline.release();
                if clicked {
                    self.clock.seek(self.timeline.at(f));
                    self.select(bar);
                    if bar.is_none() && event.click_count >= 2 {
                        self.timeline.refit();
                    }
                }
            }
            _ => {}
        }
        true
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
        let distance = if altitude < 1.0 {
            format!("{:.0} m", altitude * 1000.0)
        } else if altitude < 10.0 {
            format!("{altitude:.1} km")
        } else if altitude < 1e6 {
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

    /// The plan's file's name, or the plan's.
    fn plan_name(&self) -> String {
        match &self.plan {
            Some(p) => p
                .path
                .as_deref()
                .and_then(|p| p.file_stem()?.to_str())
                .unwrap_or(&p.name)
                .to_string(),
            None => "None".to_string(),
        }
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
        let scene = match &self.viewport {
            Some(texture) => surface(texture.clone())
                .surface_alpha(SurfaceAlpha::Opaque)
                .surface_fit(ImageFit::Fill)
                .fill_size(),
            None => spacer(),
        };
        let labels = match &self.seen {
            Some(seen) => labels::layer(&seen.placed, seen.view.extent()),
            None => spacer(),
        };
        let map = stack([scene, labels]).key(VIEWPORT_KEY).fill_size().clip();
        let middle = match (&self.plan, self.panel_open) {
            (Some(plan), true) => row([
                map,
                column(
                    [
                        Some(panel::panel(
                            &plan.name,
                            panel::rows(self.eph, &plan.ships, self.clock.t, self.selected),
                        )),
                        match &self.editor {
                            Some(ed) if ed.ship < plan.plan.ships.len() => Some(column([
                                divider(),
                                editor::section(
                                    ed,
                                    &plan.plan.ships[ed.ship],
                                    &orbit::mission::check(self.eph, &plan.plan.ships[ed.ship]),
                                    &self.selection,
                                ),
                            ])),
                            _ => self.options_section(cx).map(|o| column([divider(), o])),
                        },
                    ]
                    .into_iter()
                    .flatten(),
                )
                .width(Size::Fixed(panel::WIDTH))
                .height(Size::Fill(1.0)),
            ])
            .width(Size::Fill(1.0))
            .height(Size::Fill(1.0)),
            _ => map,
        };
        let focus_name = self.eph.body(self.camera.focus).name.as_str();
        let ships = self.plan.as_ref().map_or(&[][..], |p| &p.ships[..]);
        let axis_px = cx.rect_of_key(timeline::AXIS_KEY).map_or(1000.0, |r| r.w);
        let main = column([
            row([
                text("Interplanetary sandbox").bold(),
                text("Focus").muted().caption(),
                select_trigger(FOCUS_KEY, focus_name).width(Size::Fixed(180.0)),
                text("Frame").muted().caption(),
                select_trigger(FRAME_KEY, self.frame_name()).width(Size::Fixed(270.0)),
                text("Plan").muted().caption(),
                select_trigger(PLAN_KEY, self.plan_name()).width(Size::Fixed(200.0)),
                // The keys' hints give way where the bar is short of room.
                text("space pause · , . warp · [ ] events · - = exposure · p plan · ctrl+s save")
                    .muted()
                    .caption()
                    .ellipsis()
                    .text_align(TextAlign::End)
                    .width(Size::Fill(1.0)),
                text(format!(
                    "{} · {}",
                    orbit::mission::when(self.clock.t),
                    self.clock.rate_text()
                ))
                .caption()
                .tabular_numerals(),
                text(output).muted().caption(),
                settings::trigger(),
            ])
            .gap(tokens::SPACE_3)
            .padding(Sides::xy(tokens::SPACE_4, tokens::SPACE_2))
            .align(Align::Center),
            middle,
            timeline::strip(
                self.eph,
                &self.timeline,
                axis_px,
                ships,
                self.plan.as_ref().map(|p| &p.windows),
                self.clock.t,
                self.selected,
            ),
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
        if self.plan_open {
            let mut options: Vec<(String, String)> = crate::plan::plans_in(&missions_dir())
                .into_iter()
                .map(|(name, _)| (name.clone(), name))
                .collect();
            options.push((NEW_PLAN.to_string(), "New plan".to_string()));
            layers.push(select_menu(PLAN_KEY, options));
        }
        if self.settings_open {
            layers.push(settings::menu(&self.settings));
        }
        if let Some(f) = self.editor.as_ref().and_then(|e| e.menu) {
            layers.push(editor::place_menu(f, Places::standard()));
        }
        stack(layers).width(Size::Fill(1.0)).height(Size::Fill(1.0))
    }

    fn on_event(&mut self, event: UiEvent, cx: &EventCx) {
        let clicked = matches!(event.kind, UiEventKind::Click | UiEventKind::Activate);
        if self.editor_event(&event) {
            return;
        }
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
        } else if event.is_hotkey("panel") {
            self.panel_open = !self.panel_open;
        } else if event.is_hotkey("save") {
            self.save();
        } else if event.is_hotkey("undo") {
            self.undo();
        } else if clicked && let Some(s) = event.route().and_then(panel::parse_ship_key) {
            self.editor = match &self.editor {
                Some(ed) if ed.ship == s => None,
                _ => Some(Editor {
                    ship: s,
                    ..Editor::default()
                }),
            };
        } else if event.is_click_or_activate(panel::ADD_SHIP_KEY)
            && let Some(plan) = self.plan.as_mut()
        {
            self.undo.push(plan.plan.clone());
            let name = format!("Ship {}", plan.plan.ships.len() + 1);
            plan.add_ship(editor::new_ship(name, self.clock.t, Places::standard()));
            let s = plan.plan.ships.len() - 1;
            self.editor = Some(Editor {
                ship: s,
                ..Editor::default()
            });
        } else if event.kind == UiEventKind::Click
            && event.route() == Some(VIEWPORT_KEY)
            && let (Some(rect), Some((x, y))) = (event.target_rect(), event.pointer)
        {
            self.clicked_map(Vec2::new(x - rect.x, y - rect.y));
        } else if clicked
            && let Some((s, k)) = event.route().and_then(panel::parse_item_key)
            && let Some(item) = self
                .plan
                .as_ref()
                .and_then(|p| p.ships.get(s)?.items.get(k))
        {
            self.clock.seek(item.start);
            self.select(Some((s, k)));
        } else if clicked
            && event.route() == Some(options::CHART_KEY)
            && let (Some(rect), Some(at)) = (event.target_rect(), event.pointer)
        {
            self.clicked_chart(at, rect);
        } else if clicked
            && let Some(i) = event.route().and_then(options::parse_point_key)
            && let Some(leg) = self.leg
            && let Some(days) = self
                .leg_item(leg)
                .and_then(|(_, item)| Some(item.options.get(i)?.days))
        {
            self.pin(leg, None, vec![days]);
        } else if event.is_click_or_activate(options::UNPIN_KEY)
            && let Some(leg) = self.leg
        {
            self.pin(leg, None, Vec::new());
        } else if event.is_click_or_activate(options::SWEEP_KEY)
            && let Some((s, step)) = self.leg
            && let Some(offered) = self
                .leg_item((s, step))
                .map(|(_, item)| item.options.iter().map(|c| c.days).collect::<Vec<_>>())
            && let Some(plan) = self.plan.as_mut()
        {
            plan.sweep(s, step, options::sweep_durations(&offered));
        } else if clicked
            && let Some(i) = event.route().and_then(options::parse_swept_key)
            && let Some(leg) = self.leg
            && let Some(days) = self.plan.as_ref().and_then(|p| {
                let sweep = p.sweeps.get(&leg)?;
                Some(sweep.options.as_ref()?.as_ref().ok()?.get(i)?.days)
            })
        {
            self.pin(leg, None, vec![days]);
        } else if self.timeline_event(&event, cx) {
            // The timeline took it.
        } else if self.settings.apply(&event) {
            // One of the menu's sliders moved.
        } else if event.is_click_or_activate(FOCUS_KEY) {
            let open = !self.focus_open;
            self.close_menus();
            self.focus_open = open;
        } else if event.is_click_or_activate(FRAME_KEY) {
            let open = !self.frame_open;
            self.close_menus();
            self.frame_open = open;
        } else if event.is_click_or_activate(PLAN_KEY) {
            let open = !self.plan_open;
            self.close_menus();
            self.plan_open = open;
        } else if event.is_click_or_activate(settings::KEY) {
            let open = !self.settings_open;
            self.close_menus();
            self.settings_open = open;
        } else if event.is_click_or_activate(&format!("{FOCUS_KEY}:dismiss")) {
            self.focus_open = false;
        } else if event.is_click_or_activate(&format!("{FRAME_KEY}:dismiss")) {
            self.frame_open = false;
        } else if event.is_click_or_activate(&format!("{PLAN_KEY}:dismiss")) {
            self.plan_open = false;
        } else if event.is_click_or_activate(&format!("{}:dismiss", settings::KEY)) {
            self.settings_open = false;
        } else if event.kind == UiEventKind::Escape {
            self.close_menus();
        } else if clicked && let Some(id) = event.route_suffix(&format!("{PLAN_KEY}:option")) {
            let dir = missions_dir();
            let loaded = match id {
                NEW_PLAN => Ok(Loaded::empty("New plan", crate::plan::new_plan_path(&dir))),
                id => Loaded::open(&dir.join(format!("{id}.toml"))),
            };
            match loaded {
                Ok(plan) => self.load(plan),
                Err(why) => self.toasts.push(ToastSpec::error(why)),
            }
            self.plan_open = false;
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

    /// The wheel over the timeline's axis or lanes zooms it about the
    /// pointer; over its gutter, the lanes scroll.
    fn on_wheel_event(&mut self, event: UiEvent, cx: &EventCx) -> bool {
        let over = event.route().is_some_and(|r| {
            r == timeline::AXIS_KEY
                || r == timeline::LANES_KEY
                || timeline::parse_bar_key(r).is_some()
        });
        match (
            over,
            cx.rect_of_key(timeline::AXIS_KEY),
            event.pointer_x(),
            event.wheel_delta(),
        ) {
            (true, Some(axis), Some(x), Some((_, dy))) if axis.w > 0.0 => {
                let f = f64::from((x - axis.x) / axis.w);
                self.timeline.wheel(f, -dy / crate::host::WHEEL_LINE);
                true
            }
            _ => false,
        }
    }

    fn selection(&self) -> Selection {
        self.selection.clone()
    }

    fn drain_toasts(&mut self) -> Vec<ToastSpec> {
        std::mem::take(&mut self.toasts)
    }

    /// While a text field is being typed into, only the chords with
    /// Ctrl: damascene matches hotkeys before the field sees the key.
    fn hotkeys(&self) -> Vec<(KeyChord, String)> {
        let ctrl = vec![
            (KeyChord::ctrl('s'), "save".into()),
            (KeyChord::ctrl('z'), "undo".into()),
        ];
        if self.editor.as_ref().is_some_and(|e| e.typing) {
            return ctrl;
        }
        let mut keys: Vec<(KeyChord, String)> = vec![
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
            (KeyChord::vim('p'), "panel".into()),
        ];
        keys.extend(ctrl);
        keys
    }

    fn drain_scroll_requests(&mut self) -> Vec<ScrollRequest> {
        std::mem::take(&mut self.scrolls)
    }

    fn theme(&self) -> Theme {
        Theme::damascene_dark()
    }
}

/// Where plans are opened from and new ones saved: `data/missions/`.
fn missions_dir() -> std::path::PathBuf {
    crate::data_dir().join("missions")
}
