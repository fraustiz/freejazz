//! Global key events: a `WH_KEYBOARD_LL` hook mapped to layout indices **by scan code**,
//! so it sees every key (Pause, PrintScreen, ScrollLock, Win, numpad, right modifiers)
//! whatever the Windows keyboard layout. Meant to be shared: the live effects use it now,
//! the macro recorder and the key test can [`subscribe`] to it later.
//!
//! The hook is installed when the first [`Subscription`] is created and removed when
//! the last one is dropped. Nothing installs it by itself (tests never do).

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Instant;

use crate::keyboard::Layout;

/// Seconds since the first call: the clock of [`KeyEvent::at`] and of the engine.
pub fn now() -> f32 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_secs_f32()
}

/// One key going down or up, as the hook saw it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyEvent {
    /// Scan code (set 1), `0xE000 |` it for extended keys.
    pub scan: u16,
    /// Windows virtual-key code.
    pub vk: u32,
    pub down: bool,
    /// Auto-repeat of a key already down.
    pub repeat: bool,
    /// The keyboard's layout index; `None` for a key the layout does not have.
    pub index: Option<u8>,
    pub at: f32,
    /// A window of this app had the focus: the key was typed into Freejazz, not into
    /// another program (the macro recorder and the key test keep only those).
    pub foreground: bool,
}

const EXTENDED: u16 = 0xE000;
const VK_PAUSE: u32 = 0x13;
const VK_NUMLOCK: u32 = 0x90;

/// Scan code (with [`EXTENDED`] for E0 keys) of a `layout.json` `code` name (the W3C
/// `KeyboardEvent.code` names of the site).
pub fn scan_for_code(code: &str) -> Option<u16> {
    let e = |s: u16| Some(EXTENDED | s);
    Some(match code {
        "Escape" => 0x01,
        "Digit1" => 0x02,
        "Digit2" => 0x03,
        "Digit3" => 0x04,
        "Digit4" => 0x05,
        "Digit5" => 0x06,
        "Digit6" => 0x07,
        "Digit7" => 0x08,
        "Digit8" => 0x09,
        "Digit9" => 0x0A,
        "Digit0" => 0x0B,
        "Minus" => 0x0C,
        "Equal" => 0x0D,
        "Backspace" => 0x0E,
        "Tab" => 0x0F,
        "KeyQ" => 0x10,
        "KeyW" => 0x11,
        "KeyE" => 0x12,
        "KeyR" => 0x13,
        "KeyT" => 0x14,
        "KeyY" => 0x15,
        "KeyU" => 0x16,
        "KeyI" => 0x17,
        "KeyO" => 0x18,
        "KeyP" => 0x19,
        "BracketLeft" => 0x1A,
        "BracketRight" => 0x1B,
        "Enter" => 0x1C,
        "ControlLeft" => 0x1D,
        "KeyA" => 0x1E,
        "KeyS" => 0x1F,
        "KeyD" => 0x20,
        "KeyF" => 0x21,
        "KeyG" => 0x22,
        "KeyH" => 0x23,
        "KeyJ" => 0x24,
        "KeyK" => 0x25,
        "KeyL" => 0x26,
        "Semicolon" => 0x27,
        "Quote" => 0x28,
        "Backquote" => 0x29,
        "ShiftLeft" => 0x2A,
        "Backslash" => 0x2B,
        "KeyZ" => 0x2C,
        "KeyX" => 0x2D,
        "KeyC" => 0x2E,
        "KeyV" => 0x2F,
        "KeyB" => 0x30,
        "KeyN" => 0x31,
        "KeyM" => 0x32,
        "Comma" => 0x33,
        "Period" => 0x34,
        "Slash" => 0x35,
        "ShiftRight" => 0x36,
        "NumpadMultiply" => 0x37,
        "AltLeft" => 0x38,
        "Space" => 0x39,
        "CapsLock" => 0x3A,
        "F1" => 0x3B,
        "F2" => 0x3C,
        "F3" => 0x3D,
        "F4" => 0x3E,
        "F5" => 0x3F,
        "F6" => 0x40,
        "F7" => 0x41,
        "F8" => 0x42,
        "F9" => 0x43,
        "F10" => 0x44,
        "NumLock" => 0x45,
        "ScrollLock" => 0x46,
        "Numpad7" => 0x47,
        "Numpad8" => 0x48,
        "Numpad9" => 0x49,
        "NumpadSubtract" => 0x4A,
        "Numpad4" => 0x4B,
        "Numpad5" => 0x4C,
        "Numpad6" => 0x4D,
        "NumpadAdd" => 0x4E,
        "Numpad1" => 0x4F,
        "Numpad2" => 0x50,
        "Numpad3" => 0x51,
        "Numpad0" => 0x52,
        "NumpadDecimal" => 0x53,
        "F11" => 0x57,
        "F12" => 0x58,
        // extended (E0) keys
        "NumpadEnter" => return e(0x1C),
        "ControlRight" => return e(0x1D),
        "NumpadDivide" => return e(0x35),
        "PrintScreen" => return e(0x37),
        "AltRight" => return e(0x38),
        "Home" => return e(0x47),
        "ArrowUp" => return e(0x48),
        "PageUp" => return e(0x49),
        "ArrowLeft" => return e(0x4B),
        "ArrowRight" => return e(0x4D),
        "End" => return e(0x4F),
        "ArrowDown" => return e(0x50),
        "PageDown" => return e(0x51),
        "Insert" => return e(0x52),
        "Delete" => return e(0x53),
        "MetaLeft" => return e(0x5B),
        "MetaRight" => return e(0x5C),
        "ContextMenu" => return e(0x5D),
        _ => return None,
    })
}

