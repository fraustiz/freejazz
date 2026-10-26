//! Backup files of the app: the CLI's format and folder
//! ([`freejazz_protocol::backup`]), `%APPDATA%\Freejazz\backups\ak980max-<pid>-<time>.json`.
//!
//! The model has no USB serial number (protocol.md 9.1), so a keyboard is known by its
//! PID: the first connection of a PID with no backup in the folder takes one. Backups
//! the CLI already wrote there count (same folder, same naming, same format), as long as
//! they load. Every file is created with `create_new`: nothing is ever overwritten.
//! Restoring needs the keyboard: see `device::Request::Restore`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use freejazz_protocol::backup::{
    BackupName, PRE_RESTORE_TAG, backup_stem_now, create_unique, load_backup, parse_backup_name,
    save_backup, save_backup_tagged,
};
use freejazz_protocol::{Error, Result, Snapshot};

use crate::paths;

/// A backup file found in the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub pid: u16,
    /// `YYYYMMDD-HHMMSS`, local time of the save.
    pub stamp: String,
    /// 1, or the `-n` suffix of a second save within the same second.
    pub n: u32,
    /// Taken just before a restore (`…-avant-restauration.json`): never picked as « la
    /// dernière sauvegarde ».
    pub pre_restore: bool,
}

impl BackupInfo {
    /// « 25/10/2026 10:03 ».
    pub fn when(&self) -> String {
        let s = &self.stamp;
        format!(
            "{}/{}/{} {}:{}",
            &s[6..8],
            &s[4..6],
            &s[0..4],
            &s[9..11],
            &s[11..13]
        )
    }
}

/// Backups in `dir`, newest first (by the time in their name). Files with another name
/// are ignored; the files are not opened.
pub fn list_backups_in(dir: &Path) -> Vec<BackupInfo> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(BackupName, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = parse_backup_name(e.file_name().to_str()?)?;
            Some((name, e.path()))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found
        .into_iter()
        .map(|(name, path)| BackupInfo {
            path,
            pid: name.pid,
            stamp: name.stamp,
            n: name.n,
            pre_restore: name.pre_restore,
        })
        .collect()
}

/// Backups in `%APPDATA%\Freejazz\backups`, newest first.
pub fn list_backups() -> Vec<BackupInfo> {
    paths::backups_dir().map_or_else(Vec::new, |d| list_backups_in(&d))
}

/// The newest backup of keyboard `pid` in `dir` that loads (a damaged file is skipped),
/// pre-restore backups left out: « la dernière sauvegarde ».
pub fn latest_backup(dir: &Path, pid: u16) -> Option<(PathBuf, Snapshot)> {
    latest_backup_info(dir, pid).map(|(b, s)| (b.path, s))
}

/// [`latest_backup`], with what its name tells (date…).
pub fn latest_backup_info(dir: &Path, pid: u16) -> Option<(BackupInfo, Snapshot)> {
    list_backups_in(dir)
        .into_iter()
        .filter(|b| b.pid == pid && !b.pre_restore)
        .find_map(|b| {
            let s = load_backup(&b.path).ok()?;
            (s.info.pid == pid).then_some((b, s))
        })
}

/// Saves `snapshot` (what the keyboard holds, read from it) just before a restore, so
/// the restore can be undone.
pub fn save_pre_restore(dir: &Path, snapshot: &Snapshot) -> Result<PathBuf> {
    save_backup_tagged(snapshot, dir, Some(PRE_RESTORE_TAG))
}

/// Backup files of keyboard `pid` in `dir` (pre-restore ones included) that do not
/// load. Empty ones, the name reserved by a save that never finished (the app was
/// killed in between), are removed first: they hold nothing.
pub fn unreadable_backups(dir: &Path, pid: u16) -> usize {
    list_backups_in(dir)
        .into_iter()
        .filter(|b| b.pid == pid)
        .filter(|b| {
            if fs::metadata(&b.path).is_ok_and(|m| m.len() == 0) {
                let _ = fs::remove_file(&b.path);
                return false;
            }
            load_backup(&b.path).is_err()
        })
        .count()
}

