//! « Effets en direct »: effects computed by Freejazz and streamed to the keyboard (the
//! engine of `live/`), one card each, and the editor of custom effects made of layers.

pub mod catalog;
pub mod editor;

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use eframe::egui::{self, RichText, Ui};
use egui_phosphor::regular as i;
use serde_json::Value;

use super::{error_line, heading, hint};
use crate::app::AppState;
use crate::device::{DeviceHandle, DeviceKind, Event};
use crate::live::{CustomEffect, Engine, EngineConfig, custom};
use crate::ui::shell::chip;
use crate::ui::theme::{self, Palette};
use crate::ui::{ButtonKind, button, card};
use catalog::Picking;
use editor::Editor;

const FPS_CHOICES: [u8; 5] = [10, 15, 20, 25, 30];

/// What is running: a card of the catalogue or a saved effect (by name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Running {
    Card(&'static str),
    Saved(String),
}

pub struct LiveUi {
    engine: Option<Engine>,
    running: Option<Running>,
    /// Images per second for the next start.
    fps: u8,
    params: HashMap<&'static str, Value>,
    picking: Picking,
    /// Saved effects, and where they live (`None`: nowhere to save).
    dir: Option<PathBuf>,
    saved: Vec<CustomEffect>,
    loaded: bool,
    confirm_delete: Option<String>,
    editor: Editor,
    /// A line for the user: (text, is an error).
    status: Option<(String, bool)>,
    /// `--mock`: no hook, no capture.
    headless: bool,
    frame: Option<crate::live::Frame>,
    pending: Option<Cmd>,
    /// Screenshot aid: the editor first.
    demo: bool,
}

/// A click in the panel; carried out with the device by [`LiveUi::run_pending`].
pub enum Cmd {
    Card(&'static catalog::Info),
    Saved(CustomEffect),
}

impl Default for LiveUi {
    fn default() -> Self {
        Self {
            engine: None,
            running: None,
            fps: 25,
            params: HashMap::new(),
            picking: Picking::default(),
            dir: custom::effects_dir(),
            saved: Vec::new(),
            loaded: false,
            confirm_delete: None,
            editor: Editor::default(),
            status: None,
            headless: false,
            frame: None,
            pending: None,
            demo: false,
        }
    }
}

impl LiveUi {
    /// Starts what was clicked.
    pub fn run_pending(&mut self, device: &DeviceHandle) {
        match self.pending.take() {
            Some(Cmd::Card(info)) => self.start_card(device, info),
            Some(Cmd::Saved(effect)) => self.start_saved(device, &effect),
            None => {}
        }
    }

    /// `--mock`: effects are saved in a temporary folder and run without the global key
    /// hook or audio capture.
    pub fn configure(&mut self, mock: bool) {
        if mock {
            self.headless = true;
            self.dir = Some(std::env::temp_dir().join("freejazz-mock-effects"));
        }
    }

    /// Screenshot aid: a three-layer effect, the editor at the top of the panel.
    pub fn demo(&mut self) {
        use crate::live::Layer;
        use serde_json::json;
        self.demo = true;
        let layout = crate::keyboard::Layout::ak980max();
        let f_row = crate::keyboard::groups::f_row(layout);
        self.editor.load(&CustomEffect {
            name: "Aurore et braises".into(),
            layers: vec![
                Layer::all_keys("wave", json!({"speed": 0.12})),
                Layer {
                    effect: "breathing".into(),
                    params: json!({"color": "#ff5c7c", "period": 3.0}),
                    keys: f_row,
                },
                Layer::all_keys("reactive", json!({"color": "#ffffff"})),
            ],
        });
    }

    /// For tests: saved effects in `dir`.
    pub fn with_dir(dir: PathBuf) -> Self {
        Self {
            dir: Some(dir),
            headless: true,
            ..Self::default()
        }
    }

    pub fn running(&self) -> Option<&Running> {
        self.running.as_ref()
    }

    pub fn preview_frame(&self) -> Option<&crate::live::Frame> {
        self.frame.as_ref()
    }

    /// The editor's live preview for this frame; repaints only while it is visible.
    pub fn update_preview(&mut self, state: &AppState, ctx: &egui::Context) {
        self.frame = None;
        if state.section == crate::app::Section::Live && state.connected {
            self.frame = self.editor.preview(ctx);
        }
    }

