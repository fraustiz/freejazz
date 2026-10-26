//! `--mock`: a fake AK980 Max. [`Sim`] is a [`HidLink`] that answers like the keyboard
//! (firmware 1.28, 64-byte reports): reads serve the blocks it holds, writes change them,
//! so the app runs the real device manager and protocol against it. Starts from the
//! fixture snapshot (the tested unit).
//!
//! Like the tested unit (protocol.md 9.7), it reports the LED colour as `#ffffff` / RGB after
//! a `SET_LED_EFFECT` and bytes 4 and 13-15 as 0, and it re-enumerates on USB (the open
//! link dies) when the report rate changes. Tests can unplug it, make it drop off in the
//! middle of a write, or stop acknowledging writes.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use freejazz_protocol::cmd::Cmd;
use freejazz_protocol::packet::check_report;
use freejazz_protocol::transport::{DeviceIdent, HidLink};
use freejazz_protocol::{Error, Keyboard, Result, Snapshot};

use crate::device::{self, DeviceHandle, DeviceKind, Manager, Port, Waker};

const FIXTURE: &str = include_str!("../assets/fixture-snapshot.json");
/// MI_02 of the AK980 Max: 64-byte input and output reports (protocol.md 9.1).
const REPORT_LEN: usize = 64;
/// Per report, so a full read takes about as long as on the keyboard.
const MOCK_LATENCY: Duration = Duration::from_millis(3);

/// The fixture as the app ships it (no read bytes attached).
pub fn fixture() -> Snapshot {
    serde_json::from_str(FIXTURE).expect("fixture-snapshot.json is a snapshot")
}

pub fn ident(snapshot: &Snapshot) -> DeviceIdent {
    DeviceIdent {
        pid: snapshot.info.pid,
        serial: None,
        path: "faux-clavier".into(),
        interface: 2,
        usage_page: 0xFF68,
        usage: 0x61,
    }
}

/// Reads the fixture through the protocol crate, as the device manager does.
pub fn read_fixture() -> Result<Snapshot> {
    let sim = Sim::new(&fixture());
    let (link, _) = sim.port().open()?;
    Keyboard::connect(link)?.read_snapshot()
}

/// Where the fake keyboard keeps its backups.
pub fn backups_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("freejazz-mock-backups")
}

/// Starts the fake keyboard behind the real device manager. Its auto-backups go to a
/// temporary folder, never to `%APPDATA%\Freejazz\backups`.
pub fn spawn(waker: Waker) -> DeviceHandle {
    let sim = Sim::new(&fixture()).with_latency(MOCK_LATENCY);
    let manager = Manager::new(sim.port(), Some(backups_dir()));
    DeviceHandle::spawn(DeviceKind::Mock, waker, move |io| device::run(io, manager))
}

struct State {
    ident: DeviceIdent,
    /// Each block as the keyboard holds it, by its GET command; the macro area (0x15)
    /// is the table and the macros from address 0.
    blocks: HashMap<u8, Vec<u8>>,
    plugged: bool,
    /// Bumped by each USB enumeration: links of an older one are dead.
    generation: u32,
    enumerations: u32,
    written: Vec<Vec<u8>>,
    /// Reports still accepted before the keyboard is unplugged.
    fail_after: Option<usize>,
    /// Reports still accepted before the keyboard stops acknowledging writes.
    refuse_after: Option<usize>,
    latency: Duration,
    /// Plugged in but held by another program: opening it finds it, its probe gets no
    /// answer.
    silent: bool,
    /// Times it was opened (each open probes it).
    probes: u32,
}

/// A simulated keyboard, shared by the links opened on it and the test holding it.
#[derive(Clone)]
pub struct Sim(Arc<Mutex<State>>);

impl Sim {
    pub fn new(snapshot: &Snapshot) -> Self {
        let mut blocks = HashMap::new();
        let mut info = snapshot.info.raw.clone();
        info.resize(48, 0);
        blocks.insert(Cmd::GetDeviceInfo as u8, info);
        blocks.insert(Cmd::GetGameMode as u8, snapshot.game_mode.encode());
        blocks.insert(Cmd::GetKey as u8, snapshot.keys.encode());
        let mut led = snapshot.led_effect.encode();
        clear_led_constants(&mut led);
        blocks.insert(Cmd::GetLedEffect as u8, led);
        blocks.insert(Cmd::GetCustomLedData as u8, snapshot.custom_leds.encode());
        let (table, data) = snapshot.macros.to_bytes().expect("fixture macros encode");
        blocks.insert(Cmd::GetMacro as u8, [table, data].concat());
        let precision = snapshot.info.rt_precision;
        blocks.insert(
            Cmd::GetMagneticAxisRt as u8,
            snapshot
                .magnetic
                .encode(precision)
                .expect("magnetic encodes"),
        );
        blocks.insert(
            Cmd::GetMagneticAxisDksData as u8,
            snapshot.dks.encode().expect("DKS encodes"),
        );
        Self(Arc::new(Mutex::new(State {
            ident: ident(snapshot),
            blocks,
            plugged: true,
            generation: 1,
            enumerations: 1,
            written: Vec::new(),
            fail_after: None,
            refuse_after: None,
            latency: Duration::ZERO,
            silent: false,
            probes: 0,
        })))
    }

