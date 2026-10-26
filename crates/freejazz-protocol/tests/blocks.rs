use freejazz_protocol::backup::{load_backup, save_backup};
use freejazz_protocol::blocks::{
    CustomLeds, DeviceInfo, DksEntry, DksTable, GameMode, KeyAction, KeyKind, KeyMap, LedEffect,
    MACRO_CAPACITY, Macro, MacroAction, Macros, MagneticKey, MagneticMap, ModelLimits, Rgb,
};
use freejazz_protocol::cmd::Cmd;
use freejazz_protocol::error::Error;
use freejazz_protocol::keyboard::{Keyboard, Snapshot};
use freejazz_protocol::transport::MockLink;

// ---------- helpers ----------

/// A 32-byte response report `55 cmd len addr ... data`.
fn reply(cmd: u8, addr: u16, len: usize, data: &[u8]) -> Vec<u8> {
    let mut r = vec![0u8; 32];
    r[0] = 0x55;
    r[1] = cmd;
    r[2] = len as u8;
    r[3..5].copy_from_slice(&addr.to_le_bytes());
    r[8..8 + data.len()].copy_from_slice(data);
    r
}

/// Scripts the keyboard to answer a chunked read of `bytes` starting at `addr_start`.
fn serve(link: &mut MockLink, cmd: u8, addr_start: u16, bytes: &[u8]) {
    for (t, chunk) in bytes.chunks(24).enumerate() {
        let addr = addr_start + 24 * t as u16;
        link.script(cmd, addr, vec![reply(cmd, addr, chunk.len(), chunk)]);
    }
}

/// Scripts the keyboard to acknowledge a chunked write of `len` bytes.
fn ack(link: &mut MockLink, cmd: u8, addr_start: u16, len: usize) {
    for t in 0..len.div_ceil(24) {
        let addr = addr_start + 24 * t as u16;
        link.script(cmd, addr, vec![reply(cmd, addr, 24.min(len - 24 * t), &[])]);
    }
}

/// Data bytes of every written report for `cmd`, concatenated in order.
fn written_block(reports: &[Vec<u8>], cmd: u8) -> Vec<u8> {
    reports
        .iter()
        .filter(|r| r[1] == cmd)
        .flat_map(|r| r[8..8 + r[2] as usize].to_vec())
        .collect()
}

/// Command codes of `reports`, consecutive duplicates removed.
fn cmd_sequence(reports: &[Vec<u8>]) -> Vec<u8> {
    let mut seq: Vec<u8> = reports.iter().map(|r| r[1]).collect();
    seq.dedup();
    seq
}

fn info_image(frame_version: u8, max_profile: u8) -> Vec<u8> {
    let mut b = vec![0u8; 48];
    b[2..4].copy_from_slice(&512u16.to_le_bytes());
    b[4..6].copy_from_slice(&0x0C45u16.to_le_bytes());
    b[6..8].copy_from_slice(&0x8043u16.to_le_bytes());
    b[8] = 0x23;
    b[9] = 1;
    b[19] = 1;
    b[28] = 255;
    b[30] = frame_version;
    b[33] = max_profile;
    b
}

fn is_validation(e: &Error) -> bool {
    matches!(e, Error::Validation(_))
}

// ---------- DeviceInfo ----------

#[test]
fn device_info_decodes_version() {
    let mut raw = info_image(1, 3);
    raw[29] = 2;
    let info = DeviceInfo::decode(&raw).unwrap();
    assert_eq!(info.firmware, "1.23");
    assert_eq!(info.max_profile, 3);
    assert_eq!(
        (info.macro_space, info.vid, info.pid),
        (512, 0x0C45, 0x8043)
    );
    assert_eq!((info.current_profile, info.tft_direction), (1, 255));
    assert_eq!((info.rt_precision, info.frame_version), (2, 1));
    assert_eq!(info.raw, raw);

    raw[8] = 0x05;
    raw[9] = 0;
    assert_eq!(DeviceInfo::decode(&raw).unwrap().firmware, "0.05");
    assert!(DeviceInfo::decode(&raw[..47]).is_err());
}

#[test]
fn macro_capacity_is_the_sites_fixed_512() {
    // protocol.md 6.6: the site ignores macroSpaceSize for this model; it stays decoded
    assert_eq!(MACRO_CAPACITY, 512);
    let mut raw = info_image(0, 1);
    raw[2..4].copy_from_slice(&300u16.to_le_bytes());
    assert_eq!(DeviceInfo::decode(&raw).unwrap().macro_space, 300);
}

// ---------- GameMode ----------

#[test]
fn game_mode_preserves_reserved_bytes() {
    let mut raw = [0u8; 56];
    raw[0] = 0x5A;
    raw[5] = 3;
    raw[10] = 7;
    raw[12] = 9;
    raw[55] = 0xEE;
    let mut gm = GameMode::decode(&raw).unwrap();
    assert_eq!(gm.report_rate(), Some(1000));
    gm.set_report_rate(8000).unwrap();
    let out = gm.encode();
    let mut expected = raw;
    expected[5] = 6;
    assert_eq!(out, expected.to_vec());
    assert_eq!((out[0], out[10], out[12], out[55]), (0x5A, 7, 9, 0xEE));

    // unsupported rate: rejected, nothing changed
    let e = gm.set_report_rate(500).unwrap_err();
    assert!(is_validation(&e));
    assert_eq!(gm.encode(), expected.to_vec());
    // codes 3..=6 = 1000/2000/4000/8000 Hz (protocol.md 6.2); others unknown
    for (code, hz) in [(3u8, 1000u32), (4, 2000), (5, 4000), (6, 8000)] {
        raw[5] = code;
        assert_eq!(GameMode::decode(&raw).unwrap().report_rate(), Some(hz));
    }
    raw[5] = 2;
    assert_eq!(GameMode::decode(&raw).unwrap().report_rate(), None);
}

#[test]
fn game_mode_fields_and_dead_zones() {
    let mut raw = [0u8; 56];
    raw[8] = 30;
    raw[9] = 20;
    raw[11] = 1;
    raw[14] = 1;
    raw[17] = 1;
    let mut gm = GameMode::decode(&raw).unwrap();
    assert!((gm.top_dead_zone_mm() - 0.30).abs() < 1e-6);
    assert!((gm.bottom_dead_zone_mm() - 0.20).abs() < 1e-6);
    assert_eq!(
        (gm.stability_mode(), gm.auto_calibration(), gm.nkro()),
        (1, 1, 1)
    );
    // rounded, not truncated (the site truncates 0.29 to 28)
    gm.set_top_dead_zone_mm(0.29).unwrap();
    assert_eq!(gm.encode()[8], 29);
    for bad in [0.51f32, -0.01, 0.255, f32::NAN] {
        assert!(
            is_validation(&gm.set_bottom_dead_zone_mm(bad).unwrap_err()),
            "{bad}"
        );
    }
    assert_eq!(gm.encode()[9], 20);
}

#[test]
fn game_mode_json_applies_edits_over_raw() {
    let mut raw = [0u8; 56];
    raw[0] = 0x5A;
    raw[5] = 3;
    raw[8] = 30;
    raw[40] = 1;
    let gm = GameMode::decode(&raw).unwrap();
    let mut v = serde_json::to_value(&gm).unwrap();
    assert_eq!(v["reportRate"], 1000);
    assert!((v["topDeadZoneMm"].as_f64().unwrap() - 0.30).abs() < 1e-6);

    // untouched JSON gives back the same bytes
    let same: GameMode = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(same, gm);

    // edited fields apply on top of raw; unread bytes stay
    v["reportRate"] = 4000.into();
    v["stabilityMode"] = 1.into();
    let edited: GameMode = serde_json::from_value(v.clone()).unwrap();
    let mut expected = raw;
    expected[5] = 5;
    expected[11] = 1;
    assert_eq!(edited.encode(), expected.to_vec());

    // an out-of-range edit is refused
    v["topDeadZoneMm"] = 0.6.into();
    assert!(serde_json::from_value::<GameMode>(v).is_err());
}

