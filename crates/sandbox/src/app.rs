//! The sandbox's UI (M1c.md, step 2 plan, item 5; step 3 plan, items 6
//! and 7): a top bar with the focus and the clock, the map view, and a
//! status line.

use crate::bodies;
use crate::camera::MapCamera;
use crate::clock::Clock;
use damascene_core::prelude::*;
use damascene_core::surface::AppTexture;
use orbit::ephem::{BodyId, Ephemeris};
use orbit::time::Epoch;
use render::Timings;

/// The map view's key: input over it goes to the camera.
pub const VIEWPORT_KEY: &str = "viewport";

/// The focus select's key.
const FOCUS_KEY: &str = "focus";

/// The longest step of wall time a frame advances, s.
const MAX_STEP_S: f64 = 0.25;

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
}

pub struct Shell {
    pub eph: &'static Ephemeris,
    /// The scene's target, once its size is known.
    pub viewport: Option<AppTexture>,
    pub camera: MapCamera,
    pub clock: Clock,
    pub stats: Stats,
    /// The bodies the focus select offers.
    bodies: Vec<BodyId>,
    focus_open: bool,
}

impl Shell {
    /// Focused on Earth at `t`.
    pub fn new(t: Epoch) -> Shell {
        let eph = Ephemeris::standard();
        let earth = eph.lookup("earth").expect("Earth is in the ephemeris");
        Shell {
            eph,
            viewport: None,
            camera: MapCamera::new(eph, earth),
            clock: Clock::new(t, eph.span()),
            stats: Stats::default(),
            bodies: bodies::drawn(eph),
            focus_open: false,
        }
    }

    /// Advances the clock and the camera by `dt` seconds of wall time,
    /// at most [`MAX_STEP_S`]: after a stall or a hidden window, time
    /// resumes rather than leaping.
    pub fn tick(&mut self, dt: f64) {
        let dt = dt.min(MAX_STEP_S);
        self.clock.tick(dt);
        self.camera.tick(dt);
    }

    /// What the map view shows now.
    pub fn scene(&self) -> render::Scene {
        let (eph, t) = (self.eph, self.clock.t);
        render::Scene {
            camera: Some(self.camera.camera(eph, t)),
            sun: eph.barycentric_position(eph.sun(), t),
            spheres: self
                .bodies
                .iter()
                .filter_map(|&b| bodies::sphere(eph, b, t))
                .collect(),
            ..Default::default()
        }
    }

    fn status(&self) -> String {
        let ms = |x: Option<f32>| x.map_or("–".to_string(), |x| format!("{x:.2} ms"));
        let frame = ms(self.stats.frame_ms);
        let gpu = match (self.stats.timestamps, self.stats.gpu) {
            (false, _) => "no GPU timestamps".to_string(),
            (true, None) => "GPU –".to_string(),
            (true, Some(t)) => format!("GPU scene {}, frame {}", ms(Some(t.scene)), ms(t.frame)),
        };
        format!("frame {frame} · {gpu}")
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
        format!("{distance} above {}", self.eph.body(self.camera.focus).name)
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
        let main = column([
            row([
                text("Interplanetary sandbox").bold(),
                text("Focus").muted().caption(),
                select_trigger(FOCUS_KEY, focus_name).width(Size::Fixed(220.0)),
                spacer(),
                text(format!(
                    "{} · {}",
                    orbit::mission::when(self.clock.t),
                    self.clock.rate_text()
                ))
                .caption()
                .tabular_numerals(),
                text("space pauses · , and . set the rate")
                    .muted()
                    .caption(),
                text(output).muted().caption(),
            ])
            .gap(tokens::SPACE_3)
            .padding(Sides::xy(tokens::SPACE_4, tokens::SPACE_2))
            .align(Align::Center),
            map,
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
        stack(layers).width(Size::Fill(1.0)).height(Size::Fill(1.0))
    }

    fn on_event(&mut self, event: UiEvent, _cx: &EventCx) {
        if event.is_hotkey("pause") {
            self.clock.paused = !self.clock.paused;
        } else if event.is_hotkey("faster") {
            self.clock.faster();
        } else if event.is_hotkey("slower") {
            self.clock.slower();
        } else if event.is_click_or_activate(FOCUS_KEY) {
            self.focus_open = !self.focus_open;
        } else if event.is_click_or_activate(&format!("{FOCUS_KEY}:dismiss")) {
            self.focus_open = false;
        } else if matches!(event.kind, UiEventKind::Click | UiEventKind::Activate)
            && let Some(id) = event.route_suffix(&format!("{FOCUS_KEY}:option"))
            && let Some(b) = self.eph.lookup(id)
        {
            self.camera.focus_on(self.eph, b, self.clock.t);
            self.focus_open = false;
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
        ]
    }

    fn theme(&self) -> Theme {
        Theme::damascene_dark()
    }
}
