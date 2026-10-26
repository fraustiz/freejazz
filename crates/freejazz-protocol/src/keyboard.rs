//! The keyboard: one read and one write per block, and the full snapshot read in the
//! site's order (protocol.md 5).

use serde::{Deserialize, Serialize};

use crate::blocks::device_info::DEVICE_INFO_LEN;
use crate::blocks::dks::DKS_LEN;
use crate::blocks::game_mode::GAME_MODE_LEN;
use crate::blocks::keys::KEY_MAP_LEN;
use crate::blocks::led::{CUSTOM_LEDS_LEN, LED_EFFECT_LEN};
use crate::blocks::macros::{MACRO_CAPACITY, MACRO_DATA_START, MACRO_TABLE_LEN};
use crate::blocks::magnetic::MAGNETIC_LEN;
use crate::blocks::{
    CustomLeds, DeviceInfo, DksTable, GameMode, KeyMap, LedEffect, Macros, MagneticMap, ModelLimits,
};
use crate::cmd::Cmd;
use crate::error::{Error, Result};
use crate::packet::encode_custom;
use crate::transfer::{TransferOpts, exchange, transfer};
use crate::transport::HidLink;

const RETRIES: u8 = 3;

/// What `SET_FACTORY_RESET` clears (the type byte, protocol.md 6.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ResetKind {
    Keys = 1,
    Lighting = 2,
    Macros = 4,
    /// The only reset (with calibration, 5) the site sends.
    All = 255,
}

/// Everything the site reads from this model, plus the Fn layer slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub info: DeviceInfo,
    pub game_mode: GameMode,
    pub keys: KeyMap,
    /// Fn layer: `None` for this model (the site never reads or writes it).
    pub fn_keys: Option<KeyMap>,
    pub led_effect: LedEffect,
    pub custom_leds: CustomLeds,
    pub macros: Macros,
    pub magnetic: MagneticMap,
    pub dks: DksTable,
    /// The bytes of every block exactly as [`Keyboard::read_snapshot`] read them; `None`
    /// for a snapshot built by the app. Not part of the JSON shape (the backup file
    /// carries them apart, as `raw`) nor of equality. Set it to `None` after editing a
    /// snapshot that came from the keyboard: the bytes then no longer describe it.
    #[serde(skip)]
    pub raw: Option<RawBlocks>,
}

/// Reads of the macro area: `(address, bytes)`.
pub type MacroReads = Vec<(u16, Vec<u8>)>;

/// The bytes of each block as read from the keyboard, including what the typed blocks
/// drop: LED bytes 4 and 13-15, the custom-LED index bytes, the macro area's layout.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RawBlocks {
    pub device_info: Vec<u8>,
    pub game_mode: Vec<u8>,
    pub keys: Vec<u8>,
    pub fn_keys: Option<Vec<u8>>,
    pub led_effect: Vec<u8>,
    pub custom_leds: Vec<u8>,
    /// The 400-byte pointer table (address 0).
    pub macro_table: Vec<u8>,
    /// Every read of the macro area after the table, in order.
    pub macro_reads: MacroReads,
    pub magnetic: Vec<u8>,
    pub dks: Vec<u8>,
}

impl RawBlocks {
    /// The macros the recorded reads hold, following the table as the site does; `Err`
    /// when no recorded read covers a range the table points to.
    pub fn decode_macros(&self) -> Result<Macros> {
        Macros::decode(&self.macro_table, |addr, len| {
            let (start, end) = (usize::from(addr), usize::from(addr) + len);
            self.macro_reads
                .iter()
                .find(|(at, b)| usize::from(*at) <= start && end <= usize::from(*at) + b.len())
                .map(|(at, b)| b[start - usize::from(*at)..end - usize::from(*at)].to_vec())
                .ok_or_else(|| {
                    Error::Invalid(format!("macro area: {len} bytes at {addr} not read"))
                })
        })
    }
}

