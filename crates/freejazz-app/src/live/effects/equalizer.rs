//! `equalizer`: the keyboard's columns are the 21 spectrum bands, filling from the bottom
//! row up.

use crate::live::audio::BANDS;
use crate::live::{Effect, Frame, FrameCtx, Needs, center, lerp, rgb};

#[derive(Default)]
pub struct Equalizer;

impl Effect for Equalizer {
    fn id(&self) -> &'static str {
        "equalizer"
    }

    fn needs(&self) -> Needs {
        Needs {
            audio: true,
            system: false,
        }
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        let Some(audio) = ctx.audio else { return };
        let (w, _) = ctx.layout.size();
        let centres: Vec<f32> = ctx.layout.keys.iter().map(|k| center(k).1).collect();
        let top = centres.iter().copied().fold(f32::MAX, f32::min);
        let bottom = centres.iter().copied().fold(f32::MIN, f32::max);
        for (k, &cy) in ctx.layout.keys.iter().zip(&centres) {
            let (cx, _) = center(k);
            let band = ((cx / w * BANDS as f32) as usize).min(BANDS - 1);
            let level = audio.bands.get(band).copied().unwrap_or(0.0);
            // 0 on the bottom row, 1 on the top one
            let height = (bottom - cy) / (bottom - top).max(0.001);
            if level > 0.02 && height <= level {
                out[usize::from(k.index)] = if height < 0.5 {
                    lerp(rgb(0, 220, 80), rgb(255, 210, 0), height * 2.0)
                } else {
                    lerp(rgb(255, 210, 0), rgb(255, 30, 30), (height - 0.5) * 2.0)
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::testing::*;
    use crate::live::{BLACK, Spectrum};

    fn column(key: &crate::keyboard::layout::KeyDef, w: f32) -> usize {
        (center(key).0 / w * BANDS as f32) as usize
    }

    #[test]
    fn columns_rise_with_their_band() {
        let mut bands = vec![0.0; BANDS];
        bands[0] = 1.0; // leftmost column full
        bands[1] = 0.3;
        let s = Spectrum { bands };
        let mut out = black();
        Equalizer.frame(&ctx(0.0, 0.04, &[], Some(&s), None), &mut out);
        let layout = crate::keyboard::Layout::ak980max();
        let (w, _) = layout.size();
        let col0: Vec<_> = layout.keys.iter().filter(|k| column(k, w) == 0).collect();
        assert!(!col0.is_empty());
        assert!(col0.iter().all(|k| out[usize::from(k.index)] != BLACK));
        let col1_lit = layout
            .keys
            .iter()
            .filter(|k| column(k, w) == 1 && out[usize::from(k.index)] != BLACK)
            .count();
        let col1_all = layout.keys.iter().filter(|k| column(k, w) == 1).count();
        assert!(col1_lit > 0 && col1_lit < col1_all, "{col1_lit}/{col1_all}");
        assert!(
            layout
                .keys
                .iter()
                .filter(|k| column(k, w) >= 2)
                .all(|k| out[usize::from(k.index)] == BLACK)
        );
    }

    #[test]
    fn dark_without_audio() {
        let mut out = black();
        Equalizer.frame(&ctx(0.0, 0.04, &[], None, None), &mut out);
        assert!(out.iter().all(|c| *c == BLACK));
        assert!(Equalizer.needs().audio);
    }
}
