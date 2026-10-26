//! Application state and the eframe app.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::device::{DeviceHandle, DeviceKind, Request};
use crate::tray::{self, TrayCommand, TrayFlags};
use crate::ui::{self, UiAction};

pub use crate::state::{AppState, Link};

/// The sections of the left navigation, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    Keys,
    Lighting,
    Live,
    Macros,
    Performance,
    Advanced,
    Settings,
    Test,
}

impl Section {
    pub const ALL: [Section; 8] = [
        Section::Keys,
        Section::Lighting,
        Section::Live,
        Section::Macros,
        Section::Performance,
        Section::Advanced,
        Section::Settings,
        Section::Test,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Keys => "Touches",
            Section::Lighting => "Éclairage",
            Section::Live => "Effets en direct",
            Section::Macros => "Macros",
            Section::Performance => "Performance",
            Section::Advanced => "Touches avancées",
            Section::Settings => "Réglages",
            Section::Test => "Test et calibrage",
        }
    }

    /// Identifier for `--section`.
    pub fn id(self) -> &'static str {
        match self {
            Section::Keys => "keys",
            Section::Lighting => "lighting",
            Section::Live => "live",
            Section::Macros => "macros",
            Section::Performance => "performance",
            Section::Advanced => "advanced",
            Section::Settings => "settings",
            Section::Test => "test",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// `--screenshot`: render, save a PNG, quit.
pub struct ScreenshotJob {
    pub path: PathBuf,
    frames: u32,
    requested: bool,
}

/// The screenshot is taken at the latest this long after start, whatever the keyboard
/// does (no keyboard, a worker that failed or never answers).
const SCREENSHOT_DEADLINE_S: f64 = 20.0;

impl ScreenshotJob {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            frames: 0,
            requested: false,
        }
    }
}

pub struct Options {
    pub mock: bool,
    /// No keyboard worker at all.
    pub offline: bool,
    /// Debug: stop the keyboard worker once connected (« Clavier déconnecté »).
    pub stop_worker: bool,
    pub section: Option<Section>,
    pub screenshot: Option<PathBuf>,
    /// Screenshot aid: pretend one change is pending.
    pub demo_dirty: bool,
    /// Screenshot aid: edit the custom colours, starting from this bundled design.
    pub demo_design: Option<String>,
    /// Screenshot aid: show an interrupted write once connected.
    pub demo_error: bool,
    /// Screenshot aid: add a sample macro and select it.
    pub demo_macros: bool,
    /// Screenshot aid: a sample layered effect in the editor of « Effets en direct ».
    pub demo_live: bool,
    /// Screenshot aid: sample key-test (`test`) or calibration (`calibration`) state.
    pub demo_test: Option<String>,
    /// Screenshot aid: a profile dialog (`save` or `apply`) open once connected.
    pub demo_profile: Option<String>,
    pub theme: Option<egui::Theme>,
    /// Screenshot aid: keys selected once connected, a group id (`gaming`) or labels
    /// (`Z,Q,S,D`).
    pub select: Option<String>,
    /// Window placed off all monitors (screenshots, measurements).
    pub offscreen: bool,
    /// `--tray`: start with the window hidden (launch with Windows).
    pub start_hidden: bool,
    /// The « show your window » event of the single-instance guard (real runs only).
    pub show_signal: Option<crate::single_instance::ShowSignal>,
    /// The notification-area icon; `None`: no icon.
    pub tray_icon: Option<egui::IconData>,
}

/// After this long without input the working set is given back to Windows.
const IDLE_TRIM: Duration = Duration::from_secs(90);
/// The working set is also given back this long after start-up, once the keyboard is
/// connected (start-up touches far more memory than the app then uses).
const STARTUP_TRIM: Duration = Duration::from_secs(5);
/// Start-up counts as done after this long even without a keyboard.
const STARTUP_MAX: Duration = Duration::from_secs(20);
/// Frames shown before a `--tray` start hides the window again.
const HIDE_FRAMES: u8 = 3;

