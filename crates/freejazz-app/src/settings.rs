//! The app's own options, kept in `%APPDATA%\Freejazz\settings.json`: closing to the
//! notification area, launch with Windows, and the live effect that was running.
//!
//! The same file holds the crash marker of a running live stream (`liveEffect`, written
//! by the device worker). Both writers go through [`update_file`]: one lock, and each
//! changes only its own keys of what the file holds, so neither drops the other's.

use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Every read-modify-write of the settings file of this process.
static FILE_LOCK: Mutex<()> = Mutex::new(());

/// Key of the crash marker: a live stream runs (its end has not been written yet).
pub const MARKER_KEY: &str = "liveEffect";

/// Changes the JSON object in `path` with `change` (the file is created if missing),
/// under the settings lock. A file that exists but is not a JSON object is replaced only
/// when `replace_unreadable`; otherwise left alone. `change` returns whether it changed
/// anything (nothing is written otherwise).
pub fn update_file(
    path: &Path,
    replace_unreadable: bool,
    change: impl FnOnce(&mut Map<String, Value>) -> bool,
) -> std::io::Result<()> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut doc = match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => map,
            _ if replace_unreadable => Map::new(),
            _ => return Ok(()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Map::new(),
        Err(e) => return Err(e),
    };
    if !change(&mut doc) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(doc)).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

/// The crash marker in the settings file `path`, if a stream is marked as running.
pub fn live_marker_in(path: &Path) -> Option<String> {
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let text = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    v.get(MARKER_KEY)?.as_str().map(str::to_owned)
}

/// Sets (or clears) the crash marker in `path`, keeping every other setting. A settings
/// file that exists but cannot be parsed is left alone.
pub fn set_live_marker_in(path: &Path, value: Option<&str>) {
    let _ = update_file(path, false, |doc| match value {
        Some(v) => {
            doc.insert(MARKER_KEY.into(), Value::String(v.to_owned()));
            true
        }
        None => doc.remove(MARKER_KEY).is_some(),
    });
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// The close button hides the window in the notification area.
    pub close_to_tray: bool,
    /// Launch with Windows (`HKCU\...\Run`, with `--tray`).
    pub autostart: bool,
    /// The last live effect started (card id or saved effect name).
    pub last_live_effect: Option<String>,
    /// A live effect was running at the last change.
    pub live_running: bool,
    /// Look for a newer release at startup.
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            autostart: false,
            last_live_effect: None,
            live_running: false,
            check_updates: true,
        }
    }
}

impl Settings {
    /// Reads `path`; missing or unreadable gives the defaults.
    pub fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn load() -> Self {
        crate::paths::settings_file()
            .map(|p| Self::load_from(&p))
            .unwrap_or_default()
    }

    /// Writes these options into `path`, keeping the keys they do not own (the crash
    /// marker): see [`update_file`].
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let Value::Object(own) = serde_json::to_value(self).map_err(std::io::Error::other)? else {
            return Err(std::io::Error::other("settings are not an object"));
        };
        update_file(path, true, |doc| {
            doc.extend(own);
            true
        })
    }

    pub fn save(&self) -> std::io::Result<()> {
        match crate::paths::settings_file() {
            Some(p) => self.save_to(&p),
            None => Ok(()),
        }
    }

    /// Records what the live engine is running (`None`: nothing); `true` when the
    /// settings changed and need saving.
    pub fn note_live(&mut self, running: Option<&str>) -> bool {
        let mut changed = false;
        if let Some(name) = running
            && self.last_live_effect.as_deref() != Some(name)
        {
            self.last_live_effect = Some(name.to_owned());
            changed = true;
        }
        let on = running.is_some();
        if self.live_running != on {
            self.live_running = on;
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_values() {
        let s = Settings::default();
        assert!(!s.autostart);
        assert!(s.close_to_tray);
        assert_eq!(s.last_live_effect, None);
        assert!(!s.live_running);
        assert!(s.check_updates);
    }

    #[test]
    fn settings_roundtrip_and_tolerate_gaps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("settings.json");
        assert_eq!(Settings::load_from(&path), Settings::default());
        let s = Settings {
            close_to_tray: false,
            autostart: true,
            last_live_effect: Some("wave".into()),
            live_running: true,
            check_updates: false,
        };
        s.save_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("closeToTray") && text.contains("lastLiveEffect"));
        assert_eq!(Settings::load_from(&path), s);
        std::fs::write(&path, r#"{"autostart": true}"#).unwrap();
        let partial = Settings::load_from(&path);
        assert!(partial.autostart && partial.close_to_tray && partial.check_updates);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(Settings::load_from(&path), Settings::default());
    }

    #[test]
    fn saving_the_options_keeps_the_live_marker() {
        // the worker marks a stream, then the app saves its options (live_running…)
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        set_live_marker_in(&path, Some("en cours"));
        let mut s = Settings::default();
        assert!(s.note_live(Some("wave")));
        s.save_to(&path).unwrap();
        assert_eq!(live_marker_in(&path).as_deref(), Some("en cours"));
        assert_eq!(Settings::load_from(&path), s);
        // the stream ends: the marker goes, the options stay
        set_live_marker_in(&path, None);
        assert_eq!(live_marker_in(&path), None);
        assert_eq!(Settings::load_from(&path), s);
        // an unreadable file is never touched by the marker, replaced by a save
        std::fs::write(&path, "not json").unwrap();
        set_live_marker_in(&path, Some("x"));
        assert_eq!(std::fs::read(&path).unwrap(), b"not json");
        s.save_to(&path).unwrap();
        assert_eq!(Settings::load_from(&path), s);
    }

    #[test]
    fn note_live_reports_changes_once() {
        let mut s = Settings::default();
        assert!(!s.note_live(None));
        assert!(s.note_live(Some("wave")));
        assert!(!s.note_live(Some("wave")));
        assert!(s.note_live(None));
        assert_eq!(s.last_live_effect.as_deref(), Some("wave"));
        assert!(!s.live_running);
    }
}
