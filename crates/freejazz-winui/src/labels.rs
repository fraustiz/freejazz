//! Dynamic keyboard layout resolver (AZERTY, QWERTY, Windows active layout)
//! Queries Windows API to resolve accurate key labels and Virtual Key mappings.

use std::collections::HashMap;
use crate::layout::{KeyDef, Layout};

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetKeyboardLayout(thread: u32) -> isize;
    fn GetForegroundWindow() -> isize;
    fn GetWindowThreadProcessId(hwnd: isize, pid: *mut u32) -> u32;
    fn MapVirtualKeyExW(code: u32, map_type: u32, hkl: isize) -> u32;
    fn LoadKeyboardLayoutW(pwszKLID: *const u16, Flags: u32) -> isize;
    fn ToUnicodeEx(
        wVirtKey: u32,
        wScanCode: u32,
        lpKeyState: *const u8,
        pwszBuff: *mut u16,
        cchBuff: i32,
        wFlags: u32,
        dwhkl: isize,
    ) -> i32;
}

const MAPVK_VSC_TO_VK_EX: u32 = 3;
const NO_STATE_CHANGE: u32 = 0x4;
const VK_SHIFT: usize = 0x10;
const VK_LSHIFT: usize = 0xA0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LayoutPreference {
    #[default]
    Auto,
    Azerty,
    Qwerty,
}

impl LayoutPreference {
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "Automatique (Windows)",
            Self::Azerty => "AZERTY (Français)",
            Self::Qwerty => "QWERTY (US)",
        }
    }
}

pub fn get_hkl(pref: LayoutPreference) -> isize {
    match pref {
        LayoutPreference::Auto => get_active_hkl(),
        LayoutPreference::Azerty => load_klid("0000040C").unwrap_or_else(get_active_hkl),
        LayoutPreference::Qwerty => load_klid("00000409").unwrap_or_else(get_active_hkl),
    }
}

fn get_active_hkl() -> isize {
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

fn load_klid(klid: &str) -> Option<isize> {
    const KLF_NOTELLSHELL: u32 = 0x80;
    let wide: Vec<u16> = klid.encode_utf16().chain(Some(0)).collect();
    let hkl = unsafe { LoadKeyboardLayoutW(wide.as_ptr(), KLF_NOTELLSHELL) };
    (hkl != 0).then_some(hkl)
}

/// Set-1 scan code of a HID keyboard usage (page 0x07)
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
        0x2D => 0x0C,                              // Minus
        0x2E => 0x0D,                              // Equal
        0x2F => 0x1A,                              // BracketLeft
        0x30 => 0x1B,                              // BracketRight
        0x31 | 0x32 => 0x2B,                       // Backslash
        0x33 => 0x27,                              // Semicolon
        0x34 => 0x28,                              // Quote
        0x35 => 0x29,                              // Backquote
        0x36 => 0x33,                              // Comma
        0x37 => 0x34,                              // Period
        0x38 => 0x35,                              // Slash
        0x39 => 0x3A,                              // Caps Lock
        0x3A..=0x43 => (hid - 0x3A + 0x3B) as u16, // F1..F10
        0x44 => 0x57,                              // F11
        0x45 => 0x58,                              // F12
        0x46 => 0xE037,                            // Print Screen
        0x47 => 0x46,                              // Scroll Lock
        0x48 => 0xE046,                            // Pause
        0x49 => 0xE052,                            // Insert
        0x4A => 0xE047,                            // Home
        0x4B => 0xE049,                            // Page Up
        0x4C => 0xE053,                            // Delete
        0x4D => 0xE04F,                            // End
        0x4E => 0xE051,                            // Page Down
        0x4F => 0xE04D,                            // Right Arrow
        0x50 => 0xE04B,                            // Left Arrow
        0x51 => 0xE050,                            // Down Arrow
        0x52 => 0xE048,                            // Up Arrow
        0x53 => 0x45,                              // Num Lock
        0x54 => 0xE035,                            // Numpad Divide
        0x55 => 0x37,                              // Numpad Multiply
        0x56 => 0x4A,                              // Numpad Minus
        0x57 => 0x4E,                              // Numpad Plus
        0x58 => 0xE01C,                            // Numpad Enter
        0x59 => 0x4F,                              // Numpad 1
        0x5A => 0x50,                              // Numpad 2
        0x5B => 0x51,                              // Numpad 3
        0x5C => 0x4B,                              // Numpad 4
        0x5D => 0x4C,                              // Numpad 5
        0x5E => 0x4D,                              // Numpad 6
        0x5F => 0x47,                              // Numpad 7
        0x60 => 0x48,                              // Numpad 8
        0x61 => 0x49,                              // Numpad 9
        0x62 => 0x52,                              // Numpad 0
        0x63 => 0x53,                              // Numpad Decimal
        0x64 => 0x56,                              // Non-US \
        0x65 => 0xE05D,                            // Context Menu
        0xE0 => 0x1D,                              // Left Ctrl
        0xE1 => 0x2A,                              // Left Shift
        0xE2 => 0x38,                              // Left Alt
        0xE3 => 0xE05B,                            // Left Win
        0xE4 => 0xE01D,                            // Right Ctrl
        0xE5 => 0x36,                              // Right Shift
        0xE6 => 0xE038,                            // Right Alt
        0xE7 => 0xE05C,                            // Right Win
        _ => return None,
    };
    Some(code)
}

