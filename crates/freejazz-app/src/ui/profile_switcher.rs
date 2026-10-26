//! The profile menu of the header: app profiles (snapshots kept on the PC) and, on a
//! keyboard that has them (`max_profile > 1`), the hardware profiles.
//!
//! Applying a profile writes the whole keyboard: it asks first, and the device manager
//! saves the current state before writing (and refuses without a backup).

use eframe::egui::{self, RichText, Ui};

use super::theme::{self, Palette};
use super::{ButtonKind, button};
use crate::app::AppState;
use crate::device::Request;
use crate::profiles;
use crate::sections::{error_line, heading, hint};

enum Modal {
    Apply(String),
    Save(String),
    Delete(String),
}

/// What the menu remembers between frames.
#[derive(Default)]
pub struct ProfileSwitcher {
    names: Vec<String>,
    loaded: bool,
    modal: Option<Modal>,
    /// Last error of a save / delete, shown in the dialog.
    error: Option<String>,
    request: Option<Request>,
}

impl ProfileSwitcher {
    /// The request the user confirmed, taken once by the app.
    pub fn take_request(&mut self) -> Option<Request> {
        self.request.take()
    }

    fn refresh(&mut self, mock: bool) {
        self.names = profiles::profiles_dir(mock)
            .map(|d| profiles::list_profiles(&d))
            .unwrap_or_default();
        self.loaded = true;
    }

    /// Screenshot aid (`--demo-profile save|apply`): two profiles, a dialog open.
    pub fn demo(&mut self, what: &str, state: &AppState) {
        let (Some(dir), Some(snapshot)) =
            (profiles::profiles_dir(state.mock), state.snapshot.as_ref())
        else {
            return;
        };
        for name in ["Jeu", "Bureau"] {
            let _ = profiles::save_profile(&dir, name, snapshot);
        }
        self.refresh(state.mock);
        self.modal = Some(if what == "save" {
            Modal::Save("Streaming".into())
        } else {
            Modal::Apply("Jeu".into())
        });
    }
}

pub fn show(ui: &mut Ui, p: &Palette, state: &mut AppState, sw: &mut ProfileSwitcher) {
    use egui_phosphor::regular as i;
    if !sw.loaded {
        sw.refresh(state.mock);
    }
    ui.label(
        RichText::new(i::USER_CIRCLE)
            .font(theme::icon(20.0))
            .color(p.text_faint),
    );
    let hardware = state
        .snapshot
        .as_ref()
        .map_or(0, |s| s.info.max_profile)
        .max(1);
    let usable = state.connected && !state.busy;
    ui.add_enabled_ui(usable, |ui| {
        let combo = egui::ComboBox::from_id_salt("profile")
            .selected_text(
                RichText::new(format!("Profil : {}", state.profile)).font(theme::regular(13.5)),
            )
            .width(190.0)
            .show_ui(ui, |ui| {
                for name in &sw.names {
                    if ui.selectable_label(*name == state.profile, name).clicked() {
                        sw.modal = Some(Modal::Apply(name.clone()));
                    }
                }
                if sw.names.is_empty() {
                    ui.label(
                        RichText::new("Aucun profil enregistré")
                            .font(theme::regular(12.5))
                            .color(p.text_faint),
                    );
                }
                ui.separator();
                if ui.button("Enregistrer l'état du clavier…").clicked() {
                    sw.error = None;
                    sw.modal = Some(Modal::Save(String::new()));
                }
                if sw.names.contains(&state.profile)
                    && ui
                        .button(format!("Supprimer « {} »…", state.profile))
                        .clicked()
                {
                    sw.error = None;
                    sw.modal = Some(Modal::Delete(state.profile.clone()));
                }
                // hardware profiles: only when the keyboard has some
                if hardware > 1 {
                    ui.separator();
                    for n in 1..=hardware {
                        if ui.button(format!("Profil matériel {n}")).clicked() {
                            sw.request = Some(Request::SetProfile(n));
                        }
                    }
                }
            });
        combo
            .response
            .on_hover_text("Profils de l'appli, enregistrés sur ce PC");
    });
    dialogs(ui, p, state, sw);
}

