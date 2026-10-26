//! The designs library: per-key colour maps saved in `%APPDATA%\Freejazz\designs\`
//! (one `<name>.json` each) and the designs that ship with the app.

use std::io;
use std::path::{Path, PathBuf};

use freejazz_protocol::blocks::{CustomLeds, Rgb};
use serde::{Deserialize, Serialize};

use crate::keyboard::Layout;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Design {
    pub name: String,
    pub leds: CustomLeds,
}

/// Whether Windows reserves `name` for a device (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`,
/// `LPT1`–`LPT9`, also with an extension such as `NUL.txt`): such a file cannot be
/// created.
pub fn is_reserved_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && upper.as_bytes()[3].is_ascii_digit()
            && upper.as_bytes()[3] != b'0')
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && ["¹", "²", "³"].iter().any(|d| upper[3..] == **d))
}

/// The file a design is stored in: characters Windows refuses in a file name become
/// `_` (`"a/b:c"` gives `a_b_c.json`), and a name Windows reserves gets a `_` in front
/// (`"CON"` gives `_CON.json`).
pub fn file_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let clean = clean.trim().trim_end_matches('.').trim();
    let clean = if clean.is_empty() { "Sans nom" } else { clean };
    if is_reserved_name(clean) {
        format!("_{clean}.json")
    } else {
        format!("{clean}.json")
    }
}

/// Saves (or replaces) the design called `name`; returns its file.
pub fn save_design(dir: &Path, name: &str, leds: &CustomLeds) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(file_name(name));
    let design = Design {
        name: name.trim().to_owned(),
        leds: leds.clone(),
    };
    std::fs::write(&path, serde_json::to_vec_pretty(&design)?)?;
    Ok(path)
}

/// The saved designs by name; files that are not designs are ignored.
pub fn list_designs(dir: &Path) -> Vec<Design> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut designs: Vec<Design> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok())
        .collect();
    designs.sort_by_key(|d| d.name.to_lowercase());
    designs
}

pub fn delete_design(dir: &Path, name: &str) -> io::Result<()> {
    std::fs::remove_file(dir.join(file_name(name)))
}

/// A design from a colour per key: `colour(t, key)` with `t` the key's position from
/// left (0) to right (1). Slots without a key stay off.
fn painted(layout: &Layout, colour: impl Fn(f32, &crate::keyboard::KeyDef) -> Rgb) -> CustomLeds {
    let min = layout.keys.iter().map(|k| k.x).fold(f32::MAX, f32::min);
    let max = layout
        .keys
        .iter()
        .map(|k| k.x + k.w)
        .fold(f32::MIN, f32::max);
    let mut leds = CustomLeds::default();
    for k in &layout.keys {
        let t = ((k.x + k.w / 2.0 - min) / (max - min)).clamp(0.0, 1.0);
        leds.0[usize::from(k.index)] = colour(t, k);
    }
    leds
}

pub fn hsv(h: f32, s: f32, v: f32) -> Rgb {
    let c = v * s;
    let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (h.fract() * 6.0) as u8 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let byte = |f: f32| ((f + m) * 255.0).round() as u8;
    Rgb {
        r: byte(r),
        g: byte(g),
        b: byte(b),
    }
}

/// Linear blend of two colours.
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Rgb {
        r: f(a.r, b.r),
        g: f(a.g, b.g),
        b: f(a.b, b.b),
    }
}

/// The designs that ship with the app (not stored in the folder, not deletable).
pub fn bundled() -> Vec<Design> {
    let layout = Layout::ak980max();
    let rgb = |r, g, b| Rgb { r, g, b };
    // physical keys of Z Q S D on an AZERTY keyboard: W A S D usages
    let zqsd = painted(layout, |_, k| {
        if matches!(k.hid, 0x1A | 0x04 | 0x16 | 0x07) {
            rgb(255, 90, 0)
        } else {
            rgb(18, 8, 40)
        }
    });
    let violet = painted(layout, |t, _| mix(rgb(50, 10, 120), rgb(220, 150, 255), t));
    let rainbow = painted(layout, |t, _| hsv(t * 0.83, 1.0, 1.0));
    let france = painted(layout, |t, _| {
        if t < 1.0 / 3.0 {
            rgb(0, 85, 164)
        } else if t < 2.0 / 3.0 {
            rgb(255, 255, 255)
        } else {
            rgb(239, 65, 53)
        }
    });
    [
        ("Jeu ZQSD", zqsd),
        ("Dégradé violet", violet),
        ("Arc-en-ciel", rainbow),
        ("France", france),
    ]
    .into_iter()
    .map(|(name, leds)| Design {
        name: name.to_owned(),
        leds,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_then_list_design() {
        let dir = tempfile::tempdir().unwrap();
        let mut leds = CustomLeds::default();
        leds.0[5] = Rgb { r: 1, g: 2, b: 3 };
        assert!(list_designs(&dir.path().join("none")).is_empty());
        save_design(dir.path(), "Mon dessin", &leds).unwrap();
        save_design(dir.path(), "Autre", &CustomLeds::default()).unwrap();
        std::fs::write(dir.path().join("note.json"), "{}").unwrap();
        let list = list_designs(dir.path());
        assert_eq!(
            list.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["Autre", "Mon dessin"]
        );
        assert_eq!(list[1].leds, leds);
        delete_design(dir.path(), "Autre").unwrap();
        assert_eq!(list_designs(dir.path()).len(), 1);
    }

    #[test]
    fn design_name_sanitized() {
        assert_eq!(file_name("a/b:c"), "a_b_c.json");
        assert_eq!(file_name("  "), "Sans nom.json");
        for reserved in ["CON", "nul", "Com1", "LPT9", "aux.json", "NUL.txt", "COM²"] {
            assert!(is_reserved_name(reserved), "{reserved}");
        }
        for fine in ["CONSOLE", "COM0", "COM10", "Nulle", "LPT", "Jeu"] {
            assert!(!is_reserved_name(fine), "{fine}");
        }
        assert_eq!(file_name("con"), "_con.json");
        let dir = tempfile::tempdir().unwrap();
        save_design(dir.path(), "NUL", &CustomLeds::default()).unwrap();
        assert_eq!(list_designs(dir.path())[0].name, "NUL");
        let dir = tempfile::tempdir().unwrap();
        let path = save_design(dir.path(), "a/b:c", &CustomLeds::default()).unwrap();
        assert_eq!(path.file_name().unwrap(), "a_b_c.json");
        assert_eq!(list_designs(dir.path())[0].name, "a/b:c");
    }

    #[test]
    fn bundled_designs_light_only_real_keys() {
        let layout = Layout::ak980max();
        let all = bundled();
        let names: Vec<&str> = all.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(
            names,
            ["Jeu ZQSD", "Dégradé violet", "Arc-en-ciel", "France"]
        );
        for d in &all {
            for slot in 0..128u8 {
                if layout.key(slot).is_none() {
                    assert_eq!(d.leds.0[usize::from(slot)], Rgb::default(), "{}", d.name);
                }
            }
        }
        // the four game keys stand out
        let zqsd = &all[0].leds;
        let lit = layout
            .keys
            .iter()
            .filter(|k| zqsd.0[usize::from(k.index)].r == 255)
            .count();
        assert_eq!(lit, 4);
        assert_ne!(all[3].leds.0[1], all[3].leds.0[100]);
    }
}
