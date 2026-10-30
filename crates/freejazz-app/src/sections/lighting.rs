//! « Éclairage »: the built-in effects, and the per-key colours with a live preview on
//! the keyboard while editing (persisted only by « Appliquer »).

pub mod color_picker;
pub mod effects;
pub mod icons;
pub mod library;
pub mod paint;
pub mod preview;

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use eframe::egui::{self, CornerRadius, Sense, Stroke, StrokeKind, Ui, vec2};
use freejazz_protocol::blocks::{LedEffect, Rgb};
use freejazz_protocol::live::{FRAME_SLOTS, LIVE_AMPLITUDE};

use super::{heading, hint};
use crate::app::{AppState, Section};
use crate::device::{DeviceHandle, DeviceKind, Request};
use crate::keyboard::Layout;
use crate::ui::shell::chip;
use crate::ui::theme::Palette;
use crate::ui::{ButtonKind, button};
use color_picker::{ColorPicker, to_color32};
use effects::is_custom;

/// At most 25 frames a second (protocol.md 6.12: the keyboard keeps up with 30).
const FRAME_INTERVAL: Duration = Duration::from_millis(40);



/// The built-in effect shown on the keyboard before « Appliquer ».
#[derive(Default)]
struct EffectSync {
    /// What the keyboard shows now (`None`: not looked at yet).
    keyboard: Option<LedEffect>,
    /// What it showed before any preview: « Annuler » sends it back.
    origin: Option<LedEffect>,
    sent_at: Option<Instant>,
}

#[derive(Default)]
struct Live {
    on: bool,
    last: Option<[Rgb; FRAME_SLOTS]>,
    sent_at: Option<Instant>,
}

pub struct LightingUi {
    effect_picker: ColorPicker,
    paint_picker: ColorPicker,
    /// Colours A and B of the painting tools; `which` is the one the picker edits.
    paint: [Rgb; 2],
    which: usize,
    /// The effect colour and colour mode last applied: the keyboard reads back white and
    /// RGB after any write (protocol.md 9.7), so the read value is not shown.
    remembered: Option<(Rgb, u8)>,
    library: library::Library,
    live: Live,
    sync: EffectSync,
    animation: preview::Preview,
    /// The approximate animation of the built-in effect, for the keyboard view.
    frame: Option<crate::live::Frame>,
    /// A live effect owns the keyboard's lighting: nothing is previewed on it.
    suspended: bool,
}

impl Default for LightingUi {
    fn default() -> Self {
        Self {
            effect_picker: ColorPicker::default(),
            paint_picker: ColorPicker::default(),
            paint: [
                Rgb {
                    r: 0x7c,
                    g: 0x5c,
                    b: 0xff,
                },
                Rgb {
                    r: 255,
                    g: 255,
                    b: 255,
                },
            ],
            which: 0,
            remembered: None,
            library: library::Library::default(),
            live: Live::default(),
            sync: EffectSync::default(),
            animation: preview::Preview::default(),
            frame: None,
            suspended: false,
        }
    }
}

impl LightingUi {
    /// Starts, feeds and stops the live preview: on while the section is open with the
    /// custom colours being edited. Called every frame. While a live effect runs
    /// (`engine_running`) it owns the stream and the lighting: nothing is sent, neither
    /// the custom colours nor a built-in effect (the worker would ignore it anyway).
    pub fn pump(
        &mut self,
        state: &AppState,
        device: &DeviceHandle,
        ctx: &egui::Context,
        engine_running: bool,
    ) {
        self.suspended = engine_running;
        if engine_running {
            self.end_live(device);
            return;
        }
        let custom = state
            .edited
            .as_ref()
            .filter(|e| is_custom(e.led_effect.mode))
            .map(|e| e.custom_leds.0);
        self.sync_effect(state, device, ctx);
        let want = state.section == Section::Lighting
            && state.connected
            && !state.busy
            && device.kind() != DeviceKind::Offline
            && custom.is_some();
        if !want {
            self.end_live(device);
            return;
        }
        let Some(frame) = custom else { return };
        if !self.live.on {
            device.send(Request::LiveBegin);
            self.live = Live {
                on: true,
                ..Live::default()
            };
        }
        if self.live.last == Some(frame) {
            return;
        }
        let wait = self.live.sent_at.map_or(Duration::ZERO, |t| {
            FRAME_INTERVAL.saturating_sub(t.elapsed())
        });
        if wait.is_zero() {
            device.send(Request::LiveFrame(Box::new(frame), LIVE_AMPLITUDE));
            self.live.last = Some(frame);
            self.live.sent_at = Some(Instant::now());
        } else {
            ctx.request_repaint_after(wait);
        }
    }

