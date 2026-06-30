//! Change discovery and metadata.
//!
//! On disk a change is a directory `<spec_dir>/changes/<name>/` containing
//! `proposal.md`, `design.md`, `tasks.md`, `specs/<cap>/spec.md`, and a
//! `.openspec.yaml` metadata file. Spectra tracks per-change state under
//! `.spectra/`: `.spectra/changes/<name>.started` records the baseline git SHA
//! and `.spectra/changes/<name>.parked` marks a parked change.

use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use std::path::PathBuf;

use crate::config::Config;

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

/// Load a single change by name. Errors if the change directory is missing.
pub fn load(cfg: &Config, name: &str) -> Result<Change> {
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

/// List active (non-archived, non-parked) change names, sorted.
pub fn list_active(cfg: &Config) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(cfg.changes_dir()) else {
        return names;
    };
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "archive"
            || ARCHIVED_PREFIX_RE.is_match(&name)
            || !CHANGE_NAME_RE.is_match(&name)
            || is_parked(cfg, &name)
        {
            continue;
        }
        names.push(name);
    }
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
