//! Backup files: a [`Snapshot`] as pretty JSON, with the bytes of every block as read.
//!
//! File `ak980max-<pid hex>-<YYYYMMDD-HHMMSS>.json` (local time):
//!
//! ```json
//! { "format": 1, "createdAt": "…", "app": "freejazz 0.1.0",
//!   "snapshot": { …the typed Snapshot, camelCase… },
//!   "raw": { "deviceInfo": "hex", "gameMode": "hex", "keys": "hex", "fnKeys": null,
//!            "ledEffect": "hex", "customLeds": "hex", "macroTable": "hex",
//!            "macroReads": [{ "addr": 400, "bytes": "hex" }, …],
//!            "magnetic": "hex", "dks": "hex" } }
//! ```
//!
//! `raw` is [`Snapshot::raw`]: each block exactly as the keyboard returned it, including
//! what the typed blocks drop (LED bytes 4 and 13-15, custom-LED index bytes, the macro
//! area as the keyboard laid it out); `null` for a snapshot built by the app. A restore
//! writes the typed `snapshot` re-encoded the site's way, not these bytes.
//! [`load_backup`] checks that `raw` decodes to `snapshot` ([`Snapshot::check_raw`]).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::blocks::macros::MACRO_DATA_START;
use crate::error::{Error, Result};
use crate::keyboard::{RawBlocks, Snapshot};

/// Version of the file layout; [`load_backup`] refuses any other.
pub const BACKUP_FORMAT: u32 = 1;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackupFile<'a> {
    format: u32,
    /// Local time of the save, RFC 3339.
    created_at: String,
    /// Program that wrote the file.
    app: String,
    snapshot: &'a Snapshot,
    raw: Option<RawJson>,
}

/// [`RawBlocks`] in the file: lowercase hex.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawJson {
    device_info: String,
    game_mode: String,
    keys: String,
    #[serde(default)]
    fn_keys: Option<String>,
    led_effect: String,
    custom_leds: String,
    macro_table: String,
    #[serde(default)]
    macro_reads: Option<Vec<MacroRead>>,
    /// Files written before the reads were kept: the macro area from address 400, laid
    /// out the site's way. Read only.
    #[serde(default, skip_serializing)]
    macro_data: Option<String>,
    magnetic: String,
    dks: String,
}

#[derive(Serialize, Deserialize)]
struct MacroRead {
    addr: u16,
    bytes: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(block: &str, s: &str) -> std::result::Result<Vec<u8>, String> {
    let bad = || format!("octets « {block} » : hexadécimal invalide");
    if !s.is_ascii() || !s.len().is_multiple_of(2) {
        return Err(bad());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| bad()))
        .collect()
}

impl RawJson {
    fn of(r: &RawBlocks) -> Self {
        Self {
            device_info: hex(&r.device_info),
            game_mode: hex(&r.game_mode),
            keys: hex(&r.keys),
            fn_keys: r.fn_keys.as_deref().map(hex),
            led_effect: hex(&r.led_effect),
            custom_leds: hex(&r.custom_leds),
            macro_table: hex(&r.macro_table),
            macro_reads: Some(
                r.macro_reads
                    .iter()
                    .map(|(addr, b)| MacroRead {
                        addr: *addr,
                        bytes: hex(b),
                    })
                    .collect(),
            ),
            macro_data: None,
            magnetic: hex(&r.magnetic),
            dks: hex(&r.dks),
        }
    }

    fn to_blocks(&self) -> std::result::Result<RawBlocks, String> {
        let macro_reads = match (&self.macro_reads, &self.macro_data) {
            (Some(reads), _) => reads
                .iter()
                .map(|r| Ok((r.addr, unhex("macroReads", &r.bytes)?)))
                .collect::<std::result::Result<_, String>>()?,
            (None, Some(data)) => {
                let data = unhex("macroData", data)?;
                if data.is_empty() {
                    Vec::new()
                } else {
                    vec![(MACRO_DATA_START, data)]
                }
            }
            (None, None) => Vec::new(),
        };
        Ok(RawBlocks {
            device_info: unhex("deviceInfo", &self.device_info)?,
            game_mode: unhex("gameMode", &self.game_mode)?,
            keys: unhex("keys", &self.keys)?,
            fn_keys: self
                .fn_keys
                .as_deref()
                .map(|s| unhex("fnKeys", s))
                .transpose()?,
            led_effect: unhex("ledEffect", &self.led_effect)?,
            custom_leds: unhex("customLeds", &self.custom_leds)?,
            macro_table: unhex("macroTable", &self.macro_table)?,
            macro_reads,
            magnetic: unhex("magnetic", &self.magnetic)?,
            dks: unhex("dks", &self.dks)?,
        })
    }
}

