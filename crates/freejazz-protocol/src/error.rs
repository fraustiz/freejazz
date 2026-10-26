use thiserror::Error;

/// Errors of the protocol layer. Nothing in this crate panics on link or input errors.
#[derive(Debug, Error)]
pub enum Error {
    /// No matching response after the first send and all retries.
    #[error("no response from the keyboard (timeout)")]
    Timeout,
    /// The command is on the deny-list and is never sent.
    #[error("command 0x{0:02X} is denied and was not sent")]
    Denied(u8),
    /// Error reported by the HID layer.
    #[error("HID error: {0}")]
    Hid(String),
    /// No answering vendor interface was found.
    #[error("keyboard not found")]
    NotFound,
    /// Invalid arguments given to a transfer, or a block of the wrong size.
    #[error("invalid transfer: {0}")]
    Invalid(String),
    /// A value refused before encoding (out of range, over capacity...). The message
    /// is French and meant for the user.
    #[error("{0}")]
    Validation(String),
    /// A backup file could not be written, read or understood. The message is French
    /// and meant for the user.
    #[error("{0}")]
    Backup(String),
    /// `GET_DEVICE_INFO` reports `firmwareStatus == 1`: the site reads nothing else.
    #[error("Le clavier attend une mise à jour de son micrologiciel : lecture interrompue")]
    UpdatePending,
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<hidapi::HidError> for Error {
    fn from(e: hidapi::HidError) -> Self {
        Error::Hid(e.to_string())
    }
}
