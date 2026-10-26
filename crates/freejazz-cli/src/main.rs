//! `freejazz-cli`: info, backup, block dumps, restore and the write self-test for the
//! Ajazz AK980 Max.
//!
//! The link to the keyboard is wrapped in [`Guarded`], which refuses before sending any
//! command outside the list of the running subcommand: `info`, `backup` and `dump` can
//! only send the block reads the site sends to this model at connection
//! (protocol.md 5). `devices` sends nothing at all. `restore` and `selftest-writes` add
//! the site's block writes (module [`selftest`]); `stream-test` the live stream 0x32 and
//! `SET_LED_EFFECT`, `depth-test` the key status 0x68 (module [`live`]).

mod live;
mod selftest;

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::LazyLock;

use clap::{Parser, Subcommand, ValueEnum};
use hidapi::HidApi;
use serde::Deserialize;

use freejazz_protocol::backup::{load_backup, save_backup};
use freejazz_protocol::blocks::device_info::DEVICE_INFO_LEN;
use freejazz_protocol::blocks::dks::DKS_LEN;
use freejazz_protocol::blocks::game_mode::GAME_MODE_LEN;
use freejazz_protocol::blocks::keys::KEY_MAP_LEN;
use freejazz_protocol::blocks::led::{CUSTOM_LEDS_LEN, LED_EFFECT_LEN};
use freejazz_protocol::blocks::macros::{MACRO_DATA_START, MACRO_TABLE_LEN};
use freejazz_protocol::blocks::magnetic::MAGNETIC_LEN;
use freejazz_protocol::blocks::{
    CustomLeds, DksTable, GameMode, KeyAction, KeyKind, KeyMap, LedEffect, MACRO_CAPACITY, Macros,
    MagneticKey, MagneticMap, Rgb,
};
use freejazz_protocol::cmd::Cmd;
use freejazz_protocol::reconnect::{Wait, reconnect_with};
use freejazz_protocol::transport::{
    DeviceIdent, HidLink, HidapiLink, ReportKind, is_site_usage_page, open_keyboard, report_items,
    site_report_len,
};
use freejazz_protocol::{Error, Keyboard, Result};

// ---------- command line ----------

