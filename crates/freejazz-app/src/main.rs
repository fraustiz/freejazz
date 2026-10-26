//! Freejazz: native configurator for the Ajazz AK980 Max.
//!
//! `freejazz [--mock | --offline] [--screenshot <file.png>] [--section <id>]
//! [--tray] [--theme dark|light] [--select <group|labels>]
//! [--keyboard-layout <KLID>] [--dirty] [--offscreen] [--debug-stop-worker]`

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::ExitCode;

use eframe::egui;

use freejazz::app::{FreejazzApp, Options, Section};

const USAGE: &str = "Utilisation : freejazz [--mock | --offline] [--screenshot <fichier.png>] \
[--section keys|lighting|live|macros|performance|advanced|settings|test] \
[--theme dark|light] [--select <groupe|Z,Q,S,D>] [--keyboard-layout <KLID>] [--dirty] [--offscreen] \
[--tray] [--debug-stop-worker]";

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut opts = Options {
        mock: false,
        offline: false,
        demo_error: false,
        demo_macros: false,
        demo_live: false,
        demo_test: None,
        demo_profile: None,
        stop_worker: false,
        section: None,
        screenshot: None,
        demo_dirty: false,
        demo_design: None,
        theme: None,
        select: None,
        offscreen: false,
        start_hidden: false,
        show_signal: None,
        tray_icon: None,
    };
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} attend une valeur"));
        match arg.as_str() {
            "--mock" => opts.mock = true,
            "--offline" => opts.offline = true,
            "--debug-stop-worker" => opts.stop_worker = true,
            "--dirty" => opts.demo_dirty = true,
            "--demo-design" => opts.demo_design = Some(value("--demo-design")?),
            "--demo-error" => opts.demo_error = true,
            "--demo-macros" => opts.demo_macros = true,
            "--demo-live" => opts.demo_live = true,
            "--demo-profile" => opts.demo_profile = Some(value("--demo-profile")?),
            "--demo-test" => opts.demo_test = Some(value("--demo-test")?),
            "--offscreen" => opts.offscreen = true,
            "--tray" => opts.start_hidden = true,
            "--keyboard-layout" => {
                let klid = value("--keyboard-layout")?;
                if !freejazz::keyboard::labels::force_layout(&klid) {
                    return Err(format!("disposition de clavier inconnue : {klid}"));
                }
            }
            "--select" => opts.select = Some(value("--select")?),
            "--screenshot" => opts.screenshot = Some(PathBuf::from(value("--screenshot")?)),
            "--section" => {
                let id = value("--section")?;
                opts.section =
                    Some(Section::from_id(&id).ok_or(format!("section inconnue : {id}"))?);
            }
            "--theme" => {
                opts.theme = Some(match value("--theme")?.as_str() {
                    "dark" => egui::Theme::Dark,
                    "light" => egui::Theme::Light,
                    other => return Err(format!("thème inconnu : {other}")),
                })
            }
            other => return Err(format!("option inconnue : {other}")),
        }
    }
    Ok(opts)
}

fn icon() -> Option<egui::IconData> {
    let decoder = png::Decoder::new(std::io::Cursor::new(
        include_bytes!("../assets/icon.png").as_slice(),
    ));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    Some(egui::IconData {
        rgba: buf,
        width: info.width,
        height: info.height,
    })
}

fn main() -> ExitCode {
    // `--relaunch-after <pid>` (after an update): let the previous process end first,
    // so it has released the single-instance lock.
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(at) = args.iter().position(|a| a == "--relaunch-after") {
        let pid = args.get(at + 1).and_then(|p| p.parse().ok());
        args.drain(at..(at + 2).min(args.len()));
        if let Some(pid) = pid {
            freejazz::updater::wait_for_exit(pid);
        }
    }
    let opts = match parse_args(args.into_iter()) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("{e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mut opts = opts;
    // One instance for real runs; measurements, mock and screenshots never signal (or
    // wake) the user's own Freejazz.
    let real = !opts.mock && !opts.offline && !opts.offscreen && opts.screenshot.is_none();
    if real {
        match freejazz::single_instance::acquire() {
            Some(signal) => opts.show_signal = Some(signal),
            None => return ExitCode::SUCCESS,
        }
    }
    opts.tray_icon = icon();
    freejazz::live::engine::install_panic_hook();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Freejazz")
        .with_app_id("freejazz")
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([1100.0, 720.0]);
    if let Some(icon) = icon() {
        viewport = viewport.with_icon(icon);
    }
    // `--screenshot` and `--offscreen` (measurements) never show a window to the
    // user: it is placed far outside every monitor, not activated (no focus stolen),
    // without a taskbar button. eframe skips the egui pass of an invisible window, so
    // it stays "visible", just off screen.
    let offscreen = opts.offscreen || opts.screenshot.is_some();
    if offscreen {
        viewport = viewport
            .with_position([-30000.0, -30000.0])
            .with_active(false)
            .with_taskbar(false);
    }
    let native = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        centered: !offscreen,
        persist_window: false,
        ..Default::default()
    };
    match eframe::run_native(
        "Freejazz",
        native,
        Box::new(|cc| Ok(Box::new(FreejazzApp::new(cc, opts)))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Freejazz n'a pas pu démarrer : {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_screenshot_flags() {
        let o = parse(&["--mock", "--screenshot", "a.png", "--section", "lighting"]).unwrap();
        assert!(o.mock);
        assert_eq!(o.screenshot, Some(PathBuf::from("a.png")));
        assert_eq!(o.section, Some(Section::Lighting));
        assert!(parse(&["--section", "nope"]).is_err());
        assert!(parse(&["--screenshot"]).is_err());
        assert!(parse(&["--bogus"]).is_err());
        assert!(parse(&["--offscreen"]).unwrap().offscreen);
        assert_eq!(
            parse(&["--select", "Z,Q,S,D"]).unwrap().select.as_deref(),
            Some("Z,Q,S,D")
        );
        assert!(parse(&["--offline"]).unwrap().offline);
        assert!(parse(&["--debug-stop-worker"]).unwrap().stop_worker);
    }

    #[test]
    fn icon_decodes() {
        let icon = icon().expect("icon.png is 8-bit RGBA");
        assert_eq!((icon.width, icon.height), (256, 256));
    }
}
