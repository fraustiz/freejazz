//! `selftest-writes`: every block writer tried on the keyboard, each write undone and
//! verified (protocol.md 9.7).
//!
//! A full backup is read and saved first, then loaded back from its file: every check
//! and every restore uses that loaded snapshot. Then, for each block in the order
//! game-mode, led, custom-leds, keys, magnetic, dks, macros, and last the LED effect and
//! the custom colours written back to back (the site's profile order, to see whether
//! the keyboard needs a pause between the two):
//!
//! 1. `sauvegarde`: the keyboard still holds the backup;
//! 2. `même valeur`: the backed-up block written, read back equal;
//! 3. `modification`: one value changed ([`Part::change`]), read back with the change;
//! 4. `restauration`: the backed-up block written again, read back equal.
//!
//! Each read-back is a whole `read_snapshot`, checked with [`Snapshot::check_raw`], the
//! rule `load_backup` applies: the typed blocks are compared, the write constants (LED
//! bytes 4 and 13-15, custom-LED index bytes, macro layout) are not. At the first FAIL
//! the whole backup is written back (`write_snapshot`, the `restore` path) and verified.
//! A keyboard that stops answering after a write (USB re-enumeration) is waited for
//! before reading back; one that does not come back, or a restore that does not verify,
//! ends the test BLOQUÉ.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use freejazz_protocol::backup::{load_backup, save_backup};
use freejazz_protocol::blocks::macros::MACRO_DATA_START;
use freejazz_protocol::blocks::{
    KeyAction, KeyKind, Macro, MacroAction, Macros, MagneticMap, ModelLimits, Rgb,
};
pub(crate) use freejazz_protocol::reconnect::{Wait, reconnect_with};
use freejazz_protocol::transport::HidLink;
use freejazz_protocol::{Error, Keyboard, RawBlocks, Result, Snapshot};

use crate::{Guarded, LAYOUT, RESTORE, color, connect, default_backup_dir, label, mm, spaced};

type Kb<L> = Keyboard<Guarded<L>>;

/// What is written: one block, or the LED effect then the custom colours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    GameMode,
    Led,
    CustomLeds,
    Keys,
    Magnetic,
    Dks,
    Macros,
    /// `SET_LED_EFFECT` immediately followed by `SET_CUSTOM_LED_DATA`, no pause.
    LedChain,
}

/// The brief's order, then the LED effect and colours back to back.
pub(crate) const PARTS: [Part; 8] = [
    Part::GameMode,
    Part::Led,
    Part::CustomLeds,
    Part::Keys,
    Part::Magnetic,
    Part::Dks,
    Part::Macros,
    Part::LedChain,
];

const STEPS: [&str; 4] = ["sauvegarde", "même valeur", "modification", "restauration"];

/// The colour of the `modification` step for custom LED index 0: Freejazz's accent.
const ACCENT: Rgb = Rgb {
    r: 0x7c,
    g: 0x5c,
    b: 0xff,
};
/// Consumer usage "Volume +", for F12.
const VOLUME_UP: u16 = 0x00E9;
/// Macro slot written by the `modification` step (assigned to no key).
const MACRO_ID: u8 = 99;

/// The `modification` step of a part.
pub(crate) enum Change {
    /// Nothing changed, and why.
    Skip(String),
    /// The snapshot to write, as the keyboard should return it, and what changed.
    To(Box<Snapshot>, String),
}

impl Part {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Part::GameMode => "game-mode",
            Part::Led => "led",
            Part::CustomLeds => "custom-leds",
            Part::Keys => "keys",
            Part::Magnetic => "magnetic",
            Part::Dks => "dks",
            Part::Macros => "macros",
            Part::LedChain => "led + custom-leds",
        }
    }

    /// Writes this part of `s` with the crate's writers.
    fn write<L: HidLink>(self, kb: &mut Keyboard<L>, s: &Snapshot) -> Result<()> {
        match self {
            Part::GameMode => kb.write_game_mode(&s.game_mode),
            Part::Led => kb.write_led_effect(&s.led_effect),
            Part::CustomLeds => kb.write_custom_leds(&s.custom_leds),
            Part::Keys => kb.write_keys(&s.keys),
            Part::Magnetic => kb.write_magnetic(&s.magnetic),
            Part::Dks => kb.write_dks(&s.dks),
            Part::Macros => kb.write_macros(&s.macros),
            Part::LedChain => {
                kb.write_led_effect(&s.led_effect)?;
                kb.write_custom_leds(&s.custom_leds)
            }
        }
    }

    /// `s` with this part's one changed value, as the keyboard should return it once
    /// written, checked against the UI limits (`Snapshot::validate_changes`); or why
    /// nothing is changed.
    pub(crate) fn change(self, s: &Snapshot) -> Result<Change> {
        let mut c = s.clone();
        c.raw = None;
        let what = match self {
            Part::GameMode => {
                // Never the report rate (a change makes the keyboard re-enumerate on
                // USB): "Mode de stabilité", a toggle of the site's settings page for
                // this model (protocol.md 6.2).
                let v = c.game_mode.stability_mode();
                if v > 1 {
                    return Ok(Change::Skip(format!(
                        "mode de stabilité {v} (ni 0 ni 1) : non modifié"
                    )));
                }
                c.game_mode.set_stability_mode(1 - v);
                format!("mode de stabilité {v} → {}", 1 - v)
            }
            Part::Led => dim(&mut c),
            Part::CustomLeds => paint(&mut c),
            Part::LedChain => {
                let dimmed = dim(&mut c);
                format!("{dimmed} ; {}", paint(&mut c))
            }
            Part::Keys => {
                let i = index_of("F12")?;
                c.keys.0[i] = KeyAction::from_kind(KeyKind::Consumer(VOLUME_UP));
                format!("touche F12 (index {i}) → multimédia 0x{VOLUME_UP:04X} (volume +)")
            }
            Part::Magnetic => match bump_trigger(&mut c)? {
                Some(what) => what,
                None => {
                    return Ok(Change::Skip(
                        "aucune touche n'a un point de déclenchement dans les limites : table                          magnétique non modifiée"
                            .into(),
                    ));
                }
            },
            Part::Dks => {
                return Ok(Change::Skip(if s.dks.0.is_empty() {
                    "table DKS vide : laissée telle quelle".into()
                } else {
                    "table DKS non vide : non modifiée par ce test".into()
                }));
            }
            Part::Macros => {
                let a = |press, delay_ms| MacroAction {
                    press,
                    kind: 3,
                    key: 0x04,
                    delay_ms,
                    raw_flags: None,
                };
                c.macros.slots.retain(|m| m.id != MACRO_ID);
                c.macros.slots.push(Macro {
                    id: MACRO_ID,
                    actions: vec![a(true, 10), a(false, 0)],
                    raw_header: None,
                });
                c.macros = as_read(&c.macros)?;
                format!(
                    "macro {MACRO_ID} : appui A, 10 ms, relâchement A (affectée à aucune touche)"
                )
            }
        };
        if c == *s {
            return Ok(Change::Skip(format!("{what} : valeur déjà en place")));
        }
        c.validate_changes(s)?;
        Ok(Change::To(Box::new(c), what))
    }
}