impl PartialEq for Snapshot {
    /// The configuration held: every block, not the bytes it was read from (`raw`).
    fn eq(&self, other: &Self) -> bool {
        let Snapshot {
            info,
            game_mode,
            keys,
            fn_keys,
            led_effect,
            custom_leds,
            macros,
            magnetic,
            dks,
            raw: _,
        } = self;
        *info == other.info
            && *game_mode == other.game_mode
            && *keys == other.keys
            && *fn_keys == other.fn_keys
            && *led_effect == other.led_effect
            && *custom_leds == other.custom_leds
            && *macros == other.macros
            && *magnetic == other.magnetic
            && *dks == other.dks
    }
}

impl Snapshot {
    /// Checks that `raw` decodes to this snapshot: every byte the typed blocks hold must
    /// agree; the bytes they drop (LED bytes 4 and 13-15, custom-LED index bytes, macro
    /// layout) are not compared, nor the LED colour and `colorMode`, which the keyboard
    /// does not report reliably ([`LedEffect::matches_readback`]). `Err` names the first
    /// block that disagrees. The one rule for backups and write read-backs.
    pub fn check_raw(&self, raw: &RawBlocks) -> std::result::Result<(), &'static str> {
        fn same<T: PartialEq>(decoded: Result<T>, typed: &T) -> bool {
            decoded.is_ok_and(|d| d == *typed)
        }
        let checks = [
            (
                "infos",
                same(DeviceInfo::decode(&raw.device_info), &self.info),
            ),
            (
                "réglages",
                same(GameMode::decode(&raw.game_mode), &self.game_mode),
            ),
            ("touches", same(KeyMap::decode(&raw.keys), &self.keys)),
            (
                "couche Fn",
                match (&raw.fn_keys, &self.fn_keys) {
                    (Some(b), Some(k)) => same(KeyMap::decode(b), k),
                    (None, None) => true,
                    _ => false,
                },
            ),
            (
                "effet lumineux",
                LedEffect::decode(&raw.led_effect)
                    .is_ok_and(|read| self.led_effect.matches_readback(&read)),
            ),
            (
                "couleurs personnalisées",
                same(CustomLeds::decode(&raw.custom_leds), &self.custom_leds),
            ),
            ("macros", same(raw.decode_macros(), &self.macros)),
            (
                "magnétique",
                same(
                    MagneticMap::decode(&raw.magnetic, self.info.rt_precision),
                    &self.magnetic,
                ),
            ),
            ("DKS", same(DksTable::decode(&raw.dks), &self.dks)),
        ];
        match checks.into_iter().find(|(_, ok)| !ok) {
            Some((block, _)) => Err(block),
            None => Ok(()),
        }
    }

    /// UI limits on what differs from `baseline` (the snapshot read from the keyboard):
    /// keys and Fn layer (macro id, DKS slot), macros (ids, [`MACRO_CAPACITY`]),
    /// magnetic (model limits). Values the keyboard itself holds are never refused.
    /// GameMode setters check their own edits.
    pub fn validate_changes(&self, baseline: &Snapshot) -> Result<()> {
        self.keys.validate_changes(&baseline.keys)?;
        match (&self.fn_keys, &baseline.fn_keys) {
            (Some(keys), Some(base)) => keys.validate_changes(base)?,
            (Some(keys), None) => keys.validate()?,
            (None, _) => {}
        }
        self.macros
            .validate_changes(&baseline.macros, MACRO_CAPACITY)?;
        self.magnetic
            .validate_changes(&baseline.magnetic, ModelLimits::ak980max())
    }
}

/// The connected keyboard. Writes check representability only (a value that does not
/// fit its bytes is refused, never truncated); they do not apply the UI limits, so a
/// snapshot read from the keyboard always restores. Before writing edits, check them
/// with [`Snapshot::validate_changes`] (or the blocks' `validate_changes`) against the
/// snapshot read from the keyboard.
pub struct Keyboard<L: HidLink> {
    link: L,
    pub info: DeviceInfo,
}

fn read_device_info(link: &mut dyn HidLink) -> Result<DeviceInfo> {
    // Default options: 500 ms, frameVersion is not known yet (protocol.md 3).
    DeviceInfo::decode(&transfer(
        link,
        &TransferOpts::read(Cmd::GetDeviceInfo, DEVICE_INFO_LEN),
    )?)
}

impl<L: HidLink> Keyboard<L> {
    /// Reads `GET_DEVICE_INFO`; later timeouts depend on its `frame_version`.
    pub fn connect(mut link: L) -> Result<Self> {
        let info = read_device_info(&mut link)?;
        Ok(Self { link, info })
    }

