//! Rappy Snappy (`0C 00 usage1 usage2`): the more pressed key of the pair wins.

use freejazz_protocol::blocks::keys::{KeyAction, KeyKind, KeyMap};
use freejazz_protocol::blocks::magnetic::{MagneticMap, ModelLimits};

use super::socd::assign_pair;
use crate::keyboard::KeyDef;

pub fn action(first_hid: u8, second_hid: u8) -> KeyAction {
    KeyAction::from_kind(KeyKind::Rs([0, first_hid, second_hid]))
}

/// A pair on `(a, b)`; like the site's save, it also sets the trigger point of both
/// keys (protocol.md 6.3) to that of the last key `b` (the model default if it has none).
pub fn set(map: &mut KeyMap, magnetic: &mut MagneticMap, a: &KeyDef, b: &KeyDef) {
    assign_pair(map, a, b, action(a.hid, b.hid));
    let last = magnetic.0[usize::from(b.index)].trigger_mm;
    let trigger = if last == 0.0 {
        ModelLimits::ak980max().trigger_trip_value
    } else {
        last
    };
    for key in [a, b] {
        magnetic.0[usize::from(key.index)].trigger_mm = trigger;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_trigger_point_of_both_keys_is_written_like_the_site() {
        use crate::keyboard::Layout;
        use freejazz_protocol::blocks::magnetic::MagneticKey;
        let key = |hid| {
            Layout::ak980max()
                .keys
                .iter()
                .find(|k| k.hid == hid)
                .unwrap()
        };
        let (a, b) = (key(0x04), key(0x07));
        let (mut map, mut mag) = (KeyMap::default(), MagneticMap::default());
        let (ia, ib) = (usize::from(a.index), usize::from(b.index));
        mag.0[ia] = MagneticKey {
            axis: 1,
            trigger_mm: 0.5,
            press_rt_mm: 0.2,
            ..Default::default()
        };
        mag.0[ib] = MagneticKey {
            axis: 2,
            trigger_mm: 2.0,
            ..Default::default()
        };
        set(&mut map, &mut mag, a, b);
        assert_eq!(mag.0[ia].trigger_mm, 2.0, "the last key's trigger point");
        assert_eq!(mag.0[ib].trigger_mm, 2.0);
        assert_eq!((mag.0[ia].axis, mag.0[ia].press_rt_mm), (1, 0.2));
        // no trigger point: the model default (1.2 mm = 120 at rtPrecision 0 or 1)
        mag.0[ib].trigger_mm = 0.0;
        set(&mut map, &mut mag, a, b);
        assert_eq!(mag.encode(1).unwrap()[ia * 8 + 2], 120);
        assert_eq!(map.0[ia], map.0[ib]);
    }

    #[test]
    fn bytes() {
        assert_eq!(action(0x04, 0x07).raw, [0x0C, 0, 0x04, 0x07]);
    }
}
