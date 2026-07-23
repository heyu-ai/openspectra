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
//! * A missing, unparseable, non-mapping, or non-UTF-8 file reads as empty,
//!   and the next write overwrites it. An *unreadable* file is different: the
//!   oracle reports the I/O error and leaves the file alone (probed), so a
//!   genuine read fault must not be flattened into "empty".
//! * `unset` saves even when nothing was removed (creating a `{}` file).
//! * `reset` **truncates** the file to `{}` (creating it when absent) and, like
//!   `set`/`unset`, refuses when the existing file cannot be read; `reset
//!   --all` **deletes** it and never reads, so it succeeds on an unreadable
//!   config. Both are idempotent. (probed — `--all` is not an inert flag.)

use anyhow::{Context, Result};
use serde_yaml::{Mapping, Value};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Absolute path of the global config file, resolved from the environment.
///
/// Follows `$HOME` (probed: the oracle honors a `HOME` override on macOS and
/// ignores `XDG_CONFIG_HOME` there); the Linux XDG branch is the standard
/// config-dir convention and is **inferred**, not measured — the oracle is a
/// macOS-only binary and cannot exercise it.
///
/// Divergence (probed, deliberate): with `HOME` unset the oracle still
/// resolves the account's home via the OS password database, while this errors
/// instead. Matching would need a `getpwuid` dependency the workspace does not
/// carry; see `docs/reverse-engineering/config.md`.
pub fn config_path() -> Result<PathBuf> {
    resolve_config_path(
        cfg!(target_os = "macos"),
        std::env::var_os("HOME").as_deref(),
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
    )
    .ok_or_else(|| {
        anyhow::anyhow!(
            "could not determine the config directory (neither HOME nor XDG_CONFIG_HOME is usable)"
        )
    })
}

/// Pure resolver behind [`config_path`], split from the env reads so every
/// platform/env combination is unit-testable without mutating process env
/// vars (which would be racy under the parallel test harness).
///
/// A relative `XDG_CONFIG_HOME` is rejected rather than joined: the XDG base
/// directory spec requires an absolute path, and honoring a relative one would
/// make the *global* config location depend on the process's working
/// directory — the same `spectra config get` would read different files from
/// different cwds.
fn resolve_config_path(
    macos_layout: bool,
    home: Option<&OsStr>,
    xdg_config_home: Option<&OsStr>,
) -> Option<PathBuf> {
    let home = home.filter(|h| !h.is_empty());
    let config_dir = if macos_layout {
        Path::new(home?).join("Library/Application Support")
    } else {
        match xdg_config_home
            .filter(|x| !x.is_empty())
            .map(Path::new)
            .filter(|x| x.is_absolute())
        {
            Some(xdg) => xdg.to_path_buf(),
            None => Path::new(home?).join(".config"),
        }
    };
    Some(config_dir.join("openspec").join("config.yaml"))
}

/// Read the settings mapping for a *write* (load-modify-save).
///
/// Lenient only about **content**: a missing file and an unparseable or
/// non-mapping one all read as empty, matching the oracle, which reports
/// "No configuration set." for a corrupt file and lets the next `set`
/// overwrite it.
///
/// Non-UTF-8 bytes count as **content**, not I/O: probed, the oracle
/// overwrites such a file exactly like corrupt YAML.
///
/// It is deliberately **not** lenient about **I/O**. Flattening a
/// `PermissionDenied`/`IsADirectory`/`EIO` read into "empty" would make the
/// following `save` write a file containing only the new key — silently
/// destroying a config that was merely unreadable, because the temp+rename in
/// [`save`] needs the *directory* write bit rather than the file's. Probed:
/// the oracle reports `Permission denied (os error 13)` and leaves the file
/// alone. This mirrors [`crate::touched`], which splits the same cases.
pub fn load(path: &Path) -> Result<Mapping> {
    // Read bytes, not `read_to_string`: the latter reports non-UTF-8 content
    // as `InvalidData`, indistinguishable from a real I/O fault. Probed, the
    // oracle treats non-UTF-8 exactly like corrupt YAML -- `set` overwrites it
    // and exits 0 -- so a decode failure must stay on the lenient side of this
    // split. Only genuine I/O faults (PermissionDenied, IsADirectory, EIO, ...)
    // are propagated.
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Mapping::new()),
        Err(e) => return Err(e.into()),
    };
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok(Mapping::new());
    };
    Ok(match serde_yaml::from_str::<Value>(&text) {
        Ok(Value::Mapping(map)) => map,
        _ => Mapping::new(),
    })
}

/// Read the settings mapping for a *display* path (`list` / `get`), where an
/// unreadable file reads as empty just like a corrupt one — probed: the oracle
/// prints "No configuration set." and `Key '<k>' not found.` for a mode-000
/// file rather than surfacing the I/O error. Only the write paths refuse.
pub fn load_for_display(path: &Path) -> Mapping {
    load(path).unwrap_or_default()
}

