//! Global (user-level) configuration store behind `spectra config`.
//!
//! Unlike [`crate::config::Config`] (the *project* `.spectra.yaml`), this
//! store is a flat YAML mapping in the user's platform config directory —
//! `~/Library/Application Support/openspec/config.yaml` on macOS,
//! `$XDG_CONFIG_HOME/openspec/config.yaml` (or `~/.config/...`) elsewhere —
//! and needs no initialized project. Probed behaviors (oracle 2.3.1, see
//! `docs/reverse-engineering/config.md`):
//!
//! * Keys are flat strings: `claude_effort.apply` is a literal key, not a
//!   nested path.
//! * Values are parsed as YAML scalars/collections (`true` → bool, `42` →
//!   int, `[1, 2]` → sequence); anything unparseable falls back to a string.
//! * A missing, unreadable, unparseable, or non-mapping file silently reads
//!   as empty, and the next write overwrites it.
//! * `unset` saves even when nothing was removed (creating a `{}` file);
//!   `reset` deletes the file and is idempotent.

use anyhow::{Context, Result};
use serde_yaml::{Mapping, Value};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Absolute path of the global config file, resolved from the environment.
///
/// Follows `$HOME` (probed: the oracle honors a `HOME` override on macOS and
/// ignores `XDG_CONFIG_HOME` there); the Linux XDG branch is the standard
/// config-dir convention and is unverifiable against the macOS-only oracle.
pub fn config_path() -> Result<PathBuf> {
    resolve_config_path(
        cfg!(target_os = "macos"),
        std::env::var_os("HOME").as_deref(),
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
    )
    .ok_or_else(|| anyhow::anyhow!("could not determine the config directory (HOME is not set)"))
}

/// Pure resolver behind [`config_path`], split from the env reads so every
/// platform/env combination is unit-testable without mutating process env
/// vars (which would be racy under the parallel test harness).
fn resolve_config_path(
    macos_layout: bool,
    home: Option<&OsStr>,
    xdg_config_home: Option<&OsStr>,
) -> Option<PathBuf> {
    let home = home.filter(|h| !h.is_empty());
    let config_dir = if macos_layout {
        Path::new(home?).join("Library/Application Support")
    } else {
        match xdg_config_home.filter(|x| !x.is_empty()) {
            Some(xdg) => PathBuf::from(xdg),
            None => Path::new(home?).join(".config"),
        }
    };
    Some(config_dir.join("openspec").join("config.yaml"))
}

/// Read the settings mapping. Lenient by design (probed): a missing,
/// unreadable, unparseable, or non-mapping file all read as empty — the
/// oracle reports "No configuration set." for a corrupt file and lets the
/// next `set` overwrite it, so failing loudly here would diverge.
pub fn load(path: &Path) -> Mapping {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Mapping::new();
    };
    match serde_yaml::from_str::<Value>(&text) {
        Ok(Value::Mapping(map)) => map,
        _ => Mapping::new(),
    }
}

/// Write the settings mapping, creating parent directories as needed.
pub fn save(path: &Path, settings: &Mapping) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let text = serde_yaml::to_string(settings).context("serializing config")?;
    crate::init::write_atomically(path, &text)
        .with_context(|| format!("writing {}", path.display()))
}

/// Look up a key. Keys are literal flat strings (probed: dotted keys are not
/// nested paths).
pub fn get_value<'a>(settings: &'a Mapping, key: &str) -> Option<&'a Value> {
    settings.get(Value::String(key.to_string()))
}

/// Parse `set`'s raw CLI value the way the oracle does: as a YAML document
/// (so `true`/`42`/`[1, 2]` keep their types and `""` becomes null), falling
/// back to a literal string when it does not parse. `--string` skips parsing
/// entirely.
pub fn parse_value(raw: &str, force_string: bool) -> Value {
    if force_string {
        return Value::String(raw.to_string());
    }
    serde_yaml::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

/// Render a value for `get`/`list` output, byte-matching the oracle: strings
/// print raw, bool/number via their scalar form, and everything else (null,
/// sequences, mappings) through the YAML serializer — whose trailing newline
/// is why `get` on a null key prints `null\n\n` (probed).
pub fn render_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => serde_yaml::to_string(other).unwrap_or_default(),
    }
}