fn dialogs(ui: &mut Ui, p: &Palette, state: &mut AppState, sw: &mut ProfileSwitcher) {
    let Some(modal) = sw.modal.take() else {
        return;
    };
    let dir = profiles::profiles_dir(state.mock);
    let ctx = ui.ctx().clone();
    let mut keep = true;
    let mut next = None;
    let shown = egui::Modal::new(egui::Id::new("profile-dialog")).show(&ctx, |ui| {
        ui.set_width(380.0);
        match &modal {
            Modal::Apply(name) => {
                heading(ui, p, &format!("Appliquer le profil « {name} »"));
                ui.add_space(6.0);
                hint(
                    ui,
                    p,
                    "Tout le clavier prend la configuration du profil (touches, éclairage, macros, \
                     réglages). L'état actuel est sauvegardé d'abord : « Restaurer » le remet en place.",
                );
                if state.dirty {
                    hint(ui, p, "Les modifications non appliquées seront abandonnées.");
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let ok = dir.is_some() && state.connected && !state.busy;
                    if button(ui, p, ButtonKind::Primary, None, "Appliquer", ok).clicked()
                        && let Some(dir) = &dir
                    {
                        sw.request = Some(Request::ApplyProfile {
                            name: name.clone(),
                            path: profiles::profile_path(dir, name),
                        });
                        state.profile = name.clone();
                        keep = false;
                    }
                    if button(ui, p, ButtonKind::Secondary, None, "Annuler", true).clicked() {
                        keep = false;
                    }
                });
            }
            Modal::Save(text) => {
                let mut text = text.clone();
                heading(ui, p, "Enregistrer l'état du clavier");
                ui.add_space(6.0);
                hint(
                    ui,
                    p,
                    "L'état actuel du clavier devient un profil que vous pourrez réappliquer.",
                );
                if state.dirty {
                    hint(ui, p, "Les modifications non appliquées n'y sont pas incluses.");
                }
                ui.add_space(6.0);
                ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .hint_text("Nom du profil")
                        .desired_width(f32::INFINITY),
                );
                let cleaned = profiles::clean_name(&text);
                match (&cleaned, text.is_empty()) {
                    (Err(why), false) => error_line(ui, p, why),
                    (Ok(name), _) if sw.names.contains(name) => {
                        hint(ui, p, "Un profil de ce nom existe : il sera remplacé.");
                    }
                    _ => {}
                }
                if let Some(e) = &sw.error {
                    error_line(ui, p, e);
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let ok = cleaned.is_ok() && dir.is_some() && state.snapshot.is_some();
                    if button(ui, p, ButtonKind::Primary, None, "Enregistrer", ok).clicked()
                        && let (Ok(name), Some(dir), Some(snapshot)) =
                            (&cleaned, &dir, state.snapshot.as_ref())
                    {
                        match profiles::save_profile(dir, name, snapshot) {
                            Ok(_) => {
                                state.profile = name.clone();
                                state.notice = Some(format!("Profil « {name} » enregistré"));
                                sw.error = None;
                                keep = false;
                            }
                            Err(e) => sw.error = Some(crate::device::french(&e)),
                        }
                    }
                    if button(ui, p, ButtonKind::Secondary, None, "Annuler", true).clicked() {
                        keep = false;
                    }
                });
                if keep {
                    next = Some(Modal::Save(text));
                }
            }
            Modal::Delete(name) => {
                heading(ui, p, &format!("Supprimer le profil « {name} »"));
                ui.add_space(6.0);
                hint(
                    ui,
                    p,
                    "Le fichier du profil est supprimé de ce PC. Le clavier n'est pas touché.",
                );
                if let Some(e) = &sw.error {
                    error_line(ui, p, e);
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if button(ui, p, ButtonKind::Primary, None, "Supprimer", dir.is_some())
                        .clicked()
                        && let Some(dir) = &dir
                    {
                        match profiles::delete_profile(dir, name) {
                            Ok(()) => {
                                state.profile = "Par défaut".into();
                                sw.error = None;
                                keep = false;
                            }
                            Err(e) => sw.error = Some(crate::device::french(&e)),
                        }
                    }
                    if button(ui, p, ButtonKind::Secondary, None, "Annuler", true).clicked() {
                        keep = false;
                    }
                });
            }
        }
    });
    if shown.should_close() {
        keep = false;
    }
    if keep {
        sw.modal = next.or(Some(modal));
    } else {
        sw.refresh(state.mock);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_opens_a_dialog_on_two_saved_profiles() {
        let mut sw = ProfileSwitcher::default();
        let mut state = AppState::new(true, false);
        state.snapshot = crate::mock::read_fixture().ok();
        sw.demo("apply", &state);
        assert!(matches!(&sw.modal, Some(Modal::Apply(n)) if n == "Jeu"));
        assert!(sw.names.iter().any(|n| n == "Bureau"));
        assert!(sw.take_request().is_none());
    }
}
