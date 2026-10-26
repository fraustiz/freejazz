//! Live lighting: effects computed on the PC and streamed to the keyboard (or drawn in
//! the app's preview, with the very same [`Effect::frame`] code and no device).
//!
//! The pieces: [`Effect`] (one look), [`Composite`] (layers of effects, each painting only
//! its keys: the custom effects), [`Engine`] (the 25 fps thread feeding the device),
//! [`input`] (global key presses), [`audio`] and [`system`] (sources started only while an
//! effect needs them).

use std::collections::BTreeSet;

use freejazz_protocol::blocks::Rgb;
use freejazz_protocol::live::FRAME_SLOTS;
use serde_json::Value;

use crate::keyboard::Layout;

pub mod audio;
pub mod custom;
pub mod effects;
pub mod engine;
pub mod input;
pub mod system;

pub use custom::{CustomEffect, Layer};
pub use engine::{Engine, EngineConfig, Renderer, end_active};

/// One colour per key slot (index of `layout.json`).
pub type Frame = [Rgb; FRAME_SLOTS];

pub const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };

/// A key press seen by the global hook, as a layout index and the moment (seconds on the
/// [`input::now`] clock).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyPress {
    pub index: u8,
    pub at: f32,
}

/// The audio spectrum: 21 log-spaced bands, each 0..=1.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Spectrum {
    pub bands: Vec<f32>,
}

impl Spectrum {
    /// Mean level of all bands.
    pub fn level(&self) -> f32 {
        if self.bands.is_empty() {
            0.0
        } else {
            self.bands.iter().sum::<f32>() / self.bands.len() as f32
        }
    }
}

/// CPU and RAM use, in percent.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SystemStats {
    pub cpu: f32,
    pub ram: f32,
}

/// What an effect reads, for one frame.
pub struct FrameCtx<'a> {
    /// Seconds since the engine (or preview) started.
    pub t: f32,
    /// Seconds since the previous frame.
    pub dt: f32,
    pub layout: &'a Layout,
    /// Presses since the previous frame.
    pub presses: &'a [KeyPress],
    /// Key depth, 0 (up) to 1 (bottomed out), when the keyboard gives it (it does not,
    /// by default: effects then fall back to presses).
    pub depths: Option<&'a [f32; FRAME_SLOTS]>,
    pub audio: Option<&'a Spectrum>,
    pub system: Option<&'a SystemStats>,
}

/// The sources an effect wants running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Needs {
    pub audio: bool,
    pub system: bool,
}

impl Needs {
    pub fn or(self, o: Needs) -> Needs {
        Needs {
            audio: self.audio || o.audio,
            system: self.system || o.system,
        }
    }
}

/// A lighting effect. `frame` receives `out` already black and paints the keys it lights.
pub trait Effect: Send {
    fn id(&self) -> &'static str;
    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame);
    fn configure(&mut self, _params: &Value) {}
    fn needs(&self) -> Needs {
        Needs::default()
    }
}

/// The ids [`create`] knows.
pub const EFFECT_IDS: &[&str] = &[
    "depth",
    "ripple",
    "heatmap",
    "ambient",
    "fire",
    "aurora",
    "matrix",
    "solid",
    "breathing",
    "wave",
    "scrolling_gradient",
    "stars",
    "reactive",
    "equalizer",
    "music",
    "gauge",
];

/// A new effect by id, configured with `params`.
pub fn create(id: &str, params: &Value) -> Option<Box<dyn Effect>> {
    use effects::*;
    let mut e: Box<dyn Effect> = match id {
        "depth" => Box::new(depth::Depth::default()),
        "ripple" => Box::new(ripple::Ripple::default()),
        "heatmap" => Box::new(heatmap::Heatmap::default()),
        "ambient" | "fire" => Box::new(ambient::Ambient::new(ambient::Variant::Fire)),
        "aurora" => Box::new(ambient::Ambient::new(ambient::Variant::Aurora)),
        "matrix" => Box::new(ambient::Ambient::new(ambient::Variant::Matrix)),
        "solid" => Box::new(blocks::Solid::default()),
        "breathing" => Box::new(blocks::Breathing::default()),
        "wave" => Box::new(blocks::Wave::default()),
        "scrolling_gradient" => Box::new(blocks::ScrollingGradient::default()),
        "stars" => Box::new(blocks::Stars::default()),
        "reactive" => Box::new(blocks::Reactive::default()),
        "equalizer" => Box::new(equalizer::Equalizer),
        "music" => Box::new(music::Music::default()),
        "gauge" => Box::new(gauge::Gauge::default()),
        _ => return None,
    };
    e.configure(params);
    Some(e)
}

/// Layers drawn bottom to top; a layer paints only its keys, and only where it is lit
/// (black lets the layers below show through).
pub struct Composite {
    layers: Vec<(Box<dyn Effect>, BTreeSet<u8>)>,
    scratch: Frame,
}

impl Composite {
    pub fn new(layers: Vec<(Box<dyn Effect>, BTreeSet<u8>)>) -> Self {
        Self {
            layers,
            scratch: [BLACK; FRAME_SLOTS],
        }
    }
}

impl Effect for Composite {
    fn id(&self) -> &'static str {
        "custom"
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        for (effect, keys) in &mut self.layers {
            self.scratch = [BLACK; FRAME_SLOTS];
            effect.frame(ctx, &mut self.scratch);
            for &k in keys.iter() {
                // black is transparent: a reactive or starry layer lets the ones below show
                let c = self.scratch[usize::from(k)];
                if c != BLACK
                    && let Some(slot) = out.get_mut(usize::from(k))
                {
                    *slot = c;
                }
            }
        }
    }

    fn needs(&self) -> Needs {
        self.layers
            .iter()
            .fold(Needs::default(), |n, (e, _)| n.or(e.needs()))
    }
}

