//! The UI's handle on the keyboard: requests go to a worker thread, events come back.
//!
//! The worker owns a [`Manager`]: the keyboard (`Option<Keyboard<L>>`, generic over
//! [`HidLink`], so tests and `--mock` run it on [`crate::mock::Sim`]), the snapshot last
//! read from it (the baseline), and the automatic backup. Every second it looks for the
//! keyboard, or checks that it is still plugged in (host-side enumeration, nothing
//! sent). On connection: `read_snapshot`, automatic backup if the folder holds none for
//! that PID, `Connected`. Each event wakes the UI (`egui::Context::request_repaint`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use freejazz_protocol::backup::load_backup;
use freejazz_protocol::blocks::{LedEffect, Rgb};
use freejazz_protocol::calibration::AxisReport;
use freejazz_protocol::keyboard::ResetKind;
use freejazz_protocol::live::FRAME_SLOTS;
use freejazz_protocol::reconnect::{Wait, reconnect_with};
use freejazz_protocol::transport::{DeviceIdent, HidLink, HidapiLink, Presence};
use freejazz_protocol::{Error, Keyboard, Snapshot};

use crate::backups;

/// How often the worker looks for the keyboard or checks it is still there.
pub const POLL: Duration = Duration::from_secs(1);
/// After a keyboard that is plugged in did not answer the `GET_DEVICE_INFO` probe
/// (another program may hold it), how long before it is probed again.
pub const PROBE_BACKOFF: Duration = Duration::from_secs(10);
/// How long closing the app waits for the worker to write the stored lighting back.
pub const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// A mode in which the keyboard streams 0xFB reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// Key-travel test (0x66 / 0x67).
    KeyTest,
    /// Calibration (0x64 / 0x65): writes the keyboard's calibration.
    Calibration,
}

/// What the UI asks of the keyboard.
#[derive(Debug)]
pub enum Request {
    /// Read the whole snapshot again (edits are dropped).
    ReadAll,
    /// « Appliquer »: the edited snapshot. Only the blocks that differ from the
    /// baseline are checked ([`Snapshot::validate_changes`]) and written; then the
    /// keyboard is read back.
    Apply(Box<Snapshot>),
    /// Writes a backup back to the keyboard: the newest of this PID when `None`.
    Restore(Option<PathBuf>),
    /// An app profile (a snapshot file, `profiles.rs`): the keyboard's state is saved
    /// first (like a restore), then the profile is written and read back.
    ApplyProfile {
        name: String,
        path: PathBuf,
    },
    /// Hardware profile (`SET_PROFILE`), only on models with `max_profile > 1`.
    SetProfile(u8),
    // UNVERIFIED-HW: types 1, 2 and 4 were never sent by the site; never run in development.
    /// « Remise à zéro »: a pre-reset backup first, then `SET_FACTORY_RESET`, then the
    /// keyboard is read again.
    FactoryReset(ResetKind),
    /// Writes only the lighting effect, at once, so the keyboard shows a choice before
    /// « Appliquer ». The baseline is untouched: « Annuler » sends the old effect back.
    PreviewLed(Box<LedEffect>),
    LiveBegin,
    /// One live lighting frame and its amplitude byte.
    LiveFrame(Box<[Rgb; FRAME_SLOTS]>, u8),
    /// Ends the live stream: the stored effect is written back.
    LiveEnd,
    // UNVERIFIED-HW: key depths use 0x68, never sent by the site to this unit.
    Depths,
    // UNVERIFIED-HW: 0x66 / 0x64, never sent by the site to this model; never run in
    // development. The keyboard then streams 0xFB reports (`Event::Axis`).
    ProbeStart(Probe),
    ProbeStop,
    /// Ends the worker (debug: `--debug-stop-worker`).
    Stop,
}

/// What the worker reports back.
#[derive(Debug)]
pub enum Event {
    /// A keyboard was found (or re-read: `ReadAll`, restore, profile) and its snapshot
    /// read: the new baseline, edits dropped.
    Connected {
        ident: DeviceIdent,
        snapshot: Box<Snapshot>,
    },
    Disconnected,
    /// « Appliquer » done and verified: the snapshot re-read from the keyboard.
    Applied(Box<Snapshot>),
    /// What the keyboard holds after a write that did not complete: the new baseline,
    /// edits kept so they can be applied again.
    Refreshed(Box<Snapshot>),
    /// A French message for the user.
    Failed(String),
    /// A write did not complete (« Écriture interrompue : … »): the UI offers
    /// « Restaurer la dernière sauvegarde ».
    WriteInterrupted(String),
    /// The backup « Restaurer la dernière sauvegarde » would write: its path and its date
    /// (« 25/10/2026 10:03 »); `None` when there is none. Follows `WriteInterrupted`.
    RestoreTarget(Option<(PathBuf, String)>),
    /// Information for the user (automatic backup written…).
    Notice(String),
    /// Raw travel of every key slot.
    Depths(Box<[u16; FRAME_SLOTS]>),
    /// The stream mode now running (`None`: stopped, by request or by error).
    Probe(Option<Probe>),
    /// One 0xFB report.
    Axis(AxisReport),
    /// Answer to `LiveBegin`: `false` when the stream was refused (no keyboard, no
    /// backup…), so whoever started it stops feeding it.
    LiveStarted(bool),
}

/// Opens the keyboard and tells whether it is still plugged in.
pub trait Port: Send + 'static {
    type Link: HidLink + 'static;
    fn open(&mut self) -> freejazz_protocol::Result<(Self::Link, DeviceIdent)>;
    /// Host-side check only: nothing may be sent to the keyboard.
    fn present(&mut self, ident: &DeviceIdent) -> bool;
}

