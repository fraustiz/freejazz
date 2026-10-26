//! Building blocks of custom effects: `solid`, `breathing`, `wave`, `scrolling_gradient`,
//! `stars`, `reactive`. Each paints the whole board; a layer keeps only its keys.

use serde_json::Value;

use super::ACCENT;
use crate::live::{Effect, Frame, FrameCtx, Rgb, center, hsv, lerp, param_color, param_f32, scale};
use freejazz_protocol::live::FRAME_SLOTS;

const TAU: f32 = std::f32::consts::TAU;

#[derive(Default)]
pub struct Solid {
    color: Option<Rgb>,
}

impl Effect for Solid {
    fn id(&self) -> &'static str {
        "solid"
    }

    fn configure(&mut self, params: &Value) {
        self.color = Some(param_color(params, "color", ACCENT));
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        let c = self.color.unwrap_or(ACCENT);
        for k in &ctx.layout.keys {
            out[usize::from(k.index)] = c;
        }
    }
}

pub struct Breathing {
    color: Rgb,
    /// Seconds per breath.
    period: f32,
}

impl Default for Breathing {
    fn default() -> Self {
        Self {
            color: ACCENT,
            period: 4.0,
        }
    }
}

impl Effect for Breathing {
    fn id(&self) -> &'static str {
        "breathing"
    }

    fn configure(&mut self, params: &Value) {
        self.color = param_color(params, "color", ACCENT);
        self.period = param_f32(params, "period", 4.0).max(0.2);
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        let v = 0.5 - 0.5 * (TAU * ctx.t / self.period).cos();
        let c = scale(self.color, v);
        for k in &ctx.layout.keys {
            out[usize::from(k.index)] = c;
        }
    }
}

/// A rainbow travelling left to right.
pub struct Wave {
    /// Turns of hue per second.
    speed: f32,
    /// Turns of hue across the board.
    spread: f32,
}

impl Default for Wave {
    fn default() -> Self {
        Self {
            speed: 0.15,
            spread: 1.0,
        }
    }
}

impl Effect for Wave {
    fn id(&self) -> &'static str {
        "wave"
    }

    fn configure(&mut self, params: &Value) {
        self.speed = param_f32(params, "speed", 0.15);
        self.spread = param_f32(params, "spread", 1.0);
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        let (w, _) = ctx.layout.size();
        for k in &ctx.layout.keys {
            let (x, _) = center(k);
            out[usize::from(k.index)] = hsv(x / w * self.spread - ctx.t * self.speed, 1.0, 1.0);
        }
    }
}

/// Two colours sliding across the board.
pub struct ScrollingGradient {
    a: Rgb,
    b: Rgb,
    /// Board widths per second.
    speed: f32,
}

impl Default for ScrollingGradient {
    fn default() -> Self {
        Self {
            a: ACCENT,
            b: Rgb {
                r: 0,
                g: 200,
                b: 255,
            },
            speed: 0.2,
        }
    }
}

impl Effect for ScrollingGradient {
    fn id(&self) -> &'static str {
        "scrolling_gradient"
    }

    fn configure(&mut self, params: &Value) {
        let d = Self::default();
        self.a = param_color(params, "color", d.a);
        self.b = param_color(params, "color2", d.b);
        self.speed = param_f32(params, "speed", 0.2);
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        let (w, _) = ctx.layout.size();
        for k in &ctx.layout.keys {
            let (x, _) = center(k);
            let m = 0.5 + 0.5 * (TAU * (x / w - ctx.t * self.speed)).sin();
            out[usize::from(k.index)] = lerp(self.a, self.b, m);
        }
    }
}

/// Keys twinkling at their own pace.
pub struct Stars {
    color: Rgb,
}

impl Default for Stars {
    fn default() -> Self {
        Self {
            color: Rgb {
                r: 255,
                g: 244,
                b: 214,
            },
        }
    }
}

/// A stable pseudo-random number in 0..1 for a key and a salt.
fn hash(index: u8, salt: u32) -> f32 {
    let mut x = u32::from(index)
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add(salt.wrapping_mul(0x85EB_CA6B));
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

impl Effect for Stars {
    fn id(&self) -> &'static str {
        "stars"
    }

    fn configure(&mut self, params: &Value) {
        self.color = param_color(params, "color", Self::default().color);
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        for k in &ctx.layout.keys {
            let phase = hash(k.index, 1);
            let rate = 0.15 + 0.35 * hash(k.index, 2);
            let s = 0.5 + 0.5 * (TAU * (ctx.t * rate + phase)).sin();
            out[usize::from(k.index)] = scale(self.color, s.powi(10));
        }
    }
}

/// Keys light when pressed and fade.
pub struct Reactive {
    color: Rgb,
    /// Seconds to fall to 1/e.
    fade: f32,
    level: [f32; FRAME_SLOTS],
}

impl Default for Reactive {
    fn default() -> Self {
        Self {
            color: ACCENT,
            fade: 0.5,
            level: [0.0; FRAME_SLOTS],
        }
    }
}

impl Effect for Reactive {
    fn id(&self) -> &'static str {
        "reactive"
    }

    fn configure(&mut self, params: &Value) {
        self.color = param_color(params, "color", ACCENT);
        self.fade = param_f32(params, "fade", 0.5).max(0.05);
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        let k = (-ctx.dt / self.fade).exp();
        for l in &mut self.level {
            *l *= k;
        }
        for p in ctx.presses {
            if let Some(l) = self.level.get_mut(usize::from(p.index)) {
                *l = 1.0;
            }
        }
        for key in &ctx.layout.keys {
            let i = usize::from(key.index);
            out[i] = scale(self.color, self.level[i]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::testing::*;
    use crate::live::{BLACK, KeyPress, create};

    fn run(id: &str, t: f32) -> Frame {
        let mut e = create(id, &Value::Null).unwrap();
        let mut out = black();
        e.frame(&ctx(t, 0.04, &[], None, None), &mut out);
        out
    }

    #[test]
    fn blocks_paint_keys_only() {
        for id in ["solid", "breathing", "wave", "scrolling_gradient", "stars"] {
            let out = run(id, 1.7);
            assert_eq!(out[127], BLACK, "{id}");
        }
        assert_ne!(run("solid", 0.0)[3], BLACK);
        assert_ne!(run("wave", 0.0)[3], BLACK);
    }

    #[test]
    fn breathing_goes_dark_and_bright() {
        assert_eq!(run("breathing", 0.0)[3], BLACK);
        assert_eq!(run("breathing", 2.0)[3], ACCENT);
    }

    #[test]
    fn wave_moves() {
        assert_ne!(run("wave", 0.0)[3], run("wave", 1.0)[3]);
    }

    #[test]
    fn stars_twinkle_unevenly() {
        let a = run("stars", 3.0);
        let lit = a.iter().filter(|c| **c != BLACK).count();
        assert!(lit < 100 && a != run("stars", 9.0));
    }

    #[test]
    fn reactive_lights_pressed_key_then_fades() {
        let mut e = Reactive::default();
        let mut out = black();
        e.frame(
            &ctx(0.0, 0.04, &[KeyPress { index: 5, at: 0.0 }], None, None),
            &mut out,
        );
        assert_eq!(out[5], ACCENT);
        assert_eq!(out[6], BLACK);
        let mut out = black();
        e.frame(&ctx(2.0, 2.0, &[], None, None), &mut out);
        assert!(out[5].b < 10);
    }
}
