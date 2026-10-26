//! « Réglages »: the GameMode fields the site offers for this model (report rate,
//! stability mode, adaptive calibration), the backups (list, restore, export, import)
//! and the partial / full resets.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui::{self, RichText, Ui};
use freejazz_protocol::keyboard::ResetKind;

use super::{error_line, heading, hint, row, switch};
use crate::app::AppState;
use crate::backups::{self, BackupInfo};
use crate::device::Request;
use crate::ui::theme::{self, Palette};
use crate::ui::{ButtonKind, button};

/// Report rates the site offers for this model (protocol.md 6.2).
const RATES_HZ: [u32; 3] = [1000, 4000, 8000];

/// Backups listed at most.
const SHOWN_BACKUPS: usize = 8;

/// The backups folder is read again at most this often while the panel is shown.
const BACKUPS_REFRESH: Duration = Duration::from_secs(3);

/// The backups of the folder, read once and kept a few seconds: the panel is drawn at
/// every frame, the folder need not be read that often.
#[derive(Default)]
pub struct BackupList {
    read: Option<(Instant, PathBuf, Vec<BackupInfo>)>,
}

impl BackupList {
    /// The backups in `dir`, newest first; the folder is read when the list is older
    /// than `max_age`, of another folder, or was [`invalidate`](Self::invalidate)d.
    pub fn get(&mut self, dir: &Path, max_age: Duration) -> &[BackupInfo] {
        let fresh = self
            .read
            .as_ref()
            .is_some_and(|(at, d, _)| d == dir && at.elapsed() < max_age);
        if !fresh {
            self.read = Some((
                Instant::now(),
                dir.to_path_buf(),
                backups::list_backups_in(dir),
            ));
        }
        self.read.as_ref().map_or(&[], |(_, _, list)| list.as_slice())
    }

    /// The folder changed (import, restore, reset): read it again next time.
    pub fn invalidate(&mut self) {
        self.read = None;
    }
}

/// What the panel remembers between frames.
#[derive(Default)]
pub struct SettingsUi {
    /// A reset waiting for its confirmation.
    confirm_reset: Option<ResetKind>,
    /// A restore waiting for its confirmation.
    confirm_restore: Option<PathBuf>,
    /// Last result of an export / import: `(is_error, message)`.
    message: Option<(bool, String)>,
    /// What the app should send to the keyboard (restore, reset).
    request: Option<Request>,
    /// The app's own options (tray, launch with Windows).
    pub app: crate::settings::Settings,
    /// The options changed and are to be saved.
    app_changed: bool,
    /// Launch with Windows touches the real registry (off in `--mock`).
    pub real_autostart: bool,
    backups: BackupList,
    /// Looks for and installs newer releases.
    pub updater: crate::updater::Updater,
}

impl SettingsUi {
    /// The request the user confirmed, taken once by the app. A restore or a reset
    /// writes a backup first: the list is read again.
    pub fn take_request(&mut self) -> Option<Request> {
        let request = self.request.take();
        if request.is_some() {
            self.backups.invalidate();
        }
        request
    }

    /// True once after an option of « Application » changed.
    pub fn take_app_changed(&mut self) -> bool {
        std::mem::take(&mut self.app_changed)
    }
}

/// What a reset clears, for its title and its confirmation (French).
pub fn reset_effect(kind: ResetKind) -> (&'static str, &'static str) {
    match kind {
        ResetKind::Keys => (
            "Remettre les touches à zéro",
            "Toutes les affectations de touches (remappages, macros affectées, DKS, Mod-Tap, \
             Toggle, SOCD, RS) sont perdues.",
        ),
        ResetKind::Lighting => (
            "Remettre l'éclairage à zéro",
            "L'effet lumineux et les couleurs touche par touche sont perdus.",
        ),
        ResetKind::Macros => (
            "Remettre les macros à zéro",
            "Toutes les macros enregistrées dans le clavier sont perdues.",
        ),
        ResetKind::All => (
            "Tout remettre à zéro",
            "Touches, éclairage et macros sont perdus : le clavier revient à l'état d'usine.",
        ),
    }
}

/// The resets offered: only « Tout remettre à zéro » (the site sends it) until the
/// experimental functions are on.
fn visible_resets(experimental: bool) -> Vec<ResetKind> {
    if experimental {
        vec![
            ResetKind::Keys,
            ResetKind::Lighting,
            ResetKind::Macros,
            ResetKind::All,
        ]
    } else {
        vec![ResetKind::All]
    }
}

