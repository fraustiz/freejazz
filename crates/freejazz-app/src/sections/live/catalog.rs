//! What the user can start or stack: the live effects with their parameters (the keys of
//! the JSON `params` the engine reads, see `live/effects/`).

use eframe::egui::{self, Ui, vec2};
use freejazz_protocol::blocks::Rgb;
use serde_json::{Value, json};

use crate::live::{param_color, param_f32};
use crate::sections::lighting::color_picker::{ColorPicker, hex, to_color32};
use crate::sections::lighting::effects::direction_name;
use crate::ui::shell::chip;
use crate::ui::theme::Palette;

pub enum Kind {
    Color(&'static str),
    Float {
        min: f32,
        max: f32,
        default: f32,
        suffix: &'static str,
    },
    /// A speed whose sign is the direction (wave, scrolling gradient): shown as a speed
    /// and « Vers la droite » / « Vers la gauche ».
    SignedSpeed {
        max: f32,
        default: f32,
    },
    Choice(&'static [(&'static str, &'static str)]),
}

pub struct Param {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
}

pub struct Info {
    pub id: &'static str,
    pub title: &'static str,
    pub blurb: &'static str,
    pub params: &'static [Param],
}

const ACCENT: &str = "#7c5cff";

const fn color(key: &'static str, label: &'static str) -> Param {
    Param {
        key,
        label,
        kind: Kind::Color(ACCENT),
    }
}

pub const SOLID: Info = Info {
    id: "solid",
    title: "Couleur fixe",
    blurb: "Une couleur unie.",
    params: &[color("color", "Couleur")],
};
pub const BREATHING: Info = Info {
    id: "breathing",
    title: "Respiration",
    blurb: "La couleur monte et descend doucement.",
    params: &[
        color("color", "Couleur"),
        Param {
            key: "period",
            label: "Durée d'un souffle",
            kind: Kind::Float {
                min: 1.0,
                max: 10.0,
                default: 4.0,
                suffix: " s",
            },
        },
    ],
};
pub const WAVE: Info = Info {
    id: "wave",
    title: "Vague",
    blurb: "Un arc-en-ciel qui traverse le clavier.",
    params: &[
        Param {
            key: "speed",
            label: "Vitesse",
            kind: Kind::SignedSpeed {
                max: 0.6,
                default: 0.15,
            },
        },
        Param {
            key: "spread",
            label: "Largeur de l'arc-en-ciel",
            kind: Kind::Float {
                min: 0.3,
                max: 3.0,
                default: 1.0,
                suffix: "",
            },
        },
    ],
};
pub const SCROLLING_GRADIENT: Info = Info {
    id: "scrolling_gradient",
    title: "Dégradé défilant",
    blurb: "Deux couleurs qui glissent sur le clavier.",
    params: &[
        color("color", "Couleur A"),
        Param {
            key: "color2",
            label: "Couleur B",
            kind: Kind::Color("#00c8ff"),
        },
        Param {
            key: "speed",
            label: "Vitesse",
            kind: Kind::SignedSpeed {
                max: 1.0,
                default: 0.2,
            },
        },
    ],
};
pub const STARS: Info = Info {
    id: "stars",
    title: "Étoiles",
    blurb: "Des touches qui scintillent chacune à son rythme.",
    params: &[Param {
        key: "color",
        label: "Couleur",
        kind: Kind::Color("#fff4d6"),
    }],
};
pub const REACTIVE: Info = Info {
    id: "reactive",
    title: "Réaction à la frappe",
    blurb: "Chaque touche s'allume quand on l'appuie, puis s'éteint.",
    params: &[
        color("color", "Couleur"),
        Param {
            key: "fade",
            label: "Durée de l'extinction",
            kind: Kind::Float {
                min: 0.1,
                max: 2.0,
                default: 0.5,
                suffix: " s",
            },
        },
    ],
};
pub const RIPPLE: Info = Info {
    id: "ripple",
    title: "Onde",
    blurb: "Une onde part de chaque touche appuyée et se propage aux voisines.",
    params: &[
        color("color", "Couleur"),
        Param {
            key: "speed",
            label: "Vitesse",
            kind: Kind::Float {
                min: 2.0,
                max: 20.0,
                default: 8.0,
                suffix: " touches/s",
            },
        },
        Param {
            key: "width",
            label: "Épaisseur",
            kind: Kind::Float {
                min: 0.3,
                max: 2.0,
                default: 0.9,
                suffix: " touche",
            },
        },
    ],
};
pub const DEPTH: Info = Info {
    id: "depth",
    title: "Profondeur",
    blurb: "La lumière suit l'enfoncement de la touche (la frappe, tant que le clavier ne donne pas la profondeur).",
    params: &[color("color", "Couleur")],
};
pub const HEATMAP: Info = Info {
    id: "heatmap",
    title: "Chaleur",
    blurb: "Les touches les plus utilisées chauffent, puis refroidissent.",
    params: &[Param {
        key: "cooling",
        label: "Refroidissement",
        kind: Kind::Float {
            min: 0.5,
            max: 10.0,
            default: 3.0,
            suffix: "",
        },
    }],
};
pub const FIRE: Info = Info {
    id: "fire",
    title: "Feu",
    blurb: "Des flammes montent du bas du clavier.",
    params: &[],
};
pub const AURORA: Info = Info {
    id: "aurora",
    title: "Aurore",
    blurb: "Des voiles de couleur qui ondulent lentement.",
    params: &[],
};
pub const MATRIX: Info = Info {
    id: "matrix",
    title: "Matrice",
    blurb: "Une pluie de caractères verts.",
    params: &[],
};
pub const EQUALIZER: Info = Info {
    id: "equalizer",
    title: "Égaliseur",
    blurb: "Le son joué sur le PC dessine des barres sur les colonnes du clavier.",
    params: &[],
};
pub const MUSIC: Info = Info {
    id: "music",
    title: "Musique",
    blurb: "Le clavier pulse au rythme du son joué sur le PC.",
    params: &[],
};
pub const GAUGE: Info = Info {
    id: "gauge",
    title: "Jauge système",
    blurb: "Le processeur ou la mémoire remplit le clavier.",
    params: &[
        Param {
            key: "metric",
            label: "Mesure",
            kind: Kind::Choice(&[("cpu", "Processeur"), ("ram", "Mémoire")]),
        },
        Param {
            key: "mode",
            label: "Affichage",
            kind: Kind::Choice(&[("bar", "Barre"), ("hue", "Teinte")]),
        },
    ],
};

/// The effects with their own card.
pub const CARDS: &[&Info] = &[
    &RIPPLE, &DEPTH, &HEATMAP, &FIRE, &AURORA, &MATRIX, &REACTIVE, &EQUALIZER, &MUSIC, &GAUGE,
];

/// The types a layer can have.
pub const LAYER_TYPES: &[&Info] = &[
    &SOLID,
    &BREATHING,
    &WAVE,
    &SCROLLING_GRADIENT,
    &STARS,
    &REACTIVE,
    &RIPPLE,
    &DEPTH,
    &HEATMAP,
    &FIRE,
    &AURORA,
    &MATRIX,
    &EQUALIZER,
    &MUSIC,
    &GAUGE,
];

pub fn find(id: &str) -> Option<&'static Info> {
    LAYER_TYPES.iter().copied().find(|i| i.id == id)
}

/// Which colour is being picked: the owner (a card id or a layer number) and the key.
#[derive(Default)]
pub struct Picking {
    pub picker: ColorPicker,
    pub open: Option<(String, &'static str)>,
}

fn parse_hex(text: &str) -> Rgb {
    crate::sections::lighting::color_picker::parse_hex(text).unwrap_or(Rgb {
        r: 0x7c,
        g: 0x5c,
        b: 0xff,
    })
}

/// Edits `params` for `info`; true when something changed. `owner` tells two pickers
/// apart.
pub fn params_ui(
    ui: &mut Ui,
    p: &Palette,
    info: &Info,
    params: &mut Value,
    owner: &str,
    picking: &mut Picking,
) -> bool {
    if !params.is_object() {
        *params = json!({});
    }
    let mut changed = false;
    for param in info.params {
        match &param.kind {
            Kind::Color(default) => {
                let mut c = param_color(params, param.key, parse_hex(default));
                let open = picking
                    .open
                    .as_ref()
                    .is_some_and(|(o, k)| o == owner && *k == param.key);
                ui.horizontal(|ui| {
                    ui.label(param.label);
                    let (rect, resp) =
                        ui.allocate_exact_size(vec2(40.0, 24.0), egui::Sense::click());
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(6), to_color32(c));
                    ui.painter().rect_stroke(
                        rect,
                        egui::CornerRadius::same(6),
                        egui::Stroke::new(
                            if open { 2.0 } else { 1.0 },
                            if open { p.accent } else { p.border },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    if resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        picking.open = if open {
                            None
                        } else {
                            Some((owner.to_owned(), param.key))
                        };
                    }
                });
                if open && picking.picker.show(ui, p, &mut c) {
                    params[param.key] = json!(hex(c));
                    changed = true;
                }
            }
            Kind::Float {
                min,
                max,
                default,
                suffix,
            } => {
                let mut v = param_f32(params, param.key, *default);
                ui.horizontal(|ui| {
                    ui.label(param.label);
                    ui.spacing_mut().slider_width = 180.0;
                    if ui
                        .add(egui::Slider::new(&mut v, *min..=*max).suffix(*suffix))
                        .changed()
                    {
                        params[param.key] = json!(v);
                        changed = true;
                    }
                });
            }
            Kind::SignedSpeed { max, default } => {
                let signed = param_f32(params, param.key, *default);
                let (mut speed, left) = (signed.abs(), signed < 0.0);
                let mut new_left = left;
                ui.horizontal_wrapped(|ui| {
                    ui.label(param.label);
                    ui.spacing_mut().slider_width = 180.0;
                    let moved = ui.add(egui::Slider::new(&mut speed, 0.02..=*max)).changed();
                    for (is_left, name) in [(false, direction_name(0)), (true, direction_name(1))] {
                        if chip(ui, p, name, left == is_left).clicked() {
                            new_left = is_left;
                        }
                    }
                    if moved || new_left != left {
                        params[param.key] = json!(if new_left { -speed } else { speed });
                        changed = true;
                    }
                });
            }
            Kind::Choice(options) => {
                let current = params
                    .get(param.key)
                    .and_then(Value::as_str)
                    .unwrap_or(options[0].0)
                    .to_owned();
                ui.horizontal(|ui| {
                    ui.label(param.label);
                    for (value, name) in *options {
                        if chip(ui, p, name, current == *value).clicked() {
                            params[param.key] = json!(value);
                            changed = true;
                        }
                    }
                });
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_layer_type_is_a_known_engine_effect_and_ids_are_unique() {
        for info in LAYER_TYPES {
            assert!(
                crate::live::create(info.id, &Value::Null).is_some(),
                "{}",
                info.id
            );
        }
        let mut ids: Vec<_> = LAYER_TYPES.iter().map(|i| i.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), LAYER_TYPES.len());
        for card in CARDS {
            assert!(find(card.id).is_some());
        }
    }

    #[test]
    fn the_signed_speed_params_are_the_direction_the_wave_reads() {
        // negative speed travels the other way: the pictures differ
        let at = |speed: f64| {
            let mut e = crate::live::create("wave", &json!({ "speed": speed })).unwrap();
            let mut out = [crate::live::BLACK; 128];
            let layout = crate::keyboard::Layout::ak980max();
            let ctx = crate::live::FrameCtx {
                t: 2.0,
                dt: 0.04,
                layout,
                presses: &[],
                depths: None,
                audio: None,
                system: None,
            };
            e.frame(&ctx, &mut out);
            out
        };
        assert_ne!(at(0.15), at(-0.15));
    }
}