    /// Whether the custom colours are streamed to the keyboard right now.
    pub fn previewing(&self) -> bool {
        self.live.on
    }

    /// The colours the keyboard view shows for the built-in effect being chosen.
    pub fn preview_frame(&self) -> Option<&crate::live::Frame> {
        self.frame.as_ref()
    }

    /// Computes this frame's animation; called before the window is drawn. It asks for a
    /// repaint only while the animation is visible.
    pub fn update_preview(&mut self, state: &AppState, ctx: &egui::Context) {
        self.frame = None;
        if state.section != Section::Lighting || !state.connected {
            return;
        }
        let Some(edited) = state.edited.as_ref() else {
            return;
        };
        let led = &edited.led_effect;
        if is_custom(led.mode) {
            return;
        }
        let (rgb, mode) = self.remembered.unwrap_or((led.rgb, led.color_mode));
        let (frame, next) = self.animation.next(&preview::Look::of(led, rgb, mode));
        self.frame = Some(frame);
        if led.mode != effects::OFF {
            ctx.request_repaint_after(next);
        }
    }

    /// A new baseline (applied, re-read): the keyboard shows what is stored again.
    pub fn on_event(&mut self, event: &crate::device::Event) {
        use crate::device::Event;
        if matches!(
            event,
            Event::Connected { .. } | Event::Applied(_) | Event::Refreshed(_)
        ) {
            self.sync = EffectSync::default();
        }
    }

    /// « Annuler » (or anything that re-reads the keyboard): the effect it showed before
    /// the previews goes back.
    pub fn on_revert(&mut self, device: &DeviceHandle) {
        if let (Some(origin), Some(shown)) = (self.sync.origin.take(), self.sync.keyboard.take())
            && shown != origin
        {
            self.remembered = Some((origin.rgb, origin.color_mode));
            device.send(Request::PreviewLed(Box::new(origin)));
        }
        self.sync = EffectSync::default();
    }

    /// Sends the chosen built-in effect to the keyboard as soon as it changes.
    fn sync_effect(&mut self, state: &AppState, device: &DeviceHandle, ctx: &egui::Context) {
        let (Some(edited), Some(base)) = (state.edited.as_ref(), state.snapshot.as_ref()) else {
            return;
        };
        let usable = state.connected
            && !state.busy
            && device.kind() != DeviceKind::Offline
            && !self.live.on
            && !is_custom(edited.led_effect.mode);
        if !usable {
            return;
        }
        let remembered = *self
            .remembered
            .get_or_insert((base.led_effect.rgb, base.led_effect.color_mode));
        let mut target = edited.led_effect.clone();
        effects::restore_colour(&mut target, remembered);
        let Some(shown) = self.sync.keyboard.clone() else {
            self.sync.origin = Some(target.clone());
            self.sync.keyboard = Some(target);
            return;
        };
        if shown == target {
            return;
        }
        let is_discrete = target.mode != shown.mode
            || target.color_mode != shown.color_mode
            || target.direction != shown.direction;
        let wait = if is_discrete {
            Duration::ZERO
        } else {
            self.sync.sent_at.map_or(Duration::ZERO, |t| {
                Duration::from_millis(50).saturating_sub(t.elapsed())
            })
        };
        if wait.is_zero() {
            device.send(Request::PreviewLed(Box::new(target.clone())));
            self.sync.keyboard = Some(target);
            self.sync.sent_at = Some(Instant::now());
        } else {
            ctx.request_repaint_after(wait);
        }
    }

