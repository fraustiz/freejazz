//! The device manager against a simulated AK980 Max (`mock::Sim`, a `HidLink` that
//! answers like the keyboard: reads serve the blocks it holds, writes change them).

use std::path::Path;
use std::time::Duration;

use freejazz::device::{Event, Manager, Request};
use freejazz::mock::{Sim, fixture};
use freejazz_protocol::Snapshot;
use freejazz_protocol::backup::{load_backup, save_backup};
use freejazz_protocol::reconnect::Wait;

const FAST: Wait = Wait {
    timeout: Duration::from_millis(200),
    poll: Duration::from_millis(5),
};

/// Reports written with a SET command (0x21–0x28), as `(cmd, report)`.
fn sets(sim: &Sim) -> Vec<(u8, Vec<u8>)> {
    sim.written()
        .into_iter()
        .filter(|r| r[0] == 0xAA && (0x21..=0x28).contains(&r[1]))
        .map(|r| (r[1], r))
        .collect()
}

fn files(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |d| d.count())
}

fn connected(m: &mut Manager<freejazz::mock::SimPort>) -> Snapshot {
    let events = m.tick();
    let snapshot = events.into_iter().find_map(|e| match e {
        Event::Connected { snapshot, .. } => Some(*snapshot),
        _ => None,
    });
    snapshot.expect("Connected")
}

fn manager(sim: &Sim, dir: &Path) -> Manager<freejazz::mock::SimPort> {
    Manager::new(sim.port(), Some(dir.to_path_buf())).with_wait(FAST)
}

/// An edit as the UI makes it: a clone of the snapshot, read bytes dropped.
fn edit(s: &Snapshot) -> Snapshot {
    let mut e = s.clone();
    e.raw = None;
    e
}

#[test]
fn apply_writes_and_publishes_snapshot() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let base = connected(&mut m);
    assert_eq!(base.led_effect.brightness, 6);
    sim.clear_written();

    let mut edited = edit(&base);
    edited.led_effect.brightness = 3;
    let after = m.apply(&edited).expect("apply");

    // only SET_LED_EFFECT was sent, with brightness at byte 9 of the data
    let sets = sets(&sim);
    assert!(!sets.is_empty());
    assert!(sets.iter().all(|(cmd, _)| *cmd == 0x23), "{sets:?}");
    assert_eq!(sets[0].1[8 + 9], 3);
    // the new snapshot is the keyboard's, read back with its bytes
    assert_eq!(after.led_effect.brightness, 3);
    let raw = after.raw.as_ref().expect("re-read from the keyboard");
    assert_eq!(raw.led_effect[9], 3);
    assert_eq!(after.check_raw(raw), Ok(()));
    assert_eq!(m.snapshot(), Some(&after));
    // the keyboard reports the colour as white after a write: the baseline keeps the
    // colour written (protocol.md 9.7), which the read-back rule does not compare
    assert_eq!(raw.led_effect[1..4], [0xFF, 0xFF, 0xFF]);
    assert_eq!(after.led_effect.rgb, base.led_effect.rgb);

    // through the request channel: Applied with the same snapshot
    edited.led_effect.brightness = 4;
    let events = m.handle(Request::Apply(Box::new(edited)));
    match events.as_slice() {
        [Event::Applied(s)] => assert_eq!(s.led_effect.brightness, 4),
        other => panic!("expected Applied, got {other:?}"),
    }
}

