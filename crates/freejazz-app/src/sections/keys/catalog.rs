//! What a key can be given: the entries of each tab of the picker, from `keycodes.json`
//! (keyboard, mouse, multimedia) and from the macros stored on the keyboard.

use freejazz_protocol::blocks::keys::{KeyAction, KeyKind};
use freejazz_protocol::blocks::macros::Macros;

use crate::keyboard::KeyDef;
use crate::keyboard::labels::{Labels, keycodes};

/// A key with no function: the keyboard sends usage 0 (nothing).
pub const DISABLED: KeyAction = KeyAction {
    raw: [0x02, 0, 0, 0],
};

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub label: String,
    /// Lower-case text the search box looks in.
    pub search: String,
    pub action: KeyAction,
}

impl Entry {
    fn new(label: String, extra: &str, action: KeyAction) -> Self {
        let search = format!("{label} {extra}").to_lowercase();
        Self {
            label,
            search,
            action,
        }
    }
}

/// Keyboard usages, named as the current Windows layout types them.
pub fn keyboard(labels: &Labels) -> Vec<Entry> {
    keycodes()
        .iter()
        .filter(|k| k.page == "KEYBOARD")
        .map(|k| {
            let usage = k.value as u8;
            let label = labels
                .hid(usage)
                .map(|l| l.replace('\n', " "))
                .unwrap_or_else(|| k.fr.clone().unwrap_or_else(|| k.name.clone()));
            Entry::new(
                label,
                &k.name,
                KeyAction::from_kind(KeyKind::Keyboard {
                    modifiers: 0,
                    usage,
                }),
            )
        })
        .collect()
}

pub fn media() -> Vec<Entry> {
    keycodes()
        .iter()
        .filter(|k| k.page == "CONSUMER_KEY")
        .map(|k| {
            let label = k.fr.clone().unwrap_or_else(|| k.name.clone());
            Entry::new(label, "", KeyAction::from_kind(KeyKind::Consumer(k.value)))
        })
        .collect()
}

pub fn mouse() -> Vec<Entry> {
    keycodes()
        .iter()
        .filter(|k| k.page == "MOUSE")
        .map(|k| {
            let label = k.fr.clone().unwrap_or_else(|| k.name.clone());
            let kind = KeyKind::Mouse {
                kind: k.param1.unwrap_or(1),
                value: k.value as u8,
            };
            Entry::new(label, "", KeyAction::from_kind(kind))
        })
        .collect()
}

/// The macros on the keyboard, played once per press.
pub fn macros(macros: &Macros) -> Vec<Entry> {
    macros
        .slots
        .iter()
        .map(|m| {
            let n = m.actions.len();
            let label = format!(
                "Macro {} ({n} action{})",
                m.id,
                if n > 1 { "s" } else { "" }
            );
            let kind = KeyKind::Macro {
                id: m.id,
                p2: 0,
                p3: 0,
            };
            Entry::new(label, "", KeyAction::from_kind(kind))
        })
        .collect()
}

/// A key that no longer has its own function: anything but « Par défaut » and the
/// usage the keycap carries (the site counts that one as unmodified).
pub fn is_remapped(action: KeyAction, key: &KeyDef) -> bool {
    match action.kind() {
        KeyKind::Default => false,
        KeyKind::Keyboard {
            modifiers: 0,
            usage,
        } => usage != key.hid,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard::Layout;

    fn labels() -> Labels {
        Labels::build(|_| None)
    }

    #[test]
    fn the_catalog_comes_from_keycodes_json() {
        let kb = keyboard(&labels());
        assert_eq!(kb.len(), 107);
        let media = media();
        let volume = media.iter().find(|e| e.label == "Volume +").unwrap();
        assert_eq!(volume.action.raw, [0x03, 0xE9, 0x00, 0x00]);
        let mouse = mouse();
        assert_eq!(mouse.len(), 7);
        let left = mouse.iter().find(|e| e.label.contains("gauche")).unwrap();
        assert_eq!(left.action.raw, [0x01, 1, 1, 0]);
        let wheel_down = mouse.iter().find(|e| e.label.contains("bas")).unwrap();
        assert_eq!(wheel_down.action.raw, [0x01, 3, 255, 0]);
        // the A key sends usage 4
        assert!(kb.iter().any(|e| e.action.raw == [0x02, 0, 0x04, 0]));
    }

    #[test]
    fn macros_are_listed_by_id() {
        let snapshot = crate::mock::fixture();
        let entries = macros(&snapshot.macros);
        assert_eq!(entries.len(), snapshot.macros.slots.len());
        for (e, m) in entries.iter().zip(&snapshot.macros.slots) {
            assert_eq!(e.action.raw, [0x06, m.id, 0, 0]);
        }
    }

    #[test]
    fn remapped_means_not_its_own_function() {
        let layout = Layout::ak980max();
        let z = layout.keys.iter().find(|k| k.hid == 0x1D).unwrap();
        assert!(!is_remapped(KeyAction::default(), z));
        assert!(!is_remapped(
            KeyAction::from_kind(KeyKind::Keyboard {
                modifiers: 0,
                usage: 0x1D
            }),
            z
        ));
        // the site's encoder writes a KEYBOARD action without code as 02 00 00 00
        assert_eq!(DISABLED.raw, [0x02, 0, 0, 0]);
        assert!(is_remapped(DISABLED, z));
        assert!(is_remapped(
            KeyAction::from_kind(KeyKind::Consumer(0xE9)),
            z
        ));
        assert!(is_remapped(
            KeyAction {
                raw: [0x44, 1, 2, 3]
            },
            z
        ));
    }
}
