//! `stream-test` (the live LED stream, 0x32) and `depth-test` (key depth, 0x68).
//!
//! `stream-test` sends a rainbow sweep as the site's music stream does (protocol.md
//! 6.12), measures the frames per second achieved, then — the stream stopped, nothing
//! else sent — polls `GET_LED_EFFECT` for `--watch` seconds, and last restores the effect
//! of a backup (`end_live`, one `SET_LED_EFFECT`) unless `--no-restore`. The guard allows
//! only `GET_DEVICE_INFO` 0x10, `GET_LED_EFFECT` 0x13, the stream 0x32 and, for the
//! restore, `SET_LED_EFFECT` 0x23.
//!
//! `depth-test` polls `GET_MAGNETIC_AXIS_STATUS` 0x68, which the site never sends to
//! this model: it refuses without `--unverified` (UNVERIFIED-HW, to check on the hardware).

use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use freejazz_protocol::backup::load_backup;
use freejazz_protocol::blocks::led::LED_EFFECT_LEN;
use freejazz_protocol::blocks::{LedEffect, Rgb};
use freejazz_protocol::cmd::Cmd;
use freejazz_protocol::live::{FRAME_SLOTS, LIVE_AMPLITUDE, depth_to_mm};
use freejazz_protocol::{Error, Result};

use crate::{LAYOUT, color, connect, label, mm, spaced};

/// Connection, effect reads, the stream, and the restore of the stored effect.
const STREAM: &[u8] = &[0x10, 0x13, 0x23, 0x32];
/// The same without the restore.
const STREAM_NO_RESTORE: &[u8] = &[0x10, 0x13, 0x32];
// UNVERIFIED-HW: 0x68 is sent by the site only during calibration v2 (frameVersion 1).
const DEPTH: &[u8] = &[0x10, 0x68];

/// Options of `stream-test`.
pub(crate) struct StreamArgs<'a> {
    pub seconds: f32,
    /// Target frames per second; 0 = as fast as the keyboard answers.
    pub fps: f32,
    /// Backup whose LED effect `end_live` restores.
    pub backup: Option<&'a Path>,
    /// Seconds of `GET_LED_EFFECT` polling after the stream, before the restore.
    pub watch: f32,
    pub no_restore: bool,
    pub yes: bool,
}

/// HSV (hue in degrees, full saturation and value) to RGB.
fn hue(h: f32) -> Rgb {
    let h = h.rem_euclid(360.0) / 60.0;
    let x = ((1.0 - (h % 2.0 - 1.0).abs()) * 255.0).round() as u8;
    let (r, g, b) = match h as u32 {
        0 => (255, x, 0),
        1 => (x, 255, 0),
        2 => (0, 255, x),
        3 => (0, x, 255),
        4 => (x, 0, 255),
        _ => (255, 0, x),
    };
    Rgb { r, g, b }
}

/// Rainbow across the keyboard's width, moving by one turn per second, at time `t`.
fn rainbow(t: f32) -> [Rgb; FRAME_SLOTS] {
    let width = LAYOUT.keys.iter().map(|k| k.x + k.w).fold(1.0f32, f32::max);
    let mut frame = [Rgb::default(); FRAME_SLOTS];
    for k in &LAYOUT.keys {
        frame[k.index] = hue((k.x + k.w / 2.0) / width * 360.0 - t * 360.0);
    }
    frame
}

fn effect_line(e: &LedEffect, raw: &[u8]) -> String {
    format!(
        "mode {} couleur {} colorMode {} luminosité {} vitesse {} direction {} — {}",
        e.mode,
        color(e.rgb),
        e.color_mode,
        e.brightness,
        e.speed,
        e.direction,
        spaced(raw)
    )
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[i]
}