// ---------- Keys ----------

#[test]
fn key_action_kinds_roundtrip() {
    let cases: Vec<([u8; 4], KeyKind)> = vec![
        ([0, 0, 0, 0], KeyKind::Default),
        ([1, 1, 4, 0], KeyKind::Mouse { kind: 1, value: 4 }),
        (
            [2, 0, 0x04, 0],
            KeyKind::Keyboard {
                modifiers: 0,
                usage: 0x04,
            },
        ),
        // modifier mask AND usage (protocol.md 6.3): kept byte for byte
        (
            [2, 0x02, 0x04, 0],
            KeyKind::Keyboard {
                modifiers: 0x02,
                usage: 0x04,
            },
        ),
        ([3, 0xE9, 0x00, 0], KeyKind::Consumer(0x00E9)),
        ([3, 0x23, 0x02, 0], KeyKind::Consumer(0x0223)),
        (
            [6, 7, 1, 3],
            KeyKind::Macro {
                id: 7,
                p2: 1,
                p3: 3,
            },
        ),
        ([7, 1, 2, 3], KeyKind::Cb([1, 2, 3])),
        ([8, 5, 0, 0], KeyKind::Dks(5)),
        ([9, 0xE0, 0x04, 40], KeyKind::Mt([0xE0, 0x04, 40])),
        ([0x0A, 0x2C, 0, 0], KeyKind::Tgl(0x2C)),
        ([0x0B, 3, 0x04, 0x07], KeyKind::Socd([3, 0x04, 0x07])),
        ([0x0C, 0, 0x04, 0x07], KeyKind::Rs([0, 0x04, 0x07])),
        ([0x0D, 0x01, 0x02, 0x03], KeyKind::Func(0x01_02_03)),
        ([0x0E, 1, 2, 0], KeyKind::End([1, 2])),
        ([0x0F, 1, 2, 3], KeyKind::Mpt([1, 2, 3])),
        (
            [0x91, 0x23, 0x45, 0x67],
            KeyKind::FuncV2 {
                first: 0x1123,
                second: 0x4567,
            },
        ),
        (
            [0xFF, 0xFF, 0xFF, 0xFF],
            KeyKind::FuncV2 {
                first: 0x7FFF,
                second: 0xFFFF,
            },
        ),
        ([0x2F, 1, 2, 3], KeyKind::Unknown([0x2F, 1, 2, 3])),
    ];
    for (raw, kind) in cases {
        let a = KeyAction { raw };
        assert_eq!(a.kind(), kind, "{raw:02X?}");
        assert_eq!(KeyAction::from_kind(a.kind()).raw, raw, "{raw:02X?}");
    }
}

#[test]
fn non_canonical_bytes_decode_as_unknown() {
    // known pages carrying bytes the site never writes keep them via Unknown
    for raw in [
        [0, 1, 0, 0],
        [1, 1, 1, 9],
        [2, 0, 4, 1],
        [3, 0xE9, 0, 1],
        [8, 5, 1, 0],
        [0x0A, 0x2C, 0, 1],
        [0x0E, 1, 2, 3],
        [4, 9, 9, 9],
        [5, 0, 0, 0],
        [0x7F, 0, 0, 0],
    ] {
        let a = KeyAction { raw };
        assert_eq!(a.kind(), KeyKind::Unknown(raw), "{raw:02X?}");
        assert_eq!(KeyAction::from_kind(a.kind()).raw, raw);
    }
}

#[test]
fn unknown_page_type_roundtrips() {
    let mut bytes = vec![0u8; 512];
    bytes[40..44].copy_from_slice(&[0x2F, 1, 2, 3]);
    bytes[44..48].copy_from_slice(&[4, 9, 9, 9]); // SYSTEM_KEY: not decoded by the site
    bytes[48..52].copy_from_slice(&[2, 0, 0x29, 0]);
    let map = KeyMap::decode(&bytes).unwrap();
    assert_eq!(map.0[10].kind(), KeyKind::Unknown([0x2F, 1, 2, 3]));
    assert_eq!(map.0[11].kind(), KeyKind::Unknown([4, 9, 9, 9]));
    assert_eq!(map.encode(), bytes);

    // through JSON (backup file / front end) too
    let json = serde_json::to_string(&map).unwrap();
    let back: KeyMap = serde_json::from_str(&json).unwrap();
    assert_eq!(back.encode(), bytes);
    assert!(KeyMap::decode(&bytes[..508]).is_err());
}

#[test]
fn key_json_is_tagged_and_checked() {
    let a = KeyAction::from_kind(KeyKind::Keyboard {
        modifiers: 0,
        usage: 0x04,
    });
    let v = serde_json::to_value(a).unwrap();
    assert_eq!(v["type"], "keyboard");
    assert_eq!(v["data"]["usage"], 4);
    let v = serde_json::to_value(KeyAction::from_kind(KeyKind::FuncV2 {
        first: 1,
        second: 2,
    }))
    .unwrap();
    assert_eq!(v["type"], "funcV2");
    // values that do not fit the 4 bytes are refused, never truncated
    let too_big = serde_json::json!({ "type": "func", "data": 0x0100_0000u32 });
    assert!(serde_json::from_value::<KeyAction>(too_big).is_err());
    let too_big = serde_json::json!({ "type": "funcV2", "data": { "first": 0x8000, "second": 0 } });
    assert!(serde_json::from_value::<KeyAction>(too_big).is_err());
}

#[test]
fn key_map_rejects_bad_macro_and_dks_refs() {
    let mut map = KeyMap::default();
    map.0[3] = KeyAction::from_kind(KeyKind::Macro {
        id: 99,
        p2: 0,
        p3: 0,
    });
    map.0[4] = KeyAction::from_kind(KeyKind::Dks(63));
    assert!(map.validate().is_ok());
    map.0[3] = KeyAction::from_kind(KeyKind::Macro {
        id: 100,
        p2: 0,
        p3: 0,
    });
    assert!(is_validation(&map.validate().unwrap_err()));
    map.0[3] = KeyAction::default();
    map.0[4] = KeyAction::from_kind(KeyKind::Dks(64));
    assert!(is_validation(&map.validate().unwrap_err()));
}

// ---------- Lighting ----------

#[test]
fn led_effect_write_sets_magic() {
    let raw = [3, 10, 20, 30, 0, 1, 2, 3, 1, 4, 2, 1, 7, 0x77, 0, 0];
    let e = LedEffect::decode(&raw).unwrap();
    assert_eq!(e.mode, 3);
    assert_eq!(
        e.rgb,
        Rgb {
            r: 10,
            g: 20,
            b: 30
        }
    );
    assert_eq!(e.secondary, Rgb { r: 1, g: 2, b: 3 });
    assert_eq!(
        (
            e.color_mode,
            e.brightness,
            e.speed,
            e.direction,
            e.effect_mode_type
        ),
        (1, 4, 2, 1, 7)
    );
    let out = e.encode();
    assert_eq!(out.len(), 16);
    assert_eq!((out[4], out[13], out[14], out[15]), (255, 0, 0xAA, 0x55));
    assert_eq!(out[..4], raw[..4]);
    assert_eq!(out[5..13], raw[5..13]);
}