    /// The keyboard went away, refused the stream or the stream broke: the engine stops
    /// (and with it the hook, the capture and the sampling).
    pub fn on_event(&mut self, event: &Event) {
        match event {
            Event::Disconnected => self.stop(),
            Event::Failed(m) if self.engine.is_some() && m.starts_with("Effet en direct") => {
                self.stop();
                self.status = Some((m.clone(), true));
            }
            Event::LiveStarted(false) if self.engine.is_some() => {
                self.stop();
                self.status = Some((
                    "Le clavier n'a pas accepté l'effet en direct (voir le message en haut)".into(),
                    true,
                ));
            }
            _ => {}
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut engine) = self.engine.take() {
            engine.stop();
        }
        self.running = None;
    }

    /// Stops a running effect because something else needs the keyboard (a write, the
    /// key-depth test), and says so in the panel.
    pub fn stop_for(&mut self, why: &str) {
        if self.engine.is_some() {
            self.stop();
            self.status = Some((format!("Effet en direct arrêté : {why}"), false));
        }
    }

    fn config(&self, device: &DeviceHandle) -> EngineConfig {
        let mut config = if self.headless || device.kind() != DeviceKind::Real {
            EngineConfig::headless()
        } else {
            EngineConfig::default()
        };
        config.fps = f32::from(self.fps);
        config
    }

    /// Takes the engine just started (the previous one was stopped before it, so its
    /// `LiveEnd` reaches the keyboard before the new `LiveBegin`).
    fn start(&mut self, what: Running, effect: Result<Engine, String>) {
        match effect {
            Ok(engine) => {
                let warnings = engine.warnings().join(" ");
                self.status = (!warnings.is_empty()).then_some((warnings, true));
                self.engine = Some(engine);
                self.running = Some(what);
            }
            Err(m) => self.status = Some((m, true)),
        }
    }

    pub fn start_card(&mut self, device: &DeviceHandle, info: &'static catalog::Info) {
        self.stop();
        let params = self.params.get(info.id).cloned().unwrap_or(Value::Null);
        let result = match device.sender() {
            Some(tx) => Engine::start(tx, info.id, &params, self.config(device)),
            None => Err("Aucun clavier connecté".to_owned()),
        };
        self.start(Running::Card(info.id), result);
    }

    pub fn start_saved(&mut self, device: &DeviceHandle, effect: &CustomEffect) {
        self.stop();
        let result = match device.sender() {
            Some(tx) => Ok(Engine::start_effect(
                tx,
                effect.build(),
                &effect.name,
                self.config(device),
            )),
            None => Err("Aucun clavier connecté".to_owned()),
        };
        self.start(Running::Saved(effect.name.clone()), result);
    }

    fn reload(&mut self) {
        self.saved = self.dir.as_deref().map(custom::list).unwrap_or_default();
        self.loaded = true;
    }

    pub fn save_current(&mut self) {
        let Some(dir) = self.dir.clone() else {
            self.status = Some(("Dossier des effets introuvable".into(), true));
            return;
        };
        match self.editor.to_effect() {
            Err(m) => self.status = Some((m, true)),
            Ok(effect) => match custom::save(&dir, &effect) {
                Ok(_) => {
                    self.status = Some((format!("Effet « {} » enregistré", effect.name), false));
                    self.editor.saved(&effect.name);
                    self.reload();
                }
                Err(e) => self.status = Some((format!("Enregistrement impossible : {e}"), true)),
            },
        }
    }

    fn delete(&mut self, name: &str) {
        if self.running == Some(Running::Saved(name.to_owned())) {
            self.stop();
        }
        if let Some(dir) = &self.dir {
            match custom::delete(dir, name) {
                Ok(()) => self.status = Some((format!("Effet « {name} » supprimé"), false)),
                Err(e) => self.status = Some((format!("Suppression impossible : {e}"), true)),
            }
        }
        self.reload();
    }
}

pub fn show(
    ui: &mut Ui,
    p: &Palette,
    state: &mut AppState,
    selected: &BTreeSet<u8>,
    live: &mut LiveUi,
) {
    if !live.loaded {
        live.reload();
    }
    let usable = state.connected && !state.busy;
    if live.demo {
        editor_section(ui, p, selected, live);
        return;
    }

    hint(
        ui,
        p,
        "L'effet s'arrête quand tu quittes l'appli ; il continue fenêtre fermée tant que \
         l'icône est dans la zone de notification.",
    );
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Images par seconde").color(p.text_dim));
        for fps in FPS_CHOICES {
            if chip(ui, p, &fps.to_string(), live.fps == fps).clicked() {
                live.fps = fps;
            }
        }
        match &live.running {
            Some(_) => hint(ui, p, &format!("En cours à {} img/s", live.fps)),
            None => hint(ui, p, "pris en compte au prochain démarrage (30 au plus)"),
        }
    });
    if let Some((text, is_error)) = live.status.clone() {
        if is_error {
            error_line(ui, p, &text);
        } else {
            hint(ui, p, &text);
        }
    }
    if !usable {
        hint(
            ui,
            p,
            "Un clavier connecté est nécessaire pour démarrer un effet.",
        );
    }

    heading(ui, p, "Effets");
    for info in catalog::CARDS {
        effect_card(ui, p, live, info, usable);
    }

    heading(ui, p, "Mes effets");
    if live.saved.is_empty() {
        hint(
            ui,
            p,
            "Aucun effet enregistré. Créez-en un avec l'éditeur ci-dessous.",
        );
    }
    let saved = live.saved.clone();
    for effect in &saved {
        saved_card(ui, p, live, effect, usable);
    }

    editor_section(ui, p, selected, live);
}

