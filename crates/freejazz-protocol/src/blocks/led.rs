//! `GET/SET_LED_EFFECT` 0x13 / 0x23, 16 bytes (protocol.md 6.4) and
//! `GET/SET_CUSTOM_LED_DATA` 0x14 / 0x24, 512 bytes (protocol.md 6.5).

use serde::{Deserialize, Serialize};

use super::{KEY_SLOTS, check_len};
use crate::error::Result;

pub const LED_EFFECT_LEN: usize = 16;
pub const CUSTOM_LEDS_LEN: usize = KEY_SLOTS * 4;

/// `mode` of the custom per-key colours (protocol.md 6.5).
pub const MODE_CUSTOM: u8 = 128;

/// What firmware 1.28 reports for [`MODE_CUSTOM`] after it was written (protocol.md 9.7).
pub const MODE_CUSTOM_READ: u8 = 20;

/// The mode to write: the read-back value of the custom mode is the custom mode.
fn canonical_mode(mode: u8) -> u8 {
    if mode == MODE_CUSTOM_READ {
        MODE_CUSTOM
    } else {
        mode
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Lighting effect. Byte 4 (`driverSetting`), 13 and the check code 14-15 are not
/// kept: they are written as the site writes them (255, 0, 0xAA 0x55).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LedEffect {
    /// 0 off, 1-19 built-in effects (`effects.json`), 128 custom colours (read back as
    /// 20: decoded as 128, never written as 20).
    pub mode: u8,
    pub rgb: Rgb,
    pub secondary: Rgb,
    /// 0 single colour, 1 RGB.
    pub color_mode: u8,
    /// 1-5 for this model.
    pub brightness: u8,
    /// 1-5.
    pub speed: u8,
    /// 0 right, 1 left, 2 up, 3 down (depending on the effect).
    pub direction: u8,
    pub effect_mode_type: u8,
}

impl LedEffect {
    pub fn decode(b: &[u8]) -> Result<Self> {
        check_len("LED_EFFECT", b, LED_EFFECT_LEN)?;
        Ok(Self {
            mode: canonical_mode(b[0]),
            rgb: Rgb {
                r: b[1],
                g: b[2],
                b: b[3],
            },
            secondary: Rgb {
                r: b[5],
                g: b[6],
                b: b[7],
            },
            color_mode: b[8],
            brightness: b[9],
            speed: b[10],
            direction: b[11],
            effect_mode_type: b[12],
        })
    }

    /// Read-back rule: `read` (from `GET_LED_EFFECT` after writing `self`, or a backup's
    /// bytes) holds `self`. The colour (bytes 1-3) and `colorMode` (byte 8) are not
    /// compared: firmware 1.28 reports #ffffff / RGB after any write while the keyboard
    /// shows what was written (protocol.md 9.7), so they cannot be verified by reading
    /// back. Everything else is.
    pub fn matches_readback(&self, read: &LedEffect) -> bool {
        LedEffect {
            mode: canonical_mode(self.mode),
            rgb: read.rgb,
            color_mode: read.color_mode,
            ..self.clone()
        } == LedEffect {
            mode: canonical_mode(read.mode),
            ..read.clone()
        }
    }

    /// Whether `read` also shows the colour and `colorMode` of `self` (informational:
    /// see [`LedEffect::matches_readback`]).
    pub fn colour_verified_by(&self, read: &LedEffect) -> bool {
        (self.rgb, self.color_mode) == (read.rgb, read.color_mode)
    }

    pub fn encode(&self) -> Vec<u8> {
        let (p, s) = (self.rgb, self.secondary);
        vec![
            canonical_mode(self.mode),
            p.r,
            p.g,
            p.b,
            255, // driverSetting
            s.r,
            s.g,
            s.b,
            self.color_mode,
            self.brightness,
            self.speed,
            self.direction,
            self.effect_mode_type,
            0,
            0xAA, // checkCodeL
            0x55, // checkCodeH
        ]
    }
}

/// Custom colour of each key slot (entry `i` = LED of layout index `i`). Read entries
/// are taken by position; the index byte is written as the position, like the site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomLeds(#[serde(with = "super::fixed_array")] pub [Rgb; KEY_SLOTS]);

impl Default for CustomLeds {
    fn default() -> Self {
        Self([Rgb::default(); KEY_SLOTS])
    }
}

impl CustomLeds {
    pub fn decode(b: &[u8]) -> Result<Self> {
        check_len("CUSTOM_LED_DATA", b, CUSTOM_LEDS_LEN)?;
        let mut leds = Self::default();
        for (c, e) in leds.0.iter_mut().zip(b.chunks_exact(4)) {
            *c = Rgb {
                r: e[1],
                g: e[2],
                b: e[3],
            };
        }
        Ok(leds)
    }

    pub fn encode(&self) -> Vec<u8> {
        (0u8..)
            .zip(self.0.iter())
            .flat_map(|(i, c)| [i, c.r, c.g, c.b])
            .collect()
    }
}