#[test]
fn led_readback_ignores_colour_and_color_mode() {
    // Firmware 1.28 reports #ffffff / RGB after any write (protocol.md 9.7): bytes 1-3
    // and 8 cannot be verified by reading back; mode, brightness, speed, direction can.
    let written =
        LedEffect::decode(&[9, 0xFF, 0xB0, 0xB0, 0, 0, 0, 0, 0, 6, 3, 0, 0, 0, 0, 0]).unwrap();
    let read =
        LedEffect::decode(&[9, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 1, 6, 3, 0, 0, 0, 0, 0]).unwrap();
    assert!(written.matches_readback(&read));
    assert!(!written.colour_verified_by(&read));
    for (i, v) in [(0, 1), (9, 5), (10, 2), (11, 1), (5, 7), (12, 1)] {
        let mut b = read.encode();
        b[i] = v;
        let other = LedEffect::decode(&b).unwrap();
        assert!(!written.matches_readback(&other), "byte {i}");
    }
    // the same rule in the snapshot check load_backup and the selftest use
    let im = images();
    let mut link = MockLink::new(32);
    serve_snapshot(&mut link, &im);
    let snap = Keyboard::connect(link).unwrap().read_snapshot().unwrap();
    let mut raw = snap.raw.clone().unwrap();
    raw.led_effect[1..4].copy_from_slice(&[0xFF; 3]);
    raw.led_effect[8] = 1;
    assert_eq!(snap.check_raw(&raw), Ok(()));
    raw.led_effect[9] += 1;
    assert_eq!(snap.check_raw(&raw), Err("effet lumineux"));
}

#[test]
fn led_custom_mode_128_reads_back_as_20() {
    let mut b = [0u8; 16];
    b[0] = 128;
    b[9] = 3;
    let written = LedEffect::decode(&b).unwrap();
    b[0] = 20;
    let read = LedEffect::decode(&b).unwrap();
    assert!(written.matches_readback(&read));
    // a hand-built 20 is written as 128, never as 20
    let raw20 = LedEffect {
        mode: 20,
        ..written.clone()
    };
    assert!(written.matches_readback(&raw20));
    assert_eq!(raw20.encode()[0], 128);
    assert_eq!(read.encode()[0], 128);
}

#[test]
fn custom_leds_write_position_as_index() {
    let mut raw = vec![0u8; 512];
    for i in 0..128 {
        raw[4 * i..4 * i + 4].copy_from_slice(&[i as u8, i as u8, 255 - i as u8, 7]);
    }
    let leds = CustomLeds::decode(&raw).unwrap();
    assert_eq!(leds.0[2], Rgb { r: 2, g: 253, b: 7 });
    assert_eq!(leds.encode(), raw);
}

// ---------- Magnetic ----------

fn magnetic_entry(axis: u8, flags: u8, trigger: u16, press: u16, release: u16) -> [u8; 8] {
    let mut e = [0u8; 8];
    e[0] = axis;
    e[1] = flags;
    e[2..4].copy_from_slice(&trigger.to_le_bytes());
    e[4..6].copy_from_slice(&press.to_le_bytes());
    e[6..8].copy_from_slice(&release.to_le_bytes());
    e
}

#[test]
fn magnetic_scales_with_precision() {
    let close = |a: f32, b: f32| (a - b).abs() < 1e-6;
    // precision 0: everything / 100
    let mut bytes = vec![0u8; 1024];
    // flags 0xFD: whole_fast, no rampage, and bits 2-7 the types do not model
    bytes[..8].copy_from_slice(&magnetic_entry(1, 0xFD, 120, 10, 20));
    let m = MagneticMap::decode(&bytes, 0).unwrap();
    let k = &m.0[0];
    assert!(
        close(k.trigger_mm, 1.20) && close(k.press_rt_mm, 0.10) && close(k.release_rt_mm, 0.20)
    );
    assert!(k.whole_fast && !k.rampage);
    assert_eq!(k.other_flags, 0xFC);
    assert_eq!(m.encode(0).unwrap(), bytes);

    // precision 2: trigger and RT / 1000
    bytes[..8].copy_from_slice(&magnetic_entry(2, 0b10, 1200, 100, 200));
    let m = MagneticMap::decode(&bytes, 2).unwrap();
    let k = &m.0[0];
    assert!(
        close(k.trigger_mm, 1.20) && close(k.press_rt_mm, 0.10) && close(k.release_rt_mm, 0.20)
    );
    assert!(!k.whole_fast && k.rampage);
    assert_eq!(m.encode(2).unwrap(), bytes);

    // precision 1: trigger / 100, RT / 1000
    bytes[..8].copy_from_slice(&magnetic_entry(1, 0, 120, 100, 200));
    let m = MagneticMap::decode(&bytes, 1).unwrap();
    assert!(close(m.0[0].trigger_mm, 1.20) && close(m.0[0].press_rt_mm, 0.10));
    assert_eq!(m.encode(1).unwrap(), bytes);

    // a value that does not fit 16 bits is refused, never truncated
    let mut m = MagneticMap::default();
    m.0[0].trigger_mm = 700.0;
    assert!(m.encode(0).is_err());
}

#[test]
fn magnetic_refuses_values_off_the_device_step() {
    let mut m = MagneticMap::default();
    m.0[0] = MagneticKey {
        axis: 1,
        trigger_mm: 1.20,
        press_rt_mm: 0.015,
        release_rt_mm: 0.02,
        ..MagneticKey::default()
    };
    // precision 0: RT in 0.01 mm steps, 0.015 would be rounded: refused
    let e = m.encode(0).unwrap_err();
    assert!(is_validation(&e), "{e}");
    assert!(e.to_string().contains("sensibilité"), "{e}");
    // precision > 0: RT in 0.001 mm steps
    assert!(m.encode(1).is_ok());
    // trigger in 0.01 mm steps below precision 2
    m.0[0].press_rt_mm = 0.10;
    m.0[0].trigger_mm = 1.205;
    for precision in [0, 1] {
        let e = m.encode(precision).unwrap_err();
        assert!(is_validation(&e), "{e}");
        assert!(e.to_string().contains("déclenchement"), "{e}");
    }
    assert!(m.encode(2).is_ok());
    m.0[0].trigger_mm = -0.01;
    assert!(is_validation(&m.encode(2).unwrap_err()));
}

#[test]
fn rt_rejects_out_of_range() {
    let limits = ModelLimits::ak980max();
    assert!((limits.min_trigger_trip - 0.10).abs() < 1e-6);
    assert!((limits.max_trigger_trip - 3.30).abs() < 1e-6);
    assert_eq!(limits.axis_list.len(), 3);

    let mut map = MagneticMap::default();
    // all-zero entries (slots without a key) are not checked
    assert!(map.validate(limits).is_ok());
    map.0[3] = MagneticKey {
        axis: 1,
        whole_fast: false,
        rampage: false,
        trigger_mm: 1.20,
        press_rt_mm: 0.10,
        release_rt_mm: 0.10,
        other_flags: 0,
    };
    assert!(map.validate(limits).is_ok());

    for ok in [0.10f32, 3.30, 1.21] {
        map.0[3].trigger_mm = ok;
        assert!(map.validate(limits).is_ok(), "{ok}");
    }
    for bad in [3.31f32, 0.09, 1.205, 0.0, f32::NAN] {
        map.0[3].trigger_mm = bad;
        let e = map.validate(limits).unwrap_err();
        assert!(is_validation(&e), "{bad}");
        assert!(e.to_string().contains("déclenchement"), "{e}");
    }
    map.0[3].trigger_mm = 1.20;

    // RT off = both sensitivities 0
    map.0[3].press_rt_mm = 0.0;
    map.0[3].release_rt_mm = 0.0;
    assert!(map.validate(limits).is_ok());
    for (press, release) in [(3.31f32, 0.10f32), (0.10, 0.005), (0.0, 0.10)] {
        map.0[3].press_rt_mm = press;
        map.0[3].release_rt_mm = release;
        let e = map.validate(limits).unwrap_err();
        assert!(is_validation(&e), "{press} {release}");
        assert!(e.to_string().contains("sensibilité"), "{e}");
    }
}

// ---------- Macros ----------

fn act(press: bool, kind: u8, key: u8, delay_ms: u16) -> MacroAction {
    MacroAction {
        press,
        kind,
        key,
        delay_ms,
        raw_flags: None,
    }
}

fn mac(id: u8, actions: Vec<MacroAction>) -> Macro {
    Macro {
        id,
        actions,
        raw_header: None,
    }
}

