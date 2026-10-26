//! Records what is typed in the window as macro actions: key down / key up with the
//! time between events, the key found by its physical position (layout independent) in
//! `layout.json`.

use eframe::egui::{Event, Key, Modifiers};
use freejazz_protocol::blocks::macros::MacroAction;

use crate::keyboard::Layout;
use crate::live::input::{self, Subscription};

/// Shortest delay the site accepts between two actions, and the longest.
pub const MIN_DELAY_MS: u16 = 5;
pub const MAX_DELAY_MS: u16 = 60_000;

/// `type` field of a keyboard action (protocol.md 6.6).
const KIND_KEYBOARD: u8 = 3;

/// The `code` (DOM name, as in `layout.json`) of the key at egui's physical key.
fn dom_code(key: Key) -> Option<String> {
    use Key::*;
    let letters = [
        A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    ];
    if let Some(i) = letters.iter().position(|&k| k == key) {
        return Some(format!("Key{}", char::from(b'A' + i as u8)));
    }
    let digits = [Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9];
    if let Some(i) = digits.iter().position(|&k| k == key) {
        return Some(format!("Digit{i}"));
    }
    let fixed = match key {
        ArrowDown => "ArrowDown",
        ArrowLeft => "ArrowLeft",
        ArrowRight => "ArrowRight",
        ArrowUp => "ArrowUp",
        Escape => "Escape",
        Tab => "Tab",
        Backspace => "Backspace",
        Enter => "Enter",
        Space => "Space",
        Insert => "Insert",
        Delete => "Delete",
        Home => "Home",
        PageUp => "PageUp",
        PageDown => "PageDown",
        Comma => "Comma",
        Backslash => "Backslash",
        Slash => "Slash",
        Minus => "Minus",
        Period => "Period",
        Equals => "Equal",
        Semicolon => "Semicolon",
        Quote => "Quote",
        OpenBracket => "BracketLeft",
        CloseBracket => "BracketRight",
        Backtick => "Backquote",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        F10 => "F10",
        F11 => "F11",
        F12 => "F12",
        _ => return None,
    };
    Some(fixed.to_owned())
}

/// HID usage of an egui key, through `layout.json`; `None` for a key the keyboard
/// does not have.
pub fn hid_for(key: Key) -> Option<u8> {
    let code = dom_code(key)?;
    Layout::ak980max()
        .keys
        .iter()
        .find(|k| k.code == code)
        .map(|k| k.hid)
}

/// Left Ctrl, Shift, Alt: egui reports modifiers as state, not as key events.
const MODIFIER_USAGES: [u8; 3] = [0xE0, 0xE1, 0xE2];

fn modifier_state(m: Modifiers) -> [bool; 3] {
    [m.ctrl, m.shift, m.alt]
}

/// Collects actions while recording. Feed it every frame's events.
#[derive(Default)]
pub struct Recorder {
    /// Time of the previous action.
    last: Option<f64>,
    mods: [bool; 3],
    pub actions: Vec<MacroAction>,
    /// The global key hook while recording: it sees Pause, Impr écran, Arrêt défil, Win,
    /// the numeric pad and the right-hand modifiers, which the window never gets. Its
    /// times are on the [`input::now`] clock.
    hook: Option<Subscription>,
    /// Why the hook could not be installed (the window's own key events are used).
    pub warning: Option<String>,
}

impl Recorder {
    /// A recording fed by the low-level hook; without it (refused by Windows) by the
    /// window's key events.
    pub fn with_hook() -> Self {
        match input::subscribe() {
            Ok(sub) => Self {
                hook: Some(sub),
                ..Self::default()
            },
            Err(e) => Self {
                warning: Some(e),
                ..Self::default()
            },
        }
    }

    pub fn hooked(&self) -> bool {
        self.hook.is_some()
    }

    fn pump_hook(&mut self) {
        let Some(sub) = &self.hook else { return };
        let events = sub.drain();
        self.feed(events);
    }

    /// Records the hook's events. Keys typed while another program had the focus are
    /// not the user's macro: they are dropped (the hook sees every key of the system).
    fn feed(&mut self, events: impl IntoIterator<Item = input::KeyEvent>) {
        for e in events.into_iter().filter(|e| !e.repeat && e.foreground) {
            if let Some(key) = e.index.and_then(|i| Layout::ak980max().key(i)) {
                self.push(key.hid, e.down, f64::from(e.at));
            }
        }
    }

    /// Appends an action at `time` (seconds) and gives the previous one the time that
    /// went by since.
    fn push(&mut self, usage: u8, press: bool, time: f64) {
        if let (Some(last), Some(prev)) = (self.last, self.actions.last_mut()) {
            let ms = ((time - last) * 1000.0)
                .round()
                .clamp(0.0, f64::from(MAX_DELAY_MS));
            prev.delay_ms = (ms as u16).max(MIN_DELAY_MS);
        }
        self.last = Some(time);
        self.actions.push(MacroAction {
            press,
            kind: KIND_KEYBOARD,
            key: usage,
            delay_ms: 0,
            raw_flags: None,
        });
    }

