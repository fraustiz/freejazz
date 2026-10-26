//! `GET_KEY` 0x12 / `SET_KEY` 0x22 (and the Fn layer 0x16 / 0x26), 512 bytes:
//! 128 actions of 4 bytes `[page, p1, p2, p3]` (protocol.md 6.3).

use serde::{Deserialize, Serialize};

use super::dks::MAX_DKS_SLOT;
use super::macros::MAX_MACRO_ID;
use super::{KEY_SLOTS, check_len};
use crate::error::{Error, Result};

pub const KEY_MAP_LEN: usize = KEY_SLOTS * 4;

/// One key slot as stored by the keyboard. The bytes are the source of truth;
/// [`KeyAction::kind`] is a view of them, and every 4-byte value round-trips (pages
/// the app does not model decode as [`KeyKind::Unknown`] carrying the bytes).
/// JSON: the [`KeyKind`] (tagged `{ "type", "data" }`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(try_from = "KeyKind", into = "KeyKind")]
pub struct KeyAction {
    pub raw: [u8; 4],
}

/// Decoded key action. Layouts are the site's encoder; byte values the site never
/// writes on a known page (e.g. a non-zero 4th byte on KEYBOARD) decode as `Unknown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum KeyKind {
    /// `00 00 00 00`: the key's own function.
    Default,
    /// `01 kind value 00`: kind 1 button (value 1/2/4/8/16), 3 wheel (1 up, 255 down).
    Mouse { kind: u8, value: u8 },
    /// `02 modifiers usage 00`: HID usage (page 0x07); the site writes `modifiers = 0`
    /// but a mask (1 LCtrl ... 64 RAlt) read from the keyboard is kept.
    Keyboard { modifiers: u8, usage: u8 },
    /// `03 lo hi 00`: consumer usage (page 0x0C).
    Consumer(u16),
    /// `06 id mode count`: macro 0-99; mode 0 normal, 1 repeat `count` times, 2 until
    /// pressed again.
    Macro { id: u8, p2: u8, p3: u8 },
    /// `07 p1 p2 p3`: key combination (not decoded by the site).
    Cb([u8; 3]),
    /// `08 slot 00 00`: DKS table entry 0-63.
    Dks(u8),
    /// `09 hold tap time`: Mod-Tap, time in 10 ms units.
    Mt([u8; 3]),
    /// `0A usage 00 00`: toggle.
    Tgl(u8),
    /// `0B mode usage1 usage2`.
    Socd([u8; 3]),
    /// `0C 00 usage1 usage2`: Rappy Snappy.
    Rs([u8; 3]),
    /// `0D v>>16 v>>8 v`: 24-bit big-endian function id.
    Func(u32),
    /// `0E p1 p2 00`.
    End([u8; 2]),
    /// `0F p1 p2 p3`: multi-point travel.
    Mpt([u8; 3]),
    /// `0x80|first>>8, first, second>>8, second`: `first` is 15 bits.
    FuncV2 { first: u16, second: u16 },
    /// Anything else, bytes kept as read.
    Unknown([u8; 4]),
}

impl KeyAction {
    pub fn kind(&self) -> KeyKind {
        let [page, a, b, c] = self.raw;
        match (page, a, b, c) {
            (0x00, 0, 0, 0) => KeyKind::Default,
            (0x01, kind, value, 0) => KeyKind::Mouse { kind, value },
            (0x02, modifiers, usage, 0) => KeyKind::Keyboard { modifiers, usage },
            (0x03, lo, hi, 0) => KeyKind::Consumer(u16::from_le_bytes([lo, hi])),
            (0x06, id, p2, p3) => KeyKind::Macro { id, p2, p3 },
            (0x07, ..) => KeyKind::Cb([a, b, c]),
            (0x08, slot, 0, 0) => KeyKind::Dks(slot),
            (0x09, ..) => KeyKind::Mt([a, b, c]),
            (0x0A, usage, 0, 0) => KeyKind::Tgl(usage),
            (0x0B, ..) => KeyKind::Socd([a, b, c]),
            (0x0C, ..) => KeyKind::Rs([a, b, c]),
            (0x0D, ..) => KeyKind::Func(u32::from_be_bytes([0, a, b, c])),
            (0x0E, p1, p2, 0) => KeyKind::End([p1, p2]),
            (0x0F, ..) => KeyKind::Mpt([a, b, c]),
            (0x80.., ..) => KeyKind::FuncV2 {
                first: u16::from_be_bytes([page & 0x7F, a]),
                second: u16::from_be_bytes([b, c]),
            },
            _ => KeyKind::Unknown(self.raw),
        }
    }

