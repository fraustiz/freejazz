//! Toggle (`0A usage 00 00`): a click switches the held state of `usage`.

use freejazz_protocol::blocks::keys::{KeyAction, KeyKind};

pub fn action(usage: u8) -> KeyAction {
    KeyAction::from_kind(KeyKind::Tgl(usage))
}

pub fn usage(action: KeyAction) -> Option<u8> {
    match action.kind() {
        KeyKind::Tgl(u) => Some(u),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        assert_eq!(action(0x39).raw, [0x0A, 0x39, 0, 0]);
        assert_eq!(usage(action(0x39)), Some(0x39));
        assert_eq!(usage(KeyAction::default()), None);
    }
}
