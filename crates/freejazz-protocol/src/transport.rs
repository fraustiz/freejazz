use std::collections::{HashMap, VecDeque};

use hidapi::{HidApi, HidDevice};

use crate::blocks::device_info::DEVICE_INFO_LEN;
use crate::cmd::Cmd;
use crate::error::{Error, Result};
use crate::packet::check_report;
use crate::transfer::{TransferOpts, transfer};

/// A HID report pipe. `write` receives the report WITHOUT the report-id byte.
pub trait HidLink: Send {
    fn write(&mut self, report: &[u8]) -> Result<()>;
    /// Returns the number of bytes read; 0 means timeout.
    fn read(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize>;
    fn report_len(&self) -> usize;
}

const VID: u16 = 0x0C45;
const PIDS: [u16; 6] = [0x8043, 0x8050, 0x8051, 0x8052, 0x8053, 0x8054];
/// The site's report length when the descriptor gives none (protocol.md 1).
const DEFAULT_REPORT_LEN: usize = 32;
/// Usage pages of the collections the site opens (protocol.md 1). Not 0xFF67, the
/// large-report interface (animations, screen) of other models: on the AK980 Max it is
/// MI_03, with 4096-byte output reports.
const SITE_USAGE_PAGES: [u16; 6] = [0xFF68, 0xFF80, 0xFF60, 0xFF00, 0xFF01, 0xFF1B];

pub fn is_site_usage_page(page: u16) -> bool {
    SITE_USAGE_PAGES.contains(&page)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReportKind {
    Input,
    Output,
    Feature,
}

/// An Input, Output or Feature main item of a HID report descriptor, with the global
/// state in force: report id, size of one field in bits, number of fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportItem {
    pub kind: ReportKind,
    pub report_id: u8,
    pub size: u32,
    pub count: u32,
}

/// The main items of a HID report descriptor, in order.
pub fn report_items(desc: &[u8]) -> Vec<ReportItem> {
    let mut items = Vec::new();
    let (mut size, mut count, mut id) = (0u32, 0u32, 0u8);
    let mut stack = Vec::new();
    let mut i = 0;
    while i < desc.len() {
        let prefix = desc[i];
        if prefix == 0xFE {
            // long item: 0xFE, data size, tag, data
            i += 3 + usize::from(desc.get(i + 1).copied().unwrap_or(0));
            continue;
        }
        let n = [0, 1, 2, 4][usize::from(prefix & 3)];
        let data = desc.get(i + 1..i + 1 + n).unwrap_or(&[]);
        let value = data
            .iter()
            .rev()
            .fold(0u32, |v, &b| (v << 8) | u32::from(b));
        let kind = match prefix & 0xFC {
            0x74 => {
                size = value;
                None
            }
            0x94 => {
                count = value;
                None
            }
            0x84 => {
                id = value as u8;
                None
            }
            0xA4 => {
                stack.push((size, count, id));
                None
            }
            0xB4 => {
                if let Some(s) = stack.pop() {
                    (size, count, id) = s;
                }
                None
            }
            0x80 => Some(ReportKind::Input),
            0x90 => Some(ReportKind::Output),
            0xB0 => Some(ReportKind::Feature),
            _ => None,
        };
        if let Some(kind) = kind {
            items.push(ReportItem {
                kind,
                report_id: id,
                size,
                count,
            });
        }
        i += 1 + n;
    }
    items
}

/// The report length the site uses with a collection: `reportCount` of its first
/// output item, 32 when there is none (protocol.md 1).
pub fn site_report_len(desc: &[u8]) -> usize {
    report_items(desc)
        .iter()
        .find(|item| item.kind == ReportKind::Output && item.count > 0)
        .map_or(DEFAULT_REPORT_LEN, |item| item.count as usize)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdent {
    pub pid: u16,
    pub serial: Option<String>,
    pub path: String,
    pub interface: i32,
    pub usage_page: u16,
    pub usage: u16,
}

pub struct HidapiLink {
    dev: HidDevice,
    report_len: usize,
}

impl HidLink for HidapiLink {
    fn write(&mut self, report: &[u8]) -> Result<()> {
        check_report(report)?;
        let mut buf = Vec::with_capacity(report.len() + 1);
        buf.push(0); // report id
        buf.extend_from_slice(report);
        self.dev.write(&buf)?;
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize> {
        Ok(self.dev.read_timeout(buf, timeout_ms)?)
    }

    fn report_len(&self) -> usize {
        self.report_len
    }
}

/// Opens the collection that answers a read-only `GET_DEVICE_INFO` probe, among the
/// collections the site opens (usage pages of protocol.md 1). The report length is the
/// site's: `reportCount` of the collection's first output report (64 on the tested unit),
/// read from the descriptor Windows reconstructs; 32 if it cannot be read.
///
/// `Err(NotFound)`: no such collection is plugged in (nothing was sent).
/// `Err(Timeout)`: one is, but did not answer the probe (another program may hold the
/// keyboard): the caller should wait before probing again.
pub fn open_keyboard() -> Result<(HidapiLink, DeviceIdent)> {
    let api = HidApi::new()?;
    let mut silent = false;
    for info in api.device_list() {
        if info.vendor_id() != VID
            || !PIDS.contains(&info.product_id())
            || !is_site_usage_page(info.usage_page())
        {
            continue;
        }
        let Ok(dev) = info.open_device(&api) else {
            continue;
        };
        let mut desc = vec![0u8; hidapi::MAX_REPORT_DESCRIPTOR_SIZE];
        let report_len = match dev.get_report_descriptor(&mut desc) {
            Ok(n) => site_report_len(&desc[..n]),
            Err(_) => DEFAULT_REPORT_LEN,
        };
        let mut link = HidapiLink { dev, report_len };
        if probe(&mut link) {
            let ident = DeviceIdent {
                pid: info.product_id(),
                serial: info.serial_number().map(str::to_owned),
                path: info.path().to_string_lossy().into_owned(),
                interface: info.interface_number(),
                usage_page: info.usage_page(),
                usage: info.usage(),
            };
            return Ok((link, ident));
        }
        silent = true;
    }
    Err(if silent {
        Error::Timeout
    } else {
        Error::NotFound
    })
}

/// Tells whether the keyboard's HID collection is still plugged in. Host-side
/// enumeration of this vendor's devices only: nothing is sent to the keyboard. The
/// hidapi context is kept between checks, so only the first one enumerates every HID
/// device (a full enumeration every second costs ~20 ms of CPU).
#[derive(Default)]
pub struct Presence {
    api: Option<HidApi>,
}

impl Presence {
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` when hidapi cannot enumerate (a gone keyboard then shows up as a link
    /// error).
    pub fn check(&mut self, path: &str) -> bool {
        if self.api.is_none() {
            self.api = HidApi::new().ok();
        }
        let Some(api) = self.api.as_mut() else {
            return true;
        };
        if api.reset_devices().is_err() || api.add_devices(VID, 0).is_err() {
            return true;
        }
        api.device_list()
            .any(|d| PIDS.contains(&d.product_id()) && d.path().to_string_lossy() == path)
    }
}

/// Sends the site's own first request, a `GET_DEVICE_INFO` read (500 ms, no retry), and
/// tells whether the link answers it.
pub fn probe(link: &mut dyn HidLink) -> bool {
    let opts = TransferOpts {
        retries: 0,
        ..TransferOpts::read(Cmd::GetDeviceInfo, DEVICE_INFO_LEN)
    };
    transfer(link, &opts).is_ok()
}

/// Scripted link for tests: each write whose `(cmd, addr)` has a queued script
/// entry delivers that entry's reports to later reads; otherwise it is silent.
pub struct MockLink {
    report_len: usize,
    scripts: HashMap<(u8, u16), VecDeque<Vec<Vec<u8>>>>,
    pending: VecDeque<Vec<u8>>,
    /// Every report written, in order.
    pub written: Vec<Vec<u8>>,
}

impl MockLink {
    pub fn new(report_len: usize) -> Self {
        Self {
            report_len,
            scripts: HashMap::new(),
            pending: VecDeque::new(),
            written: Vec::new(),
        }
    }

    /// Queues one reply set for the next unanswered write of `(cmd, addr)`.
    pub fn script(&mut self, cmd: u8, addr: u16, reports: Vec<Vec<u8>>) {
        self.scripts.entry((cmd, addr)).or_default().push_back(reports);
    }
}

impl HidLink for MockLink {
    fn write(&mut self, report: &[u8]) -> Result<()> {
        check_report(report)?;
        self.written.push(report.to_vec());
        if report.len() >= 5 {
            let key = (report[1], u16::from_le_bytes([report[3], report[4]]));
            if let Some(reports) = self.scripts.get_mut(&key).and_then(VecDeque::pop_front) {
                self.pending.extend(reports);
            }
        }
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8], _timeout_ms: i32) -> Result<usize> {
        match self.pending.pop_front() {
            Some(r) => {
                let n = r.len().min(buf.len());
                buf[..n].copy_from_slice(&r[..n]);
                Ok(n)
            }
            None => Ok(0),
        }
    }

    fn report_len(&self) -> usize {
        self.report_len
    }
}