#[test]
fn apply_on_disconnect_reports_error() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let base = connected(&mut m);

    // custom colours: 512 bytes, several reports; the keyboard goes away after 3
    let mut edited = edit(&base);
    edited.custom_leds.0[5].r = 0x12;
    sim.fail_after_writes(3);
    let err = m.apply(&edited).unwrap_err();
    assert!(err.starts_with("Écriture interrompue : "), "{err}");
    assert!(!m.connected());
    assert_eq!(m.snapshot(), None);

    // the UI hears of it on the next step; the keyboard is still unplugged
    assert!(matches!(m.tick().as_slice(), [Event::Disconnected]));
    assert!(m.tick().is_empty());
    sim.plug();
    let again = connected(&mut m);
    // the next connection works, and shows what the keyboard holds: the first chunks
    // of the colours (entry 5 is in the first one)
    assert_eq!(again.custom_leds.0[5].r, 0x12);
    assert_eq!(again.keys, base.keys);

    // through the channel: the UI is told, and offered the restore
    let mut edited = edit(&again);
    edited.custom_leds.0[5].r = 0x34;
    sim.fail_after_writes(3);
    let events = m.handle(Request::Apply(Box::new(edited)));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::WriteInterrupted(msg) if msg.starts_with("Écriture interrompue : "))),
        "{events:?}"
    );
    assert!(events.iter().any(|e| matches!(e, Event::Disconnected)));
}

#[test]
fn interrupted_write_on_a_keyboard_that_still_answers_publishes_what_it_holds() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let base = connected(&mut m);
    let mut edited = edit(&base);
    edited.custom_leds.0[5].r = 0x12;
    sim.refuse_writes_after(3);
    let events = m.handle(Request::Apply(Box::new(edited)));
    assert!(m.connected());
    assert!(
        events.iter().any(|e| matches!(e, Event::Refreshed(_))),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::WriteInterrupted(_)))
    );
}

#[test]
fn auto_backup_once_per_device() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    connected(&mut m);
    assert_eq!(files(dir.path()), 1, "first connection: one backup");
    let name = std::fs::read_dir(dir.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name();
    let name = name.to_string_lossy();
    assert!(name.starts_with("ak980max-8043-"), "{name}");

    // unplug, plug back: no second backup
    sim.unplug();
    assert!(matches!(m.tick().as_slice(), [Event::Disconnected]));
    sim.plug();
    connected(&mut m);
    // the app restarts: the file on disk counts
    drop(m);
    let mut m = manager(&sim, dir.path());
    connected(&mut m);
    assert_eq!(files(dir.path()), 1);

    // a backup taken by the CLI counts as well (same folder, same naming)
    let other = tempfile::tempdir().unwrap();
    let mut cli = fixture();
    cli.raw = None;
    save_backup(&cli, other.path()).unwrap();
    let mut m = manager(&sim, other.path());
    connected(&mut m);
    assert_eq!(files(other.path()), 1);

    // another unit (PID 0x8050) gets its own
    let mut s = fixture();
    s.info.pid = 0x8050;
    s.info.raw[6..8].copy_from_slice(&0x8050u16.to_le_bytes());
    let sim2 = Sim::new(&s);
    let mut m = manager(&sim2, dir.path());
    connected(&mut m);
    assert_eq!(files(dir.path()), 2);
    let saved: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| load_backup(&e.unwrap().path()).unwrap().info.pid)
        .collect();
    assert!(saved.contains(&0x8043) && saved.contains(&0x8050));
}

#[test]
fn apply_validates_only_changes() {
    // the keyboard holds a trigger point outside the UI limits (3,40 mm on key 1)
    let mut held = fixture();
    held.magnetic.0[1].trigger_mm = 3.4;
    let sim = Sim::new(&held);
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let base = connected(&mut m);
    assert_eq!(base.magnetic.0[1].trigger_mm, 3.4);
    sim.clear_written();

    // an unrelated change is accepted: the held value is not judged
    let mut edited = edit(&base);
    edited.led_effect.speed = 2;
    m.apply(&edited)
        .expect("unchanged values are not validated");

    // a changed magnetic key is: refused in French before anything is sent
    sim.clear_written();
    let mut bad = edit(m.snapshot().unwrap());
    bad.magnetic.0[2].trigger_mm = 3.5;
    let err = m.apply(&bad).unwrap_err();
    assert!(err.contains("hors limites"), "{err}");
    assert!(sets(&sim).is_empty(), "nothing written");
    assert!(m.connected());

    // and a change within the limits next to the held value goes through
    let mut ok = edit(m.snapshot().unwrap());
    ok.magnetic.0[2].trigger_mm = 2.0;
    let after = m.apply(&ok).unwrap();
    assert_eq!(after.magnetic.0[2].trigger_mm, 2.0);
    assert_eq!(after.magnetic.0[1].trigger_mm, 3.4);
    assert!(sets(&sim).iter().all(|(cmd, _)| *cmd == 0x27));
}