/// Where the keyboard's backups are (the mock keeps its own).
fn backups_dir(state: &AppState) -> Option<PathBuf> {
    if state.mock {
        Some(crate::mock::backups_dir())
    } else {
        crate::paths::backups_dir()
    }
}

/// « Application »: options that do not touch the keyboard.
fn app_options(ui: &mut Ui, p: &Palette, st: &mut SettingsUi) {
    heading(ui, p, "Application");
    let mut on = st.app.close_to_tray;
    if row(ui, p, "Fermer = zone de notification", |ui| {
        switch(ui, p, &mut on, true).changed()
    })
    .inner
    {
        st.app.close_to_tray = on;
        st.app_changed = true;
    }
    let mut on = st.app.autostart;
    if row(ui, p, "Lancer avec Windows", |ui| {
        switch(ui, p, &mut on, st.real_autostart).changed()
    })
    .inner
    {
        let result = if st.real_autostart {
            crate::autostart::set(on)
        } else {
            Ok(())
        };
        match result {
            Ok(()) => {
                st.app.autostart = on;
                st.app_changed = true;
                st.message = None;
            }
            Err(e) => st.message = Some((true, e)),
        }
    }
    hint(
        ui,
        p,
        "Au démarrage de Windows, Freejazz se lance dans la zone de notification, fenêtre masquée.",
    );
    ui.add_space(10.0);
}

/// « Mises à jour » and « À propos ».
fn updates(ui: &mut Ui, p: &Palette, st: &mut SettingsUi) {
    use crate::updater::{self, UpdateState};
    heading(ui, p, "Mises à jour");
    let mut on = st.app.check_updates;
    let enabled = st.updater.enabled();
    if row(ui, p, "Rechercher les mises à jour au démarrage", |ui| {
        switch(ui, p, &mut on, enabled).changed()
    })
    .inner
    {
        st.app.check_updates = on;
        st.app_changed = true;
    }
    let busy = st.updater.busy();
    let state = st.updater.state.clone();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if button(
            ui,
            p,
            ButtonKind::Secondary,
            None,
            "Rechercher maintenant",
            enabled && !busy,
        )
        .clicked()
        {
            st.updater.check(true);
        }
        let status = match &state {
            _ if !enabled => "Désactivé en mode démonstration.".to_owned(),
            UpdateState::Checking => "Recherche en cours…".to_owned(),
            UpdateState::UpToDate => format!("Freejazz {} est à jour.", updater::VERSION),
            UpdateState::Downloading(r) => format!("Téléchargement de la version {}…", r.version),
            _ => String::new(),
        };
        if !status.is_empty() {
            hint(ui, p, &status);
        }
    });
    if let UpdateState::Available(release) | UpdateState::Downloading(release) = &state {
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!("Version {} disponible", release.version))
                .font(theme::strong(14.0))
                .color(p.text),
        );
        if !release.notes.is_empty() {
            egui::ScrollArea::vertical()
                .id_salt("update-notes")
                .max_height(130.0)
                .show(ui, |ui| hint(ui, p, &release.notes));
        }
        hint(
            ui,
            p,
            "La mise à jour est vérifiée (SHA-256), puis Freejazz se ferme et se relance.",
        );
        if button(
            ui,
            p,
            ButtonKind::Primary,
            None,
            "Mettre à jour",
            matches!(state, UpdateState::Available(_)),
        )
        .clicked()
        {
            st.updater.start_update();
        }
    }
    if let Some(e) = st.updater.error.clone() {
        error_line(ui, p, &e);
    }
    ui.add_space(10.0);
    heading(ui, p, "À propos");
    ui.horizontal(|ui| {
        hint(ui, p, &format!("Freejazz {} · licence MIT ·", updater::VERSION));
        if ui.link("github.com/fraustiz/freejazz").clicked() {
            updater::open_url(updater::REPO_URL);
        }
    });
    ui.add_space(10.0);
}

