//! `GET/SET_MAGNETIC_AXIS_DKS_DATA` 0x18 / 0x28, 1024 bytes: 64 entries of 16 bytes
//! (protocol.md 6.8). Entry `s` is the one key action `08 s 00 00` refers to.

use serde::{Deserialize, Serialize};

use super::check_len;
use crate::error::{Error, Result};

pub const DKS_LEN: usize = 1024;
const ENTRY_LEN: usize = 16;
pub const MAX_DKS_SLOT: u8 = 63;

/// One DKS entry; every byte read is written back.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DksEntry {
    /// Slot 0-63.
    pub index: u8,
    /// Press points in 0.1 mm: key start, bottom (bytes 0-1).
    pub make: [u8; 2],
    /// Release points in 0.1 mm: release from bottom, end of release (bytes 2-3).
    pub brk: [u8; 2],
    /// HID usage of actions 1-4, 0 = none (bytes 5, 7, 9, 11).
    pub actions: [u8; 4],
    /// `states[action][stage]`, stages = press at point 1, bottom, release from bottom,
    /// end of release: bit 0 = the action is triggered at that stage, bit 1 = it is
    /// held during it. Stage byte `s` (byte 12 + s) holds trigger bits in its low
    /// nibble and hold bits in its high nibble, bit `k` for action `k + 1`.
    pub states: [[u8; 4]; 4],
    /// Bytes 4, 6, 8, 10 as read: never read by the site (which writes 0), perhaps the
    /// high byte of a 16-bit action (protocol.md 6.8).
    #[serde(default)]
    pub reserved: [u8; 4],
}

impl DksEntry {
    /// A free slot for the site: both press and release points zero (bytes 0-3).
    pub fn is_empty(&self) -> bool {
        self.make == [0, 0] && self.brk == [0, 0]
    }

    fn decode(index: u8, e: &[u8]) -> Self {
        let mut states = [[0u8; 4]; 4];
        for (stage, &byte) in e[12..16].iter().enumerate() {
            for (action, row) in states.iter_mut().enumerate() {
                row[stage] = ((byte >> action) & 1) | (((byte >> (action + 4)) & 1) << 1);
            }
        }
        Self {
            index,
            make: [e[0], e[1]],
            brk: [e[2], e[3]],
            actions: [e[5], e[7], e[9], e[11]],
            states,
            reserved: [e[4], e[6], e[8], e[10]],
        }
    }

    fn encode(&self) -> [u8; ENTRY_LEN] {
        let mut e = [0u8; ENTRY_LEN];
        e[..2].copy_from_slice(&self.make);
        e[2..4].copy_from_slice(&self.brk);
        for (k, (&usage, &reserved)) in self.actions.iter().zip(&self.reserved).enumerate() {
            e[4 + 2 * k] = reserved;
            e[5 + 2 * k] = usage;
        }
        for stage in 0..4 {
            e[12 + stage] = self
                .states
                .iter()
                .enumerate()
                .fold(0u8, |byte, (action, row)| {
                    byte | ((row[stage] & 1) << action) | (((row[stage] >> 1) & 1) << (action + 4))
                });
        }
        e
    }
}

/// The DKS entries in slot order when read. A slot the site treats as free
/// ([`DksEntry::is_empty`]) is listed only when it still holds other non-zero bytes,
/// so that they are written back.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DksTable(pub Vec<DksEntry>);

impl DksTable {
    /// All-zero slots are skipped.
    pub fn decode(b: &[u8]) -> Result<Self> {
        check_len("MAGNETIC_AXIS_DKS_DATA", b, DKS_LEN)?;
        Ok(Self(
            (0u8..)
                .zip(b.chunks_exact(ENTRY_LEN))
                .filter(|(_, e)| e.iter().any(|&v| v != 0))
                .map(|(i, e)| DksEntry::decode(i, e))
                .collect(),
        ))
    }

    /// Representability: slots 0-63 without duplicates; every state value 0-3.
    pub fn validate(&self) -> Result<()> {
        let mut seen = [false; MAX_DKS_SLOT as usize + 1];
        for d in &self.0 {
            let Some(slot) = seen.get_mut(usize::from(d.index)) else {
                return Err(Error::Validation(format!(
                    "DKS : emplacement {} hors limites (0–{MAX_DKS_SLOT})",
                    d.index
                )));
            };
            if *slot {
                return Err(Error::Validation(format!(
                    "DKS : emplacement {} en double",
                    d.index
                )));
            }
            *slot = true;
            if let Some(v) = d.states.iter().flatten().find(|&&v| v > 3) {
                return Err(Error::Validation(format!(
                    "DKS emplacement {} : état {v} invalide (0–3)",
                    d.index
                )));
            }
        }
        Ok(())
    }

    /// 1024 zero bytes, then each entry at `16 × index`.
    pub fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut out = vec![0u8; DKS_LEN];
        for d in &self.0 {
            let at = ENTRY_LEN * usize::from(d.index);
            out[at..at + ENTRY_LEN].copy_from_slice(&d.encode());
        }
        Ok(out)
    }
}
