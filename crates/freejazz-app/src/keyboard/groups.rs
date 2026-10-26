//! Ready-made key groups for quick selection. Groups that depend on what a key types
//! (letters, the gaming cluster) are computed from the labels of the current Windows
//! layout; the others from the usages of the layout.

use std::collections::{BTreeSet, HashMap};

use super::layout::Layout;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    All,
    Letters,
    Digits,
    FRow,
    Arrows,
    Numpad,
    Modifiers,
    Gaming,
}

impl Group {
    pub const ALL: [Group; 8] = [
        Group::All,
        Group::Letters,
        Group::Digits,
        Group::FRow,
        Group::Arrows,
        Group::Numpad,
        Group::Modifiers,
        Group::Gaming,
    ];

    /// Identifier for `--select`.
    pub fn id(self) -> &'static str {
        match self {
            Group::All => "all",
            Group::Letters => "letters",
            Group::Digits => "digits",
            Group::FRow => "f_row",
            Group::Arrows => "arrows",
            Group::Numpad => "numpad",
            Group::Modifiers => "modifiers",
            Group::Gaming => "gaming",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.id() == id)
    }

    pub fn title(self) -> &'static str {
        match self {
            Group::All => "Tout",
            Group::Letters => "Lettres",
            Group::Digits => "Chiffres",
            Group::FRow => "F1–F12",
            Group::Arrows => "Flèches",
            Group::Numpad => "Pavé num.",
            Group::Modifiers => "Modificateurs",
            Group::Gaming => "Jeu",
        }
    }

    /// `labels`: label of each key by index ([`super::labels::Labels::for_layout`]).
    pub fn keys(self, layout: &Layout, labels: &HashMap<u8, String>) -> BTreeSet<u8> {
        match self {
            Group::All => all(layout),
            Group::Letters => letters(layout, labels),
            Group::Digits => digits(layout),
            Group::FRow => f_row(layout),
            Group::Arrows => arrows(layout),
            Group::Numpad => numpad(layout),
            Group::Modifiers => modifiers(layout),
            Group::Gaming => gaming(layout, labels),
        }
    }
}

fn by_hid(layout: &Layout, keep: impl Fn(u8) -> bool) -> BTreeSet<u8> {
    layout
        .keys
        .iter()
        .filter(|k| keep(k.hid))
        .map(|k| k.index)
        .collect()
}

pub fn all(layout: &Layout) -> BTreeSet<u8> {
    layout.keys.iter().map(|k| k.index).collect()
}

fn is_letter(label: &str) -> bool {
    let mut chars = label.chars();
    matches!((chars.next(), chars.next()), (Some(c), None) if c.is_alphabetic())
}

/// Keys that type a letter in the current layout (on AZERTY, M sits on the QWERTY `;`).
pub fn letters(layout: &Layout, labels: &HashMap<u8, String>) -> BTreeSet<u8> {
    layout
        .keys
        .iter()
        .filter(|k| labels.get(&k.index).is_some_and(|l| is_letter(l)))
        .map(|k| k.index)
        .collect()
}

/// The number row (1 to 0).
pub fn digits(layout: &Layout) -> BTreeSet<u8> {
    by_hid(layout, |h| (0x1E..=0x27).contains(&h))
}

pub fn f_row(layout: &Layout) -> BTreeSet<u8> {
    by_hid(layout, |h| (0x3A..=0x45).contains(&h))
}

pub fn arrows(layout: &Layout) -> BTreeSet<u8> {
    by_hid(layout, |h| (0x4F..=0x52).contains(&h))
}

/// Num Lock and the keypad.
pub fn numpad(layout: &Layout) -> BTreeSet<u8> {
    by_hid(layout, |h| (0x53..=0x63).contains(&h))
}

/// Ctrl, Maj, Alt, Win (both sides).
pub fn modifiers(layout: &Layout) -> BTreeSet<u8> {
    by_hid(layout, |h| (0xE0..=0xE7).contains(&h))
}

/// The movement keys (ZQSD on AZERTY, WASD on QWERTY), Space, left Shift and left
/// Ctrl. The layout's labels decide which letters: ZQSD when the key above S types Z.
pub fn gaming(layout: &Layout, labels: &HashMap<u8, String>) -> BTreeSet<u8> {
    let above_s = layout
        .keys
        .iter()
        .find(|k| k.hid == 0x1A)
        .and_then(|k| labels.get(&k.index));
    let letters: [&str; 4] = if above_s.is_some_and(|l| l == "Z") {
        ["Z", "Q", "S", "D"]
    } else {
        ["W", "A", "S", "D"]
    };
    let mut keys: BTreeSet<u8> = layout
        .keys
        .iter()
        .filter(|k| {
            labels
                .get(&k.index)
                .is_some_and(|l| letters.contains(&l.as_str()))
        })
        .map(|k| k.index)
        .collect();
    keys.extend(by_hid(layout, |h| matches!(h, 0x2C | 0xE0 | 0xE1)));
    keys
}

/// Keys whose label is one of `wanted` (`--select Z,Q,S,D`).
pub fn by_label(layout: &Layout, labels: &HashMap<u8, String>, wanted: &[&str]) -> BTreeSet<u8> {
    layout
        .keys
        .iter()
        .filter(|k| {
            labels
                .get(&k.index)
                .is_some_and(|l| wanted.iter().any(|w| w.eq_ignore_ascii_case(l)))
        })
        .map(|k| k.index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard::labels::Labels;
    use crate::keyboard::labels::tests::azerty;

    #[test]
    fn gaming_is_zqsd_on_azerty_and_wasd_on_qwerty() {
        let layout = Layout::ak980max();
        let fr = azerty().for_layout(layout);
        // Physical W A S D positions (indices 34, 49, 50, 51) + Space, L-Shift, L-Ctrl.
        let expected: BTreeSet<u8> = [34, 49, 50, 51, 83, 64, 80].into();
        assert_eq!(gaming(layout, &fr), expected);

        let us = Labels::build(|hid| match hid {
            0x04..=0x1D => Some(((b'A' + hid - 0x04) as char).to_string()),
            _ => None,
        })
        .for_layout(layout);
        assert_eq!(gaming(layout, &us), expected);
    }

    #[test]
    fn groups_have_the_expected_sizes() {
        let layout = Layout::ak980max();
        let labels = azerty().for_layout(layout);
        assert_eq!(all(layout).len(), 101);
        assert_eq!(digits(layout).len(), 10);
        assert_eq!(f_row(layout).len(), 12);
        assert_eq!(arrows(layout).len(), 4);
        assert_eq!(numpad(layout).len(), 17);
        assert_eq!(modifiers(layout).len(), 6); // no right Alt / Win: Fn sits there
        // 26 letters; on AZERTY M is on the QWERTY `;` and `,` on the QWERTY M.
        assert_eq!(letters(layout, &labels).len(), 26);
        assert!(!letters(layout, &labels).contains(&71));
        assert!(letters(layout, &labels).contains(&58)); // M on the QWERTY `;`
        let zqsd = by_label(layout, &labels, &["Z", "Q", "S", "D"]);
        assert_eq!(zqsd, [34, 49, 50, 51].into());
        for g in Group::ALL {
            assert_eq!(Group::from_id(g.id()), Some(g));
        }
    }
}
