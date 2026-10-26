//! `GET/SET_MAGNETIC_AXIS_RT` 0x17 / 0x27, 1024 bytes: 128 entries of 8 bytes
//! (protocol.md 6.7), and the model's limits (`model.json` → `performanceConfig`).

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use super::{KEY_SLOTS, check_len, mm, mm2};
use crate::error::{Error, Result};

pub const MAGNETIC_LEN: usize = KEY_SLOTS * 8;

/// Tolerance on millimetre comparisons (the finest device step is 0.001 mm).
const MM_EPS: f32 = 1e-4;

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MagneticKey {
    /// Switch type: `value` of a `model.json` `axisList` entry (1 axis_9, 2 jade, 3 king).
    pub axis: u8,
    /// Per key: Rapid Trigger active over the full travel instead of only past the
    /// trigger point (flags bit 0, `isWholeFast`).
    pub whole_fast: bool,
    /// "Mode rage" (flags bit 1), hidden for this model.
    pub rampage: bool,
    /// Trigger point.
    pub trigger_mm: f32,
    /// Rapid Trigger press sensitivity; 0 together with `release_rt_mm` = RT off.
    pub press_rt_mm: f32,
    pub release_rt_mm: f32,
    /// Bits 2-7 of the flags byte, in place, as read (bits 0-1 are the two flags above).
    #[serde(default)]
    pub other_flags: u8,
}

impl MagneticKey {
    /// An all-zero entry: a slot without a key, not checked against the limits.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// `(trigger, rt)` divisors: trigger / 1000 if `rtPrecision == 2` else / 100,
/// RT / 1000 if `rtPrecision > 0` else / 100.
fn scales(rt_precision: u8) -> (f32, f32) {
    let trigger = if rt_precision == 2 { 1000.0 } else { 100.0 };
    let rt = if rt_precision > 0 { 1000.0 } else { 100.0 };
    (trigger, rt)
}

/// `value × scale` as a u16, refused (never rounded or truncated) when it is not a
/// whole number of device steps or does not fit 16 bits.
fn to_raw(i: usize, what: &str, value: f32, scale: f32) -> Result<u16> {
    let exact = value * scale;
    let raw = exact.round();
    // 1/100 of a step absorbs f32 noise (at most ~0.004 step at 65535)
    if (0.0..=f32::from(u16::MAX)).contains(&raw) && (exact - raw).abs() <= 0.01 {
        return Ok(raw as u16);
    }
    Err(Error::Validation(format!(
        "Touche {i} : {what} de {} mm impossible à enregistrer          (le clavier accepte 0 à {} mm, par pas de {} mm)",
        mm(value),
        mm(f32::from(u16::MAX) / scale),
        mm(1.0 / scale)
    )))
}

fn in_range(v: f32, min: f32, max: f32) -> bool {
    v.is_finite() && v >= min - MM_EPS && v <= max + MM_EPS
}

/// Entry `i` = key of layout index `i`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MagneticMap(#[serde(with = "super::fixed_array")] pub [MagneticKey; KEY_SLOTS]);

impl Default for MagneticMap {
    fn default() -> Self {
        Self([MagneticKey::default(); KEY_SLOTS])
    }
}

impl MagneticMap {
    pub fn decode(b: &[u8], rt_precision: u8) -> Result<Self> {
        check_len("MAGNETIC_AXIS_RT", b, MAGNETIC_LEN)?;
        let (ts, rs) = scales(rt_precision);
        let mut map = Self::default();
        for (k, e) in map.0.iter_mut().zip(b.chunks_exact(8)) {
            let at = |i: usize| f32::from(u16::from_le_bytes([e[i], e[i + 1]]));
            *k = MagneticKey {
                axis: e[0],
                whole_fast: e[1] & 0b01 != 0,
                rampage: e[1] & 0b10 != 0,
                trigger_mm: at(2) / ts,
                press_rt_mm: at(4) / rs,
                release_rt_mm: at(6) / rs,
                other_flags: e[1] & !0b11,
            };
        }
        Ok(map)
    }

    /// Raw values are `mm × scale`. Checks representability only (whole device steps,
    /// 16 bits), not the model's limits: see [`MagneticMap::validate_changes`].
    pub fn encode(&self, rt_precision: u8) -> Result<Vec<u8>> {
        let (ts, rs) = scales(rt_precision);
        let mut out = Vec::with_capacity(MAGNETIC_LEN);
        for (i, k) in self.0.iter().enumerate() {
            out.push(k.axis);
            out.push((k.other_flags & !0b11) | u8::from(k.whole_fast) | (u8::from(k.rampage) << 1));
            for (what, v, scale) in [
                ("point de déclenchement", k.trigger_mm, ts),
                ("sensibilité à l'appui", k.press_rt_mm, rs),
                ("sensibilité au relâchement", k.release_rt_mm, rs),
            ] {
                out.extend_from_slice(&to_raw(i, what, v, scale)?.to_le_bytes());
            }
        }
        Ok(out)
    }

