//! Layout definition for AK980 Max

use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KeyDef {
    pub index: u8,
    pub label: String,
    pub hid: u8,
    #[serde(default)]
    pub code: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Layout {
    pub pid: u16,
    pub keys: Vec<KeyDef>,
}

impl Layout {
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn ak980max() -> &'static Layout {
        static LAYOUT: OnceLock<Layout> = OnceLock::new();
        LAYOUT.get_or_init(|| {
            Self::parse(include_str!("../../../data/ak980max/layout.json"))
                .expect("data/ak980max/layout.json is valid")
        })
    }

    pub fn size(&self) -> (f32, f32) {
        self.keys.iter().fold((0.0f32, 0.0f32), |(w, h), k| {
            (w.max(k.x + k.w), h.max(k.y + k.h))
        })
    }

    #[allow(dead_code)]
    pub fn key(&self, index: u8) -> Option<&KeyDef> {
        self.keys.iter().find(|k| k.index == index)
    }
}