    pub fn link(&self) -> &L {
        &self.link
    }

    pub fn link_mut(&mut self) -> &mut L {
        &mut self.link
    }

    /// Per-chunk timeout of `cmd` (protocol.md 3).
    pub fn timeout_ms(&self, cmd: Cmd) -> u32 {
        let slow = self.info.frame_version == 1;
        match cmd {
            Cmd::GetDeviceInfo => 500,
            Cmd::SetCustomLedData | Cmd::SetProfile | Cmd::GetMagneticAxisStatus => 2000,
            Cmd::SetKey if !slow => 1000,
            Cmd::GetLedData if !slow => 50,
            _ if slow => 2000,
            _ => 500,
        }
    }

    fn read_block(&mut self, cmd: Cmd, addr: u16, len: usize) -> Result<Vec<u8>> {
        let opts = TransferOpts {
            addr_start: addr,
            timeout_ms: self.timeout_ms(cmd),
            retries: RETRIES,
            ..TransferOpts::read(cmd, len)
        };
        transfer(&mut self.link, &opts)
    }

    /// The bytes of a block as read, for checks and diagnostics (bytes the typed blocks
    /// do not keep, such as the custom-LED index bytes). Only the block reads the site
    /// sends to this model (protocol.md 5); any other command is refused before sending.
    pub fn read_raw(&mut self, cmd: Cmd, addr: u16, len: usize) -> Result<Vec<u8>> {
        use Cmd::*;
        match cmd {
            GetDeviceInfo
            | GetGameMode
            | GetKey
            | GetLedEffect
            | GetCustomLedData
            | GetMacro
            | GetMagneticAxisRt
            | GetMagneticAxisDksData => self.read_block(cmd, addr, len),
            _ => Err(Error::Invalid(format!(
                "0x{:02X} is not a block read the site sends to this model",
                cmd as u8
            ))),
        }
    }

    fn write_block(&mut self, cmd: Cmd, addr: u16, data: Vec<u8>, last_flag: bool) -> Result<()> {
        let opts = TransferOpts {
            addr_start: addr,
            timeout_ms: self.timeout_ms(cmd),
            retries: RETRIES,
            need_last_flag: last_flag,
            ..TransferOpts::write(cmd, data)
        };
        transfer(&mut self.link, &opts).map(drop)
    }

    /// Re-reads `GET_DEVICE_INFO` into `self.info`.
    pub fn read_device_info(&mut self) -> Result<DeviceInfo> {
        self.info = read_device_info(&mut self.link)?;
        Ok(self.info.clone())
    }

    pub fn read_game_mode(&mut self) -> Result<GameMode> {
        GameMode::decode(&self.read_block(Cmd::GetGameMode, 0, GAME_MODE_LEN)?)
    }

    /// A report-rate change makes the keyboard re-enumerate on USB after this write.
    pub fn write_game_mode(&mut self, game_mode: &GameMode) -> Result<()> {
        self.write_block(Cmd::SetGameMode, 0, game_mode.encode(), true)
    }

    pub fn read_keys(&mut self) -> Result<KeyMap> {
        KeyMap::decode(&self.read_block(Cmd::GetKey, 0, KEY_MAP_LEN)?)
    }

    pub fn write_keys(&mut self, keys: &KeyMap) -> Result<()> {
        self.write_block(Cmd::SetKey, 0, keys.encode(), true)
    }

    // UNVERIFIED-HW: the site never sends GET_FN_KEY 0x16 to this model (protocol.md 5).
    pub fn read_fn_keys(&mut self) -> Result<KeyMap> {
        KeyMap::decode(&self.read_block(Cmd::GetFnKey, 0, KEY_MAP_LEN)?)
    }

    // UNVERIFIED-HW: the site never sends SET_FN_KEY 0x26 to this model (protocol.md 5).
    pub fn write_fn_keys(&mut self, keys: &KeyMap) -> Result<()> {
        self.write_block(Cmd::SetFnKey, 0, keys.encode(), true)
    }