/// The real keyboard, through hidapi.
#[derive(Default)]
pub struct HidPort {
    presence: Presence,
}

impl Port for HidPort {
    type Link = HidapiLink;

    fn open(&mut self) -> freejazz_protocol::Result<(HidapiLink, DeviceIdent)> {
        freejazz_protocol::transport::open_keyboard()
    }

    fn present(&mut self, ident: &DeviceIdent) -> bool {
        self.presence.check(&ident.path)
    }
}

/// A protocol error, in French for the user.
pub fn french(e: &Error) -> String {
    match e {
        Error::Timeout => "le clavier ne répond plus".into(),
        Error::Denied(cmd) => format!("commande 0x{cmd:02X} interdite, non envoyée"),
        Error::Hid(msg) => format!("erreur USB ({msg})"),
        Error::NotFound => "clavier introuvable".into(),
        Error::Invalid(msg) => format!("données invalides ({msg})"),
        Error::Validation(msg) | Error::Backup(msg) => msg.clone(),
        Error::UpdatePending => e.to_string(),
    }
}

/// Why a write request failed.
#[derive(Debug)]
enum WriteError {
    /// Nothing was sent (no keyboard, a value refused, no backup…).
    Refused(String),
    /// A write did not complete or did not read back: « Écriture interrompue : … ».
    Interrupted(String),
}

impl WriteError {
    fn message(self) -> String {
        match self {
            WriteError::Refused(m) => m,
            WriteError::Interrupted(why) => format!("Écriture interrompue : {why}"),
        }
    }
}

fn refused(e: Error) -> WriteError {
    WriteError::Refused(french(&e))
}

const NO_KEYBOARD: &str = "Aucun clavier connecté";
const NO_BACKUP: &str = "Aucune sauvegarde de ce clavier : écriture refusée";

/// The keyboard as the worker holds it.
pub struct Manager<P: Port> {
    port: P,
    kb: Option<Keyboard<P::Link>>,
    ident: Option<DeviceIdent>,
    /// The snapshot last read from the keyboard, with its bytes (`raw`).
    baseline: Option<Snapshot>,
    /// Auto-backup folder; `None`: no automatic backup.
    backup_dir: Option<PathBuf>,
    /// PIDs whose automatic backup is known to exist. Nothing is written to a keyboard
    /// whose PID is not here.
    backed_up: Vec<u16>,
    /// PID of the keyboard last connected (to name the backup to restore after a write
    /// that disconnected it).
    last_pid: Option<u16>,
    wait: Wait,
    /// The effect to write back when a live stream ends.
    live: Option<LedEffect>,
    /// `settings.json`, where a running stream is marked (`liveEffect`) so the next
    /// start can end it if the app dies; `None`: no marker (tests, `--mock`).
    marker: Option<PathBuf>,
    /// An effect only previewed (`PreviewLed`) that the keyboard shows instead of the
    /// stored one: written back when the worker ends.
    previewed: Option<LedEffect>,
    /// The LED effect last written by this session, with the PID: firmware 1.28 reads
    /// its colour back as white / RGB (protocol.md 9.7), so a later read takes the
    /// colour from here when the rest of the effect matches.
    written_led: Option<(u16, LedEffect)>,
    /// No probe of the keyboard before this time (a probe went unanswered).
    next_probe: Option<Instant>,
    probe_backoff: Duration,
    /// The 0xFB stream the keyboard was asked for.
    probe: Option<Probe>,
    /// Last connection error shown, so it is not repeated every second.
    last_error: Option<String>,
    events: Vec<Event>,
}

impl<P: Port> Manager<P> {
    pub fn new(port: P, backup_dir: Option<PathBuf>) -> Self {
        Self {
            port,
            kb: None,
            ident: None,
            baseline: None,
            backup_dir,
            backed_up: Vec::new(),
            last_pid: None,
            wait: Wait::USB,
            live: None,
            marker: None,
            previewed: None,
            written_led: None,
            next_probe: None,
            probe_backoff: PROBE_BACKOFF,
            probe: None,
            last_error: None,
            events: Vec::new(),
        }
    }

    /// Marks a running stream in the settings file `path` (crash recovery).
    pub fn with_marker(mut self, path: Option<PathBuf>) -> Self {
        self.marker = path;
        self
    }

    /// How long to wait before probing again a keyboard that did not answer (10 s).
    pub fn with_probe_backoff(mut self, backoff: Duration) -> Self {
        self.probe_backoff = backoff;
        self
    }

    /// How long to wait for the keyboard after a USB re-enumeration (10 s by default).
    pub fn with_wait(mut self, wait: Wait) -> Self {
        self.wait = wait;
        self
    }

    pub fn connected(&self) -> bool {
        self.kb.is_some()
    }