fn type_char(scancode: u16, shift: bool, hkl: isize) -> Option<String> {
    unsafe {
        let vk = MapVirtualKeyExW((scancode & 0xFF) as u32, MAPVK_VSC_TO_VK_EX, hkl);
        if vk == 0 {
            return None;
        }
        let mut state = [0u8; 256];
        if shift {
            state[VK_SHIFT] = 0x80;
            state[VK_LSHIFT] = 0x80;
        }
        let mut buf = [0u16; 8];
        let n = ToUnicodeEx(
            vk,
            (scancode & 0xFF) as u32,
            state.as_ptr(),
            buf.as_mut_ptr(),
            buf.len() as i32,
            NO_STATE_CHANGE,
            hkl,
        );
        let len = match n {
            0 => return None,
            n if n < 0 => 1,
            n => (n as usize).min(buf.len()),
        };
        String::from_utf16(&buf[..len]).ok()
    }
}

use freejazz_protocol::blocks::keys::{KeyAction, KeyKind};

pub fn is_remapped(action: KeyAction, key: &KeyDef) -> bool {
    match action.kind() {
        KeyKind::Default => false,
        KeyKind::Keyboard {
            modifiers: 0,
            usage,
        } => usage != key.hid,
        _ => true,
    }
}

pub fn hid_to_friendly_name(hid: u8) -> Option<&'static str> {
    match hid {
        0x28 => Some("Entrée"),
        0x29 => Some("Échap"),
        0x2A => Some("Retour"),
        0x2B => Some("Tab"),
        0x2C => Some("Espace"),
        0x39 => Some("Verr.Maj"),
        0x3A => Some("F1"),
        0x3B => Some("F2"),
        0x3C => Some("F3"),
        0x3D => Some("F4"),
        0x3E => Some("F5"),
        0x3F => Some("F6"),
        0x40 => Some("F7"),
        0x41 => Some("F8"),
        0x42 => Some("F9"),
        0x43 => Some("F10"),
        0x44 => Some("F11"),
        0x45 => Some("F12"),
        0x46 => Some("Impr"),
        0x47 => Some("Arrêt"),
        0x48 => Some("Pause"),
        0x49 => Some("Inser"),
        0x4A => Some("Début"),
        0x4B => Some("Pg.Préc"),
        0x4C => Some("Suppr"),
        0x4D => Some("Fin"),
        0x4E => Some("Pg.Suiv"),
        0x4F => Some("→"),
        0x50 => Some("←"),
        0x51 => Some("↓"),
        0x52 => Some("↑"),
        0x53 => Some("Verr.Num"),
        0x54 => Some("/"),
        0x55 => Some("*"),
        0x56 => Some("-"),
        0x57 => Some("+"),
        0x58 => Some("Entrée"),
        0x59 => Some("1"),
        0x5A => Some("2"),
        0x5B => Some("3"),
        0x5C => Some("4"),
        0x5D => Some("5"),
        0x5E => Some("6"),
        0x5F => Some("7"),
        0x60 => Some("8"),
        0x61 => Some("9"),
        0x62 => Some("0"),
        0x63 => Some("."),
        0x65 => Some("Menu"),
        0xE0 => Some("Ctrl"),
        0xE1 => Some("Maj"),
        0xE2 => Some("Alt"),
        0xE3 => Some("Win"),
        0xE4 => Some("Ctrl"),
        0xE5 => Some("Maj"),
        0xE6 => Some("AltGr"),
        0xE7 => Some("Win"),
        _ => None,
    }
}

