//! The picker: tabs and a wrapped grid of entries.

use eframe::egui::{TextEdit, Ui};
use freejazz_protocol::blocks::keys::KeyAction;

use super::catalog::Entry;
use crate::ui::shell::chip;
use crate::ui::theme::{self, Palette};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Keyboard,
    Media,
    Mouse,
    Macros,
    Disable,
    Default,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Keyboard,
        Tab::Media,
        Tab::Mouse,
        Tab::Macros,
        Tab::Disable,
        Tab::Default,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Keyboard => "Clavier",
            Tab::Media => "Multimédia",
            Tab::Mouse => "Souris",
            Tab::Macros => "Macros",
            Tab::Disable => "Désactiver",
            Tab::Default => "Par défaut",
        }
    }
}

pub fn tabs(ui: &mut Ui, p: &Palette, tab: &mut Tab) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = eframe::egui::vec2(6.0, 6.0);
        for t in Tab::ALL {
            if chip(ui, p, t.title(), *tab == t).clicked() {
                *tab = t;
            }
        }
    });
}

/// The search box of a long tab; true when it is shown.
pub fn search(ui: &mut Ui, text: &mut String) {
    ui.add(
        TextEdit::singleline(text)
            .hint_text("Rechercher")
            .desired_width(220.0)
            .font(theme::regular(13.0)),
    );
}

/// The entries matching `filter`; the one clicked, if any. `current` is lit.
pub fn entries(
    ui: &mut Ui,
    p: &Palette,
    list: &[Entry],
    current: Option<KeyAction>,
    filter: &str,
) -> Option<KeyAction> {
    let filter = filter.trim().to_lowercase();
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = eframe::egui::vec2(6.0, 6.0);
        for e in list.iter().filter(|e| e.search.contains(&filter)) {
            if chip(ui, p, &e.label, current == Some(e.action)).clicked() {
                picked = Some(e.action);
            }
        }
    });
    picked
}
