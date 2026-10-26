//! The keyboard widget: keycaps painted from the layout, scaled to the available
//! width, with hover and an accent selection. Every section shows its per-key values
//! through it (`render` gives each key its fill, label and badge).

use std::collections::BTreeSet;
use std::hash::Hash;
use std::sync::Arc;

use eframe::egui::{
    Color32, CornerRadius, CursorIcon, Galley, Id, Key, Modifiers, Painter, Pos2, Rect, Response,
    Sense, Shadow, Stroke, StrokeKind, Ui, emath::GuiRounding, pos2, vec2,
};

use super::layout::{KeyDef, Layout};
use super::selection;
use crate::ui::theme::{self, Palette};

/// How a section shows one key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeyLook {
    /// Keycap colour (lighting); `None` for the theme's keycap.
    pub fill: Option<Color32>,
    /// Legend; `\n` stacks two lines (shifted character above the plain one).
    pub label: String,
    /// Small text in the top-right corner (a value, a mark).
    pub badge: Option<String>,
}

impl KeyLook {
    pub fn label(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..Default::default()
        }
    }
}

/// Margin of the case around the keys, in key units.
const CASE_PAD: f32 = 0.32;

pub struct KeyboardView {
    id: Id,
    max_unit: f32,
    max_height: Option<f32>,
    shortcuts: bool,
}

impl KeyboardView {
    pub fn new(id_salt: impl Hash + std::fmt::Debug) -> Self {
        Self {
            id: Id::new(("keyboard_view", id_salt)),
            max_unit: 52.0,
            max_height: None,
            shortcuts: true,
        }
    }

    /// Whether Ctrl+A / Échap act on the selection (on by default). A section that
    /// reads those keys itself (key test, macro recording) turns it off.
    pub fn shortcuts(mut self, on: bool) -> Self {
        self.shortcuts = on;
        self
    }

    /// Largest size of a 1u key, in points.
    pub fn max_unit(mut self, unit: f32) -> Self {
        self.max_unit = unit;
        self
    }

    /// Height the drawing may take, case included.
    pub fn max_height(mut self, height: f32) -> Self {
        self.max_height = Some(height);
        self
    }