/// Resolves the displayed label of a key under a given Windows keyboard layout
pub fn resolve_label(key: &KeyDef, action: Option<&KeyAction>, hkl: isize) -> String {
    if let Some(act) = action {
        if is_remapped(*act, key) {
            match act.kind() {
                KeyKind::Default => {}
                KeyKind::Keyboard { modifiers, usage } => {
                    if modifiers == 0 && usage == 0 {
                        return "OFF".to_string();
                    }
                    if let Some(name) = hid_to_friendly_name(usage) {
                        return name.to_string();
                    }
                    if let Some(sc) = hid_to_scancode(usage) {
                        if let Some(c) = type_char(sc, false, hkl) {
                            let trimmed = c.trim();
                            if !trimmed.is_empty() {
                                return trimmed.to_uppercase();
                            }
                        }
                    }
                    return format!("K_{:02X}", usage);
                }
                KeyKind::Consumer(code) => {
                    return match code {
                        233 => "Vol+".to_string(),
                        234 => "Vol-".to_string(),
                        226 => "Muet".to_string(),
                        205 => "Play".to_string(),
                        183 => "Stop".to_string(),
                        182 => "Préc".to_string(),
                        181 => "Suiv".to_string(),
                        402 => "Calc".to_string(),
                        404 => "Poste".to_string(),
                        _ => "Media".to_string(),
                    };
                }
                KeyKind::Mouse { kind, value } => {
                    return match (kind, value) {
                        (1, 1) => "Clic.G".to_string(),
                        (1, 2) => "Clic.D".to_string(),
                        (1, 4) => "Clic.M".to_string(),
                        (3, 1) => "Mol+".to_string(),
                        (3, 255) => "Mol-".to_string(),
                        (1, 16) => "Avanc".to_string(),
                        (1, 8) => "Recul".to_string(),
                        _ => "Souris".to_string(),
                    };
                }
                KeyKind::Socd(_) => return "SOCD".to_string(),
                KeyKind::Mt(_) => return "M-Tap".to_string(),
                KeyKind::Tgl(_) => return "Toggle".to_string(),
                KeyKind::Dks(_) => return "DKS".to_string(),
                _ => return "Custom".to_string(),
            }
        }
    }

    // Keep standard short names for modifier/control keys
    match key.code.as_str() {
        "Escape" => return "Échap".to_string(),
        "Backspace" => return "Retour".to_string(),
        "Tab" => return "Tab".to_string(),
        "CapsLock" => return "Verr.Maj".to_string(),
        "Enter" => return "Entrée".to_string(),
        "ShiftLeft" | "ShiftRight" => return "Maj".to_string(),
        "ControlLeft" | "ControlRight" => return "Ctrl".to_string(),
        "AltLeft" => return "Alt".to_string(),
        "AltRight" => return "AltGr".to_string(),
        "MetaLeft" | "MetaRight" => return "Win".to_string(),
        "Space" => return "Espace".to_string(),
        "ContextMenu" => return "Menu".to_string(),
        "Insert" => return "Inser".to_string(),
        "Delete" => return "Suppr".to_string(),
        "Home" => return "Début".to_string(),
        "End" => return "Fin".to_string(),
        "PageUp" => return "Pg.Préc".to_string(),
        "PageDown" => return "Pg.Suiv".to_string(),
        "NumLock" => return "Verr.Num".to_string(),
        "PrintScreen" => return "Impr".to_string(),
        "ScrollLock" => return "Arrêt".to_string(),
        "Pause" => return "Pause".to_string(),
        "ArrowUp" => return "↑".to_string(),
        "ArrowDown" => return "↓".to_string(),
        "ArrowLeft" => return "←".to_string(),
        "ArrowRight" => return "→".to_string(),
        "Numpad0" => return "0".to_string(),
        "Numpad1" => return "1".to_string(),
        "Numpad2" => return "2".to_string(),
        "Numpad3" => return "3".to_string(),
        "Numpad4" => return "4".to_string(),
        "Numpad5" => return "5".to_string(),
        "Numpad6" => return "6".to_string(),
        "Numpad7" => return "7".to_string(),
        "Numpad8" => return "8".to_string(),
        "Numpad9" => return "9".to_string(),
        "NumpadDivide" => return "/".to_string(),
        "NumpadMultiply" => return "*".to_string(),
        "NumpadSubtract" => return "-".to_string(),
        "NumpadAdd" => return "+".to_string(),
        "NumpadDecimal" => return ".".to_string(),
        "NumpadEnter" => return "Entrée".to_string(),
        _ => {}
    }

    if key.hid == 175 || key.label.eq_ignore_ascii_case("fn") || key.code.eq_ignore_ascii_case("fn") {
        return "Fn".to_string();
    }

    // Function keys F1..F12
    if key.code.starts_with('F') && key.code[1..].chars().all(|c| c.is_ascii_digit()) {
        return key.code.clone();
    }

    if let Some(sc) = hid_to_scancode(key.hid) {
        let plain = type_char(sc, false, hkl);
        let shifted = type_char(sc, true, hkl);

        let clean_plain = plain.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let clean_shifted = shifted.as_deref().map(str::trim).filter(|s| !s.is_empty());

        if let (Some(p), Some(s)) = (clean_plain, clean_shifted) {
            let p_upper = p.to_uppercase();
            if s == p || s == p_upper {
                return p_upper;
            } else if p.chars().count() == 1 && s.chars().count() == 1 {
                // Dual character label (e.g. 1/& or ?/, or :/.)
                return format!("{}\n{}", s, p);
            } else {
                return p_upper;
            }
        } else if let Some(p) = clean_plain {
            return p.to_uppercase();
        } else if let Some(s) = clean_shifted {
            return s.to_string();
        }
    }

    key.label.clone()
}

