//! « Performance »: trigger point, Rapid Trigger and switch type of the selected keys,
//! dead zones of the whole keyboard (GameMode).

pub mod axis;
pub mod trigger;

use std::collections::BTreeSet;

use eframe::egui::{RichText, Ui};
use freejazz_protocol::blocks::game_mode::DEAD_ZONE_MAX_MM;
use freejazz_protocol::blocks::magnetic::ModelLimits;

use super::{error_line, heading, hint, mm_slider, row, switch};
use crate::app::AppState;
use crate::keyboard::Layout;
use crate::ui::shell::chip;
use crate::ui::theme::{self, Palette};
use trigger::{common, rapid_on, round2};

pub fn show(ui: &mut Ui, p: &Palette, state: &mut AppState, selected: &BTreeSet<u8>) {
    let (Some(edited), Some(base)) = (state.edited.as_mut(), state.snapshot.as_ref()) else {
        return;
    };
    let limits = ModelLimits::ak980max();
    let layout = Layout::ak980max();
    let keys: Vec<u8> = selected
        .iter()
        .copied()
        .filter(|&i| layout.key(i).is_some() && !edited.magnetic.0[usize::from(i)].is_empty())
        .collect();

    heading(ui, p, "Course des touches sélectionnées");
    if keys.is_empty() {
        hint(
            ui,
            p,
            "Sélectionnez une ou plusieurs touches sur le clavier (ou un groupe) pour régler \
             leur point de déclenchement.",
        );
    } else {
        let map = &mut edited.magnetic;
        let first = usize::from(keys[0]);
        let mixed = |ui: &mut Ui, same: bool| {
            if !same {
                ui.label(
                    RichText::new("valeurs différentes")
                        .font(theme::regular(12.0))
                        .color(p.text_faint),
                );
            }
        };

        // trigger point
        let shared = common(map, &keys, |k| k.trigger_mm);
        let mut v = shared.unwrap_or(map.0[first].trigger_mm);
        let changed = row(ui, p, "Point de déclenchement", |ui| {
            let c = mm_slider(ui, &mut v, limits.min_trigger_trip, limits.max_trigger_trip);
            mixed(ui, shared.is_some());
            c
        })
        .inner;
        if changed {
            trigger::set_trigger(map, &keys, v, limits);
        }

        // Rapid Trigger
        let shared = common(map, &keys, rapid_on);
        let mut on = shared.unwrap_or(false);
        let changed = row(ui, p, "Rapid Trigger", |ui| {
            let c = switch(ui, p, &mut on, true).changed();
            mixed(ui, shared.is_some());
            c
        })
        .inner;
        if changed {
            trigger::set_rapid(map, &keys, on, limits);
        }

        if keys.iter().any(|&i| rapid_on(&map.0[usize::from(i)])) {
            let range = limits
                .axis(map.0[first].axis)
                .map_or((0.01, 3.3), |a| (a.min_sensitivity, a.max_sensitivity));
            for (press, label) in [
                (true, "Sensibilité à l'appui"),
                (false, "Sensibilité au relâchement"),
            ] {
                let get = |k: &freejazz_protocol::blocks::magnetic::MagneticKey| {
                    if press {
                        k.press_rt_mm
                    } else {
                        k.release_rt_mm
                    }
                };
                let rt_keys: Vec<u8> = keys
                    .iter()
                    .copied()
                    .filter(|&i| rapid_on(&map.0[usize::from(i)]))
                    .collect();
                let shared = common(map, &rt_keys, get);
                let mut v = shared.unwrap_or_else(|| get(&map.0[usize::from(rt_keys[0])]));
                let changed = row(ui, p, label, |ui| {
                    let c = mm_slider(ui, &mut v, range.0, range.1);
                    mixed(ui, shared.is_some());
                    c
                })
                .inner;
                if changed {
                    trigger::set_sensitivity(map, &keys, press, v, limits);
                }
            }
            let shared = common(map, &keys, |k| k.whole_fast);
            let mut on = shared.unwrap_or(false);
            let changed = row(ui, p, "Rapid Trigger sur toute la course", |ui| {
                let c = switch(ui, p, &mut on, true).changed();
                mixed(ui, shared.is_some());
                c
            })
            .inner;
            if changed {
                trigger::set_whole_fast(map, &keys, on);
            }
        }

        // switch type
        let shared = common(map, &keys, |k| k.axis);
        let mut picked = None;
        row(ui, p, "Type de switch", |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (value, name) in axis::choices(limits) {
                if chip(ui, p, name, shared == Some(value)).clicked() {
                    picked = Some(value);
                }
            }
        });
        if let Some(value) = picked {
            axis::set_axis(map, &keys, value, limits);
        }
    }

    ui.add_space(8.0);
    heading(ui, p, "Zones mortes (tout le clavier)");
    hint(
        ui,
        p,
        "Course ignorée en haut et en bas de chaque touche. 0 mm : désactivée.",
    );
    let mut errors: Vec<String> = Vec::new();
    let game = &mut edited.game_mode;
    let (mut top, mut bottom) = (game.top_dead_zone_mm(), game.bottom_dead_zone_mm());
    if row(ui, p, "Zone morte haute", |ui| {
        mm_slider(ui, &mut top, 0.0, DEAD_ZONE_MAX_MM)
    })
    .inner
        && let Err(e) = game.set_top_dead_zone_mm(round2(top))
    {
        errors.push(e.to_string());
    }
    if row(ui, p, "Zone morte basse", |ui| {
        mm_slider(ui, &mut bottom, 0.0, DEAD_ZONE_MAX_MM)
    })
    .inner
        && let Err(e) = game.set_bottom_dead_zone_mm(round2(bottom))
    {
        errors.push(e.to_string());
    }

    // limits, on what changed only
    if let Err(e) = edited.magnetic.validate_changes(&base.magnetic, limits) {
        errors.push(e.to_string());
    }
    if !errors.is_empty() {
        ui.add_space(6.0);
        for e in errors {
            error_line(ui, p, &e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard::{Group, labels};

    #[test]
    fn zqsd_to_0_80_mm_is_valid_against_the_fixture() {
        let base = crate::mock::fixture();
        let mut edited = base.clone();
        let layout = Layout::ak980max();
        let keys: Vec<u8> = Group::Gaming
            .keys(layout, &labels::key_labels())
            .into_iter()
            .collect();
        assert!(keys.len() >= 4);
        let limits = ModelLimits::ak980max();
        trigger::set_trigger(&mut edited.magnetic, &keys, 0.8, limits);
        assert!(edited.validate_changes(&base).is_ok());
        assert!(edited.magnetic.encode(base.info.rt_precision).is_ok());
        assert_ne!(edited.magnetic, base.magnetic);
        // dead zones go through the setters
        edited.game_mode.set_top_dead_zone_mm(round2(0.3)).unwrap();
        assert_eq!(edited.game_mode.top_dead_zone_mm(), 0.3);
        assert!(edited.game_mode.set_top_dead_zone_mm(0.6).is_err());
    }
}