/// Convert a YAML value to JSON for `list --json`, preserving scalar types.
/// Non-string mapping keys (possible only in a hand-edited file) are
/// stringified; non-finite floats become null (JSON has no NaN).
pub fn to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => (*b).into(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.into()
            } else if let Some(u) = n.as_u64() {
                u.into()
            } else {
                n.as_f64()
                    .and_then(serde_json::Number::from_f64)
                    .map_or(serde_json::Value::Null, serde_json::Value::Number)
            }
        }
        Value::String(s) => s.clone().into(),
        Value::Sequence(seq) => seq.iter().map(to_json).collect(),
        Value::Mapping(map) => serde_json::Value::Object(
            map.iter()
                .map(|(k, v)| (key_string(k), to_json(v)))
                .collect(),
        ),
        Value::Tagged(tagged) => to_json(&tagged.value),
    }
}

/// A mapping key as a display string (string keys verbatim; anything else via
/// its rendered scalar form, trimmed of the serializer's trailing newline).
pub fn key_string(key: &Value) -> String {
    match key {
        Value::String(s) => s.clone(),
        other => render_value(other).trim_end().to_string(),
    }
}

/// `config set`: load-modify-save. Overwrites a corrupt file (probed).
pub fn set_value(path: &Path, key: &str, raw: &str, force_string: bool) -> Result<()> {
    let mut settings = load(path);
    settings.insert(
        Value::String(key.to_string()),
        parse_value(raw, force_string),
    );
    save(path, &settings)
}

/// `config unset`: remove and save unconditionally — the oracle reports
/// success for a missing key and even creates a `{}` file when none existed
/// (probed), so this does not distinguish "removed" from "was absent".
pub fn unset_value(path: &Path, key: &str) -> Result<()> {
    let mut settings = load(path);
    settings.remove(Value::String(key.to_string()));
    save(path, &settings)
}

