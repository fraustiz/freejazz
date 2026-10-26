//! Aperçu des effets lumineux intégrés du clavier, dessiné sur le clavier de l'appli.
//!
//! Les motifs sont relevés sur des vidéos du vrai clavier (téléphone, 16 images à 0,25 s,
//! vitesse 3, luminosité 5) ; le micrologiciel n'est pas documenté, c'est donc une
//! reconstitution à l'oeil. Le temps de chaque motif est mis à l'échelle de la vitesse
//! (1 plus lent, 5 plus rapide, à peu près linéaire, 3 = la vidéo). Les effets réactifs
//! (2, 3, 13, 14, 15) sont pilotés par des appuis simulés, à intervalle régulier, sur des
//! touches variées de la zone principale. « Une couleur » = la couleur choisie ; sinon
//! (multicolore) le spectre. Les modes 6 et 8 n'ont pas de choix de couleur.
//!
//! | Mode | Comportement observé sur le clavier |
//! |------|--------------------------------------|
//! | 1 Statique | arc-en-ciel fixe : rouge à gauche, vert, bleu au centre, retour au vert/rouge puis violet vers le pavé numérique ; une couleur : uniforme |
//! | 2 Point lumineux | l'appui allume la touche seule (couleur au hasard), qui s'éteint en ~1 s ; une couleur : toujours la même |
//! | 3 Extinction | tout est allumé (couleur fixe au hasard par touche) ; l'appui éteint la touche, qui se rallume en ~1,2 s |
//! | 4 Étoiles | chaque touche scintille à son rythme (~1,6 s), nouvelle couleur au hasard à chaque scintillement ; une couleur : variations de luminosité |
//! | 5 Neige | touches éparses (~40 %) qui s'allument d'un coup puis s'éteignent en ~1 s, couleur au hasard ; le reste est sombre |
//! | 6 Fleurs | toutes les touches allumées, chacune dérive en continu dans le spectre (~0,25 tour/s), couleurs décalées entre voisines |
//! | 7 Respiration | tout le clavier s'éteint et se rallume en bloc (~5 s) ; multicolore : nouvelle teinte à chaque respiration (rouge, jaune...) |
//! | 8 Cycle spectral | tout le clavier d'une seule couleur qui parcourt le spectre (jaune, rouge, violet, bleu...), ~6,8 s par tour |
//! | 9 Fontaine | ondes de teinte circulaires depuis le centre du clavier qui s'étendent ; une couleur : anneaux de luminosité |
//! | 10 Multicolore | bandes horizontales (une teinte par rangée) qui défilent vers le bas (direction 3) ou le haut, ~4 s par cycle ; une couleur : bandes de luminosité |
//! | 11 Suivre le courant | bandes verticales de teintes qui défilent horizontalement (dir. 0 : vers la droite), ~3 s par cycle ; une couleur : bandes sombres |
//! | 12 Virage sinueux | comme 11 mais les bandes sont plutôt horizontales et ondulent (frontière en S) en se décalant vers la droite (dir. 0) |
//! | 13 Prêt à déclencher | l'appui allume toute la rangée de la touche, arc-en-ciel le long de la rangée, extinction en ~0,9 s |
//! | 14 Un coup, deux oiseaux | l'appui allume la colonne de la touche et la colonne symétrique (miroir autour du centre), arc-en-ciel vertical, ~0,9 s |
//! | 15 Ondes de diffusion | l'appui lance une large onde (bande de ~9 touches) qui gagne toute la surface en ~2 s, touches multicolores |
//! | 16 Flux continu | un trait d'~9 touches court le long d'une rangée (dir. 0 : vers la droite) en ~1,4 s, puis passe à une autre rangée ; arc-en-ciel le long du trait |
//! | 17 Montagnes | touches allumées sauf des creux en V (zigzag à 2 pics) qui glissent lentement (~4 s par motif) ; couleurs pastel par touche |
//! | 18 Vent et pluie | traînées diagonales (haut-gauche vers bas-droite) allumées/éteintes qui se déplacent ; couleurs au hasard par touche |
//! | 19 Aller-retour | bande large qui fait l'aller-retour en x (~3 s), décalée d'une rangée à l'autre ; multicolore : arc-en-ciel de fond atténué hors bande |