    /// Stops the preview: the keyboard goes back to its stored effect. Called before
    /// anything that writes the effect.
    pub fn end_live(&mut self, device: &DeviceHandle) {
        if self.live.on {
            device.send(Request::LiveEnd);
            self.live = Live::default();
        }
    }
}

/// A colour swatch; clicked when chosen.
fn swatch(ui: &mut Ui, p: &Palette, colour: Rgb, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 28.0), Sense::click());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(7), to_color32(colour));
    let stroke = if active {
        Stroke::new(2.0, p.accent)
    } else {
        Stroke::new(1.0, p.border)
    };
    ui.painter()
        .rect_stroke(rect, CornerRadius::same(7), stroke, StrokeKind::Inside);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A chip with the site's icon of the effect.
fn effect_chip(ui: &mut Ui, p: &Palette, mode: u8, text: &str, on: bool) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), crate::ui::theme::regular(12.5), p.text);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 44.0, 30.0), Sense::click());
    let (fill, stroke, fg) = if on {
        (p.accent_soft, p.accent, p.text)
    } else if response.hovered() {
        (p.surface_hi, p.text_faint, p.text)
    } else {
        (p.surface, p.border, p.text_dim)
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(15),
        fill,
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    if let Some(tex) = icons::texture(ui.ctx(), mode, 16.0) {
        let icon = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 16.0, rect.center().y),
            vec2(16.0, 16.0),
        );
        ui.painter().image(
            tex.id(),
            icon,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            fg,
        );
    }
    ui.painter().galley(
        egui::pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0),
        galley,
        fg,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Chips 1–5; a value read above 5 (6 on this unit) is shown too, but an edit stays
/// within 1–5.
fn level(ui: &mut Ui, p: &Palette, value: &mut u8) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for v in 1..=5u8 {
            if chip(ui, p, &v.to_string(), *value == v).clicked() {
                *value = v;
                changed = true;
            }
        }
        if *value > 5 {
            chip(ui, p, &value.to_string(), true);
        }
    });
    changed
}

fn labelled(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(2.0);
    heading(ui, p, text);
}

