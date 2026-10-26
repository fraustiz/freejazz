//! `heatmap`: pressed keys heat up (blue to red) and cool down exponentially.

use serde_json::Value;

use crate::live::{Effect, Frame, FrameCtx, lerp, param_f32, rgb, scale};
use freejazz_protocol::live::FRAME_SLOTS;

/// Heat one press adds.
const PER_PRESS: f32 = 0.34;

pub struct Heatmap {
    heat: [f32; FRAME_SLOTS],
    /// Seconds for the heat to fall to 1/e.
    cooling: f32,
}

impl Default for Heatmap {
    fn default() -> Self {
        Self {
            heat: [0.0; FRAME_SLOTS],
            cooling: 3.0,
        }
    }
}

impl Heatmap {
    pub fn heat(&self, index: u8) -> f32 {
        self.heat[usize::from(index)]
    }
}

impl Effect for Heatmap {
    fn id(&self) -> &'static str {
        "heatmap"
    }

    fn configure(&mut self, params: &Value) {
        self.cooling = param_f32(params, "cooling", 3.0).max(0.1);
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        let k = (-ctx.dt / self.cooling).exp();
        for h in &mut self.heat {
            *h *= k;
        }
        for p in ctx.presses {
            if let Some(h) = self.heat.get_mut(usize::from(p.index)) {
                *h = (*h + PER_PRESS).min(1.0);
            }
        }
        for key in &ctx.layout.keys {
            let h = self.heat[usize::from(key.index)];
            // a little heat already shows (blue), a lot is red
            let c = lerp(rgb(0, 40, 255), rgb(255, 20, 0), h);
            out[usize::from(key.index)] = scale(c, (h * 3.0).min(1.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::testing::*;
    use crate::live::{BLACK, KeyPress};

    #[test]
    fn heatmap_cools_down() {
        let mut e = Heatmap::default();
        let presses = [KeyPress { index: 3, at: 0.0 }; 3];
        let mut out = black();
        e.frame(&ctx(0.0, 0.0, &presses, None, None), &mut out);
        let hot = out[3];
        assert!(hot.r > 200 && hot.b < 60, "{hot:?}"); // three presses: red
        assert_eq!(out[4], BLACK);

        let mut last = e.heat(3);
        let mut t = 0.0;
        for _ in 0..50 {
            t += 0.2;
            let mut out = black();
            e.frame(&ctx(t, 0.2, &[], None, None), &mut out);
            assert!(e.heat(3) < last, "never warms up");
            last = e.heat(3);
        }
        // 10 s later: cold again (exponential, 3 s time constant)
        assert!(last < 0.05, "{last}");
        let mut out = black();
        e.frame(&ctx(t, 0.0, &[], None, None), &mut out);
        assert!(out[3].r < 40, "{:?}", out[3]);
    }

    #[test]
    fn single_press_is_bluish() {
        let mut e = Heatmap::default();
        let mut out = black();
        e.frame(
            &ctx(0.0, 0.0, &[KeyPress { index: 3, at: 0.0 }], None, None),
            &mut out,
        );
        assert!(out[3].b > out[3].r);
    }
}