    /// The snapshot last read from the keyboard.
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.baseline.as_ref()
    }

    fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// The once-a-second step: connects when no keyboard is held, otherwise checks it is
    /// still plugged in.
    pub fn tick(&mut self) -> Vec<Event> {
        match (&self.kb, &self.ident) {
            (Some(_), Some(ident)) => {
                let ident = ident.clone();
                if !self.port.present(&ident) {
                    self.disconnect();
                }
            }
            _ => self.try_connect(),
        }
        self.take_events()
    }

    fn disconnect(&mut self) {
        let was = self.kb.take().is_some() || self.ident.is_some();
        self.ident = None;
        self.baseline = None;
        self.live = None;
        self.previewed = None;
        self.probe = None;
        if was {
            self.events.push(Event::Disconnected);
        }
    }

    fn fail_once(&mut self, message: String) {
        if self.last_error.as_ref() != Some(&message) {
            self.events.push(Event::Failed(message.clone()));
            self.last_error = Some(message);
        }
    }

    fn open(&mut self) -> freejazz_protocol::Result<(Keyboard<P::Link>, DeviceIdent)> {
        let (link, ident) = self.port.open()?;
        Ok((Keyboard::connect(link)?, ident))
    }

    fn try_connect(&mut self) {
        if self.next_probe.is_some_and(|at| Instant::now() < at) {
            return; // it did not answer a moment ago: leave it alone a while
        }
        let (link, ident) = match self.port.open() {
            Ok(x) => x,
            // not plugged in (nothing was sent): look again in a second
            Err(Error::NotFound) => return,
            Err(_) => {
                self.next_probe = Some(Instant::now() + self.probe_backoff);
                return;
            }
        };
        let read = Keyboard::connect(link).and_then(|mut kb| {
            let s = kb.read_snapshot()?;
            Ok((kb, s))
        });
        let (kb, snapshot) = match read {
            Ok(x) => x,
            Err(e) => {
                self.next_probe = Some(Instant::now() + self.probe_backoff);
                self.fail_once(format!("Lecture du clavier impossible : {}", french(&e)));
                return;
            }
        };
        self.next_probe = None;
        self.last_error = None;
        self.kb = Some(kb);
        self.ident = Some(ident.clone());
        if self.last_pid != Some(snapshot.info.pid) {
            self.written_led = None;
        }
        self.last_pid = Some(snapshot.info.pid);
        let snapshot = self.set_baseline(snapshot);
        self.events.push(Event::Connected {
            ident,
            snapshot: Box::new(snapshot),
        });
        // after `Connected`, so the UI keeps a failure on screen
        if let Err(why) = self.ensure_backup() {
            self.events.push(Event::Failed(format!(
                "Sauvegarde automatique impossible : {why}"
            )));
        }
        self.recover_live();
    }

    /// The new baseline, read from the keyboard: the colour this session wrote is kept
    /// when the keyboard reads it back as white (`written_led`), a live stream will end
    /// on it (a write during a stream is not undone by its end), and no preview is left
    /// to undo (the keyboard holds what was read). Returns it.
    fn set_baseline(&mut self, mut snapshot: Snapshot) -> Snapshot {
        if let Some((pid, led)) = &self.written_led
            && *pid == snapshot.info.pid
            && led.matches_readback(&snapshot.led_effect)
        {
            snapshot.led_effect.rgb = led.rgb;
            snapshot.led_effect.color_mode = led.color_mode;
        }
        if self.live.is_some() {
            self.live = Some(snapshot.led_effect.clone());
        }
        self.previewed = None;
        self.baseline = Some(snapshot.clone());
        snapshot
    }

    /// Startup recovery: the settings file says a stream was running when the app last
    /// ended (killed, crashed): the stored effect is written back (`LiveBegin` /
    /// `LiveEnd`, backup rule as a stream) and the marker cleared. Runs on connection,
    /// in the worker, so a start hidden in the notification area recovers too.
    fn recover_live(&mut self) {
        let Some(path) = self.marker.clone() else {
            return;
        };
        if crate::settings::live_marker_in(&path).is_none() || self.require_backup().is_err() {
            return;
        }
        let (Some(kb), Some(base)) = (self.kb.as_mut(), self.baseline.as_ref()) else {
            return;
        };
        let stored = base.led_effect.clone();
        match kb.begin_live().and_then(|()| kb.end_live(&stored)) {
            Ok(()) => {
                crate::settings::set_live_marker_in(&path, None);
                self.events.push(Event::Notice(
                    "Un effet en direct tournait encore à la dernière fermeture : éclairage \
                     enregistré rétabli"
                        .into(),
                ));
            }
            Err(e) => self.events.push(Event::Failed(format!(
                "Éclairage enregistré non rétabli : {}",
                french(&e)
            ))),
        }
    }

    /// The automatic backup of the connected keyboard (by PID): taken unless the folder
    /// already holds one. `Err` says why there is none, in French.
    fn ensure_backup(&mut self) -> Result<(), String> {
        let Some(base) = self.baseline.as_ref() else {
            return Err(NO_KEYBOARD.into());
        };
        let pid = base.info.pid;
        if self.backed_up.contains(&pid) {
            return Ok(());
        }
        let Some(dir) = self.backup_dir.as_ref() else {
            return Err("dossier des sauvegardes inconnu".into());
        };
        let saved = backups::ensure_auto_backup(dir, base).map_err(|e| french(&e))?;
        self.backed_up.push(pid);
        if let Some(saved) = saved {
            let name = saved.path.file_name().unwrap_or_default().to_string_lossy();
            self.events.push(Event::Notice(match saved.unreadable {
                0 => format!("Sauvegarde automatique : {name}"),
                n => format!(
                    "Aucune sauvegarde lisible de ce clavier ({n} fichier{} illisible{}) : \
                     sauvegarde automatique {name}",
                    if n > 1 { "s" } else { "" },
                    if n > 1 { "s" } else { "" }
                ),
            }));
        }
        Ok(())
    }

    /// Before any request that writes: a backup of this keyboard must exist (retried
    /// here when the one at connection failed).
    fn require_backup(&mut self) -> Result<(), WriteError> {
        if self.kb.is_none() {
            return Err(WriteError::Refused(NO_KEYBOARD.into()));
        }
        self.ensure_backup()
            .map_err(|why| WriteError::Refused(format!("{NO_BACKUP} ({why})")))
    }

    /// Runs one request.
    pub fn handle(&mut self, request: Request) -> Vec<Event> {
        // anything else than a probe request ends the 0xFB stream first
        if !matches!(
            request,
            Request::ProbeStart(_) | Request::ProbeStop | Request::Stop | Request::Depths
        ) {
            self.probe_stop();
        }
        match request {
            Request::ReadAll => self.read_all(),
            Request::Apply(edited) => match self.apply_changes(&edited) {
                Ok(s) => self.events.push(Event::Applied(Box::new(s))),
                Err(e) => self.report_write_error(e),
            },
            Request::Restore(path) => match self.restore_backup(path) {
                Ok((s, name, before)) => {
                    self.publish_connected(s);
                    self.events.push(Event::Notice(format!(
                        "Sauvegarde restaurée : {name} (état précédent gardé dans {before})"
                    )));
                }
                Err(e) => self.report_write_error(e),
            },
            Request::ApplyProfile { name, path } => match self.restore_backup(Some(path)) {
                Ok((s, _, before)) => {
                    self.publish_connected(s);
                    self.events.push(Event::Notice(format!(
                        "Profil « {name} » appliqué (état précédent gardé dans {before})"
                    )));
                }
                Err(e) => self.report_write_error(e),
            },
            Request::SetProfile(n) => self.set_profile(n),
            Request::FactoryReset(kind) => self.factory_reset(kind),
            Request::PreviewLed(led) => self.preview_led(&led),
            Request::LiveBegin => self.live_begin(),
            Request::LiveFrame(frame, amplitude) => self.live_frame(&frame, amplitude),
            Request::LiveEnd => self.live_end(),
            Request::Depths => self.depths(),
            Request::ProbeStart(kind) => self.probe_start(kind),
            Request::ProbeStop => self.probe_stop(),
            Request::Stop => {}
        }
        self.take_events()
    }

    fn report_write_error(&mut self, e: WriteError) {
        let interrupted = matches!(e, WriteError::Interrupted(_));
        let message = e.message();
        if interrupted {
            self.events.push(Event::WriteInterrupted(message));
            let target = self.restore_target();
            self.events.push(Event::RestoreTarget(target));
        } else {
            self.events.push(Event::Failed(message));
        }
    }

    /// The backup `Restore(None)` would write for the keyboard last connected.
    fn restore_target(&self) -> Option<(PathBuf, String)> {
        let (dir, pid) = (self.backup_dir.as_ref()?, self.last_pid?);
        backups::latest_backup_info(dir, pid).map(|(b, _)| {
            let when = b.when();
            (b.path, when)
        })
    }

    fn publish_connected(&mut self, snapshot: Snapshot) {
        if let Some(ident) = self.ident.clone() {
            self.events.push(Event::Connected {
                ident,
                snapshot: Box::new(snapshot),
            });
        }
    }

    fn read_all(&mut self) {
        let Some(kb) = self.kb.as_mut() else {
            self.events.push(Event::Failed(NO_KEYBOARD.into()));
            return;
        };
        match kb.read_snapshot() {
            Ok(s) => {
                let s = self.set_baseline(s);
                self.publish_connected(s);
            }
            Err(e) => {
                let message = format!("Lecture du clavier impossible : {}", french(&e));
                if !self.still_answers() {
                    self.disconnect();
                }
                self.events.push(Event::Failed(message));
            }
        }
    }

    fn still_answers(&mut self) -> bool {
        self.kb
            .as_mut()
            .is_some_and(|kb| kb.read_device_info().is_ok())
    }

    /// « Appliquer ». `Err` is a French message; « Écriture interrompue : … » when a
    /// write did not complete (the keyboard may hold part of it).
    pub fn apply(&mut self, edited: &Snapshot) -> Result<Snapshot, String> {
        self.apply_changes(edited).map_err(WriteError::message)
    }

    /// Checks what differs from the baseline, writes only those blocks (GameMode last:
    /// a report-rate change re-enumerates the keyboard), reads everything back.
    fn apply_changes(&mut self, edited: &Snapshot) -> Result<Snapshot, WriteError> {
        let (Some(kb), Some(base)) = (self.kb.as_ref(), self.baseline.as_ref()) else {
            return Err(WriteError::Refused(NO_KEYBOARD.into()));
        };
        edited.validate_changes(base).map_err(refused)?;
        if edited.fn_keys != base.fn_keys {
            // UNVERIFIED-HW: the site never writes the Fn layer of this model.
            return Err(WriteError::Refused(
                "La couche Fn n'est pas écrite sur ce modèle".into(),
            ));
        }
        let keys = edited.keys != base.keys;
        let led = edited.led_effect != base.led_effect;
        let custom = edited.custom_leds != base.custom_leds;
        let macros = edited.macros != base.macros;
        let magnetic = edited.magnetic != base.magnetic;
        let dks = edited.dks != base.dks;
        let game_mode = edited.game_mode != base.game_mode;
        // Encoded first: nothing is sent when one block is not representable.
        if macros {
            edited.macros.to_bytes().map_err(refused)?;
        }
        if magnetic {
            edited
                .magnetic
                .encode(kb.info.rt_precision)
                .map_err(refused)?;
        }
        if dks {
            edited.dks.encode().map_err(refused)?;
        }
        if !(keys || led || custom || macros || magnetic || dks || game_mode) {
            return Ok(base.clone());
        }
        self.require_backup()?;
        let base = self.baseline.as_ref().expect("connected");
        let rate_changes = edited.game_mode.report_rate_code() != base.game_mode.report_rate_code();
        let mut expected = edited.clone();
        expected.info = base.info.clone();
        self.write_and_read_back(&expected, rate_changes, |kb| {
            if keys {
                kb.write_keys(&edited.keys)?;
            }
            if led {
                kb.write_led_effect(&edited.led_effect)?;
            }
            if custom {
                kb.write_custom_leds(&edited.custom_leds)?;
            }
            if macros {
                kb.write_macros(&edited.macros)?;
            }
            if magnetic {
                kb.write_magnetic(&edited.magnetic)?;
            }
            if dks {
                kb.write_dks(&edited.dks)?;
            }
            if game_mode {
                kb.write_game_mode(&edited.game_mode)?;
            }
            Ok(())
        })
    }

    /// Runs `write`, then reads the whole keyboard back and checks it holds `expected`
    /// ([`Snapshot::check_raw`], the backups' rule). After a report-rate change the
    /// keyboard re-enumerates: it is waited for (`self.wait`, 10 s) and reopened. A
    /// keyboard that stopped answering is disconnected. The new baseline is the
    /// snapshot read back, with its bytes; only the LED colour and `colorMode`, which
    /// firmware 1.28 reports as white / RGB after a write (protocol.md 9.7), are taken
    /// from `expected` when the rest of the effect reads back.
    fn write_and_read_back(
        &mut self,
        expected: &Snapshot,
        rate_changes: bool,
        write: impl FnOnce(&mut Keyboard<P::Link>) -> freejazz_protocol::Result<()>,
    ) -> Result<Snapshot, WriteError> {
        let Some(kb) = self.kb.as_mut() else {
            return Err(WriteError::Refused(NO_KEYBOARD.into()));
        };
        let written = write(kb);
        if rate_changes {
            // the link is gone with the old USB device: wait for the new one
            self.kb = None;
            let wait = self.wait;
            match reconnect_with(wait, || self.open()) {
                Ok((kb, ident)) => {
                    self.kb = Some(kb);
                    self.ident = Some(ident);
                }
                Err(e) => {
                    self.disconnect();
                    return Err(WriteError::Interrupted(format!(
                        "le clavier n'est pas revenu après le changement de fréquence d'envoi \
                         ({})",
                        french(&e)
                    )));
                }
            }
        } else if let Err(e) = &written
            && !self.still_answers()
        {
            self.disconnect();
            return Err(WriteError::Interrupted(french(e)));
        }
        let Some(kb) = self.kb.as_mut() else {
            return Err(WriteError::Refused(NO_KEYBOARD.into()));
        };
        let mut fresh = match kb.read_snapshot() {
            Ok(s) => s,
            Err(e) => {
                self.disconnect();
                return Err(WriteError::Interrupted(format!(
                    "relecture impossible ({})",
                    french(&e)
                )));
            }
        };
        if expected.led_effect.matches_readback(&fresh.led_effect) {
            fresh.led_effect.rgb = expected.led_effect.rgb;
            fresh.led_effect.color_mode = expected.led_effect.color_mode;
            self.written_led = Some((fresh.info.pid, expected.led_effect.clone()));
        }
        let fresh = self.set_baseline(fresh);
        let failure = match (&written, fresh.raw.as_ref()) {
            (Err(e), _) if !rate_changes => Some(french(e)),
            (_, Some(raw)) => expected.check_raw(raw).err().map(|block| {
                format!("le bloc « {block} » relu ne correspond pas à ce qui a été écrit")
            }),
            (_, None) => Some("relecture sans octets".into()),
        };
        match failure {
            None => Ok(fresh),
            Some(why) => {
                // the keyboard holds part of the write: show what it holds
                self.events.push(Event::Refreshed(Box::new(fresh)));
                Err(WriteError::Interrupted(why))
            }
        }
    }

    /// Writes a backup back (the newest of the connected PID when `path` is `None`).
    pub fn restore(&mut self, path: Option<PathBuf>) -> Result<Snapshot, String> {
        self.restore_backup(path)
            .map(|(s, _, _)| s)
            .map_err(WriteError::message)
    }

    /// What the keyboard holds is first saved as a pre-restore backup (`…-avant-
    /// restauration.json`, never picked as « la dernière sauvegarde »), so the restore
    /// can be undone; no restore without it. Then the whole backup is written
    /// (`write_snapshot`, the CLI's restore) and read back. Returns the snapshot read
    /// back, the restored file name and the pre-restore file name.
    fn restore_backup(
        &mut self,
        path: Option<PathBuf>,
    ) -> Result<(Snapshot, String, String), WriteError> {
        self.require_backup()?;
        let (Some(kb), Some(base)) = (self.kb.as_ref(), self.baseline.as_ref()) else {
            return Err(WriteError::Refused(NO_KEYBOARD.into()));
        };
        let pid = kb.info.pid;
        let (path, backup) = match path {
            Some(p) => {
                let s = load_backup(&p).map_err(refused)?;
                (p, s)
            }
            None => {
                let dir = self
                    .backup_dir
                    .as_ref()
                    .ok_or_else(|| WriteError::Refused("Dossier des sauvegardes inconnu".into()))?;
                backups::latest_backup(dir, pid)
                    .ok_or_else(|| WriteError::Refused(backups::no_backup_message(dir, pid)))?
            }
        };
        if backup.info.pid != pid {
            return Err(WriteError::Refused(format!(
                "Sauvegarde d'un autre clavier (PID {:04X}, branché : {pid:04X}) : restauration refusée",
                backup.info.pid
            )));
        }
        if backup.fn_keys.is_some() {
            // UNVERIFIED-HW: the site never writes the Fn layer of this model.
            return Err(WriteError::Refused(
                "La sauvegarde contient une couche Fn, jamais écrite sur ce modèle : \
                 restauration refusée"
                    .into(),
            ));
        }
        let dir = self
            .backup_dir
            .as_ref()
            .ok_or_else(|| WriteError::Refused("Dossier des sauvegardes inconnu".into()))?;
        let before = backups::save_pre_restore(dir, base).map_err(|e| {
            WriteError::Refused(format!(
                "Sauvegarde de l'état actuel impossible : restauration refusée ({})",
                french(&e)
            ))
        })?;
        let before = before
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let rate_changes = backup.game_mode.report_rate_code() != base.game_mode.report_rate_code();
        let mut expected = backup.clone();
        expected.info = base.info.clone();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let fresh =
            self.write_and_read_back(&expected, rate_changes, |kb| kb.write_snapshot(&backup))?;
        Ok((fresh, name, before))
    }

    // UNVERIFIED-HW: see `Request::FactoryReset`.
    /// The keyboard's current state is saved first (`…-avant-restauration.json`, so
    /// « Restaurer » can undo the reset); no backup, no reset. Then the reset is sent and
    /// everything is read again.
    fn factory_reset(&mut self, kind: ResetKind) {
        let result = self.reset_and_read(kind);
        match result {
            Ok(before) => self.events.push(Event::Notice(format!(
                "Remise à zéro envoyée (état précédent gardé dans {before})"
            ))),
            Err(e) => self.report_write_error(e),
        }
    }

    fn reset_and_read(&mut self, kind: ResetKind) -> Result<String, WriteError> {
        self.require_backup()?;
        let (Some(kb), Some(base)) = (self.kb.as_mut(), self.baseline.as_ref()) else {
            return Err(WriteError::Refused(NO_KEYBOARD.into()));
        };
        let dir = self
            .backup_dir
            .as_ref()
            .ok_or_else(|| WriteError::Refused("Dossier des sauvegardes inconnu".into()))?;
        let before = backups::save_pre_restore(dir, base).map_err(|e| {
            WriteError::Refused(format!(
                "Sauvegarde de l'état actuel impossible : remise à zéro refusée ({})",
                french(&e)
            ))
        })?;
        let before = before
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if let Err(e) = kb.factory_reset(kind) {
            return Err(WriteError::Interrupted(french(&e)));
        }
        let read = kb.read_snapshot();
        let fresh = read.map_err(|e| {
            self.disconnect();
            WriteError::Interrupted(format!("relecture impossible ({})", french(&e)))
        })?;
        // the reset changed the effect: the colour written earlier no longer applies
        self.written_led = None;
        let fresh = self.set_baseline(fresh);
        self.publish_connected(fresh);
        Ok(before)
    }

    fn set_profile(&mut self, n: u8) {
        if let Err(e) = self.require_backup() {
            self.events.push(Event::Failed(e.message()));
            return;
        }
        let Some(kb) = self.kb.as_mut() else {
            self.events.push(Event::Failed(NO_KEYBOARD.into()));
            return;
        };
        match kb.set_profile(n) {
            Ok(true) => self.read_all(),
            Ok(false) => self.events.push(Event::Failed(format!(
                "Le clavier n'a pas pris le profil {n}"
            ))),
            Err(e) => self.events.push(Event::Failed(french(&e))),
        }
    }

    /// `SET_LED_EFFECT` alone (same checks and backup rule as « Appliquer »); ignored
    /// while a live stream runs, which owns the lighting.
    fn preview_led(&mut self, led: &LedEffect) {
        if self.live.is_some() {
            return;
        }
        if let Err(e) = self.require_backup() {
            self.events.push(Event::Failed(e.message()));
            return;
        }
        let (Some(kb), Some(base)) = (self.kb.as_mut(), self.baseline.as_ref()) else {
            return;
        };
        let mut probe = base.clone();
        probe.led_effect = led.clone();
        if let Err(e) = probe.validate_changes(base) {
            self.events.push(Event::Failed(french(&e)));
            return;
        }
        match kb.write_led_effect(led) {
            Ok(()) => self.previewed = (*led != base.led_effect).then(|| led.clone()),
            Err(e) => self.events.push(Event::Failed(french(&e))),
        }
    }

    /// The stream itself changes nothing stored, but its end writes the effect back.
    /// Answered by `LiveStarted`; a started stream is marked in the settings file.
    fn live_begin(&mut self) {
        let started = self.require_backup().and_then(|()| {
            let (Some(kb), Some(base)) = (self.kb.as_mut(), self.baseline.as_ref()) else {
                return Err(WriteError::Refused(NO_KEYBOARD.into()));
            };
            kb.begin_live().map_err(refused)?;
            Ok(base.led_effect.clone())
        });
        match started {
            Ok(stored) => {
                self.live = Some(stored);
                if let Some(path) = &self.marker {
                    crate::settings::set_live_marker_in(path, Some("en cours"));
                }
                self.events.push(Event::LiveStarted(true));
            }
            Err(e) => {
                self.events.push(Event::Failed(e.message()));
                self.events.push(Event::LiveStarted(false));
            }
        }
    }

    fn live_frame(&mut self, frame: &[Rgb; FRAME_SLOTS], amplitude: u8) {
        if self.live.is_none() {
            return;
        }
        let Some(kb) = self.kb.as_mut() else { return };
        if let Err(e) = kb.send_frame(frame, amplitude) {
            let message = format!("Effet en direct interrompu : {}", french(&e));
            if !self.still_answers() {
                self.disconnect();
            } else {
                self.live_end();
            }
            self.events.push(Event::Failed(message));
        }
    }

    /// Writes the stored effect back. The marker is cleared only once that write went
    /// through: otherwise the next start does it.
    fn live_end(&mut self) {
        let Some(stored) = self.live.take() else {
            return;
        };
        let Some(kb) = self.kb.as_mut() else {
            return;
        };
        match kb.end_live(&stored) {
            Ok(()) => {
                if let Some(path) = &self.marker {
                    crate::settings::set_live_marker_in(path, None);
                }
            }
            Err(e) => self.events.push(Event::Failed(format!(
                "Éclairage enregistré non rétabli : {}",
                french(&e)
            ))),
        }
    }

    /// Whether a live stream runs (`LiveBegin` accepted, not ended).
    pub fn streaming(&self) -> bool {
        self.live.is_some()
    }

    // UNVERIFIED-HW: 0x68 (see `Keyboard::read_key_depths`).
    fn depths(&mut self) {
        let Some(kb) = self.kb.as_mut() else {
            self.events.push(Event::Failed(NO_KEYBOARD.into()));
            return;
        };
        match kb.read_key_depths() {
            Ok(d) => self.events.push(Event::Depths(Box::new(d))),
            Err(e) => self.events.push(Event::Failed(format!(
                "Profondeur des touches illisible : {}",
                french(&e)
            ))),
        }
    }

    // UNVERIFIED-HW: 0x66 / 0x64 (see `calibration.rs`).
    fn probe_start(&mut self, kind: Probe) {
        self.probe_stop();
        let started = match kind {
            // calibration changes what the keyboard holds: a backup first
            Probe::Calibration => {
                self.require_backup()
                    .map_err(WriteError::message)
                    .and_then(|()| {
                        let kb = self.kb.as_mut().ok_or_else(|| NO_KEYBOARD.to_owned())?;
                        kb.start_calibration().map_err(|e| french(&e))
                    })
            }
            Probe::KeyTest => match self.kb.as_mut() {
                Some(kb) => kb.start_key_test().map_err(|e| french(&e)),
                None => Err(NO_KEYBOARD.to_owned()),
            },
        };
        match started {
            Ok(()) => {
                self.probe = Some(kind);
                self.events.push(Event::Probe(Some(kind)));
            }
            Err(why) => {
                self.events.push(Event::Failed(why));
                self.events.push(Event::Probe(None));
            }
        }
    }

    /// Stops the 0xFB stream if one runs (the stop commands have no reply).
    fn probe_stop(&mut self) {
        let Some(kind) = self.probe.take() else {
            return;
        };
        if let Some(kb) = self.kb.as_mut() {
            let _ = match kind {
                Probe::KeyTest => kb.stop_key_test(),
                Probe::Calibration => kb.stop_calibration(),
            };
        }
        self.events.push(Event::Probe(None));
    }

    /// Whether the worker should poll for 0xFB reports.
    pub fn probing(&self) -> bool {
        self.probe.is_some()
    }

    /// Drains the 0xFB reports that arrived (a few ms of waiting at most).
    pub fn pump_probe(&mut self) -> Vec<Event> {
        let Some(kb) = self.kb.as_mut().filter(|_| self.probe.is_some()) else {
            return Vec::new();
        };
        for _ in 0..64 {
            match kb.read_axis_report(5) {
                Ok(Some(report)) => self.events.push(Event::Axis(report)),
                Ok(None) => break,
                Err(e) => {
                    let message = format!("Flux du clavier interrompu : {}", french(&e));
                    if !self.still_answers() {
                        self.disconnect();
                    } else {
                        self.probe = None;
                        self.events.push(Event::Probe(None));
                    }
                    self.events.push(Event::Failed(message));
                    break;
                }
            }
        }
        self.take_events()
    }

    /// Before the worker ends: a live stream gives the keyboard its effect back, and so
    /// does an effect only previewed.
    pub fn shutdown(&mut self) {
        self.probe_stop();
        self.live_end();
        if self.previewed.take().is_some()
            && let (Some(kb), Some(base)) = (self.kb.as_mut(), self.baseline.as_ref())
            && let Err(e) = kb.write_led_effect(&base.led_effect)
        {
            self.events.push(Event::Failed(format!(
                "Éclairage enregistré non rétabli : {}",
                french(&e)
            )));
        }
    }
}

