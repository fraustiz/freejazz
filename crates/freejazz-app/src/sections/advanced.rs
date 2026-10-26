//! « Touches avancées »: DKS, Mod-Tap, Toggle, SOCD and Rappy Snappy. Pick keys on the
//! keyboard, pick a kind here; each writes the key entry and, for DKS, the DKS table.

pub mod dks;
pub mod mod_tap;
pub mod rs;
pub mod socd;
pub mod toggle;

use std::collections::BTreeSet;

use eframe::egui::{self, RichText, Ui};
use freejazz_protocol::Snapshot;
use freejazz_protocol::blocks::dks::DksTable;
use freejazz_protocol::blocks::keys::{KeyAction, KeyKind, KeyMap};
use freejazz_protocol::blocks::magnetic::ModelLimits;

use super::keys as keys_section;
use super::{error_line, heading, hint, mm_slider_step, row};
use crate::app::AppState;
use crate::keyboard::labels::{self, Labels, keycodes};
use crate::keyboard::{KeyDef, Layout};
use crate::ui::shell::chip;
use crate::ui::theme::{self, Palette};
use crate::ui::{ButtonKind, button};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Dks,
    ModTap,
    Toggle,
    Socd,
    Rs,
}

impl Tab {
    const ALL: [Tab; 5] = [Tab::Dks, Tab::ModTap, Tab::Toggle, Tab::Socd, Tab::Rs];

    fn title(self) -> &'static str {
        match self {
            Tab::Dks => "DKS",
            Tab::ModTap => "Mod-Tap",
            Tab::Toggle => "Toggle",
            Tab::Socd => "SOCD",
            Tab::Rs => "Rappy Snappy",
        }
    }
}

/// What the panel remembers between frames.
pub struct AdvancedUi {
    tab: Tab,
    mt_hold: u8,
    mt_tap: u8,
    mt_ms: u16,
    tgl: u8,
    socd_mode: u8,
    /// The selection the values above were last loaded for.
    loaded: BTreeSet<u8>,
    error: Option<String>,
}

impl Default for AdvancedUi {
    fn default() -> Self {
        Self {
            tab: Tab::default(),
            mt_hold: 0xE0, // Left Ctrl
            mt_tap: 0x39,  // Caps Lock
            mt_ms: mod_tap::DEFAULT_MS,
            tgl: 0x39,
            socd_mode: 3,
            loaded: BTreeSet::new(),
            error: None,
        }
    }
}

/// The DKS points are stored in tenths of a millimetre.
const DKS_STEP_MM: f64 = 0.1;

fn is_advanced(a: KeyAction) -> bool {
    matches!(
        a.kind(),
        KeyKind::Dks(_) | KeyKind::Mt(_) | KeyKind::Tgl(_) | KeyKind::Socd(_) | KeyKind::Rs(_)
    )
}

/// Gives `key` its own function back. A SOCD or RS pair loses both keys (changing one
/// clears the other on the site), a DKS slot is freed with its last key.
pub fn remove(keys: &mut KeyMap, table: &mut DksTable, layout: &Layout, key: u8) {
    let old = keys.0[usize::from(key)];
    keys.0[usize::from(key)] = KeyAction::default();
    match old.kind() {
        KeyKind::Socd(_) | KeyKind::Rs(_) => {
            for k in &layout.keys {
                if keys.0[usize::from(k.index)] == old {
                    keys.0[usize::from(k.index)] = KeyAction::default();
                }
            }
        }
        KeyKind::Dks(slot) => dks::release_if_unused(table, keys, slot),
        _ => {}
    }
}

fn usage_name(labels: &Labels, usage: u8) -> String {
    labels
        .hid(usage)
        .map(|l| l.replace('\n', " "))
        .unwrap_or_else(|| format!("0x{usage:02X}"))
}

