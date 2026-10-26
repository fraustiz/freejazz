//! The key test: keys light up in the keyboard view as they are pressed (egui key
//! events), and show their travel when the experimental depth stream runs.

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::{self, Key};
use freejazz_protocol::calibration::AxisReport;

use crate::keyboard::Layout;
use crate::live::input::{self, Subscription};

/// The `layout.json` `code` (W3C physical key name) of an egui key, when the keyboard
/// has such a key. egui reports no Shift / Ctrl / Alt / CapsLock / Menu keys and does
/// not tell the numeric pad from the main block.
pub fn code_of(key: Key) -> Option<String> {
    let name = key.name();
    let code = match key {
        Key::A
        | Key::B
        | Key::C
        | Key::D
        | Key::E
        | Key::F
        | Key::G
        | Key::H
        | Key::I
        | Key::J
        | Key::K
        | Key::L
        | Key::M
        | Key::N
        | Key::O
        | Key::P
        | Key::Q
        | Key::R
        | Key::S
        | Key::T
        | Key::U
        | Key::V
        | Key::W
        | Key::X
        | Key::Y
        | Key::Z => format!("Key{name}"),
        Key::Num0
        | Key::Num1
        | Key::Num2
        | Key::Num3
        | Key::Num4
        | Key::Num5
        | Key::Num6
        | Key::Num7
        | Key::Num8
        | Key::Num9 => format!("Digit{name}"),
        Key::F1
        | Key::F2
        | Key::F3
        | Key::F4
        | Key::F5
        | Key::F6
        | Key::F7
        | Key::F8
        | Key::F9
        | Key::F10
        | Key::F11
        | Key::F12 => name.to_owned(),
        Key::Escape => "Escape".into(),
        Key::Tab => "Tab".into(),
        Key::Backspace => "Backspace".into(),
        Key::Enter => "Enter".into(),
        Key::Space => "Space".into(),
        Key::Insert => "Insert".into(),
        Key::Delete => "Delete".into(),
        Key::Home => "Home".into(),
        Key::PageUp => "PageUp".into(),
        Key::PageDown => "PageDown".into(),
        Key::ArrowUp => "ArrowUp".into(),
        Key::ArrowDown => "ArrowDown".into(),
        Key::ArrowLeft => "ArrowLeft".into(),
        Key::ArrowRight => "ArrowRight".into(),
        Key::Minus => "Minus".into(),
        Key::Equals => "Equal".into(),
        Key::Backtick => "Backquote".into(),
        Key::OpenBracket => "BracketLeft".into(),
        Key::CloseBracket => "BracketRight".into(),
        Key::Backslash => "Backslash".into(),
        Key::Semicolon => "Semicolon".into(),
        Key::Quote => "Quote".into(),
        Key::Comma => "Comma".into(),
        Key::Period => "Period".into(),
        Key::Slash => "Slash".into(),
        _ => return None,
    };
    Some(code)
}

/// The layout index of a physical key name.
pub fn index_of_code(layout: &Layout, code: &str) -> Option<u8> {
    layout.keys.iter().find(|k| k.code == code).map(|k| k.index)
}

#[derive(Default)]
pub struct KeyTest {
    /// May the global key hook be installed? (Never in tests nor in `--mock`.)
    hook_allowed: bool,
    /// The hook while the test runs: Pause, Impr écran, Arrêt défil, Win, the numeric pad
    /// and the right-hand modifiers only reach the app that way.
    hook: Option<Subscription>,
    /// Why the hook could not be installed (the window's own key events are used).
    pub hook_error: Option<String>,
    /// Held right now.
    pub pressed: BTreeSet<u8>,
    /// Pressed at least once since « Effacer ».
    pub tested: BTreeSet<u8>,
    /// Travel (hundredths of a millimetre) of the keys the keyboard reports as moving.
    pub stroke: BTreeMap<u8, u16>,
    /// The key last seen moving, for the travel readout.
    pub last: Option<u8>,
}

impl KeyTest {
    pub fn clear(&mut self) {
        let (allowed, hook, error) = (self.hook_allowed, self.hook.take(), self.hook_error.take());
        *self = Self {
            hook_allowed: allowed,
            hook,
            hook_error: error,
            ..Self::default()
        };
    }

    pub fn allow_hook(&mut self, allowed: bool) {
        self.hook_allowed = allowed;
    }

    /// Whether the low-level hook feeds the test right now.
    pub fn hooked(&self) -> bool {
        self.hook.is_some()
    }

