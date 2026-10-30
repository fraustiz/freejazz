//! Catalog of HID, Multimedia/Consumer, and Mouse actions for key remapping

use freejazz_protocol::blocks::keys::{KeyAction, KeyKind};
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
pub struct RawKeycode {
    pub page: String,
    pub name: String,
    pub value: u16,
    #[serde(default)]
    pub param1: Option<u8>,
    #[serde(default)]
    pub fr: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KeyActionOption {
    pub label: String,
    pub category: &'static str,
    pub action: KeyAction,
}

fn raw_keycodes() -> &'static [RawKeycode] {
    static CODES: OnceLock<Vec<RawKeycode>> = OnceLock::new();
    CODES.get_or_init(|| {
        serde_json::from_str(include_str!("../../../data/ak980max/keycodes.json"))
            .expect("keycodes.json is valid")
    })
}

pub fn standard_keyboard_actions() -> Vec<KeyActionOption> {
    raw_keycodes()
        .iter()
        .filter(|k| k.page == "KEYBOARD")
        .map(|k| {
            let usage = k.value as u8;
            let label = k.fr.clone().unwrap_or_else(|| k.name.clone());
            KeyActionOption {
                label,
                category: "Clavier",
                action: KeyAction::from_kind(KeyKind::Keyboard {
                    modifiers: 0,
                    usage,
                }),
            }
        })
        .collect()
}

pub fn media_actions() -> Vec<KeyActionOption> {
    raw_keycodes()
        .iter()
        .filter(|k| k.page == "CONSUMER_KEY")
        .map(|k| {
            let label = k.fr.clone().unwrap_or_else(|| k.name.clone());
            KeyActionOption {
                label,
                category: "Multimédia",
                action: KeyAction::from_kind(KeyKind::Consumer(k.value)),
            }
        })
        .collect()
}

pub fn mouse_actions() -> Vec<KeyActionOption> {
    raw_keycodes()
        .iter()
        .filter(|k| k.page == "MOUSE")
        .map(|k| {
            let label = k.fr.clone().unwrap_or_else(|| k.name.clone());
            let kind = KeyKind::Mouse {
                kind: k.param1.unwrap_or(1),
                value: k.value as u8,
            };
            KeyActionOption {
                label,
                category: "Souris",
                action: KeyAction::from_kind(kind),
            }
        })
        .collect()
}

pub fn action_display_name(action: &KeyAction) -> String {
    match action.kind() {
        KeyKind::Default => "Par défaut (action d'origine)".to_string(),
        KeyKind::Keyboard { modifiers, usage } => {
            if modifiers == 0 && usage == 0 {
                return "Désactivée (aucune action)".to_string();
            }
            if let Some(k) = raw_keycodes().iter().find(|k| k.page == "KEYBOARD" && k.value == usage as u16) {
                k.fr.clone().unwrap_or_else(|| k.name.clone())
            } else {
                format!("Touche HID 0x{:02X}", usage)
            }
        }
        KeyKind::Consumer(code) => {
            if let Some(k) = raw_keycodes().iter().find(|k| k.page == "CONSUMER_KEY" && k.value == code) {
                k.fr.clone().unwrap_or_else(|| k.name.clone())
            } else {
                format!("Multimédia 0x{:X}", code)
            }
        }
        KeyKind::Mouse { kind, value } => {
            if let Some(k) = raw_keycodes().iter().find(|k| k.page == "MOUSE" && k.value == value as u16 && k.param1.unwrap_or(1) == kind) {
                k.fr.clone().unwrap_or_else(|| k.name.clone())
            } else {
                format!("Souris {}", value)
            }
        }
        KeyKind::Socd(_) => "Paire SOCD (Snap Tap)".to_string(),
        KeyKind::Mt(_) => "Mod-Tap (Tap / Hold)".to_string(),
        KeyKind::Tgl(u) => format!("Toggle (Verrouillage HID 0x{:02X})", u),
        KeyKind::Dks(slot) => format!("DKS (Palier slot #{})", slot),
        _ => "Action personnalisée".to_string(),
    }
}
