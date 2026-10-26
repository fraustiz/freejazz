use std::time::{Duration, Instant};

use crate::cmd::Cmd;
use crate::error::{Error, Result};
use crate::packet::{Response, check_report, encode_custom, encode_request, parse_response};
use crate::transport::HidLink;

#[derive(Debug, Clone)]
pub struct TransferOpts {
    pub cmd: Cmd,
    pub total: usize,
    pub addr_start: u16,
    /// Data to write; `None` for a read.
    pub data: Option<Vec<u8>>,
    pub timeout_ms: u32,
    /// Extra attempts after the first send.
    pub retries: u8,
    pub need_last_flag: bool,
    pub check_addr: bool,
    pub extra: Vec<u8>,
    pub custom_header: Option<Vec<u8>>,
    pub skip_response: bool,
}

impl TransferOpts {
    fn base(cmd: Cmd, total: usize, data: Option<Vec<u8>>) -> Self {
        Self {
            cmd,
            total,
            addr_start: 0,
            data,
            timeout_ms: 500,
            retries: 3,
            need_last_flag: true,
            check_addr: false,
            extra: Vec::new(),
            custom_header: None,
            skip_response: false,
        }
    }

    pub fn read(cmd: Cmd, total: usize) -> Self {
        Self::base(cmd, total, None)
    }

    pub fn write(cmd: Cmd, data: Vec<u8>) -> Self {
        Self::base(cmd, data.len(), Some(data))
    }
}

/// Chunked request/response transfer. Returns the concatenated response data
/// truncated to `total`. Stops at the first chunk that fails.
pub fn transfer(link: &mut dyn HidLink, opts: &TransferOpts) -> Result<Vec<u8>> {
    if opts.cmd.is_denied() {
        return Err(Error::Denied(opts.cmd as u8));
    }
    if let Some(d) = &opts.data
        && d.len() < opts.total
    {
        return Err(Error::Invalid("data shorter than total".into()));
    }
    let report_len = link.report_len();
    if report_len <= 8 {
        return Err(Error::Invalid(format!("report length {report_len} too small")));
    }
    let chunk = report_len - 8;
    let n = opts.total.div_ceil(chunk);
    let mut out = Vec::with_capacity(opts.total);

    for t in 0..n {
        let offset = t * chunk;
        let len = chunk.min(opts.total - offset);
        let addr = u16::try_from(usize::from(opts.addr_start) + offset)
            .map_err(|_| Error::Invalid("address overflow".into()))?;
        let last = opts.need_last_flag && t == n - 1;
        let payload: &[u8] = match &opts.data {
            Some(d) => &d[offset..offset + len],
            None => &[],
        };
        let report = match &opts.custom_header {
            Some(h) => encode_custom(h, payload, report_len),
            None => encode_request(opts.cmd, len as u8, addr, &opts.extra, last, payload, report_len),
        };
        check_report(&report)?;
        if opts.skip_response {
            link.write(&report)?;
            continue;
        }
        let data = send_chunk(link, &report, opts, addr)?;
        if opts.data.is_none() && data.len() < len {
            // a short read would shift every later chunk to the wrong offset
            return Err(Error::Invalid(format!(
                "short response: {} of {len} bytes at 0x{addr:04X}",
                data.len()
            )));
        }
        out.extend_from_slice(&data[..len.min(data.len())]);
    }
    out.truncate(opts.total);
    Ok(out)
}

/// Sends one prepared report and waits for the response echoing `cmd`, with the
/// same drain / deadline / retry rules as a `transfer` chunk. For single-report
/// commands whose answer is read from the response header (`SET_PROFILE`).
pub fn exchange(
    link: &mut dyn HidLink,
    cmd: Cmd,
    report: &[u8],
    timeout_ms: u32,
    retries: u8,
) -> Result<Response> {
    if cmd.is_denied() {
        return Err(Error::Denied(cmd as u8));
    }
    check_report(report)?;
    await_response(link, report, cmd, None, timeout_ms, retries)
}

fn send_chunk(link: &mut dyn HidLink, report: &[u8], opts: &TransferOpts, addr: u16) -> Result<Vec<u8>> {
    let addr = opts.check_addr.then_some(addr);
    await_response(link, report, opts.cmd, addr, opts.timeout_ms, opts.retries).map(|r| r.data)
}

/// Writes `report` (re-sent up to `retries` times) until a response echoing `cmd`
/// (and `addr`, when given) arrives within `timeout_ms` of each send.
fn await_response(
    link: &mut dyn HidLink,
    report: &[u8],
    cmd: Cmd,
    addr: Option<u16>,
    timeout_ms: u32,
    retries: u8,
) -> Result<Response> {
    let mut buf = vec![0u8; link.report_len()];
    for _attempt in 0..=u32::from(retries) {
        // Drop late replies from earlier attempts or chunks (bounded).
        for _ in 0..64 {
            if link.read(&mut buf, 0)? == 0 {
                break;
            }
        }
        link.write(report)?;
        let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_ms));
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            let ms = i32::try_from(left.as_millis()).unwrap_or(i32::MAX).max(1);
            let got = link.read(&mut buf, ms)?;
            if got == 0 {
                break; // read_timeout already waited
            }
            if let Some(r) = parse_response(&buf[..got])
                && r.cmd == cmd as u8
                && addr.is_none_or(|a| r.addr == a)
            {
                return Ok(r);
            }
        }
    }
    Err(Error::Timeout)
}
