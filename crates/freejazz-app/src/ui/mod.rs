//! The interface: layout A (navigation on the left, header, keyboard on top of the
//! section panel, bottom bar with « Appliquer »).

pub mod bottom_bar;
pub mod header;
pub mod nav;
pub mod profile_switcher;
pub mod shell;
pub mod theme;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, Frame, Margin, Response, Sense, Stroke, StrokeKind, Ui,
    vec2,
};

use crate::app::Section;
use theme::Palette;

/// What the user asked for this frame; the app carries it out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiAction {
    Select(Section),
    Apply,
    Revert,
    Reload,
    /// « Restaurer la dernière sauvegarde » after an interrupted write.
    RestoreLatest,
    DismissError,
}

/// Phosphor icon of each section.
pub fn section_icon(section: Section) -> &'static str {
    use egui_phosphor::regular as i;
    match section {
        Section::Keys => i::KEYBOARD,
        Section::Lighting => i::PALETTE,
        Section::Live => i::SPARKLE,
        Section::Macros => i::RECORD,
        Section::Performance => i::GAUGE,
        Section::Advanced => i::STACK,
        Section::Settings => i::GEAR_SIX,
        Section::Test => i::CROSSHAIR,
    }
}

/// One-line description under the section title.
pub fn section_blurb(section: Section) -> &'static str {
    match section {
        Section::Keys => "Réassignez chaque touche : lettres, multimédia, souris, macros.",
        Section::Lighting => "Effets intégrés, couleurs touche par touche et dessins enregistrés.",
        Section::Live => "Effets calculés par Freejazz et envoyés en continu au clavier.",
        Section::Macros => "Enregistrez des séquences de touches avec leurs délais.",
        Section::Performance => "Point de déclenchement, Rapid Trigger et zones mortes.",
        Section::Advanced => "DKS, Mod-Tap, Toggle, SOCD et RS.",
        Section::Settings => "Fréquence d'envoi, mode stabilité, calibrage automatique.",
        Section::Test => "Testez chaque touche et calibrez les switchs.",
    }
}

/// A card: surface, hairline border, rounded.
pub fn card(p: &Palette) -> Frame {
    Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(theme::RADIUS_CARD))
        .inner_margin(Margin::same(20))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
}

/// A painted button with an optional leading icon.
pub fn button(
    ui: &mut Ui,
    p: &Palette,
    kind: ButtonKind,
    icon: Option<&str>,
    label: &str,
    enabled: bool,
) -> Response {
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), theme::strong(14.0), Color32::PLACEHOLDER);
    let icon_w = if icon.is_some() { 24.0 } else { 0.0 };
    let size = vec2(galley.size().x + icon_w + 36.0, 40.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let hovered = enabled && response.hovered();
    let (fill, stroke, fg) = match (kind, enabled) {
        (ButtonKind::Primary, true) => (
            if hovered { p.accent_hover } else { p.accent },
            Stroke::NONE,
            p.on_accent,
        ),
        (ButtonKind::Primary, false) => (
            p.accent.gamma_multiply(0.35),
            Stroke::NONE,
            p.on_accent.gamma_multiply(0.6),
        ),
        (ButtonKind::Secondary, true) => (
            if hovered { p.surface_hi } else { p.surface },
            Stroke::new(1.0, if hovered { p.text_faint } else { p.border }),
            p.text,
        ),
        (ButtonKind::Secondary, false) => (p.surface, Stroke::new(1.0, p.border), p.text_faint),
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(theme::RADIUS_CONTROL),
        fill,
        stroke,
        StrokeKind::Inside,
    );
    let left = rect.center().x - (galley.size().x + icon_w) / 2.0;
    if let Some(icon) = icon {
        painter.text(
            egui::pos2(left + 8.0, rect.center().y),
            Align2::CENTER_CENTER,
            icon,
            theme::icon(16.0),
            fg,
        );
    }
    painter.galley(
        egui::pos2(left + icon_w, rect.center().y - galley.size().y / 2.0),
        galley,
        fg,
    );
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// A rounded chip: optional coloured dot, then text.
pub fn pill(
    ui: &mut Ui,
    p: &Palette,
    dot: Option<Color32>,
    icon: Option<&str>,
    text: &str,
) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::regular(13.0), p.text);
    let icon_w = if icon.is_some() { 20.0 } else { 0.0 };
    let dot_w = if dot.is_some() { 16.0 } else { 0.0 };
    let size = vec2(galley.size().x + 24.0 + dot_w + icon_w, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(15),
        p.surface_hi,
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
    let mut x = rect.left() + 12.0;
    if let Some(color) = dot {
        let c = egui::pos2(x + 4.0, rect.center().y);
        painter.circle_filled(c, 7.0, color.gamma_multiply(0.18));
        painter.circle_filled(c, 4.0, color);
        x += dot_w;
    }
    if let Some(icon) = icon {
        painter.text(
            egui::pos2(x + 6.0, rect.center().y),
            Align2::CENTER_CENTER,
            icon,
            theme::icon(14.0),
            p.accent,
        );
        x += icon_w;
    }
    painter.galley(
        egui::pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    response
}

/// A square badge holding an icon, tinted with the accent.
pub fn icon_badge(ui: &mut Ui, p: &Palette, icon: &str, size: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same((size / 4.0) as u8), p.accent_soft);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon,
        theme::icon(size * 0.5),
        p.accent,
    );
}