fn editor_section(ui: &mut Ui, p: &Palette, selected: &BTreeSet<u8>, live: &mut LiveUi) {
    ui.add_space(6.0);
    heading(ui, p, "Éditeur d'effet à calques");
    live.editor.show(ui, p, selected, &mut live.picking);
    if live.editor.take_save_request() {
        live.save_current();
    }
}

fn start_stop(ui: &mut Ui, p: &Palette, running: bool, usable: bool) -> egui::Response {
    if running {
        button(ui, p, ButtonKind::Secondary, Some(i::STOP), "Arrêter", true)
    } else {
        button(
            ui,
            p,
            ButtonKind::Primary,
            Some(i::PLAY),
            "Démarrer",
            usable,
        )
    }
}

fn effect_card(
    ui: &mut Ui,
    p: &Palette,
    live: &mut LiveUi,
    info: &'static catalog::Info,
    usable: bool,
) {
    let running = live.running == Some(Running::Card(info.id));
    let mut clicked = false;
    card(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(info.title)
                        .font(theme::strong(15.0))
                        .color(p.text),
                );
                hint(ui, p, info.blurb);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                clicked = start_stop(ui, p, running, usable).clicked();
            });
        });
        if !info.params.is_empty() {
            let params = live.params.entry(info.id).or_insert(Value::Null);
            catalog::params_ui(ui, p, info, params, info.id, &mut live.picking);
        }
    });
    if clicked {
        if running {
            live.stop();
        } else {
            live.pending = Some(Cmd::Card(info));
        }
    }
}

