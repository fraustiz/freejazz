//! « Touches »: what each key sends. Pick keys on the keyboard, pick an action here.

pub mod catalog;
pub mod picker;

use std::collections::BTreeSet;

use eframe::egui::{RichText, Ui};
use freejazz_protocol::blocks::dks::DksTable;
use freejazz_protocol::blocks::keys::{KeyAction, KeyKind, KeyMap};

use super::advanced;
use super::{heading, hint};
use crate::app::AppState;
use crate::keyboard::labels::{self, Labels};
use crate::keyboard::{KeyDef, Layout};
use crate::ui::theme::{self, Palette};
use crate::ui::{ButtonKind, button};
use catalog::{DISABLED, Entry, is_remapped};
use picker::Tab;

/// What the panel remembers between frames.
#[derive(Default)]
pub struct KeysUi {
    tab: Tab,
    search: String,
    /// The selection a « Par défaut » confirmation is open for.
    confirm: Option<BTreeSet<u8>>,
}

/// Gives `action` to `keys`. With several keys, a key holding an action the app does
/// not understand is left alone (returned count); a single selected key is a deliberate
/// choice.
pub fn assign(map: &mut KeyMap, keys: &[u8], action: KeyAction) -> usize {
    let mut skipped = 0;
    for &i in keys {
        let slot = &mut map.0[usize::from(i)];
        if keys.len() > 1 && matches!(slot.kind(), KeyKind::Unknown(_)) {
            skipped += 1;
        } else {
            *slot = action;
        }
    }
    skipped
}

/// [`assign`], first releasing each key from the SOCD / RS pair or the DKS slot it
/// belongs to ([`advanced::remove`]: both halves of a pair are cleared, a DKS slot no
/// key uses any more is freed), so that no half pair or unused slot is left behind.
pub fn assign_releasing(
    map: &mut KeyMap,
    table: &mut DksTable,
    layout: &Layout,
    keys: &[u8],
    action: KeyAction,
) -> usize {
    let mut skipped = 0;
    for &i in keys {
        if keys.len() > 1 && matches!(map.0[usize::from(i)].kind(), KeyKind::Unknown(_)) {
            skipped += 1;
        } else {
            advanced::remove(map, table, layout, i);
            map.0[usize::from(i)] = action;
        }
    }
    skipped
}

/// One line of the « Par défaut » confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResetLine {
    pub key: String,
    pub now: String,
    pub after: String,
}

/// The keys that would change when reset, with what they send now and afterwards.
pub fn reset_plan(
    map: &KeyMap,
    keys: &[u8],
    layout: &Layout,
    labels: &Labels,
) -> Vec<(u8, ResetLine)> {
    keys.iter()
        .filter_map(|&i| {
            let key = layout.key(i)?;
            let action = map.0[usize::from(i)];
            (action != KeyAction::default()).then(|| {
                let own = labels.default_label(key).replace('\n', " ");
                (
                    i,
                    ResetLine {
                        key: own.clone(),
                        now: labels.key_label(key, action).replace('\n', " "),
                        after: own,
                    },
                )
            })
        })
        .collect()
}

/// Number of remapped keys, for the summary.
pub fn remapped_count(map: &KeyMap, layout: &Layout) -> usize {
    layout
        .keys
        .iter()
        .filter(|k| is_remapped(map.0[usize::from(k.index)], k))
        .count()
}

/// Colour of a remapped keycap.
pub fn tint(p: &Palette) -> eframe::egui::Color32 {
    if p.dark {
        eframe::egui::Color32::from_rgb(0x3a, 0x30, 0x7a)
    } else {
        eframe::egui::Color32::from_rgb(0xdc, 0xd3, 0xff)
    }
}