/// Writes `snapshot` to a new file in `dir` (created if missing) and returns its path.
/// Never overwrites: the file is created with `create_new` ([`create_unique`]); a
/// second backup within the same second gets a `-2`, `-3`… suffix (two processes
/// saving at once get two files). The content is written into it and flushed. Refuses a
/// snapshot whose bytes as read (`raw`) no longer describe it (edited after reading).
pub fn save_backup(snapshot: &Snapshot, dir: &Path) -> Result<PathBuf> {
    save_backup_tagged(snapshot, dir, None)
}

/// Tag of the backup taken just before a restore: `…-<time>-avant-restauration.json`.
/// It is listed, but a restore never picks it as « la dernière sauvegarde ».
pub const PRE_RESTORE_TAG: &str = "avant-restauration";

/// [`save_backup`], with `-<tag>` after the time in the file name.
pub fn save_backup_tagged(snapshot: &Snapshot, dir: &Path, tag: Option<&str>) -> Result<PathBuf> {
    if let Some(raw) = &snapshot.raw
        && let Err(block) = snapshot.check_raw(raw)
    {
        return Err(Error::Backup(format!(
            "Sauvegarde refusée : le bloc « {block} » ne correspond plus aux octets lus du \
             clavier (instantané modifié après sa lecture)"
        )));
    }
    let now = chrono::Local::now();
    let file = BackupFile {
        format: BACKUP_FORMAT,
        created_at: now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        app: concat!("freejazz ", env!("CARGO_PKG_VERSION")).to_owned(),
        snapshot,
        raw: snapshot.raw.as_ref().map(RawJson::of),
    };
    let mut text = serde_json::to_string_pretty(&file)
        .map_err(|e| Error::Backup(format!("Sauvegarde impossible à encoder : {e}")))?;
    text.push('\n');

    let io_err = |what: &str, p: &Path, e: std::io::Error| {
        Error::Backup(format!("{what} {} : {e}", p.display()))
    };
    fs::create_dir_all(dir).map_err(|e| io_err("Impossible de créer le dossier", dir, e))?;
    let mut stem = backup_stem(snapshot.info.pid, &now);
    if let Some(tag) = tag {
        stem = format!("{stem}-{tag}");
    }
    let (mut file, path) = create_unique(dir, &stem)
        .map_err(|e| io_err("Impossible de créer la sauvegarde dans", dir, e))?;
    // Written straight into the file this call created (`create_new`): no other file is
    // ever opened, renamed over or removed. A save cut short leaves a file that does
    // not load, which every reader skips; a failed one removes its own file.
    if let Err(e) = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(io_err("Impossible d'écrire la sauvegarde", &path, e));
    }
    Ok(path)
}

/// File name stem of a backup of keyboard `pid` taken at `at`:
/// `ak980max-<pid hex>-<YYYYMMDD-HHMMSS>`.
pub fn backup_stem(pid: u16, at: &chrono::DateTime<chrono::Local>) -> String {
    format!("ak980max-{pid:04x}-{}", at.format("%Y%m%d-%H%M%S"))
}

/// [`backup_stem`] at the current local time.
pub fn backup_stem_now(pid: u16) -> String {
    backup_stem(pid, &chrono::Local::now())
}

/// A backup file name taken apart: the PID, the timestamp `YYYYMMDD-HHMMSS`, the
/// pre-restore tag, and the collision suffix (1 for none, `-2` → 2…). `None` for any
/// other name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BackupName {
    pub stamp: String,
    pub n: u32,
    pub pid: u16,
    /// Taken just before a restore ([`PRE_RESTORE_TAG`]).
    pub pre_restore: bool,
}

pub fn parse_backup_name(name: &str) -> Option<BackupName> {
    let rest = name.strip_prefix("ak980max-")?.strip_suffix(".json")?;
    let (pid, rest) = rest.split_once('-')?;
    if pid.len() != 4 {
        return None;
    }
    let pid = u16::from_str_radix(pid, 16).ok()?;
    let stamp = rest.get(..15)?;
    let mut suffix = rest.get(15..)?;
    let pre_restore = match suffix.strip_prefix("-avant-restauration") {
        Some(after) => {
            suffix = after;
            true
        }
        None => false,
    };
    let n = match suffix {
        "" => 1,
        s => s.strip_prefix('-')?.parse().ok().filter(|&n| n >= 2)?,
    };
    let b = stamp.as_bytes();
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    if b.len() != 15 || b[8] != b'-' || !digits(0..8) || !digits(9..15) {
        return None;
    }
    Some(BackupName {
        stamp: stamp.to_owned(),
        n,
        pid,
        pre_restore,
    })
}

