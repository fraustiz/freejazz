//! Painting the per-key colours: selection, fill, gradient.

use freejazz_protocol::blocks::{CustomLeds, Rgb};

use crate::designs::mix;
use crate::keyboard::Layout;

pub fn paint(leds: &mut CustomLeds, keys: &[u8], colour: Rgb) {
    for &k in keys {
        leds.0[usize::from(k)] = colour;
    }
}

/// Every key of the keyboard.
pub fn fill_all(leds: &mut CustomLeds, layout: &Layout, colour: Rgb) {
    for k in &layout.keys {
        leds.0[usize::from(k.index)] = colour;
    }
}

/// `from` on the leftmost selected key to `to` on the rightmost, by key position.
pub fn gradient(leds: &mut CustomLeds, layout: &Layout, keys: &[u8], from: Rgb, to: Rgb) {
    let centres: Vec<(u8, f32)> = keys
        .iter()
        .filter_map(|&i| layout.key(i).map(|k| (i, k.x + k.w / 2.0)))
        .collect();
    let min = centres.iter().map(|c| c.1).fold(f32::MAX, f32::min);
    let max = centres.iter().map(|c| c.1).fold(f32::MIN, f32::max);
    for (i, x) in centres {
        let t = if max > min {
            (x - min) / (max - min)
        } else {
            0.0
        };
        leds.0[usize::from(i)] = mix(from, to, t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgb = Rgb { r: 255, g: 0, b: 0 };
    const BLUE: Rgb = Rgb { r: 0, g: 0, b: 255 };

    fn key(hid: u8) -> u8 {
        Layout::ak980max()
            .keys
            .iter()
            .find(|k| k.hid == hid)
            .unwrap()
            .index
    }

    #[test]
    fn paint_touches_only_the_selection() {
        let mut leds = CustomLeds::default();
        let a = key(0x04);
        paint(&mut leds, &[a], RED);
        assert_eq!(leds.0.iter().filter(|c| **c == RED).count(), 1);
        assert_eq!(leds.0[usize::from(a)], RED);
    }

    #[test]
    fn fill_leaves_empty_slots_off() {
        let layout = Layout::ak980max();
        let mut leds = CustomLeds::default();
        fill_all(&mut leds, layout, RED);
        assert_eq!(
            leds.0.iter().filter(|c| **c == RED).count(),
            layout.keys.len()
        );
    }

    #[test]
    fn gradient_runs_left_to_right() {
        let layout = Layout::ak980max();
        let (q, w, p) = (key(0x14), key(0x1A), key(0x13)); // Q (left), W, P (right)
        let mut leds = CustomLeds::default();
        gradient(&mut leds, layout, &[p, q, w], RED, BLUE);
        assert_eq!(leds.0[usize::from(q)], RED);
        assert_eq!(leds.0[usize::from(p)], BLUE);
        let mid = leds.0[usize::from(w)];
        assert!(mid.r > 0 && mid.r < 255 && mid.b > 0, "{mid:?}");
        // one key: its start colour
        gradient(&mut leds, layout, &[q], BLUE, RED);
        assert_eq!(leds.0[usize::from(q)], BLUE);
    }
}
