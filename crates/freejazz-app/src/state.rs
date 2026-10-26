//! What the UI shows: the keyboard's snapshot, the edited clone, the connection.

use freejazz_protocol::Snapshot;
use freejazz_protocol::live::FRAME_SLOTS;
use freejazz_protocol::transport::DeviceIdent;

use crate::app::Section;
use crate::device::Event;

/// Connection to the keyboard, as the header shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Searching,
    Connected,
    Disconnected,
}

/// State shared with the UI. `snapshot` is what the keyboard holds; `edited` is the
/// clone the sections change (the blocks' raw-preserving fields travel with it).
///
/// Edits made while a write is in flight are kept: when the write ends, `edited` is
/// replaced by the keyboard's snapshot only if it is still exactly what was sent;
/// otherwise it stays, and `dirty` tells what remains to apply.
pub struct AppState {
    pub connected: bool,
    pub link: Link,
    pub ident: Option<DeviceIdent>,
    pub snapshot: Option<Snapshot>,
    pub edited: Option<Snapshot>,
    /// `edited` differs from `snapshot`.
    pub dirty: bool,
    /// A write is in progress.
    pub busy: bool,
    /// What « Appliquer » sent, while `busy`.
    pub sent: Option<Snapshot>,
    pub section: Section,
    /// Last error, in French.
    pub error: Option<String>,
    /// The error is an interrupted write: offer « Restaurer la dernière sauvegarde ».
    pub offer_restore: bool,
    /// Last information message (automatic backup…).
    pub notice: Option<String>,
    /// The backup « Restaurer la dernière sauvegarde » writes: path and date.
    pub restore_target: Option<(std::path::PathBuf, String)>,
    /// Edits held when a write ended in a disconnect: given back on reconnection.
    pub carried_edits: Option<Snapshot>,
    /// Raw key travel, when read.
    pub depths: Option<Box<[u16; FRAME_SLOTS]>>,
    pub mock: bool,
    /// « Fonctions expérimentales »: unlocks the actions never sent to this keyboard
    /// (partial resets, calibration, key depth). Off at startup, never saved.
    pub experimental: bool,
    /// App profile in use (profiles stored on the PC; `max_profile` is 0 on this unit).
    pub profile: String,
}

impl AppState {
    pub fn new(mock: bool, offline: bool) -> Self {
        Self {
            connected: false,
            link: if offline {
                Link::Disconnected
            } else {
                Link::Searching
            },
            ident: None,
            snapshot: None,
            edited: None,
            dirty: false,
            busy: false,
            sent: None,
            section: Section::Keys,
            error: None,
            offer_restore: false,
            notice: None,
            restore_target: None,
            carried_edits: None,
            depths: None,
            mock,
            experimental: false,
            profile: "Par défaut".into(),
        }
    }

    /// Takes a snapshot read from the keyboard as the new baseline; edits are dropped.
    pub fn set_snapshot(&mut self, snapshot: Snapshot) {
        let mut edited = snapshot.clone();
        // The read bytes describe the keyboard, not an edited copy.
        edited.raw = None;
        self.edited = Some(edited);
        self.snapshot = Some(snapshot);
        self.dirty = false;
    }

    /// A new baseline read after a write. `edited` follows it when the user changed
    /// nothing since « Appliquer » (`adopt`) — otherwise the edits stay.
    fn rebase(&mut self, snapshot: Snapshot, adopt: bool) {
        let unchanged = matches!((&self.sent, &self.edited), (Some(s), Some(e)) if s == e);
        self.sent = None;
        if adopt && unchanged {
            self.set_snapshot(snapshot);
        } else {
            self.snapshot = Some(snapshot);
            self.refresh_dirty();
        }
    }

    /// Recomputes `dirty` after the sections changed `edited`.
    pub fn refresh_dirty(&mut self) {
        self.dirty = match (&self.snapshot, &self.edited) {
            (Some(s), Some(e)) => s != e,
            _ => false,
        };
    }

    /// « Annuler les changements »: back to what the keyboard holds.
    pub fn revert(&mut self) {
        if let Some(s) = self.snapshot.take() {
            self.set_snapshot(s);
        }
    }

