//! Bottom bar: pending-changes indicator, « Annuler les changements », « Appliquer ».

use eframe::egui::{self, Align, Align2, Layout, Sense, Ui, pos2, vec2};

use super::theme::{self, Palette};
use super::{ButtonKind, UiAction, button};
use crate::app::AppState;

pub fn show(ui: &mut Ui, p: &Palette, state: &AppState, actions: &mut Vec<UiAction>) {
    use egui_phosphor::regular as i;
    ui.horizontal_centered(|ui| {
        let (icon, color, text) = if let Some(error) = &state.error {
            (i::WARNING_CIRCLE, p.danger, error.as_str())
        } else if state.busy {
            (i::CIRCLE_NOTCH, p.accent, "Écriture sur le clavier…")
        } else if state.dirty {
            (i::CIRCLE, p.warning, "Changements non appliqués")
        } else if state.connected {
            let text = state.notice.as_deref().unwrap_or("Le clavier est à jour");
            (i::CHECK_CIRCLE, p.success, text)
        } else {
            (i::PLUGS, p.text_faint, "Aucun clavier connecté")
        };
        status(ui, p, icon, color, text, state.dirty && !state.busy);
        let restore_label = match &state.restore_target {
            Some((_, when)) => format!("Restaurer ({when})"),
            None => "Restaurer la dernière sauvegarde".to_owned(),
        };
        if state.error.is_some()
            && state.offer_restore
            && button(
                ui,
                p,
                ButtonKind::Secondary,
                Some(i::CLOCK_COUNTER_CLOCKWISE),
                &restore_label,
                state.connected && !state.busy,
            )
            .on_hover_text(match &state.restore_target {
                Some((_, when)) => format!(
                    "Restaurer la sauvegarde du {when} : elle est réécrite sur le clavier, \
                     l'état actuel est d'abord sauvegardé (« avant-restauration »)"
                ),
                None => "Réécrit la sauvegarde la plus récente de ce clavier ; l'état actuel \
                         est d'abord sauvegardé (« avant-restauration »)"
                    .to_owned(),
            })
            .clicked()
        {
            actions.push(UiAction::RestoreLatest);
        }
        if state.error.is_some()
            && ui
                .add(
                    egui::Button::new(egui::RichText::new(i::X).font(theme::icon(15.0)))
                        .frame(false),
                )
                .on_hover_text("Fermer")
                .clicked()
        {
            actions.push(UiAction::DismissError);
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let can = state.dirty && !state.busy && state.connected;
            if button(
                ui,
                p,
                ButtonKind::Primary,
                Some(i::UPLOAD_SIMPLE),
                "Appliquer",
                can,
            )
            .clicked()
            {
                actions.push(UiAction::Apply);
            }
            ui.add_space(2.0);
            if button(
                ui,
                p,
                ButtonKind::Secondary,
                Some(i::ARROW_COUNTER_CLOCKWISE),
                "Annuler les changements",
                can,
            )
            .clicked()
            {
                actions.push(UiAction::Revert);
            }
        });
    });
}

fn status(ui: &mut Ui, p: &Palette, icon: &str, color: egui::Color32, text: &str, pending: bool) {
    let font = theme::regular(13.5);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, p.text);
    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 30.0, 30.0), Sense::hover());
    let painter = ui.painter();
    let c = pos2(rect.left() + 9.0, rect.center().y);
    if pending {
        painter.circle_filled(c, 8.0, color.gamma_multiply(0.18));
        painter.circle_filled(c, 4.5, color);
    } else {
        painter.text(c, Align2::CENTER_CENTER, icon, theme::icon(17.0), color);
    }
    let text_color = if pending { p.text } else { p.text_dim };
    painter.galley(
        pos2(rect.left() + 26.0, rect.center().y - galley.size().y / 2.0),
        galley,
        text_color,
    );
}