/// The worker loop: requests as they come, [`Manager::tick`] every [`POLL`]. Live
/// frames queued behind each other are coalesced (only the newest is sent).
pub fn run<P: Port>(io: WorkerIo, mut manager: Manager<P>) {
    let emit = |events: Vec<Event>| events.into_iter().all(|e| io.emit(e));
    if !emit(manager.tick()) {
        return;
    }
    let mut last_tick = Instant::now();
    let mut held: Option<Request> = None;
    loop {
        let mut wait = POLL.saturating_sub(last_tick.elapsed());
        if manager.probing() {
            wait = wait.min(Duration::from_millis(15));
        }
        let next = match held.take() {
            Some(r) => Ok(r),
            None => io.requests.recv_timeout(wait),
        };
        let events = match next {
            Ok(Request::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Request::LiveFrame(mut frame, mut amplitude)) => {
                loop {
                    match io.requests.try_recv() {
                        Ok(Request::LiveFrame(f, a)) => (frame, amplitude) = (f, a),
                        Ok(other) => {
                            held = Some(other);
                            break;
                        }
                        Err(_) => break,
                    }
                }
                manager.handle(Request::LiveFrame(frame, amplitude))
            }
            Ok(request) => manager.handle(request),
            Err(RecvTimeoutError::Timeout) => manager.pump_probe(),
        };
        if !emit(events) {
            break;
        }
        if last_tick.elapsed() >= POLL {
            last_tick = Instant::now();
            if !emit(manager.tick()) {
                break;
            }
        }
    }
    manager.shutdown();
}