    /// Makes the keyboard ignore the connection probe (another program holds it).
    pub fn set_silent(&self, silent: bool) {
        self.state().silent = silent;
    }

    /// How many times the keyboard was opened and probed.
    pub fn probes(&self) -> u32 {
        self.state().probes
    }

    pub fn with_latency(self, latency: Duration) -> Self {
        self.state().latency = latency;
        self
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn port(&self) -> SimPort {
        SimPort(self.clone())
    }

    /// Every report written so far, in order.
    pub fn written(&self) -> Vec<Vec<u8>> {
        self.state().written.clone()
    }

    pub fn clear_written(&self) {
        self.state().written.clear();
    }

    pub fn unplug(&self) {
        self.state().plugged = false;
    }

    /// Plugs the keyboard (back) in: a new USB enumeration.
    pub fn plug(&self) {
        let mut s = self.state();
        s.plugged = true;
        s.generation += 1;
        s.enumerations += 1;
        s.fail_after = None;
        s.refuse_after = None;
    }

    /// Accepts `n` more reports, then is unplugged as the next one is written.
    pub fn fail_after_writes(&self, n: usize) {
        self.state().fail_after = Some(n);
    }

    /// Accepts `n` more reports, then stays plugged but acknowledges no more writes.
    pub fn refuse_writes_after(&self, n: usize) {
        self.state().refuse_after = Some(n);
    }

    /// USB enumerations so far (1 at creation).
    pub fn enumerations(&self) -> u32 {
        self.state().enumerations
    }
}

/// Bytes the keyboard reads back as 0 whatever was written (protocol.md 9.7).
fn clear_led_constants(led: &mut [u8]) {
    led[4] = 0;
    led[13..16].fill(0);
}

fn reply(cmd: u8, len: u8, addr: u16, data: &[u8]) -> Vec<u8> {
    let mut r = vec![0u8; REPORT_LEN];
    r[0] = 0x55;
    r[1] = cmd;
    r[2] = len;
    r[3..5].copy_from_slice(&addr.to_le_bytes());
    let n = data.len().min(REPORT_LEN - 8);
    r[8..8 + n].copy_from_slice(&data[..n]);
    r
}

impl State {
    /// The keyboard's answer to `report`, if any; changes what it holds.
    fn answer(&mut self, report: &[u8]) -> Option<Vec<u8>> {
        let (cmd, len) = (report[1], report[2]);
        let addr = u16::from_le_bytes([report[3], report[4]]);
        let at = usize::from(addr);
        let data = &report[8..8 + usize::from(len).min(REPORT_LEN - 8)];
        match cmd {
            0x10..=0x18 => {
                let block = self.blocks.get(&cmd)?;
                let mut out = vec![0u8; data.len()];
                if at < block.len() {
                    let end = (at + out.len()).min(block.len());
                    out[..end - at].copy_from_slice(&block[at..end]);
                }
                Some(reply(cmd, len, addr, &out))
            }
            0x21..=0x28 => {
                let get = cmd - 0x10;
                let block = self.blocks.entry(get).or_default();
                if block.len() < at + data.len() {
                    block.resize(at + data.len(), 0);
                }
                let rate_before = (get == Cmd::GetGameMode as u8).then(|| block[5]);
                block[at..at + data.len()].copy_from_slice(data);
                if get == Cmd::GetLedEffect as u8 {
                    // firmware 1.28: white / RGB read back after a write
                    block[1..4].fill(0xFF);
                    block[8] = 1;
                    if block[0] == 128 {
                        // custom colours read back as mode 20 (protocol.md 9.7)
                        block[0] = 20;
                    }
                    clear_led_constants(block);
                }
                if rate_before.is_some_and(|r| r != block[5]) {
                    // a new report rate: the keyboard re-enumerates, no acknowledgement
                    self.generation += 1;
                    self.enumerations += 1;
                    return None;
                }
                Some(reply(cmd, len, addr, &[]))
            }
            // live frame (0x32) and key status (0x68): acknowledged, zero travel
            0x32 | 0x68 => Some(reply(cmd, len, addr, &vec![0; data.len()])),
            // key test / calibration start: acknowledged (no 0xFB report is simulated)
            0x64 | 0x66 => Some(reply(cmd, len, addr, &[])),
            // SET_PROFILE: the keyboard echoes the profile in byte 2
            0x90 => Some(reply(cmd, report[2], 0, &[])),
            _ => None,
        }
    }
}

/// A link opened on a [`Sim`]; dead once the keyboard is unplugged or re-enumerated.
pub struct SimLink {
    sim: Sim,
    generation: u32,
    pending: VecDeque<Vec<u8>>,
}

impl SimLink {
    fn gone() -> Error {
        Error::Hid("le périphérique n'est plus connecté".into())
    }
}

impl HidLink for SimLink {
    fn write(&mut self, report: &[u8]) -> Result<()> {
        check_report(report)?;
        let mut s = self.sim.state();
        if !s.plugged || s.generation != self.generation {
            return Err(Self::gone());
        }
        if let Some(n) = s.fail_after.as_mut() {
            if *n == 0 {
                s.plugged = false;
                return Err(Self::gone());
            }
            *n -= 1;
        }
        s.written.push(report.to_vec());
        let is_set = (0x21..=0x28).contains(&report.get(1).copied().unwrap_or(0));
        let refused = match s.refuse_after.as_mut() {
            Some(0) => is_set,
            Some(n) => {
                *n -= 1;
                false
            }
            None => false,
        };
        let latency = s.latency;
        let answer = if refused || report.len() < 8 || report[0] != 0xAA {
            None
        } else {
            s.answer(report)
        };
        drop(s);
        if !latency.is_zero() {
            std::thread::sleep(latency);
        }
        self.pending.extend(answer);
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8], _timeout_ms: i32) -> Result<usize> {
        {
            let s = self.sim.state();
            if !s.plugged || s.generation != self.generation {
                return Err(Self::gone());
            }
        }
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
        REPORT_LEN
    }
}