/// Scan code (as [`scan_for_code`]) to layout index, for the keys of `layout`.
pub fn scan_map(layout: &Layout) -> HashMap<u16, u8> {
    layout
        .keys
        .iter()
        .filter_map(|k| Some((scan_for_code(&k.code)?, k.index)))
        .collect()
}

fn map() -> &'static HashMap<u16, u8> {
    static MAP: OnceLock<HashMap<u16, u8>> = OnceLock::new();
    MAP.get_or_init(|| scan_map(Layout::ak980max()))
}

/// Layout index of a hook event. The low-level hook reports Pause as scan 0x45 and
/// NumLock as 0x45 + extended, so those two are told apart by virtual key.
pub fn index_for(scan: u32, extended: bool, vk: u32) -> Option<u8> {
    let table = map();
    match vk {
        VK_PAUSE => pause_index(),
        VK_NUMLOCK => table.get(&0x45).copied(),
        _ => table
            .get(&((scan as u16 & 0xFF) | if extended { EXTENDED } else { 0 }))
            .copied(),
    }
}

fn pause_index() -> Option<u8> {
    static PAUSE: OnceLock<Option<u8>> = OnceLock::new();
    *PAUSE.get_or_init(|| {
        Layout::ak980max()
            .keys
            .iter()
            .find(|k| k.code == "Pause")
            .map(|k| k.index)
    })
}

/// Keys currently down (to flag auto-repeat), by scan key.
static DOWN: Mutex<Vec<u16>> = Mutex::new(Vec::new());
static SUBS: Mutex<Vec<(u64, Sender<KeyEvent>)>> = Mutex::new(Vec::new());
static HOOK: Mutex<Option<HookThread>> = Mutex::new(None);
static NEXT_ID: Mutex<u64> = Mutex::new(0);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Turns one raw hook callback into an event and hands it to the subscribers. Pure
/// apart from the subscriber list and the repeat memory, so tests call it directly.
/// `foreground`: the focused window belongs to this process.
pub fn process(scan: u32, flags: u32, vk: u32, down: bool, foreground: bool) {
    const LLKHF_EXTENDED: u32 = 0x01;
    let extended = flags & LLKHF_EXTENDED != 0;
    // The fake Shift Windows injects around some extended keys while NumLock is on.
    if extended && scan == 0x2A {
        return;
    }
    let key = (scan as u16 & 0xFF) | if extended { EXTENDED } else { 0 };
    let repeat = {
        let mut keys = lock(&DOWN);
        let pos = keys.iter().position(|&k| k == key);
        match (down, pos) {
            (true, Some(_)) => true,
            (true, None) => {
                keys.push(key);
                false
            }
            (false, Some(p)) => {
                keys.swap_remove(p);
                false
            }
            (false, None) => false,
        }
    };
    let event = KeyEvent {
        scan: key,
        vk,
        down,
        repeat,
        index: index_for(scan, extended, vk),
        at: now(),
        foreground,
    };
    for (_, tx) in lock(&SUBS).iter() {
        let _ = tx.send(event);
    }
}

