//! Layout A: navigation on the left over the full height; on the right, header,
//! keyboard area, section panel and bottom bar.

use std::collections::BTreeSet;

use eframe::egui::{
    self, Align2, CentralPanel, CornerRadius, Frame, Margin, Panel, RichText, Sense, Stroke, Ui,
    pos2, vec2,
};

use super::theme::{self, Palette};
use super::{UiAction, bottom_bar, card, header, icon_badge, nav, section_blurb, section_icon};
use crate::app::{AppState, Link, Section};
use crate::keyboard::{Group, KeyLook, KeyboardView, Layout, labels};
use crate::sections::SectionsUi;

pub fn show(
    ui: &mut Ui,
    state: &mut AppState,
    selected: &mut BTreeSet<u8>,
    sections: &mut SectionsUi,
) -> Vec<UiAction> {
    let p = Palette::of(ui.ctx());
    let mut actions = Vec::new();

    Panel::left("nav")
        .exact_size(theme::NAV_WIDTH)
        .resizable(false)
        .frame(Frame::new().fill(p.chrome).inner_margin(Margin {
            left: 12,
            right: 12,
            top: 18,
            bottom: 0,
        }))
        .show(ui, |ui| nav::show(ui, &p, state, &mut actions));

    Panel::top("header")
        .exact_size(theme::HEADER_HEIGHT)
        .resizable(false)
        .frame(
            Frame::new()
                .fill(p.chrome)
                .inner_margin(Margin::symmetric(theme::GUTTER as i8 + 4, 0)),
        )
        .show(ui, |ui| {
            header::show(ui, &p, state, &mut sections.profiles, &mut actions);
        });

    Panel::bottom("bottom_bar")
        .exact_size(theme::BOTTOM_HEIGHT)
        .resizable(false)
        .frame(
            Frame::new()
                .fill(p.chrome)
                .inner_margin(Margin::symmetric(theme::GUTTER as i8 + 4, 0)),
        )
        .show(ui, |ui| bottom_bar::show(ui, &p, state, &mut actions));

    CentralPanel::default()
        .frame(
            Frame::new()
                .fill(p.bg)
                .inner_margin(Margin::same(theme::GUTTER as i8)),
        )
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = theme::GUTTER - 4.0;
            keyboard_area(ui, &p, state, selected, sections);
            section_panel(ui, &p, state, selected, sections);
        });

    actions
}

fn keyboard_area(
    ui: &mut Ui,
    p: &Palette,
    state: &AppState,
    selected: &mut BTreeSet<u8>,
    sections: &SectionsUi,
) {
    use egui_phosphor::regular as i;
    let max_height = (ui.available_height() * 0.52).max(220.0);
    card(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let layout = Layout::ak980max();
        let labels = labels::current();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Clavier")
                    .font(theme::strong(15.0))
                    .color(p.text),
            );
            if state.connected {
                let hint = match selected.len() {
                    0 => "Cliquez une touche, glissez pour en prendre plusieurs".to_owned(),
                    1 => "1 touche sélectionnée".to_owned(),
                    n => format!("{n} touches sélectionnées"),
                };
                ui.label(RichText::new(hint).font(theme::regular(12.5)).color(
                    if selected.is_empty() {
                        p.text_faint
                    } else {
                        p.text_dim
                    },
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let by_index = labels.for_layout(layout);
                    for group in Group::ALL.into_iter().rev() {
                        let keys = group.keys(layout, &by_index);
                        let on = !keys.is_empty() && *selected == keys;
                        if chip(ui, p, group.title(), on).clicked() {
                            *selected = if on { BTreeSet::new() } else { keys };
                        }
                    }
                });
            }
        });
        ui.add_space(6.0);

        if state.link != Link::Connected {
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(vec2(width, max_height), Sense::hover());
            let painter = ui.painter_at(rect);
            let (icon, line) = match state.link {
                Link::Searching => (i::CIRCLE_NOTCH, "Recherche du clavier…"),
                _ => (i::PLUGS, "Branchez votre AK980 Max en USB"),
            };
            painter.text(
                rect.center() - vec2(0.0, 14.0),
                Align2::CENTER_CENTER,
                icon,
                theme::icon(34.0),
                p.text_faint,
            );
            painter.text(
                rect.center() + vec2(0.0, 22.0),
                Align2::CENTER_CENTER,
                line,
                theme::regular(14.0),
                p.text_dim,
            );
            return;
        }

        let shown = state.edited.as_ref().or(state.snapshot.as_ref());
        let keys = shown.map(|s| &s.keys);
        let magnetic = shown.map(|s| &s.magnetic);
        let section = state.section;
        let preview = sections.preview_frame(section);
        KeyboardView::new("main")
            .max_height(max_height)
            .shortcuts(view_shortcuts(section, sections.macros.is_recording()))
            .show(ui, layout, selected, |index| {
                let Some(key) = layout.key(index) else {
                    return KeyLook::default();
                };
                let mut look = match keys {
                    Some(map) if matches!(section, Section::Keys | Section::Advanced | Section::Lighting | Section::Macros) => {
                        crate::sections::keys::look(key, map.0[index as usize], &labels, p)
                    }
                    Some(map) => KeyLook::label(labels.key_label(key, map.0[index as usize])),
                    None => KeyLook::label(labels.default_label(key)),
                };
                if section == Section::Lighting
                    && let Some(s) = shown
                    && crate::sections::lighting::effects::is_custom(s.led_effect.mode)
                {
                    let c = s.custom_leds.0[index as usize];
                    look.fill = Some(crate::sections::lighting::color_picker::to_color32(c));
                }
                if let Some(frame) = preview
                    && let Some(c) = frame.get(index as usize)
                {
                    look.fill = Some(crate::sections::lighting::color_picker::to_color32(*c));
                }
                if section == Section::Performance {
                    look.badge = magnetic.and_then(|m| {
                        crate::sections::performance::trigger::badge(&m.0[index as usize])
                    });
                }
                if section == Section::Test {
                    sections.test.decorate(index, &mut look, p);
                }
                look
            });
    });
}