/// `config reset`: delete the file. Idempotent (probed: exits 0 when the
/// file is already gone).
pub fn reset(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn os(s: &str) -> &OsStr {
        OsStr::new(s)
    }

    #[test]
    fn resolve_config_path_macos_uses_application_support_and_ignores_xdg() {
        assert_eq!(
            resolve_config_path(true, Some(os("/Users/u")), Some(os("/xdg"))),
            Some(PathBuf::from(
                "/Users/u/Library/Application Support/openspec/config.yaml"
            ))
        );
    }

    #[test]
    fn resolve_config_path_linux_prefers_xdg_then_home_dot_config() {
        assert_eq!(
            resolve_config_path(false, Some(os("/home/u")), Some(os("/xdg"))),
            Some(PathBuf::from("/xdg/openspec/config.yaml"))
        );
        assert_eq!(
            resolve_config_path(false, Some(os("/home/u")), None),
            Some(PathBuf::from("/home/u/.config/openspec/config.yaml"))
        );
    }

    #[test]
    fn resolve_config_path_treats_empty_env_values_as_unset() {
        assert_eq!(
            resolve_config_path(false, Some(os("/home/u")), Some(os(""))),
            Some(PathBuf::from("/home/u/.config/openspec/config.yaml"))
        );
        assert_eq!(resolve_config_path(true, Some(os("")), None), None);
        assert_eq!(resolve_config_path(false, None, None), None);
    }

    #[test]
    fn parse_value_types_scalars_like_the_oracle() {
        assert_eq!(parse_value("true", false), Value::Bool(true));
        assert_eq!(parse_value("TRUE", false), Value::Bool(true));
        assert_eq!(parse_value("false", false), Value::Bool(false));
        assert_eq!(parse_value("42", false), Value::Number(42.into()));
        assert_eq!(parse_value("2.5", false), Value::Number(2.5.into()));
        assert_eq!(parse_value("null", false), Value::Null);
        // Probed: `set empty ""` stores null (a YAML empty scalar).
        assert_eq!(parse_value("", false), Value::Null);
        assert_eq!(
            parse_value("hello world", false),
            Value::String("hello world".into())
        );
    }

    #[test]
    fn parse_value_parses_flow_collections() {
        let arr = parse_value("[1, 2, 3]", false);
        assert_eq!(arr, Value::Sequence(vec![1.into(), 2.into(), 3.into()]));
        let obj = parse_value("{a: 1}", false);
        let Value::Mapping(map) = obj else {
            panic!("expected mapping, got {obj:?}");
        };
        assert_eq!(map.get(Value::String("a".into())), Some(&Value::from(1)));
    }

    #[test]
    fn parse_value_falls_back_to_a_literal_string_when_unparseable() {
        // Probed: `set weird "{unclosed"` stores the raw string.
        assert_eq!(
            parse_value("{unclosed", false),
            Value::String("{unclosed".into())
        );
        assert_eq!(
            parse_value("a: b: c", false),
            Value::String("a: b: c".into())
        );
    }

    #[test]
    fn parse_value_with_force_string_skips_yaml_parsing() {
        assert_eq!(parse_value("true", true), Value::String("true".into()));
        assert_eq!(parse_value("", true), Value::String(String::new()));
    }

    #[test]
    fn render_value_matches_probed_oracle_bytes() {
        // Scalars have no trailing newline...
        assert_eq!(render_value(&Value::String("hello".into())), "hello");
        assert_eq!(render_value(&Value::Bool(true)), "true");
        assert_eq!(render_value(&Value::Number(42.into())), "42");
        assert_eq!(render_value(&Value::Number(2.5.into())), "2.5");
        // ...but null and collections go through the YAML serializer, whose
        // trailing newline the oracle passes straight through.
        assert_eq!(render_value(&Value::Null), "null\n");
        assert_eq!(
            render_value(&Value::Sequence(vec![1.into(), 2.into(), 3.into()])),
            "- 1\n- 2\n- 3\n"
        );
        let map = parse_value("{a: 1}", false);
        assert_eq!(render_value(&map), "a: 1\n");
    }

    #[test]
    fn load_reads_missing_corrupt_and_non_mapping_files_as_empty() {
        let dir = TempDir::new("gconfig-load");
        let path = dir.join("config.yaml");
        assert!(load(&path).is_empty(), "missing file");

        std::fs::write(&path, "not: [valid: yaml\n").unwrap();
        assert!(load(&path).is_empty(), "corrupt file");

        std::fs::write(&path, "- 1\n- 2\n").unwrap();
        assert!(load(&path).is_empty(), "non-mapping file");
    }

    #[test]
    fn set_get_round_trips_through_the_file() {
        let dir = TempDir::new("gconfig-roundtrip");
        let path = dir.join("nested").join("config.yaml");
        set_value(&path, "tdd", "true", false).unwrap();
        set_value(&path, "claude_effort.apply", "high", false).unwrap();

        let settings = load(&path);
        assert_eq!(get_value(&settings, "tdd"), Some(&Value::Bool(true)));
        // Probed: dotted keys are literal flat keys, not nested paths.
        assert_eq!(
            get_value(&settings, "claude_effort.apply"),
            Some(&Value::String("high".into()))
        );
        assert_eq!(get_value(&settings, "claude_effort"), None);
    }

    #[test]
    fn set_with_force_string_quotes_the_scalar_in_the_file() {
        let dir = TempDir::new("gconfig-string");
        let path = dir.join("config.yaml");
        set_value(&path, "tdd", "true", true).unwrap();
        // Probed file bytes: a forced-string "true" is quoted to stay a string.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "tdd: 'true'\n");
    }

    #[test]
    fn set_overwrites_a_corrupt_file() {
        let dir = TempDir::new("gconfig-corrupt");
        let path = dir.join("config.yaml");
        std::fs::write(&path, "not: [valid: yaml\n").unwrap();
        set_value(&path, "k", "v", false).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "k: v\n");
    }

    #[test]
    fn unset_is_idempotent_and_creates_an_empty_file_when_missing() {
        let dir = TempDir::new("gconfig-unset");
        let path = dir.join("config.yaml");
        // Probed: unset on a missing file succeeds and leaves a `{}` file.
        unset_value(&path, "ghost").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}\n");

        set_value(&path, "only", "one", false).unwrap();
        unset_value(&path, "only").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}\n");
        // Unsetting again is still fine.
        unset_value(&path, "only").unwrap();
    }

    #[test]
    fn reset_deletes_the_file_and_is_idempotent() {
        let dir = TempDir::new("gconfig-reset");
        let path = dir.join("config.yaml");
        set_value(&path, "k", "v", false).unwrap();
        assert!(path.exists());
        reset(&path).unwrap();
        assert!(!path.exists());
        // Probed: a second reset still succeeds.
        reset(&path).unwrap();
    }

    #[test]
    fn to_json_preserves_scalar_types_and_stringifies_odd_keys() {
        assert_eq!(to_json(&Value::Null), serde_json::Value::Null);
        assert_eq!(to_json(&Value::Bool(true)), serde_json::json!(true));
        assert_eq!(to_json(&parse_value("42", false)), serde_json::json!(42));
        assert_eq!(to_json(&parse_value("2.5", false)), serde_json::json!(2.5));
        assert_eq!(
            to_json(&parse_value("[1, 2, 3]", false)),
            serde_json::json!([1, 2, 3])
        );
        assert_eq!(
            to_json(&parse_value("{a: 1}", false)),
            serde_json::json!({"a": 1})
        );
        // A hand-edited file can have a non-string key; it is stringified.
        let odd: Value = serde_yaml::from_str("1: x\n").unwrap();
        assert_eq!(to_json(&odd), serde_json::json!({"1": "x"}));
    }
}