pub struct FreejazzApp {
    state: AppState,
    device: DeviceHandle,
    screenshot: Option<ScreenshotJob>,
    demo_dirty: bool,
    demo_design: Option<String>,
    demo_error: bool,
    demo_macros: bool,
    demo_profile: Option<String>,
    /// `--debug-stop-worker`: `Stop` not sent yet.
    stop_worker: bool,
    /// `Stop` was sent.
    stopped: bool,
    /// A keyboard was connected at some point (screenshot readiness).
    was_connected: bool,
    /// Keys selected on the keyboard view, by layout index.
    selected: BTreeSet<u8>,
    /// `--select`, applied once connected.
    select: Option<String>,
    sections: crate::sections::SectionsUi,
    tray_rx: Receiver<TrayCommand>,
    tray: TrayFlags,
    /// Frames left before a hidden start hides the window.
    hide_frames: u8,
    /// A real exit was asked (tray « Quitter », screenshot): closing does not hide.
    quitting: bool,
    /// An update downloaded and checked: installed once the app has ended cleanly.
    pending_install: Option<PathBuf>,
    last_active: Instant,
    idle_trimmed: bool,
    started: Instant,
    /// When start-up was over (keyboard connected, or none after a while).
    ready_at: Option<Instant>,
    startup_trimmed: bool,
    /// Focus and minimised state at the previous pass (memory trim on change).
    was_focused: bool,
    was_minimized: bool,
    /// Settings are written to disk (not in `--mock` / `--offline`).
    persist: bool,
}

