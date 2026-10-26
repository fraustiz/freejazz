//! `GET_GAME_MODE` 0x11 / `SET_GAME_MODE` 0x21, 56 bytes (protocol.md 6.2).
//!
//! The block is kept as the 56 bytes read; setters change single bytes, so bytes the
//! site does not read (0, 10, 12-13, 21-55) are written back as read. The site zeroes
//! them; Freejazz deliberately does not.

use serde::{Deserialize, Serialize};

use super::{check_len, mm};
use crate::error::{Error, Result};

pub const GAME_MODE_LEN: usize = 56;

/// `reportRate` codes (protocol.md 6.2). Codes 0-2 are unconfirmed and not offered.
const REPORT_RATES: [(u8, u32); 4] = [(3, 1000), (4, 2000), (5, 4000), (6, 8000)];

/// Dead zones offered by the site: 0-0.50 mm by 0.01 mm (protocol.md 6.2).
pub const DEAD_ZONE_MAX_MM: f32 = 0.50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "GameModeJson", into = "GameModeJson")]
pub struct GameMode {
    pub raw: [u8; GAME_MODE_LEN],
}

macro_rules! byte_fields {
    ($($(#[$doc:meta])* $get:ident / $set:ident = $idx:literal;)*) => {
        $(
            $(#[$doc])*
            pub fn $get(&self) -> u8 {
                self.raw[$idx]
            }
            pub fn $set(&mut self, v: u8) {
                self.raw[$idx] = v;
            }
        )*
    };
}

impl GameMode {
    pub fn decode(b: &[u8]) -> Result<Self> {
        check_len("GAME_MODE", b, GAME_MODE_LEN)?;
        let mut raw = [0u8; GAME_MODE_LEN];
        raw.copy_from_slice(b);
        Ok(Self { raw })
    }

    /// The 56 bytes to write: the bytes read, with the edits made since.
    pub fn encode(&self) -> Vec<u8> {
        self.raw.to_vec()
    }

    byte_fields! {
        game_mode / set_game_mode = 1;
        fn_switch / set_fn_switch = 2;
        /// Minutes, 0 = never.
        sleep_time / set_sleep_time = 3;
        key_delay / set_key_delay = 4;
        system_mode / set_system_mode = 6;
        /// 0/1 "Mode de stabilité".
        stability_mode / set_stability_mode = 11;
        /// 0/1 "Calibration dynamique adaptative".
        auto_calibration / set_auto_calibration = 14;
        single_key_wakeup / set_single_key_wakeup = 15;
        /// 0 volume, 1 keyboard brightness, 2 screen brightness.
        push_button_mode / set_push_button_mode = 16;
        /// Read and written back as is (no setting on the site).
        nkro / set_nkro = 17;
        power_mode / set_power_mode = 20;
    }

    /// Raw `reportRate` code (byte 5).
    pub fn report_rate_code(&self) -> u8 {
        self.raw[5]
    }

    /// Report rate in Hz; `None` for a code outside the site's table.
    pub fn report_rate(&self) -> Option<u32> {
        REPORT_RATES
            .iter()
            .find(|(c, _)| *c == self.raw[5])
            .map(|(_, hz)| *hz)
    }

    /// Changing the rate makes the keyboard re-enumerate on USB after the write.
    pub fn set_report_rate(&mut self, hz: u32) -> Result<()> {
        let (code, _) = REPORT_RATES.iter().find(|(_, h)| *h == hz).ok_or_else(|| {
            Error::Validation(format!(
                "Fréquence d'envoi de {hz} Hz non prise en charge (1000, 2000, 4000 ou 8000 Hz)"
            ))
        })?;
        self.raw[5] = *code;
        Ok(())
    }

    pub fn top_dead_zone_mm(&self) -> f32 {
        f32::from(self.raw[8]) / 100.0
    }

    /// Rounded to 0.01 mm (the site truncates); refused outside 0-0.50 mm or off-step.
    pub fn set_top_dead_zone_mm(&mut self, value: f32) -> Result<()> {
        self.raw[8] = dead_zone_byte("haute", value)?;
        Ok(())
    }

    pub fn bottom_dead_zone_mm(&self) -> f32 {
        f32::from(self.raw[9]) / 100.0
    }

    pub fn set_bottom_dead_zone_mm(&mut self, value: f32) -> Result<()> {
        self.raw[9] = dead_zone_byte("basse", value)?;
        Ok(())
    }
}

fn dead_zone_byte(which: &str, value: f32) -> Result<u8> {
    let hundredths = (value * 100.0).round();
    let in_range = value.is_finite() && (-1e-4..=DEAD_ZONE_MAX_MM + 1e-4).contains(&value);
    if !in_range || (value * 100.0 - hundredths).abs() > 1e-3 {
        return Err(Error::Validation(format!(
            "Zone morte {which} : {} mm hors limites (0,00–0,50 mm, par pas de 0,01 mm)",
            mm(value)
        )));
    }
    Ok(hundredths as u8)
}

/// JSON shape: the raw bytes plus the decoded fields. On the way back, `raw` is the
/// starting point and a field whose value differs from what `raw` holds is applied
/// as an edit (validated); absent fields leave `raw` untouched.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameModeJson {
    raw: Vec<u8>,
    #[serde(default)]
    game_mode: Option<u8>,
    #[serde(default)]
    fn_switch: Option<u8>,
    #[serde(default)]
    sleep_time: Option<u8>,
    #[serde(default)]
    key_delay: Option<u8>,
    /// Hz; `null` when the code is outside the site's table.
    #[serde(default)]
    report_rate: Option<u32>,
    #[serde(default)]
    system_mode: Option<u8>,
    #[serde(default)]
    top_dead_zone_mm: Option<f32>,
    #[serde(default)]
    bottom_dead_zone_mm: Option<f32>,
    #[serde(default)]
    stability_mode: Option<u8>,
    #[serde(default)]
    auto_calibration: Option<u8>,
    #[serde(default)]
    single_key_wakeup: Option<u8>,
    #[serde(default)]
    push_button_mode: Option<u8>,
    #[serde(default)]
    nkro: Option<u8>,
    #[serde(default)]
    power_mode: Option<u8>,
}

impl From<GameMode> for GameModeJson {
    fn from(g: GameMode) -> Self {
        Self {
            raw: g.raw.to_vec(),
            game_mode: Some(g.game_mode()),
            fn_switch: Some(g.fn_switch()),
            sleep_time: Some(g.sleep_time()),
            key_delay: Some(g.key_delay()),
            report_rate: g.report_rate(),
            system_mode: Some(g.system_mode()),
            top_dead_zone_mm: Some(g.top_dead_zone_mm()),
            bottom_dead_zone_mm: Some(g.bottom_dead_zone_mm()),
            stability_mode: Some(g.stability_mode()),
            auto_calibration: Some(g.auto_calibration()),
            single_key_wakeup: Some(g.single_key_wakeup()),
            push_button_mode: Some(g.push_button_mode()),
            nkro: Some(g.nkro()),
            power_mode: Some(g.power_mode()),
        }
    }
}

impl TryFrom<GameModeJson> for GameMode {
    type Error = Error;

    fn try_from(j: GameModeJson) -> Result<Self> {
        let mut g = GameMode::decode(&j.raw)?;
        type Setter = fn(&mut GameMode, u8);
        let bytes: [(Option<u8>, Setter); 11] = [
            (j.game_mode, GameMode::set_game_mode),
            (j.fn_switch, GameMode::set_fn_switch),
            (j.sleep_time, GameMode::set_sleep_time),
            (j.key_delay, GameMode::set_key_delay),
            (j.system_mode, GameMode::set_system_mode),
            (j.stability_mode, GameMode::set_stability_mode),
            (j.auto_calibration, GameMode::set_auto_calibration),
            (j.single_key_wakeup, GameMode::set_single_key_wakeup),
            (j.push_button_mode, GameMode::set_push_button_mode),
            (j.nkro, GameMode::set_nkro),
            (j.power_mode, GameMode::set_power_mode),
        ];
        for (value, set) in bytes {
            if let Some(v) = value {
                set(&mut g, v);
            }
        }
        if let Some(hz) = j.report_rate
            && g.report_rate() != Some(hz)
        {
            g.set_report_rate(hz)?;
        }
        let edited = |new: f32, current: f32| !new.is_finite() || (new - current).abs() > 1e-6;
        if let Some(v) = j.top_dead_zone_mm
            && edited(v, g.top_dead_zone_mm())
        {
            g.set_top_dead_zone_mm(v)?;
        }
        if let Some(v) = j.bottom_dead_zone_mm
            && edited(v, g.bottom_dead_zone_mm())
        {
            g.set_bottom_dead_zone_mm(v)?;
        }
        Ok(g)
    }
}
