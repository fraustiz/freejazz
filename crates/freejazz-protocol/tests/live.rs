//! Live LED stream (0x32) and key depth (0x68) against a scripted keyboard.

use freejazz_protocol::Keyboard;
use freejazz_protocol::blocks::{DeviceInfo, LedEffect, Rgb};
use freejazz_protocol::cmd::Cmd;
use freejazz_protocol::error::Error;
use freejazz_protocol::live::{
    FRAME_SLOTS, LIVE_AMPLITUDE, depth_to_mm, frame_bytes, layout_order,
};
use freejazz_protocol::transport::MockLink;

/// A `GET_DEVICE_INFO` image: frame version `fv`, RT precision `rt`.
fn info_image(fv: u8, rt: u8) -> Vec<u8> {
    let mut b = vec![0u8; 48];
    b[4..6].copy_from_slice(&0x0C45u16.to_le_bytes());
    b[6..8].copy_from_slice(&0x8043u16.to_le_bytes());
    b[8] = 0x28;
    b[9] = 1;
    b[29] = rt;
    b[30] = fv;
    b
}

fn reply(cmd: u8, addr: u16, data: &[u8], report_len: usize) -> Vec<u8> {
    let mut r = vec![0u8; report_len];
    r[0] = 0x55;
    r[1] = cmd;
    r[3..5].copy_from_slice(&addr.to_le_bytes());
    let n = data.len().min(report_len - 8);
    r[8..8 + n].copy_from_slice(&data[..n]);
    r
}

/// A keyboard on a mock link of `report_len`, connected (its info read served).
fn keyboard(report_len: usize, fv: u8, rt: u8) -> Keyboard<MockLink> {
    let mut link = MockLink::new(report_len);
    let info = info_image(fv, rt);
    let c = report_len - 8;
    for t in 0..48usize.div_ceil(c) {
        let a = t * c;
        let end = (a + c).min(48);
        link.script(
            0x10,
            a as u16,
            vec![reply(0x10, a as u16, &info[a..end], report_len)],
        );
    }
    Keyboard::connect(link).unwrap()
}

fn clear(kb: &mut Keyboard<MockLink>) {
    kb.link_mut().written.clear();
}

/// A frame where slot `i` is `(i, 255 - i, i / 2)`.
fn test_frame() -> [Rgb; FRAME_SLOTS] {
    std::array::from_fn(|i| Rgb {
        r: i as u8,
        g: 255 - i as u8,
        b: (i / 2) as u8,
    })
}

#[test]
fn layout_order_is_layout_json() {
    let order = layout_order();
    assert_eq!(order.len(), 101);
    assert_eq!(
        &order[..14],
        &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 106]
    );
    assert_eq!(*order.last().unwrap(), 94);
    let mut sorted = order.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 101, "no index twice");
    assert!(order.iter().all(|&i| usize::from(i) < FRAME_SLOTS));
}

#[test]
fn frame_bytes_are_layout_keys_only() {
    let bytes = frame_bytes(&test_frame());
    assert_eq!(bytes.len(), 404);
    for (k, &i) in layout_order().iter().enumerate() {
        let e = &bytes[k * 4..k * 4 + 4];
        assert_eq!(e, &[i, i, 255 - i, i / 2]);
    }
}