#[test]
fn report_rate_change_reconnects_then_reads_back() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let base = connected(&mut m);
    let mut edited = edit(&base);
    edited.game_mode.set_report_rate(1000).unwrap();
    let after = m.apply(&edited).expect("reconnected and read back");
    assert_eq!(after.game_mode.report_rate(), Some(1000));
    assert!(sim.enumerations() >= 2, "the keyboard re-enumerated");
    assert!(m.connected());
}

#[test]
fn restore_latest_backup_writes_it_back() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let base = connected(&mut m);
    let mut edited = edit(&base);
    edited.keys.0[3] = base.keys.0[4];
    edited.led_effect.brightness = 2;
    m.apply(&edited).unwrap();

    let events = m.handle(Request::Restore(None));
    let restored = events
        .iter()
        .find_map(|e| match e {
            Event::Connected { snapshot, .. } => Some(snapshot.as_ref().clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{events:?}"));
    assert_eq!(restored.keys, base.keys);
    assert_eq!(restored.led_effect.brightness, base.led_effect.brightness);
    assert_eq!(
        files(dir.path()),
        2,
        "the automatic backup and the pre-restore one"
    );
}

#[test]
fn requests_while_disconnected_fail_in_french() {
    let sim = Sim::new(&fixture());
    sim.unplug();
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    assert!(m.tick().is_empty());
    let mut s = fixture();
    s.led_effect.brightness = 1;
    assert_eq!(m.apply(&s).unwrap_err(), "Aucun clavier connecté");
    let events = m.handle(Request::SetProfile(1));
    assert!(matches!(events.as_slice(), [Event::Failed(_)]));
}

#[test]
fn writes_are_refused_without_a_backup() {
    // no backup folder at all
    let sim = Sim::new(&fixture());
    let mut m = Manager::new(sim.port(), None).with_wait(FAST);
    let base = connected(&mut m);
    sim.clear_written();
    let mut edited = edit(&base);
    edited.led_effect.brightness = 2;
    let err = m.apply(&edited).unwrap_err();
    assert!(
        err.starts_with("Aucune sauvegarde de ce clavier : écriture refusée"),
        "{err}"
    );
    for request in [
        Request::Apply(Box::new(edited.clone())),
        Request::Restore(None),
        Request::LiveBegin,
        Request::SetProfile(1),
    ] {
        let live = matches!(request, Request::LiveBegin);
        let events = m.handle(request);
        assert!(
            matches!(events.first(), Some(Event::Failed(msg)) if msg.starts_with("Aucune sauvegarde de ce clavier")),
            "{events:?}"
        );
        // a refused stream says so, for the engine to stop
        let expected = if live { 2 } else { 1 };
        assert_eq!(events.len(), expected, "{events:?}");
        if live {
            assert!(matches!(events[1], Event::LiveStarted(false)));
        }
    }
    assert!(sets(&sim).is_empty(), "nothing written");

    // a folder that cannot be written (a file stands in its way): the failure is shown
    // after `Connected`, so the UI keeps it
    let root = tempfile::tempdir().unwrap();
    let blocked = root.path().join("backups");
    std::fs::write(&blocked, "not a folder").unwrap();
    let sim = Sim::new(&fixture());
    let mut m = Manager::new(sim.port(), Some(blocked.clone())).with_wait(FAST);
    let events = m.tick();
    let connected_at = events
        .iter()
        .position(|e| matches!(e, Event::Connected { .. }))
        .expect("Connected");
    let failed_at = events
        .iter()
        .position(|e| matches!(e, Event::Failed(msg) if msg.starts_with("Sauvegarde automatique impossible")))
        .unwrap_or_else(|| panic!("{events:?}"));
    assert!(failed_at > connected_at, "{events:?}");
    let base = m.snapshot().unwrap().clone();
    let mut edited = edit(&base);
    edited.led_effect.brightness = 2;
    sim.clear_written();
    assert!(m.apply(&edited).is_err());
    assert!(sets(&sim).is_empty());
    // once the folder can be written, the backup is retried before the write
    std::fs::remove_file(&blocked).unwrap();
    m.apply(&edited).expect("backed up, then written");
    assert_eq!(files(&blocked), 1);
}

#[test]
fn restore_keeps_a_pre_restore_backup_that_latest_skips() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let base = connected(&mut m);
    let mut edited = edit(&base);
    edited.led_effect.brightness = 2;
    m.apply(&edited).unwrap();

    m.restore(None).expect("restored");
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    let pre = names
        .iter()
        .find(|n| n.contains("-avant-restauration"))
        .unwrap_or_else(|| panic!("{names:?}"));
    // the pre-restore file holds what the keyboard had before the restore
    let held = load_backup(&dir.path().join(pre)).unwrap();
    assert_eq!(held.led_effect.brightness, 2);

    // a second « Restaurer la dernière sauvegarde » still takes the automatic backup
    let mut again = edit(m.snapshot().unwrap());
    again.led_effect.brightness = 1;
    m.apply(&again).unwrap();
    let restored = m.restore(None).unwrap();
    assert_eq!(restored.led_effect.brightness, base.led_effect.brightness);
}

#[test]
fn an_interrupted_write_names_the_backup_to_restore() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let base = connected(&mut m);
    let auto = std::fs::read_dir(dir.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut edited = edit(&base);
    edited.custom_leds.0[5].r = 0x12;
    sim.fail_after_writes(3);
    let events = m.handle(Request::Apply(Box::new(edited)));
    let target = events
        .iter()
        .find_map(|e| match e {
            Event::RestoreTarget(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{events:?}"));
    let (path, when) = target.expect("a backup exists");
    assert_eq!(path, auto);
    assert_eq!(when.len(), "25/10/2026 10:03".len(), "{when}");
}

#[test]
fn factory_reset_saves_a_backup_first_and_is_refused_without_one() {
    use freejazz_protocol::keyboard::ResetKind;
    // no backup folder: nothing is sent
    let sim = Sim::new(&fixture());
    let mut m = Manager::new(sim.port(), None).with_wait(FAST);
    connected(&mut m);
    sim.clear_written();
    let events = m.handle(Request::FactoryReset(ResetKind::Macros));
    assert!(
        matches!(events.as_slice(), [Event::Failed(msg)] if msg.starts_with("Aucune sauvegarde")),
        "{events:?}"
    );
    assert!(sim.written().iter().all(|r| r[1] != 0x0F));

    // with a folder: a pre-reset file, then one 0x0F report with the type
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    connected(&mut m);
    let before = files(dir.path());
    sim.clear_written();
    m.handle(Request::FactoryReset(ResetKind::Macros));
    assert_eq!(files(dir.path()), before + 1);
    let resets: Vec<_> = sim.written().into_iter().filter(|r| r[1] == 0x0F).collect();
    assert_eq!(resets.len(), 1);
    assert_eq!(resets[0][2], 4);
}

#[test]
fn probes_start_stop_and_calibration_needs_a_backup() {
    use freejazz::device::Probe;
    // calibration without a backup folder: nothing is sent
    let sim = Sim::new(&fixture());
    let mut m = Manager::new(sim.port(), None).with_wait(FAST);
    connected(&mut m);
    sim.clear_written();
    let events = m.handle(Request::ProbeStart(Probe::Calibration));
    assert!(
        matches!(events.as_slice(), [Event::Failed(msg), Event::Probe(None)] if msg.starts_with("Aucune sauvegarde")),
        "{events:?}"
    );
    assert!(sim.written().is_empty());
    assert!(!m.probing());

    // the key test: 0x66, then 0x67 on stop (and on any other request)
    let dir = tempfile::tempdir().unwrap();
    let sim = Sim::new(&fixture());
    let mut m = manager(&sim, dir.path());
    connected(&mut m);
    sim.clear_written();
    let events = m.handle(Request::ProbeStart(Probe::KeyTest));
    assert!(matches!(events.as_slice(), [Event::Probe(Some(Probe::KeyTest))]));
    assert!(m.probing() && m.pump_probe().is_empty());
    let events = m.handle(Request::ReadAll);
    assert!(matches!(events[0], Event::Probe(None)), "{events:?}");
    let cmds: Vec<u8> = sim.written().iter().map(|r| r[1]).collect();
    assert_eq!(&cmds[..2], &[0x66, 0x67]);
    assert!(!m.probing());
}

#[test]
fn apply_profile_backs_up_first() {
    use freejazz::profiles;
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let profiles_dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let base = connected(&mut m);
    profiles::save_profile(profiles_dir.path(), "Actuel", &base).unwrap();
    let path = profiles::profile_path(profiles_dir.path(), "Actuel");

    // a change on the keyboard, then the profile puts the saved state back
    let mut edited = edit(&base);
    edited.led_effect.brightness = 2;
    m.apply(&edited).unwrap();
    let before = files(dir.path());
    sim.clear_written();
    let events = m.handle(Request::ApplyProfile {
        name: "Actuel".into(),
        path: path.clone(),
    });
    assert_eq!(files(dir.path()), before + 1, "pre-restore backup first");
    let Some(Event::Connected { snapshot, .. }) = events.first() else {
        panic!("{events:?}");
    };
    assert_eq!(snapshot.led_effect.brightness, base.led_effect.brightness);
    assert!(
        matches!(events.last(), Some(Event::Notice(n)) if n.contains("Profil « Actuel » appliqué")),
        "{events:?}"
    );

    // no backup folder: refused, nothing written
    let sim = Sim::new(&fixture());
    let mut m = Manager::new(sim.port(), None).with_wait(FAST);
    connected(&mut m);
    sim.clear_written();
    let events = m.handle(Request::ApplyProfile {
        name: "Actuel".into(),
        path,
    });
    assert!(matches!(events.as_slice(), [Event::Failed(_)]), "{events:?}");
    assert!(sets(&sim).is_empty());
}

#[test]
fn custom_lighting_apply_succeeds_and_later_writes_send_128() {
    // firmware 1.28 reads mode 128 back as 20 (protocol.md 9.7)
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let base = connected(&mut m);
    let mut edited = edit(&base);
    edited.led_effect.mode = 128;
    edited.custom_leds.0[3].g = 0x44;
    sim.clear_written();
    let after = m.apply(&edited).expect("custom lighting applies");
    let raw = after.raw.as_ref().expect("re-read");
    assert_eq!(raw.led_effect[0], 20, "the simulated keyboard reads 20");
    assert_eq!(after.led_effect.mode, 128, "the baseline keeps 128");
    let led_sets: Vec<_> = sets(&sim).into_iter().filter(|(c, _)| *c == 0x23).collect();
    assert!(led_sets.iter().all(|(_, r)| r[8] == 128), "{led_sets:?}");

    // a later write (brightness) and the end of a live effect send 128 too
    sim.clear_written();
    let mut again = edit(&after);
    again.led_effect.brightness = 2;
    m.apply(&again).expect("second apply");
    m.handle(Request::LiveBegin);
    m.handle(Request::LiveEnd);
    let led_sets: Vec<_> = sets(&sim).into_iter().filter(|(c, _)| *c == 0x23).collect();
    assert!(led_sets.len() >= 2, "{led_sets:?}");
    assert!(led_sets.iter().all(|(_, r)| r[8] == 128), "{led_sets:?}");
}

/// SET_LED_EFFECT reports written, as their data (8 bytes of header dropped).
fn led_writes(sim: &Sim) -> Vec<Vec<u8>> {
    sets(sim)
        .into_iter()
        .filter(|(c, _)| *c == 0x23)
        .map(|(_, r)| r[8..].to_vec())
        .collect()
}

#[test]
fn a_write_during_a_stream_is_not_undone_by_its_end() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let base = connected(&mut m);
    let events = m.handle(Request::LiveBegin);
    assert!(matches!(events.as_slice(), [Event::LiveStarted(true)]), "{events:?}");
    let mut edited = edit(&base);
    edited.led_effect.brightness = 2;
    assert!(matches!(m.handle(Request::Apply(Box::new(edited))).as_slice(), [Event::Applied(_)]));
    sim.clear_written();
    m.handle(Request::LiveEnd);
    let led = led_writes(&sim);
    assert_eq!(led.len(), 1);
    assert_eq!(led[0][9], 2, "the end writes the new stored effect, not the old one");
}

#[test]
fn the_stream_is_marked_until_its_end_is_written() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.json");
    let mut m = manager(&sim, &dir.path().join("backups")).with_marker(Some(settings.clone()));
    connected(&mut m);
    m.handle(Request::LiveBegin);
    assert!(freejazz::settings::live_marker_in(&settings).is_some());
    // the app saves its options meanwhile: the marker stays
    let mut options = freejazz::settings::Settings::default();
    options.note_live(Some("ripple"));
    options.save_to(&settings).unwrap();
    assert!(freejazz::settings::live_marker_in(&settings).is_some());
    // the end could not be written (keyboard gone): still marked for the next start
    sim.unplug();
    m.handle(Request::LiveEnd);
    assert!(freejazz::settings::live_marker_in(&settings).is_some());
    // written: cleared
    sim.plug();
    let mut m = manager(&sim, &dir.path().join("backups")).with_marker(Some(settings.clone()));
    connected(&mut m);
    m.handle(Request::LiveBegin);
    m.handle(Request::LiveEnd);
    assert_eq!(freejazz::settings::live_marker_in(&settings), None);
    assert!(freejazz::settings::Settings::load_from(&settings).live_running);
}

#[test]
fn a_stream_left_running_by_a_dead_app_is_ended_on_connection() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("settings.json");
    freejazz::settings::set_live_marker_in(&settings, Some("en cours"));
    let mut m = manager(&sim, &dir.path().join("backups")).with_marker(Some(settings.clone()));
    let events = m.tick();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Notice(n) if n.contains("éclairage enregistré rétabli"))),
        "{events:?}"
    );
    // the stored effect was written back, after the backup, and the marker is gone
    assert_eq!(led_writes(&sim).len(), 1);
    assert_eq!(freejazz::settings::live_marker_in(&settings), None);
    // no marker: a connection writes nothing
    sim.clear_written();
    sim.unplug();
    m.tick();
    sim.plug();
    m.tick();
    assert!(sets(&sim).is_empty());
}

