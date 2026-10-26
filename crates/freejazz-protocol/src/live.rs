//! Live lighting stream (the site's music stream, `GET_LED_DATA` 0x32 written, protocol.md
//! 6.12) and key depth (`GET_MAGNETIC_AXIS_STATUS` 0x68, protocol.md 6.11).

use std::sync::LazyLock;

use serde::Deserialize;

use crate::blocks::{DeviceInfo, KEY_SLOTS, LedEffect, Rgb};
use crate::cmd::Cmd;
use crate::error::{Error, Result};
use crate::keyboard::Keyboard;
use crate::transfer::{TransferOpts, transfer};
use crate::transport::HidLink;

/// Slots of a live frame: one colour per key slot (index of `layout.json`).
pub const FRAME_SLOTS: usize = KEY_SLOTS;

/// Amplitude byte of the stream: the site computes it only for dot-matrix models; for the
/// AK980 MAX it always sends 0 (protocol.md 6.12).
pub const LIVE_AMPLITUDE: u8 = 0;

/// Keys per `GET_MAGNETIC_AXIS_STATUS` request, as the site batches them.
const STATUS_BATCH: usize = 7;
/// Bytes per key in a `GET_MAGNETIC_AXIS_STATUS` request and reply.
const STATUS_ENTRY: usize = 8;
const RETRIES: u8 = 3;

#[derive(Deserialize)]
struct Layout {
    keys: Vec<LayoutKey>,
}

#[derive(Deserialize)]
struct LayoutKey {
    index: u8,
}

static ORDER: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let layout: Layout = serde_json::from_str(include_str!("../../../data/ak980max/layout.json"))
        .expect("embedded layout.json is valid");
    layout.keys.into_iter().map(|k| k.index).collect()
});

/// Key indices in `layout.json` order (the rows of the site's `keyList`): the order of
/// the stream entries and of the key-status batches. 101 keys; slots without a key are
/// absent.
pub fn layout_order() -> &'static [u8] {
    &ORDER
}

/// The stream image of `frame`: `[index, r, g, b]` for each layout key, in layout order
/// (404 bytes for this model; slots without a key are not sent).
pub fn frame_bytes(frame: &[Rgb; FRAME_SLOTS]) -> Vec<u8> {
    layout_order()
        .iter()
        .flat_map(|&i| {
            let c = frame[usize::from(i)];
            [i, c.r, c.g, c.b]
        })
        .collect()
}

/// Travel in millimetres of a raw `GET_MAGNETIC_AXIS_STATUS` value: raw / 1000 when
/// `rtPrecision == 2`, else raw / 100 (protocol.md 6.11).
pub fn depth_to_mm(raw: u16, info: &DeviceInfo) -> f32 {
    let scale = if info.rt_precision == 2 {
        1000.0
    } else {
        100.0
    };
    f32::from(raw) / scale
}

/// One key of a `GET_MAGNETIC_AXIS_STATUS` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyStatus {
    pub index: u8,
    /// 2 = calibrated (calibration v2).
    pub state: u8,
    pub adc: u16,
    /// Raw travel; see [`depth_to_mm`].
    pub travel: u16,
}

impl<L: HidLink> Keyboard<L> {
    /// Before streaming: nothing. The site sends nothing before its music stream (neither
    /// `SET_LED_EFFECT` nor `SET_KEYBOARD_CUSTOM_FUNCTION_ON` 0x30, protocol.md 6.12-6.13).
    pub fn begin_live(&mut self) -> Result<()> {
        Ok(())
    }

    /// After streaming: re-sends `stored` (the effect the keyboard held before the stream)
    /// with `SET_LED_EFFECT`. The site sends nothing when it stops; on the tested unit the
    /// keyboard's return to its effect after the stream stops could not be confirmed by
    /// reads (protocol.md 6.12), so it is restored explicitly. Take `stored` from the
    /// snapshot read before any write, not from a later `GET_LED_EFFECT`, whose colour
    /// is not reliable after a write (protocol.md 9.7).
    pub fn end_live(&mut self, stored: &LedEffect) -> Result<()> {
        self.write_led_effect(stored)
    }

