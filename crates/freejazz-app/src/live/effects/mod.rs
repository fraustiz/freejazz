//! The effects. `depth`, `ripple`, `heatmap` and `ambient` react to presses or move on
//! their own; `blocks` holds the simple building blocks custom effects are made of;
//! `equalizer`, `music` and `gauge` read the audio spectrum or the system load.

pub mod ambient;
pub mod blocks;
pub mod depth;
pub mod equalizer;
pub mod gauge;
pub mod heatmap;
pub mod music;
pub mod ripple;

use super::Rgb;

/// The app's accent colour (`#7c5cff`), the default of most effects.
pub const ACCENT: Rgb = Rgb {
    r: 0x7c,
    g: 0x5c,
    b: 0xff,
};
