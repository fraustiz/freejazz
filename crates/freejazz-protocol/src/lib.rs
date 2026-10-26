//! HID protocol of the Ajazz AK980 Max, as described in `docs/protocol.md`.

pub mod backup;
pub mod blocks;
pub mod calibration;
pub mod cmd;
pub mod error;
pub mod keyboard;
pub mod live;
pub mod packet;
pub mod reconnect;
pub mod transfer;
pub mod transport;

pub use error::{Error, Result};
pub use keyboard::{Keyboard, RawBlocks, Snapshot};
