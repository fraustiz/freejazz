//! Typed blocks of `docs/protocol.md` section 6: decoding the bytes read from the
//! keyboard and encoding them back. All types share one serde JSON shape
//! (camelCase), used by the backup file and by the front end.

pub mod device_info;
pub mod dks;
pub mod game_mode;
pub mod keys;
pub mod led;
pub mod macros;
pub mod magnetic;

pub use device_info::DeviceInfo;
pub use dks::{DksEntry, DksTable};
pub use game_mode::GameMode;
pub use keys::{KeyAction, KeyKind, KeyMap};
pub use led::{CustomLeds, LedEffect, Rgb};
pub use macros::{MACRO_CAPACITY, Macro, MacroAction, Macros};
pub use magnetic::{AxisLimits, MagneticKey, MagneticMap, ModelLimits};

use crate::error::{Error, Result};

/// Number of key slots in the key, lighting and magnetic blocks.
pub const KEY_SLOTS: usize = 128;

/// Errors unless `bytes` is exactly `len` long.
pub(crate) fn check_len(block: &str, bytes: &[u8], len: usize) -> Result<()> {
    if bytes.len() == len {
        Ok(())
    } else {
        Err(Error::Invalid(format!(
            "{block}: {} bytes, expected {len}",
            bytes.len()
        )))
    }
}

/// A French-formatted number of millimetres ("1,205").
pub(crate) fn mm(v: f32) -> String {
    format!("{v}").replace('.', ",")
}

/// A French-formatted limit with two decimals ("0,10").
pub(crate) fn mm2(v: f32) -> String {
    format!("{v:.2}").replace('.', ",")
}

/// Serde for `[T; N]` of any length (serde derives only up to 32), as a JSON array.
pub(crate) mod fixed_array {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer, T: Serialize, const N: usize>(
        a: &[T; N],
        s: S,
    ) -> Result<S::Ok, S::Error> {
        s.collect_seq(a)
    }

    pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>, const N: usize>(
        d: D,
    ) -> Result<[T; N], D::Error> {
        let v = Vec::<T>::deserialize(d)?;
        let len = v.len();
        v.try_into()
            .map_err(|_| D::Error::invalid_length(len, &format!("{N} elements").as_str()))
    }
}