#[test]
fn the_worker_ends_streams_and_previews_before_it_stops() {
    // a preview left on the keyboard: the shutdown writes the stored effect back
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let base = connected(&mut m);
    let mut led = base.led_effect.clone();
    led.mode = 8;
    m.handle(Request::PreviewLed(Box::new(led)));
    sim.clear_written();
    m.shutdown();
    let written = led_writes(&sim);
    assert_eq!(written.len(), 1);
    assert_eq!(written[0][0], base.led_effect.mode);
    // previewing the stored effect again leaves nothing to undo
    let mut m = manager(&sim, dir.path());
    let stored = connected(&mut m).led_effect;
    let mut led = stored.clone();
    led.mode = 8;
    m.handle(Request::PreviewLed(Box::new(led)));
    m.handle(Request::PreviewLed(Box::new(stored)));
    sim.clear_written();
    m.shutdown();
    assert!(led_writes(&sim).is_empty());

    // through the handle: requests already sent are carried out, then the worker ends
    let sim = Sim::new(&fixture());
    let manager = manager(&sim, dir.path());
    let mut handle = freejazz::device::DeviceHandle::spawn(
        freejazz::device::DeviceKind::Mock,
        std::sync::Arc::new(|| {}),
        move |io| freejazz::device::run(io, manager),
    );
    loop {
        match handle.poll() {
            Some(Event::Connected { .. }) => break,
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    handle.send(Request::LiveBegin);
    for _ in 0..3 {
        handle.send(Request::LiveFrame(Box::new([Default::default(); freejazz_protocol::live::FRAME_SLOTS]), 0));
    }
    sim.clear_written();
    assert!(handle.shutdown(Duration::from_secs(2)), "the worker ended");
    let written = sim.written();
    let last_frame = written.iter().rposition(|r| r[1] == 0x32);
    let end = written.iter().rposition(|r| r[1] == 0x23);
    assert!(end.is_some(), "the stored effect was written back");
    assert!(last_frame.is_none_or(|f| f < end.unwrap()));
}

#[test]
fn a_keyboard_that_does_not_answer_is_probed_less_often() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path()).with_probe_backoff(Duration::from_millis(150));
    sim.set_silent(true);
    m.tick();
    assert_eq!(sim.probes(), 1);
    m.tick();
    m.tick();
    assert_eq!(sim.probes(), 1, "no new probe right after a silent one");
    std::thread::sleep(Duration::from_millis(170));
    m.tick();
    assert_eq!(sim.probes(), 2);
    sim.set_silent(false);
    std::thread::sleep(Duration::from_millis(170));
    connected(&mut m);
}

