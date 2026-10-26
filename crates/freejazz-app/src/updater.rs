//! In-app updater: looks for a newer GitHub release, and on request downloads it,
//! checks its SHA-256, and replaces the running executables.
//!
//! Everything slow runs on a short-lived background thread; the UI only polls. Nothing is
//! installed without the click on « Mettre à jour ». Only HTTPS to GitHub's own hosts is
//! used, redirects included. `--mock` / `--offline` / screenshots never reach the network.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Latest-release endpoint of the project.
pub const API_URL: &str = "https://api.github.com/repos/fraustiz/freejazz/releases/latest";
/// The project page, shown in « À propos ».
pub const REPO_URL: &str = "https://github.com/fraustiz/freejazz";
/// Version of this build.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Hosts a request or a redirect may go to.
const ALLOWED_HOSTS: [&str; 4] = [
    "api.github.com",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// Executables of the release archive (app first).
const EXES: [&str; 2] = ["freejazz.exe", "freejazz-cli.exe"];

const CHECK_TIMEOUT: Duration = Duration::from_secs(8);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_API_BYTES: u64 = 1 << 20;
const MAX_ZIP_BYTES: u64 = 64 << 20;
const MAX_EXE_BYTES: u64 = 128 << 20;
const MAX_REDIRECTS: usize = 5;
/// Release notes are cut to this many characters in the panel.
const MAX_NOTES_CHARS: usize = 1500;

// ---- versions and release description -------------------------------------------------

/// `major.minor.patch`; no pre-release or build suffix is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    /// Parses `1.2.3` or `v1.2.3`.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let mut number = || {
            let part = parts.next()?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse::<u64>().ok()
        };
        let version = Self(number()?, number()?, number()?);
        if parts.next().is_some() {
            return None;
        }
        Some(version)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// A release offered for installation.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: Version,
    pub notes: String,
    pub zip_url: String,
    pub sha_url: String,
}

/// Name of the archive of a version.
pub fn zip_name(version: Version) -> String {
    format!("freejazz-v{version}-windows-x86_64.zip")
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
}

/// Reads the JSON of `releases/latest`.
pub fn parse_release(json: &str) -> Result<Release, String> {
    let raw: RawRelease =
        serde_json::from_str(json).map_err(|e| format!("réponse de GitHub illisible : {e}"))?;
    if raw.draft || raw.prerelease {
        return Err("version préliminaire ignorée".into());
    }
    let version = Version::parse(&raw.tag_name)
        .ok_or_else(|| format!("étiquette de version inconnue : {}", raw.tag_name))?;
    let zip = zip_name(version);
    let sha = format!("{zip}.sha256");
    let find = |name: &str| {
        raw.assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.browser_download_url.clone())
            .ok_or_else(|| format!("fichier manquant dans la version : {name}"))
    };
    let (zip_url, sha_url) = (find(&zip)?, find(&sha)?);
    for url in [&zip_url, &sha_url] {
        if !url_allowed(url) {
            return Err(format!("adresse refusée : {url}"));
        }
    }
    let notes = raw.body.unwrap_or_default().replace('\r', "");
    let notes = notes.trim();
    let notes = if notes.chars().count() > MAX_NOTES_CHARS {
        let cut: String = notes.chars().take(MAX_NOTES_CHARS).collect();
        format!("{cut}…")
    } else {
        notes.to_owned()
    };
    Ok(Release {
        version,
        notes,
        zip_url,
        sha_url,
    })
}

/// The release in `json` if it is newer than `current`.
pub fn newer_release(json: &str, current: &str) -> Result<Option<Release>, String> {
    let current = Version::parse(current).ok_or("version actuelle illisible")?;
    let release = parse_release(json)?;
    Ok((release.version > current).then_some(release))
}

/// `https://<allowed host>/...`, no credentials, no port.
pub fn url_allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.contains(['@', ':', '\\']) {
        return false;
    }
    ALLOWED_HOSTS.iter().any(|h| host.eq_ignore_ascii_case(h))
}

// ---- checksum and archive -------------------------------------------------------------

