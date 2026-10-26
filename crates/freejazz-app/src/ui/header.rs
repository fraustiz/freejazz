//! Header: device name and firmware, app profile switcher, report rate, connection.

use eframe::egui::{self, Align, Layout, RichText, Ui};

use super::profile_switcher::{self, ProfileSwitcher};
use super::theme::{self, Palette};
use super::{UiAction, pill};
use crate::app::{AppState, Link};

pub fn show(
    ui: &mut Ui,
    p: &Palette,
    state: &mut AppState,
    profiles: &mut ProfileSwitcher,
    actions: &mut Vec<UiAction>,
) {
    use egui_phosphor::regular as i;
    ui.horizontal_centered(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(state.device_name())
                        .font(theme::strong(19.0))
                        .color(p.text),
                );
                if state.mock {
                    demo_badge(ui, p);
                }
            });
            let sub = match (state.link, state.firmware()) {
                (Link::Connected, Some(fw)) => format!("Ajazz  ·  firmware {fw}  ·  USB"),
                (Link::Searching, _) => "Recherche du clavier…".to_owned(),
                _ => "Branchez le clavier en USB".to_owned(),
            };
            ui.label(
                RichText::new(sub)
                    .font(theme::regular(12.5))
                    .color(p.text_faint),
            );
        });

        ui.add_space(24.0);
        profile_switcher::show(ui, p, state, profiles);

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (dot, text) = match state.link {
                Link::Connected => (p.success, "Connecté"),
                Link::Searching => (p.warning, "Connexion…"),
                Link::Disconnected => (p.text_faint, "Clavier déconnecté"),
            };
            pill(ui, p, Some(dot), None, text);
            if let Some(hz) = state.report_rate() {
                pill(ui, p, None, Some(i::LIGHTNING), &format!("{} Hz", hz))
                    .on_hover_text("Fréquence d'envoi");
            }
            if !state.connected
                && state.mock
                && state.link == Link::Disconnected
                && ui.button("Réessayer").clicked()
            {
                actions.push(UiAction::Reload);
            }
        });
    });
}

fn demo_badge(ui: &mut Ui, p: &Palette) {
    egui::Frame::new()
        .fill(p.accent_soft)
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(
                RichText::new("DÉMO")
                    .font(theme::strong(10.5))
                    .color(p.accent),
            );
        });
}
