//! What each key shows: the character Windows types for it in the current layout
//! (AZERTY on the author's setup), French names for the keys that type no character, and the
//! name of the action a remapped key sends.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use freejazz_protocol::blocks::keys::{KeyAction, KeyKind};
use serde::Deserialize;

use super::layout::{KeyDef, Layout};

/// Set-1 scan code of a HID keyboard usage (page 0x07); extended keys carry the `E0`
/// prefix in the high byte (`0xE04D` for the right arrow). `None` for usages without
/// a scan code here (Pause sends a sequence, not one code).
pub fn hid_to_scancode(hid: u8) -> Option<u16> {
    const LETTERS: [u16; 26] = [
        0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18,
        0x19, 0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C,
    ];
    let code = match hid {
        0x04..=0x1D => LETTERS[(hid - 0x04) as usize],
        0x1E..=0x27 => (hid - 0x1E + 0x02) as u16, // 1..9, 0
        0x28 => 0x1C,                              // Enter
        0x29 => 0x01,                              // Escape
        0x2A => 0x0E,                              // Backspace
        0x2B => 0x0F,                              // Tab
        0x2C => 0x39,                              // Space
        0x2D => 0x0C,
        0x2E => 0x0D,
        0x2F => 0x1A,
        0x30 => 0x1B,
        0x31 | 0x32 => 0x2B,
        0x33 => 0x27,
        0x34 => 0x28,
        0x35 => 0x29,
        0x36 => 0x33,
        0x37 => 0x34,
        0x38 => 0x35,
        0x39 => 0x3A,                              // Caps Lock
        0x3A..=0x43 => (hid - 0x3A + 0x3B) as u16, // F1..F10
        0x44 => 0x57,
        0x45 => 0x58,
        0x46 => 0xE037, // Print Screen
        0x47 => 0x46,   // Scroll Lock
        0x49 => 0xE052,
        0x4A => 0xE047,
        0x4B => 0xE049,
        0x4C => 0xE053,
        0x4D => 0xE04F,
        0x4E => 0xE051,
        0x4F => 0xE04D, // Right
        0x50 => 0xE04B,
        0x51 => 0xE050,
        0x52 => 0xE048,
        0x53 => 0x45, // Num Lock
        0x54 => 0xE035,
        0x55 => 0x37,
        0x56 => 0x4A,
        0x57 => 0x4E,
        0x58 => 0xE01C,
        0x59 => 0x4F,
        0x5A => 0x50,
        0x5B => 0x51,
        0x5C => 0x4B,
        0x5D => 0x4C,
        0x5E => 0x4D,
        0x5F => 0x47,
        0x60 => 0x48,
        0x61 => 0x49,
        0x62 => 0x52,
        0x63 => 0x53,
        0x64 => 0x56,
        0x65 => 0xE05D,
        0xE0 => 0x1D,
        0xE1 => 0x2A,
        0xE2 => 0x38,
        0xE3 => 0xE05B,
        0xE4 => 0xE01D,
        0xE5 => 0x36,
        0xE6 => 0xE038,
        0xE7 => 0xE05C,
        _ => return None,
    };
    Some(code)
}

/// Usages whose label is the character the layout types (letters, digits,
/// punctuation).
fn types_a_character(hid: u8) -> bool {
    matches!(hid, 0x04..=0x27 | 0x2D..=0x38 | 0x64)
}

/// French names of the keys that type no character.
fn fixed_name(hid: u8) -> Option<&'static str> {
    const F: [&str; 12] = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
    ];
    const F_HIGH: [&str; 12] = [
        "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
    ];
    const NUMPAD: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];
    Some(match hid {
        0x28 => "Entrée",
        0x29 => "Échap",
        0x2A => "Retour",
        0x2B => "Tab",
        0x2C => "Espace",
        0x39 => "Verr maj",
        0x3A..=0x45 => F[(hid - 0x3A) as usize],
        0x46 => "Impr écran",
        0x47 => "Arrêt défil",
        0x48 => "Pause",
        0x49 => "Inser",
        0x4A => "Début",
        0x4B => "Pg préc",
        0x4C => "Suppr",
        0x4D => "Fin",
        0x4E => "Pg suiv",
        0x4F => "→",
        0x50 => "←",
        0x51 => "↓",
        0x52 => "↑",
        0x53 => "Verr num",
        0x54 => "/",
        0x55 => "*",
        0x56 => "−",
        0x57 => "+",
        0x58 => "Entrée",
        0x59..=0x62 => NUMPAD[(hid - 0x59) as usize],
        0x63 => ".",
        0x65 => "Menu",
        0x68..=0x73 => F_HIGH[(hid - 0x68) as usize],
        0x7F => "Muet",
        0x80 => "Volume +",
        0x81 => "Volume −",
        0xE0 | 0xE4 => "Ctrl",
        0xE1 | 0xE5 => "Maj",
        0xE2 => "Alt",
        0xE6 => "Alt Gr",
        0xE3 | 0xE7 => "Win",
        _ => return None,
    })
}