/// The hash in a `.sha256` file (`<64 hex>  <name>`; `file_name`, when the file names
/// one, must be the archive's).
pub fn parse_sha256(text: &str, file_name: &str) -> Result<String, String> {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let mut parts = line.split_whitespace();
    let hash = parts.next().unwrap_or("").to_ascii_lowercase();
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("fichier .sha256 invalide".into());
    }
    if let Some(name) = parts.next()
        && name.trim_start_matches('*') != file_name
    {
        return Err(format!("le .sha256 concerne un autre fichier : {name}"));
    }
    Ok(hash)
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Refuses `data` whose SHA-256 is not `expected` (lowercase hex).
pub fn verify_sha256(data: &[u8], expected: &str) -> Result<(), String> {
    if sha256_hex(data) == expected {
        Ok(())
    } else {
        Err("somme de contrôle incorrecte : téléchargement refusé".into())
    }
}

/// Writes the executables of the archive `zip_bytes` into `dest`.
pub fn extract_exes(zip_bytes: &[u8], dest: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes))
        .map_err(|e| format!("archive illisible : {e}"))?;
    for name in EXES {
        let file = archive
            .by_name(name)
            .map_err(|_| format!("{name} absent de l'archive"))?;
        let mut data = Vec::new();
        file.take(MAX_EXE_BYTES)
            .read_to_end(&mut data)
            .map_err(|e| format!("{name} illisible : {e}"))?;
        if !data.starts_with(b"MZ") {
            return Err(format!("{name} n'est pas un exécutable Windows"));
        }
        std::fs::write(dest.join(name), data).map_err(|e| format!("écriture de {name} : {e}"))?;
    }
    Ok(())
}

// ---- network --------------------------------------------------------------------------

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .max_redirects(0)
        .http_status_as_error(false)
        .https_only(true)
        .user_agent(format!("freejazz-updater/{VERSION}"))
        .build()
        .into()
}

/// GETs `url`, following redirects by hand so each hop is checked.
fn fetch(timeout: Duration, url: &str, limit: u64) -> Result<Vec<u8>, String> {
    let agent = agent(timeout);
    let mut url = url.to_owned();
    for _ in 0..=MAX_REDIRECTS {
        if !url_allowed(&url) {
            return Err(format!("adresse refusée : {url}"));
        }
        let mut response = agent.get(&url).call().map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            url = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .ok_or("redirection sans destination")?
                .to_owned();
            continue;
        }
        if status != 200 {
            return Err(format!("GitHub a répondu {status}"));
        }
        return response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|e| e.to_string());
    }
    Err("trop de redirections".into())
}

fn check_latest() -> Result<Option<Release>, String> {
    let body = fetch(CHECK_TIMEOUT, API_URL, MAX_API_BYTES)?;
    let json = String::from_utf8(body).map_err(|_| "réponse de GitHub illisible")?;
    newer_release(&json, VERSION)
}

