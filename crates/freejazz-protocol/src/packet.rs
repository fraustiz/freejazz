use crate::cmd::Cmd;
use crate::error::{Error, Result};

/// Builds a request report (without the report-id byte), zero padded to `report_len`.
///
/// Layout: `[0]=0xAA, [1]=cmd, [2]=chunk_len, [3..5]=addr LE`, `extra` (at most 3
/// bytes) from `[5]`, `[6]=last` only when `extra.len() < 2`, data from `[8]`.
pub fn encode_request(
    cmd: Cmd,
    chunk_len: u8,
    addr: u16,
    extra: &[u8],
    last: bool,
    data: &[u8],
    report_len: usize,
) -> Vec<u8> {
    let mut p = vec![0u8; report_len.max(8)];
    p[0] = 0xAA;
    p[1] = cmd as u8;
    p[2] = chunk_len;
    p[3..5].copy_from_slice(&addr.to_le_bytes());
    let n = extra.len().min(3);
    p[5..5 + n].copy_from_slice(&extra[..n]);
    if extra.len() < 2 {
        p[6] = last as u8;
    }
    let m = data.len().min(p.len() - 8);
    p[8..8 + m].copy_from_slice(&data[..m]);
    p
}

/// Builds a "custom header" request: header verbatim, data right after it.
pub fn encode_custom(header: &[u8], data: &[u8], report_len: usize) -> Vec<u8> {
    let mut p = vec![0u8; report_len.max(header.len())];
    p[..header.len()].copy_from_slice(header);
    let m = data.len().min(p.len() - header.len());
    p[header.len()..header.len() + m].copy_from_slice(&data[..m]);
    p
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub cmd: u8,
    pub len_or_type: u8,
    pub addr: u16,
    pub data: Vec<u8>,
}

/// Parses a report; `None` unless it starts with 0x55 and holds a full header.
pub fn parse_response(buf: &[u8]) -> Option<Response> {
    if buf.len() < 8 || buf[0] != 0x55 {
        return None;
    }
    Some(Response {
        cmd: buf[1],
        len_or_type: buf[2],
        addr: u16::from_le_bytes([buf[3], buf[4]]),
        data: buf[8..].to_vec(),
    })
}

/// Deny-list guard for any outgoing report: a request (`0xAA`) whose command byte
/// is denied is refused. Every `HidLink::write` implementation must call this.
pub fn check_report(report: &[u8]) -> Result<()> {
    if report.len() >= 2 && report[0] == 0xAA && Cmd::is_denied_code(report[1]) {
        return Err(Error::Denied(report[1]));
    }
    Ok(())
}