/// LED brightness −1 (+1 at the bottom of the range).
fn dim(c: &mut Snapshot) -> String {
    let b = c.led_effect.brightness;
    let n = if b > 1 { b - 1 } else { b + 1 };
    c.led_effect.brightness = n;
    format!("luminosité {b} → {n}")
}

/// Custom LED index 0 → [`ACCENT`].
fn paint(c: &mut Snapshot) -> String {
    let old = c.custom_leds.0[0];
    c.custom_leds.0[0] = ACCENT;
    format!(
        "couleur personnalisée de l'index 0 ({}) {} → {}",
        label(0),
        color(old),
        color(ACCENT)
    )
}

/// The trigger point of one key +0.01 mm (−0.01 mm at the top of the range), changed in
/// the encoded bytes so that the value is exactly the one a read decodes. The key is
/// index 0 when its trigger is within the UI limits, else the first layout key whose
/// trigger is (an empty table, e.g. after a reset, has none: SKIP).
fn bump_trigger(c: &mut Snapshot) -> Result<Option<String>> {
    let limits = ModelLimits::ak980max();
    let valid = |t: f32| t >= limits.min_trigger_trip - 1e-4 && t <= limits.max_trigger_trip + 1e-4;
    let Some(i) = std::iter::once(0)
        .chain(LAYOUT.keys.iter().map(|k| k.index))
        .find(|&i| valid(c.magnetic.0[i].trigger_mm))
    else {
        return Ok(None);
    };
    let precision = c.info.rt_precision;
    let mut bytes = c.magnetic.encode(precision)?;
    // 0.01 mm in device steps: the trigger is in 1/1000 mm when rtPrecision is 2,
    // 1/100 mm otherwise (protocol.md 6.7).
    let step: u16 = if precision == 2 { 10 } else { 1 };
    let old = c.magnetic.0[i].trigger_mm;
    let at = 8 * i + 2;
    let raw = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let new = if old + 0.01 <= limits.max_trigger_trip + 1e-4 {
        raw.checked_add(step)
    } else {
        raw.checked_sub(step)
    }
    .ok_or_else(|| {
        Error::Invalid(format!(
            "point de déclenchement de l'index {i} hors échelle"
        ))
    })?;
    bytes[at..at + 2].copy_from_slice(&new.to_le_bytes());
    c.magnetic = MagneticMap::decode(&bytes, precision)?;
    Ok(Some(format!(
        "touche index {i} ({}){} : déclenchement {} → {} mm",
        label(i),
        if i == 0 {
            ""
        } else {
            ", l'index 0 étant hors limites"
        },
        mm(old),
        mm(c.magnetic.0[i].trigger_mm)
    )))
}

/// The macros as the keyboard returns them once written: the bytes written, decoded like
/// a read (headers and flags as stored).
fn as_read(macros: &Macros) -> Result<Macros> {
    let (table, data) = macros.to_bytes()?;
    RawBlocks {
        macro_table: table,
        macro_reads: vec![(MACRO_DATA_START, data)],
        ..RawBlocks::default()
    }
    .decode_macros()
}