pub fn show(
    ui: &mut Ui,
    p: &Palette,
    state: &mut AppState,
    selected: &BTreeSet<u8>,
    ui_state: &mut KeysUi,
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

    if keys.is_empty() {
        heading(ui, p, "Touches réassignées");
        let n = remapped_count(&edited.keys, layout);
        hint(
            ui,
            p,
            &match n {
                0 => "Aucune touche réassignée.".to_owned(),
                1 => "1 touche réassignée, colorée sur le clavier.".to_owned(),
                n => format!("{n} touches réassignées, colorées sur le clavier."),
            },
        );
        hint(
            ui,
            p,
            "Sélectionnez une ou plusieurs touches sur le clavier pour changer ce qu'elles envoient.",
        );
        return;
    }

    // what the selection sends now
    let summary = match keys.as_slice() {
        [one] => {
            let key = layout.key(*one).expect("filtered above");
            let own = labels.default_label(key).replace('\n', " ");
            let now = labels
                .key_label(key, edited.keys.0[usize::from(*one)])
                .replace('\n', " ");
            format!("Touche {own} : envoie {now}")
        }
        many => format!("{} touches sélectionnées", many.len()),
    };
    heading(ui, p, &summary);
    if keys
        .iter()
        .any(|&i| matches!(edited.keys.0[usize::from(i)].kind(), KeyKind::Unknown(_)))
    {
        hint(
            ui,
            p,
            "Action inconnue : Freejazz la conserve telle quelle tant que vous ne la remplacez pas.",
        );
    }
    ui.add_space(2.0);
    picker::tabs(ui, p, &mut ui_state.tab);
    ui.add_space(4.0);

    let current = common_action(&edited.keys, &keys);
    let mut chosen: Option<KeyAction> = None;
    match ui_state.tab {
        tab @ (Tab::Keyboard | Tab::Media | Tab::Mouse | Tab::Macros) => {
            let list: Vec<Entry> = match tab {
                Tab::Keyboard => catalog::keyboard(&labels),
                Tab::Media => catalog::media(),
                Tab::Mouse => catalog::mouse(),
                _ => catalog::macros(&edited.macros),
            };
            if tab == Tab::Keyboard {
                picker::search(ui, &mut ui_state.search);
            }
            if list.is_empty() {
                hint(
                    ui,
                    p,
                    "Aucune macro enregistrée : créez-en dans la section Macros.",
                );
            }
            let filter = if tab == Tab::Keyboard {
                ui_state.search.as_str()
            } else {
                ""
            };
            chosen = picker::entries(ui, p, &list, current, filter);
        }
        Tab::Disable => {
            hint(
                ui,
                p,
                "La touche n'envoie plus rien (elle reste réactivable à tout moment).",
            );
            if button(
                ui,
                p,
                ButtonKind::Secondary,
                None,
                "Désactiver la touche",
                true,
            )
            .clicked()
            {
                chosen = Some(DISABLED);
            }
        }
        Tab::Default => {
            let plan = reset_plan(&edited.keys, &keys, layout, &labels);
            if plan.is_empty() {
                hint(ui, p, "Ces touches ont déjà leur fonction d'origine.");
            } else {
                hint(
                    ui,
                    p,
                    "Chaque touche retrouve la fonction de son emplacement sur le clavier.",
                );
                if ui_state.confirm.as_ref() != Some(selected)
                    && button(
                        ui,
                        p,
                        ButtonKind::Secondary,
                        None,
                        "Rétablir la fonction d'origine",
                        true,
                    )
                    .clicked()
                {
                    ui_state.confirm = Some(selected.clone());
                }
                if ui_state.confirm.as_ref() == Some(selected) {
                    heading(ui, p, "Confirmer le retour à la fonction d'origine ?");
                    for (_, line) in plan.iter().take(8) {
                        ui.label(
                            RichText::new(format!(
                                "{} : envoie {}, enverra {}",
                                line.key, line.now, line.after
                            ))
                            .font(theme::regular(13.0))
                            .color(p.text),
                        );
                    }
                    if plan.len() > 8 {
                        hint(ui, p, &format!("… et {} autres touches", plan.len() - 8));
                    }
                    ui.horizontal(|ui| {
                        if button(ui, p, ButtonKind::Primary, None, "Confirmer", true).clicked() {
                            chosen = Some(KeyAction::default());
                            ui_state.confirm = None;
                        }
                        if button(ui, p, ButtonKind::Secondary, None, "Annuler", true).clicked() {
                            ui_state.confirm = None;
                        }
                    });
                }
            }
        }
    }
    // a confirmation belongs to its selection and its tab
    if ui_state.tab != Tab::Default || ui_state.confirm.as_ref() != Some(selected) {
        ui_state.confirm = None;
    }

    if let Some(action) = chosen {
        let skipped =
            assign_releasing(&mut edited.keys, &mut edited.dks, layout, &keys, action);
        if skipped > 0 {
            state.notice = Some(format!(
                "{skipped} touche(s) à action inconnue laissée(s) inchangée(s)"
            ));
        }
    }
}