#[test]
fn the_colour_written_survives_later_reads_and_goes_into_the_pre_restore_backup() {
    use freejazz_protocol::blocks::Rgb;
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let base = connected(&mut m);
    let colour = Rgb {
        r: 0x12,
        g: 0x34,
        b: 0x56,
    };
    let mut edited = edit(&base);
    edited.led_effect.rgb = colour;
    edited.led_effect.color_mode = 0;
    m.apply(&edited).unwrap();
    // « Relire »: the keyboard reports white, the baseline keeps the colour written
    let events = m.handle(Request::ReadAll);
    let Some(Event::Connected { snapshot, .. }) = events.first() else {
        panic!("{events:?}");
    };
    assert_eq!(snapshot.raw.as_ref().unwrap().led_effect[1..4], [0xFF; 3]);
    assert_eq!(snapshot.led_effect.rgb, colour);
    // the backup taken before a restore holds that colour
    m.restore(None).unwrap();
    let pre = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().contains("avant-restauration"))
        .unwrap();
    let held = load_backup(&pre).unwrap();
    assert_eq!(held.led_effect.rgb, colour);
    assert_eq!(held.led_effect.color_mode, 0);
}

#[test]
fn no_readable_backup_is_said_so() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("ak980max-8043-20261025-100349.json"), "{}").unwrap();
    let mut m = manager(&sim, dir.path());
    let events = m.tick();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Notice(n) if n.starts_with("Aucune sauvegarde lisible"))),
        "{events:?}"
    );
}

#[test]
fn previewing_an_effect_writes_only_the_led_block_and_keeps_the_baseline() {
    let sim = Sim::new(&fixture());
    let dir = tempfile::tempdir().unwrap();
    let mut m = manager(&sim, dir.path());
    let snapshot = connected(&mut m);
    let before = sets(&sim).len();
    let mut led = snapshot.led_effect.clone();
    led.mode = 8;
    let events = m.handle(Request::PreviewLed(Box::new(led.clone())));
    assert!(events.is_empty(), "{events:?}");
    let written = sets(&sim);
    assert_eq!(written.len(), before + 1);
    assert_eq!(written.last().unwrap().0, 0x23, "SET_LED_EFFECT");
    // the baseline is the stored effect: « Annuler » is an ordinary preview of it
    assert_eq!(m.snapshot().unwrap().led_effect, snapshot.led_effect);
    m.handle(Request::PreviewLed(Box::new(snapshot.led_effect.clone())));
    assert_eq!(sets(&sim).len(), before + 2);
}