/// Wakes the UI after an event (the UI only repaints on demand).
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// The worker's side of the channels.
pub struct WorkerIo {
    pub requests: Receiver<Request>,
    events: Sender<Event>,
    waker: Waker,
}

impl WorkerIo {
    /// Sends `event` and wakes the UI; `false` once the UI is gone.
    pub fn emit(&self, event: Event) -> bool {
        let sent = self.events.send(event).is_ok();
        (self.waker)();
        sent
    }
}

/// Which keyboard the handle talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// The keyboard plugged in, through hidapi.
    Real,
    /// The fake keyboard of `--mock`.
    Mock,
    /// No worker: never connects (`--offline`).
    Offline,
}

pub struct DeviceHandle {
    kind: DeviceKind,
    requests: Option<Sender<Request>>,
    events: Option<Receiver<Event>>,
    /// The worker thread, joined (briefly) when the handle goes.
    thread: Option<JoinHandle<()>>,
}

/// Name of the worker thread.
const WORKER_THREAD: &str = "freejazz-device";

/// The worker of the app, for the panic hook: its request channel and whether it has
/// ended.
static WORKER: Mutex<Option<(Sender<Request>, Arc<AtomicBool>)>> = Mutex::new(None);

/// For the panic hook: asks the worker to end (it ends any stream and preview, writing
/// the stored lighting back) and waits for it, `timeout` at most. Nothing to wait for on
/// the worker's own thread.
pub fn emergency_stop(timeout: Duration) {
    let worker = WORKER.lock().unwrap_or_else(|e| e.into_inner()).take();
    let Some((tx, done)) = worker else {
        return;
    };
    let _ = tx.send(Request::Stop);
    if thread::current().name() == Some(WORKER_THREAD) {
        return;
    }
    let deadline = Instant::now() + timeout;
    while !done.load(Ordering::SeqCst) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
}