/// The legend of a character key from what it types without and with Shift: a single
/// capital for letters, else the shifted character above the plain one (`"1\n&"`).
pub fn compose(plain: Option<&str>, shifted: Option<&str>) -> Option<String> {
    fn clean(s: Option<&str>) -> Option<String> {
        let s: String = s?.chars().filter(|c| !c.is_control()).collect();
        let s = s.trim().to_owned();
        (!s.is_empty()).then_some(s)
    }
    let plain = clean(plain);
    let shifted = clean(shifted);
    match (plain, shifted) {
        (None, None) => None,
        (Some(p), None) => Some(p.to_uppercase()),
        (None, Some(s)) => Some(s),
        (Some(p), Some(s)) => {
            let upper = p.to_uppercase();
            if s == p || s == upper {
                Some(upper)
            } else {
                Some(format!("{s}\n{p}"))
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct Keycode {
    pub page: String,
    pub name: String,
    pub value: u16,
    #[serde(default)]
    pub param1: Option<u8>,
    #[serde(default)]
    pub fr: Option<String>,
}

pub(crate) fn keycodes() -> &'static [Keycode] {
    static CODES: OnceLock<Vec<Keycode>> = OnceLock::new();
    CODES.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../data/ak980max/keycodes.json"))
            .expect("data/ak980max/keycodes.json is valid")
    })
}

fn keycode(page: &str, value: u16, param1: Option<u8>) -> Option<&'static Keycode> {
    keycodes()
        .iter()
        .find(|k| k.page == page && k.value == value && (param1.is_none() || k.param1 == param1))
}

/// Labels of every keyboard usage for one Windows layout.
#[derive(Debug, Clone)]
pub struct Labels {
    by_hid: HashMap<u8, String>,
}

impl Labels {
    /// `chars(hid)`: the legend the layout gives a character key (see [`compose`]).
    pub fn build(chars: impl Fn(u8) -> Option<String>) -> Self {
        let mut by_hid = HashMap::new();
        for hid in 0..=u8::MAX {
            let label = if let Some(name) = fixed_name(hid) {
                Some(name.to_owned())
            } else if types_a_character(hid) {
                chars(hid).or_else(|| keycode("KEYBOARD", hid as u16, None).map(|k| k.name.clone()))
            } else {
                keycode("KEYBOARD", hid as u16, None)
                    .map(|k| k.fr.clone().unwrap_or(k.name.clone()))
            };
            if let Some(label) = label {
                by_hid.insert(hid, label);
            }
        }
        Self { by_hid }
    }

    /// Label of a keyboard usage, if known.
    pub fn hid(&self, hid: u8) -> Option<&str> {
        self.by_hid.get(&hid).map(String::as_str)
    }

    /// What the key shows when it keeps its own function.
    pub fn default_label(&self, key: &KeyDef) -> String {
        self.hid(key.hid)
            .map(str::to_owned)
            .unwrap_or_else(|| key.label.clone())
    }

    fn usage(&self, usage: u8) -> String {
        self.hid(usage)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Touche 0x{usage:02X}"))
    }