/// `(press, kind, key, delay_ms)`: an action without the bytes it was read from.
fn plain(a: &MacroAction) -> (bool, u8, u8, u16) {
    (a.press, a.kind, a.key, a.delay_ms)
}

#[test]
fn macro_encode_layout() {
    let macros = Macros {
        slots: vec![mac(0, vec![act(true, 1, 1, 300), act(false, 1, 1, 0)])],
    };
    let (table, data) = macros.encode(MACRO_CAPACITY).unwrap();
    assert_eq!(table.len(), 400);
    assert_eq!(table[..4], 400u32.to_le_bytes());
    assert!(table[4..].iter().all(|&b| b == 0));
    assert_eq!(data, vec![4, 0, 0, 0, 0x2C, 0x01, 1, 0x90, 0, 0, 1, 0x10]);

    // keyboard actions use 0xB0/0x30; macros are laid out by id with absolute pointers
    let macros = Macros {
        slots: vec![
            mac(5, vec![act(true, 3, 0x04, 10), act(false, 3, 0x04, 0)]),
            mac(0, vec![act(true, 1, 1, 300), act(false, 2, 1, 0)]),
            mac(9, vec![]),
        ],
    };
    let (table, data) = macros.encode(MACRO_CAPACITY).unwrap();
    assert_eq!(table[..4], 400u32.to_le_bytes());
    assert_eq!(table[20..24], 412u32.to_le_bytes());
    assert_eq!(table[36..40], [0, 0, 0, 0]); // a new macro without actions gets pointer 0
    assert_eq!(data[8..12], [0, 0, 1, 0x10]); // kind 2 is written as the mouse type too
    assert_eq!(
        data[12..],
        [4, 0, 0, 0, 10, 0, 0x04, 0xB0, 0, 0, 0x04, 0x30]
    );

    // decoding follows the pointers, and encoding what was decoded gives the bytes back
    let decoded = Macros::decode(&table, |addr, len| {
        let at = usize::from(addr) - 400;
        Ok(data[at..at + len].to_vec())
    })
    .unwrap();
    let ids: Vec<u8> = decoded.slots.iter().map(|m| m.id).collect();
    assert_eq!(ids, vec![0, 5]);
    let actions: Vec<_> = decoded.slots[1].actions.iter().map(plain).collect();
    assert_eq!(actions, vec![(true, 3, 0x04, 10), (false, 3, 0x04, 0)]);
    assert_eq!(plain(&decoded.slots[0].actions[0]), (true, 1, 1, 300));
    assert_eq!(decoded.to_bytes().unwrap(), (table, data));
}

#[test]
fn macro_keeps_bytes_it_does_not_model() {
    // header bytes 2-3, an odd count (n = 5: 2 actions), flag low bits, flag type 2
    let mut table = vec![0u8; 400];
    table[..4].copy_from_slice(&400u32.to_le_bytes());
    let data = vec![5, 0, 0xAB, 0xCD, 0x2C, 0x01, 0x04, 0xB5, 0, 0, 1, 0xA0];
    let mut m = Macros::decode(&table, |addr, len| {
        let at = usize::from(addr) - 400;
        Ok(data[at..at + len].to_vec())
    })
    .unwrap();
    assert_eq!(plain(&m.slots[0].actions[0]), (true, 3, 0x04, 300));
    assert_eq!(plain(&m.slots[0].actions[1]), (true, 2, 1, 0));
    assert_eq!(m.to_bytes().unwrap(), (table.clone(), data.clone()));

    // edits follow the site's encoding; bits no edited field covers stay
    m.slots[0].actions[0].press = false; // 0xB5 -> 0x35 (low bits kept)
    m.slots[0].actions[1].kind = 3; // 0xA0 -> 0xB0
    m.slots[0].actions.push(act(true, 2, 1, 0)); // new: 0x90
    let (_, edited) = m.to_bytes().unwrap();
    assert_eq!(edited[..4], [6, 0, 0xAB, 0xCD]); // count edited, bytes 2-3 kept
    assert_eq!((edited[7], edited[11], edited[15]), (0x35, 0xB0, 0x90));
}

#[test]
fn macro_over_capacity_rejected() {
    let m = |id: u8, n: usize| mac(id, vec![act(true, 3, 0x04, 10); n]);
    let encode = |slots: Vec<Macro>, capacity: u16| Macros { slots }.encode(capacity);
    // 4 x macros + 4 x actions <= capacity (protocol.md 6.6)
    assert!(encode(vec![m(0, 127)], MACRO_CAPACITY).is_ok());
    let e = encode(vec![m(0, 128)], MACRO_CAPACITY).unwrap_err();
    assert!(is_validation(&e));
    assert!(e.to_string().contains("512"), "{e}");
    assert!(encode(vec![m(0, 63), m(1, 63)], MACRO_CAPACITY).is_ok()); // 8 + 504
    assert!(is_validation(
        &encode(vec![m(0, 63), m(1, 64)], MACRO_CAPACITY).unwrap_err()
    ));
    assert!(is_validation(&encode(vec![m(0, 127)], 256).unwrap_err()));
    // id 0..=99, no duplicates
    assert!(encode(vec![m(99, 1)], MACRO_CAPACITY).is_ok());
    assert!(is_validation(
        &encode(vec![m(100, 1)], MACRO_CAPACITY).unwrap_err()
    ));
    assert!(is_validation(
        &encode(vec![m(3, 1), m(3, 1)], MACRO_CAPACITY).unwrap_err()
    ));

    // restore path: no capacity limit, but ids must still fit the 100-pointer table
    assert!(
        Macros {
            slots: vec![m(0, 128)]
        }
        .to_bytes()
        .is_ok()
    );
    assert!(is_validation(
        &Macros {
            slots: vec![m(100, 1)]
        }
        .to_bytes()
        .unwrap_err()
    ));
    assert!(is_validation(
        &Macros {
            slots: vec![m(3, 1), m(3, 1)]
        }
        .to_bytes()
        .unwrap_err()
    ));
}

// ---------- DKS ----------

#[test]
fn dks_skips_empty_entries() {
    // slot 0 carries bytes 4/6/8/10 (unread by the site): kept
    let e0 = [
        16, 30, 30, 16, 0x01, 0x04, 0x02, 0x05, 0x03, 0, 0x04, 0, 0x01, 0x10, 0x12, 0xF2,
    ];
    let e5 = [
        10, 25, 25, 10, 0, 0x2C, 0, 0, 0, 0, 0, 0, 0x01, 0x10, 0x10, 0x10,
    ];
    let mut bytes = vec![0u8; 1024];
    bytes[..16].copy_from_slice(&e0);
    bytes[80..96].copy_from_slice(&e5);
    // slot 9: bytes 0-3 zero = free for the site, but leftovers it must not lose
    bytes[9 * 16 + 5] = 0x04;
    bytes[9 * 16 + 12] = 0x11;

    let t = DksTable::decode(&bytes).unwrap();
    // all-zero slots are skipped
    assert_eq!(
        t.0.iter().map(|d| d.index).collect::<Vec<_>>(),
        vec![0, 5, 9]
    );
    assert!(!t.0[0].is_empty() && !t.0[1].is_empty() && t.0[2].is_empty());
    let d = &t.0[0];
    assert_eq!(
        (d.make, d.brk, d.actions),
        ([16, 30], [30, 16], [0x04, 0x05, 0, 0])
    );
    assert_eq!(d.reserved, [1, 2, 3, 4]);
    // states[action][stage]: bit 0 = triggered at that stage, bit 1 = held during it
    assert_eq!(d.states[0], [1, 2, 2, 2]);
    assert_eq!(d.states[1], [0, 0, 1, 3]);
    assert_eq!(d.states[2], [0, 0, 0, 2]);
    assert_eq!(t.encode().unwrap(), bytes);

    // slot 63 is the last one; 64 is refused, never written past the table
    let entry = |index| DksEntry {
        index,
        make: [16, 30],
        brk: [30, 16],
        ..DksEntry::default()
    };
    let last = DksTable(vec![entry(63)]).encode().unwrap();
    assert_eq!(last[63 * 16..63 * 16 + 4], [16, 30, 30, 16]);
    assert!(is_validation(
        &DksTable(vec![entry(64)]).encode().unwrap_err()
    ));
    let mut dup = t.clone();
    dup.0[1].index = 0;
    assert!(is_validation(&dup.encode().unwrap_err()));
    let mut bad_state = t;
    bad_state.0[0].states[0][0] = 4;
    assert!(is_validation(&bad_state.encode().unwrap_err()));
}