impl DeviceHandle {
    /// Runs `worker` on its own thread.
    pub fn spawn(
        kind: DeviceKind,
        waker: Waker,
        worker: impl FnOnce(WorkerIo) + Send + 'static,
    ) -> Self {
        let (req_tx, req_rx) = channel();
        let (ev_tx, ev_rx) = channel();
        let io = WorkerIo {
            requests: req_rx,
            events: ev_tx,
            waker,
        };
        let done = Arc::new(AtomicBool::new(false));
        let thread = {
            let done = Arc::clone(&done);
            thread::Builder::new()
                .name(WORKER_THREAD.into())
                .spawn(move || {
                    worker(io);
                    done.store(true, Ordering::SeqCst);
                })
                .expect("spawn device thread")
        };
        *WORKER.lock().unwrap_or_else(|e| e.into_inner()) = Some((req_tx.clone(), done));
        Self {
            kind,
            requests: Some(req_tx),
            events: Some(ev_rx),
            thread: Some(thread),
        }
    }

    /// The keyboard plugged in; automatic backups in `%APPDATA%\Freejazz\backups`; a
    /// running stream marked in `settings.json`.
    pub fn real(waker: Waker) -> Self {
        let manager = Manager::new(HidPort::default(), crate::paths::backups_dir())
            .with_marker(crate::paths::settings_file());
        Self::spawn(DeviceKind::Real, waker, move |io| run(io, manager))
    }

