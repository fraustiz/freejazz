//! Real-time keyboard input listener for the key matrix tester

use std::time::Duration;
use windows_reactor::CancellationToken;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;

use crate::labels::{self, LayoutPreference};
use crate::layout::Layout;

/// Polls physical key states continuously in the background using the resolved layout
pub fn poll_keys(token: CancellationToken, pref: LayoutPreference) -> Vec<u8> {
    let layout = Layout::ak980max();
    let hkl = labels::get_hkl(pref);
    let vk_map = labels::build_vk_map(layout, hkl);

    let mut detected = Vec::new();
    let mut loops = 0;

    // Check each key at ~10ms intervals until cancellation, key detection, or timeout (200ms)
    while !token.is_cancelled() && loops < 20 {
        loops += 1;
        for (&vk, &idx) in &vk_map {
            unsafe {
                let state = GetAsyncKeyState(vk as i32);
                if (state as u16 & 0x8000) != 0 {
                    detected.push(idx);
                }
            }
        }

        if !detected.is_empty() {
            break;
        }

        std::thread::sleep(Duration::from_millis(10));
    }

    detected
}
