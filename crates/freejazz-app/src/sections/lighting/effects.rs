//! The keyboard's built-in lighting effects (`data/ak980max/effects.json`) and the
//! rules for the effect block.

use std::sync::OnceLock;

use freejazz_protocol::blocks::LedEffect;
use freejazz_protocol::blocks::led::MODE_CUSTOM;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub mode: u8,
    pub fr: String,
    pub has_color: bool,
    #[serde(default)]
    pub color_modes: Vec<u8>,
    pub has_speed: bool,
    pub default_speed: Option<u8>,
    pub has_direction: bool,
    #[serde(default)]
    pub directions: Vec<u8>,
    pub default_direction: Option<u8>,
}

/// The built-in effects (modes 1–19), in the site's order.
pub fn all() -> &'static [Effect] {
    static ALL: OnceLock<Vec<Effect>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut list: Vec<Effect> =
            serde_json::from_str(include_str!("../../../../../data/ak980max/effects.json"))
                .expect("data/ak980max/effects.json is valid");
        list.retain(|e| e.mode != MODE_CUSTOM);
        list
    })
}

pub fn find(mode: u8) -> Option<&'static Effect> {
    all().iter().find(|e| e.mode == mode)
}

/// Mode 128 (custom colours) reads back as 20 (protocol.md 9.7): the same mode.
pub fn is_custom(mode: u8) -> bool {
    mode == MODE_CUSTOM || mode == 20
}

pub const OFF: u8 = 0;

/// Names of the `direction` values.
pub fn direction_name(direction: u8) -> &'static str {
    match direction {
        0 => "Vers la droite",
        1 => "Vers la gauche",
        2 => "Vers le haut",
        _ => "Vers le bas",
    }
}

/// Chooses `effect` like the site: its default direction (0 without one), a speed within
/// 1–5, and a colour mode the effect offers.
pub fn select(led: &mut LedEffect, effect: &Effect) {
    led.mode = effect.mode;
    led.direction = effect.default_direction.unwrap_or(0);
    if effect.has_speed && !(1..=5).contains(&led.speed) {
        led.speed = effect.default_speed.unwrap_or(3);
    }
    if effect.has_color && !effect.color_modes.contains(&led.color_mode) {
        led.color_mode = effect.color_modes.first().copied().unwrap_or(0);
    }
}

/// The colour and colour mode the keyboard reads back after any write are `#ffffff` and
/// RGB whatever was written (protocol.md 9.7): when `led` shows that and the app knows
/// what it last applied, `remembered` replaces it.
pub fn restore_colour(led: &mut LedEffect, remembered: (freejazz_protocol::blocks::Rgb, u8)) {
    const WHITE: freejazz_protocol::blocks::Rgb = freejazz_protocol::blocks::Rgb {
        r: 255,
        g: 255,
        b: 255,
    };
    if (led.rgb, led.color_mode) == (WHITE, 1) && remembered != (WHITE, 1) {
        (led.rgb, led.color_mode) = remembered;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freejazz_protocol::blocks::Rgb;

    #[test]
    fn nineteen_builtin_effects_with_french_names() {
        assert_eq!(all().len(), 19);
        assert_eq!(find(1).unwrap().fr, "Statique constant");
        assert!(find(128).is_none());
        assert!(find(10).unwrap().directions == [2, 3]);
    }

    #[test]
    fn custom_is_128_or_its_read_value_20() {
        assert!(is_custom(128) && is_custom(20));
        assert!(!is_custom(9) && !is_custom(0));
    }

    #[test]
    fn choosing_an_effect_sets_its_defaults() {
        let mut led = crate::mock::fixture().led_effect;
        led.speed = 0;
        select(&mut led, find(10).unwrap());
        assert_eq!((led.mode, led.direction, led.speed), (10, 3, 3));
        select(&mut led, find(6).unwrap()); // no colour: colour mode untouched
        assert_eq!(led.direction, 0);
    }

    #[test]
    fn a_read_back_white_gives_way_to_the_colour_last_applied() {
        let white = Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        let pink = Rgb {
            r: 255,
            g: 0,
            b: 128,
        };
        let mut led = crate::mock::fixture().led_effect;
        (led.rgb, led.color_mode) = (white, 1);
        restore_colour(&mut led, (pink, 0));
        assert_eq!((led.rgb, led.color_mode), (pink, 0));
        // a real colour is left alone
        restore_colour(&mut led, (white, 0));
        assert_eq!(led.rgb, pink);
    }
}