    pub fn offline() -> Self {
        Self {
            kind: DeviceKind::Offline,
            requests: None,
            events: None,
            thread: None,
        }
    }

    /// Ends the worker: `Stop` goes after every request already sent (a `LiveEnd`, a
    /// preview undone…), the worker then ends any stream and preview itself, and the
    /// thread is waited for, `timeout` at most. `true` once it has ended.
    pub fn shutdown(&mut self, timeout: Duration) -> bool {
        if let Some(tx) = self.requests.take() {
            let _ = tx.send(Request::Stop);
        }
        let Some(thread) = self.thread.take() else {
            return true;
        };
        let deadline = Instant::now() + timeout;
        while !thread.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        if thread.is_finished() {
            let _ = thread.join();
            true
        } else {
            false
        }
    }

    pub fn kind(&self) -> DeviceKind {
        self.kind
    }

    /// A second way into the worker, for the live effects engine (its own thread).
    pub fn sender(&self) -> Option<Sender<Request>> {
        self.requests.clone()
    }

    pub fn send(&self, request: Request) {
        if let Some(tx) = &self.requests {
            // A gone worker shows up as `Disconnected` on the next poll.
            let _ = tx.send(request);
        }
    }

    /// Writes a backup back to the keyboard (the newest of its PID when `None`).
    pub fn restore_backup(&self, path: Option<PathBuf>) {
        self.send(Request::Restore(path));
    }

    /// The next pending event, if any. A worker that stopped reports `Disconnected`
    /// once.
    pub fn poll(&mut self) -> Option<Event> {
        match self.events.as_ref()?.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.events = None;
                self.requests = None;
                Some(Event::Disconnected)
            }
        }
    }
}

impl Drop for DeviceHandle {
    fn drop(&mut self) {
        self.shutdown(SHUTDOWN_WAIT);
    }
}