pub(crate) fn stream_test(args: StreamArgs<'_>, trace: bool) -> Result<ExitCode> {
    let stored = match (args.backup, args.no_restore) {
        (Some(path), _) => Some(load_backup(path)?.led_effect),
        (None, true) => None,
        (None, false) => {
            return Err(Error::Validation(
                "Précisez --backup <fichier> (l'effet à remettre après le flux), ou --no-restore"
                    .into(),
            ));
        }
    };
    if let Some(e) = &stored {
        println!(
            "Effet à remettre après le flux (sauvegarde) : mode {} couleur {} colorMode {} luminosité {} vitesse {} direction {}",
            e.mode,
            color(e.rgb),
            e.color_mode,
            e.brightness,
            e.speed,
            e.direction
        );
    }
    println!(
        "Flux : arc-en-ciel pendant {} s, {} ; puis {} s de lecture de GET_LED_EFFECT ; puis {}",
        args.seconds,
        if args.fps > 0.0 {
            format!("{} images/s visées", args.fps)
        } else {
            "aussi vite que le clavier répond".into()
        },
        args.watch,
        if args.no_restore {
            "rien (--no-restore)".to_string()
        } else {
            "SET_LED_EFFECT de l'effet de la sauvegarde".to_string()
        }
    );
    if !args.yes {
        eprintln!("Rien n'a été envoyé au clavier. Ajoutez --yes pour lancer le flux.");
        return Ok(ExitCode::from(2));
    }

    let allowed = if args.no_restore {
        STREAM_NO_RESTORE
    } else {
        STREAM
    };
    let (mut kb, _) = connect(allowed, trace)?;
    let before = kb.read_raw(Cmd::GetLedEffect, 0, LED_EFFECT_LEN)?;
    let before_effect = LedEffect::decode(&before)?;
    println!("Avant le flux : {}", effect_line(&before_effect, &before));
    if let Some(e) = &stored
        && !e.matches_readback(&before_effect)
    {
        return Err(Error::Validation(
            "L'effet du clavier ne correspond pas à celui de la sauvegarde (mode, luminosité, \
             vitesse, direction) : flux non lancé"
                .into(),
        ));
    }

    kb.begin_live()?;
    let period = (args.fps > 0.0).then(|| Duration::from_secs_f32(1.0 / args.fps));
    let start = Instant::now();
    let end = start + Duration::from_secs_f32(args.seconds);
    let timeouts_before = kb.link().timeouts;
    let mut times = Vec::new();
    let mut next = start;
    let mut failure = None;
    while Instant::now() < end {
        if let Some(p) = period {
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            }
            next += p;
        }
        let frame = rainbow(start.elapsed().as_secs_f32());
        let t0 = Instant::now();
        if let Err(e) = kb.send_frame(&frame, LIVE_AMPLITUDE) {
            failure = Some(e);
            break;
        }
        times.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    let elapsed = start.elapsed().as_secs_f64();
    let stopped = Instant::now();
    let retries = kb.link().timeouts - timeouts_before;
    let frames = times.len();
    let mut sorted = times.clone();
    sorted.sort_by(f64::total_cmp);
    let mean = times.iter().sum::<f64>() / frames.max(1) as f64;
    println!(
        "Flux : {frames} images en {elapsed:.2} s = {:.1} images/s ; envoi d'une image : min {:.1} ms, moyenne {mean:.1} ms, p95 {:.1} ms, max {:.1} ms ; {retries} attente(s) sans réponse (relances)",
        frames as f64 / elapsed,
        percentile(&sorted, 0.0),
        percentile(&sorted, 0.95),
        percentile(&sorted, 1.0),
    );
    if let Some(e) = &failure {
        println!("Flux interrompu : {e}");
    }

    if args.watch > 0.0 {
        println!("Flux arrêté. Lecture de GET_LED_EFFECT toutes les 500 ms, sans rien écrire :");
        let watch_end = stopped + Duration::from_secs_f32(args.watch);
        let mut last: Option<Vec<u8>> = None;
        while Instant::now() < watch_end {
            let raw = kb.read_raw(Cmd::GetLedEffect, 0, LED_EFFECT_LEN)?;
            if last.as_ref() != Some(&raw) {
                println!(
                    "  +{:.2} s : {}",
                    stopped.elapsed().as_secs_f64(),
                    effect_line(&LedEffect::decode(&raw)?, &raw)
                );
                last = Some(raw);
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        println!(
            "  +{:.2} s : fin de la lecture (aucun autre changement)",
            stopped.elapsed().as_secs_f64()
        );
    }

    if let Some(e) = &stored
        && !args.no_restore
    {
        kb.end_live(e)?;
        let raw = kb.read_raw(Cmd::GetLedEffect, 0, LED_EFFECT_LEN)?;
        let read = LedEffect::decode(&raw)?;
        println!(
            "Effet remis (SET_LED_EFFECT), relu : {} — {}",
            effect_line(&read, &raw),
            if e.matches_readback(&read) {
                "OK (couleur et colorMode non vérifiables par relecture)"
            } else {
                "DIFFÉRENT"
            }
        );
    }
    Ok(if failure.is_some() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

pub(crate) fn depth_test(seconds: f32, unverified: bool, trace: bool) -> Result<ExitCode> {
    // UNVERIFIED-HW: the site never sends GET_MAGNETIC_AXIS_STATUS 0x68 to this model.
    if !unverified {
        eprintln!(
            "UNVERIFIED-HW : le site n'envoie GET_MAGNETIC_AXIS_STATUS (0x68) que pendant le \
             calibrage v2 (frameVersion 1), jamais à ce modèle, et n'envoie jamais 0x60 ; \
             lecture de la profondeur non effectuée (ajoutez --unverified, à vérifier sur le matériel)."
        );
        return Ok(ExitCode::from(2));
    }
    let (mut kb, _) = connect(DEPTH, trace)?;
    let start = Instant::now();
    let end = start + Duration::from_secs_f32(seconds);
    let mut polls = 0usize;
    let mut first = None;
    let mut deepest = [0u16; FRAME_SLOTS];
    while Instant::now() < end {
        let status = kb.read_key_status()?;
        for s in &status {
            let d = &mut deepest[usize::from(s.index)];
            *d = (*d).max(s.travel);
        }
        first.get_or_insert(status);
        polls += 1;
    }
    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "{polls} lectures en {elapsed:.2} s = {:.1} lectures/s",
        polls as f64 / elapsed
    );
    if let Some(first) = first {
        println!("Première lecture (index, état, ADC, course brute) :");
        for s in &first {
            println!(
                "  {:>3} {:<14} état {} ADC {} course {}",
                s.index,
                label(usize::from(s.index)),
                s.state,
                s.adc,
                s.travel
            );
        }
    }
    let pressed: Vec<String> = deepest
        .iter()
        .enumerate()
        .filter(|&(_, &d)| d > 0)
        .map(|(i, &d)| format!("{} {} mm", label(i), mm(depth_to_mm(d, &kb.info))))
        .collect();
    println!(
        "Course maximale non nulle : {}",
        if pressed.is_empty() {
            "aucune".into()
        } else {
            pressed.join(", ")
        }
    );
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hue_primaries() {
        assert_eq!(hue(0.0), Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(hue(120.0), Rgb { r: 0, g: 255, b: 0 });
        assert_eq!(hue(240.0), Rgb { r: 0, g: 0, b: 255 });
        assert_eq!(hue(-120.0), hue(240.0));
    }

    #[test]
    fn rainbow_colours_only_layout_keys() {
        let f = rainbow(0.0);
        let lit = f.iter().filter(|c| **c != Rgb::default()).count();
        assert_eq!(lit, LAYOUT.keys.len());
    }

    #[test]
    fn depth_test_refuses_without_unverified() {
        assert_eq!(depth_test(1.0, false, false).unwrap(), ExitCode::from(2));
    }
}
