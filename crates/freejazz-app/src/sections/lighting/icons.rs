//! The site's icon of each built-in effect (`assets/effects/*.svg`, see `ORIGINE.md`),
//! rasterised once per size and kept as a texture.

use eframe::egui::{self, Color32, TextureHandle, TextureOptions, Ui, vec2};
use egui_extras::image::load_svg_bytes_with_size;

/// The SVG of effect `mode` (the same pairing as the site); `None` for unknown modes.
pub fn svg(mode: u8) -> Option<&'static [u8]> {
    Some(match mode {
        1 => include_bytes!("../../../assets/effects/carbon_sun.svg"),
        2 => include_bytes!("../../../assets/effects/carbon_data-enrichment.svg"),
        3 => include_bytes!("../../../assets/effects/carbon_idea.svg"),
        4 => include_bytes!("../../../assets/effects/ion_sparkles-outline.svg"),
        5 => include_bytes!("../../../assets/effects/carbon_snow-heavy.svg"),
        6 => include_bytes!("../../../assets/effects/ion_flower-outline.svg"),
        7 => include_bytes!("../../../assets/effects/carbon_windy.svg"),
        8 => include_bytes!("../../../assets/effects/ion_ios-loop-strong.svg"),
        9 => include_bytes!("../../../assets/effects/carbon_soil-moisture-global.svg"),
        10 => include_bytes!("../../../assets/effects/carbon_chevron-sort.svg"),
        11 => include_bytes!("../../../assets/effects/carbon_binding-02.svg"),
        12 => include_bytes!("../../../assets/effects/carbon_mountain.svg"),
        13 => include_bytes!("../../../assets/effects/carbon_lightning.svg"),
        14 => include_bytes!("../../../assets/effects/carbon_switch-layer-2.svg"),
        15 => include_bytes!("../../../assets/effects/ion_radio-outline.svg"),
        16 => include_bytes!("../../../assets/effects/carbon_smoke.svg"),
        17 => include_bytes!("../../../assets/effects/ion_prism-outline.svg"),
        18 => include_bytes!("../../../assets/effects/ion_rainy-outline.svg"),
        19 => include_bytes!("../../../assets/effects/ion_swap-horizontal-outline.svg"),
        128 => include_bytes!("../../../assets/effects/ion_create-outline.svg"),
        _ => return None,
    })
}

/// The icon as a texture of `size` points, white (tint it when drawing).
pub fn texture(ctx: &egui::Context, mode: u8, size: f32) -> Option<TextureHandle> {
    let bytes = svg(mode)?;
    let px = (size * ctx.pixels_per_point()).ceil().max(8.0) as u32;
    let id = egui::Id::new(("effect-icon", mode, px));
    if let Some(t) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(t);
    }
    let size_hint = egui::load::SizeHint::Size {
        width: px,
        height: px,
        maintain_aspect_ratio: true,
    };
    let image = load_svg_bytes_with_size(bytes, size_hint, &Default::default()).ok()?;
    let t = ctx.load_texture(format!("effect-{mode}-{px}"), image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    Some(t)
}

/// Draws the icon of `mode` in `tint`.
pub fn show(ui: &mut Ui, mode: u8, size: f32, tint: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    if let Some(t) = texture(ui.ctx(), mode, size) {
        ui.painter().image(
            t.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            tint,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_effect_has_a_rasterisable_icon() {
        let ctx = egui::Context::default();
        for e in super::super::effects::all() {
            assert!(svg(e.mode).is_some(), "mode {}", e.mode);
            assert!(texture(&ctx, e.mode, 24.0).is_some(), "mode {}", e.mode);
        }
        assert!(svg(128).is_some() && svg(99).is_none());
    }
}
