//! SOCD (`0B mode usage1 usage2`): the same action sits on both keys of a pair.

use freejazz_protocol::blocks::keys::{KeyAction, KeyKind, KeyMap};
use freejazz_protocol::blocks::magnetic::{MagneticMap, ModelLimits};

use crate::keyboard::KeyDef;

/// Which key wins when both are down.
pub const MODES: [(u8, &str); 4] = [
    (3, "Dernière entrée prioritaire"),
    (1, "Première touche prioritaire"),
    (2, "Seconde touche prioritaire"),
    (4, "Annulation (aucune des deux)"),
];

pub fn action(mode: u8, first_hid: u8, second_hid: u8) -> KeyAction {
    KeyAction::from_kind(KeyKind::Socd([mode, first_hid, second_hid]))
}

/// Puts `action` on both keys.
pub fn assign_pair(map: &mut KeyMap, a: &KeyDef, b: &KeyDef, action: KeyAction) {
    map.0[usize::from(a.index)] = action;
    map.0[usize::from(b.index)] = action;
}

/// A SOCD pair on `(a, b)`, as `mode`; like the site's save, it also rewrites the
/// magnetic entry of both keys (protocol.md 6.3): both get `a`'s switch type, trigger
/// point (the model default if `a` has none), `isWholeFast` and `isRampageMode`; the
/// Rapid Trigger sensitivities are `a`'s (default 0.1 mm for a 0), both 0 when it is
/// off, the release one only kept when press and release differed. Each key keeps its
/// own other flag bits.
pub fn set(map: &mut KeyMap, magnetic: &mut MagneticMap, a: &KeyDef, b: &KeyDef, mode: u8) {
    assign_pair(map, a, b, action(mode, a.hid, b.hid));
    let limits = ModelLimits::ak980max();
    let first = magnetic.0[usize::from(a.index)];
    let default_rt = limits
        .axis(first.axis)
        .map_or(0.1, |l| l.sensitivity_value);
    let rt_on = first.press_rt_mm > 0.0 || first.release_rt_mm > 0.0;
    let separate = rt_on && first.press_rt_mm != first.release_rt_mm;
    let or_default = |v: f32| if v == 0.0 { default_rt } else { v };
    let press = or_default(first.press_rt_mm);
    let release = if separate {
        or_default(first.release_rt_mm)
    } else {
        press
    };
    let trigger = if first.trigger_mm == 0.0 {
        limits.trigger_trip_value
    } else {
        first.trigger_mm
    };
    for key in [a, b] {
        let k = &mut magnetic.0[usize::from(key.index)];
        k.trigger_mm = trigger;
        k.axis = first.axis;
        k.whole_fast = first.whole_fast;
        k.rampage = first.rampage;
        (k.press_rt_mm, k.release_rt_mm) = if rt_on {
            (press, release)
        } else {
            (0.0, 0.0)
        };
    }
}

pub fn mode(action: KeyAction) -> Option<u8> {
    match action.kind() {
        KeyKind::Socd([m, ..]) => Some(m),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard::Layout;

    fn key_with(hid: u8) -> &'static KeyDef {
        Layout::ak980max()
            .keys
            .iter()
            .find(|k| k.hid == hid)
            .unwrap()
    }

    #[test]
    fn the_magnetic_block_of_both_keys_is_written_like_the_site() {
        use freejazz_protocol::blocks::magnetic::MagneticKey;
        let (q, d) = (key_with(0x14), key_with(0x07));
        let (mut map, mut mag) = (KeyMap::default(), MagneticMap::default());
        mag.0[usize::from(q.index)] = MagneticKey {
            axis: 2,
            whole_fast: true,
            trigger_mm: 0.0,
            press_rt_mm: 0.0,
            release_rt_mm: 0.3,
            ..Default::default()
        };
        mag.0[usize::from(d.index)] = MagneticKey {
            axis: 3,
            trigger_mm: 2.0,
            other_flags: 4,
            ..Default::default()
        };
        set(&mut map, &mut mag, q, d, 3);
        let want = MagneticKey {
            axis: 2,
            whole_fast: true,
            trigger_mm: 1.2,
            press_rt_mm: 0.1,
            release_rt_mm: 0.3,
            ..Default::default()
        };
        assert_eq!(mag.0[usize::from(q.index)], want);
        assert_eq!(
            mag.0[usize::from(d.index)],
            MagneticKey {
                other_flags: 4,
                ..want
            }
        );
        // bytes: type, flags, trigger 120, press 100, release 300 (rtPrecision 1)
        let bytes = mag.encode(1).unwrap();
        let at = usize::from(q.index) * 8;
        assert_eq!(bytes[at..at + 8], [2, 1, 120, 0, 100, 0, 0x2C, 1]);

        // equal sensitivities stay equal; Rapid Trigger off stays off
        let mut mag = MagneticMap::default();
        mag.0[usize::from(q.index)] = MagneticKey {
            axis: 1,
            trigger_mm: 0.8,
            press_rt_mm: 0.2,
            release_rt_mm: 0.2,
            ..Default::default()
        };
        set(&mut map, &mut mag, q, d, 3);
        assert_eq!(mag.0[usize::from(d.index)].release_rt_mm, 0.2);
        assert_eq!(mag.0[usize::from(d.index)].trigger_mm, 0.8);
        mag.0[usize::from(q.index)].press_rt_mm = 0.0;
        mag.0[usize::from(q.index)].release_rt_mm = 0.0;
        set(&mut map, &mut mag, q, d, 3);
        assert_eq!(mag.0[usize::from(d.index)].press_rt_mm, 0.0);
        assert_eq!(mag.0[usize::from(d.index)].release_rt_mm, 0.0);
    }

    #[test]
    fn a_pair_is_written_on_both_keys() {
        let (q, d) = (key_with(0x14), key_with(0x07));
        let mut map = KeyMap::default();
        set(&mut map, &mut MagneticMap::default(), q, d, 4);
        let want = [0x0B, 4, 0x14, 0x07];
        assert_eq!(map.0[usize::from(q.index)].raw, want);
        assert_eq!(map.0[usize::from(d.index)].raw, want);
        assert_eq!(mode(map.0[usize::from(q.index)]), Some(4));
    }
}