/// The site's music stream: chunk `c = reportCount - 8` (`xZ`: `t = a - 8`), one
/// transfer per chunk, extra header `[amp, 0]` except `[amp]` on the last chunk.
fn check_chunks(report_len: usize) {
    let mut kb = keyboard(report_len, 0, 0);
    let c = report_len - 8;
    let n = 404usize.div_ceil(c);
    for t in 0..n {
        let a = (t * c) as u16;
        kb.link_mut()
            .script(0x32, a, vec![reply(0x32, a, &[], report_len)]);
    }
    clear(&mut kb);
    let frame = test_frame();
    kb.send_frame(&frame, LIVE_AMPLITUDE).unwrap();
    let bytes = frame_bytes(&frame);
    let w = &kb.link().written;
    assert_eq!(w.len(), n, "{report_len}-byte reports: {n} chunks");
    for (t, r) in w.iter().enumerate() {
        let last = t == n - 1;
        let len = c.min(404 - t * c);
        assert_eq!(r.len(), report_len);
        assert_eq!(r[0], 0xAA);
        assert_eq!(r[1], 0x32);
        assert_eq!(usize::from(r[2]), len);
        assert_eq!(usize::from(u16::from_le_bytes([r[3], r[4]])), t * c);
        assert_eq!(r[5], 0, "amplitude");
        assert_eq!(
            r[6],
            u8::from(last),
            "byte 6: last-chunk flag only on the last"
        );
        assert_eq!(r[7], 0);
        assert_eq!(&r[8..8 + len], &bytes[t * c..t * c + len]);
        assert!(r[8 + len..].iter().all(|&b| b == 0), "zero padded");
    }
}

#[test]
fn send_frame_chunks() {
    check_chunks(64); // the tested unit: 8 chunks, the last of 12 bytes
    check_chunks(32); // the site's default: 17 chunks, the last of 20 bytes
}

#[test]
fn send_frame_amplitude_in_byte_5() {
    let mut kb = keyboard(64, 0, 0);
    for t in 0..8u16 {
        kb.link_mut()
            .script(0x32, t * 56, vec![reply(0x32, t * 56, &[], 64)]);
    }
    clear(&mut kb);
    kb.send_frame(&test_frame(), 7).unwrap();
    let w = &kb.link().written;
    assert!(w.iter().all(|r| r[5] == 7));
    assert_eq!(w[7][6], 1);
    assert!(w[..7].iter().all(|r| r[6] == 0));
}

#[test]
fn send_frame_without_reply_retries_then_errors() {
    let mut kb = keyboard(64, 0, 0);
    clear(&mut kb);
    let err = kb.send_frame(&test_frame(), 0).unwrap_err();
    assert!(matches!(err, Error::Timeout));
    assert_eq!(
        kb.link().written.len(),
        4,
        "first chunk sent once + 3 retries"
    );
}

#[test]
fn live_timeouts() {
    let kb = keyboard(64, 0, 0);
    assert_eq!(kb.timeout_ms(Cmd::GetLedData), 50);
    assert_eq!(kb.timeout_ms(Cmd::GetMagneticAxisStatus), 2000);
    let kb = keyboard(64, 1, 0);
    assert_eq!(kb.timeout_ms(Cmd::GetLedData), 2000);
}

#[test]
fn begin_live_sends_nothing() {
    let mut kb = keyboard(64, 0, 0);
    clear(&mut kb);
    kb.begin_live().unwrap();
    assert!(kb.link().written.is_empty());
}

#[test]
fn end_live_resends_stored_effect() {
    let stored =
        LedEffect::decode(&[9, 0xFF, 0xB0, 0xB0, 0, 0, 0, 0, 0, 6, 3, 0, 0, 0, 0, 0]).unwrap();
    let mut kb = keyboard(64, 0, 0);
    kb.link_mut().script(0x23, 0, vec![reply(0x23, 0, &[], 64)]);
    clear(&mut kb);
    kb.end_live(&stored).unwrap();
    let w = &kb.link().written;
    assert_eq!(w.len(), 1);
    assert_eq!(&w[0][..8], &[0xAA, 0x23, 16, 0, 0, 0, 1, 0]);
    assert_eq!(&w[0][8..24], stored.encode().as_slice());
}

/// One 0x68 reply entry: `[state, adc lo, adc hi, travel lo, travel hi, 0, 0, 0]`.
fn status_entry(state: u8, adc: u16, travel: u16) -> [u8; 8] {
    let (a, t) = (adc.to_le_bytes(), travel.to_le_bytes());
    [state, a[0], a[1], t[0], t[1], 0, 0, 0]
}