    /// Draws the keyboard across the available width and handles the selection:
    /// click toggles a key, Shift+click adds the keys in between (reading order),
    /// dragging adds the keys of the rectangle, Ctrl+A selects all, Échap clears.
    /// The response is `changed()` when the selection changed.
    pub fn show(
        self,
        ui: &mut Ui,
        layout: &Layout,
        selected: &mut BTreeSet<u8>,
        render: impl Fn(u8) -> KeyLook,
    ) -> Response {
        let p = Palette::of(ui.ctx());
        let ppp = ui.pixels_per_point();
        let (units_w, units_h) = layout.size();
        let (case_w, case_h) = (units_w + 2.0 * CASE_PAD, units_h + 2.0 * CASE_PAD);
        let width = ui.available_width();
        let mut unit = (width / case_w).min(self.max_unit);
        if let Some(h) = self.max_height {
            unit = unit.min(h / case_h);
        }
        let unit = unit.max(12.0);
        let (rect, mut response) =
            ui.allocate_exact_size(vec2(width, unit * case_h), Sense::click_and_drag());
        response = response.on_hover_cursor(CursorIcon::Default);
        let case = Rect::from_center_size(rect.center(), vec2(unit * case_w, unit * case_h))
            .round_to_pixels(ppp);
        let origin = case.min + vec2(CASE_PAD, CASE_PAD) * unit;
        let gap = (unit * 0.075).clamp(2.0, 4.0);
        let keys: Vec<(&KeyDef, Rect)> = layout
            .keys
            .iter()
            .map(|k| {
                let r = Rect::from_min_size(origin + vec2(k.x, k.y) * unit, vec2(k.w, k.h) * unit);
                (k, r.shrink(gap / 2.0).round_to_pixels(ppp))
            })
            .collect();

        // --- interaction
        let before = selected.clone();
        let anchor_id = self.id.with("anchor");
        let origin_id = self.id.with("drag_origin");
        let ctx = ui.ctx().clone();
        let mut anchor: Option<u8> = ctx.data(|d| d.get_temp(anchor_id)).flatten();
        let pointer = response.hover_pos().or(response.interact_pointer_pos());
        let key_at = |pos: Pos2| {
            keys.iter()
                .find(|(_, r)| r.expand(gap / 2.0).contains(pos))
                .map(|(k, _)| k.index)
        };
        let hovered = pointer.and_then(key_at);

        if response.drag_started() {
            let start = ui.input(|i| i.pointer.press_origin());
            ctx.data_mut(|d| d.insert_temp(origin_id, start));
        }
        let drag_rect = if response.dragged() || response.drag_stopped() {
            let start: Option<Pos2> = ctx.data(|d| d.get_temp(origin_id)).flatten();
            start
                .zip(response.interact_pointer_pos().or(pointer))
                .map(|(a, b)| Rect::from_two_pos(a, b))
        } else {
            None
        };
        let in_drag: BTreeSet<u8> = drag_rect
            .map(|area| selection::in_rect(keys.iter().map(|(k, r)| (k.index, *r)), area))
            .unwrap_or_default();
        if response.drag_stopped() {
            selected.extend(in_drag.iter().copied());
            ctx.data_mut(|d| d.remove::<Option<Pos2>>(origin_id));
        }
        if response.clicked()
            && let Some(key) = hovered
        {
            let shift = ui.input(|i| i.modifiers.shift);
            let order = layout.row_order();
            selection::click(selected, &mut anchor, &order, key, shift);
        }
        // Shortcuts, unless a text field has the focus.
        if self.shortcuts && ctx.memory(|m| m.focused().is_none()) {
            if ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::A)) {
                selected.extend(layout.keys.iter().map(|k| k.index));
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                selected.clear();
                anchor = None;
            }
        }
        ctx.data_mut(|d| d.insert_temp(anchor_id, anchor));
        if *selected != before {
            response.mark_changed();
        }
        if hovered.is_some() && drag_rect.is_none() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }

        // --- drawing
        let painter = ui.painter_at(rect.expand(8.0));
        let case_radius = CornerRadius::same((unit * 0.32).clamp(8.0, 16.0) as u8);
        painter.add(
            Shadow {
                offset: [0, 6],
                blur: 22,
                spread: 0,
                color: Color32::from_black_alpha(if p.dark { 90 } else { 22 }),
            }
            .as_shape(case, case_radius),
        );
        let colors = Colors::of(&p);
        painter.rect(
            case,
            case_radius,
            colors.case,
            Stroke::new(1.0, colors.case_edge),
            StrokeKind::Inside,
        );
        for (key, r) in &keys {
            let state = KeyState {
                selected: selected.contains(&key.index),
                hovered: hovered == Some(key.index) && drag_rect.is_none(),
                previewed: in_drag.contains(&key.index),
            };
            keycap(
                &painter,
                &colors,
                *r,
                unit,
                key,
                &render(key.index),
                state,
                ppp,
            );
        }
        if let Some(area) = drag_rect {
            painter.rect(
                area.intersect(case),
                CornerRadius::same(4),
                p.accent.gamma_multiply(0.12),
                Stroke::new(1.0, p.accent.gamma_multiply(0.8)),
                StrokeKind::Inside,
            );
        }
        response
    }
}

#[derive(Debug, Clone, Copy)]
struct KeyState {
    selected: bool,
    hovered: bool,
    /// Inside the rectangle being dragged.
    previewed: bool,
}

struct Colors {
    case: Color32,
    case_edge: Color32,
    /// Face and skirt of the main keys, then of the modifiers and function keys.
    face: Color32,
    skirt: Color32,
    mod_face: Color32,
    mod_skirt: Color32,
    highlight: Color32,
    text: Color32,
    mod_text: Color32,
    accent: Color32,
    accent_skirt: Color32,
    on_accent: Color32,
}