    /// One frame: the modifiers' state, then the key events (repeats ignored).
    pub fn frame(&mut self, events: &[Event], modifiers: Modifiers, time: f64) {
        if self.hook.is_some() {
            self.pump_hook();
            return;
        }
        let now = modifier_state(modifiers);
        for (i, usage) in MODIFIER_USAGES.into_iter().enumerate() {
            if now[i] != self.mods[i] {
                self.push(usage, now[i], time);
            }
        }
        self.mods = now;
        for e in events {
            if let Event::Key {
                key,
                physical_key,
                pressed,
                repeat: false,
                ..
            } = e
                && let Some(usage) = hid_for(physical_key.unwrap_or(*key))
            {
                self.push(usage, *pressed, time);
            }
        }
    }

    /// Ends the recording: whatever is still held is released, and the last action
    /// has no delay.
    pub fn finish(mut self, time: f64) -> Vec<MacroAction> {
        let time = if self.hook.is_some() {
            self.pump_hook();
            f64::from(input::now())
        } else {
            time
        };
        let mut held: Vec<u8> = Vec::new();
        for a in &self.actions {
            if a.press && !held.contains(&a.key) {
                held.push(a.key);
            } else if !a.press {
                held.retain(|&k| k != a.key);
            }
        }
        for k in held {
            self.push(k, false, time);
        }
        if let Some(last) = self.actions.last_mut() {
            last.delay_ms = 0;
        }
        self.actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: Key, pressed: bool) -> Event {
        Event::Key {
            key: k,
            physical_key: Some(k),
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn recorder_maps_q_to_hid_0x14() {
        assert_eq!(hid_for(Key::Q), Some(0x14));
        assert_eq!(hid_for(Key::Num1), Some(0x1E));
        assert_eq!(hid_for(Key::Enter), Some(0x28));
        assert_eq!(hid_for(Key::Copy), None);
    }

    #[test]
    fn physical_key_wins_over_the_layout() {
        // AZERTY: the key typing the letter A sits where QWERTY has Q
        let mut r = Recorder::default();
        let e = Event::Key {
            key: Key::A,
            physical_key: Some(Key::Q),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        r.frame(&[e], Modifiers::NONE, 0.0);
        assert_eq!(r.actions[0].key, 0x14);
        assert!(r.actions[0].press);
    }

    #[test]
    fn delays_follow_the_event_times() {
        let mut r = Recorder::default();
        r.frame(&[key(Key::B, true)], Modifiers::NONE, 1.0);
        r.frame(&[key(Key::B, false)], Modifiers::NONE, 1.120);
        r.frame(&[key(Key::O, true)], Modifiers::NONE, 1.121);
        let actions = r.finish(1.5);
        let seq: Vec<_> = actions
            .iter()
            .map(|a| (a.key, a.press, a.delay_ms))
            .collect();
        // B down 120 ms, B up (minimum), O down held until the end, released
        assert_eq!(seq[0], (0x05, true, 120));
        assert_eq!(seq[1], (0x05, false, MIN_DELAY_MS));
        assert_eq!(seq[2], (0x12, true, 379));
        assert_eq!(seq[3], (0x12, false, 0));
        assert!(actions.iter().all(|a| a.kind == 3));
    }

    #[test]
    fn keys_typed_in_another_window_are_not_recorded() {
        let index = Layout::ak980max()
            .keys
            .iter()
            .find(|k| k.code == "KeyQ")
            .unwrap()
            .index;
        let event = |down, foreground, at| input::KeyEvent {
            scan: 0x10,
            vk: 0x51,
            down,
            repeat: false,
            index: Some(index),
            at,
            foreground,
        };
        let mut r = Recorder::default();
        r.feed([event(true, false, 0.0), event(false, false, 0.1)]);
        assert!(r.actions.is_empty(), "typed elsewhere: dropped");
        r.feed([event(true, true, 0.2), event(false, true, 0.3)]);
        assert_eq!(r.actions.len(), 2);
        assert_eq!(r.actions[0].key, 0x14);
    }

    #[test]
    fn repeats_are_ignored_and_modifiers_become_actions() {
        let mut r = Recorder::default();
        let repeat = Event::Key {
            key: Key::A,
            physical_key: Some(Key::A),
            pressed: true,
            repeat: true,
            modifiers: Modifiers::NONE,
        };
        r.frame(&[repeat], Modifiers::NONE, 0.0);
        assert!(r.actions.is_empty());
        r.frame(&[], Modifiers::SHIFT, 0.1);
        r.frame(&[], Modifiers::NONE, 0.2);
        assert_eq!(r.actions.len(), 2);
        assert_eq!((r.actions[0].key, r.actions[0].press), (0xE1, true));
        assert_eq!((r.actions[1].key, r.actions[1].press), (0xE1, false));
    }
}
