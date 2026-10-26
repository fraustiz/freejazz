//! « Macros »: the list of macros stored in the keyboard (100 at most, 512 bytes in all),
//! an editor for the actions of one, and a recorder that captures what is typed in the
//! window. Assigning a macro to a key is done in « Touches ».

pub mod editor;
pub mod list;
pub mod recorder;

use eframe::egui::{self, Event, ScrollArea, Ui};
use freejazz_protocol::blocks::macros::{MACRO_CAPACITY, MAX_MACRO_ID};
use freejazz_protocol::blocks::macros::{Macro, Macros};

use super::{error_line, heading, hint};
use crate::app::AppState;
use crate::keyboard::labels;
use crate::ui::theme::Palette;
use crate::ui::{ButtonKind, button};
use recorder::Recorder;

/// What the panel remembers between frames.
#[derive(Default)]
pub struct MacrosUi {
    /// Macro being edited.
    selected: Option<u8>,
    /// Recording into `selected`.
    recording: Option<Recorder>,
    /// Macro whose deletion waits for a confirmation (keys run it).
    confirm_delete: Option<u8>,
    /// May a recording install the global key hook? (Not in `--mock`, nor in tests.)
    hook_allowed: bool,
}

impl MacrosUi {
    pub fn allow_hook(&mut self, allowed: bool) {
        self.hook_allowed = allowed;
    }

    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Leaving the section ends a recording: what was typed so far is dropped.
    pub fn leave(&mut self) {
        self.recording = None;
    }
}

/// Where a finished recording goes: appended to the macro, which is created if the
/// keyboard has none with this id.
fn append(
    macros: &mut Macros,
    id: u8,
    actions: Vec<freejazz_protocol::blocks::macros::MacroAction>,
) {
    if let Some(m) = macros.slots.iter_mut().find(|m| m.id == id) {
        m.actions.extend(actions);
    } else if id <= MAX_MACRO_ID {
        macros.slots.push(Macro {
            id,
            actions,
            raw_header: None,
        });
        macros.slots.sort_by_key(|m| m.id);
    }
}

