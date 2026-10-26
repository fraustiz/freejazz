//! `GET_MACRO` 0x15 / `SET_MACRO` 0x25 (protocol.md 6.6).
//!
//! Address space: 0-399 a table of 100 u32 absolute pointers (0 = no macro), then the
//! macros from 400 in id order, each `[2 x count u16, 0, 0]` + 4 bytes per action.

use serde::{Deserialize, Serialize};

use super::check_len;
use crate::error::{Error, Result};

pub const MACRO_TABLE_LEN: usize = 400;
/// Address of the first macro.
pub const MACRO_DATA_START: u16 = 400;
pub const MAX_MACRO_ID: u8 = 99;
/// Bytes for macros on the AK980 MAX: the site's fixed 512 (it ignores `macroSpaceSize`
/// for this model, protocol.md 6.6).
pub const MACRO_CAPACITY: u16 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroAction {
    /// Press (true) or release.
    pub press: bool,
    /// Type field of the flags (bits 4-6) as read: 1 mouse, 3 keyboard. Written like the
    /// site: 1 and 2 as the mouse type (`0x90`/`0x10`), anything else as the keyboard
    /// type (`0xB0`/`0x30`).
    pub kind: u8,
    /// HID usage, or mouse button 1 left, 2 right, 4 middle, 8 back, 16 forward.
    pub key: u8,
    /// Delay after the action.
    pub delay_ms: u16,
    /// Flags byte as read; `None` for an action built by the app. Written back as is
    /// while `press` and `kind` still match it; after an edit its bits 0-3 are kept.
    #[serde(default)]
    pub raw_flags: Option<u8>,
}

/// `(press, kind)` held by a flags byte.
fn flag_fields(f: u8) -> (bool, u8) {
    (f & 0x80 != 0, (f >> 4) & 0x07)
}

impl MacroAction {
    pub fn decode(b: [u8; 4]) -> Self {
        let (press, kind) = flag_fields(b[3]);
        Self {
            press,
            kind,
            key: b[2],
            delay_ms: u16::from_le_bytes([b[0], b[1]]),
            raw_flags: Some(b[3]),
        }
    }