    /// Sends one live frame as the site's music stream does (protocol.md 6.12): the
    /// layout keys' `[index, r, g, b]` ([`frame_bytes`]), cmd 0x32, one transfer per
    /// chunk of `reportCount - 8` bytes at address `t × chunk`, extra header
    /// `[amplitude, 0]` on every chunk but the last, which gets `[amplitude]` (byte 6 =
    /// 1). Each chunk waits for its reply (50 ms, 3 retries). Use [`LIVE_AMPLITUDE`].
    pub fn send_frame(&mut self, frame: &[Rgb; FRAME_SLOTS], amplitude: u8) -> Result<()> {
        let bytes = frame_bytes(frame);
        let timeout_ms = self.timeout_ms(Cmd::GetLedData);
        let link = self.link_mut();
        let chunk = chunk_len(link.report_len())?;
        let n = bytes.len().div_ceil(chunk);
        for (t, data) in bytes.chunks(chunk).enumerate() {
            let extra = if t == n - 1 {
                vec![amplitude]
            } else {
                vec![amplitude, 0]
            };
            let opts = TransferOpts {
                addr_start: addr(t * chunk)?,
                timeout_ms,
                retries: RETRIES,
                extra,
                ..TransferOpts::write(Cmd::GetLedData, data.to_vec())
            };
            transfer(link, &opts)?;
        }
        Ok(())
    }

    // UNVERIFIED-HW: the site sends GET_MAGNETIC_AXIS_STATUS 0x68 only during calibration
    // v2 (frameVersion 1), never to this unit (frameVersion 0); to check on the hardware.
    /// Status of every layout key, read as the site's calibration v2 does (protocol.md
    /// 6.11): batches of 7 keys in layout order, request `[index, 0 × 7]` per key, address
    /// `batch × (reportCount - 8)` checked in the reply, last-chunk flag only on the last
    /// batch, 2000 ms. Reply per key: `[state, adc lo, adc hi, travel lo, travel hi, …]`.
    pub fn read_key_status(&mut self) -> Result<Vec<KeyStatus>> {
        let timeout_ms = self.timeout_ms(Cmd::GetMagneticAxisStatus);
        let link = self.link_mut();
        let chunk = chunk_len(link.report_len())?;
        let order = layout_order();
        let batches = order.len().div_ceil(STATUS_BATCH);
        let mut out = Vec::with_capacity(order.len());
        for (b, keys) in order.chunks(STATUS_BATCH).enumerate() {
            let mut data = vec![0u8; keys.len() * STATUS_ENTRY];
            for (k, &i) in keys.iter().enumerate() {
                data[k * STATUS_ENTRY] = i;
            }
            let opts = TransferOpts {
                addr_start: addr(b * chunk)?,
                timeout_ms,
                retries: RETRIES,
                need_last_flag: b == batches - 1,
                check_addr: true,
                ..TransferOpts::write(Cmd::GetMagneticAxisStatus, data)
            };
            let reply = transfer(link, &opts)?;
            for (k, &index) in keys.iter().enumerate() {
                let Some(e) = reply.get(k * STATUS_ENTRY..(k + 1) * STATUS_ENTRY) else {
                    break; // the site stops at a short reply
                };
                out.push(KeyStatus {
                    index,
                    state: e[0],
                    adc: u16::from_le_bytes([e[1], e[2]]),
                    travel: u16::from_le_bytes([e[3], e[4]]),
                });
            }
        }
        Ok(out)
    }

    // UNVERIFIED-HW: see `read_key_status` (0x68, not 0x60: the site never sends 0x60,
    // whose format is unknown).
    /// Raw travel of every key slot (0 for slots without a key); see [`depth_to_mm`].
    pub fn read_key_depths(&mut self) -> Result<[u16; FRAME_SLOTS]> {
        let mut depths = [0u16; FRAME_SLOTS];
        for s in self.read_key_status()? {
            depths[usize::from(s.index)] = s.travel;
        }
        Ok(depths)
    }
}

fn chunk_len(report_len: usize) -> Result<usize> {
    report_len
        .checked_sub(8)
        .filter(|&c| c > 0)
        .ok_or_else(|| Error::Invalid(format!("report length {report_len} too small")))
}

fn addr(offset: usize) -> Result<u16> {
    u16::try_from(offset).map_err(|_| Error::Invalid("address overflow".into()))
}