use std::f32::consts::TAU;
use std::time::{Duration, Instant};

use freejazz_protocol::blocks::{LedEffect, Rgb};
use freejazz_protocol::live::FRAME_SLOTS;

use crate::keyboard::Layout;
use crate::live::{BLACK, Frame, hsv, max_rgb, rgb, scale};

/// What the animation depends on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    pub mode: u8,
    /// 1..=5
    pub speed: u8,
    /// 0 right, 1 left, 2 up, 3 down.
    pub direction: u8,
    pub color: Rgb,
    /// Colour mode 1: every colour of the spectrum.
    pub multicolor: bool,
    /// 0..=6
    pub brightness: u8,
}

impl Look {
    pub fn of(led: &LedEffect, color: Rgb, color_mode: u8) -> Self {
        Self {
            mode: led.mode,
            speed: led.speed.clamp(1, 5),
            direction: led.direction,
            color,
            multicolor: color_mode == 1,
            brightness: led.brightness.min(6),
        }
    }

    /// Time scale: 1 at speed 3 (what the footage shows), 0.6 at 1, 1.4 at 5.
    fn rate(&self) -> f32 {
        0.4 + 0.2 * f32::from(self.speed)
    }

    /// A colour of the spectrum (multicolour) or the chosen colour.
    fn tint(&self, hue: f32) -> Rgb {
        if self.multicolor {
            hsv(hue, 1.0, 1.0)
        } else {
            self.color
        }
    }

    /// A moving colour wave at `phase` (turns): hue in multicolour, brightness
    /// (0.15..1) with the chosen colour otherwise.
    fn wave(&self, phase: f32) -> Rgb {
        if self.multicolor {
            hsv(phase, 1.0, 1.0)
        } else {
            scale(self.color, 0.575 + 0.425 * (TAU * phase).cos())
        }
    }
}

fn noise(a: u32, b: u32) -> f32 {
    let mut x = a
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(b.wrapping_mul(0x85EB_CA6B));
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x & 0xFFFF) as f32 / 65535.0
}

/// 0..1 sawtooth.
fn fract(v: f32) -> f32 {
    v - v.floor()
}

/// 0..1 triangle of period 1 (0 at the integers).
fn tri(v: f32) -> f32 {
    1.0 - (2.0 * fract(v) - 1.0).abs()
}

fn smooth(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    v * v * (3.0 - 2.0 * v)
}

/// Piecewise-linear interpolation through `(position, value)` stops.
fn lerp_stops(stops: &[(f32, f32)], x: f32) -> f32 {
    let x = x.clamp(stops[0].0, stops[stops.len() - 1].0);
    for w in stops.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0).max(1e-6);
        }
    }
    stops[stops.len() - 1].1
}

/// The fixed rainbow of mode 1 (and the backdrop of 19) along the width, 0..1 → turns.
fn static_hue(u: f32, row: f32) -> f32 {
    lerp_stops(
        &[
            (0.0, 0.0),
            (0.12, 0.12),
            (0.25, 0.33),
            (0.5, 0.6),
            (0.7, 0.3),
            (0.8, 0.0),
            (0.9, -0.1),
            (1.0, -0.2),
        ],
        u,
    ) - 0.03 * row
}

/// One simulated key press of the reactive effects.
struct Press {
    n: u32,
    /// Seconds (effect time) since the press.
    age: f32,
    x: f32,
    y: f32,
}