/// An automatic backup that was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoBackup {
    pub path: PathBuf,
    /// Files of this keyboard that were there but did not load: « aucune sauvegarde
    /// lisible » before this one.
    pub unreadable: usize,
}

/// Takes the automatic backup of a keyboard seen for the first time: `snapshot` (read
/// from it, with its bytes) is saved unless `dir` already holds a backup of its PID that
/// loads. `Ok(Some(_))` when a file was written.
pub fn ensure_auto_backup(dir: &Path, snapshot: &Snapshot) -> Result<Option<AutoBackup>> {
    let pid = snapshot.info.pid;
    if latest_backup(dir, pid).is_some() {
        return Ok(None);
    }
    let unreadable = unreadable_backups(dir, pid);
    save_backup(snapshot, dir).map(|path| Some(AutoBackup { path, unreadable }))
}

/// French reason why `Restore(None)` finds nothing for `pid` in `dir`.
pub fn no_backup_message(dir: &Path, pid: u16) -> String {
    match unreadable_backups(dir, pid) {
        0 => format!("Aucune sauvegarde de ce clavier dans {}", dir.display()),
        n => format!(
            "Aucune sauvegarde lisible de ce clavier dans {} ({n} fichier{} illisible{})",
            dir.display(),
            if n > 1 { "s" } else { "" },
            if n > 1 { "s" } else { "" }
        ),
    }
}

/// Copies backup `src` to `dest` (a path the user chose), refusing to replace a file.
pub fn export_backup(src: &Path, dest: &Path) -> Result<()> {
    load_backup(src)?;
    let bytes = fs::read(src).map_err(|e| {
        Error::Backup(format!(
            "Impossible de lire la sauvegarde {} : {e}",
            src.display()
        ))
    })?;
    let write = || -> std::io::Result<()> {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dest)?;
        f.write_all(&bytes)?;
        f.sync_all()
    };
    write().map_err(|e| {
        Error::Backup(format!(
            "Exportation impossible vers {} : {e}",
            dest.display()
        ))
    })
}