/// A drop-down of keyboard usages; true when changed. `none` adds « Aucune ».
fn usage_picker(
    ui: &mut Ui,
    labels: &Labels,
    id: impl std::hash::Hash + std::fmt::Debug,
    usage: &mut u8,
    none: bool,
) -> bool {
    let mut changed = false;
    let text = if *usage == 0 && none {
        "Aucune".to_owned()
    } else {
        usage_name(labels, *usage)
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .width(190.0)
        .show_ui(ui, |ui| {
            if none && ui.selectable_label(*usage == 0, "Aucune").clicked() {
                *usage = 0;
                changed = true;
            }
            for k in keycodes().iter().filter(|k| k.page == "KEYBOARD") {
                let u = k.value as u8;
                if u != 0
                    && ui
                        .selectable_label(*usage == u, usage_name(labels, u))
                        .clicked()
                {
                    *usage = u;
                    changed = true;
                }
            }
        });
    changed
}

fn detail(labels: &Labels, a: KeyAction) -> String {
    match a.kind() {
        KeyKind::Mt([hold, tap, units]) => format!(
            "Mod-Tap : clic {}, maintien {} ({} ms)",
            usage_name(labels, tap),
            usage_name(labels, hold),
            u16::from(units) * 10
        ),
        KeyKind::Tgl(u) => format!("Toggle : {}", usage_name(labels, u)),
        KeyKind::Socd([m, a, b]) => format!(
            "SOCD {} / {} : {}",
            usage_name(labels, a),
            usage_name(labels, b),
            socd::MODES
                .iter()
                .find(|(v, _)| *v == m)
                .map_or("mode inconnu", |(_, n)| n)
        ),
        KeyKind::Rs([_, a, b]) => format!(
            "Rappy Snappy {} / {}",
            usage_name(labels, a),
            usage_name(labels, b)
        ),
        KeyKind::Dks(s) => format!("DKS, emplacement {s}"),
        _ => String::new(),
    }
}

pub fn show(
    ui: &mut Ui,
    p: &Palette,
    state: &mut AppState,
    selected: &BTreeSet<u8>,
    ui_state: &mut AdvancedUi,
) {
    let Some(edited) = state.edited.as_mut() else {
        return;
    };
    let layout = Layout::ak980max();
    let labels = labels::current();
    let keys: Vec<u8> = selected
        .iter()
        .copied()
        .filter(|&i| layout.key(i).is_some())
        .collect();

    // a new selection that already holds an advanced key shows its values
    if &ui_state.loaded != selected {
        ui_state.loaded = selected.clone();
        ui_state.error = None;
        if let Some(&first) = keys.first() {
            let a = edited.keys.0[usize::from(first)];
            if let Some((hold, tap, ms)) = mod_tap::parts(a) {
                (ui_state.mt_hold, ui_state.mt_tap, ui_state.mt_ms) = (hold, tap, ms);
                ui_state.tab = Tab::ModTap;
            } else if let Some(u) = toggle::usage(a) {
                ui_state.tgl = u;
                ui_state.tab = Tab::Toggle;
            } else if let Some(m) = socd::mode(a) {
                ui_state.socd_mode = m;
                ui_state.tab = Tab::Socd;
            } else {
                match a.kind() {
                    KeyKind::Dks(_) => ui_state.tab = Tab::Dks,
                    KeyKind::Rs(_) => ui_state.tab = Tab::Rs,
                    _ => {}
                }
            }
        }
    }

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for t in Tab::ALL {
            if chip(ui, p, t.title(), ui_state.tab == t).clicked() {
                ui_state.tab = t;
                ui_state.error = None;
            }
        }
    });
    ui.add_space(2.0);

    let mut chosen: Option<KeyAction> = None;
    match ui_state.tab {
        Tab::ModTap => {
            hint(
                ui,
                p,
                "Un clic envoie une touche, un maintien en envoie une autre.",
            );
            if keys.is_empty() {
                select_hint(ui, p);
            } else {
                row(ui, p, "Touche envoyée sur un clic", |ui| {
                    usage_picker(ui, &labels, "mt_tap", &mut ui_state.mt_tap, false)
                });
                row(ui, p, "Touche envoyée en maintien", |ui| {
                    usage_picker(ui, &labels, "mt_hold", &mut ui_state.mt_hold, false)
                });
                row(ui, p, "Durée du maintien", |ui| {
                    ui.add(
                        egui::Slider::new(&mut ui_state.mt_ms, mod_tap::MIN_MS..=mod_tap::MAX_MS)
                            .step_by(10.0)
                            .suffix(" ms"),
                    )
                });
                if assign_button(ui, p, keys.len()) {
                    chosen = Some(mod_tap::action(
                        ui_state.mt_hold,
                        ui_state.mt_tap,
                        ui_state.mt_ms,
                    ));
                }
            }
        }
        Tab::Toggle => {
            hint(
                ui,
                p,
                "Un clic maintient la touche enfoncée jusqu'au clic suivant ; un appui long \
                 se comporte normalement.",
            );
            if keys.is_empty() {
                select_hint(ui, p);
            } else {
                row(ui, p, "Touche à maintenir", |ui| {
                    usage_picker(ui, &labels, "tgl", &mut ui_state.tgl, false)
                });
                if assign_button(ui, p, keys.len()) {
                    chosen = Some(toggle::action(ui_state.tgl));
                }
            }
        }
        Tab::Socd | Tab::Rs => {
            let is_socd = ui_state.tab == Tab::Socd;
            hint(
                ui,
                p,
                if is_socd {
                    "Deux touches opposées (par exemple Q et D) : que se passe-t-il quand les \
                     deux sont enfoncées ?"
                } else {
                    "Deux touches opposées : la plus enfoncée l'emporte."
                },
            );
            let pair: Option<(&KeyDef, &KeyDef)> = match keys.as_slice() {
                [a, b] => layout.key(*a).zip(layout.key(*b)),
                _ => None,
            };
            match pair {
                None => hint(
                    ui,
                    p,
                    "Sélectionnez exactement deux touches sur le clavier.",
                ),
                Some((a, b)) => {
                    let (na, nb) = (usage_name(&labels, a.hid), usage_name(&labels, b.hid));
                    heading(ui, p, &format!("Paire : {na} et {nb}"));
                    if is_socd {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                            for (m, name) in socd::MODES {
                                if chip(ui, p, name, ui_state.socd_mode == m).clicked() {
                                    ui_state.socd_mode = m;
                                }
                            }
                        });
                    }
                    if button(ui, p, ButtonKind::Primary, None, "Créer la paire", true).clicked() {
                        // the two keys leave any pair or DKS they were in
                        for k in [a, b] {
                            remove(&mut edited.keys, &mut edited.dks, layout, k.index);
                        }
                        if is_socd {
                            socd::set(
                                &mut edited.keys,
                                &mut edited.magnetic,
                                a,
                                b,
                                ui_state.socd_mode,
                            );
                        } else {
                            rs::set(&mut edited.keys, &mut edited.magnetic, a, b);
                        }
                    }
                }
            }
        }
        Tab::Dks => dks_tab(ui, p, edited, &keys, &labels, ui_state),
    }

    if let Some(action) = chosen {
        let skipped =
            keys_section::assign_releasing(&mut edited.keys, &mut edited.dks, layout, &keys, action);
        if skipped > 0 {
            state.notice = Some(format!(
                "{skipped} touche(s) à action inconnue laissée(s) inchangée(s)"
            ));
        }
    }

    if let Some(e) = &ui_state.error {
        error_line(ui, p, e);
    }

    // the advanced keys the keyboard holds
    ui.add_space(6.0);
    let Some(edited) = state.edited.as_mut() else {
        return;
    };
    let list: Vec<(u8, String)> = layout
        .keys
        .iter()
        .filter(|k| is_advanced(edited.keys.0[usize::from(k.index)]))
        .map(|k| {
            (
                k.index,
                format!(
                    "{} : {}",
                    labels.default_label(k).replace('\n', " "),
                    detail(&labels, edited.keys.0[usize::from(k.index)])
                ),
            )
        })
        .collect();
    heading(ui, p, "Touches avancées sur le clavier");
    if list.is_empty() {
        hint(ui, p, "Aucune touche avancée.");
    }
    let mut delete = None;
    for (index, text) in &list {
        ui.horizontal(|ui| {
            if chip(ui, p, "Supprimer", false).clicked() {
                delete = Some(*index);
            }
            ui.label(RichText::new(text).font(theme::regular(13.0)).color(p.text));
        });
    }
    if let Some(index) = delete {
        remove(&mut edited.keys, &mut edited.dks, layout, index);
    }
}

