use freejazz_protocol::cmd::Cmd;
use freejazz_protocol::error::Error;
use freejazz_protocol::packet::{encode_request, parse_response};
use freejazz_protocol::transfer::{TransferOpts, transfer};
use freejazz_protocol::transport::{
    MockLink, ReportItem, ReportKind, is_site_usage_page, probe, report_items, site_report_len,
};

fn reply(cmd: u8, addr: u16, fill: u8) -> Vec<u8> {
    let mut r = vec![0u8; 32];
    r[0] = 0x55;
    r[1] = cmd;
    r[3..5].copy_from_slice(&addr.to_le_bytes());
    for b in &mut r[8..] {
        *b = fill;
    }
    r
}

#[test]
fn encode_request_layout() {
    let p = encode_request(Cmd::GetDeviceInfo, 24, 0x0118, &[], true, &[], 32);
    assert_eq!(p.len(), 32);
    assert_eq!(&p[..8], &[0xAA, 0x10, 0x18, 0x18, 0x01, 0x00, 0x01, 0x00]);
    assert!(p[8..].iter().all(|&b| b == 0));
}

#[test]
fn encode_request_extra_two_bytes_keeps_byte6() {
    let p = encode_request(Cmd::GetKey, 24, 0, &[7, 0], true, &[], 32);
    assert_eq!(p[5], 7);
    assert_eq!(p[6], 0);
}

#[test]
fn parse_rejects_bad_magic() {
    assert!(parse_response(&[0xAA; 32]).is_none());
    assert!(parse_response(&[0x55, 1]).is_none());
    let r = parse_response(&reply(0x10, 0x0030, 9)).unwrap();
    assert_eq!((r.cmd, r.addr, r.data.len()), (0x10, 0x30, 24));
}

#[test]
fn transfer_splits_512_into_22_chunks() {
    let mut link = MockLink::new(32);
    for t in 0..22u16 {
        link.script(0x12, t * 24, vec![reply(0x12, t * 24, t as u8)]);
    }
    let out = transfer(&mut link, &TransferOpts::read(Cmd::GetKey, 512)).unwrap();
    assert_eq!(out.len(), 512);
    assert_eq!(out[24], 1);
    assert_eq!(link.written.len(), 22);
    for (t, w) in link.written.iter().enumerate() {
        assert_eq!(w.len(), 32);
        assert_eq!(u16::from_le_bytes([w[3], w[4]]), t as u16 * 24);
        assert_eq!(w[2], if t == 21 { 8 } else { 24 });
        assert_eq!(w[6], (t == 21) as u8);
    }
}

#[test]
fn ignores_responses_for_other_commands() {
    let mut link = MockLink::new(32);
    link.script(0x12, 0, vec![reply(0x13, 0, 1), reply(0x12, 0, 2)]);
    let out = transfer(&mut link, &TransferOpts::read(Cmd::GetKey, 4)).unwrap();
    assert_eq!(out, vec![2; 4]);
}

#[test]
fn transfer_retries_then_errors() {
    let mut link = MockLink::new(32);
    let err = transfer(&mut link, &TransferOpts::read(Cmd::GetKey, 24)).unwrap_err();
    assert!(matches!(err, Error::Timeout));
    assert_eq!(link.written.len(), 4);
}

#[test]
fn transfer_mid_write_timeout_returns_err() {
    let mut link = MockLink::new(32);
    for t in 0..22u16 {
        if t != 5 {
            link.script(0x22, t * 24, vec![reply(0x22, t * 24, 0)]);
        }
    }
    let opts = TransferOpts::write(Cmd::SetKey, vec![1; 512]);
    assert!(transfer(&mut link, &opts).is_err());
    // chunks 0..=4 once, chunk 5 four times, nothing after
    assert_eq!(link.written.len(), 5 + 4);
}

#[test]
fn a_short_read_response_is_an_error() {
    let mut link = MockLink::new(32);
    // chunk 0 answered with 12 data bytes instead of 24
    let mut short = reply(0x12, 0, 7);
    short.truncate(8 + 12);
    link.script(0x12, 0, vec![short]);
    link.script(0x12, 24, vec![reply(0x12, 24, 1)]);
    let err = transfer(&mut link, &TransferOpts::read(Cmd::GetKey, 48)).unwrap_err();
    assert!(
        matches!(err, Error::Invalid(ref m) if m.contains("short")),
        "{err:?}"
    );
    // nothing more is asked once a chunk is short
    assert_eq!(link.written.len(), 1);
    // a write's acknowledgement carries no data: accepted
    let mut link = MockLink::new(32);
    let mut ack = reply(0x22, 0, 0);
    ack.truncate(8);
    link.script(0x22, 0, vec![ack]);
    assert!(transfer(&mut link, &TransferOpts::write(Cmd::SetKey, vec![1; 24])).is_ok());
}

