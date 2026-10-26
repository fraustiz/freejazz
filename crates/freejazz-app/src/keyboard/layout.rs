//! Physical layout of the AK980 Max (`data/ak980max/layout.json`): key positions in
//! key units and the HID usage each key sends by default.

use std::sync::OnceLock;

use serde::Deserialize;

/// One physical key. `index` is the key's slot in the keyboard's tables (key map,
/// LEDs, magnetic settings); `hid` the usage it sends by default.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct KeyDef {
    pub index: u8,
    /// English name from the site, used when Windows gives no label.
    pub label: String,
    pub hid: u8,
    #[serde(default)]
    pub code: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Layout {
    pub pid: u16,
    pub keys: Vec<KeyDef>,
}

impl Layout {
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// The AK980 Max, embedded.
    pub fn ak980max() -> &'static Layout {
        static LAYOUT: OnceLock<Layout> = OnceLock::new();
        LAYOUT.get_or_init(|| {
            Layout::parse(include_str!("../../../../data/ak980max/layout.json"))
                .expect("data/ak980max/layout.json is valid")
        })
    }

    /// Width and height of the board in key units.
    pub fn size(&self) -> (f32, f32) {
        self.keys.iter().fold((0.0f32, 0.0f32), |(w, h), k| {
            (w.max(k.x + k.w), h.max(k.y + k.h))
        })
    }

    pub fn key(&self, index: u8) -> Option<&KeyDef> {
        self.keys.iter().find(|k| k.index == index)
    }

    /// Key indices in reading order: row by row, left to right.
    pub fn row_order(&self) -> Vec<u8> {
        let mut keys: Vec<&KeyDef> = self.keys.iter().collect();
        keys.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
        keys.into_iter().map(|k| k.index).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ak980max_has_101_distinct_keys_without_overlap() {
        let layout = Layout::ak980max();
        assert_eq!(layout.pid, 0x8043);
        assert_eq!(layout.keys.len(), 101);
        let mut seen = std::collections::BTreeSet::new();
        for k in &layout.keys {
            assert!(seen.insert(k.index), "index {} twice", k.index);
        }
        let (w, h) = layout.size();
        assert!(
            (w - 20.8).abs() < 1e-3 && (h - 6.0).abs() < 1e-3,
            "{w} x {h}"
        );
        for (i, a) in layout.keys.iter().enumerate() {
            for b in &layout.keys[i + 1..] {
                let overlap = a.x + 0.01 < b.x + b.w
                    && b.x + 0.01 < a.x + a.w
                    && a.y + 0.01 < b.y + b.h
                    && b.y + 0.01 < a.y + a.h;
                assert!(!overlap, "{} overlaps {}", a.label, b.label);
            }
        }
    }

    #[test]
    fn row_order_reads_left_to_right_then_down() {
        let order = Layout::ak980max().row_order();
        assert_eq!(&order[..3], &[0, 1, 2]); // Esc, F1, F2
        let q = order.iter().position(|&i| i == 33).unwrap(); // Q (QWERTY name)
        let w = order.iter().position(|&i| i == 34).unwrap();
        let a = order.iter().position(|&i| i == 49).unwrap();
        assert!(q < w && w < a);
        assert_eq!(order.len(), 101);
    }
}