// ---------- UI limits ----------

#[test]
fn ui_limits_apply_to_edits_only() {
    let limits = ModelLimits::ak980max();
    let valid = MagneticKey {
        axis: 1,
        trigger_mm: 1.20,
        press_rt_mm: 0.10,
        release_rt_mm: 0.10,
        ..MagneticKey::default()
    };
    let mut baseline = MagneticMap::default();
    baseline.0[1] = MagneticKey {
        axis: 1,
        ..MagneticKey::default()
    }; // as read: trigger 0
    let mut edited = baseline.clone();
    assert!(is_validation(&edited.validate(limits).unwrap_err()));
    assert!(edited.validate_changes(&baseline, limits).is_ok());
    edited.0[5] = valid;
    assert!(edited.validate_changes(&baseline, limits).is_ok());
    edited.0[6] = MagneticKey {
        trigger_mm: 3.31,
        ..valid
    };
    assert!(is_validation(
        &edited.validate_changes(&baseline, limits).unwrap_err()
    ));
    edited.0[6] = valid;
    edited.0[1].trigger_mm = 0.05; // the odd key itself edited: checked
    assert!(is_validation(
        &edited.validate_changes(&baseline, limits).unwrap_err()
    ));

    let mut keys = KeyMap::default();
    keys.0[3] = KeyAction::from_kind(KeyKind::Macro {
        id: 120,
        p2: 0,
        p3: 0,
    }); // as read
    let mut edited = keys.clone();
    assert!(is_validation(&edited.validate().unwrap_err()));
    assert!(edited.validate_changes(&keys).is_ok());
    edited.0[4] = KeyAction::from_kind(KeyKind::Dks(64));
    assert!(is_validation(&edited.validate_changes(&keys).unwrap_err()));

    let read = Macros {
        slots: vec![mac(0, vec![act(true, 3, 0x04, 10); 140])],
    }; // 564 bytes
    assert!(read.validate_changes(&read, MACRO_CAPACITY).is_ok());
    let mut grown = read.clone();
    grown.slots.push(mac(1, vec![act(true, 3, 0x04, 10)]));
    assert!(is_validation(
        &grown.validate_changes(&read, MACRO_CAPACITY).unwrap_err()
    ));
}

// ---------- Keyboard ----------

struct Images {
    info: Vec<u8>,
    game_mode: Vec<u8>,
    keys: Vec<u8>,
    led: Vec<u8>,
    custom: Vec<u8>,
    macro_table: Vec<u8>,
    macro_data: Vec<u8>,
    magnetic: Vec<u8>,
    dks: Vec<u8>,
}

fn images() -> Images {
    let mut game_mode = vec![0u8; 56];
    for (i, v) in [
        (0, 0x5A),
        (5, 6),
        (8, 30),
        (9, 20),
        (10, 7),
        (11, 1),
        (12, 9),
        (14, 1),
        (17, 1),
        (40, 0xEE),
    ] {
        game_mode[i] = v;
    }

    let mut keys = vec![0u8; 512];
    let actions: [[u8; 4]; 8] = [
        [2, 0, 0x29, 0],
        [2, 0x02, 0x04, 0],
        [3, 0xE9, 0, 0],
        [6, 0, 0, 0],
        [8, 5, 0, 0],
        [0x2F, 1, 2, 3],
        [0x91, 0x23, 0x45, 0x67],
        [9, 0xE0, 0x04, 40],
    ];
    for (i, a) in actions.iter().enumerate() {
        keys[4 * i..4 * i + 4].copy_from_slice(a);
    }

    let led = vec![128, 10, 20, 30, 255, 0, 0, 0, 0, 5, 3, 0, 0, 0, 0xAA, 0x55];

    let mut custom = vec![0u8; 512];
    for i in 0..128usize {
        custom[4 * i..4 * i + 4].copy_from_slice(&[i as u8, i as u8, 255 - i as u8, (i * 2) as u8]);
    }

    // macro 0 at 400 (2 actions), macro 7 at 412 (3 actions)
    let mut macro_table = vec![0u8; 400];
    macro_table[..4].copy_from_slice(&400u32.to_le_bytes());
    macro_table[28..32].copy_from_slice(&412u32.to_le_bytes());
    let macro_data = vec![
        4, 0, 0, 0, 0x2C, 0x01, 0x04, 0xB0, 0x0A, 0, 0x04, 0x30, //
        6, 0, 0, 0, 5, 0, 1, 0x90, 5, 0, 1, 0x10, 0, 0, 0x05, 0xB0,
    ];

    let mut magnetic = vec![0u8; 1024];
    magnetic[..8].copy_from_slice(&magnetic_entry(1, 0, 120, 0, 0));
    magnetic[8..16].copy_from_slice(&magnetic_entry(2, 1, 150, 10, 10));
    magnetic[16..24].copy_from_slice(&magnetic_entry(3, 3, 330, 5, 330));

    let mut dks = vec![0u8; 1024];
    dks[..16].copy_from_slice(&[
        10, 25, 25, 10, 0, 0x2C, 0, 0, 0, 0, 0, 0, 0x01, 0x10, 0x10, 0x10,
    ]);
    dks[80..96].copy_from_slice(&[
        16, 30, 30, 16, 0, 0x04, 0, 0x05, 0, 0, 0, 0, 0x01, 0x10, 0x12, 0xF2,
    ]);

    Images {
        info: info_image(0, 3),
        game_mode,
        keys,
        led,
        custom,
        macro_table,
        macro_data,
        magnetic,
        dks,
    }
}

/// Scripts the reads of a macro area (pointer table, data from 400) the way the site
/// reads it: the table, then each macro's header and its actions.
fn serve_macros(link: &mut MockLink, table: &[u8], data: &[u8]) {
    serve(link, 0x15, 0, table);
    for p in table.chunks_exact(4) {
        let p = u32::from_le_bytes([p[0], p[1], p[2], p[3]]) as usize;
        if p == 0 {
            continue;
        }
        let header = &data[p - 400..p - 400 + 4];
        serve(link, 0x15, p as u16, header);
        let len = usize::from(u16::from_le_bytes([header[0], header[1]]) / 2) * 4;
        if len > 0 {
            serve(
                link,
                0x15,
                p as u16 + 4,
                &data[p - 400 + 4..p - 400 + 4 + len],
            );
        }
    }
}

/// Scripts the reads of `Keyboard::connect` then `read_snapshot` of `im`.
fn serve_snapshot(link: &mut MockLink, im: &Images) {
    // device info once for connect, once for the snapshot
    serve(link, 0x10, 0, &im.info);
    serve(link, 0x10, 0, &im.info);
    serve(link, 0x11, 0, &im.game_mode);
    serve(link, 0x12, 0, &im.keys);
    serve(link, 0x13, 0, &im.led);
    serve(link, 0x14, 0, &im.custom);
    serve_macros(link, &im.macro_table, &im.macro_data);
    serve(link, 0x17, 0, &im.magnetic);
    serve(link, 0x18, 0, &im.dks);
}

