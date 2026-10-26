//! The rows of one macro: key, press / release, delay after the action.

use eframe::egui::{self, RichText, Ui};
use egui_phosphor::regular as i;
use freejazz_protocol::blocks::macros::MacroAction;

use super::recorder::{MAX_DELAY_MS, MIN_DELAY_MS};
use crate::keyboard::Layout;
use crate::keyboard::labels::Labels;
use crate::ui::theme::{self, Palette};

/// Name of what an action presses.
pub fn action_name(labels: &Labels, a: &MacroAction) -> String {
    if matches!(a.kind, 1 | 2) {
        return match a.key {
            1 => "Clic gauche",
            2 => "Clic droit",
            4 => "Clic milieu",
            8 => "Souris précédent",
            16 => "Souris suivant",
            _ => "Souris",
        }
        .to_owned();
    }
    labels
        .hid(a.key)
        .map(|l| l.replace('\n', " "))
        .unwrap_or_else(|| format!("0x{:02X}", a.key))
}

/// Keys the picker offers: those of the keyboard, in reading order.
fn pickable(labels: &Labels) -> Vec<(u8, String)> {
    let layout = Layout::ak980max();
    layout
        .row_order()
        .into_iter()
        .filter_map(|i| layout.key(i))
        .map(|k| {
            let name = labels
                .hid(k.hid)
                .map(|l| l.replace('\n', " "))
                .unwrap_or_else(|| k.label.clone());
            (k.hid, name)
        })
        .collect()
}

/// Edits `actions` in place; true when something changed.
pub fn rows(
    ui: &mut Ui,
    p: &Palette,
    labels: &Labels,
    id: u8,
    actions: &mut Vec<MacroAction>,
) -> bool {
    let mut changed = false;
    let mut remove = None;
    let keys = pickable(labels);
    let last = actions.len().saturating_sub(1);
    ui.spacing_mut().item_spacing.y = 4.0;
    for (n, a) in actions.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_sized(
                [24.0, 26.0],
                egui::Label::new(
                    RichText::new((n + 1).to_string())
                        .font(theme::regular(12.0))
                        .color(p.text_faint),
                ),
            );
            let before = a.key;
            if matches!(a.kind, 1 | 2) {
                ui.add_sized([150.0, 26.0], egui::Label::new(action_name(labels, a)));
            } else {
                egui::ComboBox::from_id_salt(("macro-key", id, n))
                    .width(150.0)
                    .selected_text(action_name(labels, a))
                    .show_ui(ui, |ui| {
                        for (hid, name) in &keys {
                            ui.selectable_value(&mut a.key, *hid, name);
                        }
                    });
            }
            changed |= a.key != before;
            let text = if a.press { "Appui" } else { "Relâchement" };
            if ui
                .add_sized([124.0, 26.0], egui::Button::new(text))
                .on_hover_text("Basculer entre appui et relâchement")
                .clicked()
            {
                a.press = !a.press;
                changed = true;
            }
            let min = if n == last { 0 } else { MIN_DELAY_MS };
            let before = a.delay_ms;
            ui.add(
                egui::DragValue::new(&mut a.delay_ms)
                    .range(min..=MAX_DELAY_MS)
                    // opening a macro never edits it, even one the site wrote out of range
                    .clamp_existing_to_range(false)
                    .suffix(" ms")
                    .speed(5),
            );
            changed |= a.delay_ms != before;
            if ui
                .add(egui::Button::new(
                    RichText::new(i::TRASH).font(theme::icon(14.0)),
                ))
                .on_hover_text("Supprimer cette action")
                .clicked()
            {
                remove = Some(n);
            }
        });
    }
    if let Some(n) = remove {
        actions.remove(n);
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn showing_a_macro_never_edits_its_delays() {
        // the site writes delays out of the editor's range (0 after the last action
        // is allowed, others below MIN_DELAY_MS are not): opening must not clamp them
        let mut actions = vec![MacroAction::default(); 3];
        actions[0].delay_ms = 1;
        actions[1].delay_ms = MAX_DELAY_MS + 7;
        let before = actions.clone();
        let (labels, p) = (Labels::build(|_| None), Palette::DARK);
        let ctx = egui::Context::default();
        theme::install(&ctx, None);
        let mut changed = false;
        for _ in 0..3 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                changed |= rows(ui, &p, &labels, 0, &mut actions);
            });
            out.textures_delta.clear();
        }
        assert!(!changed);
        assert_eq!(actions, before);
    }
}