    pub fn encode(&self) -> [u8; 4] {
        let [lo, hi] = self.delay_ms.to_le_bytes();
        let kind = if matches!(self.kind, 1 | 2) {
            0x10
        } else {
            0x30
        };
        let site = if self.press { 0x80 | kind } else { kind };
        let flags = match self.raw_flags {
            Some(f) if flag_fields(f) == (self.press, self.kind) => f,
            Some(f) => (f & 0x0F) | site,
            None => site,
        };
        [lo, hi, self.key, flags]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Macro {
    /// 0-99 (the id key actions refer to). Names are not stored in the keyboard.
    pub id: u8,
    pub actions: Vec<MacroAction>,
    /// 4-byte header as read (`[n lo, n hi, b2, b3]`); `None` for a macro built by the
    /// app. Written back as is while `n / 2` still equals the number of actions
    /// (an odd `n` included); otherwise `n` is rewritten and bytes 2-3 kept.
    #[serde(default)]
    pub raw_header: Option<[u8; 4]>,
}

impl Macro {
    fn header(&self) -> [u8; 4] {
        // Fits: `Macros::to_bytes` bounds the macros by the 16-bit address space.
        let count = self.actions.len() as u16;
        match self.raw_header {
            Some(h) if u16::from_le_bytes([h[0], h[1]]) / 2 == count => h,
            Some([_, _, b2, b3]) => {
                let [lo, hi] = (2 * count).to_le_bytes();
                [lo, hi, b2, b3]
            }
            None => {
                let [lo, hi] = (2 * count).to_le_bytes();
                [lo, hi, 0, 0]
            }
        }
    }

    /// Written to the keyboard: with actions, or read from it without any (kept as is);
    /// a macro left without actions gets pointer 0, like the site.
    fn is_written(&self) -> bool {
        !self.actions.is_empty()
            || self
                .raw_header
                .is_some_and(|h| u16::from_le_bytes([h[0], h[1]]) / 2 == 0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Macros {
    pub slots: Vec<Macro>,
}

impl Macros {
    /// Decodes the macros from the 400-byte pointer table, following each pointer with
    /// `read(addr, len)` as the site does: the 4-byte header at `p`, then the actions
    /// at `p + 4`.
    pub fn decode(
        table: &[u8],
        mut read: impl FnMut(u16, usize) -> Result<Vec<u8>>,
    ) -> Result<Self> {
        check_len("MACRO table", table, MACRO_TABLE_LEN)?;
        let mut slots = Vec::new();
        for (id, p) in (0u8..).zip(table.chunks_exact(4)) {
            let ptr = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            if ptr == 0 {
                continue;
            }
            let out_of_range = || Error::Invalid(format!("macro {id}: pointer {ptr} out of range"));
            let addr = u16::try_from(ptr).map_err(|_| out_of_range())?;
            let header = read(addr, 4)?;
            check_len("MACRO header", &header, 4)?;
            let count = usize::from(u16::from_le_bytes([header[0], header[1]]) / 2);
            let mut actions = Vec::with_capacity(count);
            if count > 0 {
                let len = count * 4;
                if usize::from(addr) + 4 + len > 0x1_0000 {
                    return Err(out_of_range());
                }
                let data = read(addr + 4, len)?;
                check_len("MACRO actions", &data, len)?;
                actions.extend(
                    data.chunks_exact(4)
                        .map(|c| MacroAction::decode([c[0], c[1], c[2], c[3]])),
                );
            }
            let raw_header = Some([header[0], header[1], header[2], header[3]]);
            slots.push(Macro {
                id,
                actions,
                raw_header,
            });
        }
        Ok(Self { slots })
    }

    /// Bytes counted against the capacity, as the site counts them:
    /// 4 per macro + 4 per action (the pointer table is not counted).
    pub fn used_bytes(&self) -> usize {
        self.slots.iter().map(|m| 4 + 4 * m.actions.len()).sum()
    }

    /// Ids 0-99 without duplicates (the 100-pointer table can hold nothing else).
    fn check_ids(&self) -> Result<()> {
        let mut seen = [false; MAX_MACRO_ID as usize + 1];
        for m in &self.slots {
            let Some(slot) = seen.get_mut(usize::from(m.id)) else {
                return Err(Error::Validation(format!(
                    "Macro {} : numéro hors limites (0–{MAX_MACRO_ID})",
                    m.id
                )));
            };
            if *slot {
                return Err(Error::Validation(format!("Macro {} en double", m.id)));
            }
            *slot = true;
        }
        Ok(())
    }

    /// UI limits: ids 0-99 without duplicates and [`Macros::used_bytes`] within
    /// `capacity` ([`MACRO_CAPACITY`] for this model).
    pub fn validate(&self, capacity: u16) -> Result<()> {
        self.check_ids()?;
        let used = self.used_bytes();
        if used > usize::from(capacity) {
            return Err(Error::Validation(format!(
                "Macros trop longues : {used} octets nécessaires, {capacity} disponibles \
                 (4 octets par macro et 4 par action)"
            )));
        }
        Ok(())
    }

    /// [`Macros::validate`] only when the macros differ from `baseline` (those read from
    /// the keyboard): what the keyboard itself holds is not refused.
    pub fn validate_changes(&self, baseline: &Macros, capacity: u16) -> Result<()> {
        if self == baseline {
            return Ok(());
        }
        self.validate(capacity)
    }

    /// [`Macros::validate`] then [`Macros::to_bytes`].
    pub fn encode(&self, capacity: u16) -> Result<(Vec<u8>, Vec<u8>)> {
        self.validate(capacity)?;
        self.to_bytes()
    }

    /// Returns `(table, data)`: the 400-byte pointer table (written at 0) and the macros
    /// (written at 400, in id order). Checks representability only (ids, 16-bit
    /// addresses), not the capacity.
    pub fn to_bytes(&self) -> Result<(Vec<u8>, Vec<u8>)> {
        self.check_ids()?;
        let mut sorted: Vec<&Macro> = self.slots.iter().filter(|m| m.is_written()).collect();
        sorted.sort_by_key(|m| m.id);
        let mut table = vec![0u8; MACRO_TABLE_LEN];
        let mut data = Vec::with_capacity(self.used_bytes());
        for m in sorted {
            let addr = usize::from(MACRO_DATA_START) + data.len();
            if addr + 4 + 4 * m.actions.len() > 0x1_0000 {
                return Err(Error::Validation(format!(
                    "Macros : {} octets, plus que l'espace d'adresses du clavier",
                    self.used_bytes()
                )));
            }
            let at = 4 * usize::from(m.id);
            table[at..at + 4].copy_from_slice(&(addr as u32).to_le_bytes());
            data.extend_from_slice(&m.header());
            for a in &m.actions {
                data.extend_from_slice(&a.encode());
            }
        }
        Ok((table, data))
    }
}
