//! Custom effects: a stack of layers, saved as JSON in `%APPDATA%\Freejazz\effects\`
//! (one `<name>.json` each).

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Composite, Effect, create};
use crate::keyboard::Layout;

/// One layer: an effect (by id, with its parameters) painting only `keys`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub effect: String,
    #[serde(default)]
    pub params: Value,
    pub keys: BTreeSet<u8>,
}

impl Layer {
    /// A layer over every key of the keyboard.
    pub fn all_keys(effect: &str, params: Value) -> Self {
        Self {
            effect: effect.to_owned(),
            params,
            keys: Layout::ak980max().keys.iter().map(|k| k.index).collect(),
        }
    }
}

/// Layers drawn bottom to top.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomEffect {
    pub name: String,
    pub layers: Vec<Layer>,
}

impl CustomEffect {
    /// The effect to run; layers with an unknown effect id are skipped.
    pub fn build(&self) -> Box<dyn Effect> {
        Box::new(Composite::new(
            self.layers
                .iter()
                .filter_map(|l| Some((create(&l.effect, &l.params)?, l.keys.clone())))
                .collect(),
        ))
    }
}

/// `%APPDATA%\Freejazz\effects`.
pub fn effects_dir() -> Option<PathBuf> {
    crate::paths::app_data_dir().map(|d| d.join("effects"))
}

/// Saves (or replaces) the effect; returns its file.
pub fn save(dir: &Path, effect: &CustomEffect) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(crate::designs::file_name(&effect.name));
    std::fs::write(&path, serde_json::to_vec_pretty(effect)?)?;
    Ok(path)
}

pub fn load(path: &Path) -> io::Result<CustomEffect> {
    serde_json::from_slice(&std::fs::read(path)?).map_err(io::Error::other)
}

/// The saved effects by name; files that are not effects are ignored.
pub fn list(dir: &Path) -> Vec<CustomEffect> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut all: Vec<CustomEffect> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| load(&e.path()).ok())
        .collect();
    all.sort_by_key(|e| e.name.to_lowercase());
    all
}

pub fn delete(dir: &Path, name: &str) -> io::Result<()> {
    std::fs::remove_file(dir.join(crate::designs::file_name(name)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let effect = CustomEffect {
            name: "Mon effet/test".into(),
            layers: vec![
                Layer::all_keys("wave", json!({"speed": 1.5})),
                Layer {
                    effect: "ripple".into(),
                    params: json!({"color": "#7c5cff"}),
                    keys: [1u8, 2, 3].into(),
                },
            ],
        };
        let path = save(dir.path(), &effect).unwrap();
        assert!(path.ends_with("Mon effet_test.json"));
        assert_eq!(load(&path).unwrap(), effect);
        assert_eq!(list(dir.path()), vec![effect.clone()]);
        std::fs::write(dir.path().join("junk.json"), b"{}").unwrap();
        assert_eq!(list(dir.path()).len(), 1);
        delete(dir.path(), &effect.name).unwrap();
        assert!(list(dir.path()).is_empty());
    }

    #[test]
    fn unknown_layers_are_skipped_when_built() {
        let e = CustomEffect {
            name: "x".into(),
            layers: vec![Layer::all_keys("nope", Value::Null)],
        };
        assert_eq!(e.build().id(), "custom");
    }
}
