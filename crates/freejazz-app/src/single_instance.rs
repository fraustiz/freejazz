//! One Freejazz at a time: a named mutex, and a named event the second launch sets so the
//! first one shows its window.

use std::ptr::null;

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, OpenEventW, SetEvent,
};

pub const INSTANCE_MUTEX: &str = r"Local\Freejazz.Instance";
pub const SHOW_EVENT: &str = r"Local\Freejazz.Show";

/// The « show your window » event of the first instance.
pub struct ShowSignal(pub HANDLE);

// SAFETY: an event HANDLE can be used from any thread.
unsafe impl Send for ShowSignal {}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `Some(signal)` for the first instance; `None` when one already runs (it was asked to
/// show its window).
pub fn acquire() -> Option<ShowSignal> {
    acquire_named(INSTANCE_MUTEX, SHOW_EVENT)
}

pub fn acquire_named(mutex: &str, event: &str) -> Option<ShowSignal> {
    let (mutex, event) = (wide(mutex), wide(event));
    // SAFETY: NUL-terminated names. The mutex handle stays open until the process ends on
    // purpose: that is what keeps the other launches out.
    unsafe {
        CreateMutexW(null(), 0, mutex.as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let ev = OpenEventW(EVENT_MODIFY_STATE, 0, event.as_ptr());
            if !ev.is_null() {
                SetEvent(ev);
            }
            return None;
        }
        Some(ShowSignal(CreateEventW(null(), 0, 0, event.as_ptr())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    #[test]
    fn the_second_launch_is_refused_and_wakes_the_first() {
        let (m, e) = (r"Local\FreejazzTest.Instance", r"Local\FreejazzTest.Show");
        let first = acquire_named(m, e).expect("first instance");
        assert!(acquire_named(m, e).is_none());
        // SAFETY: the event handle is valid.
        assert_eq!(unsafe { WaitForSingleObject(first.0, 1000) }, WAIT_OBJECT_0);
    }
}