/// Downloads and checks `release`, extracts it into a fresh temp folder and returns it.
fn download(release: &Release) -> Result<PathBuf, String> {
    let sha_text = fetch(DOWNLOAD_TIMEOUT, &release.sha_url, 4096)?;
    let expected = parse_sha256(
        &String::from_utf8_lossy(&sha_text),
        &zip_name(release.version),
    )?;
    let zip = fetch(DOWNLOAD_TIMEOUT, &release.zip_url, MAX_ZIP_BYTES)?;
    verify_sha256(&zip, &expected)?;
    let dir = std::env::temp_dir().join(format!("freejazz-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("dossier temporaire : {e}"))?;
    if let Err(e) = extract_exes(&zip, &dir) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(dir)
}

// ---- installation ---------------------------------------------------------------------

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Puts `new` at `target`: a running executable cannot be overwritten but can be
/// renamed, so the old file steps aside (`.old`, deleted at the next start) first.
pub fn replace_file(new: &Path, target: &Path) -> Result<(), String> {
    let (copy, old) = (with_suffix(target, ".new"), with_suffix(target, ".old"));
    std::fs::copy(new, &copy).map_err(|e| format!("copie de la nouvelle version : {e}"))?;
    let _ = std::fs::remove_file(&old);
    if let Err(e) = std::fs::rename(target, &old) {
        let _ = std::fs::remove_file(&copy);
        return Err(format!("remplacement impossible : {e}"));
    }
    if let Err(e) = std::fs::rename(&copy, target) {
        let _ = std::fs::rename(&old, target);
        let _ = std::fs::remove_file(&copy);
        return Err(format!("remplacement impossible : {e}"));
    }
    Ok(())
}

/// Replaces the running app with the one in `staged`, and the CLI beside it when there is
/// one (a CLI in use is left as it was).
pub fn install(staged: &Path, exe: &Path) -> Result<(), String> {
    replace_file(&staged.join(EXES[0]), exe)?;
    if let Some(cli) = exe.parent().map(|d| d.join(EXES[1]))
        && cli.exists()
    {
        let _ = replace_file(&staged.join(EXES[1]), &cli);
    }
    let _ = std::fs::remove_dir_all(staged);
    Ok(())
}

/// Deletes the `.old` files an earlier update left beside the executable.
pub fn cleanup_old() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = std::fs::remove_file(with_suffix(&exe, ".old"));
    if let Some(dir) = exe.parent() {
        let _ = std::fs::remove_file(dir.join(format!("{}.old", EXES[1])));
    }
}

/// Installs `staged` over the running executable, then starts the app again; it waits
/// for this process to end before taking the single-instance lock.
pub fn install_and_relaunch(staged: &Path) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = install(staged, &exe);
    let _ = std::process::Command::new(&exe)
        .arg("--relaunch-after")
        .arg(std::process::id().to_string())
        .spawn();
}

/// `--relaunch-after <pid>`: waits (at most 15 s) for that process to end.
pub fn wait_for_exit(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };
    // SAFETY: plain handle calls; the handle is closed.
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !handle.is_null() {
            WaitForSingleObject(handle, 15_000);
            CloseHandle(handle);
        }
    }
}

/// Opens `url` in the default browser.
pub fn open_url(url: &str) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .spawn();
}

// ---- state driven by the UI -----------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Default)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(Release),
}

enum Msg {
    Checked(Result<Option<Release>, String>),
    Staged(Result<PathBuf, String>),
}

/// Where the updater is; the app polls it every frame.
#[derive(Default)]
pub struct Updater {
    /// False in `--mock`, `--offline` and screenshots: nothing is ever sent.
    enabled: bool,
    pub state: UpdateState,
    /// The last failure to show (a silent check never sets it).
    pub error: Option<String>,
    manual: bool,
    rx: Option<Receiver<Msg>>,
    ctx: Option<eframe::egui::Context>,
}