/// Ctrl+A / Échap select on the keyboard view, except where the section reads those keys
/// itself: the key test (every key is a test) and a macro recording.
pub fn view_shortcuts(section: Section, recording: bool) -> bool {
    section != Section::Test && !(section == Section::Macros && recording)
}

/// A small toggle chip (key groups).
pub fn chip(ui: &mut Ui, p: &Palette, text: &str, on: bool) -> egui::Response {
    let width = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::regular(12.5), p.text)
        .size()
        .x;
    let (rect, response) = ui.allocate_exact_size(vec2(width + 20.0, 26.0), Sense::click());
    let hovered = response.hovered();
    let (fill, stroke, fg) = if on {
        (p.accent_soft, p.accent, p.text)
    } else if hovered {
        (p.surface_hi, p.text_faint, p.text)
    } else {
        (p.surface, p.border, p.text_dim)
    };
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(13),
        fill,
        Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    let g = painter.layout_no_wrap(text.to_owned(), theme::regular(12.5), fg);
    painter.galley(
        pos2(
            rect.center().x - g.size().x / 2.0,
            rect.center().y - g.size().y / 2.0,
        ),
        g,
        fg,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn section_panel(
    ui: &mut Ui,
    p: &Palette,
    state: &mut AppState,
    selected: &BTreeSet<u8>,
    sections: &mut SectionsUi,
) {
    use egui_phosphor::regular as i;
    let height = ui.available_height();
    card(p).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(height - 40.0);
        ui.horizontal(|ui| {
            icon_badge(ui, p, section_icon(state.section), 40.0);
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(
                    RichText::new(state.section.title())
                        .font(theme::strong(18.0))
                        .color(p.text),
                );
                ui.label(
                    RichText::new(section_blurb(state.section))
                        .font(theme::regular(13.0))
                        .color(p.text_dim),
                );
            });
        });
        ui.add_space(6.0);
        let rect = ui.available_rect_before_wrap();
        ui.painter()
            .hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.border));

        if state.connected && state.edited.is_some() && has_panel(state.section) {
            ui.add_space(10.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    match state.section {
                        Section::Performance => {
                            crate::sections::performance::show(ui, p, state, selected);
                        }
                        Section::Keys => {
                            crate::sections::keys::show(ui, p, state, selected, &mut sections.keys);
                        }
                        Section::Lighting => {
                            crate::sections::lighting::show(
                                ui,
                                p,
                                state,
                                selected,
                                &mut sections.lighting,
                            );
                        }
                        Section::Advanced => {
                            crate::sections::advanced::show(
                                ui,
                                p,
                                state,
                                selected,
                                &mut sections.advanced,
                            );
                        }
                        Section::Macros => {
                            crate::sections::macros::show(ui, p, state, &mut sections.macros);
                        }
                        Section::Live => {
                            crate::sections::live::show(
                                ui,
                                p,
                                state,
                                selected,
                                &mut sections.live,
                            );
                        }
                        Section::Settings => {
                            crate::sections::settings::show(ui, p, state, &mut sections.settings);
                        }
                        Section::Test => {
                            crate::sections::test::show(ui, p, state, &mut sections.test);
                        }
                    }
                });
            return;
        }

        // empty state until the section's own task fills it
        let center = pos2(
            rect.center().x,
            rect.top() + (rect.height() / 2.0).max(60.0),
        );
        let painter = ui.painter();
        painter.circle_filled(center - vec2(0.0, 18.0), 26.0, p.surface_hi);
        painter.text(
            center - vec2(0.0, 18.0),
            Align2::CENTER_CENTER,
            i::HAMMER,
            theme::icon(22.0),
            p.text_faint,
        );
        painter.text(
            center + vec2(0.0, 26.0),
            Align2::CENTER_CENTER,
            "Bientôt disponible",
            theme::strong(14.0),
            p.text_dim,
        );
        painter.text(
            center + vec2(0.0, 46.0),
            Align2::CENTER_CENTER,
            "Cette section arrive dans une prochaine version de Freejazz.",
            theme::regular(12.5),
            p.text_faint,
        );
    });
}

/// Sections that have their own panel (the others show « Bientôt disponible »).
fn has_panel(section: Section) -> bool {
    matches!(
        section,
        Section::Performance
            | Section::Live
            | Section::Keys
            | Section::Advanced
            | Section::Lighting
            | Section::Macros
            | Section::Settings
            | Section::Test
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_shortcuts_are_off_in_the_test_section_and_while_recording() {
        assert!(view_shortcuts(Section::Keys, false));
        assert!(view_shortcuts(Section::Macros, false));
        assert!(!view_shortcuts(Section::Macros, true));
        assert!(!view_shortcuts(Section::Test, false));
    }
}
