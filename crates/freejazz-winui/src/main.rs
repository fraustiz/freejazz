use std::collections::HashSet;
use std::path::PathBuf;
use windows_reactor::*;

mod effects;
mod icons;
mod input;
mod keycodes;
mod labels;
mod layout;

use effects::{all_effects, direction_name, find_effect, Effect};
use freejazz_protocol::blocks::dks::{DksEntry, DksTable, MAX_DKS_SLOT};
use freejazz_protocol::blocks::keys::{KeyAction, KeyKind, KeyMap};
use freejazz_protocol::blocks::led::{CustomLeds, Rgb, MODE_CUSTOM};
use keycodes::{action_display_name, media_actions, mouse_actions, standard_keyboard_actions, KeyActionOption};
use labels::LayoutPreference;
use layout::Layout;

fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() == 6 {
        let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
        let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
        let b = u8::from_str_radix(&clean[4..6], 16).ok()?;
        Some((r, g, b))
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Performance,
    Lighting,
    Advanced,
    Keys,
    Test,
    Device,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightingSubTab {
    Effects,
    CustomPaint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvancedSubTab {
    Socd,
    ModTap,
    Toggle,
    Dks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemapCategory {
    Keyboard,
    Media,
    Mouse,
    Special,
}

#[derive(Clone)]
pub enum Message {
    SelectSection(Section),
    SelectKey(u8),
    KeysPolled(Vec<u8>),
    CheckDevice,
    SetLayoutPreference(LayoutPreference),

    // Lighting (19 Effects)
    SelectLightingSubTab(LightingSubTab),
    SelectEffect(usize),
    SetBrightness(f64),
    SetSpeed(f64),
    SetColorMode(u8),
    SetDirection(u8),
    SetColor(u8, u8, u8),
    SetColorPreset(u8, u8, u8),
    SetHexColor(String),
    SetColorChannelR(f64),
    SetColorChannelG(f64),
    SetColorChannelB(f64),
    ApplyLighting,

    // Lighting (Custom Per-Key Paint)
    SetBrushColor(u8, u8, u8),
    SetBrushHexColor(String),
    PaintKey(u8),
    PaintAllKeys,
    ClearAllCustomLeds,
    ApplyCustomLeds,

    // Performance
    ToggleApplyGlobally(bool),
    SelectSwitchType(u8),
    SetActuation(f64),
    ToggleRapidTrigger(bool),
    SetPressRt(f64),
    SetReleaseRt(f64),
    SetTopDeadzone(f64),
    SetBottomDeadzone(f64),
    SelectReportRate(u32),
    ApplyPerformance,

    // Remap Keys
    SelectRemapCategory(RemapCategory),
    AssignKeyAction(KeyAction),
    ResetSelectedKeyAction,
    ResetAllKeysDefault,
    ApplyKeyMap,

    // Advanced Keys
    SelectAdvancedTab(AdvancedSubTab),
    SelectSocdMode(u8),
    SetSocdPreset(u8, u8),
    ApplySocd,
    ClearSocd,
    SetModTapHold(u8),
    SetModTapTap(u8),
    SetModTapDelay(f64),
    ApplyModTap,
    ClearModTap,
    ApplyToggle,
    ClearToggle,
    // DKS
    SetDksStartMm(f64),
    SetDksBottomMm(f64),
    SetDksAction1(u8),
    SetDksAction2(u8),
    ApplyDks,
    ClearDks,

    // Backups
    CreateBackup,
    RestoreBackup(String),
    RefreshBackups,
    OpenBackupFolder,

    // Test
    ClearTest,
    TestFnKey,
}

struct AppModel {
    current_section: Section,
    connected: bool,
    device_name: String,
    firmware_version: String,
    pid_hex: String,
    status_detail: String,

    // Layout
    layout_preference: LayoutPreference,
    selected_key: Option<u8>,
    pressed_keys: HashSet<u8>,

    // Protocol state cache
    keys_map: KeyMap,
    custom_leds: CustomLeds,
    dks_table: DksTable,

    // Section 1: Lighting
    lighting_tab: LightingSubTab,
    led_mode_index: usize, // Index in all_effects() (0..18)
    led_brightness: f64,   // 1 to 5
    led_speed: f64,        // 1 to 5
    led_direction: u8,     // 0: right, 1: left, 2: up, 3: down
    led_color_mode: u8,    // 0: single, 1: RGB
    led_r: u8,
    led_g: u8,
    led_b: u8,
    paint_brush_color: Rgb,

    // Section 2: Performance
    apply_globally: bool,
    switch_type: u8,       // 1: Riz / Axis 9, 2: Jade Pro, 3: King / Magicien
    actuation_mm: f64,     // 0.1 to 3.3 mm
    rapid_trigger: bool,   // On / Off
    press_rt_mm: f64,      // 0.01 to 2.0 mm
    release_rt_mm: f64,    // 0.01 to 2.0 mm
    top_deadzone_mm: f64,  // 0.0 to 0.50 mm
    bottom_deadzone_mm: f64, // 0.0 to 0.50 mm
    polling_rate_hz: u32,  // 1000, 2000, 4000, 8000 Hz

    // Section 3: Touches Avancées
    advanced_tab: AdvancedSubTab,
    socd_mode: u8,         // 3: Dernier appui (Last Win), 1: Premier, 2: Second, 4: Neutre
    socd_key1: u8,         // Key index 1
    socd_key2: u8,         // Key index 2
    mt_hold_key: u8,       // HID keycode for hold (0xE0 = LCtrl)
    mt_tap_key: u8,        // HID keycode for tap (0x39 = CapsLock)
    mt_delay_ms: f64,      // 100 to 1000 ms (default 400)
    dks_start_mm: f64,     // Palier 1
    dks_bottom_mm: f64,    // Palier 2
    dks_action1_hid: u8,   // Action au palier 1
    dks_action2_hid: u8,   // Action au palier 2

    // Section 4: Mappage
    remap_category: RemapCategory,

    // Section 6: Sauvegardes
    backup_files: Vec<PathBuf>,
}

impl Component for AppModel {
    type Input = ();
    type Message = Message;

    fn create(_input: &(), context: &ComponentContext<Self>) -> Self {
        let mut app = Self {
            current_section: Section::Performance,
            connected: false,
            device_name: "Ajazz AK980 Max".to_string(),
            firmware_version: "Non détecté".to_string(),
            pid_hex: "----".to_string(),
            status_detail: "Interrogation du port HID USB...".to_string(),

            layout_preference: LayoutPreference::Auto,
            selected_key: None,
            pressed_keys: HashSet::new(),

            keys_map: KeyMap::default(),
            custom_leds: CustomLeds::default(),
            dks_table: DksTable::default(),

            // Lighting defaults
            lighting_tab: LightingSubTab::Effects,
            led_mode_index: 0,
            led_brightness: 4.0,
            led_speed: 3.0,
            led_direction: 0,
            led_color_mode: 0,
            led_r: 0,
            led_g: 180,
            led_b: 255,
            paint_brush_color: Rgb { r: 0, g: 180, b: 255 },

            // Performance defaults
            apply_globally: true,
            switch_type: 1, // Axis 9
            actuation_mm: 1.8,
            rapid_trigger: true,
            press_rt_mm: 0.20,
            release_rt_mm: 0.20,
            top_deadzone_mm: 0.05,
            bottom_deadzone_mm: 0.05,
            polling_rate_hz: 8000,

            // Advanced keys defaults
            advanced_tab: AdvancedSubTab::Socd,
            socd_mode: 3,  // Last Win / Snap Tap
            socd_key1: 49, // KeyA
            socd_key2: 51, // KeyD
            mt_hold_key: 0xE0, // Left Ctrl
            mt_tap_key: 0x39,  // Caps Lock
            mt_delay_ms: 400.0,
            dks_start_mm: 1.0,
            dks_bottom_mm: 3.0,
            dks_action1_hid: 0x04, // 'A'
            dks_action2_hid: 0xE1, // Left Shift

            // Remap
            remap_category: RemapCategory::Keyboard,

            // Backups
            backup_files: Vec::new(),
        };

        app.check_device();
        app.refresh_backups();
        let pref = app.layout_preference;
        context.spawn_background(move |token| Message::KeysPolled(input::poll_keys(token, pref)));
        app
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::KeysPolled(detected) => {
                for idx in detected {
                    self.pressed_keys.insert(idx);
                }
                let pref = self.layout_preference;
                context.spawn_background(move |token| Message::KeysPolled(input::poll_keys(token, pref)));
            }
            Message::SelectSection(sec) => {
                self.current_section = sec;
                if sec == Section::Lighting {
                    match self.lighting_tab {
                        LightingSubTab::Effects => self.preview_lighting(),
                        LightingSubTab::CustomPaint => self.preview_custom_leds(),
                    }
                }
            }
            Message::SetLayoutPreference(pref) => {
                self.layout_preference = pref;
            }
            Message::SelectKey(idx) => {
                // If in custom paint mode, clicking a key immediately paints it and sends preview!
                if self.current_section == Section::Lighting && self.lighting_tab == LightingSubTab::CustomPaint {
                    self.custom_leds.0[usize::from(idx)] = self.paint_brush_color;
                    self.preview_custom_leds();
                } else if self.current_section == Section::Test {
                    if self.pressed_keys.contains(&idx) {
                        self.pressed_keys.remove(&idx);
                    } else {
                        self.pressed_keys.insert(idx);
                    }
                } else {
                    self.selected_key = Some(idx);
                    // Pre-fill DKS if key has one
                    if let Some(k) = Layout::ak980max().key(idx) {
                        self.dks_action1_hid = k.hid;
                    }
                }
            }
            Message::TestFnKey => {
                if self.pressed_keys.contains(&85) {
                    self.pressed_keys.remove(&85);
                } else {
                    self.pressed_keys.insert(85);
                }
            }
            Message::CheckDevice => {
                self.check_device();
            }

            // Lighting updates (with live hardware preview)
            Message::SelectLightingSubTab(tab) => {
                self.lighting_tab = tab;
                match tab {
                    LightingSubTab::Effects => self.preview_lighting(),
                    LightingSubTab::CustomPaint => self.preview_custom_leds(),
                }
            }
            Message::SelectEffect(idx) => {
                self.led_mode_index = idx.min(all_effects().len().saturating_sub(1));
                if let Some(eff) = find_effect(all_effects()[self.led_mode_index].mode) {
                    if let Some(s) = eff.default_speed {
                        self.led_speed = s as f64;
                    }
                    if let Some(d) = eff.default_direction {
                        self.led_direction = d;
                    }
                }
                self.preview_lighting();
            }
            Message::SetBrightness(val) => {
                self.led_brightness = val.clamp(1.0, 5.0).round();
                self.preview_lighting();
            }
            Message::SetSpeed(val) => {
                self.led_speed = val.clamp(1.0, 5.0).round();
                self.preview_lighting();
            }
            Message::SetColorMode(mode) => {
                self.led_color_mode = mode;
                self.preview_lighting();
            }
            Message::SetDirection(dir) => {
                self.led_direction = dir;
                self.preview_lighting();
            }
            Message::SetColor(r, g, b) => {
                self.led_r = r;
                self.led_g = g;
                self.led_b = b;
                self.led_color_mode = 0; // single color
                self.preview_lighting();
            }
            Message::SetColorPreset(r, g, b) => {
                self.led_r = r;
                self.led_g = g;
                self.led_b = b;
                self.led_color_mode = 0; // single color
                self.preview_lighting();
            }
            Message::SetHexColor(hex) => {
                if let Some((r, g, b)) = parse_hex_color(&hex) {
                    self.led_r = r;
                    self.led_g = g;
                    self.led_b = b;
                    self.led_color_mode = 0;
                    self.preview_lighting();
                }
            }
            Message::SetColorChannelR(val) => {
                self.led_r = val.clamp(0.0, 255.0).round() as u8;
                self.led_color_mode = 0;
                self.preview_lighting();
            }
            Message::SetColorChannelG(val) => {
                self.led_g = val.clamp(0.0, 255.0).round() as u8;
                self.led_color_mode = 0;
                self.preview_lighting();
            }
            Message::SetColorChannelB(val) => {
                self.led_b = val.clamp(0.0, 255.0).round() as u8;
                self.led_color_mode = 0;
                self.preview_lighting();
            }
            Message::ApplyLighting => {
                self.apply_lighting();
            }

            // Custom LED Painting (with live hardware preview)
            Message::SetBrushColor(r, g, b) => {
                self.paint_brush_color = Rgb { r, g, b };
            }
            Message::SetBrushHexColor(hex) => {
                if let Some((r, g, b)) = parse_hex_color(&hex) {
                    self.paint_brush_color = Rgb { r, g, b };
                }
            }
            Message::PaintKey(idx) => {
                self.custom_leds.0[usize::from(idx)] = self.paint_brush_color;
                self.preview_custom_leds();
            }
            Message::PaintAllKeys => {
                for c in self.custom_leds.0.iter_mut() {
                    *c = self.paint_brush_color;
                }
                self.preview_custom_leds();
            }
            Message::ClearAllCustomLeds => {
                for c in self.custom_leds.0.iter_mut() {
                    *c = Rgb { r: 0, g: 0, b: 0 };
                }
                self.preview_custom_leds();
            }
            Message::ApplyCustomLeds => {
                self.apply_custom_leds();
            }

            // Performance updates
            Message::ToggleApplyGlobally(val) => {
                self.apply_globally = val;
            }
            Message::SelectSwitchType(st) => {
                self.switch_type = st;
            }
            Message::SetActuation(val) => {
                self.actuation_mm = (val * 100.0).round() / 100.0;
            }
            Message::ToggleRapidTrigger(val) => {
                self.rapid_trigger = val;
            }
            Message::SetPressRt(val) => {
                self.press_rt_mm = (val * 100.0).round() / 100.0;
            }
            Message::SetReleaseRt(val) => {
                self.release_rt_mm = (val * 100.0).round() / 100.0;
            }
            Message::SetTopDeadzone(val) => {
                self.top_deadzone_mm = (val * 100.0).round() / 100.0;
            }
            Message::SetBottomDeadzone(val) => {
                self.bottom_deadzone_mm = (val * 100.0).round() / 100.0;
            }
            Message::SelectReportRate(hz) => {
                self.polling_rate_hz = hz;
            }
            Message::ApplyPerformance => {
                self.apply_performance();
            }

            // Remap Keys updates
            Message::SelectRemapCategory(cat) => {
                self.remap_category = cat;
            }
            Message::AssignKeyAction(act) => {
                if let Some(idx) = self.selected_key {
                    self.keys_map.0[usize::from(idx)] = act;
                    self.status_detail = format!("Touche #{} configurée en attente d'application USB.", idx);
                }
            }
            Message::ResetSelectedKeyAction => {
                if let Some(idx) = self.selected_key {
                    self.keys_map.0[usize::from(idx)] = KeyAction::default();
                    self.status_detail = format!("Touche #{} réinitialisée à son comportement d'origine.", idx);
                }
            }
            Message::ResetAllKeysDefault => {
                self.keys_map = KeyMap::default();
                self.status_detail = "Toutes les touches ont été remises à zéro (prêtes pour USB).".to_string();
            }
            Message::ApplyKeyMap => {
                self.apply_key_map();
            }

            // Advanced Keys updates
            Message::SelectAdvancedTab(tab) => {
                self.advanced_tab = tab;
            }
            Message::SelectSocdMode(mode) => {
                self.socd_mode = mode;
            }
            Message::SetSocdPreset(k1, k2) => {
                self.socd_key1 = k1;
                self.socd_key2 = k2;
            }
            Message::ApplySocd => {
                self.apply_socd();
            }
            Message::ClearSocd => {
                self.clear_socd();
            }
            Message::SetModTapHold(hid) => {
                self.mt_hold_key = hid;
            }
            Message::SetModTapTap(hid) => {
                self.mt_tap_key = hid;
            }
            Message::SetModTapDelay(ms) => {
                self.mt_delay_ms = (ms / 10.0).round() * 10.0;
            }
            Message::ApplyModTap => {
                self.apply_mod_tap();
            }
            Message::ClearModTap => {
                self.clear_selected_key_action();
            }
            Message::ApplyToggle => {
                self.apply_toggle();
            }
            Message::ClearToggle => {
                self.clear_selected_key_action();
            }

            // DKS updates
            Message::SetDksStartMm(val) => {
                self.dks_start_mm = (val * 10.0).round() / 10.0;
            }
            Message::SetDksBottomMm(val) => {
                self.dks_bottom_mm = (val * 10.0).round() / 10.0;
            }
            Message::SetDksAction1(hid) => {
                self.dks_action1_hid = hid;
            }
            Message::SetDksAction2(hid) => {
                self.dks_action2_hid = hid;
            }
            Message::ApplyDks => {
                self.apply_dks();
            }
            Message::ClearDks => {
                self.clear_dks();
            }

            // Backups
            Message::CreateBackup => {
                self.create_backup();
            }
            Message::RestoreBackup(path_str) => {
                self.restore_backup(&path_str);
            }
            Message::RefreshBackups => {
                self.refresh_backups();
            }
            Message::OpenBackupFolder => {
                self.open_backup_folder();
            }

            // Test
            Message::ClearTest => {
                self.pressed_keys.clear();
            }
        }
    }

    fn view(&self, _input: &(), context: &mut ViewContext<Self>) -> View {
        context.window_title("Freejazz — Ajazz AK980 Max (WinUI 3)");
        context.window_visuals(
            WindowVisuals::new()
                .backdrop(WindowBackdrop::Mica)
                .client_size(1280.0, 920.0),
        );

        Grid::new()
            .columns([GridLength::Pixel(250.0), GridLength::Star(1.0)])
            .children((
                self.render_sidebar(context),
                ScrollViewer::new()
                    .grid_column(1)
                    .content(self.render_main_content(context)),
            ))
            .into()
    }
}

impl AppModel {
    fn render_sidebar(&self, context: &ViewContext<Self>) -> View {
        let status_dot = if self.connected { "Connecté en USB" } else { "Périphérique déconnecté" };

        let make_nav_btn = |sec: Section, label: &str, symbol: Symbol| {
            let is_active = self.current_section == sec;
            Button::new()
                .horizontal_alignment(HorizontalAlignment::Stretch)
                .horizontal_content_alignment(HorizontalAlignment::Left)
                .style(if is_active { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SelectSection(sec)))
                .content(
                    StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(12.0)
                        .children((
                            SymbolIcon::new().symbol(symbol),
                            TextBlock::new().text(label).font_size(13.5),
                        ))
                )
        };

        Border::new()
            .grid_column(0)
            .border_brush(Color::argb(40, 255, 255, 255))
            .border_thickness(Thickness::new(0.0, 0.0, 1.0, 0.0))
            .padding(16.0)
            .content(
                StackPanel::new()
                    .spacing(10.0)
                    .children((
                        // Brand
                        StackPanel::new().spacing(2.0).margin(Thickness::new(8.0, 8.0, 8.0, 16.0)).children((
                            TextBlock::new().text("Freejazz").font_size(24.0),
                            TextBlock::new().text("AK980 Max Edition").font_size(12.0),
                            TextBlock::new().text(status_dot).font_size(11.5).margin(Thickness::new(0.0, 6.0, 0.0, 0.0)),
                        )),

                        // Nav items (Strictly NO EMOJIS)
                        make_nav_btn(Section::Performance, "Performance & RT", Symbol::Go),
                        make_nav_btn(Section::Lighting, "Éclairage RGB", Symbol::Favorite),
                        make_nav_btn(Section::Advanced, "Touches avancées", Symbol::Switch),
                        make_nav_btn(Section::Keys, "Mappage des touches", Symbol::Keyboard),
                        make_nav_btn(Section::Test, "Test de la matrice", Symbol::Repair),
                        make_nav_btn(Section::Device, "Périphérique & Profils", Symbol::Setting),
                    ))
            )
            .into()
    }

    fn render_main_content(&self, context: &ViewContext<Self>) -> View {
        let page_view = match self.current_section {
            Section::Performance => self.render_performance_page(context),
            Section::Lighting => self.render_lighting_page(context),
            Section::Advanced => self.render_advanced_page(context),
            Section::Keys => self.render_keys_page(context),
            Section::Test => self.render_test_page(context),
            Section::Device => self.render_device_page(context),
        };

        StackPanel::new()
            .spacing(18.0)
            .margin(24.0)
            .children((
                self.render_header_card(context),
                self.render_keyboard_canvas(context),
                page_view,
            ))
            .into()
    }

    fn render_header_card(&self, context: &ViewContext<Self>) -> View {
        let status_color = if self.connected { "Connecté en USB" } else { "Non connecté" };

        let make_pref_btn = |pref: LayoutPreference, label: &str| {
            Button::new()
                .style(if self.layout_preference == pref { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SetLayoutPreference(pref)))
                .content(label)
        };

        Border::new()
            .padding(14.0)
            .corner_radius(8.0)
            .background(Color::argb(35, 255, 255, 255))
            .border_brush(Color::argb(55, 255, 255, 255))
            .border_thickness(1.0)
            .content(
                StackPanel::new().spacing(6.0).children((
                    StackPanel::new().orientation(Orientation::Horizontal).spacing(16.0).children((
                        TextBlock::new()
                            .text(format!("{} — {}", self.device_name, status_color))
                            .font_size(16.0)
                            .font_weight(FontWeight::SEMI_BOLD),
                        
                        // Layout selector on header bar
                        StackPanel::new().orientation(Orientation::Horizontal).spacing(8.0).children((
                            TextBlock::new().text("Disposition :").font_size(12.5).vertical_alignment(VerticalAlignment::Center),
                            make_pref_btn(LayoutPreference::Auto, "Auto (Windows)"),
                            make_pref_btn(LayoutPreference::Azerty, "AZERTY"),
                            make_pref_btn(LayoutPreference::Qwerty, "QWERTY"),
                        )),
                    )),
                    TextBlock::new()
                        .text(&self.status_detail)
                        .font_size(12.0),
                ))
            )
            .into()
    }

    fn render_keyboard_canvas(&self, context: &ViewContext<Self>) -> View {
        let layout = Layout::ak980max();
        let unit: f64 = 45.0;
        let gap: f64 = 3.5;

        let (total_w, total_h) = layout.size();
        let canvas_w = (total_w as f64) * unit;
        let canvas_h = (total_h as f64) * unit;

        let hkl = labels::get_hkl(self.layout_preference);
        let mut key_views = Vec::with_capacity(layout.keys.len());

        let is_custom_paint = self.current_section == Section::Lighting && self.lighting_tab == LightingSubTab::CustomPaint;

        for k in &layout.keys {
            let left = (k.x as f64) * unit;
            let top = (k.y as f64) * unit;
            let width = (k.w as f64) * unit - gap;
            let height = (k.h as f64) * unit - gap;

            let is_selected = self.selected_key == Some(k.index);
            let is_tested = self.pressed_keys.contains(&k.index);
            let is_socd_pair = self.current_section == Section::Advanced
                && (self.socd_key1 == k.index || self.socd_key2 == k.index);

            let key_action = self.keys_map.0.get(usize::from(k.index));
            let is_remapped = key_action.map_or(false, |a| labels::is_remapped(*a, k));

            let is_mod = matches!(
                k.code.as_str(),
                "Escape" | "Tab" | "CapsLock" | "ShiftLeft" | "ShiftRight"
                    | "ControlLeft" | "ControlRight" | "AltLeft" | "AltRight"
                    | "MetaLeft" | "MetaRight" | "Enter" | "Backspace"
                    | "ContextMenu" | "PrintScreen" | "ScrollLock" | "Pause"
                    | "Insert" | "Delete" | "Home" | "End" | "PageUp" | "PageDown"
                    | "NumLock" | "NumpadEnter"
            );

            // Dynamic localized keycap label based on the active layout and any remapped action
            let display_label = labels::resolve_label(k, key_action, hkl);

            let border_brush = if is_selected {
                Color::rgb(0, 180, 255)
            } else if is_tested {
                Color::rgb(74, 222, 128)
            } else if is_socd_pair {
                Color::rgb(249, 115, 22)
            } else if is_remapped {
                Color::rgb(192, 132, 252) // Light purple for remapped keys
            } else if is_mod {
                Color::argb(255, 115, 128, 148)
            } else {
                Color::argb(255, 138, 152, 178)
            };

            let bg_color = if is_custom_paint {
                let rgb = self.custom_leds.0[usize::from(k.index)];
                if rgb.r == 0 && rgb.g == 0 && rgb.b == 0 {
                    Color::argb(255, 35, 40, 50)
                } else {
                    Color::rgb(rgb.r, rgb.g, rgb.b)
                }
            } else if is_selected {
                Color::rgb(0, 120, 215)
            } else if is_tested {
                Color::rgb(34, 197, 94) // Emerald green
            } else if is_socd_pair {
                Color::rgb(194, 65, 12)
            } else if is_remapped {
                Color::argb(255, 88, 55, 125) // Deep purple for remapped keys
            } else if is_mod {
                Color::argb(255, 46, 52, 64)
            } else {
                Color::argb(255, 66, 75, 92)
            };

            let text_color = if is_tested {
                Color::rgb(15, 23, 42)
            } else {
                Color::rgb(250, 252, 255)
            };

            let label_max_line = display_label.lines().map(|l| l.chars().count()).max().unwrap_or(0);
            let label_font_size = if display_label.contains('\n') {
                8.5
            } else if label_max_line <= 1 {
                11.5
            } else if label_max_line <= 3 {
                9.5
            } else if label_max_line <= 5 {
                8.5
            } else {
                7.5
            };

            let k_index = k.index;
            let key_item = Border::new()
                .canvas_left(left)
                .canvas_top(top)
                .width(width)
                .height(height)
                .corner_radius(5.0)
                .border_brush(border_brush)
                .border_thickness(if is_selected || is_tested || is_socd_pair || is_remapped { 2.0 } else { 1.2 })
                .background(bg_color)
                .padding(1.0)
                .on_pointer_pressed(context.callback(move |_| Message::SelectKey(k_index)))
                .content(
                    TextBlock::new()
                        .text(display_label)
                        .font_size(label_font_size)
                        .font_weight(FontWeight::SEMI_BOLD)
                        .foreground(text_color)
                        .horizontal_alignment(HorizontalAlignment::Center)
                        .vertical_alignment(VerticalAlignment::Center)
                );

            key_views.push(KeyedView::new(format!("k_{}", k.index), key_item));
        }

        Border::new()
            .padding(18.0)
            .corner_radius(12.0)
            .background(Color::argb(255, 30, 34, 42))
            .border_brush(Color::argb(255, 105, 120, 145))
            .border_thickness(2.5)
            .content(
                Canvas::new()
                    .width(canvas_w)
                    .height(canvas_h)
                    .keyed_children(key_views)
            )
            .into()
    }

    // ==========================================
    // PAGE 1: PERFORMANCE & RT
    // ==========================================
    fn render_performance_page(&self, context: &ViewContext<Self>) -> View {
        let key_info = if let Some(idx) = self.selected_key {
            let label = Layout::ak980max()
                .key(idx)
                .map(|k| k.label.clone())
                .unwrap_or_else(|| format!("#{}", idx));
            format!("Touche sélectionnée : [{}] (slot #{})", label, idx)
        } else {
            "Aucune touche sélectionnée — configuration globale active".to_string()
        };

        let make_switch_btn = |st: u8, name: &str| {
            Button::new()
                .style(if self.switch_type == st { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SelectSwitchType(st)))
                .content(name)
        };

        let make_rate_btn = |hz: u32| {
            Button::new()
                .style(if self.polling_rate_hz == hz { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SelectReportRate(hz)))
                .content(format!("{} Hz", hz))
        };

        let make_scope_btn = |global: bool, text: &str| {
            Button::new()
                .style(if self.apply_globally == global { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::ToggleApplyGlobally(global)))
                .content(text)
        };

        let rt_controls: View = if self.rapid_trigger {
            View::from(StackPanel::new().spacing(12.0).children((
                TextBlock::new()
                    .text(format!("Sensibilité à la frappe (Press RT) : {:.2} mm", self.press_rt_mm))
                    .font_size(13.0),
                Slider::new()
                    .minimum(0.01)
                    .maximum(2.00)
                    .step_frequency(0.01)
                    .value(self.press_rt_mm)
                    .on_value_changed(context.callback(Message::SetPressRt)),

                TextBlock::new()
                    .text(format!("Sensibilité au relâchement (Release RT) : {:.2} mm", self.release_rt_mm))
                    .font_size(13.0),
                Slider::new()
                    .minimum(0.01)
                    .maximum(2.00)
                    .step_frequency(0.01)
                    .value(self.release_rt_mm)
                    .on_value_changed(context.callback(Message::SetReleaseRt)),
            )))
        } else {
            View::from(StackPanel::new())
        };

        Border::new()
            .padding(20.0)
            .corner_radius(8.0)
            .background(Color::argb(35, 255, 255, 255))
            .border_brush(Color::argb(45, 255, 255, 255))
            .border_thickness(1.0)
            .content(
                StackPanel::new().spacing(16.0).children((
                    TextBlock::new()
                        .text("Switches Magnétiques & Rapid Trigger (Hall Effect)")
                        .font_size(18.0)
                        .font_weight(FontWeight::SEMI_BOLD),

                    // Target Scope
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new().text("Portée de l'application des réglages :").font_size(13.5),
                        StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                            make_scope_btn(true, "Toutes les touches (Global)"),
                            make_scope_btn(false, "Touche sélectionnée uniquement"),
                        )),
                        TextBlock::new().text(key_info).font_size(12.0),
                    )),

                    // Switch Type
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new().text("Type de switch magnétique installé :").font_size(13.5),
                        StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                            make_switch_btn(1, "Switch magnétique riz (Axis 9)"),
                            make_switch_btn(2, "Axe Jade Pro"),
                            make_switch_btn(3, "Axe Magicien (King)"),
                        )),
                    )),

                    // Actuation Point
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new()
                            .text(format!("Point d'actionnement initial : {:.2} mm", self.actuation_mm))
                            .font_size(13.5),
                        Slider::new()
                            .minimum(0.10)
                            .maximum(3.30)
                            .step_frequency(0.05)
                            .value(self.actuation_mm)
                            .on_value_changed(context.callback(Message::SetActuation)),
                    )),

                    // Rapid Trigger
                    StackPanel::new().spacing(8.0).children((
                        StackPanel::new().orientation(Orientation::Horizontal).spacing(12.0).children((
                            ToggleSwitch::new()
                                .is_on(self.rapid_trigger)
                                .on_toggled(context.callback(Message::ToggleRapidTrigger)),
                            TextBlock::new()
                                .text(if self.rapid_trigger { "Rapid Trigger : Actif (Détection dynamique continue)" } else { "Rapid Trigger : Inactif" })
                                .font_size(13.5),
                        )),
                        rt_controls,
                    )),

                    // Deadzones (GameMode)
                    StackPanel::new().spacing(8.0).children((
                        TextBlock::new().text("Zones mortes matérielles (GameMode) :").font_size(13.5),
                        TextBlock::new().text(format!("Zone morte supérieure : {:.2} mm", self.top_deadzone_mm)).font_size(12.5),
                        Slider::new()
                            .minimum(0.00)
                            .maximum(0.50)
                            .step_frequency(0.01)
                            .value(self.top_deadzone_mm)
                            .on_value_changed(context.callback(Message::SetTopDeadzone)),

                        TextBlock::new().text(format!("Zone morte inférieure : {:.2} mm", self.bottom_deadzone_mm)).font_size(12.5),
                        Slider::new()
                            .minimum(0.00)
                            .maximum(0.50)
                            .step_frequency(0.01)
                            .value(self.bottom_deadzone_mm)
                            .on_value_changed(context.callback(Message::SetBottomDeadzone)),
                    )),

                    // Polling Rate
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new().text("Fréquence de scrutation USB (Polling Rate) :").font_size(13.5),
                        StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                            make_rate_btn(1000),
                            make_rate_btn(2000),
                            make_rate_btn(4000),
                            make_rate_btn(8000),
                        )),
                    )),

                    Button::new()
                        .style(ButtonStyle::Accent)
                        .on_click(context.message(Message::ApplyPerformance))
                        .content("Enregistrer dans la mémoire flash (USB)"),
                ))
            )
            .into()
    }

    // ==========================================
    // PAGE 2: ÉCLAIRAGE RGB (19 EFFECTS + CUSTOM PAINT)
    // ==========================================
    fn render_lighting_page(&self, context: &ViewContext<Self>) -> View {
        let make_subtab_btn = |tab: LightingSubTab, label: &str| {
            Button::new()
                .style(if self.lighting_tab == tab { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SelectLightingSubTab(tab)))
                .content(label)
        };

        let sub_view = match self.lighting_tab {
            LightingSubTab::Effects => self.render_lighting_effects_subtab(context),
            LightingSubTab::CustomPaint => self.render_lighting_custom_paint_subtab(context),
        };

        Border::new()
            .padding(20.0)
            .corner_radius(8.0)
            .background(Color::argb(35, 255, 255, 255))
            .border_brush(Color::argb(45, 255, 255, 255))
            .border_thickness(1.0)
            .content(
                StackPanel::new().spacing(16.0).children((
                    TextBlock::new()
                        .text("Éclairage RGB & Rétroéclairage")
                        .font_size(18.0)
                        .font_weight(FontWeight::SEMI_BOLD),

                    // Navigation entre 19 animations et peinture touche par touche
                    StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                        make_subtab_btn(LightingSubTab::Effects, "Animations intégrées (19 effets)"),
                        make_subtab_btn(LightingSubTab::CustomPaint, "Peinture personnalisée (Touche par touche)"),
                    )),

                    sub_view,
                ))
            )
            .into()
    }

    fn render_lighting_effects_subtab(&self, context: &ViewContext<Self>) -> View {
        let effects_list = all_effects();
        let current_effect: Option<&Effect> = effects_list.get(self.led_mode_index);
        let eff_mode = current_effect.map_or(1, |e| e.mode);
        let eff_has_speed = current_effect.map_or(false, |e| e.has_speed);
        let eff_has_dir = current_effect.map_or(false, |e| e.has_direction);
        let eff_has_color = current_effect.map_or(false, |e| e.has_color);
        let eff_color_modes = current_effect.map_or(&[] as &[u8], |e| &e.color_modes);
        let supports_single_and_rgb = eff_color_modes.contains(&0) && eff_color_modes.contains(&1);

        let make_dir_btn = |dir: u8| {
            Button::new()
                .style(if self.led_direction == dir { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SetDirection(dir)))
                .content(direction_name(dir))
        };

        let make_cm_btn = |mode: u8, name: &str| {
            Button::new()
                .style(if self.led_color_mode == mode { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SetColorMode(mode)))
                .content(name)
        };

        let make_swatch = |r: u8, g: u8, b: u8, name: &str| {
            Button::new()
                .style(ButtonStyle::Subtle)
                .on_click(context.message(Message::SetColorPreset(r, g, b)))
                .content(
                    StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(8.0)
                        .children((
                            Border::new()
                                .width(14.0)
                                .height(14.0)
                                .corner_radius(3.0)
                                .background(Color::rgb(r, g, b))
                                .border_brush(Color::rgb(255, 255, 255))
                                .border_thickness(1.0),
                            TextBlock::new().text(name).font_size(12.0),
                        ))
                )
        };

        let mut effect_cards = Vec::with_capacity(effects_list.len());
        for (i, eff) in effects_list.iter().enumerate() {
            let is_selected = self.led_mode_index == i;
            let icon_view: View = if let Some(enc) = icons::effect_icon(eff.mode) {
                View::from(Image::new().source_data(enc).width(20.0).height(20.0))
            } else {
                View::from(Border::new().width(20.0).height(20.0))
            };

            let card = Border::new()
                .margin(Thickness::new(0.0, 0.0, 8.0, 8.0))
                .padding(Thickness::new(6.0, 4.0, 6.0, 4.0))
                .corner_radius(6.0)
                .background(if is_selected { Color::argb(70, 0, 120, 215) } else { Color::argb(25, 255, 255, 255) })
                .border_brush(if is_selected { Color::rgb(0, 180, 255) } else { Color::argb(40, 255, 255, 255) })
                .border_thickness(if is_selected { 1.5 } else { 1.0 })
                .content(
                    Button::new()
                        .style(ButtonStyle::Subtle)
                        .horizontal_alignment(HorizontalAlignment::Stretch)
                        .on_click(context.message(Message::SelectEffect(i)))
                        .content(
                            StackPanel::new()
                                .orientation(Orientation::Horizontal)
                                .spacing(10.0)
                                .children((
                                    icon_view,
                                    TextBlock::new()
                                        .text(format!("{:02}. {}", eff.mode, eff.fr))
                                        .font_size(12.5)
                                        .font_weight(if is_selected { FontWeight::SEMI_BOLD } else { FontWeight::NORMAL })
                                        .vertical_alignment(VerticalAlignment::Center),
                                ))
                        )
                );

            effect_cards.push(KeyedView::new(format!("eff_{}", eff.mode), card));
        }

        let speed_ctrl: View = if eff_has_speed {
            View::from(StackPanel::new().spacing(6.0).children((
                TextBlock::new()
                    .text(format!("Vitesse de l'animation : Niveau {} / 5", self.led_speed as u32))
                    .font_size(13.5),
                Slider::new()
                    .minimum(1.0)
                    .maximum(5.0)
                    .step_frequency(1.0)
                    .value(self.led_speed)
                    .on_value_changed(context.callback(Message::SetSpeed)),
            )))
        } else {
            View::from(StackPanel::new())
        };

        let dir_ctrl: View = if eff_has_dir {
            View::from(StackPanel::new().spacing(6.0).children((
                TextBlock::new().text("Direction de propagation de la lumière :").font_size(13.5),
                StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                    make_dir_btn(0),
                    make_dir_btn(1),
                    make_dir_btn(2),
                    make_dir_btn(3),
                )),
            )))
        } else {
            View::from(StackPanel::new())
        };

        let color_ctrl: View = if eff_has_color {
            let cm_toggle: View = if supports_single_and_rgb {
                View::from(StackPanel::new().spacing(6.0).children((
                    TextBlock::new().text("Mode de rendu des couleurs :").font_size(13.5),
                    StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                        make_cm_btn(0, "Couleur unique personnalisée"),
                        make_cm_btn(1, "Spectre multicolore RGB"),
                    )),
                )))
            } else {
                View::from(StackPanel::new())
            };

            let palette: View = if self.led_color_mode == 0 {
                let hex_str = format!("#{:02X}{:02X}{:02X}", self.led_r, self.led_g, self.led_b);
                View::from(StackPanel::new().spacing(12.0).children((
                    TextBlock::new()
                        .text("Sélection de la couleur personnalisée :")
                        .font_size(13.5)
                        .font_weight(FontWeight::SEMI_BOLD),

                    // Hex display and direct text edit
                    Border::new()
                        .padding(Thickness::new(10.0, 8.0, 10.0, 8.0))
                        .corner_radius(6.0)
                        .background(Color::argb(35, 255, 255, 255))
                        .border_brush(Color::argb(60, 255, 255, 255))
                        .border_thickness(1.0)
                        .content(
                            StackPanel::new()
                                .orientation(Orientation::Horizontal)
                                .spacing(12.0)
                                .vertical_alignment(VerticalAlignment::Center)
                                .children((
                                    Border::new()
                                        .width(26.0)
                                        .height(26.0)
                                        .corner_radius(4.0)
                                        .background(Color::rgb(self.led_r, self.led_g, self.led_b))
                                        .border_brush(Color::rgb(255, 255, 255))
                                        .border_thickness(1.5),
                                    TextBlock::new()
                                        .text(format!("Hexadécimal : {}  (R: {}, G: {}, B: {})", hex_str, self.led_r, self.led_g, self.led_b))
                                        .font_size(13.5)
                                        .font_weight(FontWeight::SEMI_BOLD)
                                        .vertical_alignment(VerticalAlignment::Center),
                                    TextBox::new()
                                        .placeholder_text("#RRGGBB")
                                        .text(hex_str)
                                        .width(105.0)
                                        .on_text_changed(context.callback(Message::SetHexColor)),
                                ))
                        ),

                    // Interactive WinUI ColorPicker (wheel / spectrum, sliders, hex input)
                    ColorPicker::new()
                        .color(Color::rgb(self.led_r, self.led_g, self.led_b))
                        .is_alpha_enabled(false)
                        .is_hex_input_visible(true)
                        .is_color_slider_visible(true)
                        .is_color_channel_text_input_visible(true)
                        .on_color_changed(context.callback(|c: Color| Message::SetColor(c.r, c.g, c.b))),

                    TextBlock::new().text("Palette de teintes rapides :").font_size(12.5),
                    StackPanel::new().orientation(Orientation::Horizontal).spacing(8.0).children((
                        make_swatch(239, 68, 68, "Rouge"),
                        make_swatch(249, 115, 22, "Orange"),
                        make_swatch(234, 179, 8, "Jaune"),
                        make_swatch(34, 197, 94, "Vert"),
                        make_swatch(6, 182, 212, "Cyan"),
                        make_swatch(59, 130, 246, "Bleu"),
                        make_swatch(139, 92, 246, "Violet"),
                        make_swatch(236, 72, 153, "Rose"),
                        make_swatch(255, 255, 255, "Blanc"),
                    )),
                )))
            } else {
                View::from(StackPanel::new())
            };

            View::from(StackPanel::new().spacing(10.0).children((
                cm_toggle,
                palette,
            )))
        } else {
            View::from(StackPanel::new())
        };

        let selected_header_logo: View = if let Some(enc) = icons::effect_icon(eff_mode) {
            View::from(Image::new().source_data(enc).width(28.0).height(28.0))
        } else {
            View::from(StackPanel::new())
        };

        let selected_eff_name = current_effect.map_or("Inconnu", |e| e.fr.as_str());

        StackPanel::new().spacing(14.0).children((
            // Gallery of effects with official icons
            StackPanel::new().spacing(8.0).children((
                TextBlock::new().text("Sélectionnez une animation (avec logos officiels du constructeur) :").font_size(13.5),
                VariableSizedWrapGrid::new()
                    .item_width(225.0)
                    .item_height(46.0)
                    .orientation(Orientation::Horizontal)
                    .keyed_children(effect_cards),
            )),

            // Selected effect banner with logo
            Border::new()
                .padding(12.0)
                .corner_radius(6.0)
                .background(Color::argb(40, 0, 120, 215))
                .border_brush(Color::rgb(0, 180, 255))
                .border_thickness(1.0)
                .content(
                    StackPanel::new().orientation(Orientation::Horizontal).spacing(12.0).children((
                        selected_header_logo,
                        TextBlock::new()
                            .text(format!("Animation sélectionnée : Effet #{} — {}", eff_mode, selected_eff_name))
                            .font_size(14.0)
                            .font_weight(FontWeight::SEMI_BOLD)
                            .vertical_alignment(VerticalAlignment::Center),
                    ))
                ),

            // Brightness
            StackPanel::new().spacing(6.0).children((
                TextBlock::new()
                    .text(format!("Luminosité du clavier : Niveau {} / 5", self.led_brightness as u32))
                    .font_size(13.5),
                Slider::new()
                    .minimum(1.0)
                    .maximum(5.0)
                    .step_frequency(1.0)
                    .value(self.led_brightness)
                    .on_value_changed(context.callback(Message::SetBrightness)),
            )),

            speed_ctrl,
            dir_ctrl,
            color_ctrl,

            Button::new()
                .style(ButtonStyle::Accent)
                .on_click(context.message(Message::ApplyLighting))
                .content("Appliquer l'éclairage au clavier (USB)"),
        )).into()
    }

    fn render_lighting_custom_paint_subtab(&self, context: &ViewContext<Self>) -> View {
        let make_swatch = |r: u8, g: u8, b: u8, name: &str| {
            let is_curr = self.paint_brush_color.r == r && self.paint_brush_color.g == g && self.paint_brush_color.b == b;
            Button::new()
                .style(if is_curr { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SetBrushColor(r, g, b)))
                .content(
                    StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(8.0)
                        .children((
                            Border::new()
                                .width(14.0)
                                .height(14.0)
                                .corner_radius(3.0)
                                .background(Color::rgb(r, g, b))
                                .border_brush(Color::rgb(255, 255, 255))
                                .border_thickness(1.0),
                            TextBlock::new().text(name).font_size(12.0),
                        ))
                )
        };

        StackPanel::new().spacing(14.0).children((
            TextBlock::new()
                .text("Mode Peinture Touche par Touche : Choisissez une couleur de pinceau ci-dessous, puis cliquez directement sur les touches du clavier virtuel ci-dessus pour peindre vos zones de jeu.")
                .font_size(13.0),

            // Current brush preview
            Border::new()
                .padding(12.0)
                .corner_radius(6.0)
                .background(Color::argb(25, 255, 255, 255))
                .border_brush(Color::argb(45, 255, 255, 255))
                .border_thickness(1.0)
                .content(
                    StackPanel::new().orientation(Orientation::Horizontal).spacing(12.0).children((
                        Border::new()
                            .width(28.0)
                            .height(28.0)
                            .corner_radius(4.0)
                            .background(Color::rgb(self.paint_brush_color.r, self.paint_brush_color.g, self.paint_brush_color.b))
                            .border_brush(Color::rgb(255, 255, 255))
                            .border_thickness(1.5),
                        TextBlock::new()
                            .text(format!(
                                "Couleur actuelle du pinceau : #{:02X}{:02X}{:02X} (R={}, G={}, B={})",
                                self.paint_brush_color.r, self.paint_brush_color.g, self.paint_brush_color.b,
                                self.paint_brush_color.r, self.paint_brush_color.g, self.paint_brush_color.b
                            ))
                            .font_size(13.5)
                            .font_weight(FontWeight::SEMI_BOLD)
                            .vertical_alignment(VerticalAlignment::Center),
                        TextBox::new()
                            .placeholder_text("#RRGGBB")
                            .text(format!("#{:02X}{:02X}{:02X}", self.paint_brush_color.r, self.paint_brush_color.g, self.paint_brush_color.b))
                            .width(105.0)
                            .on_text_changed(context.callback(Message::SetBrushHexColor)),
                    ))
                ),

            // Native WinUI 3 ColorPicker for Brush
            ColorPicker::new()
                .color(Color::rgb(self.paint_brush_color.r, self.paint_brush_color.g, self.paint_brush_color.b))
                .is_alpha_enabled(false)
                .is_hex_input_visible(true)
                .is_color_slider_visible(true)
                .is_color_channel_text_input_visible(true)
                .on_color_changed(context.callback(|c: Color| Message::SetBrushColor(c.r, c.g, c.b))),

            // Swatches
            StackPanel::new().spacing(8.0).children((
                TextBlock::new().text("Sélectionnez une teinte pour peindre :").font_size(13.0),
                StackPanel::new().orientation(Orientation::Horizontal).spacing(8.0).children((
                    make_swatch(239, 68, 68, "Rouge"),
                    make_swatch(249, 115, 22, "Orange"),
                    make_swatch(234, 179, 8, "Jaune"),
                    make_swatch(34, 197, 94, "Vert"),
                    make_swatch(6, 182, 212, "Cyan"),
                    make_swatch(59, 130, 246, "Bleu"),
                    make_swatch(139, 92, 246, "Violet"),
                    make_swatch(236, 72, 153, "Rose"),
                    make_swatch(255, 255, 255, "Blanc"),
                    make_swatch(0, 0, 0, "Éteint (Noir)"),
                )),
            )),

            // Quick actions
            StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                Button::new()
                    .on_click(context.message(Message::PaintAllKeys))
                    .content("Tout peindre avec la couleur active"),
                Button::new()
                    .on_click(context.message(Message::ClearAllCustomLeds))
                    .content("Éteindre toutes les touches"),
            )),

            Button::new()
                .style(ButtonStyle::Accent)
                .on_click(context.message(Message::ApplyCustomLeds))
                .content("Enregistrer et activer les couleurs personnalisées sur le clavier (USB)"),
        )).into()
    }

    // ==========================================
    // PAGE 3: TOUCHES AVANCÉES (SOCD, MOD-TAP, TOGGLE, DKS)
    // ==========================================
    fn render_advanced_page(&self, context: &ViewContext<Self>) -> View {
        let sub_content = match self.advanced_tab {
            AdvancedSubTab::Socd => self.render_socd_subtab(context),
            AdvancedSubTab::ModTap => self.render_mod_tap_subtab(context),
            AdvancedSubTab::Toggle => self.render_toggle_subtab(context),
            AdvancedSubTab::Dks => self.render_dks_subtab(context),
        };

        let make_tab_btn = |tab: AdvancedSubTab, name: &str| {
            Button::new()
                .style(if self.advanced_tab == tab { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SelectAdvancedTab(tab)))
                .content(name)
        };

        Border::new()
            .padding(20.0)
            .corner_radius(8.0)
            .background(Color::argb(35, 255, 255, 255))
            .border_brush(Color::argb(45, 255, 255, 255))
            .border_thickness(1.0)
            .content(
                StackPanel::new().spacing(16.0).children((
                    TextBlock::new()
                        .text("Touches Avancées & Gaming Compétitif")
                        .font_size(18.0)
                        .font_weight(FontWeight::SEMI_BOLD),

                    // Sub-navigation bar
                    StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                        make_tab_btn(AdvancedSubTab::Socd, "SOCD / Snap Tap"),
                        make_tab_btn(AdvancedSubTab::ModTap, "Mod-Tap"),
                        make_tab_btn(AdvancedSubTab::Toggle, "Toggle (Verrouillage)"),
                        make_tab_btn(AdvancedSubTab::Dks, "DKS (Dynamic Keystroke)"),
                    )),

                    sub_content,
                ))
            )
            .into()
    }

    fn render_socd_subtab(&self, context: &ViewContext<Self>) -> View {
        let make_preset_btn = |k1: u8, k2: u8, label: &str| {
            let active = self.socd_key1 == k1 && self.socd_key2 == k2;
            Button::new()
                .style(if active { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SetSocdPreset(k1, k2)))
                .content(label)
        };

        let make_mode_btn = |mode: u8, label: &str| {
            Button::new()
                .style(if self.socd_mode == mode { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SelectSocdMode(mode)))
                .content(label)
        };

        let k1_def = Layout::ak980max().key(self.socd_key1);
        let k2_def = Layout::ak980max().key(self.socd_key2);
        let k1_name = k1_def.map(|k| k.label.as_str()).unwrap_or("Inconnu");
        let k2_name = k2_def.map(|k| k.label.as_str()).unwrap_or("Inconnu");

        StackPanel::new().spacing(14.0).children((
            TextBlock::new()
                .text("Le SOCD (Snap Tap / Last Win) résout les conflits entre touches opposées instantanément. La dernière touche enfoncée prévaut immédiatement sans relâcher la première, offrant un contre-strafe parfait dans CS2, Valorant et Apex.")
                .font_size(12.5),

            // Presets
            StackPanel::new().spacing(6.0).children((
                TextBlock::new().text("Paires directionnelles prédéfinies :").font_size(13.5),
                StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                    make_preset_btn(49, 51, "A + D (Strafe QWERTY)"),
                    make_preset_btn(33, 51, "Q + D (Strafe AZERTY)"),
                    make_preset_btn(34, 50, "W + S (Avancer / Reculer)"),
                    make_preset_btn(65, 50, "Z + S (Avancer / Reculer AZERTY)"),
                )),
            )),

            // Current pair details
            Border::new()
                .padding(12.0)
                .corner_radius(6.0)
                .background(Color::argb(25, 255, 255, 255))
                .border_brush(Color::rgb(249, 115, 22))
                .border_thickness(1.0)
                .content(
                    TextBlock::new()
                        .text(format!("Paire configurée : Touche 1 = [{}] (slot #{}) | Touche 2 = [{}] (slot #{})", k1_name, self.socd_key1, k2_name, self.socd_key2))
                        .font_size(13.0)
                        .font_weight(FontWeight::SEMI_BOLD)
                ),

            // Mode selection
            StackPanel::new().spacing(6.0).children((
                TextBlock::new().text("Règle de résolution des conflits :").font_size(13.5),
                StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                    make_mode_btn(3, "Dernière touche (Snap Tap / Last Win)"),
                    make_mode_btn(1, "Première touche prioritaire"),
                    make_mode_btn(2, "Seconde touche prioritaire"),
                    make_mode_btn(4, "Annulation neutre"),
                )),
            )),

            StackPanel::new().orientation(Orientation::Horizontal).spacing(12.0).children((
                Button::new()
                    .style(ButtonStyle::Accent)
                    .on_click(context.message(Message::ApplySocd))
                    .content("Activer la paire SOCD sur le clavier"),
                Button::new()
                    .on_click(context.message(Message::ClearSocd))
                    .content("Désactiver / Rétablir touches normales"),
            )),
        )).into()
    }

    fn render_mod_tap_subtab(&self, context: &ViewContext<Self>) -> View {
        let key_info = if let Some(idx) = self.selected_key {
            let label = Layout::ak980max().key(idx).map(|k| k.label.clone()).unwrap_or_else(|| format!("#{}", idx));
            format!("Touche cible sélectionnée : [{}] (slot #{})", label, idx)
        } else {
            "Sélectionnez une touche sur le visualiseur matriciel ci-dessus".to_string()
        };

        let make_hold_btn = |hid: u8, name: &str| {
            Button::new()
                .style(if self.mt_hold_key == hid { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SetModTapHold(hid)))
                .content(name)
        };

        let make_tap_btn = |hid: u8, name: &str| {
            Button::new()
                .style(if self.mt_tap_key == hid { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SetModTapTap(hid)))
                .content(name)
        };

        StackPanel::new().spacing(14.0).children((
            TextBlock::new()
                .text("Mod-Tap permet à une touche d'assurer deux fonctions distinctes : un appui bref (Tap) envoie une frappe normale, tandis qu'un maintien prolongé (Hold) active un modificateur (comme Ctrl, Maj ou Alt).")
                .font_size(12.5),

            TextBlock::new().text(key_info).font_size(13.0).font_weight(FontWeight::SEMI_BOLD),

            // Hold actions
            StackPanel::new().spacing(6.0).children((
                TextBlock::new().text("Action au maintien prolongé (Hold) :").font_size(13.5),
                StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                    make_hold_btn(0xE0, "Left Ctrl"),
                    make_hold_btn(0xE1, "Left Shift"),
                    make_hold_btn(0xE2, "Left Alt"),
                    make_hold_btn(0xE3, "Left Meta (Win)"),
                )),
            )),

            // Tap actions
            StackPanel::new().spacing(6.0).children((
                TextBlock::new().text("Action à la frappe courte (Tap) :").font_size(13.5),
                StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                    make_tap_btn(0x39, "Caps Lock"),
                    make_tap_btn(0x29, "Escape"),
                    make_tap_btn(0x2C, "Espace"),
                    make_tap_btn(0x28, "Entrée"),
                )),
            )),

            // Delay slider
            StackPanel::new().spacing(6.0).children((
                TextBlock::new().text(format!("Délai de bascule (Hold timer) : {} ms", self.mt_delay_ms as u32)).font_size(13.5),
                Slider::new()
                    .minimum(50.0)
                    .maximum(1000.0)
                    .step_frequency(10.0)
                    .value(self.mt_delay_ms)
                    .on_value_changed(context.callback(Message::SetModTapDelay)),
            )),

            StackPanel::new().orientation(Orientation::Horizontal).spacing(12.0).children((
                Button::new()
                    .style(ButtonStyle::Accent)
                    .on_click(context.message(Message::ApplyModTap))
                    .content("Assigner Mod-Tap sur la touche"),
                Button::new()
                    .on_click(context.message(Message::ClearModTap))
                    .content("Rétablir comportement par défaut"),
            )),
        )).into()
    }

    fn render_toggle_subtab(&self, context: &ViewContext<Self>) -> View {
        let key_info = if let Some(idx) = self.selected_key {
            let label = Layout::ak980max().key(idx).map(|k| k.label.clone()).unwrap_or_else(|| format!("#{}", idx));
            format!("Touche cible sélectionnée : [{}] (slot #{})", label, idx)
        } else {
            "Sélectionnez une touche sur le visualiseur matriciel ci-dessus".to_string()
        };

        StackPanel::new().spacing(14.0).children((
            TextBlock::new()
                .text("Toggle transforme une touche en verrouillage continu : une première pression maintient la touche virtuellement enfoncée, une seconde pression la relâche. Idéal pour sprinter ou viser en permanence.")
                .font_size(12.5),

            TextBlock::new().text(key_info).font_size(13.0).font_weight(FontWeight::SEMI_BOLD),

            StackPanel::new().orientation(Orientation::Horizontal).spacing(12.0).children((
                Button::new()
                    .style(ButtonStyle::Accent)
                    .on_click(context.message(Message::ApplyToggle))
                    .content("Activer le mode Toggle sur la touche"),
                Button::new()
                    .on_click(context.message(Message::ClearToggle))
                    .content("Rétablir comportement normal"),
            )),
        )).into()
    }

    fn render_dks_subtab(&self, context: &ViewContext<Self>) -> View {
        let key_info = if let Some(idx) = self.selected_key {
            let label = Layout::ak980max().key(idx).map(|k| k.label.clone()).unwrap_or_else(|| format!("#{}", idx));
            format!("Touche sélectionnée pour DKS : [{}] (slot #{})", label, idx)
        } else {
            "Sélectionnez une touche sur le visualiseur matriciel pour lui assigner un profil DKS.".to_string()
        };

        let make_act1_btn = |hid: u8, name: &str| {
            Button::new()
                .style(if self.dks_action1_hid == hid { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SetDksAction1(hid)))
                .content(name)
        };

        let make_act2_btn = |hid: u8, name: &str| {
            Button::new()
                .style(if self.dks_action2_hid == hid { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SetDksAction2(hid)))
                .content(name)
        };

        StackPanel::new().spacing(14.0).children((
            TextBlock::new()
                .text("DKS (Dynamic Keystroke) exploite les capteurs à effet Hall pour associer plusieurs actions indépendantes le long de la course d'une même touche : par exemple, marcher doucement à 1.0 mm et courir (Shift) en enfonçant à fond à 3.0 mm.")
                .font_size(12.5),

            Border::new()
                .padding(12.0)
                .corner_radius(6.0)
                .background(Color::argb(25, 255, 255, 255))
                .border_brush(Color::argb(45, 255, 255, 255))
                .border_thickness(1.0)
                .content(
                    TextBlock::new().text(key_info).font_size(13.0).font_weight(FontWeight::SEMI_BOLD)
                ),

            // Sliders for depth points
            StackPanel::new().spacing(10.0).children((
                TextBlock::new().text(format!("Palier 1 (Début de course) : {:.1} mm", self.dks_start_mm)).font_size(13.0),
                Slider::new()
                    .minimum(0.2)
                    .maximum(3.5)
                    .step_frequency(0.1)
                    .value(self.dks_start_mm)
                    .on_value_changed(context.callback(Message::SetDksStartMm)),

                TextBlock::new().text(format!("Palier 2 (Fond de touche) : {:.1} mm", self.dks_bottom_mm)).font_size(13.0),
                Slider::new()
                    .minimum(0.5)
                    .maximum(3.8)
                    .step_frequency(0.1)
                    .value(self.dks_bottom_mm)
                    .on_value_changed(context.callback(Message::SetDksBottomMm)),
            )),

            // Actions for stages
            StackPanel::new().spacing(6.0).children((
                TextBlock::new().text("Action déclenchée au Palier 1 :").font_size(13.0),
                StackPanel::new().orientation(Orientation::Horizontal).spacing(8.0).children((
                    make_act1_btn(0x04, "A"),
                    make_act1_btn(0x1A, "W"),
                    make_act1_btn(0x1D, "Z"),
                    make_act1_btn(0x2C, "Espace"),
                )),
            )),

            StackPanel::new().spacing(6.0).children((
                TextBlock::new().text("Action supplémentaire déclenchée au Palier 2 (Fond) :").font_size(13.0),
                StackPanel::new().orientation(Orientation::Horizontal).spacing(8.0).children((
                    make_act2_btn(0xE1, "Left Shift (Sprint)"),
                    make_act2_btn(0xE0, "Left Ctrl (S'accroupir)"),
                    make_act2_btn(0x2C, "Espace (Sauter)"),
                    make_act2_btn(0x07, "D"),
                )),
            )),

            StackPanel::new().orientation(Orientation::Horizontal).spacing(12.0).children((
                Button::new()
                    .style(ButtonStyle::Accent)
                    .on_click(context.message(Message::ApplyDks))
                    .content("Enregistrer le profil DKS sur cette touche"),
                Button::new()
                    .on_click(context.message(Message::ClearDks))
                    .content("Supprimer le DKS de cette touche"),
            )),
        )).into()
    }

    // ==========================================
    // PAGE 4: MAPPAGE DES TOUCHES (INTERACTIF)
    // ==========================================
    fn render_keys_page(&self, context: &ViewContext<Self>) -> View {
        let (selected_slot, current_action_desc) = match self.selected_key {
            Some(idx) => {
                let label = Layout::ak980max().key(idx).map(|k| k.label.clone()).unwrap_or_else(|| format!("#{}", idx));
                let act = self.keys_map.0.get(usize::from(idx));
                let desc = act.map_or("Par défaut".to_string(), action_display_name);
                (format!("Touche sélectionnée : [{}] (slot #{})", label, idx), format!("Action actuellement assignée : {}", desc))
            }
            None => (
                "Aucune touche sélectionnée — cliquez sur une touche du visualiseur ci-dessus".to_string(),
                "Sélectionnez une touche pour inspecter ou modifier son signal HID.".to_string(),
            ),
        };

        let make_cat_btn = |cat: RemapCategory, label: &str| {
            Button::new()
                .style(if self.remap_category == cat { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SelectRemapCategory(cat)))
                .content(label)
        };

        // Action options list based on active category
        let options_list: Vec<KeyActionOption> = match self.remap_category {
            RemapCategory::Keyboard => standard_keyboard_actions(),
            RemapCategory::Media => media_actions(),
            RemapCategory::Mouse => mouse_actions(),
            RemapCategory::Special => vec![
                KeyActionOption {
                    label: "Désactiver la touche (aucun signal)".to_string(),
                    category: "Spécial",
                    action: KeyAction { raw: [0x02, 0, 0, 0] },
                },
                KeyActionOption {
                    label: "Rétablir l'action par défaut".to_string(),
                    category: "Spécial",
                    action: KeyAction::default(),
                },
            ],
        };

        let mut action_chips = Vec::with_capacity(options_list.len());
        for (i, opt) in options_list.into_iter().enumerate() {
            let act_clone = opt.action;
            let chip = Border::new()
                .margin(Thickness::new(0.0, 0.0, 6.0, 6.0))
                .corner_radius(5.0)
                .background(Color::argb(25, 255, 255, 255))
                .border_brush(Color::argb(45, 255, 255, 255))
                .border_thickness(1.0)
                .content(
                    Button::new()
                        .style(ButtonStyle::Subtle)
                        .on_click(context.message(Message::AssignKeyAction(act_clone)))
                        .content(
                            TextBlock::new()
                                .text(opt.label)
                                .font_size(12.0)
                                .margin(Thickness::new(4.0, 2.0, 4.0, 2.0))
                        )
                );
            action_chips.push(KeyedView::new(format!("act_{}_{}", i, opt.category), chip));
        }

        Border::new()
            .padding(20.0)
            .corner_radius(8.0)
            .background(Color::argb(35, 255, 255, 255))
            .border_brush(Color::argb(45, 255, 255, 255))
            .border_thickness(1.0)
            .content(
                StackPanel::new().spacing(16.0).children((
                    TextBlock::new()
                        .text("Mappage & Réassignation des Touches (HID)")
                        .font_size(18.0)
                        .font_weight(FontWeight::SEMI_BOLD),

                    // Selection info card
                    Border::new()
                        .padding(12.0)
                        .corner_radius(6.0)
                        .background(Color::argb(30, 255, 255, 255))
                        .border_brush(Color::rgb(0, 180, 255))
                        .border_thickness(1.0)
                        .content(
                            StackPanel::new().spacing(4.0).children((
                                TextBlock::new().text(selected_slot).font_size(13.5).font_weight(FontWeight::SEMI_BOLD),
                                TextBlock::new().text(current_action_desc).font_size(12.5),
                            ))
                        ),

                    // Category selection
                    StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                        make_cat_btn(RemapCategory::Keyboard, "Touches Clavier Standard"),
                        make_cat_btn(RemapCategory::Media, "Multimédia & Son"),
                        make_cat_btn(RemapCategory::Mouse, "Actions Souris"),
                        make_cat_btn(RemapCategory::Special, "Désactiver / Spécial"),
                    )),

                    // Actions gallery
                    StackPanel::new().spacing(8.0).children((
                        TextBlock::new().text("Cliquez sur l'action à assigner à la touche sélectionnée :").font_size(13.0),
                        VariableSizedWrapGrid::new()
                            .item_width(175.0)
                            .item_height(38.0)
                            .orientation(Orientation::Horizontal)
                            .keyed_children(action_chips),
                    )),

                    // Bottom controls
                    StackPanel::new().orientation(Orientation::Horizontal).spacing(12.0).children((
                        Button::new()
                            .style(ButtonStyle::Accent)
                            .on_click(context.message(Message::ApplyKeyMap))
                            .content("Enregistrer le mappage dans la mémoire flash (USB)"),
                        Button::new()
                            .on_click(context.message(Message::ResetSelectedKeyAction))
                            .content("Rétablir la touche sélectionnée par défaut"),
                        Button::new()
                            .on_click(context.message(Message::ResetAllKeysDefault))
                            .content("Réinitialiser TOUT le clavier par défaut"),
                    )),
                ))
            )
            .into()
    }

    // ==========================================
    // PAGE 5: TESTEUR DE MATRICE & TOUCHE FN
    // ==========================================
    fn render_test_page(&self, context: &ViewContext<Self>) -> View {
        let make_pref_btn = |pref: LayoutPreference, label: &str| {
            Button::new()
                .style(if self.layout_preference == pref { ButtonStyle::Accent } else { ButtonStyle::Subtle })
                .on_click(context.message(Message::SetLayoutPreference(pref)))
                .content(label)
        };

        let fn_tested = self.pressed_keys.contains(&85);

        Border::new()
            .padding(20.0)
            .corner_radius(8.0)
            .background(Color::argb(35, 255, 255, 255))
            .border_brush(Color::argb(45, 255, 255, 255))
            .border_thickness(1.0)
            .content(
                StackPanel::new().spacing(14.0).children((
                    TextBlock::new()
                        .text("Testeur de matrice et switches magnétiques")
                        .font_size(18.0)
                        .font_weight(FontWeight::SEMI_BOLD),

                    TextBlock::new()
                        .text(format!("Touches testées avec succès : {} / 101", self.pressed_keys.len()))
                        .font_size(14.0)
                        .font_weight(FontWeight::SEMI_BOLD),

                    // Layout switcher specifically on the test page
                    StackPanel::new().spacing(6.0).children((
                        TextBlock::new().text("Disposition de frappe pour la détection :").font_size(13.0),
                        StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                            make_pref_btn(LayoutPreference::Auto, "Détection Windows automatique"),
                            make_pref_btn(LayoutPreference::Azerty, "Forcer AZERTY (Français)"),
                            make_pref_btn(LayoutPreference::Qwerty, "Forcer QWERTY (US)"),
                        )),
                    )),

                    // Special Fn key test card
                    Border::new()
                        .padding(12.0)
                        .corner_radius(6.0)
                        .background(if fn_tested { Color::argb(60, 34, 197, 94) } else { Color::argb(30, 255, 255, 255) })
                        .border_brush(if fn_tested { Color::rgb(74, 222, 128) } else { Color::argb(50, 255, 255, 255) })
                        .border_thickness(1.0)
                        .content(
                            StackPanel::new().orientation(Orientation::Horizontal).spacing(14.0).children((
                                TextBlock::new()
                                    .text(if fn_tested { "Touche Fn (slot #85) : TESTÉE AVEC SUCCÈS" } else { "Touche Fn (slot #85) : En attente de test" })
                                    .font_size(13.0)
                                    .font_weight(FontWeight::SEMI_BOLD)
                                    .vertical_alignment(VerticalAlignment::Center),
                                Button::new()
                                    .style(if fn_tested { ButtonStyle::Subtle } else { ButtonStyle::Accent })
                                    .on_click(context.message(Message::TestFnKey))
                                    .content(if fn_tested { "Annuler le test Fn" } else { "Valider le test de la touche Fn" }),
                            ))
                        ),

                    TextBlock::new()
                        .text("Tapez physiquement sur vos touches. Les touches réagissent instantanément et s'illuminent en vert émeraude. Note : La touche Fn étant gérée en interne par le micrologiciel du clavier, vous pouvez également la valider via le bouton ci-dessus ou en cliquant directement sur la touche Fn de la matrice.")
                        .font_size(12.5),

                    StackPanel::new().orientation(Orientation::Horizontal).spacing(12.0).children((
                        Button::new()
                            .on_click(context.message(Message::ClearTest))
                            .content("Réinitialiser le test"),
                    )),
                ))
            )
            .into()
    }

    // ==========================================
    // PAGE 6: PÉRIPHÉRIQUE & PROFILS / SAUVEGARDES
    // ==========================================
    fn render_device_page(&self, context: &ViewContext<Self>) -> View {
        let mut backup_items = Vec::new();
        for (i, p) in self.backup_files.iter().take(5).enumerate() {
            let filename = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Sauvegarde".to_string());
            let full_path_str = p.to_string_lossy().to_string();
            let full_path_clone = full_path_str.clone();

            let item = Border::new()
                .margin(Thickness::new(0.0, 0.0, 0.0, 6.0))
                .padding(8.0)
                .corner_radius(4.0)
                .background(Color::argb(25, 255, 255, 255))
                .content(
                    StackPanel::new().orientation(Orientation::Horizontal).spacing(12.0).children((
                        TextBlock::new().text(filename).font_size(12.5).vertical_alignment(VerticalAlignment::Center),
                        Button::new()
                            .style(ButtonStyle::Accent)
                            .on_click(context.message(Message::RestoreBackup(full_path_clone)))
                            .content("Restaurer dans le clavier"),
                    ))
                );
            backup_items.push(KeyedView::new(format!("bk_{}", i), item));
        }

        Border::new()
            .padding(20.0)
            .corner_radius(8.0)
            .background(Color::argb(35, 255, 255, 255))
            .border_brush(Color::argb(45, 255, 255, 255))
            .border_thickness(1.0)
            .content(
                StackPanel::new().spacing(16.0).children((
                    TextBlock::new()
                        .text("Informations Périphérique & Profils Matériels")
                        .font_size(18.0)
                        .font_weight(FontWeight::SEMI_BOLD),

                    // Hardware Specs Card
                    Border::new()
                        .padding(14.0)
                        .corner_radius(6.0)
                        .background(Color::argb(25, 255, 255, 255))
                        .content(
                            StackPanel::new().spacing(6.0).children((
                                TextBlock::new().text(format!("Modèle : {}", self.device_name)).font_size(13.5),
                                TextBlock::new().text(format!("PID USB : 0x{} (Vendor ID 0x0C45 Sonix Technology)", self.pid_hex)).font_size(13.5),
                                TextBlock::new().text(format!("Version Micrologiciel (Firmware) : {}", self.firmware_version)).font_size(13.5),
                                TextBlock::new().text(&self.status_detail).font_size(12.0),
                                Button::new()
                                    .on_click(context.message(Message::CheckDevice))
                                    .content("Actualiser la détection HID"),
                            ))
                        ),

                    // Backup & Profiles Card
                    Border::new()
                        .padding(14.0)
                        .corner_radius(6.0)
                        .background(Color::argb(25, 255, 255, 255))
                        .content(
                            StackPanel::new().spacing(12.0).children((
                                TextBlock::new().text("Sauvegardes Complètes de Configuration (JSON)").font_size(15.0).font_weight(FontWeight::SEMI_BOLD),
                                TextBlock::new().text("Exportez l'intégralité de la mémoire du clavier (effets RGB, seuils magnétiques, GameMode, touches réassignées, DKS) dans un profil JSON portable.").font_size(12.5),

                                StackPanel::new().orientation(Orientation::Horizontal).spacing(10.0).children((
                                    Button::new()
                                        .style(ButtonStyle::Accent)
                                        .on_click(context.message(Message::CreateBackup))
                                        .content("Créer une nouvelle sauvegarde (JSON)"),
                                    Button::new()
                                        .on_click(context.message(Message::RefreshBackups))
                                        .content("Actualiser la liste"),
                                    Button::new()
                                        .on_click(context.message(Message::OpenBackupFolder))
                                        .content("Ouvrir le dossier dans l'Explorateur"),
                                )),

                                TextBlock::new().text("Dernières sauvegardes locales enregistrées :").font_size(13.0).font_weight(FontWeight::SEMI_BOLD),
                                StackPanel::new().keyed_children(backup_items),
                            ))
                        ),
                ))
            )
            .into()
    }

    // ==========================================
    // HARDWARE PROTOCOL LOGIC
    // ==========================================
    fn check_device(&mut self) {
        match freejazz_protocol::transport::open_keyboard() {
            Ok((link, ident)) => {
                self.connected = true;
                self.pid_hex = format!("{:04X}", ident.pid);
                match freejazz_protocol::Keyboard::connect(link) {
                    Ok(mut kb) => {
                        let info = &kb.info;
                        self.firmware_version = format!("{}", info.firmware);
                        self.status_detail = format!("Périphérique prêt sur {} (interface {})", ident.path, ident.interface);

                        // Read lighting state
                        if let Ok(effect) = kb.read_led_effect() {
                            let effects_list = all_effects();
                            if let Some(pos) = effects_list.iter().position(|e| e.mode == effect.mode) {
                                self.led_mode_index = pos;
                            }
                            self.led_brightness = (effect.brightness as f64).clamp(1.0, 5.0);
                            self.led_speed = (effect.speed as f64).clamp(1.0, 5.0);
                            self.led_direction = effect.direction;
                            self.led_color_mode = effect.color_mode;
                            self.led_r = effect.rgb.r;
                            self.led_g = effect.rgb.g;
                            self.led_b = effect.rgb.b;
                        }

                        // Read custom leds
                        if let Ok(c_leds) = kb.read_custom_leds() {
                            self.custom_leds = c_leds;
                        }

                        // Read keys map
                        if let Ok(keys) = kb.read_keys() {
                            self.keys_map = keys;
                        }

                        // Read dks
                        if let Ok(dks) = kb.read_dks() {
                            self.dks_table = dks;
                        }

                        // Read magnetic state
                        if let Ok(mag) = kb.read_magnetic() {
                            if let Some(first_active) = mag.0.iter().find(|k| !k.is_empty()) {
                                self.actuation_mm = first_active.trigger_mm as f64;
                                self.rapid_trigger = first_active.whole_fast;
                                self.press_rt_mm = if first_active.press_rt_mm > 0.0 { first_active.press_rt_mm as f64 } else { 0.20 };
                                self.release_rt_mm = if first_active.release_rt_mm > 0.0 { first_active.release_rt_mm as f64 } else { 0.20 };
                                self.switch_type = first_active.axis;
                            }
                        }

                        // Read game mode state
                        if let Ok(gm) = kb.read_game_mode() {
                            self.top_deadzone_mm = gm.top_dead_zone_mm() as f64;
                            self.bottom_deadzone_mm = gm.bottom_dead_zone_mm() as f64;
                            if let Some(hz) = gm.report_rate() {
                                self.polling_rate_hz = hz;
                            }
                        }
                    }
                    Err(e) => {
                        self.status_detail = format!("Périphérique ouvert, mais lecture du profil impossible : {:?}", e);
                    }
                }
            }
            Err(e) => {
                self.connected = false;
                self.firmware_version = "Inconnu".to_string();
                self.pid_hex = "----".to_string();
                self.status_detail = format!("Clavier déconnecté : branchez le câble USB ({:?})", e);
            }
        }
    }

    fn preview_lighting(&mut self) {
        let eff_mode = all_effects()[self.led_mode_index].mode;

        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                let effect = freejazz_protocol::blocks::LedEffect {
                    mode: eff_mode,
                    rgb: Rgb {
                        r: self.led_r,
                        g: self.led_g,
                        b: self.led_b,
                    },
                    secondary: Rgb::default(),
                    color_mode: self.led_color_mode,
                    brightness: (self.led_brightness.round() as u8).clamp(1, 5),
                    speed: (self.led_speed.round() as u8).clamp(1, 5),
                    direction: self.led_direction,
                    effect_mode_type: 0,
                };

                let _ = kb.write_led_effect(&effect);
            }
        }
    }

    fn preview_custom_leds(&mut self) {
        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                let _ = kb.write_custom_leds(&self.custom_leds);
                let effect = freejazz_protocol::blocks::LedEffect {
                    mode: MODE_CUSTOM,
                    rgb: Rgb {
                        r: 255,
                        g: 255,
                        b: 255,
                    },
                    secondary: Rgb::default(),
                    color_mode: 0,
                    brightness: (self.led_brightness.round() as u8).clamp(1, 5),
                    speed: 3,
                    direction: 0,
                    effect_mode_type: 0,
                };
                let _ = kb.write_led_effect(&effect);
            }
        }
    }

    fn apply_lighting(&mut self) {
        let eff_mode = all_effects()[self.led_mode_index].mode;

        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                let effect = freejazz_protocol::blocks::LedEffect {
                    mode: eff_mode,
                    rgb: Rgb {
                        r: self.led_r,
                        g: self.led_g,
                        b: self.led_b,
                    },
                    secondary: Rgb::default(),
                    color_mode: self.led_color_mode,
                    brightness: (self.led_brightness.round() as u8).clamp(1, 5),
                    speed: (self.led_speed.round() as u8).clamp(1, 5),
                    direction: self.led_direction,
                    effect_mode_type: 0,
                };

                if let Err(e) = kb.write_led_effect(&effect) {
                    self.status_detail = format!("Erreur écriture éclairage : {:?}", e);
                } else {
                    self.status_detail = format!("Effet d'éclairage #{} appliqué avec succès au clavier !", eff_mode);
                }
            }
        }
    }

    fn apply_custom_leds(&mut self) {
        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                // 1. Write custom led matrix
                if let Err(e) = kb.write_custom_leds(&self.custom_leds) {
                    self.status_detail = format!("Erreur écriture custom leds : {:?}", e);
                    return;
                }

                // 2. Switch lighting mode to custom (128)
                let effect = freejazz_protocol::blocks::LedEffect {
                    mode: MODE_CUSTOM,
                    rgb: Rgb {
                        r: 255,
                        g: 255,
                        b: 255,
                    },
                    secondary: Rgb::default(),
                    color_mode: 0,
                    brightness: (self.led_brightness.round() as u8).clamp(1, 5),
                    speed: 3,
                    direction: 0,
                    effect_mode_type: 0,
                };
                let _ = kb.write_led_effect(&effect);

                self.status_detail = "Couleurs personnalisées appliquées avec succès sur l'ensemble des touches !".to_string();
            }
        }
    }

    fn apply_performance(&mut self) {
        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                // 1. Write magnetic thresholds
                if let Ok(mut mag) = kb.read_magnetic() {
                    let trigger_f32 = self.actuation_mm as f32;
                    let press_f32 = if self.rapid_trigger { self.press_rt_mm as f32 } else { 0.0 };
                    let release_f32 = if self.rapid_trigger { self.release_rt_mm as f32 } else { 0.0 };

                    if self.apply_globally || self.selected_key.is_none() {
                        for k in mag.0.iter_mut() {
                            if !k.is_empty() {
                                k.trigger_mm = trigger_f32;
                                k.press_rt_mm = press_f32;
                                k.release_rt_mm = release_f32;
                                k.whole_fast = self.rapid_trigger;
                                k.axis = self.switch_type;
                            }
                        }
                    } else if let Some(idx) = self.selected_key {
                        let k = &mut mag.0[usize::from(idx)];
                        k.trigger_mm = trigger_f32;
                        k.press_rt_mm = press_f32;
                        k.release_rt_mm = release_f32;
                        k.whole_fast = self.rapid_trigger;
                        k.axis = self.switch_type;
                    }

                    if let Err(e) = kb.write_magnetic(&mag) {
                        self.status_detail = format!("Erreur écriture switch magnétique : {:?}", e);
                        return;
                    }
                }

                // 2. Write GameMode (deadzones and report rate)
                if let Ok(mut gm) = kb.read_game_mode() {
                    let _ = gm.set_top_dead_zone_mm(self.top_deadzone_mm as f32);
                    let _ = gm.set_bottom_dead_zone_mm(self.bottom_deadzone_mm as f32);
                    let _ = gm.set_report_rate(self.polling_rate_hz);

                    if let Err(e) = kb.write_game_mode(&gm) {
                        self.status_detail = format!("Erreur écriture game_mode : {:?}", e);
                        return;
                    }
                }

                self.status_detail = "Seuils magnétiques et zones mortes enregistrés dans la mémoire flash !".to_string();
            }
        }
    }

    fn apply_key_map(&mut self) {
        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                if let Err(e) = kb.write_keys(&self.keys_map) {
                    self.status_detail = format!("Erreur écriture mappage des touches : {:?}", e);
                } else {
                    self.status_detail = "Mappage des touches enregistré avec succès dans la mémoire flash !".to_string();
                }
            }
        }
    }

    fn apply_socd(&mut self) {
        let k1_def = Layout::ak980max().key(self.socd_key1);
        let k2_def = Layout::ak980max().key(self.socd_key2);

        let (Some(a), Some(b)) = (k1_def, k2_def) else {
            self.status_detail = "Touches de la paire SOCD introuvables".to_string();
            return;
        };

        let action = KeyAction::from_kind(KeyKind::Socd([self.socd_mode, a.hid, b.hid]));
        self.keys_map.0[usize::from(a.index)] = action;
        self.keys_map.0[usize::from(b.index)] = action;

        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                if let Err(e) = kb.write_keys(&self.keys_map) {
                    self.status_detail = format!("Erreur écriture SOCD : {:?}", e);
                    return;
                }
                self.status_detail = format!("Paire SOCD ([{}] + [{}], mode {}) activée avec succès !", a.label, b.label, self.socd_mode);
            }
        }
    }

    fn clear_socd(&mut self) {
        self.keys_map.0[usize::from(self.socd_key1)] = KeyAction::default();
        self.keys_map.0[usize::from(self.socd_key2)] = KeyAction::default();

        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                if let Err(e) = kb.write_keys(&self.keys_map) {
                    self.status_detail = format!("Erreur réinitialisation SOCD : {:?}", e);
                    return;
                }
                self.status_detail = "Paire SOCD désactivée : touches rétablies en mode standard.".to_string();
            }
        }
    }

    fn apply_mod_tap(&mut self) {
        let Some(idx) = self.selected_key else {
            self.status_detail = "Sélectionnez d'abord une touche sur le visualiseur matriciel".to_string();
            return;
        };

        let units = ((self.mt_delay_ms.clamp(50.0, 1000.0) + 5.0) / 10.0) as u8;
        let action = KeyAction::from_kind(KeyKind::Mt([self.mt_hold_key, self.mt_tap_key, units]));
        self.keys_map.0[usize::from(idx)] = action;

        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                if let Err(e) = kb.write_keys(&self.keys_map) {
                    self.status_detail = format!("Erreur écriture Mod-Tap : {:?}", e);
                    return;
                }
                self.status_detail = format!("Mod-Tap appliqué sur la touche #{} (Délai: {} ms)", idx, self.mt_delay_ms as u32);
            }
        }
    }

    fn apply_toggle(&mut self) {
        let Some(idx) = self.selected_key else {
            self.status_detail = "Sélectionnez d'abord une touche sur le visualiseur matriciel".to_string();
            return;
        };

        let target_hid = Layout::ak980max().key(idx).map(|k| k.hid).unwrap_or(0x04);
        let action = KeyAction::from_kind(KeyKind::Tgl(target_hid));
        self.keys_map.0[usize::from(idx)] = action;

        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                if let Err(e) = kb.write_keys(&self.keys_map) {
                    self.status_detail = format!("Erreur écriture Toggle : {:?}", e);
                    return;
                }
                self.status_detail = format!("Toggle activé sur la touche #{} (HID 0x{:02X})", idx, target_hid);
            }
        }
    }

    fn apply_dks(&mut self) {
        let Some(idx) = self.selected_key else {
            self.status_detail = "Sélectionnez d'abord une touche sur le visualiseur matriciel".to_string();
            return;
        };

        // Find existing or free DKS slot (0..=63)
        let slot = match self.keys_map.0[usize::from(idx)].kind() {
            KeyKind::Dks(s) => s,
            _ => {
                let mut found = None;
                for s in 0..=MAX_DKS_SLOT {
                    let used_in_table = self.dks_table.0.iter().any(|e| e.index == s && !e.is_empty());
                    let used_by_key = self.keys_map.0.iter().any(|a| matches!(a.kind(), KeyKind::Dks(n) if n == s));
                    if !used_in_table && !used_by_key {
                        found = Some(s);
                        break;
                    }
                }
                match found {
                    Some(s) => s,
                    None => {
                        self.status_detail = "Plus d'emplacement DKS libre (maximum 64 profils).".to_string();
                        return;
                    }
                }
            }
        };

        let start_u8 = (self.dks_start_mm * 10.0).round().clamp(2.0, 38.0) as u8;
        let bottom_u8 = (self.dks_bottom_mm * 10.0).round().clamp(start_u8 as f64 + 1.0, 40.0) as u8;

        let mut entry = DksEntry {
            index: slot,
            make: [start_u8, bottom_u8],
            brk: [bottom_u8, start_u8],
            actions: [self.dks_action1_hid, self.dks_action2_hid, 0, 0],
            ..Default::default()
        };
        entry.states[0] = [3, 2, 0, 0]; // Stage 1: trigger and hold
        entry.states[1] = [0, 3, 2, 0]; // Stage 2: trigger at bottom

        // Update or insert into dks_table
        if let Some(pos) = self.dks_table.0.iter().position(|e| e.index == slot) {
            self.dks_table.0[pos] = entry;
        } else {
            self.dks_table.0.push(entry);
        }

        self.keys_map.0[usize::from(idx)] = KeyAction::from_kind(KeyKind::Dks(slot));

        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                let _ = kb.write_dks(&self.dks_table);
                let _ = kb.write_keys(&self.keys_map);
                self.status_detail = format!("Profil DKS (slot #{}) enregistré avec succès sur la touche #{} !", slot, idx);
            }
        }
    }

    fn clear_dks(&mut self) {
        let Some(idx) = self.selected_key else { return; };
        if let KeyKind::Dks(slot) = self.keys_map.0[usize::from(idx)].kind() {
            self.dks_table.0.retain(|e| e.index != slot);
        }
        self.keys_map.0[usize::from(idx)] = KeyAction::default();

        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                let _ = kb.write_dks(&self.dks_table);
                let _ = kb.write_keys(&self.keys_map);
                self.status_detail = format!("DKS supprimé : touche #{} remise par défaut.", idx);
            }
        }
    }

    fn clear_selected_key_action(&mut self) {
        let Some(idx) = self.selected_key else { return; };
        self.keys_map.0[usize::from(idx)] = KeyAction::default();

        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                if let Err(e) = kb.write_keys(&self.keys_map) {
                    self.status_detail = format!("Erreur rétablissement touche : {:?}", e);
                    return;
                }
                self.status_detail = format!("Touche #{} rétablie à son comportement d'origine.", idx);
            }
        }
    }

    // ==========================================
    // BACKUPS & PROFILES
    // ==========================================
    fn refresh_backups(&mut self) {
        let mut list = Vec::new();
        if let Ok(dir) = freejazz_protocol::backup::default_backup_dir() {
            let _ = std::fs::create_dir_all(&dir);
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    let path = e.path();
                    if path.extension().map_or(false, |ext| ext == "json") {
                        list.push(path);
                    }
                }
            }
        }
        list.sort_by(|a, b| b.cmp(a));
        self.backup_files = list;
    }

    fn create_backup(&mut self) {
        if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
            if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                match kb.read_snapshot() {
                    Ok(snapshot) => {
                        if let Ok(dir) = freejazz_protocol::backup::default_backup_dir() {
                            match freejazz_protocol::backup::save_backup_tagged(&snapshot, &dir, Some("winui")) {
                                Ok(path) => {
                                    self.status_detail = format!("Sauvegarde créée : {}", path.file_name().unwrap_or_default().to_string_lossy());
                                    self.refresh_backups();
                                }
                                Err(e) => {
                                    self.status_detail = format!("Erreur sauvegarde : {:?}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        self.status_detail = format!("Erreur lecture snapshot : {:?}", e);
                    }
                }
            }
        }
    }

    fn restore_backup(&mut self, path_str: &str) {
        let p = std::path::Path::new(path_str);
        match freejazz_protocol::backup::load_backup(p) {
            Ok(snapshot) => {
                if let Ok((link, _)) = freejazz_protocol::transport::open_keyboard() {
                    if let Ok(mut kb) = freejazz_protocol::Keyboard::connect(link) {
                        if let Err(e) = kb.write_snapshot(&snapshot) {
                            self.status_detail = format!("Erreur restauration dans le clavier : {:?}", e);
                        } else {
                            self.status_detail = format!("Profil restauré avec succès depuis {} !", p.file_name().unwrap_or_default().to_string_lossy());
                            self.check_device();
                        }
                    }
                }
            }
            Err(e) => {
                self.status_detail = format!("Erreur chargement fichier JSON : {:?}", e);
            }
        }
    }

    fn open_backup_folder(&self) {
        if let Ok(dir) = freejazz_protocol::backup::default_backup_dir() {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::process::Command::new("explorer.exe").arg(&dir).spawn();
        }
    }
}

fn main() {
    App::run_component::<AppModel>(()).unwrap();
}