    /// Encodes `kind` as the site does. Bits that do not fit are dropped (`Func` above
    /// 24 bits, `FuncV2::first` above 15 bits): use `KeyAction::try_from` to refuse them.
    pub fn from_kind(kind: KeyKind) -> Self {
        let raw = match kind {
            KeyKind::Default => [0; 4],
            KeyKind::Mouse { kind, value } => [0x01, kind, value, 0],
            KeyKind::Keyboard { modifiers, usage } => [0x02, modifiers, usage, 0],
            KeyKind::Consumer(code) => {
                let [lo, hi] = code.to_le_bytes();
                [0x03, lo, hi, 0]
            }
            KeyKind::Macro { id, p2, p3 } => [0x06, id, p2, p3],
            KeyKind::Cb([a, b, c]) => [0x07, a, b, c],
            KeyKind::Dks(slot) => [0x08, slot, 0, 0],
            KeyKind::Mt([a, b, c]) => [0x09, a, b, c],
            KeyKind::Tgl(usage) => [0x0A, usage, 0, 0],
            KeyKind::Socd([a, b, c]) => [0x0B, a, b, c],
            KeyKind::Rs([a, b, c]) => [0x0C, a, b, c],
            KeyKind::Func(v) => {
                let [_, a, b, c] = v.to_be_bytes();
                [0x0D, a, b, c]
            }
            KeyKind::End([p1, p2]) => [0x0E, p1, p2, 0],
            KeyKind::Mpt([a, b, c]) => [0x0F, a, b, c],
            KeyKind::FuncV2 { first, second } => {
                let [fh, fl] = first.to_be_bytes();
                let [sh, sl] = second.to_be_bytes();
                [0x80 | (fh & 0x7F), fl, sh, sl]
            }
            KeyKind::Unknown(raw) => raw,
        };
        Self { raw }
    }
}

impl TryFrom<KeyKind> for KeyAction {
    type Error = Error;

    /// Like [`KeyAction::from_kind`], but refuses values that do not fit the 4 bytes.
    fn try_from(kind: KeyKind) -> Result<Self> {
        match kind {
            KeyKind::Func(v) if v > 0x00FF_FFFF => Err(Error::Validation(format!(
                "Fonction 0x{v:X} hors limites (24 bits au plus)"
            ))),
            KeyKind::FuncV2 { first, .. } if first > 0x7FFF => Err(Error::Validation(format!(
                "Fonction V2 : première valeur {first} hors limites (0–32767)"
            ))),
            _ => Ok(Self::from_kind(kind)),
        }
    }
}

impl From<KeyAction> for KeyKind {
    fn from(a: KeyAction) -> Self {
        a.kind()
    }
}

/// The 128 key slots of a layer; slot `i` is the key of layout index `i`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyMap(#[serde(with = "super::fixed_array")] pub [KeyAction; KEY_SLOTS]);

impl Default for KeyMap {
    fn default() -> Self {
        Self([KeyAction::default(); KEY_SLOTS])
    }
}

impl KeyMap {
    pub fn decode(b: &[u8]) -> Result<Self> {
        check_len("GET_KEY", b, KEY_MAP_LEN)?;
        let mut map = Self::default();
        for (action, bytes) in map.0.iter_mut().zip(b.chunks_exact(4)) {
            action.raw.copy_from_slice(bytes);
        }
        Ok(map)
    }

    pub fn encode(&self) -> Vec<u8> {
        self.0.iter().flat_map(|a| a.raw).collect()
    }

    /// UI limits on every slot: macro id at most 99, DKS slot at most 63.
    pub fn validate(&self) -> Result<()> {
        self.0
            .iter()
            .enumerate()
            .try_for_each(|(i, a)| check_action(i, a))
    }

    /// [`KeyMap::validate`] restricted to the slots that differ from `baseline` (the
    /// layer read from the keyboard): actions the keyboard itself holds are not refused.
    pub fn validate_changes(&self, baseline: &KeyMap) -> Result<()> {
        let changed = self.0.iter().zip(baseline.0.iter()).enumerate();
        changed
            .filter(|(_, (a, b))| a != b)
            .try_for_each(|(i, (a, _))| check_action(i, a))
    }
}

fn check_action(i: usize, action: &KeyAction) -> Result<()> {
    match action.kind() {
        KeyKind::Macro { id, .. } if id > MAX_MACRO_ID => Err(Error::Validation(format!(
            "Touche {i} : macro {id} hors limites (0–{MAX_MACRO_ID})"
        ))),
        KeyKind::Dks(slot) if slot > MAX_DKS_SLOT => Err(Error::Validation(format!(
            "Touche {i} : emplacement DKS {slot} hors limites (0–{MAX_DKS_SLOT})"
        ))),
        _ => Ok(()),
    }
}
