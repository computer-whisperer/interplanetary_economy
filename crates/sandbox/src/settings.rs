//! The settings menu: a popover under the top bar's gear, with the
//! departures from realism (M1c.md, Realism), each in stops, 0 being
//! true to life.

use crate::look;
use damascene_core::prelude::*;

/// The gear's key, and the popover's (`{KEY}:dismiss` closes it).
pub const KEY: &str = "settings";

/// The sliders' width, logical pixels.
const SLIDER_PX: f32 = 240.0;

/// What the settings menu sets: stops over the truth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// The sky's (the stars' and the Milky Way's) over the scene's
    /// exposure.
    pub background_lift: i32,
    /// Bodies' drawn as points of light, over the scene's exposure.
    pub body_lift: i32,
    /// Earth's night lights', over their radiance.
    pub night_lights: i32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            background_lift: look::BACKGROUND_LIFT,
            body_lift: look::BODY_LIFT,
            night_lights: look::NIGHT_LIGHTS,
        }
    }
}

/// A setting's slider: its key, its name, what it lifts, its most, and
/// its field.
pub struct Control {
    pub key: &'static str,
    pub name: &'static str,
    pub about: &'static str,
    pub max: i32,
    pub field: fn(&mut Settings) -> &mut i32,
}

/// The menu's settings, in its order.
pub const CONTROLS: [Control; 3] = [
    Control {
        key: "settings:background",
        name: "Background lift",
        about: "The stars and the Milky Way, over the scene's exposure.",
        max: look::MAX_LIFT,
        field: |s| &mut s.background_lift,
    },
    Control {
        key: "settings:bodies",
        name: "Body lift",
        about: "Planets, moons and asteroids seen as points, over the scene's exposure.",
        max: look::MAX_LIFT,
        field: |s| &mut s.body_lift,
    },
    Control {
        key: "settings:night",
        name: "Night lights",
        about: "Earth's city lights, over their radiance.",
        max: look::MAX_NIGHT_LIGHTS,
        field: |s| &mut s.night_lights,
    },
];

/// 2^`stops`.
fn factor(stops: i32) -> f32 {
    f64::from(stops).exp2() as f32
}

impl Settings {
    /// The look's departures (`render::Look`'s `sky_lift`, `body_lift`
    /// and `night_lift`), as factors.
    pub fn lifts(&self) -> [f32; 3] {
        [
            factor(self.background_lift),
            factor(self.body_lift),
            factor(self.night_lights),
        ]
    }

    /// Takes `event` if it is one of the menu's sliders' (dragged, clicked
    /// or stepped by the arrow keys), by whole stops; whether it was.
    pub fn apply(&mut self, event: &UiEvent) -> bool {
        for c in &CONTROLS {
            let max = c.max as f32;
            let field = (c.field)(self);
            let mut value = *field as f32 / max;
            if slider::apply_event(&mut value, event, c.key, 1.0 / max, 4.0 / max) {
                *field = (value * max).round() as i32;
                return true;
            }
        }
        false
    }
}

/// The top bar's gear, which opens the menu.
pub fn trigger() -> El {
    icon_button(IconName::Settings).key(KEY).tooltip("Settings")
}

/// The menu, open under the gear.
pub fn menu(settings: &Settings) -> El {
    let mut settings = *settings;
    let mut rows = vec![text("Settings").bold()];
    for c in &CONTROLS {
        let stops = *(c.field)(&mut settings);
        rows.push(
            column([
                row([
                    text(c.name),
                    spacer(),
                    text(format!("{stops:+} stops")).tabular_numerals(),
                ])
                .align(Align::Center),
                slider(c.key, stops as f32 / c.max as f32).width(Size::Fixed(SLIDER_PX)),
                text(c.about)
                    .muted()
                    .caption()
                    .wrap_text()
                    .width(Size::Fixed(SLIDER_PX)),
            ])
            .gap(tokens::SPACE_1),
        );
    }
    rows.push(text("Each at 0 is true to life.").muted().caption());
    popover(
        KEY,
        Anchor::below_key(KEY),
        popover_panel([column(rows)
            .gap(tokens::SPACE_3)
            .padding(Sides::all(tokens::SPACE_3))]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lifts_are_in_stops() {
        let mut s = Settings::default();
        assert_eq!(
            s.lifts(),
            [look::BACKGROUND_LIFT, look::BODY_LIFT, look::NIGHT_LIGHTS].map(|n| 2f32.powi(n))
        );
        for c in &CONTROLS {
            *(c.field)(&mut s) = 0;
        }
        assert_eq!(s.lifts(), [1.0; 3]);
    }
}