#[test]
fn read_key_depths_batches_of_seven() {
    let mut kb = keyboard(64, 0, 0);
    let order = layout_order();
    let batches: Vec<&[u8]> = order.chunks(7).collect();
    assert_eq!(batches.len(), 15);
    for (b, keys) in batches.iter().enumerate() {
        let data: Vec<u8> = keys
            .iter()
            .flat_map(|&i| status_entry(2, 1000 + u16::from(i), 3 * u16::from(i)))
            .collect();
        let a = (b * 56) as u16;
        kb.link_mut()
            .script(0x68, a, vec![reply(0x68, a, &data, 64)]);
    }
    clear(&mut kb);
    let depths = kb.read_key_depths().unwrap();

    let w = &kb.link().written;
    assert_eq!(w.len(), 15, "one 56-byte chunk per batch of 7 keys");
    for (b, (r, keys)) in w.iter().zip(&batches).enumerate() {
        assert_eq!(r[1], 0x68);
        assert_eq!(usize::from(r[2]), keys.len() * 8);
        assert_eq!(usize::from(u16::from_le_bytes([r[3], r[4]])), b * 56);
        assert_eq!(r[6], u8::from(b == 14), "last flag only on the last batch");
        for (k, &i) in keys.iter().enumerate() {
            assert_eq!(&r[8 + k * 8..16 + k * 8], &[i, 0, 0, 0, 0, 0, 0, 0]);
        }
    }
    for i in 0..FRAME_SLOTS as u8 {
        let want = if order.contains(&i) {
            3 * u16::from(i)
        } else {
            0
        };
        assert_eq!(depths[usize::from(i)], want, "slot {i}");
    }
}

#[test]
fn read_key_depths_checks_the_address() {
    let mut kb = keyboard(64, 0, 0);
    // a reply for the wrong batch address is ignored: the first chunk times out
    kb.link_mut()
        .script(0x68, 0, vec![reply(0x68, 56, &[1; 56], 64)]);
    clear(&mut kb);
    assert!(matches!(kb.read_key_depths(), Err(Error::Timeout)));
}

#[test]
fn depth_in_mm_follows_rt_precision() {
    let mut info = DeviceInfo::decode(&info_image(0, 0)).unwrap();
    assert_eq!(depth_to_mm(150, &info), 1.5);
    info.rt_precision = 2;
    assert_eq!(depth_to_mm(1500, &info), 1.5);
}

#[test]
fn factory_reset_writes_one_report_with_the_type_byte() {
    use freejazz_protocol::keyboard::ResetKind;
    let mut kb = keyboard(64, 0, 0);
    kb.factory_reset(ResetKind::Macros).unwrap();
    let last = kb.link().written.last().unwrap();
    assert_eq!(&last[..4], &[0xAA, 0x0F, 4, 0]);
    assert_eq!(last.len(), 64);
}

#[test]
fn key_test_starts_with_0x66_streams_0xfb_and_stops_with_0x67() {
    let mut kb = keyboard(64, 0, 0);
    clear(&mut kb);
    let mut fb = reply(0xFB, 0, &[], 64);
    fb[2] = 7;
    fb[10..12].copy_from_slice(&150u16.to_le_bytes());
    kb.link_mut()
        .script(0x66, 0, vec![reply(0x66, 0, &[], 64), fb]);
    kb.start_key_test().unwrap();
    let r = kb.read_axis_report(10).unwrap().unwrap();
    assert_eq!((r.index, r.key_stroke), (7, 150));
    assert_eq!(kb.read_axis_report(10).unwrap(), None);
    kb.stop_key_test().unwrap();
    let sent: Vec<_> = kb.link_mut().written.iter().map(|w| (w[0], w[1])).collect();
    assert_eq!(sent, [(0xAA, 0x66), (0xAA, 0x67)]);
}

#[test]
fn calibration_uses_0x64_and_0x65_and_never_a_denied_command() {
    let mut kb = keyboard(64, 0, 0);
    clear(&mut kb);
    kb.link_mut().script(0x64, 0, vec![reply(0x64, 0, &[], 64)]);
    kb.start_calibration().unwrap();
    kb.stop_calibration().unwrap();
    let sent: Vec<_> = kb.link_mut().written.iter().map(|w| w[1]).collect();
    assert_eq!(sent, [0x64, 0x65]);
}