    /// The test is shown: install the hook (once).
    pub fn start_hook(&mut self) {
        if self.hook_allowed && self.hook.is_none() && self.hook_error.is_none() {
            match input::subscribe() {
                Ok(sub) => self.hook = Some(sub),
                Err(e) => self.hook_error = Some(e),
            }
        }
    }

    /// The test is left: the hook goes away.
    pub fn stop_hook(&mut self) {
        self.hook = None;
        self.hook_error = None;
    }

    /// A key went down or up (from egui input).
    pub fn set_pressed(&mut self, index: u8, down: bool) {
        if down {
            self.pressed.insert(index);
            self.tested.insert(index);
        } else {
            self.pressed.remove(&index);
        }
    }

    /// Reads this frame's key events, and the held modifiers (left side only).
    pub fn read_input(&mut self, input: &egui::InputState, layout: &Layout) {
        if let Some(sub) = &self.hook {
            // every key, whatever its side or kind; only while the window has focus
            let events = sub.drain();
            if !input.focused {
                self.pressed.clear();
                return;
            }
            for e in events.into_iter().filter(|e| !e.repeat && e.foreground) {
                if let Some(index) = e.index {
                    self.set_pressed(index, e.down);
                }
            }
            return;
        }
        if !input.focused {
            self.pressed.clear();
            return;
        }
        for event in &input.events {
            if let egui::Event::Key {
                key,
                physical_key,
                pressed,
                ..
            } = event
                && let Some(code) = code_of(physical_key.unwrap_or(*key))
                && let Some(index) = index_of_code(layout, &code)
            {
                self.set_pressed(index, *pressed);
            }
        }
        for (held, code) in [
            (input.modifiers.shift, "ShiftLeft"),
            (input.modifiers.ctrl, "ControlLeft"),
            (input.modifiers.alt, "AltLeft"),
        ] {
            if let Some(index) = index_of_code(layout, code) {
                self.set_pressed(index, held);
            }
        }
    }

    /// A 0xFB report: the keyboard says which key moves, and how far.
    // UNVERIFIED-HW
    pub fn on_report(&mut self, r: &AxisReport) {
        if r.key_stroke > 0 {
            self.stroke.insert(r.index, r.key_stroke);
            self.tested.insert(r.index);
            self.last = Some(r.index);
        } else {
            self.stroke.remove(&r.index);
        }
    }
}

/// « 2,50 mm ».
pub fn mm_text(stroke: u16) -> String {
    format!("{:.2} mm", f32::from(stroke) / 100.0).replace('.', ",")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn egui_keys_find_their_layout_index() {
        let layout = Layout::ak980max();
        let index = |k| code_of(k).and_then(|c| index_of_code(layout, &c));
        assert_eq!(index(Key::Escape), Some(0));
        assert_eq!(index(Key::F1), Some(1));
        assert_eq!(index(Key::Q), Some(33)); // QWERTY name; the cap is physical
        assert_eq!(index(Key::A), Some(49));
        assert_eq!(
            index(Key::Space).and_then(|i| layout.key(i)).map(|k| k.hid),
            Some(44)
        );
        assert_eq!(code_of(Key::Copy), None);
    }

    #[test]
    fn every_reachable_key_maps_to_a_distinct_slot() {
        let layout = Layout::ak980max();
        let mut seen = BTreeSet::new();
        for key in Key::ALL {
            if let Some(i) = code_of(*key).and_then(|c| index_of_code(layout, &c)) {
                assert!(seen.insert(i), "{key:?} shares slot {i}");
            }
        }
        assert!(seen.len() > 70, "{}", seen.len());
    }

    #[test]
    fn presses_and_reports_are_remembered() {
        let mut t = KeyTest::default();
        t.set_pressed(5, true);
        t.set_pressed(5, false);
        assert!(t.pressed.is_empty() && t.tested.contains(&5));
        let mut b = vec![0u8; 64];
        b[0] = 0x55;
        b[1] = 0xFB;
        b[2] = 9;
        b[10] = 250;
        let mut r = AxisReport::parse(&b).unwrap();
        t.on_report(&r);
        assert_eq!(t.stroke.get(&9), Some(&250));
        assert_eq!(mm_text(250), "2,50 mm");
        r.key_stroke = 0;
        t.on_report(&r);
        assert!(t.stroke.is_empty() && t.tested.contains(&9));
    }
}
