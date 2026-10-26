//! « Test et calibrage »: a key test (keys light up on the keyboard view as they are
//! pressed; with the experimental depth stream, also how far they travel) and the
//! calibration wizard. Neither edits the snapshot, so « Appliquer » is not involved.
//!
//! UNVERIFIED-HW: the depth stream (0x66 / 0xFB / 0x67) and the calibration (0x64 /
//! 0x65) were never sent to a keyboard; both stay off until the user asks.

pub mod calibration;
pub mod key_test;

use eframe::egui::{self, RichText, Ui};
use egui_phosphor::regular as i;

use super::{error_line, heading, hint, switch};
use crate::app::AppState;
use crate::device::{DeviceHandle, Event, Probe, Request};
use crate::keyboard::{KeyLook, Layout};
use crate::ui::theme::{self, Palette};
use crate::ui::{ButtonKind, button};
use calibration::{Calibration, Phase};
use key_test::{KeyTest, mm_text};

const NEEDS_EXPERIMENTAL: &str =
    "Demande les fonctions expérimentales (Réglages) : jamais essayé sur ce clavier.";

/// What the panel remembers between frames.
#[derive(Default)]
pub struct TestUi {
    pub keys: KeyTest,
    pub calibration: Calibration,
    /// The 0xFB stream the keyboard confirmed.
    probe: Option<Probe>,
    /// Sent to the keyboard by the app.
    requests: Vec<Request>,
}

impl TestUi {
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.requests)
    }

    /// The worker's news about the 0xFB stream.
    pub fn on_event(&mut self, event: &Event) {
        match event {
            Event::Probe(kind) => {
                self.probe = *kind;
                self.calibration.running(*kind == Some(Probe::Calibration));
                if *kind != Some(Probe::KeyTest) {
                    self.keys.stroke.clear();
                }
            }
            Event::Axis(report) => match self.probe {
                Some(Probe::KeyTest) => self.keys.on_report(report),
                Some(Probe::Calibration) => self.calibration.on_report(report),
                None => {}
            },
            Event::Disconnected => {
                self.probe = None;
                self.calibration.abort();
                self.keys.stroke.clear();
            }
            _ => {}
        }
    }

    /// Leaving the section ends any stream.
    pub fn leave(&mut self, device: &DeviceHandle) {
        self.keys.stop_hook();
        self.calibration.abort();
        if self.probe.is_some() {
            device.send(Request::ProbeStop);
        }
    }

    /// Colours and badge of one key of the keyboard view.
    pub fn decorate(&self, index: u8, look: &mut KeyLook, p: &Palette) {
        if self.keys.stroke.contains_key(&index) {
            // the badge (travel) is drawn in the accent colour: a soft fill keeps it readable
            look.fill = Some(p.accent_soft);
        } else if self.keys.pressed.contains(&index) {
            look.fill = Some(p.accent);
        } else if self.keys.tested.contains(&index) || self.calibration.calibrated.contains(&index)
        {
            look.fill = Some(p.success.gamma_multiply(if p.dark { 0.45 } else { 0.6 }));
        }
        if let Some(&s) = self.keys.stroke.get(&index) {
            look.badge = Some(format!("{:.1}", f32::from(s) / 100.0).replace('.', ","));
        }
    }

    /// Screenshot aid (`--demo-test test|calibration`).
    pub fn demo(&mut self, what: &str) {
        let layout = Layout::ak980max();
        let order = layout.row_order();
        if what == "calibration" {
            self.probe = Some(Probe::Calibration);
            self.calibration.running(true);
            self.calibration.calibrated = order.iter().copied().take(54).collect();
        } else {
            self.probe = Some(Probe::KeyTest);
            self.keys.tested = order.iter().copied().skip(30).take(22).collect();
            for (n, &index) in order.iter().skip(34).take(3).enumerate() {
                self.keys.stroke.insert(index, [120, 250, 360][n]);
                self.keys.last = Some(index);
            }
        }
    }
}

pub fn show(ui: &mut Ui, p: &Palette, state: &mut AppState, st: &mut TestUi) {
    let layout = Layout::ak980max();
    // Space / Enter must test keys, not press whichever button was clicked last.
    ui.memory_mut(|m| {
        if let Some(id) = m.focused() {
            m.surrender_focus(id);
        }
    });
    st.keys.start_hook();
    ui.input(|input| st.keys.read_input(input, layout));
    let calibrating = st.probe == Some(Probe::Calibration);

    ui.columns(2, |cols| {
        key_test_column(&mut cols[0], p, state, st, layout, calibrating);
        calibration_column(&mut cols[1], p, state, st, layout);
    });
}

