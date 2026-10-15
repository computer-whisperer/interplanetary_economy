//! The settings menu: a popover under the top bar's gear, with the
//! sky's lift over the scene's exposure.

use crate::look;
use damascene_core::prelude::*;

/// The gear's key, and the popover's (`{KEY}:dismiss` closes it).
pub const KEY: &str = "settings";

/// The background lift's slider's key.
pub const LIFT_KEY: &str = "settings:lift";

/// The slider's width, logical pixels.
const SLIDER_PX: f32 = 220.0;

/// What the settings menu sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Stops the sky shows over the scene's exposure, 0 to
    /// [`look::MAX_LIFT`].
    pub background_lift: i32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            background_lift: look::BACKGROUND_LIFT,
        }
    }
}

impl Settings {
    /// What the sky's exposure, and points of light's, is multiplied by
    /// (`render::Look::sky_lift`).
    pub fn sky_lift(&self) -> f32 {
        f64::from(self.background_lift).exp2() as f32
    }

    /// Takes `event` if it is the menu's slider's (dragged, clicked or
    /// stepped by the arrow keys), by whole stops; whether it was.
    pub fn apply(&mut self, event: &UiEvent) -> bool {
        let max = look::MAX_LIFT as f32;
        let mut value = self.background_lift as f32 / max;
        if !slider::apply_event(&mut value, event, LIFT_KEY, 1.0 / max, 4.0 / max) {
            return false;
        }
        self.background_lift = (value * max).round() as i32;
        true
    }
}

/// The top bar's gear, which opens the menu.
pub fn trigger() -> El {
    icon_button(IconName::Settings).key(KEY).tooltip("Settings")
}

/// The menu, open under the gear.
pub fn menu(settings: &Settings) -> El {
    let lift = settings.background_lift;
    popover(
        KEY,
        Anchor::below_key(KEY),
        popover_panel([column([
            text("Settings").bold(),
            row([
                text("Background lift"),
                spacer(),
                text(format!("{lift:+} stops")).tabular_numerals(),
            ])
            .align(Align::Center),
            slider(LIFT_KEY, lift as f32 / look::MAX_LIFT as f32).width(Size::Fixed(SLIDER_PX)),
            text("The stars, the Milky Way and bodies seen as points, over the scene's exposure. 0 is true to life.")
                .muted()
                .caption()
                .wrap_text()
                .width(Size::Fixed(SLIDER_PX)),
        ])
        .gap(tokens::SPACE_2)
        .padding(Sides::all(tokens::SPACE_3))]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lift_is_in_stops() {
        let mut s = Settings::default();
        assert_eq!(s.sky_lift(), 2f32.powi(look::BACKGROUND_LIFT));
        s.background_lift = 0;
        assert_eq!(s.sky_lift(), 1.0);
    }
}