/// The simulated presses still visible at effect time `t`: one every `gap` seconds, on a
/// key of the main block picked by hash, each shown for `life` seconds.
fn presses(t: f32, gap: f32, life: f32, layout: &Layout) -> Vec<Press> {
    let main: Vec<_> = layout
        .keys
        .iter()
        .filter(|k| k.y >= 1.0 && k.y < 5.0 && k.x < 14.0)
        .collect();
    if main.is_empty() {
        return Vec::new();
    }
    let last = (t / gap).floor().max(0.0) as u32;
    let span = (life / gap).ceil() as u32;
    (last.saturating_sub(span)..=last)
        .filter_map(|n| {
            let age = t - n as f32 * gap;
            if !(0.0..life).contains(&age) {
                return None;
            }
            let key = main[(noise(n, 77) * main.len() as f32) as usize % main.len()];
            let (x, y) = crate::live::center(key);
            Some(Press { n, age, x, y })
        })
        .collect()
}

fn on_key(p: &Press, x: f32, y: f32) -> bool {
    (p.x - x).abs() < 0.1 && (p.y - y).abs() < 0.1
}

/// Frame of the effect at time `t` seconds.
pub fn frame(look: &Look, t: f32, layout: &Layout) -> Frame {
    let mut out = [BLACK; FRAME_SLOTS];
    if look.mode == 0 {
        return out;
    }
    let (w, h) = layout.size();
    let (w, h) = (w.max(1.0), h.max(1.0));
    let te = t * look.rate();
    let mirror = look.direction == 1;
    let (gap, life) = match look.mode {
        2 => (0.7, 1.1),
        3 => (0.5, 1.2),
        13 | 14 => (0.8, 0.9),
        15 => (1.5, 2.2),
        _ => (1.0, 0.0),
    };
    let taps = if life > 0.0 {
        presses(te, gap, life, layout)
    } else {
        Vec::new()
    };
    for key in &layout.keys {
        let i = u32::from(key.index);
        let (x, y) = crate::live::center(key);
        let u = x / w;
        let row = key.y;
        let c = match look.mode {
            1 => {
                if look.multicolor {
                    hsv(static_hue(u, row), 1.0, 1.0)
                } else {
                    look.color
                }
            }
            2 => taps
                .iter()
                .filter(|p| on_key(p, x, y))
                .map(|p| {
                    scale(
                        look.tint(noise(p.n, 5)),
                        (1.0 - p.age / life).max(0.0).powf(1.4),
                    )
                })
                .fold(BLACK, max_rgb),
            3 => {
                let hit = taps
                    .iter()
                    .filter(|p| on_key(p, x, y))
                    .map(|p| smooth(p.age / life))
                    .fold(1.0f32, f32::min);
                let base = if look.multicolor {
                    hsv(noise(i, 1), 0.85, 1.0)
                } else {
                    look.color
                };
                scale(base, hit)
            }
            4 => {
                let p = te / 1.6 + noise(i, 2) * 3.0;
                let colour = if look.multicolor {
                    hsv(noise(i, p.floor() as u32 + 100), 0.9, 1.0)
                } else {
                    look.color
                };
                scale(
                    colour,
                    0.2 + 0.8 * (std::f32::consts::PI * fract(p)).sin().powi(2),
                )
            }
            5 => {
                let p = te / 1.3 + noise(i, 2) * 5.0;
                let cycle = p.floor() as u32;
                if noise(i, cycle + 200) < 0.4 {
                    scale(
                        look.tint(noise(i, cycle + 300)),
                        (1.0 - fract(p)).powf(1.5).max(0.04),
                    )
                } else {
                    BLACK
                }
            }
            6 => hsv(noise(i, 6) + te * 0.25, 1.0, 1.0),
            7 => {
                let p = te / 5.0;
                let b = ((TAU * p).cos() * 0.5 + 0.7).clamp(0.0, 1.0);
                let hue = (p + 0.5).floor() * 0.15;
                scale(look.tint(hue), smooth(b))
            }
            8 => hsv(0.2 - te * 0.147, 1.0, 1.0),
            9 => {
                let (dx, dy) = ((x - w * 0.48) * 0.5, y - h * 0.45);
                look.wave(dx.hypot(dy) * 0.15 - te * 0.2)
            }
            10 => {
                let sign = if look.direction == 2 { -1.0 } else { 1.0 };
                look.wave(0.75 - 0.145 * row + sign * 0.24 * te)
            }
            11 => {
                let sign = if mirror { -1.0 } else { 1.0 };
                look.wave(0.75 - 0.6 * u + sign * 0.33 * te)
            }
            12 => {
                let (u, sign) = if mirror { (1.0 - u, -1.0) } else { (u, 1.0) };
                let bend = 1.4 * (TAU * (0.55 * u - 0.3 * te)).sin();
                look.wave(0.75 + sign * 0.33 * te - 0.12 * (row + bend))
            }
            13 => {
                let a = taps
                    .iter()
                    .filter(|p| (p.y - y).abs() < 0.5)
                    .map(|p| (1.0 - p.age / life).max(0.0).powf(1.2))
                    .fold(0.0f32, f32::max);
                scale(look.tint(u * 1.1), a)
            }
            14 => {
                let a = taps
                    .iter()
                    .filter(|p| (p.x - x).abs() < 0.6 || (w - p.x - x).abs() < 0.6)
                    .map(|p| (1.0 - p.age / life).max(0.0).powf(1.2))
                    .fold(0.0f32, f32::max);
                scale(look.tint(row * 0.17 + 0.05), a)
            }
            15 => {
                let a = taps
                    .iter()
                    .map(|p| {
                        let d = ((x - p.x) * 0.6).hypot(y - p.y);
                        let behind = p.age * 9.0 - d;
                        if behind < 0.0 {
                            0.0
                        } else {
                            (1.0 - behind / 9.0).max(0.0) * (1.0 - p.age / life)
                        }
                    })
                    .fold(0.0f32, f32::max);
                scale(look.tint(u * 1.3 + noise(i, 4) * 0.3), a)
            }
            16 => {
                let p = te / 1.4 + 0.3;
                let length = 9.0;
                let head = -length + fract(p) * (w + length);
                let xx = if mirror { w - x } else { x };
                let wanted = (noise(p.floor() as u32, 21) * 6.0).floor();
                if (row - wanted).abs() < 0.5 && xx <= head && xx >= head - length {
                    let tail = (xx - (head - length)) / length;
                    scale(look.tint(xx / w * 1.1), 0.35 + 0.65 * tail)
                } else {
                    BLACK
                }
            }
            17 => {
                let depth = tri(2.0 * u - 0.25 * te) * 3.4 - 0.9;
                let lit = smooth((y - depth) * 1.2 + 0.5);
                let base = if look.multicolor {
                    hsv(noise(i, 3) + 0.1 * te, 0.8, 1.0)
                } else {
                    look.color
                };
                scale(base, 0.12 + 0.88 * lit)
            }
            18 => {
                let ux = if mirror { w - x } else { x };
                let l = (y - 0.5 * ux) / 0.9 + te * 0.8;
                let lit = if noise(l.floor() as i32 as u32, 31) < 0.55 {
                    0.35 + 0.65 * tri(l)
                } else {
                    0.0
                };
                let colour = if look.multicolor {
                    hsv(noise(i, (te * 1.2) as u32 + 400), 0.9, 1.0)
                } else {
                    look.color
                };
                scale(colour, lit.max(0.05))
            }
            19 => {
                let centre = 0.5 + 0.5 * (TAU * (te / 3.2 + row * 0.13)).sin();
                let band = smooth(1.0 - (u - centre).abs() / 0.24);
                if look.multicolor {
                    scale(hsv(static_hue(u, row), 1.0, 1.0), 0.7 + 0.3 * band)
                } else {
                    scale(look.color, 0.08 + 0.92 * band)
                }
            }
            _ => scale(look.color, 0.5),
        };
        // brightness 0..6 (the keyboard's own steps)
        out[usize::from(key.index)] = scale(c, 0.15 + 0.85 * f32::from(look.brightness) / 6.0);
    }
    out
}