pub fn show(
    ui: &mut Ui,
    p: &Palette,
    state: &mut AppState,
    selected: &BTreeSet<u8>,
    ui_state: &mut LightingUi,
) {
    let busy = state.busy;
    let (Some(edited), Some(base)) = (state.edited.as_mut(), state.snapshot.as_mut()) else {
        return;
    };
    let layout = Layout::ak980max();
    let remembered = ui_state
        .remembered
        .get_or_insert((base.led_effect.rgb, base.led_effect.color_mode));
    if !busy {
        effects::restore_colour(&mut base.led_effect, *remembered);
        effects::restore_colour(&mut edited.led_effect, *remembered);
    }
    let led = &mut edited.led_effect;

    if ui_state.suspended {
        super::error_line(
            ui,
            p,
            "Un effet en direct est en cours : le clavier ne montre pas ces réglages \
             (arrêtez-le dans « Effets en direct »).",
        );
    }
    labelled(ui, p, "Effet");
    let custom = is_custom(led.mode);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        if chip(ui, p, "Éteint", led.mode == effects::OFF).clicked() {
            led.mode = effects::OFF;
        }
        for e in effects::all() {
            if effect_chip(ui, p, e.mode, &e.fr, led.mode == e.mode).clicked() {
                effects::select(led, e);
                remembered.1 = led.color_mode;
            }
        }
        if chip(ui, p, "Personnalisé", custom).clicked() && !custom {
            led.mode = freejazz_protocol::blocks::led::MODE_CUSTOM;
        }
    });

    if !custom {
        hint(
            ui,
            p,
            "Aperçu sur le clavier ci-dessus, reconstitué d'après le vrai clavier (les \
             effets réactifs montrent des appuis simulés). Le choix est aussi envoyé au clavier tout de suite : \
             « Annuler » remet l'effet précédent, « Appliquer » le garde.",
        );
    }
    if led.mode == effects::OFF {
        hint(ui, p, "L'éclairage est éteint.");
        return;
    }

    if let Some(effect) = effects::find(led.mode) {
        if effect.has_color {
            if effect.color_modes.len() > 1 {
                labelled(ui, p, "Couleurs");
                ui.horizontal(|ui| {
                    for (mode, name) in [(0u8, "Une couleur"), (1, "Multicolore")] {
                        if chip(ui, p, name, led.color_mode == mode).clicked() {
                            led.color_mode = mode;
                            remembered.1 = mode;
                        }
                    }
                });
            }
            labelled(ui, p, "Couleur");
            if led.color_mode == 1 {
                hint(
                    ui,
                    p,
                    "L'effet est en multicolore. Choisir une couleur ci-dessous bascule en « Une couleur ».",
                );
            }
            if ui_state.effect_picker.show(ui, p, &mut remembered.0) {
                led.rgb = remembered.0;
                remembered.1 = 0;
                led.color_mode = 0;
            }
        }
        if effect.has_speed {
            labelled(ui, p, "Vitesse");
            level(ui, p, &mut led.speed);
        }
        if effect.has_direction {
            labelled(ui, p, "Direction");
            ui.horizontal(|ui| {
                for &d in &effect.directions {
                    if chip(ui, p, effects::direction_name(d), led.direction == d).clicked() {
                        led.direction = d;
                    }
                }
            });
        }
    }
    labelled(ui, p, "Luminosité");
    level(ui, p, &mut led.brightness);

    if !custom {
        return;
    }

    // custom colours: the keyboard above shows them, the keyboard itself previews them
    let keys: Vec<u8> = selected
        .iter()
        .copied()
        .filter(|&i| layout.key(i).is_some())
        .collect();
    labelled(ui, p, "Couleurs des touches");
    hint(
        ui,
        p,
        "Sélectionnez des touches sur le clavier, choisissez une couleur, puis peignez. \
         L'aperçu s'affiche en direct sur le clavier ; « Appliquer » l'enregistre.",
    );
    ui.horizontal(|ui| {
        for i in 0..2 {
            if swatch(ui, p, ui_state.paint[i], ui_state.which == i).clicked() {
                ui_state.which = i;
            }
        }
        hint(
            ui,
            p,
            if ui_state.which == 0 {
                "Couleur A"
            } else {
                "Couleur B (pour le dégradé)"
            },
        );
    });
    let which = ui_state.which;
    ui_state
        .paint_picker
        .show(ui, p, &mut ui_state.paint[which]);
    let [a, b] = ui_state.paint;
    ui.horizontal_wrapped(|ui| {
        if button(
            ui,
            p,
            ButtonKind::Primary,
            None,
            "Peindre la sélection",
            !keys.is_empty(),
        )
        .clicked()
        {
            paint::paint(&mut edited.custom_leds, &keys, a);
        }
        if button(
            ui,
            p,
            ButtonKind::Secondary,
            None,
            "Remplir tout le clavier",
            true,
        )
        .clicked()
        {
            paint::fill_all(&mut edited.custom_leds, layout, a);
        }
        if button(
            ui,
            p,
            ButtonKind::Secondary,
            None,
            "Dégradé A vers B sur la sélection",
            keys.len() > 1,
        )
        .clicked()
        {
            paint::gradient(&mut edited.custom_leds, layout, &keys, a, b);
        }
    });
    ui.add_space(6.0);
    if let Some(leds) = ui_state.library.show(ui, p, &edited.custom_leds) {
        edited.custom_leds = leds;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceHandle;

    fn connected() -> AppState {
        let mut state = AppState::new(true, false);
        let snapshot = crate::mock::read_fixture().unwrap();
        state.apply(crate::device::Event::Connected {
            ident: crate::mock::ident(&snapshot),
            snapshot: Box::new(snapshot),
        });
        state.section = Section::Lighting;
        state
    }

    #[test]
    fn the_preview_starts_and_stops_with_the_custom_editor() {
        let ctx = egui::Context::default();
        let mut ui = LightingUi::default();
        let device = DeviceHandle::offline();
        let mut state = connected();
        // built-in effect: no preview
        ui.pump(&state, &device, &ctx, false);
        assert!(!ui.live.on);
        // an offline handle never previews, whatever the state
        state.edited.as_mut().unwrap().led_effect.mode = 128;
        ui.pump(&state, &device, &ctx, false);
        assert!(!ui.live.on);
    }

    #[test]
    fn nothing_is_sent_while_a_live_effect_runs() {
        let ctx = egui::Context::default();
        let waker_ctx = ctx.clone();
        let device = crate::mock::spawn(std::sync::Arc::new(move || waker_ctx.request_repaint()));
        let mut ui = LightingUi::default();
        let mut state = connected();
        // a built-in effect chosen while the engine runs: not marked as shown
        let original = state.edited.as_ref().unwrap().led_effect.mode;
        state.edited.as_mut().unwrap().led_effect.mode = if original == 8 { 7 } else { 8 };
        ui.pump(&state, &device, &ctx, true);
        assert!(ui.sync.keyboard.is_none() && ui.suspended);
        // custom colours: no stream of their own
        state.edited.as_mut().unwrap().led_effect.mode = 128;
        ui.pump(&state, &device, &ctx, true);
        assert!(!ui.previewing());
        // the engine stopped: the preview starts
        ui.pump(&state, &device, &ctx, false);
        assert!(ui.previewing() && !ui.suspended);
        // and gives way when an engine starts
        ui.pump(&state, &device, &ctx, true);
        assert!(!ui.previewing());
    }

    #[test]
    fn a_chosen_effect_is_sent_at_once_and_cancel_sends_the_old_one_back() {
        let ctx = egui::Context::default();
        let waker_ctx = ctx.clone();
        let device = crate::mock::spawn(std::sync::Arc::new(move || waker_ctx.request_repaint()));
        let mut ui = LightingUi::default();
        let mut state = connected();
        let original = state.edited.as_ref().unwrap().led_effect.mode;
        // first look: the keyboard is taken to show the stored effect
        ui.pump(&state, &device, &ctx, false);
        assert_eq!(ui.sync.origin.as_ref().map(|e| e.mode), Some(original));
        // choosing another effect sends it
        let other = if original == 8 { 7 } else { 8 };
        state.edited.as_mut().unwrap().led_effect.mode = other;
        ui.pump(&state, &device, &ctx, false);
        assert_eq!(ui.sync.keyboard.as_ref().map(|e| e.mode), Some(other));
        // the animation runs for it, and stops for a custom design
        ui.update_preview(&state, &ctx);
        assert!(ui.preview_frame().is_some());
        // « Annuler » puts the origin back and forgets the preview
        ui.on_revert(&device);
        assert!(ui.sync.keyboard.is_none() && ui.sync.origin.is_none());
        state.edited.as_mut().unwrap().led_effect.mode = 128;
        ui.update_preview(&state, &ctx);
        assert!(ui.preview_frame().is_none());
    }

    #[test]
    fn an_offline_handle_sends_nothing() {
        let ctx = egui::Context::default();
        let device = DeviceHandle::offline();
        let mut ui = LightingUi::default();
        let state = connected();
        ui.pump(&state, &device, &ctx, false);
        assert!(ui.sync.keyboard.is_none());
    }

    #[test]
    fn a_mock_keyboard_gets_a_coalesced_preview() {
        let ctx = egui::Context::default();
        let waker_ctx = ctx.clone();
        let device = crate::mock::spawn(std::sync::Arc::new(move || waker_ctx.request_repaint()));
        let mut ui = LightingUi::default();
        let mut state = connected();
        state.edited.as_mut().unwrap().led_effect.mode = 128;
        ui.pump(&state, &device, &ctx, false);
        assert!(ui.live.on && ui.live.last.is_some());
        // an immediate change waits for the next slot instead of being sent
        let first = ui.live.last;
        state.edited.as_mut().unwrap().custom_leds.0[3] = Rgb { r: 1, g: 2, b: 3 };
        ui.pump(&state, &device, &ctx, false);
        assert_eq!(ui.live.last, first);
        // leaving the section ends it
        state.section = Section::Keys;
        ui.pump(&state, &device, &ctx, false);
        assert!(!ui.live.on);
    }
}