/// Builds the mapping from Windows Virtual-Key code to key matrix index
pub fn build_vk_map(layout: &Layout, hkl: isize) -> HashMap<u32, u8> {
    let mut map = HashMap::new();

    for k in &layout.keys {
        // Alphanumeric keys via scancode -> VK
        if let Some(sc) = hid_to_scancode(k.hid) {
            let vk = unsafe { MapVirtualKeyExW((sc & 0xFF) as u32, MAPVK_VSC_TO_VK_EX, hkl) };
            if vk != 0 {
                map.insert(vk, k.index);
            }
        }

        // Exact match by key code for standard special / navigation / numpad keys
        match k.code.as_str() {
            "Escape" => { map.insert(0x1B, k.index); }
            "Backspace" => { map.insert(0x08, k.index); }
            "Tab" => { map.insert(0x09, k.index); }
            "Enter" => { map.insert(0x0D, k.index); }
            "Space" => { map.insert(0x20, k.index); }
            "CapsLock" => { map.insert(0x14, k.index); }
            "ShiftLeft" => { map.insert(0xA0, k.index); map.insert(0x10, k.index); }
            "ShiftRight" => { map.insert(0xA1, k.index); }
            "ControlLeft" => { map.insert(0xA2, k.index); map.insert(0x11, k.index); }
            "ControlRight" => { map.insert(0xA3, k.index); }
            "AltLeft" => { map.insert(0xA4, k.index); map.insert(0x12, k.index); }
            "AltRight" => { map.insert(0xA5, k.index); }
            "MetaLeft" => { map.insert(0x5B, k.index); }
            "MetaRight" => { map.insert(0x5C, k.index); }
            "Insert" => { map.insert(0x2D, k.index); }
            "Delete" => { map.insert(0x2E, k.index); }
            "Home" => { map.insert(0x24, k.index); }
            "End" => { map.insert(0x23, k.index); }
            "PageUp" => { map.insert(0x21, k.index); }
            "PageDown" => { map.insert(0x22, k.index); }
            "ArrowLeft" => { map.insert(0x25, k.index); }
            "ArrowUp" => { map.insert(0x26, k.index); }
            "ArrowRight" => { map.insert(0x27, k.index); }
            "ArrowDown" => { map.insert(0x28, k.index); }
            "PrintScreen" => { map.insert(0x2C, k.index); }
            "ScrollLock" => { map.insert(0x91, k.index); }
            "Pause" => { map.insert(0x13, k.index); }
            "NumLock" => { map.insert(0x90, k.index); }
            "Numpad0" => { map.insert(0x60, k.index); }
            "Numpad1" => { map.insert(0x61, k.index); }
            "Numpad2" => { map.insert(0x62, k.index); }
            "Numpad3" => { map.insert(0x63, k.index); }
            "Numpad4" => { map.insert(0x64, k.index); }
            "Numpad5" => { map.insert(0x65, k.index); }
            "Numpad6" => { map.insert(0x66, k.index); }
            "Numpad7" => { map.insert(0x67, k.index); }
            "Numpad8" => { map.insert(0x68, k.index); }
            "Numpad9" => { map.insert(0x69, k.index); }
            "NumpadMultiply" => { map.insert(0x6A, k.index); }
            "NumpadAdd" => { map.insert(0x6B, k.index); }
            "NumpadSubtract" => { map.insert(0x6D, k.index); }
            "NumpadDecimal" => { map.insert(0x6E, k.index); }
            "NumpadDivide" => { map.insert(0x6F, k.index); }
            "NumpadEnter" => { map.insert(0x0E, k.index); }
            "F1" => { map.insert(0x70, k.index); }
            "F2" => { map.insert(0x71, k.index); }
            "F3" => { map.insert(0x72, k.index); }
            "F4" => { map.insert(0x73, k.index); }
            "F5" => { map.insert(0x74, k.index); }
            "F6" => { map.insert(0x75, k.index); }
            "F7" => { map.insert(0x76, k.index); }
            "F8" => { map.insert(0x77, k.index); }
            "F9" => { map.insert(0x78, k.index); }
            "F10" => { map.insert(0x79, k.index); }
            "F11" => { map.insert(0x7A, k.index); }
            "F12" => { map.insert(0x7B, k.index); }
            _ => {}
        }

        if k.hid == 175 || k.label.eq_ignore_ascii_case("fn") {
            map.insert(0xDF, k.index); // VK_OEM_8
            map.insert(0xFC, k.index); // VK_NONAME
            map.insert(0xFF, k.index); // Vendor key
            map.insert(0x87, k.index); // VK_F24 (used by some firmware for Fn)
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layouts() {
        let hkl_azerty = get_hkl(LayoutPreference::Azerty);
        let hkl_qwerty = get_hkl(LayoutPreference::Qwerty);

        let k_q = KeyDef { index: 33, label: "Q".to_string(), hid: 20, code: "KeyQ".to_string(), x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
        assert_eq!(resolve_label(&k_q, None, hkl_azerty), "A");
        assert_eq!(resolve_label(&k_q, None, hkl_qwerty), "Q");

        let k_esc = KeyDef { index: 0, label: "Esc".to_string(), hid: 41, code: "Escape".to_string(), x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
        let k_f10 = KeyDef { index: 10, label: "F10".to_string(), hid: 67, code: "F10".to_string(), x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
        let k_home = KeyDef { index: 104, label: "Home".to_string(), hid: 74, code: "Home".to_string(), x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
        let k_num1 = KeyDef { index: 77, label: "Num 1".to_string(), hid: 89, code: "Numpad1".to_string(), x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
        let k_caps = KeyDef { index: 48, label: "Caps".to_string(), hid: 57, code: "CapsLock".to_string(), x: 0.0, y: 0.0, w: 1.0, h: 1.0 };

        let default_act = KeyAction { raw: [0x02, 0, 74, 0] }; // Home key read from firmware
        assert!(!is_remapped(default_act, &k_home));
        assert_eq!(resolve_label(&k_esc, None, hkl_azerty), "Échap");
        assert_eq!(resolve_label(&k_f10, None, hkl_azerty), "F10");
        assert_eq!(resolve_label(&k_home, Some(&default_act), hkl_azerty), "Début");
        assert_eq!(resolve_label(&k_num1, None, hkl_azerty), "1");
        assert_eq!(resolve_label(&k_caps, None, hkl_azerty), "Verr.Maj");

        let layout = Layout::ak980max();
        let vk_map_az = build_vk_map(layout, hkl_azerty);
        let vk_map_qw = build_vk_map(layout, hkl_qwerty);
        assert_eq!(vk_map_az.get(&0x41), Some(&33));
        assert_eq!(vk_map_qw.get(&0x51), Some(&33));
        assert_eq!(vk_map_az.get(&0x61), Some(&77));
        assert_eq!(vk_map_az.get(&0x6F), Some(&30));
    }
}