/// Serves `im` from a mock keyboard, reads a snapshot, sends it through the backup JSON,
/// writes it back, and checks every block is written exactly as it was read.
/// Returns the snapshot and the reports of the read and of the write.
fn roundtrip(im: &Images) -> (Snapshot, Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut link = MockLink::new(32);
    serve_snapshot(&mut link, im);
    ack(&mut link, 0x22, 0, 512);
    ack(&mut link, 0x23, 0, 16);
    ack(&mut link, 0x24, 0, 512);
    ack(&mut link, 0x25, 0, 400);
    ack(&mut link, 0x25, 400, im.macro_data.len());
    ack(&mut link, 0x27, 0, 1024);
    ack(&mut link, 0x28, 0, 1024);
    ack(&mut link, 0x21, 0, 56);

    let mut kb = Keyboard::connect(link).unwrap();
    let snap = kb.read_snapshot().unwrap();
    let reads = kb.link().written.clone();

    let json = serde_json::to_string(&snap).unwrap();
    let back: Snapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(back, snap, "lossless backup JSON");

    kb.write_snapshot(&back).unwrap();
    let writes = kb.link().written[reads.len()..].to_vec();
    assert_eq!(written_block(&writes, 0x22), im.keys, "keys");
    assert_eq!(written_block(&writes, 0x23), im.led, "LED effect");
    assert_eq!(written_block(&writes, 0x24), im.custom, "custom LEDs");
    let macros = [im.macro_table.clone(), im.macro_data.clone()].concat();
    assert_eq!(written_block(&writes, 0x25), macros, "macros");
    assert_eq!(written_block(&writes, 0x27), im.magnetic, "magnetic");
    assert_eq!(written_block(&writes, 0x28), im.dks, "DKS");
    assert_eq!(written_block(&writes, 0x21), im.game_mode, "game mode");
    (snap, reads, writes)
}

#[test]
fn snapshot_roundtrip_through_mock() {
    let im = images();
    let (snap, reads, writes) = roundtrip(&im);

    // reads in the site's order (protocol.md 5), never the Fn layer nor the serial
    assert_eq!(
        cmd_sequence(&reads),
        vec![0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x17, 0x18]
    );
    assert!(!reads.iter().any(|r| matches!(r[1], 0x16 | 0x1C | 0x84)));

    assert_eq!(snap.info.firmware, "1.23");
    assert_eq!(snap.fn_keys, None);
    assert_eq!(snap.game_mode.report_rate(), Some(8000));
    assert_eq!(snap.keys.0[5].kind(), KeyKind::Unknown([0x2F, 1, 2, 3]));
    assert_eq!(
        snap.keys.0[1].kind(),
        KeyKind::Keyboard {
            modifiers: 2,
            usage: 4
        }
    );
    assert_eq!(snap.led_effect.mode, 128);
    assert_eq!(
        snap.macros.slots.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![0, 7]
    );
    assert_eq!(snap.macros.slots[1].actions.len(), 3);
    assert_eq!(
        snap.dks.0.iter().map(|d| d.index).collect::<Vec<_>>(),
        vec![0, 5]
    );
    assert!(snap.magnetic.0[2].whole_fast && snap.magnetic.0[2].rampage);

    // the backup / front-end JSON shape is camelCase
    let json = serde_json::to_string(&snap).unwrap();
    assert!(
        json.contains("\"gameMode\"") && json.contains("\"fnKeys\":null"),
        "{json}"
    );

    // site's profile order, GameMode last (a report-rate change re-enumerates the keyboard)
    assert_eq!(
        cmd_sequence(&writes),
        vec![0x22, 0x23, 0x24, 0x25, 0x27, 0x28, 0x21]
    );
    assert!(!writes.iter().any(|r| r[1] == 0x26));

    // macro table: no last flag on any chunk; data: last flag on its last chunk
    let macro_reports: Vec<&Vec<u8>> = writes.iter().filter(|r| r[1] == 0x25).collect();
    let (table_reports, data_reports) = macro_reports.split_at(17);
    assert!(table_reports.iter().all(|r| r[6] == 0));
    assert_eq!(
        data_reports.iter().map(|r| r[6]).collect::<Vec<_>>(),
        vec![0, 1]
    );
}

#[test]
fn snapshot_keeps_unmodelled_bytes() {
    // every block carries bits the types do not model. LED bytes 4/13/14-15 and the
    // custom-LED index byte are protocol constants, written as the site writes them.
    let mut im = images();
    im.game_mode[30] = 0x42; // unread GameMode byte (also 0, 10, 12, 40 in the image)
    im.keys[32..36].copy_from_slice(&[2, 0, 0x04, 1]); // non-zero 4th byte on KEYBOARD
    im.keys[36..40].copy_from_slice(&[4, 9, 9, 9]); // SYSTEM_KEY page, not decoded
    im.magnetic[8..16].copy_from_slice(&magnetic_entry(2, 0xFD, 150, 10, 10)); // flag bits 2-7
    im.magnetic[24..32].copy_from_slice(&magnetic_entry(1, 0x04, 120, 0, 0));
    for (at, v) in [(4, 0x01), (6, 0x02), (8, 0x03), (10, 0x04)] {
        im.dks[at] = v; // slot 0, bytes the site never reads
    }
    im.dks[9 * 16 + 5] = 0x04; // slot 9: free for the site (bytes 0-3 zero), leftovers kept
    im.dks[9 * 16 + 12] = 0x11;
    // macros: header bytes 2-3, odd count, a macro without actions, flag low bits,
    // flag types 0, 2 and 5
    let mut table = vec![0u8; 400];
    for (id, p) in [(0usize, 400u32), (3, 412), (7, 424), (9, 428)] {
        table[4 * id..4 * id + 4].copy_from_slice(&p.to_le_bytes());
    }
    im.macro_table = table;
    im.macro_data = vec![
        4, 0, 0xAB, 0xCD, 0x2C, 0x01, 0x04, 0xB5, 0x0A, 0, 0x04, 0x3A, // id 0 at 400
        5, 0, 0, 0, 5, 0, 1, 0xA0, 5, 0, 1, 0x80, // id 3 at 412: n = 5, 2 actions
        0, 0, 0x11, 0x22, // id 7 at 424: no action
        2, 0, 0, 0, 0, 0, 0x05, 0xD0, // id 9 at 428
    ];

    let (snap, _, _) = roundtrip(&im);
    assert_eq!(snap.keys.0[8].kind(), KeyKind::Unknown([2, 0, 0x04, 1]));
    assert_eq!(snap.magnetic.0[1].other_flags, 0xFC);
    assert_eq!(snap.dks.0[0].reserved, [1, 2, 3, 4]);
    assert_eq!(
        snap.macros.slots.iter().map(|m| m.id).collect::<Vec<_>>(),
        vec![0, 3, 7, 9]
    );
}

#[test]
fn snapshot_with_out_of_limit_values_restores() {
    // values the keyboard itself holds but the UI would refuse: restored as read
    let mut im = images();
    im.info[29] = 2; // rt_precision 2: trigger and RT in 0.001 mm
    im.game_mode[5] = 2; // report-rate code outside the site's table
    im.game_mode[8] = 60; // 0.60 mm dead zone, above the site's 0.50
    im.keys[32..36].copy_from_slice(&[6, 120, 0, 0]); // macro id > 99
    im.keys[36..40].copy_from_slice(&[8, 70, 0, 0]); // DKS slot > 63
    let mut magnetic = vec![0u8; 1024];
    magnetic[..8].copy_from_slice(&magnetic_entry(1, 0, 1200, 100, 100));
    magnetic[8..16].copy_from_slice(&magnetic_entry(1, 0, 0, 0, 0)); // slot without a key
    magnetic[16..24].copy_from_slice(&[0xFF; 8]); // erased flash
    magnetic[24..32].copy_from_slice(&magnetic_entry(2, 0, 1205, 5, 5)); // off-step, RT < 0.01
    im.magnetic = magnetic;
    // one macro of 140 actions: 564 bytes, over the site's 512
    let mut table = vec![0u8; 400];
    table[..4].copy_from_slice(&400u32.to_le_bytes());
    im.macro_table = table;
    im.macro_data = [vec![0x18, 0x01, 0, 0], [10, 0, 0x04, 0xB0].repeat(140)].concat();

    let (snap, _, _) = roundtrip(&im);
    // the UI checks do refuse these values...
    let limits = ModelLimits::ak980max();
    assert!(is_validation(&snap.magnetic.validate(limits).unwrap_err()));
    assert!(is_validation(&snap.keys.validate().unwrap_err()));
    assert!(is_validation(
        &snap.macros.validate(MACRO_CAPACITY).unwrap_err()
    ));
    assert_eq!(snap.game_mode.report_rate(), None);
    // ...but only on what is edited
    assert!(snap.validate_changes(&snap).is_ok());
    let mut edited = snap.clone();
    edited.magnetic.0[0].trigger_mm = 3.31;
    assert!(is_validation(&edited.validate_changes(&snap).unwrap_err()));
}

