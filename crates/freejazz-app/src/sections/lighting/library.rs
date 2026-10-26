//! The designs library panel: bundled designs, saved designs, save the current one.

use eframe::egui::{TextEdit, Ui};
use freejazz_protocol::blocks::CustomLeds;

use crate::designs::{self, Design};
use crate::paths;
use crate::sections::{error_line, heading, hint};
use crate::ui::shell::chip;
use crate::ui::theme::{self, Palette};
use crate::ui::{ButtonKind, button};

#[derive(Default)]
pub struct Library {
    /// Saved designs, read when first shown and after each change.
    saved: Option<Vec<Design>>,
    name: String,
    error: Option<String>,
}

impl Library {
    fn saved(&mut self) -> &[Design] {
        self.saved.get_or_insert_with(|| {
            paths::designs_dir()
                .map(|d| designs::list_designs(&d))
                .unwrap_or_default()
        })
    }

    /// Shows the library; the design the user picked, if any.
    pub fn show(&mut self, ui: &mut Ui, p: &Palette, current: &CustomLeds) -> Option<CustomLeds> {
        let mut picked = None;
        heading(ui, p, "Bibliothèque de dessins");
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = eframe::egui::vec2(6.0, 6.0);
            for d in designs::bundled() {
                if chip(ui, p, &d.name, false).clicked() {
                    picked = Some(d.leds);
                }
            }
        });
        let saved: Vec<Design> = self.saved().to_vec();
        let mut delete = None;
        if !saved.is_empty() {
            hint(ui, p, "Vos dessins");
            for d in &saved {
                ui.horizontal(|ui| {
                    if chip(ui, p, &d.name, false).clicked() {
                        picked = Some(d.leds.clone());
                    }
                    if chip(ui, p, "Supprimer", false).clicked() {
                        delete = Some(d.name.clone());
                    }
                });
            }
        }
        if let Some(name) = delete {
            self.error = paths::designs_dir()
                .map(|dir| designs::delete_design(&dir, &name))
                .and_then(Result::err)
                .map(|e| format!("Suppression impossible : {e}"));
            self.saved = None;
        }
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.name)
                    .hint_text("Nom du dessin")
                    .desired_width(180.0)
                    .font(theme::regular(13.0)),
            );
            let ok = !self.name.trim().is_empty();
            if button(
                ui,
                p,
                ButtonKind::Secondary,
                None,
                "Enregistrer le dessin",
                ok,
            )
            .clicked()
            {
                self.error = match paths::designs_dir() {
                    Some(dir) => designs::save_design(&dir, &self.name, current)
                        .err()
                        .map(|e| format!("Enregistrement impossible : {e}")),
                    None => Some("Dossier de données introuvable".to_owned()),
                };
                self.saved = None;
            }
        });
        if let Some(e) = &self.error {
            error_line(ui, p, e);
        }
        picked
    }
}