    /// What the key shows: the label of what it sends when it is remapped, else its
    /// own label.
    pub fn key_label(&self, key: &KeyDef, action: KeyAction) -> String {
        match action.kind() {
            KeyKind::Default => self.default_label(key),
            KeyKind::Keyboard {
                modifiers: 0,
                usage: 0,
            } => "Désactivée".to_owned(),
            KeyKind::Keyboard { modifiers, usage } => {
                let mut parts: Vec<String> = Vec::new();
                for (bit, name) in [
                    (0x01, "Ctrl"),
                    (0x02, "Maj"),
                    (0x04, "Alt"),
                    (0x08, "Win"),
                    (0x10, "Ctrl"),
                    (0x20, "Maj"),
                    (0x40, "Alt Gr"),
                    (0x80, "Win"),
                ] {
                    if modifiers & bit != 0 && !parts.iter().any(|p| p == name) {
                        parts.push(name.to_owned());
                    }
                }
                if usage != 0 || parts.is_empty() {
                    // A legend on two lines reads as one character here.
                    let label = self.usage(usage);
                    let label = match label.split_once('\n') {
                        Some((_, plain)) if !parts.is_empty() => plain.to_owned(),
                        _ => label,
                    };
                    parts.push(label);
                }
                parts.join("+")
            }
            KeyKind::Consumer(code) => keycode("CONSUMER_KEY", code, None)
                .and_then(|k| k.fr.clone())
                .unwrap_or_else(|| format!("Multimédia 0x{code:X}")),
            KeyKind::Mouse { kind, value } => keycode("MOUSE", value as u16, Some(kind))
                .and_then(|k| k.fr.clone())
                .unwrap_or_else(|| "Souris".to_owned()),
            KeyKind::Macro { id, .. } => format!("Macro {id}"),
            KeyKind::Dks(slot) => format!("DKS {slot}"),
            KeyKind::Mt(_) => "Mod-Tap".to_owned(),
            KeyKind::Tgl(_) => "Toggle".to_owned(),
            KeyKind::Socd(_) => "SOCD".to_owned(),
            KeyKind::Rs(_) => "RS".to_owned(),
            KeyKind::Cb(_) => "Combinaison".to_owned(),
            KeyKind::Func(_) | KeyKind::FuncV2 { .. } => "Fonction".to_owned(),
            KeyKind::End(_) => "END".to_owned(),
            KeyKind::Mpt(_) => "MPT".to_owned(),
            KeyKind::Unknown(_) => "Action inconnue".to_owned(),
        }
    }

    /// Default label of each key of `layout`, by key index.
    pub fn for_layout(&self, layout: &Layout) -> HashMap<u8, String> {
        layout
            .keys
            .iter()
            .map(|k| (k.index, self.default_label(k)))
            .collect()
    }
}

#[cfg(windows)]
mod win {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetKeyboardLayout(thread: u32) -> isize;
        fn GetForegroundWindow() -> isize;
        fn LoadKeyboardLayoutW(klid: *const u16, flags: u32) -> isize;
        fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
        fn MapVirtualKeyExW(code: u32, map_type: u32, hkl: isize) -> u32;
        fn ToUnicodeEx(
            vk: u32,
            scan: u32,
            state: *const u8,
            buf: *mut u16,
            len: i32,
            flags: u32,
            hkl: isize,
        ) -> i32;
    }

    const MAPVK_VSC_TO_VK_EX: u32 = 3;
    /// Leave the keyboard state (pending dead keys) untouched (Windows 10 1607+).
    const NO_STATE_CHANGE: u32 = 0x4;
    const VK_SHIFT: usize = 0x10;
    const VK_LSHIFT: usize = 0xA0;

    /// The layout the user types with: that of the foreground window (Freejazz itself
    /// when it has the focus), else that of the calling thread. A window that never
    /// had the focus keeps the first layout of the list, which may not be the one in
    /// use (e.g. US International first, AZERTY selected).
    pub fn current_layout() -> isize {
        // SAFETY: plain queries; a null pid pointer is allowed.
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd != 0 {
                let thread = GetWindowThreadProcessId(hwnd, std::ptr::null_mut());
                if thread != 0 {
                    let hkl = GetKeyboardLayout(thread);
                    if hkl != 0 {
                        return hkl;
                    }
                }
            }
            GetKeyboardLayout(0)
        }
    }

    /// Handle of an installed layout from its identifier (`0001040C`), without
    /// activating it.
    pub fn load_layout(klid: &str) -> Option<isize> {
        const KLF_NOTELLSHELL: u32 = 0x80;
        let wide: Vec<u16> = klid.encode_utf16().chain(Some(0)).collect();
        // SAFETY: `wide` is a NUL-terminated UTF-16 string.
        let hkl = unsafe { LoadKeyboardLayoutW(wide.as_ptr(), KLF_NOTELLSHELL) };
        (hkl != 0).then_some(hkl)
    }

    /// What the key of `scancode` types in `hkl`, with or without Shift.
    pub fn typed(scancode: u16, shift: bool, hkl: isize) -> Option<String> {
        // SAFETY: plain query, no pointers.
        let vk = unsafe { MapVirtualKeyExW(scancode as u32, MAPVK_VSC_TO_VK_EX, hkl) };
        if vk == 0 {
            return None;
        }
        let mut state = [0u8; 256];
        if shift {
            state[VK_SHIFT] = 0x80;
            state[VK_LSHIFT] = 0x80;
        }
        let mut buf = [0u16; 8];
        // SAFETY: `state` is the 256-byte array the call reads, `buf` holds `len` u16.
        let n = unsafe {
            ToUnicodeEx(
                vk,
                scancode as u32 & 0xFF,
                state.as_ptr(),
                buf.as_mut_ptr(),
                buf.len() as i32,
                NO_STATE_CHANGE,
                hkl,
            )
        };
        let len = match n {
            0 => return None,
            n if n < 0 => 1, // dead key: its spacing character
            n => (n as usize).min(buf.len()),
        };
        String::from_utf16(&buf[..len]).ok()
    }
}