#[test]
fn denied_command_is_refused() {
    let mut link = MockLink::new(32);
    let err = transfer(&mut link, &TransferOpts::read(Cmd::OtaDeviceEnterBoot, 24)).unwrap_err();
    assert!(matches!(err, Error::Denied(_)));
    assert!(link.written.is_empty());
    assert!(Cmd::SetFlashDownload.is_denied() && Cmd::SetLedData.is_denied());
    assert!(!Cmd::GetKey.is_denied());
}

#[test]
fn custom_header_cannot_smuggle_denied_command() {
    let mut link = MockLink::new(32);
    let mut opts = TransferOpts::read(Cmd::GetKey, 24);
    opts.custom_header = Some(vec![0xAA, 0x80, 0x00]);
    let err = transfer(&mut link, &opts).unwrap_err();
    assert!(matches!(err, Error::Denied(0x80)));
    assert!(link.written.is_empty());
    // the link itself refuses too
    use freejazz_protocol::transport::HidLink;
    assert!(matches!(
        link.write(&[0xAA, 0x42, 0]),
        Err(Error::Denied(0x42))
    ));
    assert!(link.written.is_empty());
}

#[test]
fn deny_list_is_exact() {
    for c in (0x80..=0x86u8).chain([0x4F, 0x50, 0x51, 0x40, 0x41, 0x42]) {
        assert!(Cmd::is_denied_code(c), "0x{c:02X} must be denied");
    }
    for c in [0x10u8, 0x23, 0x32, 0x87, 0x43] {
        assert!(!Cmd::is_denied_code(c), "0x{c:02X} must be allowed");
    }
}

#[test]
fn late_duplicate_reply_does_not_leak_into_next_chunk() {
    let mut link = MockLink::new(32);
    // chunk 0: attempt 1 silent, attempt 2 answered, plus a late duplicate left queued
    link.script(0x12, 0, vec![]);
    link.script(0x12, 0, vec![reply(0x12, 0, 0), reply(0x12, 0, 0xEE)]);
    link.script(0x12, 24, vec![reply(0x12, 24, 1)]);
    let out = transfer(&mut link, &TransferOpts::read(Cmd::GetKey, 48)).unwrap();
    assert_eq!(out[..24], [0u8; 24]);
    assert_eq!(out[24..], [1u8; 24]);
}

// ---------- interface choice (protocol.md 1) ----------

/// Descriptors Windows reconstructs for the tested unit (2026-10-24): MI_02 (usage page
/// 0xFF68, 64-byte input and output reports) and MI_03 (0xFF67, 4096-byte output).
const MI_02: [u8; 34] = [
    0x06, 0x68, 0xFF, 0x09, 0x61, 0xA1, 0x01, 0x09, 0x62, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08,
    0x95, 0x40, 0x81, 0x02, 0x09, 0x63, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x40, 0x91,
    0x02, 0xC0,
];
const MI_03: [u8; 35] = [
    0x06, 0x67, 0xFF, 0x09, 0x61, 0xA1, 0x01, 0x09, 0x63, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08,
    0x96, 0x00, 0x10, 0x91, 0x02, 0x09, 0x62, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08, 0x95, 0x40,
    0xB1, 0x02, 0xC0,
];

#[test]
fn report_len_is_the_sites_output_report_count() {
    assert_eq!(site_report_len(&MI_02), 64);
    assert_eq!(site_report_len(&MI_03), 4096);
    // no output item: the site's default
    let keyboard_in_only = [
        0x05, 0x01, 0x09, 0x06, 0xA1, 0x01, 0x75, 0x08, 0x95, 0x08, 0x81, 0x02, 0xC0,
    ];
    assert_eq!(site_report_len(&keyboard_in_only), 32);

    let items = report_items(&MI_03);
    assert_eq!(
        items,
        vec![
            ReportItem {
                kind: ReportKind::Output,
                report_id: 0,
                size: 8,
                count: 4096
            },
            ReportItem {
                kind: ReportKind::Feature,
                report_id: 0,
                size: 8,
                count: 64
            },
        ]
    );
}

#[test]
fn only_the_sites_usage_pages_are_opened() {
    for page in [0xFF68, 0xFF80, 0xFF60, 0xFF00, 0xFF01, 0xFF1B] {
        assert!(is_site_usage_page(page), "{page:04X}");
    }
    for page in [0xFF67, 0xFF02, 0x0001, 0x000C] {
        assert!(!is_site_usage_page(page), "{page:04X}");
    }
}

#[test]
fn probe_sends_the_sites_device_info_request() {
    // 64-byte reports: GET_DEVICE_INFO (48 bytes) is one chunk with the last flag
    let mut link = MockLink::new(64);
    let mut answer = vec![0u8; 64];
    answer[..2].copy_from_slice(&[0x55, 0x10]);
    link.script(0x10, 0, vec![answer]);
    assert!(probe(&mut link));
    assert_eq!(link.written.len(), 1);
    assert_eq!(link.written[0][..8], [0xAA, 0x10, 48, 0, 0, 0, 1, 0]);
    assert_eq!(link.written[0].len(), 64);

    // no answer: one request, no retry
    let mut silent = MockLink::new(64);
    assert!(!probe(&mut silent));
    assert_eq!(silent.written.len(), 1);
}
