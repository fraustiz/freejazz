//! The notification-area icon, on its own thread with its own message loop (it must keep
//! answering while the egui window is hidden).

use std::ptr::null_mut;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;

use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
    TranslateMessage, WM_QUIT,
};

use crate::single_instance::ShowSignal;

/// While the window is hidden the working set is trimmed this often (ms).
const HIDDEN_TRIM_EVERY_MS: u32 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    Open,
    StopEffect,
    Quit,
}

/// What the app and the tray thread share.
#[derive(Clone, Default)]
pub struct TrayFlags {
    /// The icon exists (closing to the tray makes sense only then).
    pub ready: Arc<AtomicBool>,
    /// The window is hidden.
    pub hidden: Arc<AtomicBool>,
}

pub fn spawn(
    signal: Option<ShowSignal>,
    icon: egui::IconData,
    tx: Sender<TrayCommand>,
    ctx: egui::Context,
    flags: TrayFlags,
) {
    let _ = thread::Builder::new()
        .name("tray".into())
        .spawn(move || run(signal, icon, tx, ctx, flags));
}

fn run(
    signal: Option<ShowSignal>,
    icon: egui::IconData,
    tx: Sender<TrayCommand>,
    ctx: egui::Context,
    flags: TrayFlags,
) {
    // the window may be hidden: egui is woken so it handles the command
    let send = Arc::new(move |cmd: TrayCommand| {
        let _ = tx.send(cmd);
        ctx.request_repaint();
    });

    let open = MenuItem::new("Ouvrir Freejazz", true, None);
    let stop = MenuItem::new("Arrêter l'effet", true, None);
    let quit = MenuItem::new("Quitter", true, None);
    let Ok(menu) = Menu::with_items(&[&open, &stop, &PredefinedMenuItem::separator(), &quit])
    else {
        return;
    };
    let Ok(icon) = Icon::from_rgba(icon.rgba, icon.width, icon.height) else {
        return;
    };
    let Ok(_tray) = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("Freejazz — Ajazz AK980 Max")
        .with_icon(icon)
        .build()
    else {
        return;
    };
    flags.ready.store(true, Ordering::SeqCst);

    let (open_id, stop_id, quit_id) = (open.id().clone(), stop.id().clone(), quit.id().clone());
    let s = send.clone();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if e.id == open_id {
            s(TrayCommand::Open);
        } else if e.id == stop_id {
            s(TrayCommand::StopEffect);
        } else if e.id == quit_id {
            s(TrayCommand::Quit);
        }
    }));
    let s = send.clone();
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if matches!(
            e,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
        ) {
            s(TrayCommand::Open);
        }
    }));

    // message loop + the event a second launch sets; the periodic wake-up trims memory
    // while the window is hidden
    let handles: Vec<_> = signal.iter().map(|s| s.0).collect();
    loop {
        // SAFETY: valid event handle (or none) for the whole process.
        let r = unsafe {
            MsgWaitForMultipleObjects(
                handles.len() as u32,
                if handles.is_empty() {
                    std::ptr::null()
                } else {
                    handles.as_ptr()
                },
                0,
                HIDDEN_TRIM_EVERY_MS,
                QS_ALLINPUT,
            )
        };
        if !handles.is_empty() && r == WAIT_OBJECT_0 {
            send(TrayCommand::Open);
            continue;
        }
        if flags.hidden.load(Ordering::SeqCst) {
            crate::mem::trim_working_set();
        }
        // SAFETY: standard Win32 message pump.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    return;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
