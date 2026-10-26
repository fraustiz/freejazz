//! The editor of custom effects: a name and a stack of layers (type, parameters, keys),
//! previewed live in the keyboard view.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use eframe::egui::{self, CornerRadius, RichText, Sense, Ui, vec2};
use egui_phosphor::regular as i;
use serde_json::json;

use super::catalog::{self, Picking};
use crate::keyboard::{Group, Layout, labels};
use crate::live::{CustomEffect, Frame, Layer, Renderer};
use crate::sections::{heading, hint};
use crate::ui::shell::chip;
use crate::ui::theme::{self, Palette};
use crate::ui::{ButtonKind, button, card};

enum Op {
    Up(usize),
    Down(usize),
    Remove(usize),
}

pub struct Editor {
    name: String,
    layers: Vec<Layer>,
    /// The preview must be rebuilt from `layers`.
    dirty: bool,
    renderer: Option<Renderer>,
    started: Instant,
    save: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            name: String::new(),
            layers: Vec::new(),
            dirty: true,
            renderer: None,
            started: Instant::now(),
            save: false,
        }
    }
}

fn icon_button(ui: &mut Ui, p: &Palette, icon: &str, tip: &str, enabled: bool) -> egui::Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), sense);
    if resp.hovered() && enabled {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), p.surface_hi);
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        theme::icon(16.0),
        if enabled { p.text } else { p.text_faint },
    );
    resp.on_hover_text(tip)
}

impl Editor {
    pub fn load(&mut self, effect: &CustomEffect) {
        self.name = effect.name.clone();
        self.layers = effect.layers.clone();
        self.dirty = true;
    }

    pub fn saved(&mut self, _name: &str) {}

    pub fn take_save_request(&mut self) -> bool {
        std::mem::take(&mut self.save)
    }

