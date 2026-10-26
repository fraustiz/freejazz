//! `GET_DEVICE_INFO` 0x10, 48 bytes, read only (protocol.md 6.1).

use serde::{Deserialize, Serialize};

use super::check_len;
use crate::error::Result;

pub const DEVICE_INFO_LEN: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub rom_size: u8,
    /// `macroSpaceSize` as reported by the keyboard, for the record: the site ignores it
    /// for this model and uses 512 (`MACRO_CAPACITY`, protocol.md 6.6).
    pub macro_space: u16,
    pub vid: u16,
    pub pid: u16,
    /// Firmware version, e.g. "1.23".
    pub firmware: String,
    pub sensor: u16,
    pub manufacturer: u16,
    pub product: u16,
    pub work_mode: u8,
    pub battery: u8,
    pub charge: u8,
    /// Numbered from 1.
    pub current_profile: u8,
    pub axis_info: u16,
    pub tft_max_frames: u16,
    pub gif_max_frames: u16,
    pub led_max_frames: u16,
    /// 255 = no screen.
    pub tft_direction: u8,
    /// Scale of the magnetic travel values (protocol.md 6.7).
    pub rt_precision: u8,
    /// 1 = long timeouts, calibration v2, `FUNC_V2`.
    pub frame_version: u8,
    pub lighting_version: u8,
    /// 1 = waiting for a firmware update: read nothing else.
    pub firmware_status: u8,
    /// Hardware profiles when > 1.
    pub max_profile: u8,
    /// The 48 bytes as read.
    pub raw: Vec<u8>,
}

impl DeviceInfo {
    pub fn decode(b: &[u8]) -> Result<Self> {
        check_len("GET_DEVICE_INFO", b, DEVICE_INFO_LEN)?;
        let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        let version = u32::from(b[8] & 0x0F) + u32::from(b[8] >> 4) * 10 + u32::from(b[9]) * 100;
        Ok(Self {
            rom_size: b[0],
            macro_space: u16_at(2),
            vid: u16_at(4),
            pid: u16_at(6),
            firmware: format!("{}.{:02}", version / 100, version % 100),
            sensor: u16_at(10),
            manufacturer: u16_at(12),
            product: u16_at(14),
            work_mode: b[16],
            battery: b[17],
            charge: b[18],
            current_profile: b[19],
            axis_info: u16_at(20),
            tft_max_frames: u16_at(22),
            gif_max_frames: u16_at(24),
            led_max_frames: u16_at(26),
            tft_direction: b[28],
            rt_precision: b[29],
            frame_version: b[30],
            lighting_version: b[31],
            firmware_status: b[32],
            max_profile: b[33],
            raw: b.to_vec(),
        })
    }
}