impl Colors {
    fn of(p: &Palette) -> Self {
        if p.dark {
            Self {
                case: Color32::from_rgb(0x12, 0x13, 0x19),
                case_edge: p.border,
                face: Color32::from_rgb(0x2b, 0x2c, 0x3a),
                skirt: Color32::from_rgb(0x1c, 0x1d, 0x27),
                mod_face: Color32::from_rgb(0x22, 0x23, 0x2f),
                mod_skirt: Color32::from_rgb(0x18, 0x19, 0x21),
                highlight: Color32::from_white_alpha(14),
                text: Color32::from_rgb(0xec, 0xec, 0xf3),
                mod_text: Color32::from_rgb(0xa9, 0xab, 0xc0),
                accent: p.accent,
                accent_skirt: Color32::from_rgb(0x52, 0x37, 0xd6),
                on_accent: p.on_accent,
            }
        } else {
            Self {
                case: Color32::from_rgb(0xe6, 0xe6, 0xef),
                case_edge: Color32::from_rgb(0xd8, 0xd8, 0xe4),
                face: Color32::WHITE,
                skirt: Color32::from_rgb(0xcf, 0xcf, 0xdc),
                mod_face: Color32::from_rgb(0xf2, 0xf2, 0xf8),
                mod_skirt: Color32::from_rgb(0xc4, 0xc4, 0xd3),
                highlight: Color32::from_white_alpha(200),
                text: Color32::from_rgb(0x1a, 0x1b, 0x26),
                mod_text: Color32::from_rgb(0x5a, 0x5c, 0x70),
                accent: p.accent,
                accent_skirt: Color32::from_rgb(0x55, 0x3a, 0xd9),
                on_accent: p.on_accent,
            }
        }
    }
}

/// Keys printed in the main colour (letters, digits, punctuation, space); the others
/// get the modifier colour, as on a two-tone keycap set.
fn is_main_key(hid: u8) -> bool {
    matches!(hid, 0x04..=0x27 | 0x2C..=0x38 | 0x64)
}

/// Readable text over `fill`.
fn text_on(fill: Color32) -> Color32 {
    if fill.intensity() > 0.6 {
        Color32::from_rgb(0x1a, 0x1b, 0x26)
    } else {
        Color32::WHITE
    }
}

#[allow(clippy::too_many_arguments)]
fn keycap(
    painter: &Painter,
    c: &Colors,
    r: Rect,
    unit: f32,
    key: &KeyDef,
    look: &KeyLook,
    state: KeyState,
    ppp: f32,
) {
    let main = is_main_key(key.hid);
    let (mut face, mut skirt, mut text) = match look.fill {
        Some(fill) => (
            fill,
            fill.lerp_to_gamma(Color32::BLACK, 0.35),
            text_on(fill),
        ),
        None if main => (c.face, c.skirt, c.text),
        None => (c.mod_face, c.mod_skirt, c.mod_text),
    };
    let accent_face = state.selected && look.fill.is_none();
    if accent_face {
        face = c.accent;
        skirt = c.accent_skirt;
        text = c.on_accent;
    } else if state.hovered || state.previewed {
        face = face.lerp_to_gamma(c.accent, 0.16);
    }

    let radius = (unit * 0.13).clamp(3.0, 7.0);
    let inset = (unit * 0.085).clamp(2.0, 5.0);
    let face_rect = Rect::from_min_max(
        r.min + vec2(inset, inset * 0.45),
        r.max - vec2(inset, inset * 1.55),
    )
    .round_to_pixels(ppp);

    if state.selected && !accent_face {
        // coloured keycap: a ring keeps the colour visible
        painter.rect_stroke(
            r.expand(1.5),
            CornerRadius::same((radius + 2.0) as u8),
            Stroke::new(2.0, c.accent),
            StrokeKind::Outside,
        );
    } else if state.selected {
        painter.rect_filled(
            r.expand(2.0),
            CornerRadius::same((radius + 2.0) as u8),
            c.accent.gamma_multiply(0.22),
        );
    }
    painter.rect_filled(r, CornerRadius::same(radius as u8), skirt);
    let edge = if state.hovered || state.previewed {
        Stroke::new(1.0, c.accent.gamma_multiply(0.85))
    } else if accent_face {
        Stroke::new(1.0, Color32::from_white_alpha(40))
    } else {
        Stroke::new(1.0, c.highlight)
    };
    painter.rect(
        face_rect,
        CornerRadius::same((radius - 1.0).max(2.0) as u8),
        face,
        edge,
        StrokeKind::Inside,
    );

    // legend
    let pad = (unit * 0.05).clamp(1.5, 3.0);
    let max_w = face_rect.width() - 2.0 * pad;
    let max_h = face_rect.height() - pad;
    let single = look.label.chars().count() == 1;
    let start = if single {
        (unit * 0.34).clamp(10.0, 18.0)
    } else {
        (unit * 0.25).clamp(8.5, 13.5)
    };
    let lines = fit(painter, &look.label, max_w, max_h, start, single, text);
    let total: f32 = lines.iter().map(|g| g.size().y).sum();
    let mut y = face_rect.center().y - total / 2.0;
    for (i, g) in lines.iter().enumerate() {
        // On a two-line legend the shifted character (top) is dimmer.
        let color = if lines.len() == 2 && look.label.contains('\n') && i == 0 {
            text.gamma_multiply(0.72)
        } else {
            text
        };
        let pos = pos2(face_rect.center().x - g.size().x / 2.0, y).round_to_pixels(ppp);
        painter.galley(pos, g.clone(), color);
        y += g.size().y;
    }

    if let Some(badge) = &look.badge {
        let font = theme::strong((unit * 0.2).clamp(8.0, 11.0));
        let color = if accent_face { c.on_accent } else { c.accent };
        let g = painter.layout_no_wrap(badge.clone(), font, color);
        let pos = pos2(face_rect.right() - g.size().x - 3.0, face_rect.top() + 1.0);
        painter.galley(pos, g, color);
    }
}

