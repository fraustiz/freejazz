//! `ripple`: a ring spreads from each pressed key.

use serde_json::Value;

use super::ACCENT;
use crate::live::{Effect, Frame, FrameCtx, Rgb, center, max_rgb, param_color, param_f32, scale};

/// Seconds a ripple lives.
const LIFE: f32 = 1.4;

struct Wave {
    x: f32,
    y: f32,
    t0: f32,
}

pub struct Ripple {
    color: Rgb,
    /// Key units per second.
    speed: f32,
    /// Ring thickness in key units.
    width: f32,
    waves: Vec<Wave>,
}

impl Default for Ripple {
    fn default() -> Self {
        Self {
            color: ACCENT,
            speed: 8.0,
            width: 0.9,
            waves: Vec::new(),
        }
    }
}

impl Effect for Ripple {
    fn id(&self) -> &'static str {
        "ripple"
    }

    fn configure(&mut self, params: &Value) {
        self.color = param_color(params, "color", ACCENT);
        self.speed = param_f32(params, "speed", 8.0).max(0.1);
        self.width = param_f32(params, "width", 0.9).max(0.1);
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        for p in ctx.presses {
            if let Some(k) = ctx.layout.key(p.index) {
                let (x, y) = center(k);
                self.waves.push(Wave { x, y, t0: p.at });
            }
        }
        self.waves.retain(|w| ctx.t - w.t0 < LIFE);
        for k in &ctx.layout.keys {
            let (kx, ky) = center(k);
            let mut c = crate::live::BLACK;
            for w in &self.waves {
                let age = ctx.t - w.t0;
                let radius = self.speed * age;
                let d = ((kx - w.x).powi(2) + (ky - w.y).powi(2)).sqrt();
                let ring = 1.0 - (d - radius).abs() / self.width;
                if ring > 0.0 {
                    c = max_rgb(c, scale(self.color, ring * (1.0 - age / LIFE)));
                }
            }
            out[usize::from(k.index)] = c;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard::Layout;
    use crate::live::testing::*;
    use crate::live::{BLACK, KeyPress};

    #[test]
    fn ripple_lights_neighbours_first() {
        let layout = Layout::ak980max();
        let h = layout.keys.iter().find(|k| k.label == "H").unwrap();
        let (hx, hy) = center(h);
        let dist = |k: &crate::keyboard::layout::KeyDef| {
            let (x, y) = center(k);
            ((x - hx).powi(2) + (y - hy).powi(2)).sqrt()
        };
        let near = layout
            .keys
            .iter()
            .filter(|k| k.index != h.index && dist(k) < 1.3)
            .map(|k| usize::from(k.index))
            .collect::<Vec<_>>();
        let far = layout
            .keys
            .iter()
            .find(|k| (4.5..5.5).contains(&dist(k)))
            .map(|k| usize::from(k.index))
            .unwrap();
        assert!(!near.is_empty());

        let mut e = Ripple::default();
        let press = [KeyPress {
            index: h.index,
            at: 10.0,
        }];
        let mut out = black();
        e.frame(&ctx(10.0, 0.0, &press, None, None), &mut out);

        // 0.12 s later the ring (8 units/s) is about one key out: neighbours lit, far dark
        let mut out = black();
        e.frame(&ctx(10.12, 0.12, &[], None, None), &mut out);
        assert!(near.iter().any(|&i| out[i] != BLACK), "{near:?}");
        assert_eq!(out[far], BLACK);

        // later the ring reached the far key and left the neighbours
        let mut out = black();
        e.frame(&ctx(10.6, 0.48, &[], None, None), &mut out);
        assert_ne!(out[far], BLACK);
        assert!(near.iter().all(|&i| out[i] == BLACK));
    }

    #[test]
    fn ripples_die() {
        let mut e = Ripple::default();
        let press = [KeyPress { index: 20, at: 0.0 }];
        let mut out = black();
        e.frame(&ctx(0.0, 0.0, &press, None, None), &mut out);
        let mut out = black();
        e.frame(&ctx(3.0, 3.0, &[], None, None), &mut out);
        assert!(out.iter().all(|c| *c == BLACK));
        assert!(e.waves.is_empty());
    }
}