pub fn show(ui: &mut Ui, p: &Palette, state: &mut AppState, st: &mut SettingsUi) {
    app_options(ui, p, st);
    updates(ui, p, st);
    let busy = state.busy;
    let dirty = state.dirty;
    let dir = backups_dir(state);
    let experimental = state.experimental;
    let (Some(edited), Some(base)) = (state.edited.as_mut(), state.snapshot.as_ref()) else {
        return;
    };
    let pid = base.info.pid;

    heading(ui, p, "Clavier");
    let game = &mut edited.game_mode;
    let current = game.report_rate();
    let mut picked = None;
    row(ui, p, "Fréquence d'envoi", |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for hz in RATES_HZ {
            if crate::ui::shell::chip(ui, p, &format!("{hz} Hz"), current == Some(hz)).clicked() {
                picked = Some(hz);
            }
        }
    });
    if let Some(hz) = picked
        && let Err(e) = game.set_report_rate(hz)
    {
        st.message = Some((true, e.to_string()));
    }
    if current.is_none() {
        hint(
            ui,
            p,
            &format!(
                "Fréquence actuelle inconnue (code {}).",
                game.report_rate_code()
            ),
        );
    }
    hint(
        ui,
        p,
        "Changer la fréquence reconnecte le clavier en USB : comptez quelques secondes.",
    );
    let mut on = game.stability_mode() != 0;
    if row(ui, p, "Mode de stabilité", |ui| {
        switch(ui, p, &mut on, true).changed()
    })
    .inner
    {
        game.set_stability_mode(u8::from(on));
    }
    let mut on = game.auto_calibration() != 0;
    if row(ui, p, "Calibration adaptative (bêta)", |ui| {
        switch(ui, p, &mut on, true).changed()
    })
    .inner
    {
        game.set_auto_calibration(u8::from(on));
    }

    // backups
    ui.add_space(10.0);
    heading(ui, p, "Sauvegardes");
    hint(
        ui,
        p,
        "Une sauvegarde de l'état du clavier est faite avant chaque restauration et chaque \
         remise à zéro.",
    );
    let list: Vec<BackupInfo> = match dir.as_deref() {
        Some(d) => st
            .backups
            .get(d, BACKUPS_REFRESH)
            .iter()
            .filter(|b| b.pid == pid)
            .cloned()
            .collect(),
        None => Vec::new(),
    };
    if list.is_empty() {
        hint(ui, p, "Aucune sauvegarde de ce clavier.");
    }
    for b in list.iter().take(SHOWN_BACKUPS) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let tag = if b.pre_restore {
                " · avant restauration"
            } else {
                ""
            };
            ui.allocate_ui_with_layout(
                egui::vec2(250.0, 30.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(
                        RichText::new(format!("{}{tag}", b.when()))
                            .font(theme::regular(13.5))
                            .color(p.text),
                    );
                },
            );
            if st.confirm_restore.as_ref() == Some(&b.path) {
                hint(ui, p, "Remplace la configuration du clavier :");
                if button(ui, p, ButtonKind::Primary, None, "Confirmer", !busy).clicked() {
                    st.request = Some(Request::Restore(Some(b.path.clone())));
                    st.confirm_restore = None;
                }
                if button(ui, p, ButtonKind::Secondary, None, "Annuler", true).clicked() {
                    st.confirm_restore = None;
                }
            } else {
                if button(ui, p, ButtonKind::Secondary, None, "Restaurer", !busy).clicked() {
                    st.confirm_restore = Some(b.path.clone());
                }
                if button(ui, p, ButtonKind::Secondary, None, "Exporter", true).clicked()
                    && let Some(m) = export(&b.path)
                {
                    st.message = Some(m);
                }
            }
        });
    }
    if list.len() > SHOWN_BACKUPS {
        hint(
            ui,
            p,
            &format!("… et {} plus anciennes.", list.len() - SHOWN_BACKUPS),
        );
    }
    if button(
        ui,
        p,
        ButtonKind::Secondary,
        None,
        "Importer une sauvegarde…",
        dir.is_some(),
    )
    .clicked()
        && let Some(dir) = dir.as_deref()
    {
        st.message = import(dir);
        st.backups.invalidate();
    }
    match &st.message {
        Some((true, text)) => error_line(ui, p, text),
        Some((false, text)) => hint(ui, p, text),
        None => {}
    }

    // experimental
    ui.add_space(10.0);
    heading(ui, p, "Fonctions expérimentales");
    hint(
        ui,
        p,
        "Actions que le site officiel n'envoie jamais à ce modèle (remises à zéro partielles,          calibrage, profondeur d'appui) : jamais essayées sur ce clavier. Désactivées à chaque          démarrage.",
    );
    row(ui, p, "Activer les fonctions expérimentales", |ui| {
        switch(ui, p, &mut state.experimental, true)
    });

    // resets
    ui.add_space(10.0);
    heading(ui, p, "Remise à zéro");
    hint(
        ui,
        p,
        "Efface des réglages dans le clavier. Un message de confirmation précise ce qui sera perdu.",
    );
    if !experimental {
        hint(
            ui,
            p,
            "Les remises à zéro partielles demandent les fonctions expérimentales.",
        );
    }
    ui.horizontal_wrapped(|ui| {
        for kind in visible_resets(experimental) {
            let (title, _) = reset_effect(kind);
            if button(ui, p, ButtonKind::Secondary, None, title, !busy).clicked() {
                st.confirm_reset = Some(kind);
            }
        }
    });

    if let Some(kind) = st.confirm_reset {
        let (title, lost) = reset_effect(kind);
        let modal = egui::Modal::new(egui::Id::new("reset-confirm")).show(ui.ctx(), |ui| {
            ui.set_width(380.0);
            heading(ui, p, title);
            ui.add_space(6.0);
            ui.label(RichText::new(lost).font(theme::regular(13.5)).color(p.text));
            ui.add_space(4.0);
            hint(
                ui,
                p,
                "L'état actuel du clavier est sauvegardé d'abord : « Restaurer » le remet en place.",
            );
            if dirty {
                hint(ui, p, "Les modifications non appliquées seront abandonnées.");
            }
            ui.add_space(10.0);
            let mut go = false;
            let mut cancel = false;
            ui.horizontal(|ui| {
                // UNVERIFIED-HW: never run during development.
                go = button(ui, p, ButtonKind::Primary, None, "Remettre à zéro", !busy).clicked();
                cancel = button(ui, p, ButtonKind::Secondary, None, "Annuler", true).clicked();
            });
            (go, cancel)
        });
        let (go, cancel) = modal.inner;
        if go {
            st.request = Some(Request::FactoryReset(kind));
            st.confirm_reset = None;
        } else if cancel || modal.should_close() {
            st.confirm_reset = None;
        }
    }
}