#[derive(Parser)]
#[command(
    name = "freejazz-cli",
    version,
    about = "Outil en ligne de commande pour le clavier Ajazz AK980 Max"
)]
struct Cli {
    /// Affiche chaque rapport HID envoyé (→) et reçu (←)
    #[arg(long, global = true)]
    trace: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Liste les interfaces HID du clavier et la taille de leurs rapports, sans rien lui envoyer
    Devices,
    /// Lit et affiche les informations du clavier (GET_DEVICE_INFO)
    Info,
    /// Lit tout le clavier (comme le site à la connexion) et écrit une sauvegarde JSON
    Backup {
        /// Dossier de destination (par défaut %APPDATA%\Freejazz\backups)
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Lit un bloc et l'affiche : valeurs décodées, octets lus, contrôle de réécriture
    Dump {
        block: Block,
        /// Autorise la lecture de la couche Fn (0x16), que le site n'envoie jamais à ce modèle
        #[arg(long)]
        unverified: bool,
    },
    /// Réécrit une sauvegarde dans le clavier, puis la relit pour vérifier
    Restore {
        file: PathBuf,
        /// Confirme l'écriture (sans cette option, rien n'est envoyé)
        #[arg(long)]
        yes: bool,
    },
    /// Essaie chaque écriture de bloc sur le clavier : même valeur, une valeur modifiée,
    /// restauration, chacune relue (sauvegarde complète avant toute écriture)
    SelftestWrites {
        /// Confirme les écritures (sans cette option, rien n'est envoyé)
        #[arg(long)]
        yes: bool,
        /// Dossier où copier aussi les sauvegardes (en plus de %APPDATA%\Freejazz\backups)
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Envoie un arc-en-ciel en direct (flux 0x32 du mode musique du site), mesure les
    /// images/s, lit l'effet après l'arrêt du flux, puis remet l'effet d'une sauvegarde
    StreamTest {
        /// Durée du flux, en secondes
        #[arg(long, default_value_t = 10.0)]
        seconds: f32,
        /// Images par seconde visées (0 : aussi vite que le clavier répond)
        #[arg(long, default_value_t = 30.0)]
        fps: f32,
        /// Sauvegarde dont l'effet lumineux est remis après le flux
        #[arg(long)]
        backup: Option<PathBuf>,
        /// Secondes de lecture de GET_LED_EFFECT après l'arrêt du flux, avant de remettre l'effet
        #[arg(long, default_value_t = 10.0)]
        watch: f32,
        /// Ne remet pas l'effet après le flux
        #[arg(long)]
        no_restore: bool,
        /// Confirme l'envoi (sans cette option, rien n'est envoyé)
        #[arg(long)]
        yes: bool,
    },
    /// Lit la course des touches en boucle (0x68, jamais envoyée par le site à ce modèle)
    DepthTest {
        /// Durée, en secondes
        #[arg(long, default_value_t = 5.0)]
        seconds: f32,
        /// Autorise 0x68, que le site n'envoie pas à ce modèle (à vérifier sur le matériel)
        #[arg(long)]
        unverified: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Block {
    GameMode,
    Keys,
    FnKeys,
    Led,
    CustomLeds,
    Macros,
    Magnetic,
    Dks,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let t = cli.trace;
    let result = match cli.command {
        Command::Devices => devices().map(|()| ExitCode::SUCCESS),
        Command::Info => info(t).map(|()| ExitCode::SUCCESS),
        Command::Backup { dir } => backup(dir, t).map(|()| ExitCode::SUCCESS),
        Command::Dump { block, unverified } => dump(block, unverified, t),
        Command::Restore { file, yes } => restore(&file, yes, t),
        Command::SelftestWrites { yes, dir } => selftest::command(yes, dir, t),
        Command::StreamTest {
            seconds,
            fps,
            backup,
            watch,
            no_restore,
            yes,
        } => live::stream_test(
            live::StreamArgs {
                seconds,
                fps,
                backup: backup.as_deref(),
                watch,
                no_restore,
                yes,
            },
            t,
        ),
        Command::DepthTest {
            seconds,
            unverified,
        } => live::depth_test(seconds, unverified, t),
    };
    result.unwrap_or_else(|e| {
        eprintln!("Erreur : {e}");
        if matches!(e, Error::NotFound) {
            eprintln!(
                "Clavier introuvable (VID 0C45, PID 8043 ou 8050–8054) : est-il branché en USB ?"
            );
        }
        ExitCode::FAILURE
    })
}

// ---------- guarded link ----------

/// Block reads the site sends to this model at connection (protocol.md 5).
const SITE_READS: &[u8] = &[0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x17, 0x18];
/// The same plus `GET_FN_KEY` 0x16, never sent by the site to this model.
const READS_WITH_FN: &[u8] = &[0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18];
/// A restore: the reads, then the block writes of `write_snapshot` (no Fn layer).
const RESTORE: &[u8] = &[
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x17, 0x18, 0x21, 0x22, 0x23, 0x24, 0x25, 0x27, 0x28,
];

/// Wraps the HID link: refuses any report that is not a request for one of `allowed`
/// before it is sent, counts what is sent, the waits that ended without a report (each
/// one makes the transfer re-send or give up) and the lengths of the reports received,
/// and prints them with `--trace`. Prints what was sent when dropped.
struct Guarded<L: HidLink> {
    inner: L,
    allowed: &'static [u8],
    trace: bool,
    sent: BTreeMap<u8, usize>,
    timeouts: usize,
    read_lens: BTreeSet<usize>,
}

impl<L: HidLink> Guarded<L> {
    fn new(inner: L, allowed: &'static [u8], trace: bool) -> Self {
        Self {
            inner,
            allowed,
            trace,
            sent: BTreeMap::new(),
            timeouts: 0,
            read_lens: BTreeSet::new(),
        }
    }

    /// Reports sent through this link.
    fn total_sent(&self) -> usize {
        self.sent.values().sum()
    }
}

impl<L: HidLink> HidLink for Guarded<L> {
    fn write(&mut self, report: &[u8]) -> Result<()> {
        let cmd = report.get(1).copied().unwrap_or(0);
        if report.first() != Some(&0xAA) || !self.allowed.contains(&cmd) {
            return Err(Error::Denied(cmd));
        }
        if self.trace {
            eprintln!("→ {}", spaced(report));
        }
        self.inner.write(report)?;
        *self.sent.entry(cmd).or_default() += 1;
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8], timeout_ms: i32) -> Result<usize> {
        let n = self.inner.read(buf, timeout_ms)?;
        if n == 0 && timeout_ms > 0 {
            self.timeouts += 1;
        }
        if n > 0 {
            self.read_lens.insert(n);
            if self.trace {
                eprintln!("← {}", spaced(&buf[..n]));
            }
        }
        Ok(n)
    }

    fn report_len(&self) -> usize {
        self.inner.report_len()
    }
}

impl<L: HidLink> Drop for Guarded<L> {
    fn drop(&mut self) {
        let sent: Vec<String> = self
            .sent
            .iter()
            .map(|(cmd, n)| format!("0x{cmd:02X} ×{n}"))
            .collect();
        eprintln!(
            "Rapports envoyés au clavier : {} (plus la sonde GET_DEVICE_INFO 0x10 de recherche de l'interface){}",
            if sent.is_empty() {
                "aucun".into()
            } else {
                sent.join(", ")
            },
            if self.timeouts > 0 {
                format!(" ; {} attente(s) sans réponse", self.timeouts)
            } else {
                String::new()
            }
        );
    }
}

type Kb = Keyboard<Guarded<HidapiLink>>;

fn connect(allowed: &'static [u8], trace: bool) -> Result<(Kb, DeviceIdent)> {
    let (link, ident) = open_keyboard()?;
    let kb = Keyboard::connect(Guarded::new(link, allowed, trace))?;
    Ok((kb, ident))
}

// ---------- data files ----------

#[derive(Deserialize)]
struct Layout {
    keys: Vec<LayoutKey>,
}

#[derive(Deserialize)]
struct LayoutKey {
    index: usize,
    label: String,
    hid: u16,
    /// Left edge and width, in key units.
    x: f32,
    w: f32,
}

#[derive(Deserialize)]
struct KeyCode {
    page: String,
    name: String,
    value: u16,
    param1: Option<u8>,
    fr: Option<String>,
}

#[derive(Deserialize)]
struct Effect {
    mode: u8,
    name: String,
    fr: Option<String>,
}

static LAYOUT: LazyLock<Layout> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../data/ak980max/layout.json"))
        .expect("embedded layout.json is valid")
});
static KEYCODES: LazyLock<Vec<KeyCode>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../data/ak980max/keycodes.json"))
        .expect("embedded keycodes.json is valid")
});
static EFFECTS: LazyLock<Vec<Effect>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../data/ak980max/effects.json"))
        .expect("embedded effects.json is valid")
});