/// Legend Windows gives a character key in the current layout.
#[cfg(windows)]
fn windows_chars(hkl: isize, hid: u8) -> Option<String> {
    let sc = hid_to_scancode(hid)?;
    compose(
        win::typed(sc, false, hkl).as_deref(),
        win::typed(sc, true, hkl).as_deref(),
    )
}

static FORCED: Mutex<Option<isize>> = Mutex::new(None);

/// Screenshot aid (`--keyboard-layout 0001040C`): label with this installed layout
/// instead of the one in use. Returns false when Windows does not know it.
pub fn force_layout(klid: &str) -> bool {
    #[cfg(windows)]
    if let Some(hkl) = win::load_layout(klid) {
        *FORCED.lock().unwrap_or_else(|e| e.into_inner()) = Some(hkl);
        return true;
    }
    let _ = klid;
    false
}

/// Labels for the current Windows layout, rebuilt when the user switches layout.
pub fn current() -> Arc<Labels> {
    static CACHE: Mutex<Option<(isize, Arc<Labels>)>> = Mutex::new(None);
    let forced = *FORCED.lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(windows)]
    let hkl = forced.unwrap_or_else(win::current_layout);
    #[cfg(not(windows))]
    let hkl = forced.unwrap_or(0);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((cached, labels)) = cache.as_ref()
        && *cached == hkl
    {
        return labels.clone();
    }
    #[cfg(windows)]
    let labels = Arc::new(Labels::build(|hid| windows_chars(hkl, hid)));
    #[cfg(not(windows))]
    let labels = Arc::new(Labels::build(|_| None));
    *cache = Some((hkl, labels.clone()));
    labels
}