    /// « Appliquer » pressed: what to send, if anything.
    pub fn begin_apply(&mut self) -> Option<Snapshot> {
        if !self.dirty || self.busy || !self.connected {
            return None;
        }
        let edited = self.edited.clone()?;
        self.busy = true;
        self.sent = Some(edited.clone());
        Some(edited)
    }

    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Connected { ident, snapshot } => {
                self.connected = true;
                self.link = Link::Connected;
                self.ident = Some(ident);
                if !self.offer_restore {
                    self.error = None;
                }
                self.busy = false;
                self.sent = None;
                match self.carried_edits.take() {
                    // back after a write that dropped the keyboard: the edits stay
                    Some(edits) if edits.info.pid == snapshot.info.pid => {
                        self.snapshot = Some(*snapshot);
                        self.edited = Some(edits);
                        self.refresh_dirty();
                    }
                    _ => self.set_snapshot(*snapshot),
                }
            }
            Event::Disconnected => {
                if self.busy {
                    self.carried_edits = self.edited.clone();
                }
                self.connected = false;
                self.link = Link::Disconnected;
                self.busy = false;
                self.sent = None;
            }
            Event::Applied(snapshot) => {
                self.busy = false;
                self.error = None;
                self.offer_restore = false;
                // a write went through: no edits from an older write wait any more
                self.carried_edits = None;
                self.rebase(*snapshot, true);
            }
            Event::Refreshed(snapshot) => self.rebase(*snapshot, false),
            Event::Failed(message) => {
                self.forget_carried_edits();
                self.busy = false;
                self.sent = None;
                self.error = Some(message);
                self.offer_restore = false;
            }
            Event::WriteInterrupted(message) => {
                self.forget_carried_edits();
                self.busy = false;
                self.sent = None;
                self.error = Some(message);
                self.offer_restore = true;
            }
            Event::Notice(message) => self.notice = Some(message),
            Event::Depths(depths) => self.depths = Some(depths),
            Event::RestoreTarget(target) => self.restore_target = target,
            // the test section and the live effects keep these themselves
            Event::Probe(_) | Event::Axis(_) | Event::LiveStarted(_) => {}
        }
    }

    /// A write ended while the keyboard is still connected: edits carried over from a
    /// write that dropped it earlier no longer wait for a reconnection. While
    /// disconnected they stay (the worker reports `Disconnected` before the
    /// `WriteInterrupted` of the same write).
    fn forget_carried_edits(&mut self) {
        if self.connected {
            self.carried_edits = None;
        }
    }

    /// Device name for the header.
    pub fn device_name(&self) -> &'static str {
        "AK980 Max"
    }

    /// Report rate of the connected keyboard.
    pub fn report_rate(&self) -> Option<u32> {
        if !self.connected {
            return None;
        }
        self.snapshot.as_ref()?.game_mode.report_rate()
    }

    pub fn firmware(&self) -> Option<&str> {
        self.snapshot.as_ref().map(|s| s.info.firmware.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connected() -> (AppState, Snapshot) {
        let mut state = AppState::new(true, false);
        let snapshot = crate::mock::read_fixture().unwrap();
        state.apply(Event::Connected {
            ident: crate::mock::ident(&snapshot),
            snapshot: Box::new(snapshot.clone()),
        });
        (state, snapshot)
    }

    /// The keyboard's snapshot after writing `sent`: its values, read bytes attached.
    fn read_back(sent: &Snapshot, from: &Snapshot) -> Snapshot {
        let mut s = sent.clone();
        s.raw = from.raw.clone();
        s
    }

    #[test]
    fn edits_mark_dirty_and_revert_restores() {
        let (mut state, snapshot) = connected();
        assert!(state.connected && !state.dirty);
        assert_eq!(state.report_rate(), Some(8000));
        // the edited clone keeps the blocks' raw bytes
        let edited = state.edited.as_ref().unwrap();
        assert_eq!(edited.info.raw, snapshot.info.raw);
        assert_eq!(edited.game_mode.raw, snapshot.game_mode.raw);

        state.edited.as_mut().unwrap().led_effect.speed ^= 1;
        state.refresh_dirty();
        assert!(state.dirty);
        state.revert();
        assert!(!state.dirty);
        assert_eq!(state.edited.as_ref().unwrap(), &snapshot);
        assert!(state.snapshot.as_ref().unwrap().raw.is_some());
    }

    #[test]
    fn applied_takes_the_keyboards_snapshot() {
        let (mut state, snapshot) = connected();
        state.edited.as_mut().unwrap().led_effect.brightness = 2;
        state.refresh_dirty();
        let sent = state.begin_apply().unwrap();
        assert!(state.busy && state.begin_apply().is_none());
        state.apply(Event::Applied(Box::new(read_back(&sent, &snapshot))));
        assert!(!state.busy && !state.dirty);
        let base = state.snapshot.as_ref().unwrap();
        assert!(base.raw.is_some(), "the baseline is the read snapshot");
        assert_eq!(base.led_effect.brightness, 2);
        assert!(state.edited.as_ref().unwrap().raw.is_none());
    }

    #[test]
    fn edits_made_during_a_write_are_kept() {
        let (mut state, snapshot) = connected();
        state.edited.as_mut().unwrap().led_effect.brightness = 2;
        state.refresh_dirty();
        let sent = state.begin_apply().unwrap();
        // the user keeps editing while the keyboard is written
        state.edited.as_mut().unwrap().led_effect.speed = 1;
        state.refresh_dirty();
        state.apply(Event::Applied(Box::new(read_back(&sent, &snapshot))));
        let edited = state.edited.as_ref().unwrap();
        assert_eq!(edited.led_effect.speed, 1, "later edit kept");
        assert_eq!(edited.led_effect.brightness, 2);
        assert!(state.dirty, "the later edit is still to apply");
        assert_eq!(state.snapshot.as_ref().unwrap().led_effect.brightness, 2);
    }

    #[test]
    fn edits_survive_a_write_that_ends_in_a_disconnect() {
        let (mut state, snapshot) = connected();
        state.edited.as_mut().unwrap().led_effect.brightness = 2;
        state.refresh_dirty();
        state.begin_apply().unwrap();
        // edited again during the write, then the keyboard drops off
        state.edited.as_mut().unwrap().led_effect.speed = 1;
        state.apply(Event::Disconnected);
        state.apply(Event::WriteInterrupted(
            "Écriture interrompue : le clavier ne répond plus".into(),
        ));
        state.apply(Event::Connected {
            ident: crate::mock::ident(&snapshot),
            snapshot: Box::new(snapshot.clone()),
        });
        let edited = state.edited.as_ref().unwrap();
        assert_eq!(edited.led_effect.brightness, 2);
        assert_eq!(edited.led_effect.speed, 1);
        assert!(state.dirty);
        // a later re-read (« Relire », restore) is a new baseline: edits dropped
        state.apply(Event::Connected {
            ident: crate::mock::ident(&snapshot),
            snapshot: Box::new(snapshot),
        });
        assert!(!state.dirty);
    }

    #[test]
    fn carried_edits_do_not_outlive_a_later_write() {
        let (mut state, snapshot) = connected();
        let mut held = snapshot.clone();
        held.led_effect.brightness = 1;
        // edits waiting for a reconnection, then a write that ends while connected
        for event in [
            Event::Applied(Box::new(snapshot.clone())),
            Event::Failed("refusé".into()),
            Event::WriteInterrupted("Écriture interrompue : x".into()),
        ] {
            state.carried_edits = Some(held.clone());
            state.apply(event);
            assert!(state.carried_edits.is_none());
        }
        // while disconnected they wait, whatever comes before the reconnection
        state.carried_edits = Some(held);
        state.apply(Event::Disconnected);
        state.apply(Event::WriteInterrupted("Écriture interrompue : x".into()));
        state.apply(Event::Failed("Lecture du clavier impossible".into()));
        assert!(state.carried_edits.is_some());
        state.apply(Event::Connected {
            ident: crate::mock::ident(&snapshot),
            snapshot: Box::new(snapshot),
        });
        assert_eq!(state.edited.as_ref().unwrap().led_effect.brightness, 1);
    }

    #[test]
    fn an_interrupted_write_offers_the_restore_and_keeps_edits() {
        let (mut state, snapshot) = connected();
        state.edited.as_mut().unwrap().led_effect.brightness = 2;
        state.refresh_dirty();
        state.begin_apply().unwrap();
        state.apply(Event::Refreshed(Box::new(snapshot.clone())));
        state.apply(Event::WriteInterrupted(
            "Écriture interrompue : le clavier ne répond plus".into(),
        ));
        assert!(state.offer_restore && !state.busy);
        assert!(state.dirty, "edits kept to apply again");
        // the message survives a reconnection
        state.apply(Event::Disconnected);
        assert_eq!(state.report_rate(), None);
        state.apply(Event::Connected {
            ident: crate::mock::ident(&snapshot),
            snapshot: Box::new(snapshot),
        });
        assert!(state.error.is_some() && state.offer_restore);
    }
}