// ---------- Backup ----------

/// Reads a snapshot of `im` from a mock keyboard.
fn read_mock_snapshot(im: &Images) -> Snapshot {
    let mut link = MockLink::new(32);
    serve_snapshot(&mut link, im);
    Keyboard::connect(link).unwrap().read_snapshot().unwrap()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn backup_roundtrip() {
    let mut im = images();
    im.info[29] = 1; // rt_precision 1: RT in 0.001 mm
    im.magnetic[24..32].copy_from_slice(&magnetic_entry(2, 0x05, 1205, 7, 1234));
    let snap = read_mock_snapshot(&im);
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("Freejazz").join("backups"); // created on demand

    let path = save_backup(&snap, &dir).unwrap();
    assert_eq!(load_backup(&path).unwrap(), snap);

    // ak980max-<pid hex>-<YYYYMMDD-HHMMSS>.json, in `dir`
    assert_eq!(path.parent(), Some(dir.as_path()));
    let name = path.file_name().unwrap().to_str().unwrap();
    let stamp = name
        .strip_prefix("ak980max-8043-")
        .and_then(|s| s.strip_suffix(".json"))
        .unwrap_or_else(|| panic!("{name}"));
    assert!(
        stamp.len() == 15
            && stamp.char_indices().all(|(i, c)| if i == 8 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }),
        "{name}"
    );

    // pretty JSON: format 1, the typed snapshot (camelCase), every block as read in hex
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("{\n  \"format\": 1,"), "{text}");
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    // (compared decoded: `to_value` would widen the f32 millimetres to f64)
    let typed: Snapshot = serde_json::from_value(json["snapshot"].clone()).unwrap();
    assert_eq!(typed, snap);
    assert!(json["snapshot"]["gameMode"]["raw"].is_array() && json["snapshot"]["fnKeys"].is_null());
    let raw = &json["raw"];
    for (key, bytes) in [
        ("deviceInfo", &im.info),
        ("gameMode", &im.game_mode),
        ("keys", &im.keys),
        ("ledEffect", &im.led),
        ("customLeds", &im.custom),
        ("macroTable", &im.macro_table),
        ("magnetic", &im.magnetic),
        ("dks", &im.dks),
    ] {
        assert_eq!(raw[key], hex(bytes), "{key}");
    }
    assert_eq!(
        raw["macroReads"],
        expected_macro_reads(&im.macro_table, &im.macro_data)
    );
    assert_eq!(raw["fnKeys"], serde_json::Value::Null);
}

/// The reads of the macro area after the table, in the site's order, as the backup
/// lists them: each macro's header at `p`, then its actions at `p + 4`.
fn expected_macro_reads(table: &[u8], data: &[u8]) -> serde_json::Value {
    let mut reads = Vec::new();
    for p in table.chunks_exact(4) {
        let p = u32::from_le_bytes([p[0], p[1], p[2], p[3]]) as usize;
        if p == 0 {
            continue;
        }
        let header = &data[p - 400..p - 400 + 4];
        reads.push(serde_json::json!({ "addr": p, "bytes": hex(header) }));
        let len = usize::from(u16::from_le_bytes([header[0], header[1]]) / 2) * 4;
        if len > 0 {
            let actions = &data[p - 400 + 4..p - 400 + 4 + len];
            reads.push(serde_json::json!({ "addr": p + 4, "bytes": hex(actions) }));
        }
    }
    serde_json::Value::Array(reads)
}

fn read_json(path: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn backup_raw_holds_the_bytes_as_read() {
    // bytes the typed blocks do not keep: the LED constants as this keyboard returns
    // them (00, not FF / AA 55), a custom-LED index byte that is not the position, and
    // a macro area not laid out the site's way (macro 7 at 400, a gap, macro 0 at 452)
    let mut im = images();
    im.led[4] = 0;
    im.led[13] = 0x11;
    im.led[14] = 0;
    im.led[15] = 0;
    im.custom[4 * 5] = 0x77;
    let mut data = vec![0u8; 64];
    data[..16].copy_from_slice(&im.macro_data[12..28]); // macro 7, 3 actions
    data[52..64].copy_from_slice(&im.macro_data[..12]); // macro 0, 2 actions
    im.macro_data = data;
    im.macro_table[..4].copy_from_slice(&452u32.to_le_bytes());
    im.macro_table[28..32].copy_from_slice(&400u32.to_le_bytes());
    let snap = read_mock_snapshot(&im);

    let dir = tempfile::tempdir().unwrap();
    let path = save_backup(&snap, dir.path()).unwrap();
    let json = read_json(&path);
    let raw = &json["raw"];
    assert_eq!(raw["ledEffect"], hex(&im.led));
    assert_eq!(raw["customLeds"], hex(&im.custom));
    assert_eq!(raw["macroTable"], hex(&im.macro_table));
    assert_eq!(
        raw["macroReads"],
        expected_macro_reads(&im.macro_table, &im.macro_data)
    );

    // it loads (write constants and layout are not checked), and saving what was
    // loaded records the same bytes as read
    let loaded = load_backup(&path).unwrap();
    assert_eq!(loaded, snap);
    let again = save_backup(&loaded, dir.path()).unwrap();
    assert_eq!(read_json(&again)["raw"], json["raw"]);
}

#[test]
fn backup_rejects_raw_that_disagrees_with_the_snapshot() {
    let snap = read_mock_snapshot(&images());
    let dir = tempfile::tempdir().unwrap();
    let path = save_backup(&snap, dir.path()).unwrap();
    let original = read_json(&path);
    let load_with = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut json = original.clone();
        edit(&mut json);
        std::fs::write(&path, json.to_string()).unwrap();
        load_backup(&path)
    };
    let set_byte = |json: &mut serde_json::Value, block: &str, at: usize| {
        let mut s = json["raw"][block].as_str().unwrap().to_owned();
        s.replace_range(2 * at..2 * at + 2, "3c");
        json["raw"][block] = s.into();
    };

    // a byte the snapshot holds: refused
    for (block, at) in [
        ("deviceInfo", 9),
        ("gameMode", 5),
        ("keys", 2),
        ("ledEffect", 9),
        ("customLeds", 1),
        ("macroTable", 0),
        ("magnetic", 2),
        ("dks", 5),
    ] {
        let err = load_with(&|j| set_byte(j, block, at)).unwrap_err();
        assert!(
            matches!(&err, Error::Backup(m) if m.contains("incohérente")),
            "{block}: {err:?}"
        );
    }
    // the snapshot edited by hand: refused too
    let err = load_with(&|j| j["snapshot"]["ledEffect"]["brightness"] = 1.into()).unwrap_err();
    assert!(matches!(err, Error::Backup(_)), "{err:?}");
    // a macro read the table points to, missing from the file: refused
    let err = load_with(&|j| {
        j["raw"]["macroReads"].as_array_mut().unwrap().remove(0);
    })
    .unwrap_err();
    assert!(matches!(err, Error::Backup(_)), "{err:?}");

    // the write constants: accepted (LED bytes 4, 13-15; custom-LED index byte)
    for (block, at) in [
        ("ledEffect", 4),
        ("ledEffect", 13),
        ("ledEffect", 14),
        ("ledEffect", 15),
        ("customLeds", 4),
    ] {
        assert_eq!(
            load_with(&|j| set_byte(j, block, at)).unwrap(),
            snap,
            "{block} {at}"
        );
    }
}