/// Copies a backup file from elsewhere into `dir`, after checking that it loads. Its
/// name is kept when it is a backup name (`-2`… on collision), otherwise the name says
/// the PID it holds and now.
pub fn import_backup(src: &Path, dir: &Path) -> Result<PathBuf> {
    let snapshot = load_backup(src)?;
    let bytes = fs::read(src).map_err(|e| {
        Error::Backup(format!(
            "Impossible de lire la sauvegarde {} : {e}",
            src.display()
        ))
    })?;
    let stem = src
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(parse_backup_name)
        .filter(|n| n.pid == snapshot.info.pid)
        .map_or_else(
            || backup_stem_now(snapshot.info.pid),
            |n| format!("ak980max-{:04x}-{}", n.pid, n.stamp),
        );
    let io = |e: std::io::Error| {
        Error::Backup(format!(
            "Importation impossible dans {} : {e}",
            dir.display()
        ))
    };
    fs::create_dir_all(dir).map_err(io)?;
    let (mut file, path) = create_unique(dir, &stem).map_err(io)?;
    if let Err(e) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&path);
        return Err(io(e));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read() -> Snapshot {
        crate::mock::read_fixture().unwrap()
    }

    #[test]
    fn lists_newest_first_and_ignores_other_files() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "ak980max-8043-20261024-224845.json",
            "ak980max-8043-20261025-100349.json",
            "ak980max-8043-20261025-100349-2.json",
            "ak980max-8050-20261025-090000.json",
            "notes.txt",
            "ak980max-8043-2026.json",
        ] {
            fs::write(dir.path().join(name), "{}").unwrap();
        }
        let list = list_backups_in(dir.path());
        let names: Vec<_> = list
            .iter()
            .map(|b| b.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "ak980max-8043-20261025-100349-2.json",
                "ak980max-8043-20261025-100349.json",
                "ak980max-8050-20261025-090000.json",
                "ak980max-8043-20261024-224845.json",
            ]
        );
        assert_eq!(list[1].when(), "25/10/2026 10:03");
        assert_eq!(list[2].pid, 0x8050);
    }

    #[test]
    fn latest_skips_pre_restore_backups() {
        let dir = tempfile::tempdir().unwrap();
        let s = read();
        let auto = save_backup(&s, dir.path()).unwrap();
        let pre = save_pre_restore(dir.path(), &s).unwrap();
        assert!(pre.to_string_lossy().contains("-avant-restauration"));
        let list = list_backups_in(dir.path());
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|b| b.pre_restore && b.path == pre));
        assert_eq!(latest_backup(dir.path(), 0x8043).unwrap().0, auto);
    }

    #[test]
    fn a_damaged_file_does_not_count_as_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("ak980max-8043-20261025-100349.json"), "{}").unwrap();
        // a name reserved by a save the app never finished: empty, removed
        let empty = dir.path().join("ak980max-8043-20261025-100350.json");
        fs::write(&empty, "").unwrap();
        assert!(no_backup_message(dir.path(), 0x8043).starts_with("Aucune sauvegarde lisible"));
        assert!(!empty.exists());
        let s = read();
        let saved = ensure_auto_backup(dir.path(), &s).unwrap().unwrap();
        assert_eq!(saved.unreadable, 1, "the damaged file is reported");
        assert_eq!(ensure_auto_backup(dir.path(), &s).unwrap(), None);
        let (path, latest) = latest_backup(dir.path(), 0x8043).unwrap();
        assert_eq!(path, saved.path);
        assert_eq!(latest, s);
        let none = tempfile::tempdir().unwrap();
        assert!(no_backup_message(none.path(), 0x8043).starts_with("Aucune sauvegarde de ce"));
    }

    #[test]
    fn saves_in_the_same_second_never_collide() {
        let dir = tempfile::tempdir().unwrap();
        let s = read();
        // an automatic backup, a pre-restore one and a second save, all at once
        let a = save_backup(&s, dir.path()).unwrap();
        let b = save_pre_restore(dir.path(), &s).unwrap();
        let c = save_backup(&s, dir.path()).unwrap();
        assert!(a != b && b != c && a != c);
        for p in [&a, &b, &c] {
            assert_eq!(load_backup(p).unwrap(), s);
        }
        // nothing else (no temporary file) is left in the folder
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[test]
    fn export_and_import_never_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let s = read();
        let src = save_backup(&s, elsewhere.path()).unwrap();
        let dest = elsewhere.path().join("copie.json");
        export_backup(&src, &dest).unwrap();
        assert!(export_backup(&src, &dest).is_err(), "existing file kept");
        assert_eq!(load_backup(&dest).unwrap(), s);

        // a backup name is kept, with a suffix when the folder already has it
        let name = src.file_name().unwrap().to_str().unwrap().to_owned();
        fs::copy(&src, dir.path().join(&name)).unwrap();
        let c = import_backup(&src, dir.path()).unwrap();
        let stem = src.file_stem().unwrap().to_str().unwrap();
        assert_eq!(
            c.file_name().unwrap().to_str().unwrap(),
            format!("{stem}-2.json")
        );
        // another name gets a backup name; the same file twice makes two files
        let a = import_backup(&dest, dir.path()).unwrap();
        let b = import_backup(&dest, dir.path()).unwrap();
        assert_ne!(a, b);
        assert!(parse_backup_name(a.file_name().unwrap().to_str().unwrap()).is_some());
        assert_eq!(list_backups_in(dir.path()).len(), 4);
        // a file that does not load is refused
        let bad = elsewhere.path().join("bad.json");
        fs::write(&bad, "{\"format\": 1}").unwrap();
        assert!(import_backup(&bad, dir.path()).is_err());
        assert_eq!(list_backups_in(dir.path()).len(), 4);
    }
}