impl FreejazzApp {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options) -> Self {
        let ctx = cc.egui_ctx.clone();
        ui::theme::install(&ctx, opts.theme);
        let waker_ctx = ctx.clone();
        let waker = Arc::new(move || waker_ctx.request_repaint());
        let device = if opts.offline {
            DeviceHandle::offline()
        } else if opts.mock {
            crate::mock::spawn(waker)
        } else {
            DeviceHandle::real(waker)
        };
        let mut sections = crate::sections::SectionsUi::default();
        sections.live.configure(opts.mock);
        if opts.demo_live {
            sections.live.demo();
        }
        // the low-level key hook only on a real run
        sections.macros.allow_hook(!opts.mock && !opts.offline);
        sections.test.keys.allow_hook(!opts.mock && !opts.offline);
        if let Some(what) = &opts.demo_test {
            sections.test.demo(what);
        }
        let persist = !opts.mock && !opts.offline;
        sections.settings.app = crate::settings::Settings::load();
        sections.settings.real_autostart = persist;
        if persist && sections.settings.app.autostart {
            // keeps the registered path current if the exe moved
            let _ = crate::autostart::set(true);
        }
        // the updater touches the network only on a real run (never mock, offline or
        // screenshots)
        let online = persist && opts.screenshot.is_none() && !opts.offscreen;
        sections.settings.updater = crate::updater::Updater::new(online, ctx.clone());
        if online {
            crate::updater::cleanup_old();
            if sections.settings.app.check_updates {
                sections.settings.updater.check(false);
            }
        }
        let (tray_tx, tray_rx) = std::sync::mpsc::channel();
        let tray = TrayFlags::default();
        if opts.screenshot.is_none()
            && !opts.offline
            && let Some(icon) = opts.tray_icon
        {
            tray::spawn(opts.show_signal, icon, tray_tx, ctx.clone(), tray.clone());
        }
        let offline = device.kind() == DeviceKind::Offline;
        let mut state = AppState::new(opts.mock, offline);
        if let Some(section) = opts.section {
            state.section = section;
        }
        Self {
            state,
            device,
            screenshot: opts.screenshot.map(ScreenshotJob::new),
            demo_dirty: opts.demo_dirty,
            demo_design: opts.demo_design,
            demo_error: opts.demo_error,
            demo_macros: opts.demo_macros,
            demo_profile: opts.demo_profile,
            stop_worker: opts.stop_worker,
            stopped: false,
            was_connected: false,
            selected: BTreeSet::new(),
            select: opts.select,
            sections,
            tray_rx,
            tray,
            hide_frames: if opts.start_hidden { HIDE_FRAMES } else { 0 },
            quitting: false,
            pending_install: None,
            last_active: Instant::now(),
            idle_trimmed: false,
            started: Instant::now(),
            ready_at: None,
            startup_trimmed: false,
            was_focused: false,
            was_minimized: false,
            persist,
        }
    }

    /// Hides the window in the notification area: nothing is drawn, memory is trimmed.
    /// What only makes sense with the window in front ends: a macro recording, the key
    /// test and its hook, the custom colours streamed and an effect only previewed (the
    /// keyboard goes back to what it stores). A live effect keeps running.
    fn hide(&mut self, ctx: &egui::Context) {
        release_on_hide(&mut self.sections, &self.device);
        self.tray.hidden.store(true, Ordering::SeqCst);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        crate::mem::trim_working_set();
    }

    fn show(&mut self, ctx: &egui::Context) {
        self.tray.hidden.store(false, Ordering::SeqCst);
        self.last_active = Instant::now();
        self.idle_trimmed = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    /// Real exit: a running live effect ends first (the stored effect is written back).
    fn quit(&mut self, ctx: &egui::Context) {
        self.sections.live.stop();
        self.quitting = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Saves the app options when they changed, and what the live engine is running.
    fn persist_settings(&mut self) {
        let running = self.sections.live.running().map(|r| match r {
            crate::sections::live::Running::Card(id) => (*id).to_owned(),
            crate::sections::live::Running::Saved(name) => name.clone(),
        });
        let live_changed = self.sections.settings.app.note_live(running.as_deref());
        let edited = self.sections.settings.take_app_changed();
        if (live_changed || edited) && self.persist {
            let _ = self.sections.settings.app.save();
        }
    }

    fn handle(&mut self, action: UiAction) {
        match action {
            UiAction::Select(section) => self.state.section = section,
            UiAction::Revert => {
                self.sections.lighting.on_revert(&self.device);
                self.state.revert();
            }
            UiAction::Apply => {
                // the previews end first: their end would otherwise put the old effect back
                self.sections.lighting.end_live(&self.device);
                if let Some(edited) = self.state.begin_apply() {
                    self.sections.live.stop_for(WRITING);
                    self.device.send(Request::Apply(Box::new(edited)));
                }
            }
            UiAction::Reload => {
                self.sections.lighting.end_live(&self.device);
                self.sections.lighting.on_revert(&self.device);
                self.device.send(Request::ReadAll);
            }
            UiAction::RestoreLatest => {
                self.sections.lighting.end_live(&self.device);
                if self.state.connected && !self.state.busy {
                    self.sections.live.stop_for(WRITING);
                    self.state.busy = true;
                    self.state.error = None;
                    self.state.offer_restore = false;
                    let target = self.state.restore_target.as_ref().map(|t| t.0.clone());
                    self.device.restore_backup(target);
                }
            }
            UiAction::DismissError => {
                self.state.error = None;
                self.state.offer_restore = false;
            }
        }
    }

    /// Gives the working set back to Windows a few seconds after start-up (once the
    /// keyboard is read: start-up touches far more than the app then uses) and whenever
    /// the window loses the focus or is minimised. The pages come back on demand.
    fn trim_memory(&mut self, ctx: &egui::Context) {
        if !self.startup_trimmed {
            if self.ready_at.is_none()
                && (self.state.connected
                    || self.device.kind() == DeviceKind::Offline
                    || self.started.elapsed() >= STARTUP_MAX)
            {
                self.ready_at = Some(Instant::now());
            }
            match self.ready_at.map(|at| at.elapsed()) {
                Some(waited) if waited >= STARTUP_TRIM => {
                    self.startup_trimmed = true;
                    crate::mem::trim_working_set();
                }
                Some(waited) => ctx.request_repaint_after(STARTUP_TRIM - waited),
                None => ctx.request_repaint_after(Duration::from_secs(1)),
            }
        }
        let (focused, minimized) = ctx.input(|i| {
            let v = i.viewport();
            (v.focused.unwrap_or(false), v.minimized.unwrap_or(false))
        });
        if (self.was_focused && !focused) || (minimized && !self.was_minimized) {
            crate::mem::trim_working_set();
        }
        self.was_focused = focused;
        self.was_minimized = minimized;
    }

    fn screenshot_step(&mut self, ui: &egui::Ui) {
        let Some(job) = &mut self.screenshot else {
            return;
        };
        let ctx = ui.ctx();
        let shot = ui.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            if let Err(e) = save_png(&job.path, &image) {
                eprintln!("capture : {e}");
            }
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        // Wait for the keyboard (connected, failed, or stopped by `--debug-stop-worker`),
        // a few frames when offline, at most `SCREENSHOT_DEADLINE_S`; then a settled
        // layout.
        let settled = if self.stopped || self.stop_worker {
            self.was_connected && self.state.link == Link::Disconnected
        } else {
            self.state.connected || self.state.error.is_some()
        };
        let ready = settled
            || self.device.kind() == DeviceKind::Offline
            || ui.input(|i| i.time) > SCREENSHOT_DEADLINE_S;
        if ready {
            job.frames += 1;
        }
        if job.frames >= 4 && !job.requested {
            job.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint();
    }
}

/// Why a running live effect is stopped before a write.
const WRITING: &str = "le clavier est en cours d'écriture";

/// Hiding the window: see [`FreejazzApp::hide`].
fn release_on_hide(sections: &mut crate::sections::SectionsUi, device: &DeviceHandle) {
    sections.macros.leave();
    sections.test.leave(device);
    sections.lighting.end_live(device);
    sections.lighting.on_revert(device);
}

/// Keys of `--select`: a group id or labels separated by commas.
fn select_spec(spec: &str) -> BTreeSet<u8> {
    use crate::keyboard::{Group, Layout, groups, labels};
    let layout = Layout::ak980max();
    let by_index = labels::key_labels();
    match Group::from_id(spec) {
        Some(group) => group.keys(layout, &by_index),
        None => {
            let wanted: Vec<&str> = spec.split(',').map(str::trim).collect();
            groups::by_label(layout, &by_index, &wanted)
        }
    }
}

fn save_png(path: &std::path::Path, image: &egui::ColorImage) -> std::io::Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, image.size[0] as u32, image.size[1] as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
    writer
        .write_image_data(image.as_raw())
        .map_err(std::io::Error::other)?;
    writer.finish().map_err(std::io::Error::other)
}

impl eframe::App for FreejazzApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // A running live effect and the custom colours streamed end with the stored
        // effect written back, an effect only previewed gives way to the stored one.
        // Then the worker is asked to stop and waited for: those writes are done (and
        // the crash marker cleared by the worker) before the process goes.
        self.sections.live.stop();
        crate::live::end_active();
        self.sections.lighting.end_live(&self.device);
        self.sections.lighting.on_revert(&self.device);
        self.sections.test.leave(&self.device);
        self.device.shutdown(crate::device::SHUTDOWN_WAIT);
        // the update replaces the executables only now that everything is stopped
        if let Some(staged) = self.pending_install.take() {
            crate::updater::install_and_relaunch(&staged);
        }
    }

    /// Runs also while the window is hidden: tray commands, the close button, keyboard
    /// events (a live effect must notice a disconnection) and the memory trim.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.hide_frames > 0 {
            self.hide_frames -= 1;
            if self.hide_frames == 0 {
                self.hide(ctx);
            } else {
                ctx.request_repaint();
            }
        }
        while let Ok(cmd) = self.tray_rx.try_recv() {
            match cmd {
                TrayCommand::Open => self.show(ctx),
                TrayCommand::StopEffect => self.sections.live.stop(),
                TrayCommand::Quit => self.quit(ctx),
            }
        }
        if ctx.input(|i| i.viewport().close_requested())
            && self.sections.settings.app.close_to_tray
            && self.tray.ready.load(Ordering::SeqCst)
            && !self.quitting
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide(ctx);
        }
        while let Some(event) = self.device.poll() {
            self.sections.test.on_event(&event);
            self.sections.lighting.on_event(&event);
            self.sections.live.on_event(&event);
            self.state.apply(event);
        }
        if let Some(staged) = self.sections.settings.updater.poll() {
            self.pending_install = Some(staged);
            self.quit(ctx);
        }
        self.persist_settings();
        self.trim_memory(ctx);
        // a window left alone gives its memory back
        let hidden = self.tray.hidden.load(Ordering::SeqCst);
        if !hidden && !self.idle_trimmed {
            let idle = self.last_active.elapsed();
            if idle >= IDLE_TRIM {
                self.idle_trimmed = true;
                crate::mem::trim_working_set();
            } else {
                ctx.request_repaint_after(IDLE_TRIM - idle);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if ui.input(|i| !i.events.is_empty() || i.pointer.velocity() != egui::Vec2::ZERO) {
            self.last_active = Instant::now();
            self.idle_trimmed = false;
        }
        if self.state.connected {
            self.was_connected = true;
            if self.demo_error {
                self.demo_error = false;
                self.state.apply(crate::device::Event::WriteInterrupted(
                    "Écriture interrompue : le clavier ne répond plus".into(),
                ));
                self.state.apply(crate::device::Event::RestoreTarget(Some((
                    std::path::PathBuf::new(),
                    "25/10/2026 10:03".into(),
                ))));
            }
            if let Some(spec) = self.select.take() {
                self.selected = select_spec(&spec);
            }
            if self.stop_worker {
                self.stop_worker = false;
                self.stopped = true;
                self.device.send(Request::Stop);
            }
        }
        if self.demo_dirty
            && let Some(edited) = &mut self.state.edited
        {
            edited.led_effect.brightness = edited.led_effect.brightness.saturating_sub(2);
            self.demo_dirty = false;
        }
        if self.state.connected
            && let Some(name) = self.demo_design.take()
            && let Some(edited) = &mut self.state.edited
            && let Some(d) = crate::designs::bundled().into_iter().find(|d| d.name == name)
        {
            edited.led_effect.mode = freejazz_protocol::blocks::led::MODE_CUSTOM;
            edited.custom_leds = d.leds;
        }
        if self.demo_macros
            && let Some(edited) = &mut self.state.edited
        {
            self.demo_macros = false;
            self.sections.macros.demo(&mut edited.macros);
        }
        if self.state.connected
            && let Some(what) = self.demo_profile.take()
        {
            self.sections.profiles.demo(&what, &self.state);
        }
        self.state.refresh_dirty();
        self.sections.lighting.update_preview(&self.state, ui.ctx());
        self.sections.live.update_preview(&self.state, ui.ctx());

        for action in ui::shell::show(ui, &mut self.state, &mut self.selected, &mut self.sections) {
            self.handle(action);
        }
        if let Some(request) = self
            .sections
            .settings
            .take_request()
            .or_else(|| self.sections.profiles.take_request())
            && self.state.connected
            && !self.state.busy
        {
            // a restore or a reset: the previews end first, the write may take a while
            self.sections.lighting.end_live(&self.device);
            self.sections.live.stop_for(WRITING);
            self.state.busy = true;
            self.state.error = None;
            self.state.offer_restore = false;
            self.device.send(request);
        }
        self.sections.live.run_pending(&self.device);
        // a macro recording ends with its section
        if self.state.section != Section::Macros {
            self.sections.macros.leave();
        }
        // the key test and the calibration only run while their section is shown
        if self.state.section != Section::Test {
            self.sections.test.leave(&self.device);
        }
        for request in self.sections.test.take_requests() {
            if self.state.connected && !self.state.busy {
                self.sections.lighting.end_live(&self.device);
                if matches!(request, Request::ProbeStart(_)) {
                    // a live frame would end the keyboard's 0xFB stream at once
                    self.sections
                        .live
                        .stop_for("le test des touches utilise le clavier");
                }
                self.device.send(request);
            }
        }
        let engine = self.sections.live.running().is_some();
        self.sections
            .lighting
            .pump(&self.state, &self.device, ui.ctx(), engine);
        self.state.refresh_dirty();
        self.screenshot_step(ui);
    }

    fn clear_color(&self, visuals: &egui::Visuals) -> [f32; 4] {
        ui::theme::Palette::for_dark(visuals.dark_mode)
            .bg
            .to_normalized_gamma_f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hiding_ends_the_previews_and_the_recording() {
        let ctx = egui::Context::default();
        let waker_ctx = ctx.clone();
        let device = crate::mock::spawn(Arc::new(move || waker_ctx.request_repaint()));
        let mut sections = crate::sections::SectionsUi::default();
        let mut state = AppState::new(true, false);
        let snapshot = crate::mock::read_fixture().unwrap();
        state.apply(crate::device::Event::Connected {
            ident: crate::mock::ident(&snapshot),
            snapshot: Box::new(snapshot),
        });
        state.section = Section::Lighting;
        state.edited.as_mut().unwrap().led_effect.mode = 128;
        sections.lighting.pump(&state, &device, &ctx, false);
        assert!(sections.lighting.previewing());
        release_on_hide(&mut sections, &device);
        assert!(!sections.lighting.previewing());
        assert!(!sections.macros.is_recording());
    }

    #[test]
    fn section_ids_roundtrip() {
        for s in Section::ALL {
            assert_eq!(Section::from_id(s.id()), Some(s));
        }
        assert_eq!(Section::Test.title(), "Test et calibrage");
        assert_eq!(Section::from_id("nope"), None);
    }
}