    /// The effect being edited, if it can be saved.
    pub fn to_effect(&self) -> Result<CustomEffect, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err("Donnez un nom à l'effet".into());
        }
        if self.layers.is_empty() {
            return Err("Ajoutez au moins un calque".into());
        }
        Ok(CustomEffect {
            name: name.to_owned(),
            layers: self.layers.clone(),
        })
    }

    /// The preview frame for now (the same renderer as the engine, no presses, audio or
    /// system readings), and a repaint request while it shows.
    pub fn preview(&mut self, ctx: &egui::Context) -> Option<Frame> {
        if self.layers.is_empty() {
            self.renderer = None;
            return None;
        }
        if self.dirty || self.renderer.is_none() {
            let effect = CustomEffect {
                name: self.name.clone(),
                layers: self.layers.clone(),
            };
            self.renderer = Some(Renderer::new(effect.build()));
            self.dirty = false;
        }
        let t = self.started.elapsed().as_secs_f32();
        let frame = self
            .renderer
            .as_mut()
            .map(|r| r.render(t, &[], None, None, None));
        ctx.request_repaint_after(Duration::from_millis(40));
        frame
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        p: &Palette,
        selected: &BTreeSet<u8>,
        picking: &mut Picking,
    ) {
        let layout = Layout::ak980max();
        let on_board: BTreeSet<u8> = selected
            .iter()
            .copied()
            .filter(|&k| layout.key(k).is_some())
            .collect();
        hint(
            ui,
            p,
            "Empilez des calques : chacun peint ses touches, le dernier est au-dessus. \
             L'aperçu s'affiche sur le clavier ; les effets liés au son, à la frappe ou au \
             système ne réagissent que sur le vrai clavier.",
        );
        ui.horizontal(|ui| {
            ui.label("Nom");
            if ui
                .add(egui::TextEdit::singleline(&mut self.name).hint_text("Mon effet"))
                .changed()
            {
                self.dirty = true;
            }
        });

        let mut op = None;
        let count = self.layers.len();
        let by_index = labels::current().for_layout(layout);
        for (idx, layer) in self.layers.iter_mut().enumerate() {
            let Some(info) = catalog::find(&layer.effect) else {
                continue;
            };
            card(p).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let place = if idx == 0 {
                        " (fond)"
                    } else if idx + 1 == count {
                        " (dessus)"
                    } else {
                        ""
                    };
                    ui.label(
                        RichText::new(format!("Calque {} · {}{place}", idx + 1, info.title))
                            .font(theme::strong(14.0))
                            .color(p.text),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, p, i::TRASH, "Retirer le calque", true).clicked() {
                            op = Some(Op::Remove(idx));
                        }
                        if icon_button(ui, p, i::ARROW_DOWN, "Descendre", idx + 1 < count).clicked()
                        {
                            op = Some(Op::Down(idx));
                        }
                        if icon_button(ui, p, i::ARROW_UP, "Monter", idx > 0).clicked() {
                            op = Some(Op::Up(idx));
                        }
                    });
                });
                if catalog::params_ui(
                    ui,
                    p,
                    info,
                    &mut layer.params,
                    &format!("layer{idx}"),
                    picking,
                ) {
                    self.dirty = true;
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("Touches : {}", layer.keys.len()));
                    if chip(
                        ui,
                        p,
                        &format!("Sélection du clavier ({})", on_board.len()),
                        !on_board.is_empty() && layer.keys == on_board,
                    )
                    .clicked()
                        && !on_board.is_empty()
                    {
                        layer.keys = on_board.clone();
                        self.dirty = true;
                    }
                    for group in Group::ALL {
                        let keys = group.keys(layout, &by_index);
                        if chip(ui, p, group.title(), !keys.is_empty() && layer.keys == keys)
                            .clicked()
                            && !keys.is_empty()
                        {
                            layer.keys = keys;
                            self.dirty = true;
                        }
                    }
                });
            });
        }
        match op {
            Some(Op::Remove(n)) => {
                self.layers.remove(n);
                self.dirty = true;
            }
            Some(Op::Up(n)) => {
                self.layers.swap(n, n - 1);
                self.dirty = true;
            }
            Some(Op::Down(n)) => {
                self.layers.swap(n, n + 1);
                self.dirty = true;
            }
            None => {}
        }

        heading(ui, p, "Ajouter un calque");
        ui.horizontal_wrapped(|ui| {
            for info in catalog::LAYER_TYPES {
                if chip(ui, p, info.title, false).clicked() {
                    self.layers.push(Layer::all_keys(info.id, json!({})));
                    self.dirty = true;
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if button(
                ui,
                p,
                ButtonKind::Primary,
                Some(i::FLOPPY_DISK),
                "Enregistrer l'effet",
                true,
            )
            .clicked()
            {
                self.save = true;
            }
            if button(
                ui,
                p,
                ButtonKind::Secondary,
                Some(i::FILE),
                "Nouvel effet",
                true,
            )
            .clicked()
            {
                *self = Editor::default();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::BLACK;

    #[test]
    fn an_effect_needs_a_name_and_a_layer() {
        let mut e = Editor::default();
        assert!(e.to_effect().is_err());
        e.name = "Test".into();
        assert!(e.to_effect().is_err());
        e.layers
            .push(Layer::all_keys("solid", json!({"color": "#ff0000"})));
        assert_eq!(e.to_effect().unwrap().layers.len(), 1);
    }

    #[test]
    fn the_preview_follows_the_layers() {
        let ctx = egui::Context::default();
        let mut e = Editor::default();
        assert!(e.preview(&ctx).is_none());
        e.layers
            .push(Layer::all_keys("solid", json!({"color": "#ff0000"})));
        e.dirty = true;
        let frame = e.preview(&ctx).unwrap();
        assert_eq!(frame[1], crate::live::rgb(255, 0, 0));
        assert_eq!(frame[127], BLACK);
        // a layer over a few keys only overrides those
        e.layers.push(Layer {
            effect: "solid".into(),
            params: json!({"color": "#0000ff"}),
            keys: [1u8].into(),
        });
        e.dirty = true;
        let frame = e.preview(&ctx).unwrap();
        assert_eq!(frame[1], crate::live::rgb(0, 0, 255));
        assert_eq!(frame[2], crate::live::rgb(255, 0, 0));
    }
}