fn select_hint(ui: &mut Ui, p: &Palette) {
    hint(
        ui,
        p,
        "Sélectionnez une ou plusieurs touches sur le clavier.",
    );
}

fn assign_button(ui: &mut Ui, p: &Palette, n: usize) -> bool {
    let label = if n == 1 {
        "Assigner à la touche".to_owned()
    } else {
        format!("Assigner aux {n} touches")
    };
    button(ui, p, ButtonKind::Primary, None, &label, true).clicked()
}

fn dks_tab(
    ui: &mut Ui,
    p: &Palette,
    edited: &mut Snapshot,
    keys: &[u8],
    labels: &Labels,
    ui_state: &mut AdvancedUi,
) {
    hint(
        ui,
        p,
        "Jusqu'à quatre touches envoyées selon la profondeur de l'appui.",
    );
    let [key] = keys else {
        if keys.is_empty() {
            select_hint(ui, p);
        } else {
            hint(ui, p, "Sélectionnez une seule touche pour son DKS.");
        }
        return;
    };
    let layout = Layout::ak980max();
    let def = layout.key(*key).expect("filtered above");
    let max_trip = ModelLimits::ak980max().max_trigger_trip;
    let slot = match edited.keys.0[usize::from(*key)].kind() {
        KeyKind::Dks(s) => Some(s),
        _ => None,
    };
    let entry = slot.and_then(|s| edited.dks.0.iter_mut().find(|e| e.index == s));
    let Some(entry) = entry else {
        if slot.is_some() {
            hint(ui, p, "Cette touche renvoie à un DKS absent de la table.");
        }
        if button(
            ui,
            p,
            ButtonKind::Primary,
            None,
            "Créer un DKS sur cette touche",
            true,
        )
        .clicked()
        {
            // out of any pair or previous DKS first (frees its slot when unused)
            remove(&mut edited.keys, &mut edited.dks, layout, *key);
            match dks::free_slot(&edited.dks, &edited.keys) {
                Some(s) => {
                    edited.dks.0.push(dks::new_entry(s, def.hid));
                    edited.keys.0[usize::from(*key)] = KeyAction::from_kind(KeyKind::Dks(s));
                    ui_state.error = None;
                }
                None => {
                    ui_state.error = Some("Les 64 emplacements DKS sont utilisés.".to_owned());
                }
            }
        }
        return;
    };

    heading(ui, p, &format!("DKS, emplacement {}", entry.index));
    let (mut start, mut bottom) = (
        f32::from(entry.make[0]) / 10.0,
        f32::from(entry.make[1]) / 10.0,
    );
    let mut moved = false;
    row(ui, p, "Début de course", |ui| {
        moved |= mm_slider_step(ui, &mut start, 0.1, (max_trip - 0.1).max(0.1), DKS_STEP_MM);
    });
    row(ui, p, "Fond de touche", |ui| {
        moved |= mm_slider_step(ui, &mut bottom, 0.2, max_trip, DKS_STEP_MM);
    });
    if moved {
        ui_state.error = dks::set_points(
            entry,
            (start * 10.0).round() as u8,
            (bottom * 10.0).round() as u8,
            max_trip,
        )
        .err();
    }

    egui::Grid::new("dks_grid")
        .num_columns(5)
        .spacing([8.0, 8.0])
        .show(ui, |ui| {
            ui.label("");
            for stage in dks::STAGES {
                hint(ui, p, stage);
            }
            ui.end_row();
            for action in 0..4 {
                usage_picker(
                    ui,
                    labels,
                    ("dks_action", action),
                    &mut entry.actions[action],
                    true,
                );
                for stage in 0..4 {
                    let v = entry.states[action][stage];
                    let text = match v {
                        0 => "—",
                        v => dks::CELL_NAMES[usize::from(v)],
                    };
                    if chip(ui, p, text, v != 0).clicked() {
                        dks::cycle(entry, action, stage);
                    }
                }
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_of(hid: u8) -> u8 {
        Layout::ak980max()
            .keys
            .iter()
            .find(|k| k.hid == hid)
            .unwrap()
            .index
    }

    #[test]
    fn removing_one_key_of_a_pair_clears_the_other() {
        let layout = Layout::ak980max();
        let (q, d) = (index_of(0x14), index_of(0x07));
        let mut keys = KeyMap::default();
        let mut table = DksTable::default();
        let mut mag = freejazz_protocol::blocks::magnetic::MagneticMap::default();
        let (kq, kd) = (layout.key(q).unwrap(), layout.key(d).unwrap());
        socd::set(&mut keys, &mut mag, kq, kd, 3);
        remove(&mut keys, &mut table, layout, q);
        assert_eq!(keys, KeyMap::default());
    }

    #[test]
    fn assigning_to_a_paired_or_dks_key_releases_it_first() {
        let layout = Layout::ak980max();
        let (q, d, space) = (index_of(0x14), index_of(0x07), index_of(0x2C));
        let (mut keys, mut table) = (KeyMap::default(), DksTable(vec![dks::new_entry(0, 0x2C)]));
        let mut mag = freejazz_protocol::blocks::magnetic::MagneticMap::default();
        socd::set(&mut keys, &mut mag, layout.key(q).unwrap(), layout.key(d).unwrap(), 3);
        keys.0[usize::from(space)] = KeyAction::from_kind(KeyKind::Dks(0));
        let volume = KeyAction::from_kind(KeyKind::Consumer(0xE9));
        // one key of the pair: the other goes back to its default, no half pair
        keys_section::assign_releasing(&mut keys, &mut table, layout, &[q], volume);
        assert_eq!(keys.0[usize::from(q)], volume);
        assert_eq!(keys.0[usize::from(d)], KeyAction::default());
        // the last key of a DKS slot: the slot is freed
        keys_section::assign_releasing(&mut keys, &mut table, layout, &[space], volume);
        assert!(table.0.is_empty());
        assert_eq!(keys.0[usize::from(space)], volume);
        // Mod-Tap from the advanced tab goes through the same path
        socd::set(&mut keys, &mut mag, layout.key(q).unwrap(), layout.key(d).unwrap(), 3);
        let mt = mod_tap::action(0xE0, 0x39, 400);
        keys_section::assign_releasing(&mut keys, &mut table, layout, &[d], mt);
        assert_eq!(keys.0[usize::from(d)], mt);
        assert_eq!(keys.0[usize::from(q)], KeyAction::default());
    }

    #[test]
    fn removing_a_dks_key_frees_its_slot() {
        let layout = Layout::ak980max();
        let k = index_of(0x2C);
        let mut keys = KeyMap::default();
        let mut table = DksTable(vec![dks::new_entry(0, 0x2C)]);
        keys.0[usize::from(k)] = KeyAction::from_kind(KeyKind::Dks(0));
        remove(&mut keys, &mut table, layout, k);
        assert!(table.0.is_empty());
        assert_eq!(keys.0[usize::from(k)], KeyAction::default());
    }

    #[test]
    fn a_mod_tap_on_caps_lock_passes_the_snapshot_checks() {
        let base = crate::mock::fixture();
        let caps = index_of(0x39);
        let mut edited = base.clone();
        let a = mod_tap::action(0xE0, 0x39, 400);
        assert_eq!(keys_section::assign(&mut edited.keys, &[caps], a), 0);
        assert!(edited.validate_changes(&base).is_ok());
        remove(&mut edited.keys, &mut edited.dks, Layout::ak980max(), caps);
        assert_eq!(edited.keys.0[usize::from(caps)], KeyAction::default());
    }
}