/// Layout label of key slot `i`, or `—` for a slot without a key.
fn label(i: usize) -> &'static str {
    LAYOUT
        .keys
        .iter()
        .find(|k| k.index == i)
        .map_or("—", |k| k.label.as_str())
}

fn usage_name(usage: u16) -> String {
    KEYCODES
        .iter()
        .find(|k| k.page == "KEYBOARD" && k.value == usage)
        .map_or_else(|| format!("0x{usage:02X}"), |k| k.name.clone())
}

// ---------- formatting ----------

fn spaced(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 16 bytes per line, with the offset.
fn hexdump(bytes: &[u8]) {
    for (n, line) in bytes.chunks(16).enumerate() {
        println!("  {:04X}  {}", n * 16, spaced(line));
    }
}

fn color(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// Millimetres with a French decimal comma.
fn mm(v: f32) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.replace('.', ",")
}

/// Compares the bytes a restore would write with the bytes read (informational: some
/// blocks are written the site's way on purpose).
fn rewrite_check(read: &[u8], encoded: &[u8]) {
    let len = read.len().max(encoded.len());
    let diffs: Vec<usize> = (0..len)
        .filter(|&i| read.get(i) != encoded.get(i))
        .collect();
    if diffs.is_empty() {
        println!("Réécriture : identique aux {} octets lus", read.len());
        return;
    }
    println!(
        "Réécriture : {} octet(s) sur {} différent(s) des octets lus :",
        diffs.len(),
        len
    );
    let show = |b: Option<&u8>| b.map_or("--".to_owned(), |b| format!("{b:02X}"));
    for &i in diffs.iter().take(24) {
        println!(
            "  octet {i:4} : lu {} → réécrit {}",
            show(read.get(i)),
            show(encoded.get(i))
        );
    }
    if diffs.len() > 24 {
        println!("  … et {} autres", diffs.len() - 24);
    }
}

/// Report lengths in bytes by (kind, report id): the sum of `size × count` over the
/// main items of each report.
fn report_lengths(desc: &[u8]) -> BTreeMap<(ReportKind, u8), usize> {
    let mut bits: BTreeMap<(ReportKind, u8), u32> = BTreeMap::new();
    for item in report_items(desc) {
        *bits.entry((item.kind, item.report_id)).or_default() += item.size * item.count;
    }
    bits.into_iter()
        .map(|(k, b)| (k, b.div_ceil(8) as usize))
        .collect()
}

fn descriptor_of(api: &HidApi, path: &str) -> std::result::Result<Vec<u8>, String> {
    let path = CString::new(path).map_err(|e| e.to_string())?;
    let dev = api.open_path(&path).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; hidapi::MAX_REPORT_DESCRIPTOR_SIZE];
    let n = dev
        .get_report_descriptor(&mut buf)
        .map_err(|e| e.to_string())?;
    buf.truncate(n);
    Ok(buf)
}

/// The report lengths of a collection; with `site`, also the length the site uses.
fn print_lengths(desc: &[u8], site: bool) {
    let lens = report_lengths(desc);
    if lens.is_empty() {
        println!("    rapports : aucun");
    }
    for ((kind, id), len) in lens {
        let kind = match kind {
            ReportKind::Input => "entrée",
            ReportKind::Output => "sortie",
            ReportKind::Feature => "fonction",
        };
        println!("    rapport n° {id} en {kind} : {len} octets");
    }
    if site {
        println!(
            "    longueur de rapport du site (reportCount de la 1re sortie) : {} octets",
            site_report_len(desc)
        );
    }
}

// ---------- devices / info / backup ----------

/// Host-side enumeration only: no report is sent to the keyboard.
fn devices() -> Result<()> {
    let api = HidApi::new()?;
    let mut found = false;
    for d in api.device_list().filter(|d| d.vendor_id() == 0x0C45) {
        found = true;
        println!(
            "PID {:04X} interface {} — page d'usage 0x{:04X}, usage 0x{:04X} — {}{}",
            d.product_id(),
            d.interface_number(),
            d.usage_page(),
            d.usage(),
            d.product_string().unwrap_or("?"),
            if is_site_usage_page(d.usage_page()) {
                " — collection que le site ouvre"
            } else {
                ""
            }
        );
        let path = d.path().to_string_lossy();
        println!("    {path}");
        match descriptor_of(&api, &path) {
            Ok(desc) => {
                print_lengths(&desc, is_site_usage_page(d.usage_page()));
                println!(
                    "    descripteur reconstitué ({} octets) : {}",
                    desc.len(),
                    spaced(&desc)
                );
            }
            Err(e) => println!("    descripteur illisible : {e}"),
        }
    }
    if !found {
        println!("Aucun périphérique HID de VID 0C45.");
    }
    Ok(())
}