/// Creates `<stem>.json` in `dir`, or `<stem>-2.json`, `<stem>-3.json`… with
/// `create_new`: an existing file is never opened, even one another process created an
/// instant before.
pub fn create_unique(dir: &Path, stem: &str) -> std::io::Result<(fs::File, PathBuf)> {
    for n in 1u32.. {
        let path = match n {
            1 => dir.join(format!("{stem}.json")),
            n => dir.join(format!("{stem}-{n}.json")),
        };
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(f) => return Ok((f, path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    unreachable!("u32 file name suffixes exhausted")
}

/// `%APPDATA%\Freejazz\backups`, where the CLI and the app keep backups.
pub fn default_backup_dir() -> Result<PathBuf> {
    std::env::var_os("APPDATA")
        .map(|a| PathBuf::from(a).join("Freejazz").join("backups"))
        .ok_or_else(|| {
            Error::Backup("Variable APPDATA absente : dossier des sauvegardes inconnu".into())
        })
}

/// Reads the snapshot of a backup file, with its bytes as read in [`Snapshot::raw`].
/// Refuses a file whose `format` is not [`BACKUP_FORMAT`] (or has none) before looking
/// at its snapshot, and a file whose `raw` does not decode to its `snapshot` (damaged
/// or edited by hand).
pub fn load_backup(path: &Path) -> Result<Snapshot> {
    #[derive(Deserialize)]
    struct Header {
        format: Option<u64>,
    }
    #[derive(Deserialize)]
    struct Body {
        snapshot: Snapshot,
        #[serde(default)]
        raw: Option<RawJson>,
    }

    let shown = path.display();
    let text = fs::read_to_string(path)
        .map_err(|e| Error::Backup(format!("Impossible de lire la sauvegarde {shown} : {e}")))?;
    let invalid = |e: String| Error::Backup(format!("Sauvegarde {shown} illisible : {e}"));
    match serde_json::from_str::<Header>(&text)
        .map_err(|e| invalid(e.to_string()))?
        .format
    {
        Some(f) if f == u64::from(BACKUP_FORMAT) => {}
        Some(f) => {
            return Err(Error::Backup(format!(
                "Sauvegarde {shown} : format {f} inconnu (cette version lit le format {BACKUP_FORMAT})"
            )));
        }
        None => {
            return Err(Error::Backup(format!(
                "{shown} n'est pas une sauvegarde Freejazz (champ « format » absent)"
            )));
        }
    }
    let body = serde_json::from_str::<Body>(&text).map_err(|e| invalid(e.to_string()))?;
    let mut snapshot = body.snapshot;
    if let Some(raw) = body.raw {
        let raw = raw.to_blocks().map_err(invalid)?;
        if let Err(block) = snapshot.check_raw(&raw) {
            return Err(Error::Backup(format!(
                "Sauvegarde {shown} incohérente : le bloc « {block} » ne correspond pas aux \
                 octets lus du clavier (fichier abîmé ou modifié à la main)"
            )));
        }
        snapshot.raw = Some(raw);
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_names_parse_and_sort_by_time_then_suffix() {
        let a = parse_backup_name("ak980max-8043-20261025-100349.json").unwrap();
        let b = parse_backup_name("ak980max-8043-20261025-100349-2.json").unwrap();
        assert_eq!(
            (a.pid, a.stamp.as_str(), a.n),
            (0x8043, "20261025-100349", 1)
        );
        assert_eq!(b.n, 2);
        assert!(a < b);
        assert!(!a.pre_restore);
        let p = parse_backup_name("ak980max-8043-20261025-100349-avant-restauration.json").unwrap();
        let p2 =
            parse_backup_name("ak980max-8043-20261025-100349-avant-restauration-2.json").unwrap();
        assert!(p.pre_restore && p2.pre_restore);
        assert_eq!((p.n, p2.n), (1, 2));
        for bad in [
            "ak980max-8043-20261025-100349-1.json",
            "ak980max-8043-2026102-100349.json",
            "ak980max-80430-20261025-100349.json",
            "ak980max-8043-20261025-100349.json.tmp",
            "ak980max-8043-20261025_100349.json",
            "notes.json",
        ] {
            assert_eq!(parse_backup_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn create_unique_never_opens_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("x.json"), "kept").unwrap();
        let (_, p2) = create_unique(dir.path(), "x").unwrap();
        let (_, p3) = create_unique(dir.path(), "x").unwrap();
        assert!(p2.ends_with("x-2.json") && p3.ends_with("x-3.json"));
        assert_eq!(
            fs::read_to_string(dir.path().join("x.json")).unwrap(),
            "kept"
        );
    }
}