fn index_of(name: &str) -> Result<usize> {
    LAYOUT
        .keys
        .iter()
        .find(|k| k.label == name)
        .map(|k| k.index)
        .ok_or_else(|| Error::Invalid(format!("{name} absente de layout.json")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Ok,
    Skip,
    Fail,
}

impl Status {
    fn text(self) -> &'static str {
        match self {
            Status::Ok => "OK",
            Status::Skip => "SKIP",
            Status::Fail => "FAIL",
        }
    }
}

/// The steps run for one part, in [`STEPS`] order, with their notes.
pub(crate) struct Row {
    pub part: Part,
    pub steps: Vec<(Status, String)>,
}

impl Row {
    /// Records and prints the next step; `false` on a FAIL.
    fn push(&mut self, status: Status, note: String) -> bool {
        let step = STEPS.get(self.steps.len()).copied().unwrap_or("?");
        println!("[{}] {step} : {} — {note}", self.part.name(), status.text());
        self.steps.push((status, note));
        status != Status::Fail
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Every step OK or SKIP; the keyboard read at the end equals the backup.
    Passed,
    /// A step failed; the backup was written back and verified.
    Restored,
    /// The keyboard is gone, or does not hold the backup and a restore did not fix it.
    Blocked(String),
}

pub(crate) struct Report {
    pub rows: Vec<Row>,
    pub outcome: Outcome,
    /// The backup taken before any write, in each folder.
    pub backups: Vec<PathBuf>,
    /// The keyboard read after the test (only when it passed), in each folder.
    pub finals: Vec<PathBuf>,
}

/// The test can't go on: the keyboard is gone, or its state can't be verified.
struct Blocked(String);

type Res<T> = std::result::Result<T, Blocked>;

/// The rule `load_backup` applies: the bytes read back decode to `expect`.
fn check(expect: &Snapshot, after: &Snapshot) -> std::result::Result<(), &'static str> {
    match &after.raw {
        Some(raw) => expect.check_raw(raw),
        None => Err("relecture"),
    }
}

struct Session<L: HidLink, C> {
    kb: Kb<L>,
    connect: C,
    wait: Wait,
    /// Reports sent and waits without a report on the links before the current one.
    carried: (usize, usize),
}

impl<L: HidLink, C: FnMut() -> Result<Kb<L>>> Session<L, C> {
    fn counts(&self) -> (usize, usize) {
        let link = self.kb.link();
        (
            self.carried.0 + link.total_sent(),
            self.carried.1 + link.timeouts,
        )
    }

    /// Opens the keyboard again, within `wait.timeout`, once it stopped answering.
    fn reconnect(&mut self, notes: &mut Vec<String>) -> Res<()> {
        let (wait, counts, t) = (self.wait, self.counts(), Instant::now());
        let kb = reconnect_with(wait, &mut self.connect).map_err(|e| {
            Blocked(format!(
                "le clavier ne répond plus et n'est pas revenu en {} s ({e})",
                wait.timeout.as_secs_f32()
            ))
        })?;
        self.kb = kb;
        self.carried = counts;
        notes.push(format!("reconnecté en {} ms", t.elapsed().as_millis()));
        Ok(())
    }

    /// Reads the whole keyboard; if it does not answer, waits for it and reads again.
    fn read_back(&mut self, notes: &mut Vec<String>) -> Res<Snapshot> {
        match self.kb.read_snapshot() {
            Ok(s) => Ok(s),
            Err(e) => {
                notes.push(format!("relecture : {e}"));
                self.reconnect(notes)?;
                self.kb
                    .read_snapshot()
                    .map_err(|e| Blocked(format!("relecture impossible après reconnexion : {e}")))
            }
        }
    }

    /// Runs a write. After an error, a keyboard that still answers refused it
    /// (`Ok(Err(message))`); one that does not is waited for (re-enumeration) and the
    /// read-back tells what it holds.
    fn write(
        &mut self,
        write: impl FnOnce(&mut Kb<L>) -> Result<()>,
        notes: &mut Vec<String>,
    ) -> Res<std::result::Result<(), String>> {
        let Err(e) = write(&mut self.kb) else {
            return Ok(Ok(()));
        };
        if self.kb.read_device_info().is_ok() {
            return Ok(Err(format!("écriture : {e}")));
        }
        notes.push(format!("écriture : {e} ; le clavier ne répond plus"));
        self.reconnect(notes)?;
        Ok(Ok(()))
    }

    /// Writes `part` of `from`, reads the whole keyboard back and checks that it holds
    /// `expect`.
    fn step(&mut self, part: Part, from: &Snapshot, expect: &Snapshot) -> Res<(Status, String)> {
        let mut notes = Vec::new();
        let (sent0, timeouts0) = self.counts();
        let t0 = Instant::now();
        if let Err(e) = self.write(|kb| part.write(kb, from), &mut notes)? {
            notes.insert(0, e);
            return Ok((Status::Fail, notes.join(" ; ")));
        }
        let (sent1, t1) = (self.counts().0, Instant::now());
        let after = self.read_back(&mut notes)?;
        let (sent2, timeouts2) = self.counts();
        notes.push(format!(
            "écriture : {} rapport(s) en {} ms, relecture : {} rapport(s) en {} ms",
            sent1 - sent0,
            (t1 - t0).as_millis(),
            sent2 - sent1,
            t1.elapsed().as_millis()
        ));
        if timeouts2 > timeouts0 {
            notes.push(format!("{} attente(s) sans réponse", timeouts2 - timeouts0));
        }
        notes.extend(stored(part, &after));
        if matches!(part, Part::Led | Part::LedChain)
            && !expect.led_effect.colour_verified_by(&after.led_effect)
        {
            notes.push(
                "couleur et colorMode de l'effet : non vérifiables par relecture (protocol.md 9.7)"
                    .into(),
            );
        }
        let status = match check(expect, &after) {
            Ok(()) => Status::Ok,
            Err(block) => {
                notes.insert(0, format!("relu : bloc « {block} » différent de l'attendu"));
                Status::Fail
            }
        };
        Ok((status, notes.join(" ; ")))
    }

    /// The four steps of `part`; `Ok(false)` at the first FAIL, the remaining steps not
    /// run.
    fn test_part(&mut self, part: Part, backup: &Snapshot, row: &mut Row) -> Res<bool> {
        let mut notes = Vec::new();
        let now = self.read_back(&mut notes)?;
        let held = check(backup, &now);
        match held {
            Ok(()) => notes.insert(0, "le clavier contient la sauvegarde".into()),
            Err(block) => notes.insert(
                0,
                format!("le bloc « {block} » ne correspond plus à la sauvegarde"),
            ),
        }
        let status = if held.is_ok() {
            Status::Ok
        } else {
            Status::Fail
        };
        if !row.push(status, notes.join(" ; ")) {
            return Ok(false);
        }

        let (status, note) = self.step(part, backup, backup)?;
        if !row.push(status, note) {
            return Ok(false);
        }

        match part.change(backup) {
            Err(e) => {
                row.push(
                    Status::Fail,
                    format!("modification refusée avant envoi : {e}"),
                );
                return Ok(false);
            }
            Ok(Change::Skip(why)) => {
                row.push(Status::Skip, why);
            }
            Ok(Change::To(expect, what)) => {
                let (status, note) = self.step(part, &expect, &expect)?;
                if !row.push(status, format!("{what} ; {note}")) {
                    return Ok(false);
                }
            }
        }

        let (status, note) = self.step(part, backup, backup)?;
        Ok(row.push(status, note))
    }

    fn rewrite_all(&mut self, backup: &Snapshot, notes: &mut Vec<String>) -> Res<Snapshot> {
        if let Err(e) = self.write(|kb| kb.write_snapshot(backup), notes)? {
            notes.push(e);
        }
        self.read_back(notes)
    }

    /// After a FAIL: the whole backup written back (the `restore` path) and read back.
    fn restore_all(&mut self, backup: &Snapshot, dirs: &[PathBuf]) -> Outcome {
        println!("Échec : réécriture de toute la sauvegarde, puis relecture…");
        let mut notes = Vec::new();
        let outcome = match self.rewrite_all(backup, &mut notes) {
            Err(Blocked(why)) => Outcome::Blocked(why),
            Ok(after) => match check(backup, &after) {
                Ok(()) => {
                    println!("Restauration vérifiée : le clavier contient la sauvegarde.");
                    Outcome::Restored
                }
                Err(block) => Outcome::Blocked(format!(
                    "après restauration, le bloc « {block} » diffère encore de la sauvegarde ; {}",
                    save_state(&after, dirs)
                )),
            },
        };
        for n in &notes {
            println!("  {n}");
        }
        outcome
    }

    /// After every part: the whole keyboard read once more, compared with the backup
    /// (saved at `first`), and saved next to it.
    fn final_check(
        &mut self,
        backup: &Snapshot,
        dirs: &[PathBuf],
        first: &Path,
        finals: &mut Vec<PathBuf>,
    ) -> Outcome {
        let mut notes = Vec::new();
        let fin = self.read_back(&mut notes);
        for n in &notes {
            println!("  {n}");
        }
        let fin = match fin {
            Ok(s) => s,
            Err(Blocked(why)) => return Outcome::Blocked(why),
        };
        if let Err(block) = check(backup, &fin) {
            println!("Contrôle final : le bloc « {block} » diffère de la sauvegarde");
            return self.restore_all(backup, dirs);
        }
        println!(
            "Contrôle final : le clavier relu est identique à la sauvegarde (comparaison typée)."
        );
        for d in dirs {
            match save_backup(&fin, d) {
                Ok(p) => {
                    println!("Sauvegarde après le test : {}", p.display());
                    finals.push(p);
                }
                Err(e) => println!("Sauvegarde après le test impossible : {e}"),
            }
        }
        if let Some(last) = finals.first() {
            println!(
                "JSON « snapshot » des sauvegardes d'avant et d'après : {}",
                match (snapshot_json(first), snapshot_json(last)) {
                    (Some(a), Some(b)) if a == b => "identique",
                    (Some(_), Some(_)) => "DIFFÉRENT",
                    _ => "illisible",
                }
            );
        }
        if let (Some(a), Some(b)) = (&backup.raw, &fin.raw) {
            let diffs = raw_diffs(a, b);
            println!(
                "Octets relus différents de ceux d'avant le test (hors comparaison typée) : {}",
                if diffs.is_empty() {
                    "aucun".to_owned()
                } else {
                    diffs.join(" ; ")
                }
            );
        }
        Outcome::Passed
    }
}

/// What the keyboard returned for the bytes the typed blocks drop.
fn stored(part: Part, after: &Snapshot) -> Option<String> {
    let raw = after.raw.as_ref()?;
    let led = || {
        let b = &raw.led_effect;
        format!(
            "effet relu, octets 4, 13, 14, 15 : {:02X} {:02X} {:02X} {:02X}",
            b[4], b[13], b[14], b[15]
        )
    };
    let index = || {
        let off = raw
            .custom_leds
            .chunks_exact(4)
            .enumerate()
            .filter(|(i, e)| usize::from(e[0]) != *i)
            .count();
        if off == 0 {
            "index des couleurs relus = position (128/128)".to_owned()
        } else {
            format!("{off} index de couleur relus ≠ position")
        }
    };
    match part {
        Part::Led => Some(led()),
        Part::CustomLeds => Some(index()),
        Part::LedChain => Some(format!("{} ; {}", led(), index())),
        Part::Macros => Some(macro_layout(raw)),
        _ => None,
    }
}

fn macro_layout(raw: &RawBlocks) -> String {
    let pointers: Vec<String> = raw
        .macro_table
        .chunks_exact(4)
        .enumerate()
        .filter_map(|(id, p)| {
            let p = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
            (p != 0).then(|| format!("macro {id} → adresse {p}"))
        })
        .collect();
    let reads: Vec<String> = raw
        .macro_reads
        .iter()
        .map(|(addr, b)| format!("{addr} : {}", spaced(b)))
        .collect();
    format!(
        "table relue : {} ; zone relue : {}",
        if pointers.is_empty() {
            "aucun pointeur".to_owned()
        } else {
            pointers.join(", ")
        },
        if reads.is_empty() {
            "rien".to_owned()
        } else {
            reads.join(" ; ")
        }
    )
}

/// The bytes that differ, block by block (at most 16 shown per block).
fn raw_diffs(a: &RawBlocks, b: &RawBlocks) -> Vec<String> {
    let blocks: [(&str, &[u8], &[u8]); 8] = [
        ("deviceInfo", &a.device_info, &b.device_info),
        ("gameMode", &a.game_mode, &b.game_mode),
        ("keys", &a.keys, &b.keys),
        ("ledEffect", &a.led_effect, &b.led_effect),
        ("customLeds", &a.custom_leds, &b.custom_leds),
        ("macroTable", &a.macro_table, &b.macro_table),
        ("magnetic", &a.magnetic, &b.magnetic),
        ("dks", &a.dks, &b.dks),
    ];
    let hex = |v: Option<&u8>| v.map_or("--".to_owned(), |v| format!("{v:02X}"));
    let mut out = Vec::new();
    for (name, x, y) in blocks {
        let at: Vec<usize> = (0..x.len().max(y.len()))
            .filter(|&i| x.get(i) != y.get(i))
            .collect();
        if at.is_empty() {
            continue;
        }
        let shown: Vec<String> = at
            .iter()
            .take(16)
            .map(|&i| format!("{i} : {} → {}", hex(x.get(i)), hex(y.get(i))))
            .collect();
        out.push(format!(
            "{name} ({} octet(s)) {}",
            at.len(),
            shown.join(", ")
        ));
    }
    if a.macro_reads != b.macro_reads {
        let reads = |r: &[(u16, Vec<u8>)]| {
            r.iter()
                .map(|(addr, b)| format!("{} octets à {addr}", b.len()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        out.push(format!(
            "macroReads : [{}] → [{}]",
            reads(&a.macro_reads),
            reads(&b.macro_reads)
        ));
    }
    out
}

fn snapshot_json(path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(v.get_mut("snapshot")?.take())
}

/// Saves what the keyboard holds now (a read snapshot) and says where.
fn save_state(now: &Snapshot, dirs: &[PathBuf]) -> String {
    let saved: Vec<String> = dirs
        .iter()
        .map(|d| match save_backup(now, d) {
            Ok(p) => p.display().to_string(),
            Err(e) => e.to_string(),
        })
        .collect();
    format!(
        "état actuel du clavier enregistré dans : {}",
        saved.join(", ")
    )
}

/// The self-test: a backup saved in every folder of `dirs` (the first copy is loaded
/// back and used), then each part of [`PARTS`]. `Err` only before any write (keyboard
/// not found or unreadable, backup refused); after that the outcome is in the report.
pub(crate) fn run<L, C>(mut connect: C, dirs: &[PathBuf], wait: Wait) -> Result<Report>
where
    L: HidLink,
    C: FnMut() -> Result<Kb<L>>,
{
    let mut kb = connect()?;
    let initial = kb.read_snapshot()?;
    let backups = dirs
        .iter()
        .map(|d| save_backup(&initial, d))
        .collect::<Result<Vec<_>>>()?;
    let first = backups
        .first()
        .ok_or_else(|| Error::Backup("Aucun dossier de sauvegarde".into()))?
        .clone();
    let backup = load_backup(&first)?;
    if backup != initial {
        return Err(Error::Backup(format!(
            "La sauvegarde {} relue ne correspond pas au clavier : test annulé, rien n'a été écrit",
            first.display()
        )));
    }
    for p in &backups {
        println!("Sauvegarde avant toute écriture : {}", p.display());
    }

    let mut s = Session {
        kb,
        connect,
        wait,
        carried: (0, 0),
    };
    let (mut rows, mut outcome) = (Vec::new(), Outcome::Passed);
    for part in PARTS {
        let mut row = Row {
            part,
            steps: Vec::new(),
        };
        let stop = match s.test_part(part, &backup, &mut row) {
            Ok(true) => None,
            Ok(false) => Some(s.restore_all(&backup, dirs)),
            Err(Blocked(why)) => {
                row.push(Status::Fail, format!("BLOQUÉ : {why}"));
                Some(Outcome::Blocked(why))
            }
        };
        rows.push(row);
        if let Some(stop) = stop {
            outcome = stop;
            break;
        }
    }
    let mut finals = Vec::new();
    if outcome == Outcome::Passed {
        outcome = s.final_check(&backup, dirs, &first, &mut finals);
    }
    Ok(Report {
        rows,
        outcome,
        backups,
        finals,
    })
}

fn print_table(rows: &[Row]) {
    println!();
    println!(
        "{:<18} {:<11} {:<12} {:<13} {:<12}",
        "bloc", STEPS[0], STEPS[1], STEPS[2], STEPS[3]
    );
    for part in PARTS {
        let row = rows.iter().find(|r| r.part == part);
        let cell = |i: usize| {
            row.and_then(|r| r.steps.get(i))
                .map_or("—", |(s, _)| s.text())
        };
        println!(
            "{:<18} {:<11} {:<12} {:<13} {:<12}",
            part.name(),
            cell(0),
            cell(1),
            cell(2),
            cell(3)
        );
    }
}

/// `freejazz-cli selftest-writes [--yes] [--dir <d>]`: exit 0 when every step is OK or
/// SKIP, 1 after a FAIL whose restore verified, 2 without `--yes` (nothing sent), 3 when
/// BLOQUÉ.
pub(crate) fn command(yes: bool, dir: Option<PathBuf>, trace: bool) -> Result<ExitCode> {
    let names: Vec<&str> = PARTS.iter().map(|p| p.name()).collect();
    println!("Test des écritures, dans l'ordre : {}", names.join(", "));
    println!(
        "Pour chacun : {} (chaque écriture relue en entier et comparée)",
        STEPS.join(", ")
    );
    println!(
        "Modifications : mode de stabilité basculé (la fréquence d'envoi ne change pas), \
         luminosité −1, couleur personnalisée de l'index 0 → {}, F12 → volume +, \
         déclenchement de l'index 0 (ou de la première touche valide) +0,01 mm, DKS laissé tel quel s'il est vide, \
         macro {MACRO_ID} à 2 actions",
        color(ACCENT)
    );
    if !yes {
        eprintln!("Test non effectué : rien n'a été envoyé au clavier. Ajoutez --yes pour écrire.");
        return Ok(ExitCode::from(2));
    }
    let mut dirs = vec![default_backup_dir()?];
    dirs.extend(dir);
    let report = run(
        || connect(RESTORE, trace).map(|(kb, _)| kb),
        &dirs,
        Wait::USB,
    )?;
    print_table(&report.rows);
    Ok(match &report.outcome {
        Outcome::Passed => {
            println!("Résultat : toutes les écritures vérifiées ; le clavier est tel qu'il était.");
            for (before, after) in report.backups.iter().zip(&report.finals) {
                println!(
                    "  avant : {}
  après : {}",
                    before.display(),
                    after.display()
                );
            }
            ExitCode::SUCCESS
        }
        Outcome::Restored => {
            println!(
                "Résultat : ÉCHEC (voir le tableau) ; la sauvegarde a été réécrite et vérifiée, \
                 le clavier est tel qu'il était."
            );
            ExitCode::FAILURE
        }
        Outcome::Blocked(why) => {
            println!("Résultat : BLOQUÉ — {why}");
            for p in &report.backups {
                println!("Sauvegarde d'avant le test : {}", p.display());
            }
            println!(
                "N'envoyez rien d'autre au clavier avant d'avoir compris la cause ; \
                 retour à la sauvegarde : freejazz-cli restore <fichier> --yes"
            );
            ExitCode::from(3)
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, VecDeque};
    use std::ops::Range;
    use std::sync::{Arc, Mutex};

    use freejazz_protocol::blocks::{KeyAction, KeyKind, MagneticMap, Rgb};
    use freejazz_protocol::packet::check_report;

    use super::*;
    use crate::{LAYOUT, RESTORE};
    use std::time::Duration;

    const LEN: usize = 64;

    /// A keyboard that keeps what is written and serves it back, like the real one,
    /// with faults to inject. Shared by every link opened on it (reconnections).
    struct Fake {
        /// Block bytes, by read command (0x10-0x18).
        blocks: HashMap<u8, Vec<u8>>,
        /// Reports received so far, by write command.
        writes: HashMap<u8, usize>,
        /// Reports of `cmd` numbered in the range (from 0) are acknowledged, not stored.
        ignore: Option<(u8, Range<usize>)>,
        /// Reports of this write command are never acknowledged.
        mute: Option<u8>,
        /// The link dies right after this report of `cmd` (stored, not acknowledged).
        vanish_after: Option<(u8, usize)>,
        /// After the link died, no new link can be opened.
        gone: bool,
        /// Once an effect has been written, GET_LED_EFFECT reports #ffffff / RGB whatever
        /// is stored (firmware 1.28, protocol.md 9.7).
        white_led: bool,
        connects: usize,
        /// Every report received, in order.
        log: Vec<Vec<u8>>,
    }

    struct FakeLink {
        fake: Arc<Mutex<Fake>>,
        pending: VecDeque<Vec<u8>>,
        dead: bool,
    }

    impl HidLink for FakeLink {
        fn write(&mut self, r: &[u8]) -> Result<()> {
            if self.dead {
                return Err(Error::Hid("device gone".into()));
            }
            check_report(r)?;
            let mut guard = self.fake.lock().unwrap();
            let f = &mut *guard;
            f.log.push(r.to_vec());
            let (cmd, len) = (r[1], usize::from(r[2]));
            let addr = usize::from(u16::from_le_bytes([r[3], r[4]]));
            let block_cmd = if cmd >= 0x21 { cmd - 0x10 } else { cmd };
            let block = f.blocks.get_mut(&block_cmd).expect("a known block");
            let mut reply = vec![0u8; LEN];
            reply[..5].copy_from_slice(&[0x55, cmd, r[2], r[3], r[4]]);
            if cmd >= 0x21 {
                let n = f.writes.entry(cmd).or_default();
                let nth = *n;
                *n += 1;
                let ignored =
                    matches!(&f.ignore, Some((c, range)) if *c == cmd && range.contains(&nth));
                if !ignored {
                    block[addr..addr + len].copy_from_slice(&r[8..8 + len]);
                }
                if f.vanish_after == Some((cmd, nth)) {
                    f.vanish_after = None;
                    self.dead = true;
                    return Ok(());
                }
                if f.mute == Some(cmd) {
                    return Ok(());
                }
            } else {
                reply[8..8 + len].copy_from_slice(&block[addr..addr + len]);
                if cmd == 0x13 && f.white_led && f.writes.contains_key(&0x23) {
                    reply[9..12].copy_from_slice(&[0xFF; 3]);
                    reply[16] = 1;
                }
            }
            self.pending.push_back(reply);
            Ok(())
        }

        fn read(&mut self, buf: &mut [u8], _timeout_ms: i32) -> Result<usize> {
            if self.dead {
                return Err(Error::Hid("device gone".into()));
            }
            match self.pending.pop_front() {
                Some(r) => {
                    buf[..LEN].copy_from_slice(&r);
                    Ok(LEN)
                }
                None => Ok(0),
            }
        }

        fn report_len(&self) -> usize {
            LEN
        }
    }

    /// the tested keyboard as read on 2026-10-24 (protocol.md 9).
    fn tested_unit() -> Arc<Mutex<Fake>> {
        let mut info = vec![0u8; 48];
        info[..22].copy_from_slice(&[
            0, 0, 0, 0x92, 0x45, 0x0C, 0x43, 0x80, 0x28, 0x01, 0, 0, 0x66, 0x01, 0x04, 0x15, 0x11,
            0x10, 0, 0, 0x0E, 0,
        ]);
        let mut game = vec![0u8; 56];
        game[..16].copy_from_slice(&[0, 0, 0, 1, 0, 6, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0]);
        let mut keys = vec![0u8; 512];
        for (i, usage) in [
            (54, 0x0B),
            (99, 0x49),
            (103, 0x46),
            (106, 0x4A),
            (104, 0x4B),
            (105, 0x4E),
            (108, 0x4C),
        ] {
            keys[4 * i..4 * i + 4].copy_from_slice(&[2, 0, usage, 0]);
        }
        let led = vec![9, 0xFF, 0xB0, 0xB0, 0, 0, 0, 0, 0, 6, 3, 0, 0, 0, 0, 0];
        let mut custom = vec![0u8; 512];
        let mut magnetic = vec![0u8; 1024];
        for i in 0..128usize {
            custom[4 * i] = i as u8;
        }
        for k in &LAYOUT.keys {
            let i = k.index;
            custom[4 * i + 1..4 * i + 4].copy_from_slice(&[0xFF; 3]);
            let trigger: u16 = if k.label == "Spacebar" { 300 } else { 150 };
            magnetic[8 * i] = 1;
            magnetic[8 * i + 2..8 * i + 4].copy_from_slice(&trigger.to_le_bytes());
            magnetic[8 * i + 4..8 * i + 6].copy_from_slice(&10u16.to_le_bytes());
            magnetic[8 * i + 6..8 * i + 8].copy_from_slice(&10u16.to_le_bytes());
        }
        let blocks = HashMap::from([
            (0x10, info),
            (0x11, game),
            (0x12, keys),
            (0x13, led),
            (0x14, custom),
            (0x15, vec![0u8; 0x1_0000 + LEN]),
            (0x17, magnetic),
            (0x18, vec![0u8; 1024]),
        ]);
        Arc::new(Mutex::new(Fake {
            blocks,
            writes: HashMap::new(),
            ignore: None,
            mute: None,
            vanish_after: None,
            gone: false,
            white_led: false,
            connects: 0,
            log: Vec::new(),
        }))
    }

    fn connector(
        fake: &Arc<Mutex<Fake>>,
    ) -> impl FnMut() -> Result<Keyboard<Guarded<FakeLink>>> + use<> {
        let fake = fake.clone();
        move || {
            let mut f = fake.lock().unwrap();
            f.connects += 1;
            if f.gone && f.connects > 1 {
                return Err(Error::NotFound);
            }
            drop(f);
            let link = FakeLink {
                fake: fake.clone(),
                pending: VecDeque::new(),
                dead: false,
            };
            Keyboard::connect(Guarded::new(link, RESTORE, false))
        }
    }

    const FAST: Wait = Wait {
        timeout: Duration::from_millis(50),
        poll: Duration::from_millis(1),
    };

    /// What the keyboard holds now, read through a new link.
    fn held(fake: &Arc<Mutex<Fake>>) -> Snapshot {
        let mut kb = connector(fake)().unwrap();
        kb.read_snapshot().unwrap()
    }

    fn statuses(report: &Report) -> Vec<(Part, Vec<Status>)> {
        report
            .rows
            .iter()
            .map(|r| (r.part, r.steps.iter().map(|(s, _)| *s).collect()))
            .collect()
    }

    fn files(dir: &std::path::Path) -> usize {
        std::fs::read_dir(dir).unwrap().count()
    }

    #[test]
    fn every_block_is_written_changed_and_restored() {
        use Status::*;
        let fake = tested_unit();
        let before = held(&fake);
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let dirs = [a.path().to_path_buf(), b.path().to_path_buf()];
        let report = run(connector(&fake), &dirs, FAST).unwrap();

        assert_eq!(report.outcome, Outcome::Passed);
        let all_ok = vec![Ok, Ok, Ok, Ok];
        assert_eq!(
            statuses(&report),
            vec![
                (Part::GameMode, all_ok.clone()),
                (Part::Led, all_ok.clone()),
                (Part::CustomLeds, all_ok.clone()),
                (Part::Keys, all_ok.clone()),
                (Part::Magnetic, all_ok.clone()),
                (Part::Dks, vec![Ok, Ok, Skip, Ok]),
                (Part::Macros, all_ok.clone()),
                (Part::LedChain, all_ok),
            ]
        );
        // left as found (typed), backups before and after in both folders
        assert_eq!(held(&fake), before);
        assert_eq!((files(a.path()), files(b.path())), (2, 2));
        assert_eq!(report.backups.len(), 2);
        assert_eq!(report.finals.len(), 2);

        let f = fake.lock().unwrap();
        // the first write of each block, in the brief's order
        let mut order: Vec<u8> = f.log.iter().map(|r| r[1]).filter(|c| *c >= 0x21).collect();
        order.dedup();
        assert_eq!(
            order,
            vec![
                0x21, 0x23, 0x24, 0x22, 0x27, 0x28, 0x25, 0x23, 0x24, 0x23, 0x24, 0x23, 0x24
            ]
        );
        // only the site's reads and block writes, never the Fn layer
        assert!(f.log.iter().all(|r| RESTORE.contains(&r[1])));
        // the LED effect went out the site's way (FF, AA 55), the 6 nav remaps stayed
        assert_eq!(&f.blocks[&0x13][13..16], &[0, 0xAA, 0x55]);
        assert_eq!(f.blocks[&0x13][4], 0xFF);
        assert_eq!(&f.blocks[&0x12][4 * 12..4 * 13], &[0, 0, 0, 0]);
        assert_eq!(&f.blocks[&0x12][4 * 99..4 * 100], &[2, 0, 0x49, 0]);
    }

    #[test]
    fn changed_values_are_the_briefs() {
        let fake = tested_unit();
        let s = held(&fake);
        let to = |part: Part| match part.change(&s).unwrap() {
            Change::To(c, what) => {
                assert!(!what.is_empty());
                assert!(c.raw.is_none(), "an edited snapshot has no bytes as read");
                *c
            }
            Change::Skip(why) => panic!("{part:?} skipped: {why}"),
        };

        let g = to(Part::GameMode);
        assert_eq!(g.game_mode.stability_mode(), 0);
        assert_eq!(
            g.game_mode.report_rate_code(),
            6,
            "the report rate never changes"
        );
        assert_eq!(g.game_mode.raw[..11], s.game_mode.raw[..11]);
        assert_eq!(g.game_mode.raw[12..], s.game_mode.raw[12..]);

        assert_eq!(to(Part::Led).led_effect.brightness, 5);
        let accent = Rgb {
            r: 0x7c,
            g: 0x5c,
            b: 0xff,
        };
        assert_eq!(to(Part::CustomLeds).custom_leds.0[0], accent);
        let chain = to(Part::LedChain);
        assert_eq!(
            (chain.led_effect.brightness, chain.custom_leds.0[0]),
            (5, accent)
        );

        let keys = to(Part::Keys).keys;
        assert_eq!(
            keys.0[12],
            KeyAction::from_kind(KeyKind::Consumer(0x00E9)),
            "F12 is index 12"
        );
        assert_eq!(keys.0[99], s.keys.0[99]);

        // 1.50 → 1.51 mm, exactly what a read of 151 decodes to
        let m = to(Part::Magnetic).magnetic;
        let mut bytes = s.magnetic.encode(0).unwrap();
        bytes[2..4].copy_from_slice(&151u16.to_le_bytes());
        assert_eq!(m, MagneticMap::decode(&bytes, 0).unwrap());

        assert!(matches!(Part::Dks.change(&s).unwrap(), Change::Skip(_)));

        // macro 99, 2 actions, as the keyboard returns it (header and flags as stored)
        let macros = to(Part::Macros).macros;
        assert_eq!(macros.slots.len(), 1);
        let m = &macros.slots[0];
        assert_eq!((m.id, m.actions.len()), (99, 2));
        assert_eq!(m.raw_header, Some([4, 0, 0, 0]));
        let flags: Vec<_> = m.actions.iter().map(|a| a.raw_flags).collect();
        assert_eq!(flags, vec![Some(0xB0), Some(0x30)]);
    }

    #[test]
    fn the_magnetic_change_falls_back_to_a_key_within_the_limits() {
        let fake = tested_unit();
        let mut s = held(&fake);
        let mut bytes = s.magnetic.encode(0).unwrap();
        bytes[..8].fill(0); // index 0 (Esc) empty
        s.magnetic = MagneticMap::decode(&bytes, 0).unwrap();
        let Change::To(c, what) = Part::Magnetic.change(&s).unwrap() else {
            panic!("skipped");
        };
        let first = LAYOUT
            .keys
            .iter()
            .map(|k| k.index)
            .find(|&i| i != 0)
            .unwrap();
        assert!(what.contains(&format!("index {first}")), "{what}");
        bytes[8 * first + 2..8 * first + 4].copy_from_slice(&151u16.to_le_bytes());
        assert_eq!(c.magnetic, MagneticMap::decode(&bytes, 0).unwrap());

        // a table of zeros (after a reset): nothing to change
        s.magnetic = MagneticMap::default();
        assert!(matches!(
            Part::Magnetic.change(&s).unwrap(),
            Change::Skip(_)
        ));
    }

    #[test]
    fn a_fail_stops_the_test_and_restores_the_backup() {
        use Status::*;
        let fake = tested_unit();
        let before = held(&fake);
        // the changed LED effect is acknowledged but not kept
        fake.lock().unwrap().ignore = Some((0x23, 1..2));
        let dir = tempfile::tempdir().unwrap();
        let report = run(connector(&fake), &[dir.path().to_path_buf()], FAST).unwrap();

        assert_eq!(report.outcome, Outcome::Restored);
        assert_eq!(
            statuses(&report),
            vec![
                (Part::GameMode, vec![Ok, Ok, Ok, Ok]),
                (Part::Led, vec![Ok, Ok, Fail]),
            ]
        );
        let fail = &report.rows[1].steps[2].1;
        assert!(fail.contains("effet lumineux"), "{fail}");
        assert_eq!(held(&fake), before);
        assert!(report.finals.is_empty());
    }

    #[test]
    fn a_write_the_keyboard_does_not_acknowledge_is_a_fail() {
        use Status::*;
        let fake = tested_unit();
        let before = held(&fake);
        fake.lock().unwrap().mute = Some(0x28);
        let dir = tempfile::tempdir().unwrap();
        let report = run(connector(&fake), &[dir.path().to_path_buf()], FAST).unwrap();

        assert_eq!(report.outcome, Outcome::Restored);
        let (part, steps) = statuses(&report).pop().unwrap();
        assert_eq!((part, steps), (Part::Dks, vec![Ok, Fail]));
        assert!(report.rows[5].steps[1].1.contains("écriture"));
        assert_eq!(held(&fake), before);
    }

    #[test]
    fn a_keyboard_that_re_enumerates_is_waited_for() {
        let fake = tested_unit();
        let before = held(&fake);
        let connects = fake.lock().unwrap().connects;
        fake.lock().unwrap().vanish_after = Some((0x21, 0));
        let dir = tempfile::tempdir().unwrap();
        let report = run(connector(&fake), &[dir.path().to_path_buf()], FAST).unwrap();

        assert_eq!(report.outcome, Outcome::Passed);
        let (status, note) = &report.rows[0].steps[1];
        assert_eq!(*status, Status::Ok);
        assert!(note.contains("reconnecté"), "{note}");
        assert_eq!(fake.lock().unwrap().connects, connects + 2);
        assert_eq!(held(&fake), before);
    }

    #[test]
    fn a_keyboard_that_does_not_come_back_is_blocked() {
        let fake = tested_unit();
        {
            let mut f = fake.lock().unwrap();
            f.vanish_after = Some((0x21, 0));
            f.gone = true;
        }
        let dir = tempfile::tempdir().unwrap();
        let report = run(connector(&fake), &[dir.path().to_path_buf()], FAST).unwrap();

        assert!(matches!(report.outcome, Outcome::Blocked(_)));
        assert_eq!(report.rows.len(), 1);
        assert_eq!(report.backups.len(), 1);
    }

    #[test]
    fn a_restore_that_cannot_be_verified_is_blocked() {
        let fake = tested_unit();
        // from the third SET_LED_EFFECT on (the restore), nothing is kept
        fake.lock().unwrap().ignore = Some((0x23, 2..usize::MAX));
        let dir = tempfile::tempdir().unwrap();
        let report = run(connector(&fake), &[dir.path().to_path_buf()], FAST).unwrap();

        let Outcome::Blocked(why) = &report.outcome else {
            panic!("{:?}", report.outcome);
        };
        assert!(why.contains("effet lumineux"), "{why}");
        assert_eq!(
            statuses(&report)[1],
            (
                Part::Led,
                vec![Status::Ok, Status::Ok, Status::Ok, Status::Fail]
            )
        );
        // the initial backup and the state the keyboard was left in
        assert_eq!(files(dir.path()), 2);
        assert_eq!(held(&fake).led_effect.brightness, 5);
    }

    #[test]
    fn an_led_colour_the_keyboard_reports_as_white_is_not_a_fail() {
        let fake = tested_unit();
        let before = held(&fake);
        fake.lock().unwrap().white_led = true;
        let dir = tempfile::tempdir().unwrap();
        let report = run(connector(&fake), &[dir.path().to_path_buf()], FAST).unwrap();

        assert_eq!(report.outcome, Outcome::Passed);
        let (status, note) = &report.rows[1].steps[1];
        assert_eq!(*status, Status::Ok);
        assert!(note.contains("non vérifiables par relecture"), "{note}");
        // a brightness the keyboard did not keep is still a FAIL
        let fake = tested_unit();
        {
            let mut f = fake.lock().unwrap();
            f.white_led = true;
            f.ignore = Some((0x23, 1..2));
        }
        let report = run(connector(&fake), &[dir.path().to_path_buf()], FAST).unwrap();
        assert_eq!(report.outcome, Outcome::Restored);
        assert_eq!(report.rows[1].steps[2].0, Status::Fail);
        fake.lock().unwrap().white_led = false;
        assert_eq!(held(&fake), before);
    }

    #[test]
    fn without_yes_nothing_is_sent() {
        assert_eq!(command(false, None, false).unwrap(), ExitCode::from(2));
    }
}