/// A stream of [`KeyEvent`]s. Dropping it unsubscribes (and removes the hook with the
/// last one).
pub struct Subscription {
    id: u64,
    rx: Receiver<KeyEvent>,
}

/// Starts listening, installing the hook if nobody else listens. `Err` if Windows
/// refuses the hook.
pub fn subscribe() -> Result<Subscription, String> {
    let (tx, rx) = channel();
    let id = {
        let mut n = lock(&NEXT_ID);
        *n += 1;
        *n
    };
    {
        let mut hook = lock(&HOOK);
        if hook.is_none() {
            *hook = Some(HookThread::install()?);
        }
    }
    lock(&SUBS).push((id, tx));
    Ok(Subscription { id, rx })
}

impl Subscription {
    /// Everything seen since the last call.
    pub fn drain(&self) -> Vec<KeyEvent> {
        self.rx.try_iter().collect()
    }

    /// Presses (not releases or auto-repeats) of layout keys since the last call.
    pub fn presses(&self) -> Vec<super::KeyPress> {
        self.drain()
            .into_iter()
            .filter(|e| e.down && !e.repeat)
            .filter_map(|e| {
                Some(super::KeyPress {
                    index: e.index?,
                    at: e.at,
                })
            })
            .collect()
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let empty = {
            let mut subs = lock(&SUBS);
            subs.retain(|(i, _)| *i != self.id);
            subs.is_empty()
        };
        if empty {
            if let Some(h) = lock(&HOOK).take() {
                h.remove();
            }
            lock(&DOWN).clear();
        }
    }
}

#[cfg(windows)]
struct HookThread {
    thread_id: u32,
    join: std::thread::JoinHandle<()>,
}

#[cfg(windows)]
impl HookThread {
    fn install() -> Result<Self, String> {
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::System::Threading::GetCurrentThreadId;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, GetMessageW, MSG, SetWindowsHookExW, TranslateMessage,
            UnhookWindowsHookEx, WH_KEYBOARD_LL,
        };
        let (ready_tx, ready_rx) = channel::<Result<u32, String>>();
        let join = std::thread::Builder::new()
            .name("freejazz-key-hook".into())
            .spawn(move || unsafe {
                let module = GetModuleHandleW(std::ptr::null());
                let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module, 0);
                if hook.is_null() {
                    let _ =
                        ready_tx.send(Err("Le crochet clavier a été refusé par Windows".into()));
                    return;
                }
                let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                UnhookWindowsHookEx(hook);
            })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv() {
            Ok(Ok(thread_id)) => Ok(Self { thread_id, join }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("Le crochet clavier ne démarre pas".into()),
        }
    }

    fn remove(self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        let _ = self.join.join();
    }
}

#[cfg(windows)]
unsafe extern "system" fn hook_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, HC_ACTION, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
        WM_SYSKEYUP,
    };
    if code == HC_ACTION as i32 && lparam != 0 {
        // SAFETY: for HC_ACTION Windows passes a valid KBDLLHOOKSTRUCT.
        let k = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let message = wparam as u32;
        let down = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
        if down || matches!(message, WM_KEYUP | WM_SYSKEYUP) {
            process(k.scanCode, k.flags, k.vkCode, down, app_in_foreground());
        }
    }
    // SAFETY: plain pass-through to the next hook.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Whether the window that receives the keys belongs to this process.
