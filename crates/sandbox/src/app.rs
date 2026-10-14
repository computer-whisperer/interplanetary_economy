//! The sandbox's UI (M1c.md, step 2 plan, item 5): a top bar, the map
//! view, and a status line.

use damascene_core::prelude::*;
use damascene_core::surface::AppTexture;
use render::Timings;

/// The map view's key: input over it goes to the camera.
pub const VIEWPORT_KEY: &str = "viewport";

/// Stands in for the camera until step 3: accumulates what input asks
/// of it. As in KSP's map view, a right drag orbits, a middle drag pans
/// and the wheel zooms.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraStub {
    /// Degrees.
    pub yaw: f32,
    pub pitch: f32,
    /// Logical pixels.
    pub pan: (f32, f32),
    /// Wheel lines in (positive) or out.
    pub zoom: f32,
    dragging: Option<(PointerButton, (f32, f32))>,
}

impl CameraStub {
    /// Degrees of orbit per logical pixel dragged.
    const ORBIT: f32 = 0.3;

    /// A press over the map view.
    pub fn press(&mut self, button: PointerButton, at: (f32, f32)) {
        if matches!(button, PointerButton::Secondary | PointerButton::Middle) {
            self.dragging = Some((button, at));
        }
    }

    /// The pointer moved; `true` if the camera did.
    pub fn moved(&mut self, at: (f32, f32)) -> bool {
        let Some((button, from)) = self.dragging else {
            return false;
        };
        let (dx, dy) = (at.0 - from.0, at.1 - from.1);
        match button {
            PointerButton::Secondary => {
                self.yaw = (self.yaw + dx * Self::ORBIT).rem_euclid(360.0);
                self.pitch = (self.pitch - dy * Self::ORBIT).clamp(-90.0, 90.0);
            }
            _ => self.pan = (self.pan.0 + dx, self.pan.1 + dy),
        }
        self.dragging = Some((button, at));
        true
    }

    pub fn release(&mut self, button: PointerButton) {
        if self.dragging.is_some_and(|(b, _)| b == button) {
            self.dragging = None;
        }
    }

    /// Ends any drag: the window lost focus.
    pub fn cancel(&mut self) {
        self.dragging = None;
    }

    pub fn wheel(&mut self, lines: f32) {
        self.zoom += lines;
    }
}

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

#[derive(Default)]
pub struct Shell {
    /// The scene's target, once its size is known.
    pub viewport: Option<AppTexture>,
    pub camera: CameraStub,
    pub stats: Stats,
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
        column([
            row([
                text("Interplanetary sandbox").bold(),
                spacer(),
                text(output).muted().caption(),
            ])
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
        .height(Size::Fill(1.0))
    }

    fn on_event(&mut self, _event: UiEvent, _cx: &EventCx) {}

    fn theme(&self) -> Theme {
        Theme::damascene_dark()
    }
}

impl Shell {
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
        let c = &self.camera;
        format!(
            "camera stub: yaw {:.0}°, pitch {:.0}°, pan ({:.0}, {:.0}), zoom {:+.0}",
            c.yaw, c.pitch, c.pan.0, c.pan.1, c.zoom
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A right drag orbits, a middle drag pans, a left drag leaves the
    /// camera alone (it picks, from step 5), and the wheel zooms.
    #[test]
    fn camera_stub_follows_the_buttons() {
        let mut c = CameraStub::default();
        c.press(PointerButton::Secondary, (100.0, 100.0));
        assert!(c.moved((110.0, 90.0)));
        c.release(PointerButton::Secondary);
        assert_eq!((c.yaw, c.pitch), (3.0, 3.0));
        c.press(PointerButton::Middle, (0.0, 0.0));
        c.moved((5.0, -7.0));
        c.release(PointerButton::Middle);
        assert_eq!(c.pan, (5.0, -7.0));
        c.press(PointerButton::Primary, (0.0, 0.0));
        assert!(!c.moved((50.0, 50.0)));
        c.wheel(-2.0);
        assert_eq!(c.zoom, -2.0);
        assert_eq!((c.yaw, c.pitch, c.pan), (3.0, 3.0, (5.0, -7.0)));
    }
}