#[test]
fn backup_of_a_snapshot_edited_after_reading() {
    let snap = read_mock_snapshot(&images());
    let dir = tempfile::tempdir().unwrap();
    // its bytes as read no longer describe it: refused, nothing written
    let mut edited = snap.clone();
    edited.led_effect.brightness = 1;
    assert!(matches!(
        save_backup(&edited, dir.path()).unwrap_err(),
        Error::Backup(_)
    ));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);

    // without bytes as read (built by the app, or from its JSON): `raw` is null
    let built: Snapshot = serde_json::from_str(&serde_json::to_string(&edited).unwrap()).unwrap();
    let path = save_backup(&built, dir.path()).unwrap();
    assert!(read_json(&path)["raw"].is_null());
    assert_eq!(load_backup(&path).unwrap(), edited);
}

#[test]
fn backup_reads_first_format_1_files() {
    // written before the bytes as read were kept: `macroData` (the macro area from 400,
    // laid out the site's way) instead of `macroReads`
    let im = images();
    let snap = read_mock_snapshot(&im);
    let dir = tempfile::tempdir().unwrap();
    let path = save_backup(&snap, dir.path()).unwrap();
    let mut json = read_json(&path);
    let raw = json["raw"].as_object_mut().unwrap();
    raw.remove("macroReads");
    raw.insert("macroData".into(), hex(&im.macro_data).into());
    std::fs::write(&path, json.to_string()).unwrap();
    assert_eq!(load_backup(&path).unwrap(), snap);
}

#[test]
fn backup_never_overwrites() {
    let snap = read_mock_snapshot(&images());
    let dir = tempfile::tempdir().unwrap();
    // within the same second: same timestamp, distinct files
    let paths: Vec<_> = (0..3)
        .map(|_| save_backup(&snap, dir.path()).unwrap())
        .collect();
    assert!(paths[0] != paths[1] && paths[1] != paths[2] && paths[0] != paths[2]);
    for p in &paths {
        assert_eq!(load_backup(p).unwrap(), snap);
    }
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        3,
        "no temp file left"
    );
}

#[test]
fn backup_rejects_unknown_format() {
    let snap = read_mock_snapshot(&images());
    let dir = tempfile::tempdir().unwrap();
    let path = save_backup(&snap, dir.path()).unwrap();
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

    json["format"] = 2.into();
    std::fs::write(&path, json.to_string()).unwrap();
    let err = load_backup(&path).unwrap_err();
    assert!(
        matches!(&err, Error::Backup(m) if m.contains("format 2")),
        "{err:?}"
    );

    json.as_object_mut().unwrap().remove("format");
    std::fs::write(&path, json.to_string()).unwrap();
    assert!(matches!(load_backup(&path).unwrap_err(), Error::Backup(_)));

    assert!(matches!(
        load_backup(&dir.path().join("absent.json")).unwrap_err(),
        Error::Backup(_)
    ));
}

#[test]
fn write_snapshot_refuses_unrepresentable_before_sending() {
    let im = images();
    let mut link = MockLink::new(32);
    serve(&mut link, 0x10, 0, &im.info);
    let mut kb = Keyboard::connect(link).unwrap();
    let mut snap = Snapshot {
        info: kb.info.clone(),
        game_mode: GameMode::decode(&im.game_mode).unwrap(),
        keys: KeyMap::decode(&im.keys).unwrap(),
        fn_keys: None,
        led_effect: LedEffect::decode(&im.led).unwrap(),
        custom_leds: CustomLeds::decode(&im.custom).unwrap(),
        macros: Macros::default(),
        magnetic: MagneticMap::decode(&im.magnetic, 0).unwrap(),
        dks: DksTable::decode(&im.dks).unwrap(),
        raw: None,
    };
    // late in the write order; precision 0 stores RT in 0.01 mm steps
    snap.magnetic.0[0].press_rt_mm = 0.015;
    snap.magnetic.0[0].release_rt_mm = 0.015;
    let sent_before = kb.link().written.len();
    assert!(is_validation(&kb.write_snapshot(&snap).unwrap_err()));
    assert!(is_validation(
        &kb.write_magnetic(&snap.magnetic).unwrap_err()
    ));
    assert_eq!(kb.link().written.len(), sent_before, "nothing may be sent");
}

#[test]
fn read_raw_reads_only_the_sites_block_reads() {
    let im = images();
    let mut link = MockLink::new(32);
    serve(&mut link, 0x10, 0, &im.info);
    serve(&mut link, 0x14, 0, &im.custom);
    serve(&mut link, 0x15, 412, &im.macro_data[12..16]);
    let mut kb = Keyboard::connect(link).unwrap();
    assert_eq!(
        kb.read_raw(Cmd::GetCustomLedData, 0, 512).unwrap(),
        im.custom
    );
    assert_eq!(
        kb.read_raw(Cmd::GetMacro, 412, 4).unwrap(),
        im.macro_data[12..16]
    );

    // writes, the Fn layer and anything else the site does not read: nothing sent
    let before = kb.link().written.len();
    for cmd in [
        Cmd::SetKey,
        Cmd::GetFnKey,
        Cmd::GetDefaultFnKeyMatrix,
        Cmd::GetMagneticAxisStatus,
    ] {
        assert!(
            matches!(kb.read_raw(cmd, 0, 4).unwrap_err(), Error::Invalid(_)),
            "{cmd:?}"
        );
    }
    assert_eq!(kb.link().written.len(), before);
}

#[test]
fn timeouts_follow_frame_version() {
    let mut link = MockLink::new(32);
    serve(&mut link, 0x10, 0, &info_image(0, 1));
    let kb = Keyboard::connect(link).unwrap();
    assert_eq!(kb.timeout_ms(Cmd::GetKey), 500);
    assert_eq!(kb.timeout_ms(Cmd::SetKey), 1000);
    assert_eq!(kb.timeout_ms(Cmd::SetCustomLedData), 2000);
    assert_eq!(kb.timeout_ms(Cmd::SetProfile), 2000);

    let mut link = MockLink::new(32);
    serve(&mut link, 0x10, 0, &info_image(1, 1));
    let kb = Keyboard::connect(link).unwrap();
    assert_eq!(kb.timeout_ms(Cmd::GetKey), 2000);
    assert_eq!(kb.timeout_ms(Cmd::SetKey), 2000);
    assert_eq!(kb.timeout_ms(Cmd::GetDeviceInfo), 500);
}

#[test]
fn set_profile_checks_echo() {
    let mut link = MockLink::new(32);
    serve(&mut link, 0x10, 0, &info_image(0, 3));
    let mut ok = reply(0x90, 0, 0, &[]);
    ok[2] = 2;
    link.script(0x90, 0, vec![ok]);
    let mut wrong = reply(0x90, 0, 0, &[]);
    wrong[2] = 1;
    link.script(0x90, 0, vec![wrong]);
    let mut kb = Keyboard::connect(link).unwrap();

    assert!(kb.set_profile(2).unwrap());
    let sent = kb.link().written.last().unwrap().clone();
    assert_eq!(sent[..8], [0xAA, 0x90, 2, 0, 0, 0, 0, 0]);
    assert!(!kb.set_profile(3).unwrap());

    let before = kb.link().written.len();
    assert!(is_validation(&kb.set_profile(0).unwrap_err()));
    assert!(is_validation(&kb.set_profile(4).unwrap_err()));
    assert_eq!(kb.link().written.len(), before);
}

#[test]
fn set_profile_refused_without_hardware_profiles() {
    let mut link = MockLink::new(32);
    serve(&mut link, 0x10, 0, &info_image(0, 1));
    let mut kb = Keyboard::connect(link).unwrap();
    let before = kb.link().written.len();
    assert!(is_validation(&kb.set_profile(1).unwrap_err()));
    assert_eq!(kb.link().written.len(), before);
}
