//! Calibration (0x64 / 0x65) and the key-travel test (0x66 / 0x67) of frameVersion 0
//! keyboards, with the spontaneous `GET_MAGNETIC_AXIS_CALIBRATION_DATA` (0xFB) reports
//! both produce (protocol.md 6.11).
//!
//! UNVERIFIED-HW: the site never sends 0x66 / 0x67 to this model (its test panel is
//! hidden) and never offers calibration for `frameVersion` 0 in practice. Nothing here
//! has been run against a keyboard.

use crate::cmd::Cmd;
use crate::error::Result;
use crate::keyboard::Keyboard;
use crate::packet::encode_custom;
use crate::transfer::exchange;
use crate::transport::HidLink;

/// The site waits this long for the acknowledgement of 0x64 / 0x66.
const START_TIMEOUT_MS: u32 = 1000;
const RETRIES: u8 = 1;

/// One 0xFB report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisReport {
    /// Key index (a slot of `layout.json`).
    pub index: u8,
    /// 1 = calibrated (calibration v1).
    pub state: u8,
    pub max: u16,
    pub min: u16,
    pub current: u16,
    /// Travel, hundredths of a millimetre (the site shows `key_stroke / 100` mm).
    pub key_stroke: u16,
    /// Maximum travel (the site shows it `/ 10`; unit not confirmed).
    pub max_stroke: u16,
}

impl AxisReport {
    /// Parses `55 FB …` (bytes 2–13); `None` for any other report.
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < 14 || buf[0] != 0x55 || buf[1] != Cmd::GetMagneticAxisCalibrationData as u8 {
            return None;
        }
        let u16_at = |i: usize| u16::from_le_bytes([buf[i], buf[i + 1]]);
        Some(Self {
            index: buf[2],
            state: buf[3],
            max: u16_at(4),
            min: u16_at(6) & 0x7FFF,
            current: u16_at(8),
            key_stroke: u16_at(10),
            max_stroke: u16_at(12),
        })
    }

    /// Travel in millimetres.
    pub fn stroke_mm(&self) -> f32 {
        f32::from(self.key_stroke) / 100.0
    }
}

impl<L: HidLink> Keyboard<L> {
    // UNVERIFIED-HW: never sent to this model by the site.
    /// `SET_SIMULATION_TEST_ON`: `AA 66 00 …`, reply 0x66 within 1000 ms; the keyboard
    /// then sends 0xFB reports while keys move. Stop with [`Keyboard::stop_key_test`].
    pub fn start_key_test(&mut self) -> Result<()> {
        self.start_stream(Cmd::SetSimulationTestOn)
    }

    // UNVERIFIED-HW
    /// `SET_SIMULATION_TEST_OFF`: `AA 67 00 …`, no reply.
    pub fn stop_key_test(&mut self) -> Result<()> {
        self.stop_stream(Cmd::SetSimulationTestOff)
    }

    // UNVERIFIED-HW: calibration v1 (frameVersion 0), never run.
    /// `SET_CALIBRATION_ON`: `AA 64 00 …`, reply 0x64 within 1000 ms; 0xFB reports follow.
    pub fn start_calibration(&mut self) -> Result<()> {
        self.start_stream(Cmd::SetCalibrationOn)
    }

    // UNVERIFIED-HW
    /// `SET_CALIBRATION_OFF`: `AA 65 00 …`, no reply.
    pub fn stop_calibration(&mut self) -> Result<()> {
        self.stop_stream(Cmd::SetCalibrationOff)
    }

    /// The next 0xFB report within `timeout_ms`; other reports are skipped. `None` on
    /// timeout.
    pub fn read_axis_report(&mut self, timeout_ms: i32) -> Result<Option<AxisReport>> {
        let link = self.link_mut();
        let mut buf = vec![0u8; link.report_len()];
        let n = link.read(&mut buf, timeout_ms)?;
        Ok(if n == 0 {
            None
        } else {
            AxisReport::parse(&buf[..n])
        })
    }

    fn start_stream(&mut self, cmd: Cmd) -> Result<()> {
        let report = encode_custom(&[0xAA, cmd as u8, 0], &[], self.link_mut().report_len());
        exchange(self.link_mut(), cmd, &report, START_TIMEOUT_MS, RETRIES).map(|_| ())
    }

    fn stop_stream(&mut self, cmd: Cmd) -> Result<()> {
        let link = self.link_mut();
        let report = encode_custom(&[0xAA, cmd as u8, 0], &[], link.report_len());
        link.write(&report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fb(index: u8, stroke: u16) -> Vec<u8> {
        let mut r = vec![0u8; 64];
        r[0] = 0x55;
        r[1] = 0xFB;
        r[2] = index;
        r[3] = 1;
        r[6..8].copy_from_slice(&0x8123u16.to_le_bytes());
        r[10..12].copy_from_slice(&stroke.to_le_bytes());
        r
    }

    #[test]
    fn parses_a_0xfb_report() {
        let r = AxisReport::parse(&fb(42, 250)).unwrap();
        assert_eq!((r.index, r.state, r.key_stroke), (42, 1, 250));
        assert_eq!(r.min, 0x0123);
        assert!((r.stroke_mm() - 2.5).abs() < 1e-6);
        assert_eq!(AxisReport::parse(&[0x55, 0x32, 0, 0, 0, 0, 0, 0]), None);
        assert_eq!(AxisReport::parse(&[0x55, 0xFB, 1]), None);
    }
}
