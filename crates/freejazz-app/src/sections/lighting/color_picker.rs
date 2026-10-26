//! A colour picker: saturation/value square, hue bar and a hex field.

use eframe::egui::ecolor::Hsva;
use eframe::egui::epaint::{Mesh, Vertex, WHITE_UV};
use eframe::egui::{
    self, Color32, CornerRadius, Sense, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2,
};
use freejazz_protocol::blocks::Rgb;

use crate::ui::theme::{self, Palette};

pub fn to_color32(c: Rgb) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

/// `#rrggbb` (the `#` optional).
pub fn parse_hex(text: &str) -> Option<Rgb> {
    let t = text.trim().trim_start_matches('#');
    if t.len() != 6 || !t.is_ascii() {
        return None;
    }
    let v = u32::from_str_radix(t, 16).ok()?;
    Some(Rgb {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
    })
}

pub fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

pub struct ColorPicker {
    hsva: Hsva,
    hex: String,
    /// The colour last given or returned: when the caller's differs, it changed
    /// elsewhere and the picker follows.
    last: Option<Rgb>,
}

impl Default for ColorPicker {
    fn default() -> Self {
        Self {
            hsva: Hsva::new(0.0, 1.0, 1.0, 1.0),
            hex: String::new(),
            last: None,
        }
    }
}

fn gradient(ui: &Ui, rect: egui::Rect, corners: [Color32; 4]) {
    let mut mesh = Mesh::default();
    for (pos, color) in [
        rect.left_top(),
        rect.right_top(),
        rect.left_bottom(),
        rect.right_bottom(),
    ]
    .into_iter()
    .zip(corners)
    {
        mesh.vertices.push(Vertex {
            pos,
            uv: WHITE_UV,
            color,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(2, 1, 3);
    ui.painter().add(mesh);
}

impl ColorPicker {
    /// Shows the picker for `rgb`; true when the colour changed.
    pub fn show(&mut self, ui: &mut Ui, p: &Palette, rgb: &mut Rgb) -> bool {
        if self.last != Some(*rgb) {
            self.hsva = Hsva::from_srgb([rgb.r, rgb.g, rgb.b]);
            self.hex = hex(*rgb);
            self.last = Some(*rgb);
        }
        let mut changed = false;

        // saturation (x) and value (y)
        let (rect, resp) = ui.allocate_exact_size(vec2(220.0, 150.0), Sense::click_and_drag());
        let pure: Color32 = Hsva::new(self.hsva.h, 1.0, 1.0, 1.0).into();
        gradient(ui, rect, [Color32::WHITE, pure, Color32::WHITE, pure]);
        gradient(
            ui,
            rect,
            [
                Color32::TRANSPARENT,
                Color32::TRANSPARENT,
                Color32::BLACK,
                Color32::BLACK,
            ],
        );
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(4),
            Stroke::new(1.0, p.border),
            StrokeKind::Inside,
        );
        if (resp.dragged() || resp.clicked())
            && let Some(pos) = resp.interact_pointer_pos()
        {
            self.hsva.s = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            self.hsva.v = 1.0 - ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
            changed = true;
        }
        let marker = pos2(
            rect.left() + self.hsva.s * rect.width(),
            rect.top() + (1.0 - self.hsva.v) * rect.height(),
        );
        ui.painter()
            .circle_stroke(marker, 6.0, Stroke::new(2.0, Color32::WHITE));
        ui.painter()
            .circle_stroke(marker, 7.5, Stroke::new(1.0, Color32::BLACK));

        // hue
        let (bar, resp) = ui.allocate_exact_size(vec2(220.0, 16.0), Sense::click_and_drag());
        let stops = 6;
        for i in 0..stops {
            let (t0, t1) = (i as f32 / stops as f32, (i + 1) as f32 / stops as f32);
            let part = egui::Rect::from_x_y_ranges(
                (bar.left() + t0 * bar.width())..=(bar.left() + t1 * bar.width()),
                bar.y_range(),
            );
            let a: Color32 = Hsva::new(t0, 1.0, 1.0, 1.0).into();
            let b: Color32 = Hsva::new(t1.min(0.9999), 1.0, 1.0, 1.0).into();
            gradient(ui, part, [a, b, a, b]);
        }
        if (resp.dragged() || resp.clicked())
            && let Some(pos) = resp.interact_pointer_pos()
        {
            self.hsva.h = ((pos.x - bar.left()) / bar.width()).clamp(0.0, 0.9999);
            changed = true;
        }
        let x = bar.left() + self.hsva.h * bar.width();
        ui.painter().rect_stroke(
            egui::Rect::from_center_size(pos2(x, bar.center().y), vec2(6.0, 20.0)),
            CornerRadius::same(3),
            Stroke::new(2.0, Color32::WHITE),
            StrokeKind::Middle,
        );

        if changed {
            let [r, g, b] = self.hsva.to_srgb();
            *rgb = Rgb { r, g, b };
            self.hex = hex(*rgb);
            self.last = Some(*rgb);
        }

        // hex
        ui.horizontal(|ui| {
            let (sw, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
            ui.painter()
                .rect_filled(sw, CornerRadius::same(6), to_color32(*rgb));
            let edit = ui.add(
                TextEdit::singleline(&mut self.hex)
                    .desired_width(86.0)
                    .font(theme::regular(13.0)),
            );
            if edit.changed()
                && let Some(c) = parse_hex(&self.hex)
            {
                self.hsva = Hsva::from_srgb([c.r, c.g, c.b]);
                *rgb = c;
                self.last = Some(c);
                changed = true;
            }
            if edit.lost_focus() {
                self.hex = hex(*rgb);
            }
        });
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let c = parse_hex("#7c5cff").unwrap();
        assert_eq!(
            c,
            Rgb {
                r: 0x7c,
                g: 0x5c,
                b: 0xff
            }
        );
        assert_eq!(hex(c), "#7c5cff");
        assert_eq!(parse_hex("00ff00"), Some(Rgb { r: 0, g: 255, b: 0 }));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("#gggggg"), None);
        assert_eq!(parse_hex("é12345"), None);
    }
}