fn key_test_column(
    ui: &mut Ui,
    p: &Palette,
    state: &AppState,
    st: &mut TestUi,
    layout: &Layout,
    calibrating: bool,
) {
    let busy = state.busy;
    heading(ui, p, "Test des touches");
    hint(
        ui,
        p,
        "Appuyez sur vos touches : elles s'allument sur le dessin.",
    );
    hint(
        ui,
        p,
        "Maj, Ctrl, Alt : côté gauche. Verr. Maj et pavé num. non vus.",
    );
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "{} / {} touches testées",
                st.keys.tested.len(),
                layout.keys.len()
            ))
            .font(theme::strong(13.5))
            .color(p.text),
        );
        if button(ui, p, ButtonKind::Secondary, None, "Effacer", true).clicked() {
            st.keys.clear();
        }
    });

    ui.add_space(6.0);
    heading(ui, p, "Profondeur d'appui (expérimental)");
    hint(
        ui,
        p,
        "Demande au clavier de signaler la course de chaque touche (commande de test que le \
         site officiel n'envoie jamais à ce modèle). Désactivé par défaut ; à essayer avec prudence.",
    );
    if !state.experimental {
        hint(ui, p, NEEDS_EXPERIMENTAL);
    }
    let mut on = st.probe == Some(Probe::KeyTest);
    if super::row(ui, p, "Essayer la profondeur (expérimental)", |ui| {
        let usable = state.experimental || on;
        switch(
            ui,
            p,
            &mut on,
            usable && state.connected && !busy && !calibrating,
        )
        .changed()
    })
    .inner
    {
        st.requests.push(if on {
            Request::ProbeStart(Probe::KeyTest)
        } else {
            Request::ProbeStop
        });
    }
    if on {
        match st
            .keys
            .last
            .and_then(|k| st.keys.stroke.get(&k).map(|s| (k, *s)))
        {
            Some((k, s)) => {
                let name = layout.key(k).map_or("?", |d| d.label.as_str());
                hint(ui, p, &format!("Touche {name} : {}", mm_text(s)));
            }
            None => hint(ui, p, "Enfoncez une touche pour voir sa course."),
        }
    }
}

fn calibration_column(
    ui: &mut Ui,
    p: &Palette,
    state: &AppState,
    st: &mut TestUi,
    layout: &Layout,
) {
    let busy = state.busy;
    heading(ui, p, "Assistant de calibrage (expérimental)");
    match st.calibration.phase {
        Phase::Idle | Phase::Done => {
            if st.calibration.phase == Phase::Done {
                hint(
                    ui,
                    p,
                    &format!(
                        "Calibrage terminé : {} touches calibrées sur {}.",
                        st.calibration.calibrated.len(),
                        layout.keys.len()
                    ),
                );
            }
            hint(
                ui,
                p,
                "Recalibre la course de toutes les touches. Une sauvegarde du clavier est faite \
                 d'abord. Commande que le site officiel n'utilise pas pour ce modèle.",
            );
            if button(
                ui,
                p,
                ButtonKind::Secondary,
                Some(i::SLIDERS_HORIZONTAL),
                "Démarrer le calibrage",
                state.experimental && state.connected && !busy && st.probe.is_none(),
            )
            .clicked()
            {
                st.requests.push(Request::ProbeStart(Probe::Calibration));
            }
            if !state.experimental {
                hint(ui, p, NEEDS_EXPERIMENTAL);
            }
        }
        Phase::Running => {
            hint(
                ui,
                p,
                "Appuyez à fond sur chaque touche, l'une après l'autre. Les touches calibrées \
                 passent au vert.",
            );
            let done = st.calibration.calibrated.len();
            ui.add(
                egui::ProgressBar::new(st.calibration.progress(layout.keys.len()))
                    .desired_width(320.0)
                    .text(format!("{done} / {}", layout.keys.len())),
            );
            if button(ui, p, ButtonKind::Primary, Some(i::CHECK), "Terminer", true).clicked() {
                st.calibration.finish();
                st.requests.push(Request::ProbeStop);
            }
        }
    }
    if st.probe.is_some() && !state.connected {
        error_line(ui, p, "Clavier déconnecté.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_events_drive_the_wizard_and_the_depth_readout() {
        let mut t = TestUi::default();
        t.on_event(&Event::Probe(Some(Probe::Calibration)));
        assert_eq!(t.calibration.phase, Phase::Running);
        t.calibration.finish();
        t.on_event(&Event::Probe(None));
        assert_eq!(t.calibration.phase, Phase::Done);
        t.on_event(&Event::Probe(Some(Probe::KeyTest)));
        assert_eq!(t.probe, Some(Probe::KeyTest));
        t.on_event(&Event::Disconnected);
        assert_eq!(t.probe, None);
    }

    #[test]
    fn an_aborted_calibration_is_not_done() {
        // the keyboard stream ends (error, stop by the worker): not « terminé »
        let mut t = TestUi::default();
        t.on_event(&Event::Probe(Some(Probe::Calibration)));
        t.on_event(&Event::Probe(None));
        assert_eq!(t.calibration.phase, Phase::Idle);
        // disconnect
        t.on_event(&Event::Probe(Some(Probe::Calibration)));
        t.on_event(&Event::Disconnected);
        assert_eq!(t.calibration.phase, Phase::Idle);
        // the user finishes it
        t.on_event(&Event::Probe(Some(Probe::Calibration)));
        t.calibration.finish();
        t.on_event(&Event::Probe(None));
        assert_eq!(t.calibration.phase, Phase::Done);
        // a new run forgets the finish
        t.on_event(&Event::Probe(Some(Probe::Calibration)));
        t.on_event(&Event::Probe(None));
        assert_eq!(t.calibration.phase, Phase::Idle);
    }

    #[test]
    fn pressed_keys_light_up() {
        let p = Palette::DARK;
        let mut t = TestUi::default();
        t.keys.set_pressed(3, true);
        let mut look = KeyLook::default();
        t.decorate(3, &mut look, &p);
        assert_eq!(look.fill, Some(p.accent));
        t.keys.set_pressed(3, false);
        t.decorate(3, &mut look, &p);
        assert_ne!(look.fill, Some(p.accent));
        let mut other = KeyLook::default();
        t.decorate(4, &mut other, &p);
        assert_eq!(other.fill, None);
    }
}