fn info(trace: bool) -> Result<()> {
    let (kb, ident) = connect(SITE_READS, trace)?;
    let i = &kb.info;
    println!("Clavier");
    println!(
        "  VID / PID (GET_DEVICE_INFO) : {:04X} / {:04X}",
        i.vid, i.pid
    );
    println!("  Micrologiciel (firmware) : {}", i.firmware);
    println!("  rt_precision : {}", i.rt_precision);
    println!("  frame_version : {}", i.frame_version);
    println!("  max_profile : {}", i.max_profile);
    println!("  led_max_frames : {}", i.led_max_frames);
    println!("  gif_max_frames : {}", i.gif_max_frames);
    println!("  tft_max_frames : {}", i.tft_max_frames);
    println!("  tft_direction : {} (255 = pas d'écran)", i.tft_direction);
    println!(
        "  firmware_status : {} (1 = attente de mise à jour)",
        i.firmware_status
    );
    println!(
        "  macro_space (octets 2–3) : {} (le site utilise 512 pour ce modèle)",
        i.macro_space
    );
    println!("  rom_size : {}", i.rom_size);
    println!("  sensor : 0x{:04X}", i.sensor);
    println!("  manufacturer : 0x{:04X}", i.manufacturer);
    println!("  product : 0x{:04X}", i.product);
    println!("  work_mode : {}", i.work_mode);
    println!("  battery : {}", i.battery);
    println!("  charge : {}", i.charge);
    println!("  current_profile : {}", i.current_profile);
    println!("  axis_info : 0x{:04X}", i.axis_info);
    println!("  lighting_version : {}", i.lighting_version);
    println!("  octets lus ({DEVICE_INFO_LEN}) :");
    hexdump(&i.raw);

    println!("Interface utilisée");
    println!(
        "  interface n° {} (MI_{:02})",
        ident.interface, ident.interface
    );
    println!(
        "  page d'usage : 0x{:04X}, usage 0x{:04X}",
        ident.usage_page, ident.usage
    );
    println!("  chemin : {}", ident.path);
    println!(
        "  numéro de série USB : {}",
        ident
            .serial
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("(aucun)")
    );
    let link = kb.link();
    println!(
        "  taille de rapport utilisée : {} octets",
        link.report_len()
    );
    println!(
        "  taille des rapports reçus (sans numéro de rapport) : {:?} octets",
        link.read_lens
    );
    match descriptor_of(&HidApi::new()?, &ident.path) {
        Ok(desc) => {
            println!("  d'après le descripteur reconstitué par Windows :");
            print_lengths(&desc, true);
        }
        Err(e) => println!("  descripteur illisible : {e}"),
    }
    if i.firmware_status == 1 {
        println!("Attention : le clavier attend une mise à jour ; le site ne lit rien d'autre.");
    }
    Ok(())
}

fn default_backup_dir() -> Result<PathBuf> {
    freejazz_protocol::backup::default_backup_dir()
}

fn backup(dir: Option<PathBuf>, trace: bool) -> Result<()> {
    let dir = match dir {
        Some(d) => d,
        None => default_backup_dir()?,
    };
    let (mut kb, _) = connect(SITE_READS, trace)?;
    let snap = kb.read_snapshot()?;
    let path = save_backup(&snap, &dir)?;
    println!("Sauvegarde écrite : {}", path.display());
    let remapped = snap
        .keys
        .0
        .iter()
        .filter(|a| a.kind() != KeyKind::Default)
        .count();
    println!(
        "  micrologiciel {} ; {} emplacement(s) de touche non nuls ; effet lumineux {} ; {} macro(s) ; {} entrée(s) DKS",
        snap.info.firmware,
        remapped,
        snap.led_effect.mode,
        snap.macros.slots.len(),
        snap.dks.0.len()
    );
    Ok(())
}

// ---------- dump ----------

fn dump(block: Block, unverified: bool, trace: bool) -> Result<ExitCode> {
    if let Block::FnKeys = block {
        // UNVERIFIED-HW: the site never sends GET_FN_KEY 0x16 to this model.
        if !unverified {
            eprintln!(
                "UNVERIFIED-HW : le site n'envoie jamais GET_FN_KEY (0x16) à ce modèle ; \
                 lecture de la couche Fn non effectuée (ajoutez --unverified, à vérifier sur le matériel)."
            );
            return Ok(ExitCode::from(2));
        }
        let (mut kb, _) = connect(READS_WITH_FN, trace)?;
        let map = kb.read_fn_keys()?;
        print_keys(&map, false);
        hexdump(&map.encode());
        return Ok(ExitCode::SUCCESS);
    }

    let (mut kb, _) = connect(SITE_READS, trace)?;
    match block {
        Block::GameMode => dump_game_mode(&mut kb)?,
        Block::Keys => dump_keys(&mut kb)?,
        Block::Led => dump_led(&mut kb)?,
        Block::CustomLeds => dump_custom_leds(&mut kb)?,
        Block::Macros => dump_macros(&mut kb)?,
        Block::Magnetic => dump_magnetic(&mut kb)?,
        Block::Dks => dump_dks(&mut kb)?,
        Block::FnKeys => unreachable!("handled above"),
    }
    Ok(ExitCode::SUCCESS)
}

