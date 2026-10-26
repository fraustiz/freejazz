//! App profiles: named snapshots of the keyboard kept on the PC
//! (`%APPDATA%\Freejazz\profiles\<name>.json`, the backup file format, so a profile is
//! also a backup). The keyboard of this unit has no hardware profiles (`max_profile`
//! 0); applying a profile goes through the device manager (`Request::ApplyProfile`:
//! backup first, `write_snapshot`, read back).

use std::fs;
use std::path::{Path, PathBuf};

use freejazz_protocol::backup::{load_backup, save_backup};
use freejazz_protocol::{Error, Result, Snapshot};

/// Longest profile name.
pub const MAX_NAME: usize = 40;

/// Staging folder of a save (the backup writer picks its own file name).
const STAGING: &str = ".enregistrement";

/// The name as stored: trimmed; `Err` (French) when it cannot be a file name.
pub fn clean_name(name: &str) -> std::result::Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Donnez un nom au profil.".into());
    }
    if name.chars().count() > MAX_NAME {
        return Err(format!("Nom trop long ({MAX_NAME} caractères au plus)."));
    }
    if let Some(c) = name
        .chars()
        .find(|c| c.is_control() || r#"\/:*?"<>|"#.contains(*c))
    {
        return Err(format!(
            "Le caractère « {c} » n'est pas permis dans un nom."
        ));
    }
    if name.ends_with('.') {
        return Err("Le nom ne peut pas finir par un point.".into());
    }
    if crate::designs::is_reserved_name(name) {
        return Err(format!("« {name} » est un nom réservé par Windows."));
    }
    Ok(name.to_owned())
}

pub fn profile_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(format!("{name}.json"))
}

/// Profile names, sorted (case-insensitive).
pub fn list_profiles(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let file = e.file_name().into_string().ok()?;
            let name = file.strip_suffix(".json")?;
            clean_name(name).ok().filter(|n| n == name)
        })
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

/// Saves `snapshot` (as read from the keyboard) as profile `name`, replacing a profile
/// of that name.
pub fn save_profile(dir: &Path, name: &str, snapshot: &Snapshot) -> Result<PathBuf> {
    let name = clean_name(name).map_err(Error::Backup)?;
    let staging = dir.join(STAGING);
    let staged = save_backup(snapshot, &staging)?;
    let path = profile_path(dir, &name);
    let moved = fs::rename(&staged, &path);
    let _ = fs::remove_dir(&staging);
    moved.map_err(|e| Error::Backup(format!("Impossible d'écrire {} : {e}", path.display())))?;
    Ok(path)
}

pub fn load_profile(dir: &Path, name: &str) -> Result<Snapshot> {
    load_backup(&profile_path(dir, name))
}

pub fn delete_profile(dir: &Path, name: &str) -> Result<()> {
    let path = profile_path(dir, name);
    fs::remove_file(&path)
        .map_err(|e| Error::Backup(format!("Impossible de supprimer {} : {e}", path.display())))
}

/// Where the profiles are (the mock keeps its own, in a temporary folder).
pub fn profiles_dir(mock: bool) -> Option<PathBuf> {
    if mock {
        Some(std::env::temp_dir().join("freejazz-mock-profiles"))
    } else {
        crate::paths::profiles_dir()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::read_fixture;

    fn json(s: &Snapshot) -> serde_json::Value {
        serde_json::to_value(s).unwrap()
    }

    #[test]
    fn profile_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        // as read from the (fake) keyboard: its bytes travel with it
        let snap = read_fixture().unwrap();
        let path = save_profile(dir.path(), "  Jeu  ", &snap).unwrap();
        assert_eq!(path, dir.path().join("Jeu.json"));
        assert_eq!(list_profiles(dir.path()), ["Jeu"]);
        assert!(!dir.path().join(STAGING).exists());

        let back = load_profile(dir.path(), "Jeu").unwrap();
        assert_eq!(json(&back), json(&snap));

        // same name: replaced, still one profile
        save_profile(dir.path(), "Jeu", &snap).unwrap();
        assert_eq!(list_profiles(dir.path()), ["Jeu"]);

        save_profile(dir.path(), "bureau", &snap).unwrap();
        assert_eq!(list_profiles(dir.path()), ["bureau", "Jeu"]);
        delete_profile(dir.path(), "Jeu").unwrap();
        assert_eq!(list_profiles(dir.path()), ["bureau"]);
        assert!(delete_profile(dir.path(), "Jeu").is_err());
    }

    #[test]
    fn names_are_checked() {
        assert_eq!(clean_name(" Travail "), Ok("Travail".into()));
        assert!(clean_name("").is_err());
        assert!(clean_name("a/b").is_err());
        assert!(clean_name("CON:").is_err());
        for reserved in ["CON", "nul", "COM1", "lpt3", "AUX.bak"] {
            assert!(clean_name(reserved).is_err(), "{reserved}");
        }
        assert!(clean_name("Console").is_ok());
        assert!(clean_name("fin.").is_err());
        assert!(clean_name(&"x".repeat(41)).is_err());
        assert!(
            save_profile(
                tempfile::tempdir().unwrap().path(),
                "a?b",
                &read_fixture().unwrap()
            )
            .is_err()
        );
    }
}