/// Opens links on a [`Sim`].
pub struct SimPort(Sim);

impl Port for SimPort {
    type Link = SimLink;

    fn open(&mut self) -> Result<(SimLink, DeviceIdent)> {
        let mut s = self.0.state();
        if !s.plugged {
            return Err(Error::NotFound);
        }
        s.probes += 1;
        if s.silent {
            // as `open_keyboard`: found, but the probe went unanswered
            return Err(Error::Timeout);
        }
        let link = SimLink {
            sim: self.0.clone(),
            generation: s.generation,
            pending: VecDeque::new(),
        };
        Ok((link, s.ident.clone()))
    }

    fn present(&mut self, _ident: &DeviceIdent) -> bool {
        self.0.state().plugged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_facts() {
        let s = fixture();
        assert_eq!(s.info.firmware, "1.28");
        assert_eq!(s.info.max_profile, 0);
        assert_eq!(s.game_mode.report_rate(), Some(8000));
    }

    #[test]
    fn mock_serves_the_fixture_through_the_protocol() {
        let read = read_fixture().unwrap();
        assert_eq!(read, fixture());
        let raw = read.raw.as_ref().expect("read bytes kept");
        assert_eq!(read.check_raw(raw), Ok(()));
        // 64-byte reports, as on MI_02
        assert_eq!(raw.led_effect[4], 0);
    }

    #[test]
    fn mock_worker_connects_then_applies() {
        use crate::device::{Event, Request};
        use std::thread;
        let mut handle = spawn(std::sync::Arc::new(|| {}));
        fn recv(handle: &mut DeviceHandle) -> Event {
            loop {
                match handle.poll() {
                    Some(Event::Notice(_)) => {}
                    Some(e) => return e,
                    None => thread::sleep(Duration::from_millis(10)),
                }
            }
        }
        let Event::Connected { snapshot, .. } = recv(&mut handle) else {
            panic!("expected Connected");
        };
        let mut edited = *snapshot;
        edited.raw = None;
        edited.led_effect.brightness = 3;
        handle.send(Request::Apply(Box::new(edited.clone())));
        match recv(&mut handle) {
            Event::Applied(s) => {
                assert_eq!(*s, edited);
                assert!(s.raw.is_some(), "re-read from the keyboard");
            }
            other => panic!("expected Applied, got {other:?}"),
        }
    }
}
