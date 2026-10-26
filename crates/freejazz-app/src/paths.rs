//! Where the app keeps its data: `%APPDATA%\Freejazz\` (`settings.json`, `profiles\`,
//! `designs\`, `backups\`). The backups folder is the CLI's too
//! ([`freejazz_protocol::backup::default_backup_dir`]).

use std::path::PathBuf;

/// `%APPDATA%\Freejazz`; `None` without `APPDATA`.
pub fn app_data_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Freejazz"))
}

/// `%APPDATA%\Freejazz\backups`.
pub fn backups_dir() -> Option<PathBuf> {
    freejazz_protocol::backup::default_backup_dir().ok()
}

pub fn settings_file() -> Option<PathBuf> {
    app_data_dir().map(|d| d.join("settings.json"))
}

pub fn profiles_dir() -> Option<PathBuf> {
    app_data_dir().map(|d| d.join("profiles"))
}

pub fn designs_dir() -> Option<PathBuf> {
    app_data_dir().map(|d| d.join("designs"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_lives_under_appdata_freejazz() {
        let Some(root) = app_data_dir() else {
            return; // no APPDATA on this machine
        };
        assert!(root.ends_with("Freejazz"));
        assert_eq!(backups_dir(), Some(root.join("backups")));
        assert_eq!(settings_file(), Some(root.join("settings.json")));
        assert_eq!(profiles_dir(), Some(root.join("profiles")));
        assert_eq!(designs_dir(), Some(root.join("designs")));
    }
}
