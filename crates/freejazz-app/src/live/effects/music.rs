//! `music`: the stored custom design, pulsing with the overall sound level. The amplitude
//! byte of the stream stays 0 on this model, so the level modulates the colours.

use serde_json::Value;

use super::ACCENT;
use crate::live::{BLACK, Effect, Frame, FrameCtx, Needs, Rgb, scale};
use freejazz_protocol::live::FRAME_SLOTS;

/// Seconds for the pulse to fall back (attack is instant).
const RELEASE: f32 = 0.35;
/// The design never goes darker than this share.
const FLOOR: f32 = 0.25;

pub struct Music {
    design: Option<Box<[Rgb; FRAME_SLOTS]>>,
    level: f32,
}

impl Default for Music {
    fn default() -> Self {
        Self {
            design: None,
            level: 0.0,
        }
    }
}

impl Effect for Music {
    fn id(&self) -> &'static str {
        "music"
    }

    /// `{"design": [[r, g, b], ...]}` (one entry per key slot, as the custom colours
    /// are stored); without it every key takes the accent colour.
    fn configure(&mut self, params: &Value) {
        self.design = params.get("design").and_then(Value::as_array).map(|a| {
            let mut d = Box::new([BLACK; FRAME_SLOTS]);
            for (slot, v) in d.iter_mut().zip(a) {
                if let Some(c) = v.as_array().filter(|c| c.len() == 3) {
                    let n = |i: usize| c[i].as_u64().unwrap_or(0).min(255) as u8;
                    *slot = Rgb {
                        r: n(0),
                        g: n(1),
                        b: n(2),
                    };
                }
            }
            d
        });
    }

    fn needs(&self) -> Needs {
        Needs {
            audio: true,
            system: false,
        }
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        let now = ctx.audio.map_or(0.0, |s| (s.level() * 2.5).min(1.0));
        self.level = now.max(self.level - ctx.dt / RELEASE);
        let gain = FLOOR + (1.0 - FLOOR) * self.level;
        for k in &ctx.layout.keys {
            let i = usize::from(k.index);
            let base = self.design.as_ref().map_or(ACCENT, |d| d[i]);
            out[i] = scale(base, gain);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::Spectrum;
    use crate::live::testing::*;

    #[test]
    fn pulses_with_the_level_and_keeps_the_design() {
        let mut design = vec![serde_json::json!([0, 0, 0]); FRAME_SLOTS];
        design[3] = serde_json::json!([200, 100, 0]);
        let mut e = Music::default();
        e.configure(&serde_json::json!({ "design": design }));

        let quiet = Spectrum {
            bands: vec![0.0; 21],
        };
        let loud = Spectrum {
            bands: vec![0.9; 21],
        };
        let mut a = black();
        e.frame(&ctx(0.0, 0.04, &[], Some(&quiet), None), &mut a);
        let mut b = black();
        e.frame(&ctx(0.04, 0.04, &[], Some(&loud), None), &mut b);
        assert_eq!(
            b[3],
            Rgb {
                r: 200,
                g: 100,
                b: 0
            }
        );
        assert!(
            a[3].r > 0 && a[3].r < b[3].r / 2 + 10,
            "{:?} {:?}",
            a[3],
            b[3]
        );
        assert_eq!(b[4], BLACK); // the design's dark keys stay dark
        // and falls back
        let mut c = black();
        e.frame(&ctx(2.0, 2.0, &[], Some(&quiet), None), &mut c);
        assert_eq!(c[3], a[3]);
        assert!(e.needs().audio);
    }
}