fn dump_game_mode(kb: &mut Kb) -> Result<()> {
    let raw = kb.read_raw(Cmd::GetGameMode, 0, GAME_MODE_LEN)?;
    let g = GameMode::decode(&raw)?;
    println!("GAME_MODE (0x11)");
    println!("  gameMode : {}", g.game_mode());
    println!("  fnSwitch : {}", g.fn_switch());
    println!("  sleepTime : {} min", g.sleep_time());
    println!("  keyDelay : {}", g.key_delay());
    println!(
        "  reportRate : code {} = {}",
        g.report_rate_code(),
        g.report_rate()
            .map_or("hors de la table du site".into(), |hz| format!("{hz} Hz"))
    );
    println!("  systemMode : {}", g.system_mode());
    println!("  tftDisplayTime (octet 7) : {}", raw[7]);
    println!("  zone morte haute : {} mm", mm(g.top_dead_zone_mm()));
    println!("  zone morte basse : {} mm", mm(g.bottom_dead_zone_mm()));
    println!("  stabilityMode : {}", g.stability_mode());
    println!("  autoCalibration : {}", g.auto_calibration());
    println!("  singleKeyWakeup : {}", g.single_key_wakeup());
    println!("  pushButtonMode : {}", g.push_button_mode());
    println!("  nkroSwitch : {}", g.nkro());
    println!(
        "  wirelessReportRate (octets 18–19) : {}",
        u16::from_le_bytes([raw[18], raw[19]])
    );
    println!("  powerMode : {}", g.power_mode());
    let unread: Vec<String> = [0usize, 10, 12, 13]
        .into_iter()
        .chain(21..GAME_MODE_LEN)
        .filter(|&i| raw[i] != 0)
        .map(|i| format!("{i}={:02X}", raw[i]))
        .collect();
    println!(
        "  octets non lus par le site non nuls : {}",
        if unread.is_empty() {
            "aucun".into()
        } else {
            unread.join(", ")
        }
    );
    println!("Octets lus :");
    hexdump(&raw);
    rewrite_check(&raw, &g.encode());
    Ok(())
}

/// Text of a key action.
fn describe(a: KeyAction) -> String {
    match a.kind() {
        KeyKind::Default => "par défaut (00 00 00 00)".into(),
        KeyKind::Keyboard {
            modifiers: 0,
            usage,
        } => {
            format!("Clavier {} (0x{usage:02X})", usage_name(usage.into()))
        }
        KeyKind::Keyboard { modifiers, usage } => {
            format!("Clavier masque 0x{modifiers:02X} + 0x{usage:02X}")
        }
        KeyKind::Mouse { kind, value } => KEYCODES
            .iter()
            .find(|k| k.page == "MOUSE" && k.param1 == Some(kind) && k.value == u16::from(value))
            .and_then(|k| k.fr.clone())
            .unwrap_or_else(|| format!("Souris {kind}/{value}")),
        KeyKind::Consumer(code) => {
            let name = KEYCODES
                .iter()
                .find(|k| k.page == "CONSUMER_KEY" && k.value == code)
                .map(|k| k.fr.clone().unwrap_or_else(|| k.name.clone()));
            format!("Multimédia 0x{code:04X} {}", name.unwrap_or_default())
        }
        KeyKind::Macro { id, p2, p3 } => format!("Macro {id} (mode {p2}, {p3})"),
        KeyKind::Dks(slot) => format!("DKS {slot}"),
        KeyKind::Mt([hold, tap, t]) => format!(
            "MT maintien {} / clic {} / {} ms",
            usage_name(hold.into()),
            usage_name(tap.into()),
            u32::from(t) * 10
        ),
        KeyKind::Tgl(usage) => format!("TGL {}", usage_name(usage.into())),
        KeyKind::Socd([m, a, b]) => format!("SOCD mode {m} : 0x{a:02X} / 0x{b:02X}"),
        KeyKind::Rs([_, a, b]) => format!("RS 0x{a:02X} / 0x{b:02X}"),
        other => format!("{other:?} [{}]", spaced(&a.raw)),
    }
}

/// HID usage held by a KEYBOARD action: the code, or a single-bit modifier mask
/// translated as the site does (1 → L-Ctrl 0xE0 … 64 → R-Alt 0xE6).
fn read_usage(kind: KeyKind) -> Option<u16> {
    match kind {
        KeyKind::Keyboard {
            modifiers: 0,
            usage,
        } => Some(usage.into()),
        KeyKind::Keyboard {
            modifiers: m,
            usage: 0,
        } if m.is_power_of_two() => Some(0xE0 + m.trailing_zeros() as u16),
        _ => None,
    }
}