// ---- colour and parameter helpers shared by the effects ----

pub fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    Rgb { r, g, b }
}

/// `c` scaled by `f` (0..=1).
pub fn scale(c: Rgb, f: f32) -> Rgb {
    let f = f.clamp(0.0, 1.0);
    let s = |v: u8| (f32::from(v) * f).round() as u8;
    rgb(s(c.r), s(c.g), s(c.b))
}

pub fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    rgb(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b))
}

/// Per-channel maximum (light adds up without wrapping).
pub fn max_rgb(a: Rgb, b: Rgb) -> Rgb {
    rgb(a.r.max(b.r), a.g.max(b.g), a.b.max(b.b))
}

/// Hue in turns (any value, wrapped), saturation and value 0..=1.
pub fn hsv(h: f32, s: f32, v: f32) -> Rgb {
    let h = h.rem_euclid(1.0) * 6.0;
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let (i, f) = (h.floor(), h - h.floor());
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i as u8 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let c = |x: f32| (x * 255.0).round() as u8;
    rgb(c(r), c(g), c(b))
}

/// Centre of a key in key units.
pub fn center(k: &crate::keyboard::layout::KeyDef) -> (f32, f32) {
    (k.x + k.w / 2.0, k.y + k.h / 2.0)
}

/// A colour parameter: `"#rrggbb"` or `[r, g, b]`.
pub fn param_color(params: &Value, name: &str, default: Rgb) -> Rgb {
    match params.get(name) {
        Some(Value::String(s)) => {
            let h = s.trim_start_matches('#');
            if h.len() == 6
                && let Ok(n) = u32::from_str_radix(h, 16)
            {
                return rgb((n >> 16) as u8, (n >> 8) as u8, n as u8);
            }
            default
        }
        Some(Value::Array(a)) if a.len() == 3 => {
            let c = |i: usize| a[i].as_u64().map(|v| v.min(255) as u8);
            match (c(0), c(1), c(2)) {
                (Some(r), Some(g), Some(b)) => rgb(r, g, b),
                _ => default,
            }
        }
        _ => default,
    }
}

pub fn param_f32(params: &Value, name: &str, default: f32) -> f32 {
    params
        .get(name)
        .and_then(Value::as_f64)
        .map_or(default, |v| v as f32)
}

pub fn param_str<'a>(params: &'a Value, name: &str) -> Option<&'a str> {
    params.get(name).and_then(Value::as_str)
}

/// Small deterministic generator for the ambient effects (no global state, no crate).
#[derive(Debug, Clone)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// 0..1
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub fn ctx<'a>(
        t: f32,
        dt: f32,
        presses: &'a [KeyPress],
        audio: Option<&'a Spectrum>,
        system: Option<&'a SystemStats>,
    ) -> FrameCtx<'a> {
        FrameCtx {
            t,
            dt,
            layout: Layout::ak980max(),
            presses,
            depths: None,
            audio,
            system,
        }
    }

    pub fn black() -> Frame {
        [BLACK; FRAME_SLOTS]
    }

    pub fn index_of(label: &str) -> usize {
        usize::from(
            Layout::ak980max()
                .keys
                .iter()
                .find(|k| k.label == label)
                .unwrap()
                .index,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn every_registered_id_builds_and_reports_itself() {
        for id in EFFECT_IDS {
            let e = create(id, &Value::Null).unwrap_or_else(|| panic!("{id}"));
            if *id != "ambient" {
                assert_eq!(e.id(), *id);
            }
        }
        assert!(create("nope", &Value::Null).is_none());
    }

    #[test]
    fn layer_over_layer_only_overrides_its_keys() {
        let red = serde_json::json!({"color": "#ff0000"});
        let blue = serde_json::json!({"color": [0, 0, 255]});
        let all: BTreeSet<u8> = Layout::ak980max().keys.iter().map(|k| k.index).collect();
        let some: BTreeSet<u8> = [1u8, 2, 3].into();
        let mut c = Composite::new(vec![
            (create("solid", &red).unwrap(), all),
            (create("solid", &blue).unwrap(), some),
        ]);
        let mut out = black();
        c.frame(&ctx(0.0, 0.04, &[], None, None), &mut out);
        assert_eq!(out[2], rgb(0, 0, 255));
        assert_eq!(out[1], rgb(0, 0, 255));
        assert_eq!(out[4], rgb(255, 0, 0));
        // a slot without a key stays black
        assert_eq!(out[127], BLACK);
    }

    #[test]
    fn a_dark_layer_is_transparent() {
        let red = serde_json::json!({"color": "#ff0000"});
        let dark = serde_json::json!({"color": "#000000"});
        let all: BTreeSet<u8> = Layout::ak980max().keys.iter().map(|k| k.index).collect();
        let mut c = Composite::new(vec![
            (create("solid", &red).unwrap(), all.clone()),
            (create("solid", &dark).unwrap(), all),
        ]);
        let mut out = black();
        c.frame(&ctx(0.0, 0.04, &[], None, None), &mut out);
        assert_eq!(out[4], rgb(255, 0, 0));
    }

    #[test]
    fn hsv_primaries() {
        assert_eq!(hsv(0.0, 1.0, 1.0), rgb(255, 0, 0));
        assert_eq!(hsv(1.0 / 3.0, 1.0, 1.0), rgb(0, 255, 0));
        assert_eq!(
            param_color(&serde_json::json!({"c": "#7c5cff"}), "c", BLACK),
            rgb(0x7c, 0x5c, 0xff)
        );
    }
}