    /// UI limits on every non-empty entry: trigger point within min-max on the model's
    /// step; Rapid Trigger off (both 0) or both sensitivities within the limits of the
    /// key's switch.
    pub fn validate(&self, model: &ModelLimits) -> Result<()> {
        for (i, k) in self.0.iter().enumerate().filter(|(_, k)| !k.is_empty()) {
            check_key(i, k, model)?;
        }
        Ok(())
    }

    /// [`MagneticMap::validate`] restricted to the entries that differ from `baseline`
    /// (the map read from the keyboard): values the keyboard itself holds are not
    /// refused, only edits.
    pub fn validate_changes(&self, baseline: &MagneticMap, model: &ModelLimits) -> Result<()> {
        let changed = self.0.iter().zip(baseline.0.iter()).enumerate();
        for (i, (k, _)) in changed.filter(|(_, (k, b))| k != b && !k.is_empty()) {
            check_key(i, k, model)?;
        }
        Ok(())
    }
}

/// UI limits of one entry (see [`MagneticMap::validate`]).
fn check_key(i: usize, k: &MagneticKey, model: &ModelLimits) -> Result<()> {
    let (min, max, step) = (
        model.min_trigger_trip,
        model.max_trigger_trip,
        model.trigger_trip_step,
    );
    let steps = k.trigger_mm / step;
    if !in_range(k.trigger_mm, min, max) || (steps - steps.round()).abs() > 1e-3 {
        return Err(Error::Validation(format!(
            "Touche {i} : point de déclenchement de {} mm hors limites ({}–{} mm, par pas de {} mm)",
            mm(k.trigger_mm),
            mm2(min),
            mm2(max),
            mm2(step)
        )));
    }
    if k.press_rt_mm == 0.0 && k.release_rt_mm == 0.0 {
        return Ok(()); // Rapid Trigger off
    }
    let axis = model.axis(k.axis).ok_or_else(|| {
        Error::Validation(format!("Touche {i} : type de switch {} inconnu", k.axis))
    })?;
    for (what, v) in [
        ("sensibilité à l'appui", k.press_rt_mm),
        ("sensibilité au relâchement", k.release_rt_mm),
    ] {
        if !in_range(v, axis.min_sensitivity, axis.max_sensitivity) {
            return Err(Error::Validation(format!(
                "Touche {i} : {what} de {} mm hors limites ({}–{} mm)",
                mm(v),
                mm2(axis.min_sensitivity),
                mm2(axis.max_sensitivity)
            )));
        }
    }
    Ok(())
}

/// `performanceConfig` of `model.json` (extra fields ignored).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelLimits {
    /// Default trigger point, mm.
    pub trigger_trip_value: f32,
    pub min_trigger_trip: f32,
    pub max_trigger_trip: f32,
    pub trigger_trip_step: f32,
    /// Switch type used when a key's type is not in `axis_list`.
    pub default_axis_value: u8,
    pub axis_list: Vec<AxisLimits>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisLimits {
    pub name: String,
    pub value: u8,
    pub min_sensitivity: f32,
    pub max_sensitivity: f32,
    /// Default sensitivity, mm.
    pub sensitivity_value: f32,
}

impl ModelLimits {
    /// Parses the `performanceConfig` of a `model.json`.
    pub fn from_model_json(json: &str) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Model {
            performance_config: ModelLimits,
        }
        serde_json::from_str::<Model>(json)
            .map(|m| m.performance_config)
            .map_err(|e| Error::Invalid(format!("model.json: {e}")))
    }

    /// Limits of the AK980 Max (`data/ak980max/model.json`, embedded).
    pub fn ak980max() -> &'static ModelLimits {
        static LIMITS: LazyLock<ModelLimits> = LazyLock::new(|| {
            ModelLimits::from_model_json(include_str!("../../../../data/ak980max/model.json"))
                .expect("embedded data/ak980max/model.json is valid")
        });
        &LIMITS
    }

    /// Limits of switch type `value`, or of the default type when it is unknown.
    pub fn axis(&self, value: u8) -> Option<&AxisLimits> {
        let find = |v: u8| self.axis_list.iter().find(|a| a.value == v);
        find(value).or_else(|| find(self.default_axis_value))
    }
}
