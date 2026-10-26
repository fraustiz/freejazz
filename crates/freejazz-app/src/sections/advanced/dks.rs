//! DKS (dynamic keystroke, protocol.md 6.8): up to four actions on four travel stages.

use freejazz_protocol::blocks::dks::{DksEntry, DksTable, MAX_DKS_SLOT};
use freejazz_protocol::blocks::keys::{KeyKind, KeyMap};

pub const STAGES: [&str; 4] = [
    "Début de course",
    "Fond de touche",
    "Remontée depuis le fond",
    "Fin de remontée",
];

/// Cell values: bit 0 = triggered, bit 1 = held.
pub const CELL_NAMES: [&str; 4] = ["", "Déclenche", "Maintient", "Les deux"];

/// The first slot neither the table nor a key refers to.
pub fn free_slot(table: &DksTable, keys: &KeyMap) -> Option<u8> {
    (0..=MAX_DKS_SLOT).find(|&s| {
        let in_table = table.0.iter().any(|e| e.index == s && !e.is_empty());
        let used = keys
            .0
            .iter()
            .any(|a| matches!(a.kind(), KeyKind::Dks(n) if n == s));
        !in_table && !used
    })
}

/// A new entry for `hid`: the key itself as action 1, pressed at the start of the
/// travel and held to the bottom. UNVERIFIED-HW: the stage semantics are the site's
/// reading of the bytes, not observed on the keyboard.
pub fn new_entry(slot: u8, hid: u8) -> DksEntry {
    let mut e = DksEntry {
        index: slot,
        make: [16, 30],
        brk: [30, 16],
        actions: [hid, 0, 0, 0],
        ..Default::default()
    };
    e.states[0] = [3, 2, 0, 0];
    e
}

/// Sets the press points (0.1 mm); the release points mirror them like the site's.
pub fn set_points(e: &mut DksEntry, start: u8, bottom: u8, max_trip_mm: f32) -> Result<(), String> {
    let max_start = (max_trip_mm * 10.0).round() as u8 - 1;
    if bottom < 2 || start >= bottom || start > max_start {
        return Err(format!(
            "Le point de départ doit être avant le fond (départ au plus {:.1} mm, fond d'au moins 0,2 mm)",
            f32::from(max_start) / 10.0
        ));
    }
    e.make = [start, bottom];
    e.brk = [bottom, start];
    Ok(())
}

/// Next value of a grid cell: none, triggers, holds, both.
pub fn cycle(e: &mut DksEntry, action: usize, stage: usize) {
    let v = &mut e.states[action][stage];
    *v = (*v + 1) % 4;
}

/// Frees `slot` when no key uses it any more.
pub fn release_if_unused(table: &mut DksTable, keys: &KeyMap, slot: u8) {
    let used = keys
        .0
        .iter()
        .any(|a| matches!(a.kind(), KeyKind::Dks(n) if n == slot));
    if !used {
        table.0.retain(|e| e.index != slot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freejazz_protocol::blocks::keys::KeyAction;

    #[test]
    fn slots_are_handed_out_in_order() {
        let mut table = DksTable::default();
        let mut keys = KeyMap::default();
        assert_eq!(free_slot(&table, &keys), Some(0));
        table.0.push(new_entry(0, 4));
        assert_eq!(free_slot(&table, &keys), Some(1));
        keys.0[5] = KeyAction::from_kind(KeyKind::Dks(1));
        assert_eq!(free_slot(&table, &keys), Some(2));
    }

    #[test]
    fn all_64_slots_can_run_out() {
        let mut table = DksTable::default();
        for s in 0..=MAX_DKS_SLOT {
            table.0.push(new_entry(s, 4));
        }
        assert_eq!(free_slot(&table, &KeyMap::default()), None);
        assert!(table.encode().is_ok());
    }

    #[test]
    fn points_follow_the_sites_rules() {
        let mut e = new_entry(0, 4);
        assert!(set_points(&mut e, 10, 25, 3.8).is_ok());
        assert_eq!((e.make, e.brk), ([10, 25], [25, 10]));
        assert!(set_points(&mut e, 25, 10, 3.8).is_err());
        assert!(set_points(&mut e, 0, 1, 3.8).is_err());
        assert!(set_points(&mut e, 38, 39, 3.8).is_err());
    }

    #[test]
    fn cells_cycle_and_the_entry_encodes() {
        let mut e = new_entry(3, 0x2C);
        for _ in 0..4 {
            cycle(&mut e, 1, 2);
        }
        assert_eq!(e.states[1][2], 0);
        cycle(&mut e, 1, 2);
        assert_eq!(e.states[1][2], 1);
        let table = DksTable(vec![e.clone()]);
        let decoded = DksTable::decode(&table.encode().unwrap()).unwrap();
        assert_eq!(decoded.0, vec![e]);
    }

    #[test]
    fn a_slot_is_released_with_its_last_key() {
        let mut table = DksTable(vec![new_entry(2, 4)]);
        let mut keys = KeyMap::default();
        keys.0[1] = KeyAction::from_kind(KeyKind::Dks(2));
        keys.0[2] = KeyAction::from_kind(KeyKind::Dks(2));
        keys.0[1] = KeyAction::default();
        release_if_unused(&mut table, &keys, 2);
        assert_eq!(table.0.len(), 1);
        keys.0[2] = KeyAction::default();
        release_if_unused(&mut table, &keys, 2);
        assert!(table.0.is_empty());
    }
}
