//! Left navigation: brand, sections with icons, mode footer.

use eframe::egui::{self, Align2, CornerRadius, Rect, Sense, Ui, pos2, vec2};

use super::theme::{self, Palette};
use super::{UiAction, section_icon};
use crate::app::{AppState, Section};

const GROUPS: [(&str, &[Section]); 2] = [
    (
        "CONFIGURATION",
        &[
            Section::Keys,
            Section::Lighting,
            Section::Live,
            Section::Macros,
            Section::Performance,
            Section::Advanced,
        ],
    ),
    ("CLAVIER", &[Section::Settings, Section::Test]),
];

pub fn show(ui: &mut Ui, p: &Palette, state: &AppState, actions: &mut Vec<UiAction>) {
    brand(ui, p);
    ui.add_space(18.0);
    for (title, sections) in GROUPS {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(title)
                    .font(theme::strong(11.0))
                    .color(p.text_faint),
            );
        });
        ui.add_space(2.0);
        for &section in sections {
            if item(ui, p, section, state.section == section) {
                actions.push(UiAction::Select(section));
            }
        }
        ui.add_space(10.0);
    }

    // footer pinned to the bottom of the panel
    let bottom = ui.max_rect().bottom();
    let rect = Rect::from_min_max(
        pos2(ui.max_rect().left(), bottom - 56.0),
        pos2(ui.max_rect().right(), bottom),
    );
    footer(ui, p, state, rect);
}

fn brand(ui: &mut Ui, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    let painter = ui.painter();
    let logo = Rect::from_min_size(
        pos2(rect.left() + 8.0, rect.center().y - 17.0),
        vec2(34.0, 34.0),
    );
    painter.rect_filled(logo, CornerRadius::same(10), p.accent);
    let cap = logo.shrink2(vec2(8.0, 8.0));
    painter.rect_stroke(
        cap,
        CornerRadius::same(4),
        egui::Stroke::new(2.0, p.on_accent),
        egui::StrokeKind::Inside,
    );
    painter.text(
        cap.center() + vec2(0.0, 0.5),
        Align2::CENTER_CENTER,
        "F",
        theme::strong(11.0),
        p.on_accent,
    );
    painter.text(
        pos2(logo.right() + 12.0, rect.center().y - 8.0),
        Align2::LEFT_CENTER,
        "Freejazz",
        theme::strong(17.0),
        p.text,
    );
    painter.text(
        pos2(logo.right() + 12.0, rect.center().y + 11.0),
        Align2::LEFT_CENTER,
        "Configurateur Ajazz",
        theme::regular(12.0),
        p.text_faint,
    );
}

/// One navigation row; `true` when clicked.
fn item(ui: &mut Ui, p: &Palette, section: Section, selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::same(10), p.accent_soft);
        let bar = Rect::from_min_size(pos2(rect.left(), rect.center().y - 9.0), vec2(3.0, 18.0));
        painter.rect_filled(bar, CornerRadius::same(2), p.accent);
    } else if hovered {
        painter.rect_filled(rect, CornerRadius::same(10), p.surface_hi);
    }
    let (icon_color, text_color) = if selected {
        (p.accent, p.text)
    } else if hovered {
        (p.text_dim, p.text)
    } else {
        (p.text_faint, p.text_dim)
    };
    painter.text(
        pos2(rect.left() + 26.0, rect.center().y),
        Align2::CENTER_CENTER,
        section_icon(section),
        theme::icon(18.0),
        icon_color,
    );
    painter.text(
        pos2(rect.left() + 46.0, rect.center().y),
        Align2::LEFT_CENTER,
        section.title(),
        if selected {
            theme::strong(14.0)
        } else {
            theme::regular(14.0)
        },
        text_color,
    );
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn footer(ui: &mut Ui, p: &Palette, state: &AppState, rect: Rect) {
    use egui_phosphor::regular as i;
    let painter = ui.painter();
    painter.hline(rect.x_range(), rect.top(), egui::Stroke::new(1.0, p.border));
    let (icon, text) = if state.mock {
        (i::FLASK, "Faux clavier (démo)")
    } else {
        (i::USB, "Clavier USB")
    };
    painter.text(
        pos2(rect.left() + 18.0, rect.center().y),
        Align2::LEFT_CENTER,
        icon,
        theme::icon(15.0),
        p.text_faint,
    );
    painter.text(
        pos2(rect.left() + 40.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        theme::regular(12.5),
        p.text_faint,
    );
    painter.text(
        pos2(rect.right() - 12.0, rect.center().y),
        Align2::RIGHT_CENTER,
        concat!("v", env!("CARGO_PKG_VERSION")),
        theme::regular(12.0),
        p.text_faint,
    );
}