    pub fn read_led_effect(&mut self) -> Result<LedEffect> {
        LedEffect::decode(&self.read_block(Cmd::GetLedEffect, 0, LED_EFFECT_LEN)?)
    }

    pub fn write_led_effect(&mut self, effect: &LedEffect) -> Result<()> {
        self.write_block(Cmd::SetLedEffect, 0, effect.encode(), true)
    }

    pub fn read_custom_leds(&mut self) -> Result<CustomLeds> {
        CustomLeds::decode(&self.read_block(Cmd::GetCustomLedData, 0, CUSTOM_LEDS_LEN)?)
    }

    /// Visible when the effect mode is 128 (custom).
    pub fn write_custom_leds(&mut self, leds: &CustomLeds) -> Result<()> {
        self.write_block(Cmd::SetCustomLedData, 0, leds.encode(), true)
    }

    /// The pointer table, then each macro's header and actions (protocol.md 6.6).
    pub fn read_macros(&mut self) -> Result<Macros> {
        self.read_macros_raw().map(|(macros, _, _)| macros)
    }

    /// [`Keyboard::read_macros`], with the table and every later read as read.
    fn read_macros_raw(&mut self) -> Result<(Macros, Vec<u8>, MacroReads)> {
        let table = self.read_block(Cmd::GetMacro, 0, MACRO_TABLE_LEN)?;
        let mut reads = Vec::new();
        let macros = Macros::decode(&table, |addr, len| {
            let bytes = self.read_block(Cmd::GetMacro, addr, len)?;
            reads.push((addr, bytes.clone()));
            Ok(bytes)
        })?;
        Ok((macros, table, reads))
    }

    /// No capacity check here: see [`Macros::validate_changes`].
    pub fn write_macros(&mut self, macros: &Macros) -> Result<()> {
        let (table, data) = macros.to_bytes()?;
        self.send_macros(table, data)
    }

    /// Table at 0 with no last-chunk flag, then the macros at 400 if there are any.
    fn send_macros(&mut self, table: Vec<u8>, data: Vec<u8>) -> Result<()> {
        self.write_block(Cmd::SetMacro, 0, table, false)?;
        if data.is_empty() {
            return Ok(());
        }
        self.write_block(Cmd::SetMacro, MACRO_DATA_START, data, true)
    }

    pub fn read_magnetic(&mut self) -> Result<MagneticMap> {
        let bytes = self.read_block(Cmd::GetMagneticAxisRt, 0, MAGNETIC_LEN)?;
        MagneticMap::decode(&bytes, self.info.rt_precision)
    }

    /// Scaled by the connected keyboard's precision; no model limits here: see
    /// [`MagneticMap::validate_changes`].
    pub fn write_magnetic(&mut self, map: &MagneticMap) -> Result<()> {
        let bytes = map.encode(self.info.rt_precision)?;
        self.write_block(Cmd::SetMagneticAxisRt, 0, bytes, true)
    }

    pub fn read_dks(&mut self) -> Result<DksTable> {
        DksTable::decode(&self.read_block(Cmd::GetMagneticAxisDksData, 0, DKS_LEN)?)
    }

    pub fn write_dks(&mut self, table: &DksTable) -> Result<()> {
        let bytes = table.encode()?;
        self.write_block(Cmd::SetMagneticAxisDksData, 0, bytes, true)
    }

    /// Hardware profile `n` (from 1), only when `max_profile > 1` (protocol.md 6.9).
    /// `Ok(true)` when the keyboard echoes `n`. Re-read the snapshot afterwards.
    pub fn set_profile(&mut self, n: u8) -> Result<bool> {
        let max = self.info.max_profile;
        if max <= 1 {
            return Err(Error::Validation(
                "Ce clavier n'a pas de profils matériels".into(),
            ));
        }
        if n == 0 || n > max {
            return Err(Error::Validation(format!(
                "Profil {n} hors limites (1–{max})"
            )));
        }
        let timeout = self.timeout_ms(Cmd::SetProfile);
        let report = encode_custom(
            &[0xAA, Cmd::SetProfile as u8, n],
            &[],
            self.link.report_len(),
        );
        let reply = exchange(&mut self.link, Cmd::SetProfile, &report, timeout, RETRIES)?;
        Ok(reply.len_or_type == n)
    }