/// The action all the keys hold, if they hold the same one.
fn common_action(map: &KeyMap, keys: &[u8]) -> Option<KeyAction> {
    let first = map.0[usize::from(*keys.first()?)];
    keys.iter()
        .all(|&i| map.0[usize::from(i)] == first)
        .then_some(first)
}

/// Look of a key for the keyboard view: its legend and, when remapped, the tint.
pub fn look(
    key: &KeyDef,
    action: KeyAction,
    labels: &Labels,
    p: &Palette,
) -> crate::keyboard::KeyLook {
    let mut look = crate::keyboard::KeyLook::label(labels.key_label(key, action));
    if is_remapped(action, key) {
        look.fill = Some(tint(p));
    }
    look
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (freejazz_protocol::Snapshot, &'static Layout, Labels) {
        (
            crate::mock::fixture(),
            Layout::ak980max(),
            Labels::build(|_| None),
        )
    }

    fn index_of(layout: &Layout, hid: u8) -> u8 {
        layout.keys.iter().find(|k| k.hid == hid).unwrap().index
    }

    #[test]
    fn f12_to_volume_up_then_back_validates() {
        let (base, layout, _) = fixture();
        let f12 = index_of(layout, 0x45);
        let mut edited = base.clone();
        let volume = KeyAction::from_kind(KeyKind::Consumer(0xE9));
        assert_eq!(assign(&mut edited.keys, &[f12], volume), 0);
        assert_eq!(edited.keys.0[usize::from(f12)].raw, [3, 0xE9, 0, 0]);
        assert!(edited.validate_changes(&base).is_ok());
        assert!(edited.keys.encode().len() == 512);
        assign(&mut edited.keys, &[f12], base.keys.0[usize::from(f12)]);
        assert_eq!(edited.keys, base.keys);
    }

    #[test]
    fn unknown_actions_are_left_alone_in_a_group_edit() {
        let (base, layout, _) = fixture();
        let (a, b) = (index_of(layout, 0x04), index_of(layout, 0x05));
        let mut map = base.keys.clone();
        map.0[usize::from(a)] = KeyAction {
            raw: [0x44, 1, 2, 3],
        };
        let skipped = assign(&mut map, &[a, b], DISABLED);
        assert_eq!(skipped, 1);
        assert_eq!(map.0[usize::from(a)].raw, [0x44, 1, 2, 3]);
        assert_eq!(map.0[usize::from(b)], DISABLED);
        // chosen on its own, it is replaced
        assert_eq!(assign(&mut map, &[a], DISABLED), 0);
        assert_eq!(map.0[usize::from(a)], DISABLED);
    }

    #[test]
    fn the_reset_names_what_the_key_sends_afterwards() {
        let (base, layout, labels) = fixture();
        let all: Vec<u8> = layout.keys.iter().map(|k| k.index).collect();
        let plan = reset_plan(&base.keys, &all, layout, &labels);
        // the fixture holds the tested unit's remapped keys
        assert!(!plan.is_empty());
        for (i, line) in &plan {
            let key = layout.key(*i).unwrap();
            assert_eq!(line.after, labels.default_label(key).replace('\n', " "));
            assert_ne!(base.keys.0[usize::from(*i)], KeyAction::default());
        }
        let mut map = base.keys.clone();
        assign(&mut map, &all, KeyAction::default());
        assert!(map.0.iter().all(|a| *a == KeyAction::default()));
        assert!(reset_plan(&map, &all, layout, &labels).is_empty());
    }

    #[test]
    fn remapped_keys_are_tinted() {
        let (base, layout, labels) = fixture();
        let n = remapped_count(&base.keys, layout);
        assert!(n >= 6, "{n}");
        let p = Palette::DARK;
        let key = layout.key(99).unwrap();
        let look = look(key, base.keys.0[99], &labels, &p);
        assert_eq!(look.fill, Some(tint(&p)));
        assert_ne!(look.label, "Action inconnue");
    }
}
