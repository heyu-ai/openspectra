//! Change discovery and metadata.
//!
//! On disk a change is a directory `<spec_dir>/changes/<name>/` containing
//! `proposal.md`, `design.md`, `tasks.md`, `specs/<cap>/spec.md`, and a
//! `.openspec.yaml` metadata file. Spectra tracks per-change state under
//! `.spectra/`: `.spectra/changes/<name>.started` records the baseline git SHA
//! and `.spectra/changes/<name>.parked` marks a parked change.

use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use std::io::ErrorKind;
use std::path::PathBuf;

use crate::config::Config;
use crate::names::is_valid_name;

/// Active change names are kebab-case; the `YYYY-MM-DD-` prefix is reserved for
/// archived changes (recovered from the binary).
static CHANGE_NAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z0-9]+(-+[a-z0-9]+)*$").unwrap());
static ARCHIVED_PREFIX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}-").unwrap());

/// Parsed `<change>/.openspec.yaml`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ChangeMetadata {
    pub schema: Option<String>,
    /// `YYYY-MM-DD` creation date, or `None` when absent.
    pub created: Option<String>,
    pub created_by: Option<String>,
    pub created_with: Option<String>,
    pub archived_by: Option<String>,
    pub archived_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Change {
    pub name: String,
    pub dir: PathBuf,
    pub metadata: ChangeMetadata,
    /// Baseline git SHA from `.spectra/changes/<name>.started`, if present.
    pub started_sha: Option<String>,
    pub parked: bool,
}

impl Change {
    pub fn design_md(&self) -> PathBuf {
        self.dir.join("design.md")
    }
    pub fn tasks_md(&self) -> PathBuf {
        self.dir.join("tasks.md")
    }
    pub fn proposal_md(&self) -> PathBuf {
        self.dir.join("proposal.md")
    }
}

fn read_started_sha(cfg: &Config, name: &str) -> Option<String> {
    let p = cfg.root.join(".spectra").join("changes").join(format!("{name}.started"));
    std::fs::read_to_string(p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn is_parked(cfg: &Config, name: &str) -> bool {
    cfg.root
        .join(".spectra")
        .join("changes")
        .join(format!("{name}.parked"))
        .exists()
}

/// Load a change by name if it exists. Returns `Ok(None)` only when the
/// directory is genuinely absent (or `name` is invalid); any other I/O
/// failure checking for it propagates as `Err`, so callers juggling multiple
/// namespaces (e.g. `show`, which also tries `spec::try_load`) can't misread
/// a permission error as "not a change" the way a boolean `Path::is_dir()`
/// check would.
pub fn try_load(cfg: &Config, name: &str) -> Result<Option<Change>> {
    if !is_valid_name(name) {
        return Ok(None);
    }
    let dir = cfg.changes_dir().join(name);
    match std::fs::metadata(&dir) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => return Ok(None),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    }
    load(cfg, name).map(Some)
}

/// Load a single change by name. Errors if the change directory is missing
/// or `name` isn't a single path component.
pub fn load(cfg: &Config, name: &str) -> Result<Change> {
    if !is_valid_name(name) {
        return Err(anyhow!("invalid change name '{name}'"));
    }
    let dir = cfg.changes_dir().join(name);
    if !dir.is_dir() {
        return Err(anyhow!("change '{name}' not found in {}", cfg.changes_dir().display()));
    }
    let meta_path = dir.join(".openspec.yaml");
    let metadata: ChangeMetadata = if meta_path.exists() {
        let text = std::fs::read_to_string(&meta_path)?;
        // A malformed metadata file must not silently read as "no metadata"
        // (that would erase `created` and make a stale change look undated):
        // warn loudly, then fall back to defaults so drift still runs.
        serde_yaml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("warning: ignoring unparseable {} ({e})", meta_path.display());
            ChangeMetadata::default()
        })
    } else {
        ChangeMetadata::default()
    };
    Ok(Change {
        name: name.to_string(),
        dir,
        metadata,
        started_sha: read_started_sha(cfg, name),
        parked: is_parked(cfg, name),
    })
}

/// Change directory names under `changes_dir()` that pass the archive/name
/// filters (but aren't yet split by parked state), unsorted. Shared by
/// `list_active`/`list_parked` so the filter set can't drift between them.
fn walk_change_names(cfg: &Config) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(cfg.changes_dir()) else {
        return names;
    };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "archive" || ARCHIVED_PREFIX_RE.is_match(&name) || !CHANGE_NAME_RE.is_match(&name) {
            continue;
        }
        names.push(name);
    }
    names
}