/// Write the settings mapping, creating parent directories as needed.
pub fn save(path: &Path, settings: &Mapping) -> Result<()> {
    if let Some(parent) = path.parent() {
        // Bare, no path-prefixed context: probed with a mode-555 HOME, the
        // oracle emits `Error: Permission denied (os error 13)` here, so a
        // `creating <path>:` prefix would both diverge and leak the absolute
        // path. `write_atomically` still adds its own context naming the temp
        // file -- recorded as a Known divergence in
        // `docs/reverse-engineering/config.md`.
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_yaml::to_string(settings).context("serializing config")?;
    crate::init::write_atomically(path, &text)
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
///
/// The `expect` states the assumption rather than hiding it: serializing an
/// already-constructed `serde_yaml::Value` has no failure mode here, and
/// substituting an empty string on error would render an existing value as
/// nothing at exit 0 — the silent-failure shape this crate avoids elsewhere.
pub fn render_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => {
            serde_yaml::to_string(other).expect("serializing a serde_yaml::Value is infallible")
        }
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

/// `config set`: load-modify-save. Overwrites a corrupt file, refuses an
/// unreadable one (both probed — see [`load`]).
pub fn set_value(path: &Path, key: &str, raw: &str, force_string: bool) -> Result<()> {
    let mut settings = load(path)?;
    settings.insert(
        Value::String(key.to_string()),
        parse_value(raw, force_string),
    );
    save(path, &settings)
}

/// `config unset`: remove and save unconditionally — the oracle reports
/// success for a missing key and even creates a `{}` file when none existed
/// (probed), so this does not distinguish "removed" from "was absent". Like
/// `set`, it refuses when the existing file cannot be read.
pub fn unset_value(path: &Path, key: &str) -> Result<()> {
    let mut settings = load(path)?;
    settings.remove(Value::String(key.to_string()));
    save(path, &settings)
}

/// `config reset` (no `--all`): truncate the settings to an empty mapping,
/// writing `{}\n` — creating the file and its directory when absent (probed).
/// Like `set`/`unset` it reads first, so an unreadable file is refused rather
/// than replaced. Idempotent.
pub fn reset_to_empty(path: &Path) -> Result<()> {
    load(path)?;
    save(path, &Mapping::new())
}

/// `config reset --all`: delete the file outright. Unlike [`reset_to_empty`]
/// this does *not* read first, so it succeeds on an unreadable config
/// (probed). Idempotent — an already-absent file exits 0.
pub fn reset_delete(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        // "Nothing to delete" covers more than `NotFound`: when a component of
        // the path is a regular file the removal fails `ENOTDIR`, and the
        // oracle likewise treats the config as absent and exits 0 (probed).
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(())
        }
        // Bare, with no path-prefixed context: probed, the oracle emits the raw
        // OS error here (e.g. `Error: Operation not permitted (os error 1)`
        // when the config path is a directory). A prefix would both diverge and
        // leak the absolute path into a message the oracle keeps short.
        Err(e) => Err(e.into()),
    }
}

/// The exact bytes the oracle seeds a missing config with before opening it
/// in an editor (probed, xxd-confirmed).
pub const EDIT_SEED: &str = "# OpenSpec global config\n";