pub fn show(ui: &mut Ui, p: &Palette, state: &mut AppState, st: &mut MacrosUi) {
    let (Some(edited), Some(base)) = (state.edited.as_mut(), state.snapshot.as_ref()) else {
        return;
    };
    let labels = labels::current();

    // keys typed while recording belong to the macro, not to the window
    if let Some(rec) = st.recording.as_mut() {
        let (events, modifiers, time) = ui.input_mut(|i| {
            let events = i.events.clone();
            i.events.retain(|e| !matches!(e, Event::Key { .. }));
            (events, i.modifiers, i.time)
        });
        rec.frame(&events, modifiers, time);
    }

    let used = edited.macros.used_bytes();
    list::capacity_bar(ui, p, used, usize::from(MACRO_CAPACITY));
    ui.add_space(4.0);

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(240.0);
            heading(ui, p, "Macros");
            let free = list::next_free_id(&edited.macros).is_some();
            if button(
                ui,
                p,
                ButtonKind::Primary,
                None,
                "Nouvelle macro",
                free && st.recording.is_none(),
            )
            .clicked()
            {
                st.selected = list::add(&mut edited.macros);
            }
            if edited.macros.slots.is_empty() {
                hint(ui, p, "Aucune macro dans le clavier.");
            }
            ScrollArea::vertical()
                .id_salt("macro-list")
                .max_height(360.0)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    for m in &edited.macros.slots {
                        let on = st.selected == Some(m.id);
                        let uses = list::uses(&edited.keys, m.id);
                        let mut detail = list::summary(m);
                        if uses > 0 {
                            detail.push_str(&format!(
                                " · {uses} touche{}",
                                if uses > 1 { "s" } else { "" }
                            ));
                        }
                        if list::line(ui, p, &format!("Macro {}", m.id), &detail, on).clicked()
                            && st.recording.is_none()
                        {
                            st.selected = Some(m.id);
                        }
                    }
                });
        });
        ui.add_space(16.0);
        ui.vertical(|ui| {
            let Some(id) = st
                .selected
                .filter(|id| edited.macros.slots.iter().any(|m| m.id == *id))
            else {
                st.selected = None;
                hint(ui, p, "Choisissez une macro, ou créez-en une.");
                return;
            };
            heading(ui, p, &format!("Macro {id}"));
            let recording = st.recording.is_some();
            ui.horizontal(|ui| {
                if recording {
                    let n = st.recording.as_ref().map_or(0, |r| r.actions.len());
                    if button(
                        ui,
                        p,
                        ButtonKind::Primary,
                        Some(egui_phosphor::regular::STOP),
                        "Arrêter",
                        true,
                    )
                    .clicked()
                        && let Some(rec) = st.recording.take()
                    {
                        append(&mut edited.macros, id, rec.finish(ui.input(|i| i.time)));
                    }
                    hint(
                        ui,
                        p,
                        &format!("Enregistrement… {n} action{}", if n > 1 { "s" } else { "" }),
                    );
                } else {
                    if button(
                        ui,
                        p,
                        ButtonKind::Secondary,
                        Some(egui_phosphor::regular::RECORD),
                        "Enregistrer à la suite",
                        true,
                    )
                    .clicked()
                    {
                        st.recording = Some(if st.hook_allowed {
                            Recorder::with_hook()
                        } else {
                            Recorder::default()
                        });
                    }
                    let has = edited
                        .macros
                        .slots
                        .iter()
                        .any(|m| m.id == id && !m.actions.is_empty());
                    if button(ui, p, ButtonKind::Secondary, None, "Vider", has).clicked()
                        && let Some(m) = edited.macros.slots.iter_mut().find(|m| m.id == id)
                    {
                        m.actions.clear();
                    }
                    let uses = list::uses(&edited.keys, id);
                    if button(
                        ui,
                        p,
                        ButtonKind::Secondary,
                        Some(egui_phosphor::regular::TRASH),
                        "Supprimer la macro",
                        true,
                    )
                    .clicked()
                    {
                        if uses > 0 {
                            st.confirm_delete = Some(id);
                        } else {
                            list::remove(&mut edited.macros, id);
                            st.selected = None;
                        }
                    }
                    if uses > 0 {
                        hint(
                            ui,
                            p,
                            &format!(
                                "{uses} touche{} l'utilise{}",
                                if uses > 1 { "s" } else { "" },
                                if uses > 1 { "nt" } else { "" }
                            ),
                        );
                    }
                }
            });
            if st.confirm_delete.is_some_and(|c| c != id) || recording {
                st.confirm_delete = None;
            }
            if st.confirm_delete == Some(id) {
                let layout = crate::keyboard::Layout::ak980max();
                let names: Vec<String> = list::keys_using(&edited.keys, id)
                    .into_iter()
                    .filter_map(|i| layout.key(i))
                    .map(|k| labels.default_label(k).replace('\n', " "))
                    .collect();
                heading(ui, p, &format!("Supprimer la macro {id} ?"));
                hint(
                    ui,
                    p,
                    &if names.len() > 1 {
                        format!(
                            "Touches concernées : {}. Elles retrouveront leur fonction d'origine.",
                            names.join(", ")
                        )
                    } else {
                        format!(
                            "Touche concernée : {}. Elle retrouvera sa fonction d'origine.",
                            names.join(", ")
                        )
                    },
                );
                ui.horizontal(|ui| {
                    if button(ui, p, ButtonKind::Primary, None, "Supprimer", true).clicked() {
                        list::delete_releasing(&mut edited.macros, &mut edited.keys, id);
                        st.selected = None;
                        st.confirm_delete = None;
                    }
                    if button(ui, p, ButtonKind::Secondary, None, "Annuler", true).clicked() {
                        st.confirm_delete = None;
                    }
                });
            }
            if recording {
                hint(
                    ui,
                    p,
                    "Tapez sur le clavier : appuis, relâchements et délais sont capturés.",
                );
            }
            ui.add_space(4.0);
            if let Some(m) = edited.macros.slots.iter_mut().find(|m| m.id == id) {
                if m.actions.is_empty() {
                    hint(ui, p, "Aucune action : enregistrez une séquence.");
                } else {
                    ScrollArea::vertical()
                        .id_salt("macro-rows")
                        .max_height(300.0)
                        .show(ui, |ui| {
                            ui.set_min_width(520.0);
                            editor::rows(ui, p, &labels, id, &mut m.actions);
                        });
                }
            }
        });
    });

    ui.add_space(4.0);
    hint(
        ui,
        p,
        "4 octets par macro et 4 par action. Pour lancer une macro avec une touche, \
         affectez-la dans la section Touches (onglet Macros).",
    );
    if let Err(e) = edited.macros.validate_changes(&base.macros, MACRO_CAPACITY) {
        ui.add_space(6.0);
        error_line(ui, p, &e.to_string());
    }
}

impl MacrosUi {
    /// Screenshot aid (`--demo-macros`): a « bonjour » macro, selected.
    pub fn demo(&mut self, macros: &mut Macros) {
        use freejazz_protocol::blocks::macros::MacroAction;
        let Some(id) = list::add(macros) else { return };
        let keys = [
            egui::Key::B,
            egui::Key::O,
            egui::Key::N,
            egui::Key::J,
            egui::Key::O,
            egui::Key::U,
            egui::Key::R,
        ];
        let mut actions = Vec::new();
        for k in keys {
            let Some(key) = recorder::hid_for(k) else {
                continue;
            };
            for (press, delay_ms) in [(true, 40), (false, 85)] {
                actions.push(MacroAction {
                    press,
                    kind: 3,
                    key,
                    delay_ms,
                    raw_flags: None,
                });
            }
        }
        append(macros, id, actions);
        self.selected = Some(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaving_the_section_stops_a_recording() {
        let mut st = MacrosUi {
            recording: Some(Recorder::default()),
            ..Default::default()
        };
        assert!(st.is_recording());
        st.leave();
        assert!(!st.is_recording());
    }
}