    // UNVERIFIED-HW: the site only ever sends type 255 (and 5 for calibration); types
    // 1, 2 and 4 were never sent to this unit. Never run during development.
    /// `SET_FACTORY_RESET` (protocol.md 6.10): one report `AA 0F type`, no reply, then
    /// the site's 100 ms pause. Take a backup and re-read the snapshot around it.
    pub fn factory_reset(&mut self, kind: ResetKind) -> Result<()> {
        let report = encode_custom(
            &[0xAA, Cmd::SetFactoryReset as u8, kind as u8],
            &[],
            self.link.report_len(),
        );
        self.link.write(&report)?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        Ok(())
    }

    /// Reads exactly what the site reads for this model, in its order (protocol.md 5):
    /// device info, game mode, keys, LED effect, custom colours, macros, magnetic, DKS.
    /// Never the Fn layer (`fn_keys` is `None`) nor the serial number (0x84). Keeps
    /// every block's bytes as read in `raw`.
    pub fn read_snapshot(&mut self) -> Result<Snapshot> {
        let info = self.read_device_info()?;
        if info.firmware_status == 1 {
            return Err(Error::UpdatePending);
        }
        let game_mode = self.read_block(Cmd::GetGameMode, 0, GAME_MODE_LEN)?;
        let keys = self.read_block(Cmd::GetKey, 0, KEY_MAP_LEN)?;
        let led_effect = self.read_block(Cmd::GetLedEffect, 0, LED_EFFECT_LEN)?;
        let custom_leds = self.read_block(Cmd::GetCustomLedData, 0, CUSTOM_LEDS_LEN)?;
        let (macros, macro_table, macro_reads) = self.read_macros_raw()?;
        let magnetic = self.read_block(Cmd::GetMagneticAxisRt, 0, MAGNETIC_LEN)?;
        let dks = self.read_block(Cmd::GetMagneticAxisDksData, 0, DKS_LEN)?;
        Ok(Snapshot {
            game_mode: GameMode::decode(&game_mode)?,
            keys: KeyMap::decode(&keys)?,
            fn_keys: None,
            led_effect: LedEffect::decode(&led_effect)?,
            custom_leds: CustomLeds::decode(&custom_leds)?,
            macros,
            magnetic: MagneticMap::decode(&magnetic, info.rt_precision)?,
            dks: DksTable::decode(&dks)?,
            raw: Some(RawBlocks {
                device_info: info.raw.clone(),
                game_mode,
                keys,
                fn_keys: None,
                led_effect,
                custom_leds,
                macro_table,
                macro_reads,
                magnetic,
                dks,
            }),
            info,
        })
    }

    /// Writes every block of `snapshot` (`info` is read only; `fn_keys` only when
    /// `Some`): the restore of what the keyboard held, so no UI limit is applied. All
    /// blocks are encoded first: nothing is sent when one is not representable. Order:
    /// the site's profile order (protocol.md 5), then GameMode last since a report-rate
    /// change makes the keyboard re-enumerate on USB. The magnetic scale comes from the
    /// connected keyboard (`self.info`), not from `snapshot.info`.
    pub fn write_snapshot(&mut self, snapshot: &Snapshot) -> Result<()> {
        let keys = snapshot.keys.encode();
        let fn_keys = snapshot.fn_keys.as_ref().map(KeyMap::encode);
        let (macro_table, macro_data) = snapshot.macros.to_bytes()?;
        let magnetic = snapshot.magnetic.encode(self.info.rt_precision)?;
        let dks = snapshot.dks.encode()?;

        self.write_block(Cmd::SetKey, 0, keys, true)?;
        if let Some(bytes) = fn_keys {
            self.write_block(Cmd::SetFnKey, 0, bytes, true)?; // UNVERIFIED-HW
        }
        self.write_led_effect(&snapshot.led_effect)?;
        self.write_custom_leds(&snapshot.custom_leds)?;
        self.send_macros(macro_table, macro_data)?;
        self.write_block(Cmd::SetMagneticAxisRt, 0, magnetic, true)?;
        self.write_block(Cmd::SetMagneticAxisDksData, 0, dks, true)?;
        self.write_game_mode(&snapshot.game_mode)
    }
}