/// `config edit`'s pre-spawn side effect: create the config directory and, when
/// the file does not yet exist, seed it with [`EDIT_SEED`] (probed). An
/// existing file is left untouched. Without this the editor is pointed at a
/// path inside a directory that does not exist, so a real editor's save fails —
/// and since most editors still exit 0 after reporting that, the user's edits
/// vanish with no signal from the CLI.
pub fn ensure_editable(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        // Bare, for the same reason as `save` -- see the note there.
        std::fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        crate::init::write_atomically(path, EDIT_SEED)?;
    }
    Ok(())
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
        // An absolute XDG dir stands on its own -- HOME is not consulted.
        assert_eq!(
            resolve_config_path(false, None, Some(os("/xdg"))),
            Some(PathBuf::from("/xdg/openspec/config.yaml"))
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

    /// A relative `XDG_CONFIG_HOME` would make the *global* config location
    /// depend on the process CWD; the XDG spec requires an absolute path, so
    /// it falls back to `$HOME/.config` instead of being joined.
    #[test]
    fn resolve_config_path_ignores_a_relative_xdg_config_home() {
        assert_eq!(
            resolve_config_path(false, Some(os("/home/u")), Some(os("relative/dir"))),
            Some(PathBuf::from("/home/u/.config/openspec/config.yaml"))
        );
        // With no usable HOME either, a relative XDG cannot rescue it.
        assert_eq!(
            resolve_config_path(false, None, Some(os("relative/dir"))),
            None
        );
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
        assert!(load(&path).unwrap().is_empty(), "missing file");

        std::fs::write(&path, "not: [valid: yaml\n").unwrap();
        assert!(load(&path).unwrap().is_empty(), "corrupt file");

        std::fs::write(&path, "- 1\n- 2\n").unwrap();
        assert!(load(&path).unwrap().is_empty(), "non-mapping file");

        // Non-UTF-8 is a *content* problem, not an I/O fault: probed, the
        // oracle treats it exactly like corrupt YAML and lets `set` overwrite
        // it. Reading via `read_to_string` would misclassify this as
        // `InvalidData` and refuse the write.
        std::fs::write(&path, b"alpha: 1\nbeta: \"\xff\xfe\"\n").unwrap();
        assert!(load(&path).unwrap().is_empty(), "non-UTF-8 file");
        set_value(&path, "delta", "4", false).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "delta: 4\n");
    }

    /// The data-loss guard: an *I/O* read failure must not be flattened into
    /// "empty", or the save that follows writes a file holding only the new
    /// key. `write_atomically`'s temp+rename needs only the directory write
    /// bit, so it would happily replace a file it could not read.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_file_is_refused_by_the_write_paths_but_empty_for_display() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new("gconfig-unreadable");
        let path = dir.join("config.yaml");
        std::fs::write(&path, "alpha: 1\nbeta: 2\ngamma: 3\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        // Root ignores the permission bits, so the scenario cannot be built
        // there. Announce the skip and restore the mode, matching this crate's
        // sibling root-skips -- a silent early return would leave the only
        // coverage of the data-loss guard vacuous on a root CI runner with
        // nothing in the log to say so.
        if std::fs::read_to_string(&path).is_ok() {
            eprintln!(
                "skipping an_unreadable_file_is_refused_by_the_write_paths_but_empty_for_display: \
                 running as root (chmod 0o000 not enforced)"
            );
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }

        // Display paths stay lenient (probed: the oracle prints
        // "No configuration set." for a mode-000 file).
        assert!(load_for_display(&path).is_empty());

        // Write paths refuse, and -- the point of the test -- leave the bytes.
        assert!(load(&path).is_err(), "load must surface the I/O error");
        assert!(set_value(&path, "delta", "4", false).is_err());
        assert!(unset_value(&path, "alpha").is_err());
        assert!(reset_to_empty(&path).is_err());

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "alpha: 1\nbeta: 2\ngamma: 3\n",
            "the unreadable config must survive every refused write"
        );

        // `reset --all` is the documented exception: it never reads, so it
        // deletes an unreadable config rather than refusing (probed).
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        reset_delete(&path).unwrap();
        assert!(
            !path.exists(),
            "reset --all must delete even an unreadable config"
        );
    }

    #[test]
    fn set_get_round_trips_through_the_file() {
        let dir = TempDir::new("gconfig-roundtrip");
        let path = dir.join("nested").join("config.yaml");
        set_value(&path, "tdd", "true", false).unwrap();
        set_value(&path, "claude_effort.apply", "high", false).unwrap();

        let settings = load(&path).unwrap();
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

    /// Probed: plain `reset` truncates to `{}` and *creates* the file when it
    /// was absent -- it does not delete. Only `--all` deletes (next test).
    #[test]
    fn reset_to_empty_truncates_and_creates_rather_than_deleting() {
        let dir = TempDir::new("gconfig-reset-empty");
        let path = dir.join("nested").join("config.yaml");
        set_value(&path, "k", "v", false).unwrap();
        reset_to_empty(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}\n");

        // Idempotent, and it seeds a config that never existed.
        let virgin = dir.join("virgin").join("config.yaml");
        reset_to_empty(&virgin).unwrap();
        assert_eq!(std::fs::read_to_string(&virgin).unwrap(), "{}\n");
        reset_to_empty(&virgin).unwrap();
        assert_eq!(std::fs::read_to_string(&virgin).unwrap(), "{}\n");
    }

    #[test]
    fn reset_delete_removes_the_file_and_is_idempotent() {
        let dir = TempDir::new("gconfig-reset-all");
        let path = dir.join("config.yaml");
        set_value(&path, "k", "v", false).unwrap();
        assert!(path.exists());
        reset_delete(&path).unwrap();
        assert!(!path.exists());
        // Probed: a second `--all` reset still succeeds.
        reset_delete(&path).unwrap();
    }

    #[test]
    fn ensure_editable_seeds_a_missing_file_and_preserves_an_existing_one() {
        let dir = TempDir::new("gconfig-editable");
        let path = dir.join("nested").join("config.yaml");
        ensure_editable(&path).unwrap();
        // Probed oracle bytes.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EDIT_SEED);
        assert_eq!(EDIT_SEED, "# OpenSpec global config\n");

        std::fs::write(&path, "keep: me\n").unwrap();
        ensure_editable(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "keep: me\n",
            "an existing config must not be clobbered by edit"
        );
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

    /// Both branches are reachable from a hand-edited config, and neither was
    /// pinned before: a non-finite float has no JSON representation, and a
    /// tagged scalar must be unwrapped rather than dispatched on.
    #[test]
    fn to_json_handles_non_finite_floats_and_tagged_values() {
        let odd: Value = serde_yaml::from_str("x: .nan\ny: !!str 7\n").unwrap();
        assert_eq!(to_json(&odd), serde_json::json!({"x": null, "y": "7"}));
    }
}