#[cfg(windows)]
fn app_in_foreground() -> bool {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };
    // SAFETY: plain queries; a null window gives process id 0.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, &mut pid);
        pid == GetCurrentProcessId()
    }
}

#[cfg(not(windows))]
struct HookThread;

#[cfg(not(windows))]
impl HookThread {
    fn install() -> Result<Self, String> {
        Err("Le crochet clavier n'existe que sous Windows".into())
    }

    fn remove(self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_keyed_layout_key_maps_to_its_own_scan_code() {
        let layout = Layout::ak980max();
        let map = scan_map(layout);
        // 101 keys: Fn has no code and Pause is told apart by virtual key
        let scanned = || {
            layout
                .keys
                .iter()
                .filter(|k| !k.code.is_empty() && k.code != "Pause")
        };
        assert_eq!(map.len(), scanned().count(), "two keys share a scan code");
        for k in scanned() {
            assert!(
                scan_for_code(&k.code).is_some(),
                "no scan code for {}",
                k.code
            );
        }
    }

    #[test]
    fn the_awkward_keys_are_seen() {
        let layout = Layout::ak980max();
        let idx = |code: &str| layout.keys.iter().find(|k| k.code == code).map(|k| k.index);
        // PrintScreen: E0 37; ScrollLock 46; Win left E0 5B; right Ctrl E0 1D; numpad
        // Enter E0 1C; numpad 7 without E0 is not Home (E0 47)
        assert_eq!(index_for(0x37, true, 0x2C), idx("PrintScreen"));
        assert_eq!(index_for(0x46, false, 0x91), idx("ScrollLock"));
        assert_eq!(index_for(0x5B, true, 0x5B), idx("MetaLeft"));
        assert_eq!(index_for(0x1D, true, 0xA3), idx("ControlRight"));
        assert_eq!(index_for(0x1C, true, 0x0D), idx("NumpadEnter"));
        assert_eq!(index_for(0x47, false, 0x24), idx("Numpad7"));
        assert_eq!(index_for(0x47, true, 0x24), idx("Home"));
        assert_eq!(index_for(0x36, false, 0xA1), idx("ShiftRight"));
        // Pause: scan 0x45 plain, VK_PAUSE; NumLock: scan 0x45 + extended, VK_NUMLOCK
        assert_eq!(index_for(0x45, false, VK_PAUSE), idx("Pause"));
        assert_eq!(index_for(0x45, true, VK_NUMLOCK), idx("NumLock"));
        assert!(idx("Pause").is_some() && idx("NumLock").is_some());
        // a key the keyboard does not have
        assert_eq!(index_for(0x70, false, 0x15), None);
    }

    #[test]
    fn process_flags_repeats_and_drops_the_fake_shift() {
        // the hook is never installed in tests: events go straight to a channel
        let (tx, rx) = channel();
        lock(&SUBS).push((u64::MAX, tx));
        lock(&DOWN).clear();
        let esc = Layout::ak980max()
            .keys
            .iter()
            .find(|k| k.code == "Escape")
            .unwrap()
            .index;
        process(0x01, 0, 0x1B, true, true);
        process(0x01, 0, 0x1B, true, true);
        process(0x01, 0, 0x1B, false, false);
        process(0x2A, 1, 0x10, true, true); // fake shift
        lock(&SUBS).retain(|(i, _)| *i != u64::MAX);
        let ev: Vec<KeyEvent> = rx.try_iter().collect();
        assert_eq!(ev.len(), 3);
        assert!(ev[0].down && !ev[0].repeat && ev[0].index == Some(esc));
        assert!(ev[1].repeat && ev[1].foreground);
        assert!(!ev[2].down && !ev[2].foreground);
        assert!(lock(&HOOK).is_none(), "tests never install the hook");
    }
}
