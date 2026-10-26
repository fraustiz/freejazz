//! The calibration wizard: start, press every key all the way down, finish.
//!
//! UNVERIFIED-HW: calibration v1 (0x64 / 0x65, frameVersion 0) was never run against a
//! keyboard; the site does not offer it for this model in practice.

use std::collections::BTreeSet;

use freejazz_protocol::calibration::AxisReport;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Running,
    Done,
}

#[derive(Default)]
pub struct Calibration {
    pub phase: Phase,
    /// Keys the keyboard reports as calibrated (state 1) since the start.
    pub calibrated: BTreeSet<u8>,
    /// The user pressed « Terminer »: the end of the stream is a completed run.
    finished: bool,
}

impl Calibration {
    /// The keyboard confirmed (or ended) the calibration stream.
    pub fn running(&mut self, on: bool) {
        match (self.phase, on) {
            (_, true) => {
                self.phase = Phase::Running;
                self.finished = false;
                self.calibrated.clear();
            }
            // only a run the user finished is « terminé »; any other end is an abort
            (Phase::Running, false) => {
                self.phase = if self.finished {
                    Phase::Done
                } else {
                    Phase::Idle
                };
                self.finished = false;
            }
            _ => {}
        }
    }

    /// « Terminer »: the stop that follows completes the run.
    pub fn finish(&mut self) {
        self.finished = self.phase == Phase::Running;
    }

    /// The run ended without the user finishing it (disconnect, leaving the section).
    pub fn abort(&mut self) {
        if self.phase == Phase::Running {
            self.phase = Phase::Idle;
        }
        self.finished = false;
    }

    // UNVERIFIED-HW
    pub fn on_report(&mut self, r: &AxisReport) {
        if self.phase == Phase::Running && r.state == 1 {
            self.calibrated.insert(r.index);
        }
    }

    /// Share of `total` keys calibrated.
    pub fn progress(&self, total: usize) -> f32 {
        if total == 0 {
            0.0
        } else {
            self.calibrated.len() as f32 / total as f32
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(index: u8, state: u8) -> AxisReport {
        let mut b = vec![0u8; 64];
        b[0] = 0x55;
        b[1] = 0xFB;
        b[2] = index;
        b[3] = state;
        AxisReport::parse(&b).unwrap()
    }

    #[test]
    fn wizard_counts_calibrated_keys_while_running() {
        let mut c = Calibration::default();
        c.on_report(&report(1, 1));
        assert!(c.calibrated.is_empty(), "idle: ignored");
        c.running(true);
        c.on_report(&report(1, 1));
        c.on_report(&report(1, 1));
        c.on_report(&report(2, 0));
        c.on_report(&report(3, 1));
        assert_eq!(c.calibrated.len(), 2);
        assert!((c.progress(4) - 0.5).abs() < 1e-6);
        c.finish();
        c.running(false);
        assert_eq!(c.phase, Phase::Done);
        c.running(true);
        assert!(c.calibrated.is_empty() && c.phase == Phase::Running);
    }
}
