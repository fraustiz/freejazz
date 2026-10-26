//! Look of the app: palettes (dark by default, follows Windows), fonts (Inter and
//! Phosphor icons, embedded), spacing and widget style.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, TextStyle, Theme, ThemePreference, Visuals,
};

pub const ACCENT: Color32 = Color32::from_rgb(0x7c, 0x5c, 0xff);

/// Colours the custom-painted parts use; the egui widgets get the same through
/// [`Visuals`].
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Main area background.
    pub bg: Color32,
    /// Navigation, header and bottom bar.
    pub chrome: Color32,
    /// Cards.
    pub surface: Color32,
    /// Hovered rows, inputs, keycaps.
    pub surface_hi: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    /// Accent tint for selected rows and icon badges.
    pub accent_soft: Color32,
    /// Text on the accent.
    pub on_accent: Color32,
    pub success: Color32,
    pub warning: Color32,
    pub danger: Color32,
}

impl Palette {
    pub const DARK: Palette = Palette {
        dark: true,
        bg: Color32::from_rgb(0x0e, 0x0f, 0x14),
        chrome: Color32::from_rgb(0x13, 0x14, 0x1b),
        surface: Color32::from_rgb(0x18, 0x19, 0x22),
        surface_hi: Color32::from_rgb(0x21, 0x22, 0x2e),
        border: Color32::from_rgb(0x27, 0x29, 0x37),
        text: Color32::from_rgb(0xec, 0xec, 0xf3),
        text_dim: Color32::from_rgb(0xa0, 0xa2, 0xb6),
        text_faint: Color32::from_rgb(0x6c, 0x6f, 0x85),
        accent: ACCENT,
        accent_hover: Color32::from_rgb(0x91, 0x76, 0xff),
        accent_soft: Color32::from_rgb(0x28, 0x22, 0x48),
        on_accent: Color32::WHITE,
        success: Color32::from_rgb(0x3d, 0xd6, 0x8c),
        warning: Color32::from_rgb(0xff, 0xb5, 0x47),
        danger: Color32::from_rgb(0xff, 0x5d, 0x6c),
    };

    pub const LIGHT: Palette = Palette {
        dark: false,
        bg: Color32::from_rgb(0xf5, 0xf5, 0xfa),
        chrome: Color32::from_rgb(0xfc, 0xfc, 0xfe),
        surface: Color32::WHITE,
        surface_hi: Color32::from_rgb(0xf0, 0xef, 0xf7),
        border: Color32::from_rgb(0xe3, 0xe3, 0xec),
        text: Color32::from_rgb(0x1a, 0x1b, 0x26),
        text_dim: Color32::from_rgb(0x5a, 0x5c, 0x70),
        text_faint: Color32::from_rgb(0x8e, 0x90, 0xa5),
        accent: ACCENT,
        accent_hover: Color32::from_rgb(0x6a, 0x48, 0xf5),
        accent_soft: Color32::from_rgb(0xee, 0xea, 0xff),
        on_accent: Color32::WHITE,
        success: Color32::from_rgb(0x16, 0xa3, 0x63),
        warning: Color32::from_rgb(0xd9, 0x82, 0x0b),
        danger: Color32::from_rgb(0xe0, 0x36, 0x4a),
    };

    pub fn for_dark(dark: bool) -> Self {
        if dark { Self::DARK } else { Self::LIGHT }
    }

    pub fn of(ctx: &egui::Context) -> Self {
        Self::for_dark(ctx.theme() == Theme::Dark)
    }
}

/// Spacing and radii.
pub const RADIUS_CARD: u8 = 14;
pub const RADIUS_CONTROL: u8 = 9;
pub const NAV_WIDTH: f32 = 236.0;
pub const HEADER_HEIGHT: f32 = 68.0;
pub const BOTTOM_HEIGHT: f32 = 64.0;
pub const GUTTER: f32 = 20.0;

/// Family of the semibold face (titles, buttons).
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn strong(size: f32) -> FontId {
    FontId::new(size, semibold())
}

pub fn regular(size: f32) -> FontId {
    FontId::proportional(size)
}

/// Phosphor icons (regular weight).
pub fn icon(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("phosphor".into()))
}

/// Installs fonts and both styles; the theme follows Windows unless `forced`.
pub fn install(ctx: &egui::Context, forced: Option<Theme>) {
    ctx.set_fonts(fonts());
    ctx.options_mut(|o| {
        o.theme_preference = match forced {
            Some(Theme::Dark) => ThemePreference::Dark,
            Some(Theme::Light) => ThemePreference::Light,
            None => ThemePreference::System,
        };
        // Fall back to dark when Windows does not tell.
        o.fallback_theme = Theme::Dark;
    });
    ctx.style_mut_of(Theme::Dark, |s| style(s, &Palette::DARK));
    ctx.style_mut_of(Theme::Light, |s| style(s, &Palette::LIGHT));
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::empty();
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "inter-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    fonts
        .families
        .insert(FontFamily::Proportional, vec!["inter".into()]);
    fonts
        .families
        .insert(FontFamily::Monospace, vec!["inter".into()]);
    fonts
        .families
        .insert(semibold(), vec!["inter-semibold".into()]);
    // Icons get their own family: Inter has glyphs on some of Phosphor's private-use
    // codepoints and Phosphor has blank text glyphs, so the two never share a string.
    fonts.font_data.insert(
        "phosphor".into(),
        Arc::new(FontData::from_static(
            egui_phosphor::Variant::Regular.font_bytes(),
        )),
    );
    fonts.families.insert(
        FontFamily::Name("phosphor".into()),
        vec!["phosphor".into(), "inter".into()],
    );
    fonts
}

fn style(s: &mut egui::Style, p: &Palette) {
    s.text_styles = [
        (TextStyle::Small, regular(12.0)),
        (TextStyle::Body, regular(14.0)),
        (TextStyle::Button, regular(14.0)),
        (TextStyle::Heading, strong(20.0)),
        (TextStyle::Monospace, FontId::monospace(13.0)),
    ]
    .into();

    let sp = &mut s.spacing;
    sp.item_spacing = egui::vec2(10.0, 10.0);
    sp.button_padding = egui::vec2(14.0, 8.0);
    sp.interact_size = egui::vec2(36.0, 34.0);
    sp.menu_margin = Margin::same(8);
    sp.window_margin = Margin::same(16);
    sp.combo_width = 180.0;
    sp.icon_width = 16.0;

    s.visuals = visuals(p);
}

fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let control = CornerRadius::same(RADIUS_CONTROL);
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 110 } else { 40 }),
    };
    v.popup_shadow = v.window_shadow;
    v.extreme_bg_color = if p.dark {
        Color32::from_rgb(0x0b, 0x0c, 0x10)
    } else {
        Color32::WHITE
    };
    v.faint_bg_color = p.surface_hi;
    v.code_bg_color = p.surface_hi;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warning;
    v.error_fg_color = p.danger;
    v.override_text_color = None;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, p.on_accent);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text_dim);
    w.noninteractive.corner_radius = control;

    w.inactive.bg_fill = p.surface_hi;
    w.inactive.weak_bg_fill = p.surface_hi;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = control;

    w.hovered.bg_fill = p.surface_hi;
    w.hovered.weak_bg_fill = p.surface_hi;
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.7));
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.corner_radius = control;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.accent_soft;
    w.active.weak_bg_fill = p.accent_soft;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.active.corner_radius = control;
    w.active.expansion = 0.0;

    w.open = w.active;
    v
}
