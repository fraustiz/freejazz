//! A rough animation of the keyboard's built-in effects, for the keyboard view. The
//! real firmware patterns are not documented, so this is an approximation per mode,
//! speed, direction and colour: enough to tell the effects apart, not a copy.

use std::f32::consts::TAU;
use std::time::{Duration, Instant};

use freejazz_protocol::blocks::{LedEffect, Rgb};
use freejazz_protocol::live::FRAME_SLOTS;

use crate::keyboard::Layout;
use crate::live::{BLACK, Frame, hsv, scale};

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

    fn pace(&self) -> f32 {
        0.4 + 0.3 * f32::from(self.speed)
    }

    /// The effect colour at a position along the pattern (`hue` in turns).
    fn tint(&self, hue: f32) -> Rgb {
        if self.multicolor {
            hsv(hue, 1.0, 1.0)
        } else {
            self.color
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
    (x & 0xFFFF) as f32 / 65535.0
}

/// 0..1 sawtooth.
fn fract(v: f32) -> f32 {
    v - v.floor()
}

/// 1 at `phase == 0`, falling to 0 over `width` (of a turn).
fn pulse(phase: f32, width: f32) -> f32 {
    (1.0 - fract(phase) / width).clamp(0.0, 1.0)
}

/// Frame of the effect at time `t` seconds.
pub fn frame(look: &Look, t: f32, layout: &Layout) -> Frame {
    let mut out = [BLACK; FRAME_SLOTS];
    if look.mode == 0 {
        return out;
    }
    let (w, h) = layout.size();
    let (w, h) = (w.max(1.0), h.max(1.0));
    let k = look.pace();
    for key in &layout.keys {
        let i = u32::from(key.index);
        let (x, y) = crate::live::center(key);
        let (u, v) = (x / w, y / h);
        // position along the travel direction, 0..1
        let along = match look.direction {
            1 => 1.0 - u,
            2 => 1.0 - v,
            3 => v,
            _ => u,
        };
        let base = look.tint(u);
        let c = match look.mode {
            1 => look.tint(u),
            // a point lights at random, then fades
            2 => scale(
                look.tint(noise(i, 1)),
                pulse(t * k * 0.25 + noise(i, 2), 0.3),
            ),
            // all lit, one goes dark at a time
            3 => scale(
                look.tint(noise(i, 1)),
                1.0 - pulse(t * k * 0.25 + noise(i, 2), 0.2),
            ),
            4 => scale(
                if look.multicolor {
                    hsv(noise(i, 3), 0.3, 1.0)
                } else {
                    look.color
                },
                (0.5 + 0.5 * (TAU * (t * k * 0.2 + noise(i, 4))).sin()).powi(3),
            ),
            // snow falls down each column
            5 => scale(
                if look.multicolor {
                    hsv(0.55, 0.2, 1.0)
                } else {
                    look.color
                },
                pulse(t * k * 0.35 + noise((u * 40.0) as u32, 5) - v * 0.6, 0.35),
            ),
            6 => scale(
                hsv(noise(i, 6), 0.7, 1.0),
                (0.5 + 0.5 * (TAU * (t * k * 0.1 + noise(i, 7))).sin()).powi(2),
            ),
            7 => scale(base, 0.5 - 0.5 * (TAU * t * k * 0.15).cos()),
            8 => hsv(t * k * 0.06, 1.0, 1.0),
            // fountain: colour shoots up from the bottom centre
            9 => {
                let d = ((u - 0.5).abs() * 1.4 + (1.0 - v)).min(2.0);
                scale(look.tint(d * 0.3), pulse(d * 0.5 - t * k * 0.25, 0.5))
            }
            10 => look.tint(along * 1.5 - t * k * 0.1),
            11 => scale(
                look.tint(along),
                0.5 + 0.5 * (TAU * (along * 1.2 - t * k * 0.15)).sin(),
            ),
            12 => {
                let path = 0.5 + 0.3 * (TAU * (u * 1.5 - t * k * 0.1)).sin();
                scale(look.tint(u), (1.0 - (v - path).abs() * 3.0).clamp(0.0, 1.0))
            }
            13 => scale(base, pulse(t * k * 0.3 + noise(i, 8), 0.12)),
            14 => {
                let a = (fract(t * k * 0.12) - u).abs();
                let b = (fract(t * k * 0.12 + 0.5) - u).abs();
                scale(look.tint(u), (1.0 - a.min(b) * 8.0).clamp(0.0, 1.0))
            }
            15 => {
                // rings spreading from a centre that changes every few seconds
                let n = ((t * k * 0.2) as u32) % 7;
                let (cx, cy) = (noise(n, 9) * w, noise(n, 10) * h);
                let d = (x - cx).hypot(y - cy) / w;
                scale(
                    base,
                    (1.0 - (d - fract(t * k * 0.2) * 0.6).abs() * 9.0).clamp(0.0, 1.0),
                )
            }
            16 => scale(
                look.tint(along),
                0.35 + 0.65 * fract(along * 2.0 - t * k * 0.15),
            ),
            17 => {
                let ridge = 0.5 + 0.25 * (TAU * (u * 2.0 + t * k * 0.04)).sin();
                scale(look.tint(v + t * 0.02), if v > ridge { 1.0 } else { 0.35 })
            }
            // diagonal streaks falling
            18 => scale(
                base,
                pulse(
                    u * 0.4 + v - t * k * 0.3 + noise((u * 25.0) as u32, 11),
                    0.3,
                ),
            ),
            19 => {
                let p = 0.5 - 0.5 * (TAU * t * k * 0.08).cos();
                scale(look.tint(p), (1.0 - (u - p).abs() * 6.0).clamp(0.0, 1.0))
            }
            _ => scale(base, 0.5),
        };
        // brightness 0..6 (the keyboard's own steps)
        out[usize::from(key.index)] = scale(c, 0.15 + 0.85 * f32::from(look.brightness) / 6.0);
    }
    out
}

/// The animation running in the keyboard view.
pub struct Preview {
    started: Instant,
}

impl Default for Preview {
    fn default() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl Preview {
    /// Frame for now, and the delay to the next one (25 images a second).
    pub fn next(&self, look: &Look) -> (Frame, Duration) {
        (
            frame(
                look,
                self.started.elapsed().as_secs_f32(),
                Layout::ak980max(),
            ),
            Duration::from_millis(40),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::rgb;

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
            let any = (0..40).any(|n| {
                frame(&look(mode), n as f32 * 0.37, layout)
                    .iter()
                    .any(|c| *c != BLACK)
            });
            assert!(any, "mode {mode} never lights a key");
        }
        assert_ne!(frame(&look(8), 0.0, layout), frame(&look(8), 3.0, layout));
    }

    #[test]
    fn off_is_black_and_slots_without_a_key_stay_black() {
        let layout = Layout::ak980max();
        assert!(frame(&look(0), 1.0, layout).iter().all(|c| *c == BLACK));
        assert_eq!(frame(&look(1), 1.0, layout)[127], BLACK);
        assert_eq!(frame(&look(1), 1.0, layout)[1], rgb(255, 0, 0));
    }

    #[test]
    fn the_travel_direction_changes_the_picture() {
        let layout = Layout::ak980max();
        let mut l = look(10);
        l.multicolor = true;
        let right = frame(&l, 1.0, layout);
        l.direction = 1;
        assert_ne!(right, frame(&l, 1.0, layout));
    }
}