/// « Exporter »: asks where, copies the file; `None` when the dialog is cancelled.
fn export(src: &std::path::Path) -> Option<(bool, String)> {
    let name = src
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sauvegarde.json");
    let dest = rfd::FileDialog::new()
        .set_file_name(name)
        .add_filter("Sauvegarde Freejazz", &["json"])
        .save_file()?;
    Some(match backups::export_backup(src, &dest) {
        Ok(()) => (false, format!("Sauvegarde exportée : {}", dest.display())),
        Err(e) => (true, crate::device::french(&e)),
    })
}

/// « Importer »: asks which file, checks it and copies it into `dir`.
fn import(dir: &std::path::Path) -> Option<(bool, String)> {
    let src = rfd::FileDialog::new()
        .add_filter("Sauvegarde Freejazz", &["json"])
        .pick_file()?;
    Some(match backups::import_backup(&src, dir) {
        Ok(path) => (
            false,
            format!(
                "Sauvegarde importée : {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ),
        ),
        Err(e) => (true, crate::device::french(&e)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_reset_names_what_is_lost() {
        for kind in [
            ResetKind::Keys,
            ResetKind::Lighting,
            ResetKind::Macros,
            ResetKind::All,
        ] {
            let (title, lost) = reset_effect(kind);
            assert!(title.contains("zéro"));
            assert!(lost.contains("perdu"), "{lost}");
        }
        assert!(reset_effect(ResetKind::Macros).1.contains("macros"));
        assert!(
            reset_effect(ResetKind::Lighting).1.contains("éclairage")
                || reset_effect(ResetKind::Lighting).1.contains("lumineux")
        );
    }

    #[test]
    fn the_backup_list_is_not_read_at_every_frame() {
        let dir = tempfile::tempdir().unwrap();
        let mut list = BackupList::default();
        assert!(list.get(dir.path(), Duration::from_secs(60)).is_empty());
        std::fs::write(dir.path().join("ak980max-8043-20261025-100349.json"), "{}").unwrap();
        // the next frames reuse the list
        assert!(list.get(dir.path(), Duration::from_secs(60)).is_empty());
        // read again once stale, or invalidated
        assert_eq!(list.get(dir.path(), Duration::ZERO).len(), 1);
        std::fs::write(dir.path().join("ak980max-8043-20261025-100350.json"), "{}").unwrap();
        assert_eq!(list.get(dir.path(), Duration::from_secs(60)).len(), 1);
        list.invalidate();
        assert_eq!(list.get(dir.path(), Duration::from_secs(60)).len(), 2);
    }

    #[test]
    fn partial_resets_need_the_experimental_switch() {
        assert_eq!(visible_resets(false), vec![ResetKind::All]);
        assert_eq!(visible_resets(true).len(), 4);
    }

    #[test]
    fn rates_edit_the_game_mode_code() {
        let mut g = crate::mock::fixture().game_mode;
        assert_eq!(g.report_rate(), Some(8000));
        g.set_report_rate(4000).unwrap();
        assert_eq!(g.report_rate_code(), 5);
        assert!(g.set_report_rate(500).is_err());
    }
}
