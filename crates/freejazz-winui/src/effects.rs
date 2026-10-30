//! Built-in lighting effects for Ajazz AK980 Max

use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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

pub fn all_effects() -> &'static [Effect] {
    static ALL: OnceLock<Vec<Effect>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut list: Vec<Effect> =
            serde_json::from_str(include_str!("../../../data/ak980max/effects.json"))
                .expect("data/ak980max/effects.json is valid");
        list.retain(|e| e.mode != 128); // 128 is custom per-key colors
        list
    })
}

pub fn find_effect(mode: u8) -> Option<&'static Effect> {
    all_effects().iter().find(|e| e.mode == mode)
}

pub fn direction_name(dir: u8) -> &'static str {
    match dir {
        0 => "Vers la droite",
        1 => "Vers la gauche",
        2 => "Vers le haut",
        3 => "Vers le bas",
        _ => "Standard",
    }
}