fn saved_card(ui: &mut Ui, p: &Palette, live: &mut LiveUi, effect: &CustomEffect, usable: bool) {
    let running = live.running == Some(Running::Saved(effect.name.clone()));
    let (mut toggle, mut edit, mut delete) = (false, false, false);
    card(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(&effect.name)
                        .font(theme::strong(15.0))
                        .color(p.text),
                );
                let n = effect.layers.len();
                hint(
                    ui,
                    p,
                    &format!("{n} calque{}", if n > 1 { "s" } else { "" }),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                toggle = start_stop(ui, p, running, usable).clicked();
                edit = button(
                    ui,
                    p,
                    ButtonKind::Secondary,
                    Some(i::PENCIL_SIMPLE),
                    "Modifier",
                    true,
                )
                .clicked();
                delete = button(
                    ui,
                    p,
                    ButtonKind::Secondary,
                    Some(i::TRASH),
                    "Supprimer",
                    true,
                )
                .clicked();
            });
        });
        if live.confirm_delete.as_deref() == Some(&effect.name) {
            ui.horizontal(|ui| {
                error_line(ui, p, &format!("Supprimer « {} » ?", effect.name));
                if chip(ui, p, "Oui, supprimer", false).clicked() {
                    live.confirm_delete = None;
                    live.delete(&effect.name);
                }
                if chip(ui, p, "Non", false).clicked() {
                    live.confirm_delete = None;
                }
            });
        }
    });
    if toggle {
        if running {
            live.stop();
        } else {
            live.pending = Some(Cmd::Saved(effect.clone()));
        }
    }
    if edit {
        live.editor.load(effect);
    }
    if delete {
        live.confirm_delete = Some(effect.name.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::Request;
    use crate::live::Layer;
    use serde_json::json;

    #[test]
    fn a_mock_keyboard_runs_a_card_then_stops_it() {
        let _g = crate::live::engine::test_serial();
        let ctx = egui::Context::default();
        let waker_ctx = ctx.clone();
        let device = crate::mock::spawn(std::sync::Arc::new(move || waker_ctx.request_repaint()));
        let dir = tempfile::tempdir().unwrap();
        let mut live = LiveUi::with_dir(dir.path().to_path_buf());
        live.start_card(&device, &catalog::RIPPLE);
        assert_eq!(live.running(), Some(&Running::Card("ripple")));
        live.on_event(&Event::Disconnected);
        assert!(live.running().is_none());
    }

    #[test]
    fn saved_effects_run_and_are_deleted() {
        let _g = crate::live::engine::test_serial();
        let ctx = egui::Context::default();
        let waker_ctx = ctx.clone();
        let device = crate::mock::spawn(std::sync::Arc::new(move || waker_ctx.request_repaint()));
        let dir = tempfile::tempdir().unwrap();
        let mut live = LiveUi::with_dir(dir.path().to_path_buf());
        live.editor.load(&CustomEffect {
            name: "Essai".into(),
            layers: vec![Layer::all_keys("wave", json!({"speed": -0.2}))],
        });
        live.save_current();
        assert_eq!(live.saved.len(), 1);
        let effect = live.saved[0].clone();
        live.start_saved(&device, &effect);
        assert_eq!(live.running(), Some(&Running::Saved("Essai".into())));
        live.delete("Essai");
        assert!(live.saved.is_empty() && live.running().is_none());
    }

    /// A handle whose worker only records the requests it gets.
    fn recording_device() -> (DeviceHandle, std::sync::mpsc::Receiver<Request>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let device = DeviceHandle::spawn(DeviceKind::Mock, std::sync::Arc::new(|| {}), move |io| {
            for r in io.requests.iter() {
                let stop = matches!(r, Request::Stop);
                let _ = tx.send(r);
                if stop {
                    break;
                }
            }
        });
        (device, rx)
    }

    fn names(requests: &[Request]) -> Vec<&'static str> {
        requests
            .iter()
            .filter_map(|r| match r {
                Request::LiveBegin => Some("begin"),
                Request::LiveEnd => Some("end"),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn switching_effects_ends_the_old_stream_before_the_new_one_begins() {
        let _g = crate::live::engine::test_serial();
        let (device, rx) = recording_device();
        let mut live = LiveUi::with_dir(tempfile::tempdir().unwrap().keep());
        live.start_card(&device, &catalog::RIPPLE);
        std::thread::sleep(std::time::Duration::from_millis(60));
        live.start_card(&device, &catalog::AURORA);
        std::thread::sleep(std::time::Duration::from_millis(120));
        assert_eq!(live.running(), Some(&Running::Card("aurora")));
        let requests: Vec<Request> = rx.try_iter().collect();
        assert_eq!(
            names(&requests),
            ["begin", "end", "begin"],
            "the new stream runs"
        );
        let last_begin = requests
            .iter()
            .rposition(|r| matches!(r, Request::LiveBegin))
            .unwrap();
        assert!(
            requests[last_begin..]
                .iter()
                .any(|r| matches!(r, Request::LiveFrame(..))),
            "frames of the new effect are sent"
        );
        live.stop();
    }

    #[test]
    fn a_refused_stream_stops_the_engine() {
        let _g = crate::live::engine::test_serial();
        let (device, _rx) = recording_device();
        let mut live = LiveUi::with_dir(tempfile::tempdir().unwrap().keep());
        live.start_card(&device, &catalog::RIPPLE);
        assert!(live.running().is_some());
        live.on_event(&Event::LiveStarted(true));
        assert!(live.running().is_some());
        live.on_event(&Event::LiveStarted(false));
        assert!(live.running().is_none() && live.engine.is_none());
        assert!(live.status.as_ref().unwrap().1);
    }

    #[test]
    fn a_broken_stream_stops_the_engine_with_the_message() {
        let _g = crate::live::engine::test_serial();
        let ctx = egui::Context::default();
        let waker_ctx = ctx.clone();
        let device = crate::mock::spawn(std::sync::Arc::new(move || waker_ctx.request_repaint()));
        let mut live = LiveUi::with_dir(tempfile::tempdir().unwrap().keep());
        live.start_card(&device, &catalog::AURORA);
        live.on_event(&Event::Failed("Effet en direct interrompu : câble".into()));
        assert!(live.running().is_none());
        assert!(live.status.as_ref().unwrap().1);
    }
}