/// The animation running in the keyboard view.
pub struct Preview {
    started: Instant,
    /// Debug aids (environment): `FREEJAZZ_PREVIEW_TIME=<s>` freezes the time and
    /// `FREEJAZZ_PREVIEW_FX=<mode>[,<speed>[,<direction>[,mono]]]` forces the effect
    /// (red when `mono`), for deterministic screenshots.
    fixed_time: Option<f32>,
    forced: Option<Look>,
}

impl Default for Preview {
    fn default() -> Self {
        let fixed_time = std::env::var("FREEJAZZ_PREVIEW_TIME")
            .ok()
            .and_then(|v| v.trim().parse().ok());
        let forced = std::env::var("FREEJAZZ_PREVIEW_FX").ok().and_then(|v| {
            let mut it = v.split(',');
            let mode = it.next()?.trim().parse().ok()?;
            let speed = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(3);
            let direction = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            let mono = it.next().is_some_and(|s| s.trim() == "mono");
            Some(Look {
                mode,
                speed,
                direction,
                color: rgb(255, 0, 0),
                multicolor: !mono,
                brightness: 5,
            })
        });
        Self {
            started: Instant::now(),
            fixed_time,
            forced,
        }
    }
}

impl Preview {
    /// Frame for now, and the delay to the next one (25 images a second).
    pub fn next(&self, look: &Look) -> (Frame, Duration) {
        let t = self
            .fixed_time
            .unwrap_or_else(|| self.started.elapsed().as_secs_f32());
        (
            frame(self.forced.as_ref().unwrap_or(look), t, Layout::ak980max()),
            Duration::from_millis(40),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(mode: u8) -> Look {
        Look {
            mode,
            speed: 3,
            direction: 0,
            color: rgb(255, 0, 0),
            multicolor: false,
            brightness: 6,
        }
    }

    #[test]
    fn every_mode_lights_something_and_moves() {
        let layout = Layout::ak980max();
        for mode in 1..=19u8 {
            for multicolor in [false, true] {
                let mut l = look(mode);
                l.multicolor = multicolor;
                let any = (0..40).any(|n| {
                    frame(&l, n as f32 * 0.37, layout)
                        .iter()
                        .any(|c| *c != BLACK)
                });
                assert!(any, "mode {mode} never lights a key");
            }
        }
        assert_ne!(frame(&look(8), 0.0, layout), frame(&look(8), 3.0, layout));
    }

    #[test]
    fn off_is_black_and_slots_without_a_key_stay_black() {
        let layout = Layout::ak980max();
        assert!(frame(&look(0), 1.0, layout).iter().all(|c| *c == BLACK));
        assert_eq!(frame(&look(1), 1.0, layout)[127], BLACK);
        assert_ne!(frame(&look(1), 1.0, layout)[1], BLACK);
    }

    #[test]
    fn the_travel_direction_changes_the_picture() {
        let layout = Layout::ak980max();
        let mut l = look(10);
        l.multicolor = true;
        l.direction = 3;
        let down = frame(&l, 1.0, layout);
        l.direction = 2;
        assert_ne!(down, frame(&l, 1.0, layout));
        let mut l = look(11);
        l.multicolor = true;
        let right = frame(&l, 1.0, layout);
        l.direction = 1;
        assert_ne!(right, frame(&l, 1.0, layout));
    }

    #[test]
    fn a_faster_speed_moves_further() {
        let layout = Layout::ak980max();
        let mut slow = look(8);
        slow.speed = 1;
        let mut fast = look(8);
        fast.speed = 5;
        assert_ne!(frame(&slow, 2.0, layout), frame(&fast, 2.0, layout));
    }

    #[test]
    fn a_press_lights_its_row_then_fades() {
        let layout = Layout::ak980max();
        let at = |t: f32| {
            frame(&look(13), t, layout)
                .iter()
                .filter(|c| **c != BLACK)
                .count()
        };
        assert!(at(0.1) > 5);
        assert!(at(0.6) <= at(0.1));
    }
}
