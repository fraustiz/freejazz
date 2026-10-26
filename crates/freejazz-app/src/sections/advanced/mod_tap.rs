//! Mod-Tap (`09 hold tap time`, protocol.md 6.3): one key, two behaviours.

use freejazz_protocol::blocks::keys::{KeyAction, KeyKind};

pub const MIN_MS: u16 = 10;
pub const MAX_MS: u16 = 1000;
pub const DEFAULT_MS: u16 = 400;

/// The action: `hold` is sent while the key is held, `tap` on a short press; the time
/// is rounded to 10 ms and kept within 10–1000 ms.
pub fn action(hold: u8, tap: u8, ms: u16) -> KeyAction {
    let units = (ms.clamp(MIN_MS, MAX_MS) + 5) / 10;
    KeyAction::from_kind(KeyKind::Mt([hold, tap, units.min(100) as u8]))
}

/// `(hold, tap, ms)` of a Mod-Tap action.
pub fn parts(action: KeyAction) -> Option<(u8, u8, u16)> {
    match action.kind() {
        KeyKind::Mt([hold, tap, units]) => Some((hold, tap, u16::from(units) * 10)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_lock_tap_caps_hold_ctrl() {
        // CapsLock 0x39 on tap, Left Ctrl 0xE0 on hold, 400 ms
        let a = action(0xE0, 0x39, 400);
        assert_eq!(a.raw, [0x09, 0xE0, 0x39, 40]);
        assert_eq!(parts(a), Some((0xE0, 0x39, 400)));
    }

    #[test]
    fn time_is_clamped_and_rounded() {
        assert_eq!(action(1, 2, 0).raw[3], 1);
        assert_eq!(action(1, 2, 5000).raw[3], 100);
        assert_eq!(action(1, 2, 123).raw[3], 12);
        assert_eq!(parts(KeyAction::default()), None);
    }
}
