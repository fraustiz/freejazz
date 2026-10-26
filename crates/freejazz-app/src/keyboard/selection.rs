//! Selection rules, apart from drawing: click toggles a key, Shift+click adds the
//! keys between the last clicked key and this one in reading order, a dragged
//! rectangle adds the keys it touches.

use std::collections::BTreeSet;

use eframe::egui::Rect;

/// Click on `key`. `anchor` is the last key clicked without Shift.
pub fn click(
    selected: &mut BTreeSet<u8>,
    anchor: &mut Option<u8>,
    order: &[u8],
    key: u8,
    shift: bool,
) {
    match (*anchor, shift) {
        (Some(from), true) => selected.extend(range(order, from, key)),
        _ => {
            if !selected.remove(&key) {
                selected.insert(key);
            }
            *anchor = Some(key);
        }
    }
}

/// Keys from `from` to `to` included, in `order` (either direction).
pub fn range(order: &[u8], from: u8, to: u8) -> Vec<u8> {
    let (Some(a), Some(b)) = (
        order.iter().position(|&k| k == from),
        order.iter().position(|&k| k == to),
    ) else {
        return vec![to];
    };
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    order[lo..=hi].to_vec()
}

/// Keys whose rectangle intersects `area`.
pub fn in_rect(keys: impl IntoIterator<Item = (u8, Rect)>, area: Rect) -> BTreeSet<u8> {
    keys.into_iter()
        .filter(|(_, r)| r.intersects(area))
        .map(|(k, _)| k)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};

    #[test]
    fn click_toggles_and_shift_adds_a_range() {
        let order = [10, 11, 12, 13, 14, 20, 21];
        let mut sel = BTreeSet::new();
        let mut anchor = None;
        click(&mut sel, &mut anchor, &order, 12, false);
        assert_eq!(sel, [12].into());
        click(&mut sel, &mut anchor, &order, 20, true);
        assert_eq!(sel, [12, 13, 14, 20].into());
        assert_eq!(anchor, Some(12));
        click(&mut sel, &mut anchor, &order, 10, true); // backwards
        assert_eq!(sel, [10, 11, 12, 13, 14, 20].into());
        click(&mut sel, &mut anchor, &order, 13, false); // toggles off
        assert_eq!(sel, [10, 11, 12, 14, 20].into());
        assert_eq!(anchor, Some(13));
    }

    #[test]
    fn shift_click_without_anchor_toggles() {
        let mut sel = BTreeSet::new();
        let mut anchor = None;
        click(&mut sel, &mut anchor, &[1, 2, 3], 2, true);
        assert_eq!(sel, [2].into());
        assert_eq!(range(&[1, 2, 3], 9, 3), vec![3]);
    }

    #[test]
    fn rectangle_takes_the_keys_it_touches() {
        let key = |x: f32, y: f32| Rect::from_min_size(pos2(x, y), vec2(10.0, 10.0));
        let keys = [
            (1, key(0.0, 0.0)),
            (2, key(10.0, 0.0)),
            (3, key(0.0, 10.0)),
            (4, key(30.0, 30.0)),
        ];
        let area = Rect::from_two_pos(pos2(15.0, 5.0), pos2(5.0, 15.0)); // dragged up-left
        assert_eq!(in_rect(keys, area), [1, 2, 3].into());
        let none = Rect::from_min_size(pos2(21.0, 21.0), vec2(5.0, 5.0));
        assert!(in_rect(keys, none).is_empty());
    }
}