/// Lays out `label` in lines that fit `max_w` × `max_h`, shrinking the font down to a
/// floor; words wrap at spaces, `\n` forces a line.
fn fit(
    painter: &Painter,
    label: &str,
    max_w: f32,
    max_h: f32,
    start: f32,
    strong: bool,
    color: Color32,
) -> Vec<Arc<Galley>> {
    let font = |size: f32| {
        if strong {
            theme::strong(size)
        } else {
            theme::regular(size)
        }
    };
    let floor = 7.0f32.min(start);
    let mut size = start;
    loop {
        let font_id = font(size);
        let measure = |s: &str| {
            painter
                .layout_no_wrap(s.to_owned(), font_id.clone(), color)
                .size()
                .x
        };
        let mut lines: Vec<String> = Vec::new();
        for part in label.split('\n') {
            let mut current = String::new();
            for word in part.split(' ').filter(|w| !w.is_empty()) {
                let candidate = if current.is_empty() {
                    word.to_owned()
                } else {
                    format!("{current} {word}")
                };
                if current.is_empty() || measure(&candidate) <= max_w {
                    current = candidate;
                } else {
                    lines.push(std::mem::replace(&mut current, word.to_owned()));
                }
            }
            lines.push(current);
        }
        let galleys: Vec<Arc<Galley>> = lines
            .into_iter()
            .map(|l| painter.layout_no_wrap(l, font_id.clone(), color))
            .collect();
        let w = galleys.iter().map(|g| g.size().x).fold(0.0, f32::max);
        let h: f32 = galleys.iter().map(|g| g.size().y).sum();
        if (w <= max_w && h <= max_h) || size <= floor {
            return galleys;
        }
        size = (size - 0.5).max(floor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_keys_are_the_typing_keys() {
        assert!(is_main_key(0x04)); // A
        assert!(is_main_key(0x2C)); // Space
        assert!(!is_main_key(0x28)); // Enter
        assert!(!is_main_key(0xE1)); // Shift
        assert!(!is_main_key(0x3A)); // F1
    }

    #[test]
    fn text_stays_readable_on_any_fill() {
        assert_eq!(text_on(Color32::WHITE), Color32::from_rgb(0x1a, 0x1b, 0x26));
        assert_eq!(text_on(Color32::from_rgb(0x20, 0x10, 0x60)), Color32::WHITE);
    }
}