/// List active (non-archived, non-parked) change names, sorted.
pub fn list_active(cfg: &Config) -> Vec<String> {
    let mut names: Vec<String> =
        walk_change_names(cfg).into_iter().filter(|name| !is_parked(cfg, name)).collect();
    names.sort();
    names
}

/// List parked change names (those with a `.spectra/changes/<name>.parked`
/// marker), sorted.
pub fn list_parked(cfg: &Config) -> Vec<String> {
    let mut names: Vec<String> =
        walk_change_names(cfg).into_iter().filter(|name| is_parked(cfg, name)).collect();
    names.sort();
    names
}

/// Resolve a change name: use `explicit` if given, else auto-select when
/// exactly one active change exists (mirrors `spectra drift`'s auto-detect).
pub fn resolve(cfg: &Config, explicit: Option<&str>) -> Result<String> {
    if let Some(name) = explicit {
        return Ok(name.to_string());
    }
    let active = list_active(cfg);
    match active.len() {
        1 => Ok(active.into_iter().next().unwrap()),
        0 => Err(anyhow!("No active changes. Create one with: spectra new change <name>")),
        _ => Err(anyhow!(
            "Multiple changes found. Use a change name to specify one: {}",
            active.join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &std::path::Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    #[test]
    fn list_parked_finds_only_marked_changes() {
        let tmp = TempDir::new();
        let cfg = Config { root: tmp.to_path_buf(), spec_dir: "openspec".to_string(), locale: None };
        write(&cfg.changes_dir().join("shipped").join("proposal.md"), "# Shipped\n");
        write(&cfg.changes_dir().join("on-hold").join("proposal.md"), "# On hold\n");
        write(&tmp.join(".spectra").join("changes").join("on-hold.parked"), "");

        assert_eq!(list_parked(&cfg), vec!["on-hold".to_string()]);
        assert_eq!(list_active(&cfg), vec!["shipped".to_string()]);
    }

    #[test]
    fn list_parked_is_empty_when_changes_dir_missing() {
        let tmp = TempDir::new();
        let cfg = Config { root: tmp.to_path_buf(), spec_dir: "openspec".to_string(), locale: None };

        assert_eq!(list_parked(&cfg), Vec::<String>::new());
    }

    #[test]
    fn list_parked_sorts_multiple_entries() {
        let tmp = TempDir::new();
        let cfg = Config { root: tmp.to_path_buf(), spec_dir: "openspec".to_string(), locale: None };
        write(&cfg.changes_dir().join("zeta").join("proposal.md"), "# Zeta\n");
        write(&cfg.changes_dir().join("alpha").join("proposal.md"), "# Alpha\n");
        write(&tmp.join(".spectra").join("changes").join("zeta.parked"), "");
        write(&tmp.join(".spectra").join("changes").join("alpha.parked"), "");

        assert_eq!(list_parked(&cfg), vec!["alpha".to_string(), "zeta".to_string()]);
    }

    #[test]
    fn list_parked_excludes_archived_prefixed_names_even_when_marked() {
        let tmp = TempDir::new();
        let cfg = Config { root: tmp.to_path_buf(), spec_dir: "openspec".to_string(), locale: None };
        write(
            &cfg.changes_dir().join("2026-01-01-old-change").join("proposal.md"),
            "# Old\n",
        );
        write(
            &tmp.join(".spectra").join("changes").join("2026-01-01-old-change.parked"),
            "",
        );

        assert_eq!(list_parked(&cfg), Vec::<String>::new());
    }

    #[test]
    fn try_load_rejects_path_traversal_names() {
        let tmp = TempDir::new();
        let cfg = Config { root: tmp.to_path_buf(), spec_dir: "openspec".to_string(), locale: None };
        // A change directory outside `changes_dir()` a traversal attempt could reach.
        write(&tmp.join("secret").join("proposal.md"), "outside\n");

        assert!(try_load(&cfg, "../secret").unwrap().is_none());
        assert!(try_load(&cfg, "..").unwrap().is_none());
        assert!(try_load(&cfg, "sub/dir").unwrap().is_none());
        assert!(try_load(&cfg, "").unwrap().is_none());
        assert!(load(&cfg, "../secret").is_err());
    }

    /// RAII guard for a per-test scratch directory: removes it on drop even
    /// when the test panics partway through (an assertion failure must not
    /// leak the directory).
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "spectra-change-test-{}-{}-{seq}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl std::ops::Deref for TempDir {
        type Target = std::path::Path;
        fn deref(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