impl Updater {
    pub fn new(enabled: bool, ctx: eframe::egui::Context) -> Self {
        Self {
            enabled,
            ctx: Some(ctx),
            ..Self::default()
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn busy(&self) -> bool {
        matches!(
            self.state,
            UpdateState::Checking | UpdateState::Downloading(_)
        )
    }

    fn spawn(&mut self, work: impl FnOnce(&Sender<Msg>) + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let ctx = self.ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("updater".into())
            .spawn(move || {
                work(&tx);
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            });
        if spawned.is_err() {
            self.rx = None;
            self.state = UpdateState::Idle;
        }
    }

    /// Looks for a newer release in the background. `manual`: a failure is shown.
    pub fn check(&mut self, manual: bool) {
        if !self.enabled || self.busy() {
            return;
        }
        self.manual = manual;
        self.error = None;
        self.state = UpdateState::Checking;
        self.spawn(|tx| {
            let _ = tx.send(Msg::Checked(check_latest()));
        });
    }

    /// The user asked for the update: download, verify and unpack in the background.
    pub fn start_update(&mut self) {
        let UpdateState::Available(release) = self.state.clone() else {
            return;
        };
        if !self.enabled {
            return;
        }
        self.error = None;
        self.state = UpdateState::Downloading(release.clone());
        self.spawn(move |tx| {
            let _ = tx.send(Msg::Staged(download(&release)));
        });
    }

    /// Takes in what the background thread reported; `Some(folder)` once an update is
    /// downloaded, checked and ready to be installed.
    pub fn poll(&mut self) -> Option<PathBuf> {
        let rx = self.rx.as_ref()?;
        let msg = rx.try_recv().ok()?;
        self.rx = None;
        match msg {
            Msg::Checked(Ok(Some(release))) => self.state = UpdateState::Available(release),
            Msg::Checked(Ok(None)) => self.state = UpdateState::UpToDate,
            Msg::Checked(Err(e)) => {
                self.state = UpdateState::Idle;
                if self.manual {
                    self.error = Some(format!("Recherche impossible : {e}"));
                }
            }
            Msg::Staged(Ok(dir)) => return Some(dir),
            Msg::Staged(Err(e)) => {
                if let UpdateState::Downloading(release) = std::mem::take(&mut self.state) {
                    self.state = UpdateState::Available(release);
                }
                self.error = Some(format!("Mise à jour impossible : {e}"));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn release_json(tag: &str, with_sha: bool) -> String {
        let v = Version::parse(tag).unwrap();
        let zip = zip_name(v);
        let mut assets = vec![format!(
            r#"{{"name":"{zip}","browser_download_url":"https://github.com/fraustiz/freejazz/releases/download/{tag}/{zip}"}}"#
        )];
        if with_sha {
            assets.push(format!(
                r#"{{"name":"{zip}.sha256","browser_download_url":"https://github.com/fraustiz/freejazz/releases/download/{tag}/{zip}.sha256"}}"#
            ));
        }
        format!(
            r#"{{"tag_name":"{tag}","body":"Notes\r\nligne","draft":false,"prerelease":false,"assets":[{}]}}"#,
            assets.join(",")
        )
    }

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(Version::parse("v0.1.0"), Some(Version(0, 1, 0)));
        assert_eq!(Version::parse("1.20.3"), Some(Version(1, 20, 3)));
        for bad in [
            "",
            "v1",
            "1.2",
            "1.2.3.4",
            "1.2.x",
            "v1.2.3-rc1",
            "1.2.3+b",
            "-1.0.0",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        assert!(Version(1, 0, 0) > Version(0, 99, 99));
        assert_eq!(Version(0, 1, 0).to_string(), "0.1.0");
    }

    #[test]
    fn only_a_newer_release_is_offered() {
        let json = release_json("v0.2.0", true);
        let r = newer_release(&json, "0.1.0").unwrap().unwrap();
        assert_eq!(r.version, Version(0, 2, 0));
        assert_eq!(r.notes, "Notes\nligne");
        assert!(r.zip_url.ends_with("freejazz-v0.2.0-windows-x86_64.zip"));
        assert!(
            r.sha_url
                .ends_with("freejazz-v0.2.0-windows-x86_64.zip.sha256")
        );
        assert_eq!(newer_release(&json, "0.2.0").unwrap(), None);
        assert_eq!(newer_release(&json, "0.3.1").unwrap(), None);
    }

    #[test]
    fn release_description_is_checked() {
        // checksum asset missing
        assert!(parse_release(&release_json("v0.2.0", false)).is_err());
        // not JSON, odd tag, draft
        assert!(parse_release("nope").is_err());
        assert!(parse_release(r#"{"tag_name":"nightly","assets":[]}"#).is_err());
        let draft = release_json("v0.2.0", true).replace(r#""draft":false"#, r#""draft":true"#);
        assert!(parse_release(&draft).is_err());
        // asset hosted somewhere else
        let other = release_json("v0.2.0", true).replace("github.com", "example.org");
        assert!(parse_release(&other).is_err());
    }

    #[test]
    fn urls_are_limited_to_github_over_https() {
        for ok in [
            "https://github.com/fraustiz/freejazz/releases/download/v1.0.0/a.zip",
            "https://objects.githubusercontent.com/x?y=1",
            "https://release-assets.githubusercontent.com/a",
            API_URL,
        ] {
            assert!(url_allowed(ok), "{ok}");
        }
        for bad in [
            "http://github.com/a",
            "https://github.com.evil.org/a",
            "https://evil.org/github.com/a",
            "https://github.com@evil.org/a",
            "https://github.com:8443/a",
            "https://evilgithub.com/a",
            "ftp://github.com/a",
            "github.com/a",
            "https://github.com\\@evil.org",
        ] {
            assert!(!url_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn sha_file_is_parsed() {
        let hash = "AB".repeat(32);
        let name = zip_name(Version(0, 1, 0));
        let lower = "ab".repeat(32);
        assert_eq!(
            parse_sha256(&format!("{hash}  {name}\n"), &name).unwrap(),
            lower
        );
        assert_eq!(parse_sha256(&format!("{hash}\n"), &name).unwrap(), lower);
        assert_eq!(
            parse_sha256(&format!("{hash} *{name}"), &name).unwrap(),
            lower
        );
        assert!(parse_sha256(&format!("{hash}  other.zip"), &name).is_err());
        assert!(parse_sha256("abc  x.zip", "x.zip").is_err());
        assert!(parse_sha256(&format!("{}zz", "a".repeat(62)), "x").is_err());
        assert!(parse_sha256("", "x").is_err());
    }

    #[test]
    fn a_wrong_hash_is_refused() {
        let data = b"freejazz";
        let good = sha256_hex(data);
        assert_eq!(good.len(), 64);
        assert!(verify_sha256(data, &good).is_ok());
        assert!(verify_sha256(b"freejazz!", &good).is_err());
        assert!(verify_sha256(data, &"0".repeat(64)).is_err());
        // known vector
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    fn make_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(&mut out);
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn executables_are_extracted_and_checked() {
        let dir = tempfile::tempdir().unwrap();
        let ok = make_zip(&[
            ("freejazz.exe", b"MZapp"),
            ("freejazz-cli.exe", b"MZcli"),
            ("README.md", b"x"),
        ]);
        extract_exes(&ok, dir.path()).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("freejazz.exe")).unwrap(),
            b"MZapp"
        );
        assert_eq!(
            std::fs::read(dir.path().join("freejazz-cli.exe")).unwrap(),
            b"MZcli"
        );
        let missing = make_zip(&[("freejazz.exe", b"MZapp")]);
        assert!(extract_exes(&missing, dir.path()).is_err());
        let not_exe = make_zip(&[("freejazz.exe", b"zz"), ("freejazz-cli.exe", b"MZ")]);
        assert!(extract_exes(&not_exe, dir.path()).is_err());
        assert!(extract_exes(b"not a zip", dir.path()).is_err());
    }

    #[test]
    fn install_swaps_the_files_and_keeps_the_old_ones_aside() {
        let root = tempfile::tempdir().unwrap();
        let (staged, app) = (root.path().join("staged"), root.path().join("app"));
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::create_dir_all(&app).unwrap();
        std::fs::write(staged.join("freejazz.exe"), b"MZnew").unwrap();
        std::fs::write(staged.join("freejazz-cli.exe"), b"MZnewcli").unwrap();
        std::fs::write(app.join("freejazz.exe"), b"MZold").unwrap();
        std::fs::write(app.join("freejazz-cli.exe"), b"MZoldcli").unwrap();
        install(&staged, &app.join("freejazz.exe")).unwrap();
        assert_eq!(std::fs::read(app.join("freejazz.exe")).unwrap(), b"MZnew");
        assert_eq!(
            std::fs::read(app.join("freejazz-cli.exe")).unwrap(),
            b"MZnewcli"
        );
        assert_eq!(
            std::fs::read(app.join("freejazz.exe.old")).unwrap(),
            b"MZold"
        );
        assert!(!staged.exists());
    }

    #[test]
    fn a_failed_swap_keeps_the_old_file() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("freejazz.exe");
        std::fs::write(&target, b"MZold").unwrap();
        // the new file does not exist: nothing is touched
        assert!(replace_file(&root.path().join("missing"), &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"MZold");
    }

    #[test]
    fn a_disabled_updater_does_nothing() {
        let mut u = Updater::default();
        u.check(true);
        assert_eq!(u.state, UpdateState::Idle);
        assert!(!u.enabled());
        assert_eq!(u.poll(), None);
        u.start_update();
        assert_eq!(u.state, UpdateState::Idle);
    }
}