/// Label of each AK980 Max key for the current Windows layout, by key index.
pub fn key_labels() -> HashMap<u8, String> {
    current().for_layout(Layout::ak980max())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// French AZERTY, as Windows gives it, for the keys the tests use.
    pub(crate) fn azerty() -> Labels {
        Labels::build(|hid| {
            let (plain, shifted) = match hid {
                0x04 => ("q", "Q"),
                0x14 => ("a", "A"),
                0x1A => ("z", "Z"),
                0x1D => ("w", "W"),
                0x16 => ("s", "S"),
                0x07 => ("d", "D"),
                0x0B => ("h", "H"),
                0x33 => ("m", "M"),
                0x10 => (",", "?"),
                0x1E => ("&", "1"),
                0x1F => ("é", "2"),
                0x2F => ("^", "¨"),
                _ => return None,
            };
            compose(Some(plain), Some(shifted))
        })
    }

    #[test]
    fn scancodes_of_a_enter_and_right_arrow() {
        assert_eq!(hid_to_scancode(0x04), Some(0x1E));
        assert_eq!(hid_to_scancode(0x28), Some(0x1C));
        assert_eq!(hid_to_scancode(0x4F), Some(0xE04D));
        assert_eq!(hid_to_scancode(0x1D), Some(0x2C)); // Z
        assert_eq!(hid_to_scancode(0x27), Some(0x0B)); // 0
        assert_eq!(hid_to_scancode(0x45), Some(0x58)); // F12
        assert_eq!(hid_to_scancode(0x48), None); // Pause
    }

    #[test]
    fn legends_compose_like_a_keycap() {
        assert_eq!(compose(Some("a"), Some("A")).as_deref(), Some("A"));
        assert_eq!(compose(Some("&"), Some("1")).as_deref(), Some("1\n&"));
        assert_eq!(compose(Some("é"), Some("2")).as_deref(), Some("2\né"));
        assert_eq!(compose(Some("\r"), None), None);
        assert_eq!(compose(Some("ù"), Some("%")).as_deref(), Some("%\nù"));
    }

    #[test]
    fn labels_are_french_for_keys_without_characters() {
        let l = azerty();
        assert_eq!(l.hid(0x29), Some("Échap"));
        assert_eq!(l.hid(0x28), Some("Entrée"));
        assert_eq!(l.hid(0xE1), Some("Maj"));
        assert_eq!(l.hid(0x4C), Some("Suppr"));
        assert_eq!(l.hid(0x53), Some("Verr num"));
        assert_eq!(l.hid(0x14), Some("A"));
        let fn_key = Layout::ak980max().key(85).unwrap();
        assert_eq!(l.default_label(fn_key), "Fn");
    }

    #[test]
    fn fixture_keys_show_what_they_send() {
        let snapshot = crate::mock::read_fixture().unwrap();
        let layout = Layout::ak980max();
        let l = azerty();
        let label = |i: u8| l.key_label(layout.key(i).unwrap(), snapshot.keys.0[i as usize]);
        // The tested unit has six nav keycaps moved, with the keys remapped to match.
        assert_eq!(label(108), "Suppr"); // PgDn position
        assert_eq!(label(99), "Inser");
        assert_eq!(label(103), "Impr écran");
        assert_eq!(label(104), "Pg préc");
        assert_eq!(label(105), "Pg suiv");
        assert_eq!(label(106), "Début");
        // Default keys: their own label in the layout.
        assert_eq!(label(0), "Échap");
        assert_eq!(label(33), "A"); // QWERTY Q position on AZERTY
        assert_eq!(label(17), "1\n&");
        assert_eq!(label(54), "H"); // remapped to its own usage
        assert_eq!(label(64), "Maj");
    }

    #[test]
    fn actions_get_their_names() {
        let l = azerty();
        let key = Layout::ak980max().key(0).unwrap();
        let label = |kind| l.key_label(key, KeyAction::from_kind(kind));
        assert_eq!(
            label(KeyKind::Keyboard {
                modifiers: 0x01,
                usage: 0x06
            }),
            "Ctrl+C"
        );
        assert_eq!(
            label(KeyKind::Keyboard {
                modifiers: 0x03,
                usage: 0x1E
            }),
            "Ctrl+Maj+&"
        );
        assert_eq!(label(KeyKind::Consumer(0xCD)), "Lecture/Pause");
        assert_eq!(
            label(KeyKind::Mouse { kind: 1, value: 2 }),
            "Bouton droit de la souris"
        );
        assert_eq!(
            label(KeyKind::Macro {
                id: 3,
                p2: 0,
                p3: 0
            }),
            "Macro 3"
        );
        assert_eq!(label(KeyKind::Unknown([0x55, 1, 2, 3])), "Action inconnue");
    }

    #[cfg(windows)]
    #[test]
    fn windows_layout_labels_every_key() {
        let labels = key_labels();
        assert_eq!(labels.len(), 101);
        assert!(labels.values().all(|l| !l.is_empty()));
        // Whatever the layout, the A..Z keys show one capital letter.
        let a = &labels[&49];
        assert_eq!(a.chars().count(), 1, "{a:?}");
    }
}
