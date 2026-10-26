//! Trigger point and Rapid Trigger edits on a set of keys of the magnetic map.

use freejazz_protocol::blocks::magnetic::{MagneticKey, MagneticMap, ModelLimits};

/// Whole hundredths of a millimetre (the device step).
pub fn round2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

/// The value all the keys share, `None` when they differ (or there is no key).
pub fn common<T: PartialEq>(
    map: &MagneticMap,
    keys: &[u8],
    f: impl Fn(&MagneticKey) -> T,
) -> Option<T> {
    let mut it = keys.iter().map(|&i| f(&map.0[usize::from(i)]));
    let first = it.next()?;
    it.all(|v| v == first).then_some(first)
}

pub fn rapid_on(k: &MagneticKey) -> bool {
    k.press_rt_mm != 0.0 || k.release_rt_mm != 0.0
}

fn each(map: &mut MagneticMap, keys: &[u8], mut f: impl FnMut(&mut MagneticKey)) {
    for &i in keys {
        f(&mut map.0[usize::from(i)]);
    }
}

pub fn set_trigger(map: &mut MagneticMap, keys: &[u8], mm: f32, limits: &ModelLimits) {
    let mm = round2(mm).clamp(limits.min_trigger_trip, limits.max_trigger_trip);
    each(map, keys, |k| k.trigger_mm = mm);
}

/// Turning it on gives the switch's default sensitivity to a key that has none; turning
/// it off zeroes both sensitivities.
pub fn set_rapid(map: &mut MagneticMap, keys: &[u8], on: bool, limits: &ModelLimits) {
    each(map, keys, |k| {
        if !on {
            k.press_rt_mm = 0.0;
            k.release_rt_mm = 0.0;
        } else if !rapid_on(k) {
            let default = limits.axis(k.axis).map_or(0.1, |a| a.sensitivity_value);
            k.press_rt_mm = default;
            k.release_rt_mm = default;
        }
    });
}

/// Press / release sensitivity, clamped to each key's switch; keys without Rapid
/// Trigger are left alone.
pub fn set_sensitivity(
    map: &mut MagneticMap,
    keys: &[u8],
    press: bool,
    mm: f32,
    limits: &ModelLimits,
) {
    each(map, keys, |k| {
        if !rapid_on(k) {
            return;
        }
        let Some(axis) = limits.axis(k.axis) else {
            return;
        };
        let v = round2(mm).clamp(axis.min_sensitivity, axis.max_sensitivity);
        if press {
            k.press_rt_mm = v;
        } else {
            k.release_rt_mm = v;
        }
    });
}

pub fn set_whole_fast(map: &mut MagneticMap, keys: &[u8], on: bool) {
    each(map, keys, |k| k.whole_fast = on);
}

/// Value printed on the keycap: the trigger point, `0,8`, `1,2`, `0,85`.
pub fn badge(k: &MagneticKey) -> Option<String> {
    if k.is_empty() {
        return None;
    }
    let s = format!("{:.2}", k.trigger_mm);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    Some(s.replace('.', ","))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map() -> MagneticMap {
        let mut m = MagneticMap::default();
        for (i, k) in m.0.iter_mut().enumerate().take(4) {
            *k = MagneticKey {
                axis: 1,
                trigger_mm: 1.2,
                other_flags: i as u8 * 4,
                ..Default::default()
            };
        }
        m
    }

    #[test]
    fn trigger_is_clamped_to_the_model_and_encodes() {
        let (mut m, l) = (map(), ModelLimits::ak980max());
        set_trigger(&mut m, &[0, 1], 0.8, l);
        assert_eq!(m.0[0].trigger_mm, 0.8);
        assert_eq!(m.0[2].trigger_mm, 1.2);
        set_trigger(&mut m, &[0], 9.0, l);
        assert_eq!(m.0[0].trigger_mm, 3.3);
        set_trigger(&mut m, &[0], 0.0, l);
        assert_eq!(m.0[0].trigger_mm, 0.1);
        set_trigger(&mut m, &[0], 0.8, l);
        assert!(m.validate_changes(&map(), l).is_ok());
        assert!(m.encode(0).is_ok());
        // other flags survive
        assert_eq!(m.0[1].other_flags, 4);
    }

    #[test]
    fn rapid_trigger_on_uses_the_default_then_off_zeroes() {
        let (mut m, l) = (map(), ModelLimits::ak980max());
        assert_eq!(common(&m, &[0, 1], rapid_on), Some(false));
        set_rapid(&mut m, &[0], true, l);
        assert_eq!((m.0[0].press_rt_mm, m.0[0].release_rt_mm), (0.1, 0.1));
        assert_eq!(common(&m, &[0, 1], rapid_on), None);
        set_sensitivity(&mut m, &[0, 1], true, 0.25, l);
        assert_eq!(m.0[0].press_rt_mm, 0.25);
        assert_eq!(m.0[1].press_rt_mm, 0.0, "no Rapid Trigger, untouched");
        set_sensitivity(&mut m, &[0], false, 99.0, l);
        assert_eq!(m.0[0].release_rt_mm, 3.3);
        set_rapid(&mut m, &[0], false, l);
        assert!(!rapid_on(&m.0[0]));
    }

    #[test]
    fn badges_print_the_trigger() {
        let m = map();
        assert_eq!(badge(&m.0[0]).as_deref(), Some("1,2"));
        assert_eq!(badge(&m.0[9]), None);
        let mut k = m.0[0];
        k.trigger_mm = 0.85;
        assert_eq!(badge(&k).as_deref(), Some("0,85"));
        k.trigger_mm = 2.0;
        assert_eq!(badge(&k).as_deref(), Some("2"));
    }
}
