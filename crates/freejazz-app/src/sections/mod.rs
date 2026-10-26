//! The section panels (what sits under the keyboard) and the small controls they share.

pub mod advanced;
pub mod keys;
pub mod lighting;
pub mod live;
pub mod macros;
pub mod performance;
pub mod settings;
pub mod test;

/// What the section panels remember between frames.
#[derive(Default)]
pub struct SectionsUi {
    pub keys: keys::KeysUi,
    pub advanced: advanced::AdvancedUi,
    pub lighting: lighting::LightingUi,
    pub live: live::LiveUi,
    pub macros: macros::MacrosUi,
    pub settings: settings::SettingsUi,
    pub test: test::TestUi,
    pub profiles: crate::ui::profile_switcher::ProfileSwitcher,
}

impl SectionsUi {
    /// The colours that replace the keyboard view's own while `section` is shown: the
    /// built-in effect animation or the live effect preview.
    pub fn preview_frame(&self, section: crate::app::Section) -> Option<&crate::live::Frame> {
        match section {
            crate::app::Section::Lighting => self.lighting.preview_frame(),
            crate::app::Section::Live => self.live.preview_frame(),
            _ => None,
        }
    }
}

use eframe::egui::{self, CornerRadius, Response, RichText, Sense, Ui, vec2};

use crate::ui::theme::{self, Palette};

/// A titled group inside a section panel.
pub fn heading(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(RichText::new(text).font(theme::strong(14.0)).color(p.text));
}

/// A line of secondary text.
pub fn hint(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(
        RichText::new(text)
            .font(theme::regular(12.5))
            .color(p.text_dim),
    );
}

/// A validation message, in French, in the danger colour.
pub fn error_line(ui: &mut Ui, p: &Palette, text: &str) {
    use egui_phosphor::regular as i;
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(i::WARNING_CIRCLE)
                .font(theme::icon(15.0))
                .color(p.danger),
        );
        ui.label(
            RichText::new(text)
                .font(theme::regular(13.0))
                .color(p.danger),
        );
    });
}

/// `label` on the left (fixed width), the control on the right.
pub fn row<R>(
    ui: &mut Ui,
    p: &Palette,
    label: &str,
    add: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(250.0, 30.0), Sense::hover());
        ui.painter().text(
            egui::pos2(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            theme::regular(13.5),
            p.text,
        );
        add(ui)
    })
}

/// A switch; the response is `changed()` when `on` flipped.
pub fn switch(ui: &mut Ui, p: &Palette, on: &mut bool, enabled: bool) -> Response {
    let size = vec2(38.0, 22.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(size, sense);
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(response.id, *on);
    let fill = p.border.lerp_to_gamma(p.accent, t);
    let fill = if enabled {
        fill
    } else {
        fill.gamma_multiply(0.5)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(11), fill);
    let x = egui::lerp((rect.left() + 11.0)..=(rect.right() - 11.0), t);
    painter.circle_filled(egui::pos2(x, rect.center().y), 8.0, egui::Color32::WHITE);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// A millimetre slider (French decimal comma, 0.01 mm steps); true when the value
/// changed.
pub fn mm_slider(ui: &mut Ui, value: &mut f32, min: f32, max: f32) -> bool {
    mm_slider_step(ui, value, min, max, 0.01)
}

/// [`mm_slider`] with its own step (0.1 mm for the DKS points, stored in tenths).
pub fn mm_slider_step(ui: &mut Ui, value: &mut f32, min: f32, max: f32, step: f64) -> bool {
    ui.spacing_mut().slider_width = 280.0;
    let decimals = if step >= 0.1 { 1 } else { 2 };
    ui.add(
        egui::Slider::new(value, min..=max)
            .step_by(step)
            .custom_formatter(move |v, _| format!("{v:.decimals$}").replace('.', ","))
            .custom_parser(|s| s.trim().replace(',', ".").parse().ok())
            .suffix(" mm"),
    )
    .changed()
}
