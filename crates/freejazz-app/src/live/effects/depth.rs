//! `depth`: a key's brightness follows how far it is pressed. The keyboard does not give
//! depths by default (`0x68` is unverified on this unit), so without them a press lights
//! the key and the light falls off quickly.

use serde_json::Value;

use super::ACCENT;
use crate::live::{Effect, Frame, FrameCtx, Rgb, param_color, scale};
use freejazz_protocol::live::FRAME_SLOTS;

/// Seconds for a press-only glow to fall to 1/e.
const FALL: f32 = 0.18;

pub struct Depth {
    color: Rgb,
    level: [f32; FRAME_SLOTS],
}

impl Default for Depth {
    fn default() -> Self {
        Self {
            color: ACCENT,
            level: [0.0; FRAME_SLOTS],
        }
    }
}

impl Effect for Depth {
    fn id(&self) -> &'static str {
        "depth"
    }

    fn configure(&mut self, params: &Value) {
        self.color = param_color(params, "color", ACCENT);
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        if let Some(d) = ctx.depths {
            self.level = *d;
        } else {
            let k = (-ctx.dt / FALL).exp();
            for l in &mut self.level {
                *l *= k;
            }
            for p in ctx.presses {
                if let Some(l) = self.level.get_mut(usize::from(p.index)) {
                    *l = 1.0;
                }
            }
        }
        for k in &ctx.layout.keys {
            let i = usize::from(k.index);
            out[i] = scale(self.color, self.level[i]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::KeyPress;
    use crate::live::testing::*;

    #[test]
    fn brightness_follows_depth_when_known() {
        let mut e = Depth::default();
        let mut d = [0.0f32; FRAME_SLOTS];
        d[3] = 0.5;
        d[4] = 1.0;
        let mut c = ctx(1.0, 0.04, &[], None, None);
        c.depths = Some(&d);
        let mut out = black();
        e.frame(&c, &mut out);
        assert!(out[3].b > 0 && out[3].b < out[4].b);
        assert_eq!(out[5], crate::live::BLACK);
    }

    #[test]
    fn falls_back_to_presses() {
        let mut e = Depth::default();
        let press = [KeyPress { index: 3, at: 1.0 }];
        let mut out = black();
        e.frame(&ctx(1.0, 0.04, &press, None, None), &mut out);
        let first = out[3];
        assert_eq!(first, ACCENT);
        let mut out = black();
        e.frame(&ctx(1.5, 0.5, &[], None, None), &mut out);
        assert!(out[3].b < first.b / 4, "{:?}", out[3]);
    }
}
