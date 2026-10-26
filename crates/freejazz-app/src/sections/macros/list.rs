//! The list of macros (up to 100, ids 0-99) and the capacity bar.

use eframe::egui::{self, CornerRadius, RichText, Sense, Ui, vec2};
use freejazz_protocol::blocks::keys::{KeyAction, KeyKind, KeyMap};
use freejazz_protocol::blocks::macros::{MAX_MACRO_ID, Macro, Macros};

use crate::ui::theme::{self, Palette};

/// Lowest id not used yet, if any.
pub fn next_free_id(macros: &Macros) -> Option<u8> {
    (0..=MAX_MACRO_ID).find(|id| macros.slots.iter().all(|m| m.id != *id))
}

/// Adds an empty macro with the lowest free id and returns it.
pub fn add(macros: &mut Macros) -> Option<u8> {
    let id = next_free_id(macros)?;
    macros.slots.push(Macro {
        id,
        ..Macro::default()
    });
    macros.slots.sort_by_key(|m| m.id);
    Some(id)
}

/// Removes macro `id` (its pointer becomes 0 when written).
pub fn remove(macros: &mut Macros, id: u8) {
    macros.slots.retain(|m| m.id != id);
}

/// Keys that run macro `id`.
pub fn uses(keys: &KeyMap, id: u8) -> usize {
    keys.0
        .iter()
        .filter(|a| matches!(a.kind(), KeyKind::Macro { id: m, .. } if m == id))
        .count()
}

/// Layout indexes of the keys that run macro `id`.
pub fn keys_using(keys: &KeyMap, id: u8) -> Vec<u8> {
    (0u8..)
        .zip(keys.0.iter())
        .filter(|(_, a)| matches!(a.kind(), KeyKind::Macro { id: m, .. } if m == id))
        .map(|(i, _)| i)
        .collect()
}

/// Deletes macro `id` and gives its keys their own function back.
pub fn delete_releasing(macros: &mut Macros, keys: &mut KeyMap, id: u8) {
    for i in keys_using(keys, id) {
        keys.0[usize::from(i)] = KeyAction::default();
    }
    remove(macros, id);
}

/// « 12 actions · 52 octets ».
pub fn summary(m: &Macro) -> String {
    let n = m.actions.len();
    format!(
        "{n} action{} · {} octets",
        if n > 1 { "s" } else { "" },
        4 + 4 * n
    )
}

/// Used / available bytes as a bar; the colour warns near the cap and flags an excess.
pub fn capacity_bar(ui: &mut Ui, p: &Palette, used: usize, capacity: usize) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(320.0, 8.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(4), p.border);
        let ratio = (used as f32 / capacity as f32).min(1.0);
        let color = if used > capacity {
            p.danger
        } else if ratio > 0.85 {
            p.warning
        } else {
            p.accent
        };
        let mut fill = rect;
        fill.set_width(rect.width() * ratio);
        painter.rect_filled(fill, CornerRadius::same(4), color);
        ui.label(
            RichText::new(format!("{used} / {capacity} octets"))
                .font(theme::regular(12.5))
                .color(if used > capacity {
                    p.danger
                } else {
                    p.text_dim
                }),
        );
    });
}

/// A selectable line of the list; true when clicked.
pub fn line(ui: &mut Ui, p: &Palette, title: &str, detail: &str, on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    let fill = if on {
        p.accent_soft
    } else if response.hovered() {
        p.surface_hi
    } else {
        p.surface
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(theme::RADIUS_CONTROL),
        fill,
        egui::Stroke::new(1.0, if on { p.accent } else { p.border }),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.left_center() + vec2(14.0, -8.0),
        egui::Align2::LEFT_CENTER,
        title,
        theme::strong(13.5),
        p.text,
    );
    painter.text(
        rect.left_center() + vec2(14.0, 9.0),
        egui::Align2::LEFT_CENTER,
        detail,
        theme::regular(12.0),
        p.text_dim,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use freejazz_protocol::blocks::macros::MacroAction;

    #[test]
    fn ids_fill_the_gaps_and_stop_at_100() {
        let mut m = Macros::default();
        assert_eq!(add(&mut m), Some(0));
        assert_eq!(add(&mut m), Some(1));
        remove(&mut m, 0);
        assert_eq!(add(&mut m), Some(0));
        while add(&mut m).is_some() {}
        assert_eq!(m.slots.len(), 100);
        assert_eq!(next_free_id(&m), None);
    }

    #[test]
    fn deleting_a_macro_resets_the_keys_that_run_it() {
        let mut macros = Macros::default();
        let id = add(&mut macros).unwrap();
        let mut keys = KeyMap::default();
        let run = KeyAction::from_kind(KeyKind::Macro { id, p2: 0, p3: 0 });
        keys.0[5] = run;
        keys.0[70] = run;
        keys.0[6] = KeyAction::from_kind(KeyKind::Consumer(0xE9));
        assert_eq!(keys_using(&keys, id), vec![5, 70]);
        delete_releasing(&mut macros, &mut keys, id);
        assert!(macros.slots.is_empty());
        assert_eq!(keys.0[5], KeyAction::default());
        assert_eq!(keys.0[70], KeyAction::default());
        assert_ne!(keys.0[6], KeyAction::default(), "other keys are left alone");
    }

    #[test]
    fn summary_counts_the_header() {
        let m = Macro {
            id: 3,
            actions: vec![MacroAction::default(); 2],
            raw_header: None,
        };
        assert_eq!(summary(&m), "2 actions · 12 octets");
    }
}
