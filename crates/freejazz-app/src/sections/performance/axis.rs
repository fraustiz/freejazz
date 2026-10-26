//! Switch type of each key.

use freejazz_protocol::blocks::magnetic::{MagneticMap, ModelLimits};

use super::trigger::round2;

/// Switch types offered, as `(value, name)` from the model's `axisList`.
pub fn choices(limits: &ModelLimits) -> Vec<(u8, &'static str)> {
    limits
        .axis_list
        .iter()
        .map(|a| {
            let name = match a.value {
                1 => "Switch magnétique riz",
                2 => "Axe Jade Pro",
                3 => "Axe Magicien",
                _ => "Autre switch",
            };
            (a.value, name)
        })
        .collect()
}

/// Sets the switch type; Rapid Trigger sensitivities are brought into the new
/// switch's limits.
pub fn set_axis(map: &mut MagneticMap, keys: &[u8], axis: u8, limits: &ModelLimits) {
    let Some(new) = limits.axis(axis) else {
        return;
    };
    for &i in keys {
        let k = &mut map.0[usize::from(i)];
        k.axis = axis;
        for v in [&mut k.press_rt_mm, &mut k.release_rt_mm] {
            if *v != 0.0 {
                *v = round2(*v).clamp(new.min_sensitivity, new.max_sensitivity);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freejazz_protocol::blocks::magnetic::MagneticKey;

    #[test]
    fn the_switch_types_come_from_the_model() {
        let c = choices(ModelLimits::ak980max());
        assert_eq!(c.iter().map(|c| c.0).collect::<Vec<_>>(), [1, 2, 3]);
    }

    #[test]
    fn changing_the_switch_keeps_sensitivities_valid() {
        let l = ModelLimits::ak980max();
        let mut m = MagneticMap::default();
        m.0[3] = MagneticKey {
            axis: 1,
            trigger_mm: 1.2,
            press_rt_mm: 0.2,
            release_rt_mm: 0.3,
            ..Default::default()
        };
        set_axis(&mut m, &[3], 2, l);
        assert_eq!(m.0[3].axis, 2);
        assert_eq!((m.0[3].press_rt_mm, m.0[3].release_rt_mm), (0.2, 0.3));
        assert!(m.validate(l).is_ok());
    }
}