/// One line per layout key; with `compare`, the read usage against `layout.json` `hid`.
fn print_keys(map: &KeyMap, compare: bool) {
    let (mut same, mut defaults, mut differ) = (0, 0, Vec::new());
    println!("idx  touche          attendu (layout)       lu");
    for k in &LAYOUT.keys {
        let a = map.0[k.index];
        let verdict = match (a.kind(), read_usage(a.kind())) {
            (_, Some(u)) if u == k.hid => {
                same += 1;
                "OK"
            }
            (KeyKind::Default, _) => {
                defaults += 1;
                "défaut"
            }
            _ => {
                differ.push(k.index);
                "DIFFÉRENT"
            }
        };
        println!(
            "{:3}  {:14}  0x{:02X} {:16}  {:32} {}",
            k.index,
            k.label,
            k.hid,
            usage_name(k.hid),
            describe(a),
            if compare { verdict } else { "" }
        );
    }
    let outside: Vec<String> = (0..map.0.len())
        .filter(|i| !LAYOUT.keys.iter().any(|k| k.index == *i))
        .filter(|&i| map.0[i].kind() != KeyKind::Default)
        .map(|i| format!("{i} : {}", describe(map.0[i])))
        .collect();
    println!(
        "Emplacements sans touche non nuls : {}",
        if outside.is_empty() {
            "aucun".into()
        } else {
            outside.join(" ; ")
        }
    );
    if compare {
        println!(
            "Index ↔ layout.json : {same}/{} identiques, {defaults} par défaut (00 00 00 00), {} différent(s){}",
            LAYOUT.keys.len(),
            differ.len(),
            if differ.is_empty() {
                String::new()
            } else {
                format!(
                    " : {}",
                    differ
                        .iter()
                        .map(|&i| format!("{i} {}", label(i)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        );
    }
}

fn dump_keys(kb: &mut Kb) -> Result<()> {
    let raw = kb.read_raw(Cmd::GetKey, 0, KEY_MAP_LEN)?;
    let map = KeyMap::decode(&raw)?;
    println!("GET_KEY (0x12)");
    print_keys(&map, true);
    println!("Octets lus :");
    hexdump(&raw);
    rewrite_check(&raw, &map.encode());
    Ok(())
}

fn dump_led(kb: &mut Kb) -> Result<()> {
    let raw = kb.read_raw(Cmd::GetLedEffect, 0, LED_EFFECT_LEN)?;
    let e = LedEffect::decode(&raw)?;
    let name = EFFECTS
        .iter()
        .find(|x| x.mode == e.mode)
        .map(|x| x.fr.clone().unwrap_or_else(|| x.name.clone()))
        .unwrap_or_else(|| {
            if e.mode == 0 {
                "éteint".into()
            } else {
                "?".into()
            }
        });
    println!("GET_LED_EFFECT (0x13)");
    println!("  mode : {} ({name})", e.mode);
    println!(
        "  couleur : {}  secondaire : {}",
        color(e.rgb),
        color(e.secondary)
    );
    println!("  colorMode : {} (0 unique, 1 RVB)", e.color_mode);
    println!(
        "  luminosité : {}  vitesse : {}  direction : {}",
        e.brightness, e.speed, e.direction
    );
    println!("  effectModeType : {}", e.effect_mode_type);
    println!(
        "  octet 4 (driverSetting) : {:02X} ; octet 13 : {:02X} ; octets 14–15 (contrôle) : {:02X} {:02X} (le site écrit FF, 00, AA 55)",
        raw[4], raw[13], raw[14], raw[15]
    );
    println!("Octets lus :");
    hexdump(&raw);
    rewrite_check(&raw, &e.encode());
    Ok(())
}

fn dump_custom_leds(kb: &mut Kb) -> Result<()> {
    let raw = kb.read_raw(Cmd::GetCustomLedData, 0, CUSTOM_LEDS_LEN)?;
    let leds = CustomLeds::decode(&raw)?;
    println!("GET_CUSTOM_LED_DATA (0x14)");
    let mut by_color: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for k in &LAYOUT.keys {
        by_color
            .entry(color(leds.0[k.index]))
            .or_default()
            .push(&k.label);
    }
    for (c, keys) in &by_color {
        if keys.len() > 12 {
            println!("  {c} : {} touches", keys.len());
        } else {
            println!("  {c} : {}", keys.join(", "));
        }
    }
    let mismatches: Vec<String> = raw
        .chunks_exact(4)
        .enumerate()
        .filter(|(i, e)| usize::from(e[0]) != *i)
        .map(|(i, e)| format!("entrée {i} : index {}", e[0]))
        .collect();
    println!(
        "Octet d'index de chaque entrée = sa position : {}",
        if mismatches.is_empty() {
            "oui, pour les 128 entrées".into()
        } else {
            format!(
                "NON pour {} entrée(s) : {}",
                mismatches.len(),
                mismatches.join(" ; ")
            )
        }
    );
    println!("Octets lus :");
    hexdump(&raw);
    rewrite_check(&raw, &leds.encode());
    Ok(())
}

fn dump_macros(kb: &mut Kb) -> Result<()> {
    let table = kb.read_raw(Cmd::GetMacro, 0, MACRO_TABLE_LEN)?;
    let mut reads: Vec<(u16, Vec<u8>)> = Vec::new();
    let macros = Macros::decode(&table, |addr, len| {
        let bytes = kb.read_raw(Cmd::GetMacro, addr, len)?;
        reads.push((addr, bytes.clone()));
        Ok(bytes)
    })?;
    println!("GET_MACRO (0x15)");
    for m in &macros.slots {
        let h = m.raw_header.unwrap_or_default();
        println!(
            "  macro {} : {} action(s), en-tête {}",
            m.id,
            m.actions.len(),
            spaced(&h)
        );
        for a in &m.actions {
            let what = match a.kind {
                1 | 2 => format!("souris {}", a.key),
                3 => format!("touche {}", usage_name(a.key.into())),
                k => format!("type {k} code 0x{:02X}", a.key),
            };
            println!(
                "    {} {what}, puis {} ms (drapeaux {:02X})",
                if a.press { "appui" } else { "relâchement" },
                a.delay_ms,
                a.raw_flags.unwrap_or_default()
            );
        }
    }
    println!(
        "  {} macro(s), {} octets utilisés ; capacité du site {MACRO_CAPACITY} ; macro_space annoncé {}",
        macros.slots.len(),
        macros.used_bytes(),
        kb.info.macro_space
    );
    println!("Table lue :");
    hexdump(&table);
    for (addr, bytes) in &reads {
        println!("Lu à l'adresse {addr} :");
        hexdump(bytes);
    }
    // The area as read, from address 400, against the area a restore writes.
    let (table2, data2) = macros.to_bytes()?;
    let end = reads
        .iter()
        .map(|(a, b)| usize::from(*a) + b.len())
        .max()
        .unwrap_or(usize::from(MACRO_DATA_START));
    let mut area = vec![0u8; end.saturating_sub(usize::from(MACRO_DATA_START))];
    for (addr, bytes) in &reads {
        let at = usize::from(*addr) - usize::from(MACRO_DATA_START);
        area[at..at + bytes.len()].copy_from_slice(bytes);
    }
    println!("Table :");
    rewrite_check(&table, &table2);
    println!("Macros (depuis l'adresse {MACRO_DATA_START}) :");
    rewrite_check(&area, &data2);
    Ok(())
}

fn dump_magnetic(kb: &mut Kb) -> Result<()> {
    let precision = kb.info.rt_precision;
    let raw = kb.read_raw(Cmd::GetMagneticAxisRt, 0, MAGNETIC_LEN)?;
    let map = MagneticMap::decode(&raw, precision)?;
    println!("GET_MAGNETIC_AXIS_RT (0x17), rt_precision {precision}");
    let text = |k: &MagneticKey| {
        format!(
            "switch {} ; déclenchement {} mm ; RT appui {} / relâchement {} mm{}{}{}",
            k.axis,
            mm(k.trigger_mm),
            mm(k.press_rt_mm),
            mm(k.release_rt_mm),
            if k.whole_fast {
                " ; toute la course"
            } else {
                ""
            },
            if k.rampage { " ; mode rage" } else { "" },
            if k.other_flags != 0 {
                format!(" ; autres bits 0x{:02X}", k.other_flags)
            } else {
                String::new()
            }
        )
    };
    let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
    for k in &LAYOUT.keys {
        let t = text(&map.0[k.index]);
        match groups.iter_mut().find(|(g, _)| *g == t) {
            Some((_, keys)) => keys.push(&k.label),
            None => groups.push((t, vec![&k.label])),
        }
    }
    for (t, keys) in &groups {
        if keys.len() > 12 {
            println!("  {} touches : {t}", keys.len());
        } else {
            println!("  {} : {t}", keys.join(", "));
        }
    }
    let outside: Vec<String> = (0..map.0.len())
        .filter(|i| !LAYOUT.keys.iter().any(|k| k.index == *i) && !map.0[*i].is_empty())
        .map(|i| format!("{i} : {}", text(&map.0[i])))
        .collect();
    println!(
        "Emplacements sans touche non nuls : {}",
        if outside.is_empty() {
            "aucun".into()
        } else {
            outside.join(" ; ")
        }
    );
    println!("Octets lus :");
    hexdump(&raw);
    rewrite_check(&raw, &map.encode(precision)?);
    Ok(())
}

fn dump_dks(kb: &mut Kb) -> Result<()> {
    let raw = kb.read_raw(Cmd::GetMagneticAxisDksData, 0, DKS_LEN)?;
    let table = DksTable::decode(&raw)?;
    println!(
        "GET_MAGNETIC_AXIS_DKS_DATA (0x18) : {} entrée(s) non nulle(s)",
        table.0.len()
    );
    for d in &table.0 {
        let actions: Vec<String> = d
            .actions
            .iter()
            .map(|&u| {
                if u == 0 {
                    "—".into()
                } else {
                    usage_name(u.into())
                }
            })
            .collect();
        println!(
            "  emplacement {} : appui {:?} relâchement {:?} (0,1 mm) ; actions {} ; états {:?} ; réservés {:?}",
            d.index,
            d.make,
            d.brk,
            actions.join(", "),
            d.states,
            d.reserved
        );
    }
    println!("Octets lus :");
    hexdump(&raw);
    rewrite_check(&raw, &table.encode()?);
    Ok(())
}

// ---------- restore ----------

fn restore(file: &Path, yes: bool, trace: bool) -> Result<ExitCode> {
    let snap = load_backup(file)?;
    println!(
        "Sauvegarde {} : PID {:04X}, micrologiciel {}",
        file.display(),
        snap.info.pid,
        snap.info.firmware
    );
    println!(
        "Blocs à écrire, dans l'ordre : touches, effet lumineux (mode {}), couleurs personnalisées, \
         macros ({}), magnétique, DKS ({} entrée(s)), réglages GAME_MODE (fréquence {})",
        snap.led_effect.mode,
        snap.macros.slots.len(),
        snap.dks.0.len(),
        snap.game_mode.report_rate().map_or(
            format!("code {}", snap.game_mode.report_rate_code()),
            |hz| format!("{hz} Hz")
        )
    );
    if snap.fn_keys.is_some() {
        // UNVERIFIED-HW: the site never writes the Fn layer of this model.
        return Err(Error::Validation(
            "La sauvegarde contient une couche Fn, que le site n'écrit jamais pour ce modèle : \
             restauration refusée (à vérifier sur le matériel)"
                .into(),
        ));
    }
    if !yes {
        eprintln!(
            "Restauration non effectuée : rien n'a été envoyé au clavier. \
             Ajoutez --yes pour écrire ces blocs."
        );
        return Ok(ExitCode::from(2));
    }

    let (mut kb, _) = connect(RESTORE, trace)?;
    if kb.info.pid != snap.info.pid {
        return Err(Error::Validation(format!(
            "Sauvegarde d'un autre clavier (PID {:04X}, branché : {:04X}) : restauration refusée",
            snap.info.pid, kb.info.pid
        )));
    }
    let before = kb.read_snapshot()?;
    let safety = save_backup(&before, &default_backup_dir()?)?;
    println!(
        "Sauvegarde de l'état actuel avant écriture : {}",
        safety.display()
    );

    // A report-rate change makes the keyboard re-enumerate on USB after GAME_MODE (written
    // last): its acknowledgement may never come, and the link is gone.
    let rate_changes = before.game_mode.report_rate_code() != snap.game_mode.report_rate_code();
    let written = kb.write_snapshot(&snap);
    if rate_changes {
        drop(kb);
        println!("La fréquence d'envoi change : attente de la reconnexion du clavier…");
        kb = reconnect(trace)?;
    }
    match written {
        Ok(()) => {}
        Err(e) if rate_changes => {
            println!(
                "Écriture : {e} (attendu si le clavier s'est reconnecté) ; vérification par relecture"
            )
        }
        Err(e) => return Err(e),
    }

    let after = kb.read_snapshot()?;
    let checks = [
        ("touches", after.keys == snap.keys),
        (
            "effet lumineux",
            snap.led_effect.matches_readback(&after.led_effect),
        ),
        (
            "couleurs personnalisées",
            after.custom_leds == snap.custom_leds,
        ),
        ("macros", after.macros == snap.macros),
        ("magnétique", after.magnetic == snap.magnetic),
        ("DKS", after.dks == snap.dks),
        ("réglages GAME_MODE", after.game_mode == snap.game_mode),
    ];
    for (name, ok) in checks {
        println!(
            "  {name} : {}",
            if ok {
                "relu identique"
            } else {
                "DIFFÉRENT après relecture"
            }
        );
    }
    if checks.iter().all(|(_, ok)| *ok) {
        println!("Restauration vérifiée.");
        Ok(ExitCode::SUCCESS)
    } else {
        println!(
            "Restauration incomplète : l'état d'avant est dans {}",
            safety.display()
        );
        Ok(ExitCode::FAILURE)
    }
}

/// Waits up to 10 s for the keyboard to come back after a USB re-enumeration.
fn reconnect(trace: bool) -> Result<Kb> {
    reconnect_with(Wait::USB, || connect(RESTORE, trace).map(|(kb, _)| kb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_lengths_of_a_vendor_collection() {
        // Usage Page 0xFF00, Usage 1, Collection; 32 × 8-bit input, 32 × 8-bit output
        let desc = [
            0x06, 0x00, 0xFF, 0x09, 0x01, 0xA1, 0x01, 0x15, 0x00, 0x26, 0xFF, 0x00, 0x75, 0x08,
            0x95, 0x20, 0x09, 0x01, 0x81, 0x02, 0x95, 0x20, 0x09, 0x01, 0x91, 0x02, 0xC0,
        ];
        let lens = report_lengths(&desc);
        assert_eq!(lens.get(&(ReportKind::Input, 0)), Some(&32));
        assert_eq!(lens.get(&(ReportKind::Output, 0)), Some(&32));
        assert_eq!(lens.len(), 2);
    }

    #[test]
    fn report_lengths_per_report_id() {
        // report 1: 8 bytes input; report 2: 2 × 16-bit output
        let desc = [
            0x85, 0x01, 0x75, 0x08, 0x95, 0x08, 0x81, 0x02, 0x85, 0x02, 0x75, 0x10, 0x95, 0x02,
            0x91, 0x02,
        ];
        let lens = report_lengths(&desc);
        assert_eq!(lens.get(&(ReportKind::Input, 1)), Some(&8));
        assert_eq!(lens.get(&(ReportKind::Output, 2)), Some(&4));
    }

    #[test]
    fn embedded_data_parses() {
        assert_eq!(LAYOUT.keys.len(), 101);
        assert_eq!(usage_name(0x04), "A");
        assert!(EFFECTS.iter().any(|e| e.mode == 128));
    }

    #[test]
    fn modifier_masks_read_as_usages() {
        let kb = |m, u| KeyKind::Keyboard {
            modifiers: m,
            usage: u,
        };
        assert_eq!(read_usage(kb(0, 0x29)), Some(0x29));
        assert_eq!(read_usage(kb(1, 0)), Some(0xE0));
        assert_eq!(read_usage(kb(64, 0)), Some(0xE6));
        assert_eq!(read_usage(kb(3, 0)), None);
        assert_eq!(read_usage(KeyKind::Default), None);
    }

    #[test]
    fn guard_refuses_before_sending() {
        use freejazz_protocol::transport::MockLink;
        let mut g = Guarded::new(MockLink::new(32), SITE_READS, false);
        let mut set_key = vec![0u8; 32];
        set_key[..2].copy_from_slice(&[0xAA, 0x22]);
        assert!(matches!(g.write(&set_key), Err(Error::Denied(0x22))));
        let mut not_request = vec![0u8; 32];
        not_request[1] = 0x10;
        assert!(g.write(&not_request).is_err());
        let mut info = vec![0u8; 32];
        info[..2].copy_from_slice(&[0xAA, 0x10]);
        g.write(&info).unwrap();
        assert_eq!(g.inner.written, vec![info]);
    }
}
