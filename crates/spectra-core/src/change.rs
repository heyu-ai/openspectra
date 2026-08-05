//! Change discovery and metadata.
//!
//! On disk a change is a directory `<spec_dir>/changes/<name>/` with an
//! `.openspec.yaml` metadata file and, as its workflow advances, artifact files
//! such as `proposal.md`, `design.md`, `tasks.md`, and `specs/<cap>/spec.md`.
//! Parking is not a flag: like the oracle, it *moves* the whole change
//! directory to `<git common dir>/spectra-app/changes/<name>/`, so a parked
//! change is absent from `<spec_dir>/changes/` while still resolving for
//! `status`/`show`/`drift`/`instructions`.
//!
//! Spectra tracks the rest of its per-change state under `.spectra/`:
//! `.spectra/changes/<name>.started` records the baseline git SHA drift needs
//! and is OpenSpectra-only (see `docs/reverse-engineering/artifact-workflow.md`).
//! `.spectra/changes/<name>.in-progress` is likewise OpenSpectra-only on disk
//! -- the oracle keeps that state in SQLite, not as a sidecar (see
//! `docs/reverse-engineering/in-progress.md`).

use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::names::is_valid_name;

/// Active change names are kebab-case; the `YYYY-MM-DD-` prefix is reserved for
/// archived changes (recovered from the binary).
static CHANGE_NAME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z0-9]+(-+[a-z0-9]+)*$").unwrap());
static ARCHIVED_PREFIX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}-").unwrap());
/// What the oracle accepts as a change id in `park`/`unpark` — looser than
/// [`CHANGE_NAME_RE`], and the source of its
/// "must contain only lowercase letters, digits, and hyphens" error.
static ORACLE_CHANGE_ID_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-z0-9-]+$").unwrap());

/// Parsed `<change>/.openspec.yaml`. `Serialize` skips `None` fields (rather
/// than emitting `key: null`) so a round-trip through `archive::
/// stamp_archived_metadata` reproduces the sparse-field shape the reference
/// CLI itself writes (e.g. a plain `new change` omits only
/// `created_with`/`archived_by`/`archived_at`).
///
/// `extra` catches any YAML key this struct doesn't otherwise model (a field
/// from a newer reference-CLI version, or one a human added by hand) via
/// `#[serde(flatten)]`, so `stamp_archived_metadata`'s deserialize-then-
/// reserialize round trip doesn't silently drop it -- only the 6 known
/// fields above are ever read or written by name.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ChangeMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// `YYYY-MM-DD` creation date, or `None` when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_with: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<String>,
    #[serde(flatten)]
    pub extra: serde_yaml::Mapping,
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

fn started_sha_path(cfg: &Config, name: &str) -> PathBuf {
    cfg.root
        .join(".spectra")
        .join("changes")
        .join(format!("{name}.started"))
}

fn read_started_sha(cfg: &Config, name: &str) -> Option<String> {
    std::fs::read_to_string(started_sha_path(cfg, name))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Where the oracle keeps parked changes: `<git common dir>/spectra-app/changes`.
/// Parking moves the whole change directory here, so a parked change is absent
/// from `<spec_dir>/changes/` entirely. `None` when `root` is not a git
/// repository or git is unavailable, in which case nothing can be parked.
pub(crate) fn parked_root(cfg: &Config) -> Option<PathBuf> {
    crate::git::common_dir(&cfg.root).map(|d| d.join("spectra-app").join("changes"))
}

fn parked_change_dir(cfg: &Config, name: &str) -> Option<PathBuf> {
    parked_root(cfg).map(|d| d.join(name))
}

fn is_parked(cfg: &Config, name: &str) -> bool {
    is_valid_name(name)
        && parked_change_dir(cfg, name)
            .map(|d| d.is_dir())
            .unwrap_or(false)
}

/// The directory holding `name`'s artifacts: the active change directory when
/// it exists, otherwise the parked one. `status`, `show`, `drift`, and
/// `instructions` all keep working on a parked change in the oracle, so
/// resolution has to see both locations; only the listings (`list`,
/// `validate`) are split by parked state.
fn resolve_change_dir(cfg: &Config, name: &str) -> Option<PathBuf> {
    let active = cfg.changes_dir().join(name);
    if active.is_dir() {
        return Some(active);
    }
    parked_change_dir(cfg, name).filter(|d| d.is_dir())
}

fn in_progress_marker_path(cfg: &Config, name: &str) -> PathBuf {
    cfg.root
        .join(".spectra")
        .join("changes")
        .join(format!("{name}.in-progress"))
}

/// Remove any `.in-progress`/`.started`/
/// `.spectra/touched/<name>.json` sidecar files for `name`. Used by `create`
/// (a change directory of the same name deleted by hand, rather than via
/// `spectra archive`, may have left these behind — clearing them stops a
/// freshly created change from silently inheriting stale state) and by
/// `archive::archive` itself (an archived change is no longer active, so its
/// sidecar state is cruft once the move succeeds).
///
/// The oracle does not clear its in-progress marker on archive. OpenSpectra
/// deliberately diverges here, consistently with its existing defensive
/// clearing of `.started`, so a recreated same-named change cannot
/// inherit a stale marker. A missing file is not an error.
/// Every sidecar is attempted even when an earlier one fails, and the errors
/// are reported together. Returning on the first failure would let one
/// unremovable sidecar hide the others: since callers treat this as
/// best-effort and only warn, a `.in-progress` marker that cannot be removed
/// (it has no removal command, no read path, and nothing validates it) would
/// silently leave `.started` and `touched.json` in place, and the recreated
/// change would inherit the stale baseline SHA this function exists to clear.
pub(crate) fn clear_stale_sidecar_state(cfg: &Config, name: &str) -> Result<()> {
    let sidecars = [
        in_progress_marker_path(cfg, name),
        started_sha_path(cfg, name),
        crate::touched::touched_path(cfg, name),
    ];
    let mut failures = Vec::new();
    for path in sidecars {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => failures.push(format!("removing stale {}: {e}", path.display())),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(failures.join("; ")))
    }
}

/// The oracle's change-id charset check, applied by `park`/`unpark` before
/// they look for the change. Looser than [`CHANGE_NAME_RE`]: the oracle parks
/// archived-prefixed names such as `2026-01-01-old` happily, and lists them.
///
/// The `archive` guard is OpenSpectra-only. The oracle accepts
/// `spectra park archive` and moves the *entire* `changes/archive/` tree into
/// the parked store, taking every archived change with it — a data-loss bug,
/// not a feature to reproduce.
fn require_parkable_name(name: &str, verb: &str) -> Result<()> {
    if !is_valid_name(name) || !ORACLE_CHANGE_ID_RE.is_match(name) {
        return Err(anyhow!(
            "Change ID '{name}' must contain only lowercase letters, digits, and hyphens"
        ));
    }
    if name == "archive" {
        return Err(anyhow!(
            "'archive' is the archived-changes directory, not a change; refusing to {verb} it"
        ));
    }
    Ok(())
}

/// Move a change out of `<spec_dir>/changes/` and into the parked store.
///
/// Errors if `name` is not an active change (matching the oracle's
/// `Change 'X' does not exist`, which is also what parking an already-parked
/// change reports, since it is no longer under `changes/`).
///
/// Not safe against concurrent deletion of the change directory between the
/// existence check and the rename.
pub fn park(cfg: &Config, name: &str) -> Result<()> {
    require_parkable_name(name, "park")?;
    if try_load_at(cfg, &cfg.changes_dir().join(name), name)?.is_none() {
        return Err(anyhow!("Change '{name}' does not exist"));
    }
    let target = parked_change_dir(cfg, name).ok_or_else(|| {
        anyhow!(
            "cannot park '{name}': {} is not a git repository",
            cfg.root.display()
        )
    })?;
    // The oracle silently overwrites an existing parked change of the same
    // name, destroying it. Refuse instead — same ruling as the hardened
    // atomic writes in `update`/`config`: a data-loss-only divergence.
    if target.exists() {
        return Err(anyhow!(
            "a parked change named '{name}' already exists at {}; unpark or remove it first",
            target.display()
        ));
    }
    let parent = target.parent().expect("parked dir always has a parent");
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    std::fs::rename(cfg.changes_dir().join(name), &target)
        .with_context(|| format!("moving change '{name}' to {}", target.display()))?;
    Ok(())
}

/// Mark a change as in progress without exposing that state through any read
/// path. The marker write is idempotent.
///
/// Unlike [`park`] and [`unpark`], this performs **no existence check**: a
/// name with no corresponding change is accepted and recorded, matching the
/// oracle's ghost-change behavior (see
/// `docs/reverse-engineering/in-progress.md`). Names that are not a single
/// path component are still rejected.
pub fn mark_in_progress(cfg: &Config, name: &str) -> Result<()> {
    // Defensive security boundary, not oracle-probed: reject traversal names
    // even though this makes OpenSpectra deliberately stricter for that input.
    if !is_valid_name(name) {
        return Err(anyhow!("invalid change name '{name}'"));
    }
    let marker = in_progress_marker_path(cfg, name);
    let parent = marker
        .parent()
        .expect("in_progress_marker_path always has a parent");
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    std::fs::write(&marker, "").with_context(|| format!("writing {}", marker.display()))?;
    Ok(())
}

/// Move a parked change back into `<spec_dir>/changes/`.
///
/// Errors with the oracle's wording: an active change is
/// `already active (not parked)`, and an unknown name `is not parked`.
pub fn unpark(cfg: &Config, name: &str) -> Result<()> {
    require_parkable_name(name, "unpark")?;
    let active = cfg.changes_dir().join(name);
    if active.is_dir() {
        return Err(anyhow!("Change '{name}' is already active (not parked)"));
    }
    let source = parked_change_dir(cfg, name).filter(|d| d.is_dir());
    let Some(source) = source else {
        return Err(anyhow!("Change '{name}' is not parked"));
    };
    let parent = active.parent().expect("changes dir always has a parent");
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    std::fs::rename(&source, &active)
        .with_context(|| format!("moving change '{name}' back to {}", active.display()))?;
    Ok(())
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
    match try_load_at(cfg, &cfg.changes_dir().join(name), name)? {
        Some(change) => Ok(Some(change)),
        None => match parked_change_dir(cfg, name) {
            Some(dir) => try_load_at(cfg, &dir, name),
            None => Ok(None),
        },
    }
}

/// `try_load` restricted to one candidate directory.
fn try_load_at(cfg: &Config, dir: &Path, name: &str) -> Result<Option<Change>> {
    if !is_valid_name(name) {
        return Ok(None);
    }
    match std::fs::metadata(dir) {
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
    let Some(dir) = resolve_change_dir(cfg, name) else {
        return Err(anyhow!(
            "change '{name}' not found in {}",
            cfg.changes_dir().display()
        ));
    };
    let parked = is_parked(cfg, name);
    let meta_path = dir.join(".openspec.yaml");
    let metadata: ChangeMetadata = if meta_path.exists() {
        let text = std::fs::read_to_string(&meta_path)?;
        // A malformed metadata file must not silently read as "no metadata"
        // (that would erase `created` and make a stale change look undated):
        // warn loudly, then fall back to defaults so drift still runs.
        serde_yaml::from_str(&text).unwrap_or_else(|e| {
            eprintln!(
                "warning: ignoring unparseable {} ({e})",
                meta_path.display()
            );
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
        parked,
    })
}

/// Create a new change directory with `.openspec.yaml` and, best-effort, a
/// `.spectra/changes/<name>.started` baseline SHA. Artifact files are created
/// later by the workflow. Errors if `name` isn't kebab-case, is
/// archived-prefixed, is the reserved `archive` name, or the change already
/// exists.
///
/// Note: not safe against a concurrent `create` for the same name racing
/// between the existence check and the writes below (the same TOCTOU class
/// as `park`'s concurrent-deletion race, just triggered by concurrent
/// creation instead). On any failure inside `create_inner`, the partial
/// change directory is removed so a retry doesn't get a misleading
/// "already exists" error; cleanup failure itself is logged, not silenced.
pub fn create(cfg: &Config, name: &str) -> Result<Change> {
    if !CHANGE_NAME_RE.is_match(name) || ARCHIVED_PREFIX_RE.is_match(name) || name == "archive" {
        return Err(anyhow!(
            "'{name}' is not a valid change name (expected kebab-case, e.g. 'add-search-filter')"
        ));
    }
    let dir = cfg.changes_dir().join(name);
    if dir.exists() {
        return Err(anyhow!(
            "change '{name}' already exists in {}",
            cfg.changes_dir().display()
        ));
    }
    // A prior change with the same name may have been removed by hand (or
    // archived), leaving its sidecar files behind; clear them so this fresh
    // change doesn't silently inherit stale state -- see
    // `clear_stale_sidecar_state`'s own doc comment for exactly what that
    // covers. Best-effort: a failure here is unrelated to whether the
    // change creation itself can succeed, so it's logged rather than blocking
    // `create`.
    if let Err(e) = clear_stale_sidecar_state(cfg, name) {
        eprintln!("warning: failed to clear stale sidecar state for '{name}': {e}");
    }
    match create_inner(cfg, name, &dir) {
        Ok(()) => load(cfg, name),
        Err(e) => {
            if let Err(cleanup_err) = std::fs::remove_dir_all(&dir) {
                if cleanup_err.kind() != std::io::ErrorKind::NotFound {
                    eprintln!(
                        "warning: failed to remove partial change directory {} after create error: {cleanup_err}",
                        dir.display()
                    );
                }
            }
            // create_inner writes `.started` last, so a failure there can
            // leave a (possibly partial) baseline file with no change
            // directory behind it; clear it along with the change dir.
            if let Err(cleanup_err) = clear_stale_sidecar_state(cfg, name) {
                eprintln!(
                    "warning: failed to remove partial sidecar state for '{name}' after create error: {cleanup_err}"
                );
            }
            Err(e)
        }
    }
}

fn create_inner(cfg: &Config, name: &str, dir: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;

    let today = chrono::Local::now().date_naive();
    let created_by = crate::git::change_creator_identity(&cfg.root);
    let metadata_path = dir.join(".openspec.yaml");
    // Serialize through serde_yaml (same as `stamp_archived_metadata`) rather
    // than raw string formatting: a git identity containing YAML-special
    // content (`:`, ` #`, leading indicator chars) must be quoted, or the
    // file we just wrote becomes unparseable and every later command silently
    // degrades the metadata to defaults. Plain values serialize byte-identical
    // to the oracle's output (pinned by `create_writes_only_oracle_metadata`).
    // Stamp the project's *configured* schema, not the built-in name. The
    // oracle does this (probed: with `config.yaml` naming `mycustom`,
    // `spectra new change c9` writes `schema: mycustom`), and it is what makes
    // #117's gate reachable: the change-level key outranks `config.yaml`, so
    // hardcoding `spec-driven` here silently re-opened the fallback the gate
    // exists to close — every change OpenSpectra created in a custom-schema
    // project recorded a schema it does not use, and `status` then passed.
    let metadata = ChangeMetadata {
        schema: Some(
            crate::schema::configured_schema_name(cfg)
                .unwrap_or_else(|| crate::schema::SCHEMA_NAME.to_string()),
        ),
        created: Some(today.to_string()),
        created_by: Some(created_by),
        ..Default::default()
    };
    let yaml = serde_yaml::to_string(&metadata)
        .with_context(|| format!("serializing metadata for {}", metadata_path.display()))?;
    std::fs::write(&metadata_path, yaml)
        .with_context(|| format!("writing {}", metadata_path.display()))?;

    // Best-effort: a non-git root (or a repo with no commits yet) just means
    // `drift`'s Tasks dimension has no baseline to diff blocked-task detection
    // against (see tasks.rs), not an error.
    if let Some(sha) = crate::git::head_sha(&cfg.root) {
        let started = started_sha_path(cfg, name);
        let parent = started.parent().expect("started path always has a parent");
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
        std::fs::write(&started, sha).with_context(|| format!("writing {}", started.display()))?;
    }

    Ok(())
}

/// Change directory names under `changes_dir()` that pass the archive/name
/// filters, unsorted.
fn walk_change_names(cfg: &Config) -> Vec<String> {
    walk_names_in(&cfg.changes_dir())
}

/// The shared directory-name filter behind `list_active` and `list_parked`,
/// applied to whichever store holds the change directories, so the two
/// listings can't drift apart on what counts as a change.
fn walk_names_in(dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
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
        {
            continue;
        }
        names.push(name);
    }
    names
}

/// List active (non-archived) change names, sorted. Parked changes are not
/// under `changes_dir()` at all, so no extra filtering is needed.
pub fn list_active(cfg: &Config) -> Vec<String> {
    let mut names = walk_change_names(cfg);
    names.sort();
    names
}

/// List parked change names — the directories the oracle moved into
/// `<git common dir>/spectra-app/changes/` — sorted.
///
/// Unlike [`list_active`] this applies no archived-prefix filter: the store
/// has no `archive/` subdirectory to disambiguate against, and the oracle
/// parks and lists `2026-01-01-old` like any other name (probed).
pub fn list_parked(cfg: &Config) -> Vec<String> {
    let Some(dir) = parked_root(cfg) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|name| is_valid_name(name) && ORACLE_CHANGE_ID_RE.is_match(name))
        .collect();
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
        0 => Err(anyhow!(
            "No active changes. Create one with: spectra new change <name>"
        )),
        _ => Err(anyhow!(
            "Multiple changes found. Use a change name to specify one: {}",
            active.join(", ")
        )),
    }
}

/// Outcome of [`mark_task_done`]: enough for the CLI to render both the
/// human ("Task {task_id} marked as done: {task_desc}") and `--json`
/// (`{"change","status","task_desc","task_id"}`) output shapes, matching
/// the reference CLI exactly.
#[derive(Debug)]
pub struct TaskDoneOutcome {
    pub change: String,
    pub task_id: usize,
    pub task_desc: String,
}

/// Mark the `task_id`-th checkbox (1-based, across all checkboxes in
/// `tasks.md`, file order) as done, and best-effort record any newly-dirty
/// files (via `git status --porcelain`, excluding the change's own artifact
/// directory, OpenSpectra's own `.spectra/` state directory, and files
/// already recorded for an earlier task) to `.spectra/touched/<name>.json`.
///
/// Errors when the change (or its `tasks.md`) doesn't exist use the same
/// message for both cases — "tasks.md not found for change '<name>'" —
/// matching the reference CLI, which doesn't distinguish them either. Any
/// other I/O error reading `tasks.md` (permission denied, etc.) propagates
/// with its real cause instead of being folded into that message.
pub fn mark_task_done(cfg: &Config, name: &str, task_id: usize) -> Result<TaskDoneOutcome> {
    let not_found = || anyhow!("tasks.md not found for change '{name}'");
    let ch = try_load(cfg, name)?.ok_or_else(not_found)?;
    let tasks_path = ch.tasks_md();
    let md = match std::fs::read_to_string(&tasks_path) {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(not_found()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", tasks_path.display())),
    };

    let (new_md, task_desc) = crate::tasks::mark_done(&md, task_id)?;
    std::fs::write(&tasks_path, &new_md)
        .with_context(|| format!("writing {}", tasks_path.display()))?;

    // Touched-file tracking is best-effort convenience data for AI-agent
    // commit tooling; a failure here must not undo the task-done marking
    // that already succeeded above.
    match crate::git::dirty_files(&cfg.root) {
        // Not a git repo at all is an expected, common case (this tool has
        // no `init` yet, so plenty of projects aren't git-tracked) -- only
        // warn when git itself failed on a project that IS a repo, so the
        // warning stays a meaningful signal instead of firing on every
        // `task done` call for a non-git project.
        None if !crate::git::is_repo(&cfg.root) => {}
        None => eprintln!(
            "warning: couldn't determine dirty files for '{name}'; this task's touched files were not recorded"
        ),
        Some(dirty) => {
            let change_rel_dir = ch.dir.strip_prefix(&cfg.root).ok();
            let candidate_files: Vec<String> = dirty
                .into_iter()
                .filter(|f| {
                    let path = std::path::Path::new(f);
                    let under_change_dir = change_rel_dir.is_some_and(|rel| path.starts_with(rel));
                    // `.spectra/` is this tool's own state directory (the very
                    // tracking file being written here included) -- never a
                    // "touched" implementation file, whether or not the project
                    // happens to gitignore it.
                    let under_spectra_state_dir = path.starts_with(".spectra");
                    !under_change_dir && !under_spectra_state_dir
                })
                .collect();
            if let Err(e) = crate::touched::record_new(cfg, name, task_id, &task_desc, candidate_files) {
                eprintln!("warning: failed to record touched files for '{name}': {e}");
            }
        }
    }

    Ok(TaskDoneOutcome {
        change: name.to_string(),
        task_id,
        task_desc,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::touched;
    use std::fs;

    fn write(path: &std::path::Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// Place a change directory directly in the parked store, the way the
    /// oracle's `park` leaves it, without going through OpenSpectra's `park`.
    fn seed_parked(cfg: &Config, name: &str, proposal: &str) {
        write(
            &parked_root(cfg).unwrap().join(name).join("proposal.md"),
            proposal,
        );
    }

    #[test]
    fn list_parked_reads_the_oracles_store_and_active_excludes_it() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        write(
            &cfg.changes_dir().join("shipped").join("proposal.md"),
            "# Shipped\n",
        );
        seed_parked(&cfg, "on-hold", "# On hold\n");

        assert_eq!(list_parked(&cfg), vec!["on-hold".to_string()]);
        assert_eq!(list_active(&cfg), vec!["shipped".to_string()]);
    }

    #[test]
    fn list_parked_is_empty_when_the_store_is_missing() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);

        assert_eq!(list_parked(&cfg), Vec::<String>::new());
    }

    #[test]
    fn list_parked_is_empty_outside_a_git_repo() {
        // Without a git dir there is nowhere for the oracle to have parked
        // anything, and `parked_root` is None.
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        assert_eq!(list_parked(&cfg), Vec::<String>::new());
    }

    #[test]
    fn list_parked_sorts_multiple_entries() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        seed_parked(&cfg, "zeta", "# Zeta\n");
        seed_parked(&cfg, "alpha", "# Alpha\n");

        assert_eq!(
            list_parked(&cfg),
            vec!["alpha".to_string(), "zeta".to_string()]
        );
    }

    #[test]
    fn list_parked_includes_archived_prefixed_names() {
        // The oracle parks and lists them; only `list_active` needs the
        // archived-prefix filter, because `changes/` also holds `archive/`.
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        seed_parked(&cfg, "2026-01-01-old-change", "# Old\n");

        assert_eq!(list_parked(&cfg), vec!["2026-01-01-old-change".to_string()]);
    }

    #[test]
    fn a_parked_change_still_resolves_for_status_and_drift() {
        // The oracle keeps `status`, `show`, `drift`, and `instructions`
        // working on a parked change; only the listings hide it.
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        seed_parked(&cfg, "on-hold", "# On hold\n");

        let ch = load(&cfg, "on-hold").unwrap();
        assert!(ch.parked);
        assert_eq!(
            ch.proposal_md(),
            parked_root(&cfg)
                .unwrap()
                .join("on-hold")
                .join("proposal.md")
        );
        assert!(try_load(&cfg, "on-hold").unwrap().is_some());
    }

    #[test]
    fn try_load_rejects_path_traversal_names() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        // A change directory outside `changes_dir()` a traversal attempt could reach.
        write(&tmp.join("secret").join("proposal.md"), "outside\n");

        assert!(try_load(&cfg, "../secret").unwrap().is_none());
        assert!(try_load(&cfg, "..").unwrap().is_none());
        assert!(try_load(&cfg, "sub/dir").unwrap().is_none());
        assert!(try_load(&cfg, "").unwrap().is_none());
        assert!(load(&cfg, "../secret").is_err());
    }

    #[test]
    fn park_moves_the_change_directory_into_the_parked_store() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        write(
            &cfg.changes_dir().join("shipped").join("proposal.md"),
            "# Shipped\n",
        );

        park(&cfg, "shipped").unwrap();

        assert_eq!(list_parked(&cfg), vec!["shipped".to_string()]);
        assert!(!cfg.changes_dir().join("shipped").exists());
        assert_eq!(
            std::fs::read_to_string(
                parked_root(&cfg)
                    .unwrap()
                    .join("shipped")
                    .join("proposal.md")
            )
            .unwrap(),
            "# Shipped\n"
        );
    }

    #[test]
    fn parking_an_already_parked_change_reports_it_as_nonexistent() {
        // Matching the oracle: once parked the change is gone from
        // `changes/`, so a second park is "does not exist", not a no-op.
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        write(
            &cfg.changes_dir().join("shipped").join("proposal.md"),
            "# Shipped\n",
        );

        park(&cfg, "shipped").unwrap();
        let err = park(&cfg, "shipped").unwrap_err().to_string();

        assert_eq!(err, "Change 'shipped' does not exist");
        assert_eq!(list_parked(&cfg), vec!["shipped".to_string()]);
    }

    #[test]
    fn park_refuses_to_overwrite_an_existing_parked_change() {
        // Deliberate divergence: the oracle silently clobbers the parked copy.
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        seed_parked(&cfg, "clash", "# Parked original\n");
        write(
            &cfg.changes_dir().join("clash").join("proposal.md"),
            "# Active namesake\n",
        );

        assert!(park(&cfg, "clash").is_err());
        assert_eq!(
            std::fs::read_to_string(parked_root(&cfg).unwrap().join("clash").join("proposal.md"))
                .unwrap(),
            "# Parked original\n",
            "the parked copy must survive"
        );
        assert!(cfg.changes_dir().join("clash").is_dir());
    }

    #[test]
    fn mark_in_progress_marks_an_existing_change_and_is_idempotent() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        write(
            &cfg.changes_dir().join("shipping").join("proposal.md"),
            "# Shipping\n",
        );

        mark_in_progress(&cfg, "shipping").unwrap();
        assert!(in_progress_marker_path(&cfg, "shipping").is_file());

        mark_in_progress(&cfg, "shipping").unwrap();
        assert!(in_progress_marker_path(&cfg, "shipping").is_file());
    }

    #[test]
    fn mark_in_progress_marks_a_nonexistent_change() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        // Deliberately unlike `park`: the oracle's ghost-change probe accepts
        // and records a marker for a change that does not exist.
        mark_in_progress(&cfg, "ghost").unwrap();

        assert!(in_progress_marker_path(&cfg, "ghost").is_file());
        assert!(!cfg.changes_dir().join("ghost").exists());
    }

    #[test]
    fn mark_in_progress_rejects_path_traversal_names() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        assert!(mark_in_progress(&cfg, "../evil").is_err());
    }

    #[test]
    fn park_errors_when_change_does_not_exist() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);

        assert_eq!(
            park(&cfg, "ghost").unwrap_err().to_string(),
            "Change 'ghost' does not exist"
        );
    }

    #[test]
    fn unpark_moves_the_change_back_into_changes_dir() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        seed_parked(&cfg, "on-hold", "# On hold\n");

        unpark(&cfg, "on-hold").unwrap();

        assert_eq!(list_active(&cfg), vec!["on-hold".to_string()]);
        assert_eq!(list_parked(&cfg), Vec::<String>::new());
        assert_eq!(
            std::fs::read_to_string(cfg.changes_dir().join("on-hold").join("proposal.md")).unwrap(),
            "# On hold\n"
        );
    }

    #[test]
    fn unpark_errors_on_an_active_change_and_on_an_unknown_name() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        write(
            &cfg.changes_dir().join("running").join("proposal.md"),
            "# Running\n",
        );

        assert_eq!(
            unpark(&cfg, "running").unwrap_err().to_string(),
            "Change 'running' is already active (not parked)"
        );
        assert_eq!(
            unpark(&cfg, "ghost").unwrap_err().to_string(),
            "Change 'ghost' is not parked"
        );
    }

    #[test]
    fn park_and_unpark_accept_archived_prefixed_names() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        write(
            &cfg.changes_dir()
                .join("2026-01-01-old-change")
                .join("proposal.md"),
            "# Old\n",
        );

        park(&cfg, "2026-01-01-old-change").unwrap();
        assert_eq!(list_parked(&cfg), vec!["2026-01-01-old-change".to_string()]);

        unpark(&cfg, "2026-01-01-old-change").unwrap();
        assert!(cfg.changes_dir().join("2026-01-01-old-change").is_dir());
    }

    #[test]
    fn park_rejects_ids_outside_the_oracles_charset() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        write(
            &cfg.changes_dir().join("BadName").join("proposal.md"),
            "# Bad\n",
        );

        assert_eq!(
            park(&cfg, "BadName").unwrap_err().to_string(),
            "Change ID 'BadName' must contain only lowercase letters, digits, and hyphens"
        );
        assert!(park(&cfg, "../escape").is_err());
    }

    #[test]
    fn park_refuses_to_swallow_the_archive_directory() {
        // Deliberate divergence: `spectra park archive` moves the whole
        // `changes/archive/` tree into the parked store in the oracle.
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        write(
            &cfg.changes_dir()
                .join("archive")
                .join("2025-01-01-done")
                .join("proposal.md"),
            "# Done\n",
        );

        assert!(park(&cfg, "archive").is_err());
        assert!(unpark(&cfg, "archive").is_err());
        assert!(cfg
            .changes_dir()
            .join("archive")
            .join("2025-01-01-done")
            .is_dir());
    }

    #[test]
    fn create_writes_only_oracle_metadata() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        let before_create = chrono::Local::now().date_naive().to_string();
        let ch = create(&cfg, "add-search-filter").unwrap();
        let after_create = chrono::Local::now().date_naive().to_string();
        let created = ch.metadata.created.as_deref().unwrap();
        let created_by = ch.metadata.created_by.as_deref().unwrap();

        assert_eq!(ch.name, "add-search-filter");
        assert!(ch.dir.join(".openspec.yaml").is_file());
        assert!(!ch.proposal_md().exists());
        assert!(!ch.design_md().exists());
        assert!(!ch.tasks_md().exists());
        assert_eq!(
            std::fs::read_to_string(ch.dir.join(".openspec.yaml")).unwrap(),
            format!("schema: spec-driven\ncreated: {created}\ncreated_by: {created_by}\n")
        );
        assert_eq!(ch.metadata.schema.as_deref(), Some("spec-driven"));
        assert!(created == before_create || created == after_create);
        assert!(!created_by.is_empty());
        assert_eq!(ch.metadata.created_with, None);
        assert_eq!(list_active(&cfg), vec!["add-search-filter".to_string()]);
        assert_eq!(ch.started_sha, None);
    }

    #[test]
    fn create_round_trips_yaml_special_git_identity() {
        // Regression: `created_by` used to be spliced into the YAML with
        // `format!`, so an identity containing YAML-special content (": ",
        // " #") produced a .openspec.yaml that `load` could not parse back --
        // every later command warned and silently degraded the metadata to
        // defaults. serde_yaml serialization must quote it instead.
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        let run = |args: &[&str]| {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&*tmp)
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        };
        run(&["init", "-q"]);
        run(&["config", "user.name", "Weird: Name #1"]);
        run(&["config", "user.email", "weird@example.com"]);

        let ch = create(&cfg, "weird-identity").unwrap();
        assert_eq!(
            ch.metadata.created_by.as_deref(),
            Some("Weird: Name #1 <weird@example.com>")
        );

        // The file we just wrote must parse back to the same metadata (no
        // unparseable-yaml fallback-to-defaults path).
        let reloaded = load(&cfg, "weird-identity").unwrap();
        assert_eq!(reloaded.metadata.schema.as_deref(), Some("spec-driven"));
        assert_eq!(
            reloaded.metadata.created_by.as_deref(),
            Some("Weird: Name #1 <weird@example.com>")
        );
    }

    #[test]
    fn create_does_not_inherit_stale_sidecar_state_from_a_deleted_change() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        create(&cfg, "reused-name").unwrap();
        mark_in_progress(&cfg, "reused-name").unwrap();
        // Simulate a user manually deleting the change dir (not via `archive`),
        // leaving the sidecar markers behind on disk.
        std::fs::remove_dir_all(cfg.changes_dir().join("reused-name")).unwrap();
        assert!(in_progress_marker_path(&cfg, "reused-name").is_file());

        let ch = create(&cfg, "reused-name").unwrap();

        assert!(
            !ch.parked,
            "a freshly created change must not read as parked"
        );
        assert!(
            !in_progress_marker_path(&cfg, "reused-name").exists(),
            "a freshly created change must not inherit a stale in-progress marker"
        );
        assert_eq!(list_active(&cfg), vec!["reused-name".to_string()]);
        assert_eq!(list_parked(&cfg), Vec::<String>::new());
    }

    /// A sidecar that cannot be removed must not shield the ones after it.
    /// The loop used to `return` on the first failure, and the in-progress
    /// marker sits ahead of `.started` and `touched.json` -- so an
    /// unremovable marker (it has no removal command, no read path, and
    /// nothing validates it) silently left the stale baseline SHA in place,
    /// and the recreated change scored drift against the previous change's
    /// baseline. Both callers only warn, so nothing surfaced.
    #[cfg(unix)]
    #[test]
    fn clear_stale_sidecar_state_clears_later_sidecars_when_an_earlier_one_fails() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        create(&cfg, "blocked").unwrap();
        mark_in_progress(&cfg, "blocked").unwrap();
        // `create` only writes `.started` inside a git repo; this scratch dir
        // is not one, so seed it explicitly — two sidecars must share the
        // locked directory for a first-error return to be observable.
        write(&started_sha_path(&cfg, "blocked"), "deadbeef\n");
        crate::touched::record(&cfg, "blocked", 1, "task", vec!["src/lib.rs".to_string()]).unwrap();
        let touched = crate::touched::touched_path(&cfg, "blocked");
        assert!(touched.is_file(), "fixture: touched.json must exist");

        // Make removals inside .spectra/changes/ fail while .spectra/touched/
        // stays writable, so a first-error return would be observable.
        let changes_state_dir = in_progress_marker_path(&cfg, "blocked")
            .parent()
            .unwrap()
            .to_path_buf();
        let original = std::fs::metadata(&changes_state_dir).unwrap().permissions();
        let mut locked = original.clone();
        locked.set_mode(0o555);
        std::fs::set_permissions(&changes_state_dir, locked).unwrap();

        let result = clear_stale_sidecar_state(&cfg, "blocked");

        std::fs::set_permissions(&changes_state_dir, original).unwrap();

        let err = result.expect_err("removal failures must be reported, not swallowed");
        let msg = err.to_string();
        assert!(
            msg.contains("blocked.in-progress") && msg.contains("blocked.started"),
            "every failing sidecar must be named, not just the first: {msg}"
        );
        assert!(
            !touched.exists(),
            "touched.json must still be cleared even though earlier sidecars failed"
        );
    }

    #[test]
    fn create_rejects_reserved_archive_name() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        assert!(create(&cfg, "archive").is_err());
    }

    #[test]
    fn create_rejects_non_kebab_case_names() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        assert!(create(&cfg, "Not_Kebab_Case").is_err());
        assert!(create(&cfg, "").is_err());
    }

    #[test]
    fn create_rejects_archived_prefixed_names() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        assert!(create(&cfg, "2026-01-01-old-change").is_err());
    }

    #[test]
    fn create_errors_when_change_already_exists() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        create(&cfg, "add-search-filter").unwrap();
        assert!(create(&cfg, "add-search-filter").is_err());
    }

    #[test]
    fn create_cleans_up_partial_directory_on_write_failure() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        let dir = cfg.changes_dir().join("add-search-filter");
        let run = |args: &[&str]| {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&*tmp)
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@t.co"]);
        run(&["config", "user.name", "t"]);
        write(&tmp.join("README.md"), "hi\n");
        run(&["add", "README.md"]);
        run(&["commit", "-q", "-m", "init"]);
        // `.spectra` as a plain file (not a directory) makes the `.started`
        // baseline write fail after the metadata write already succeeded,
        // forcing create() down the partial-failure cleanup path.
        write(&tmp.join(".spectra"), "");

        assert!(create(&cfg, "add-search-filter").is_err());
        assert!(
            !dir.exists(),
            "failed create() must not leave a partial change directory behind"
        );
    }

    #[test]
    fn create_writes_started_sha_when_root_is_a_git_repo() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        let run = |args: &[&str]| {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&*tmp)
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@t.co"]);
        run(&["config", "user.name", "t"]);
        write(&tmp.join("README.md"), "hi\n");
        run(&["add", "README.md"]);
        run(&["commit", "-q", "-m", "init"]);
        let expected_sha = String::from_utf8(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&*tmp)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_string();

        let ch = create(&cfg, "add-search-filter").unwrap();
        assert_eq!(ch.started_sha.as_deref(), Some(expected_sha.as_str()));
    }

    fn git_repo_cfg(tmp: &TempDir) -> Config {
        let run = |args: &[&str]| {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&**tmp)
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@t.co"]);
        run(&["config", "user.name", "t"]);
        write(&tmp.join("README.md"), "hi\n");
        run(&["add", "README.md"]);
        run(&["commit", "-q", "-m", "init"]);
        Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        }
    }

    #[test]
    fn mark_task_done_toggles_the_checkbox_and_returns_the_description() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        create(&cfg, "add-search-filter").unwrap();
        write(
            &cfg.changes_dir().join("add-search-filter").join("tasks.md"),
            "- [ ] first\n- [ ] second\n",
        );

        let outcome = mark_task_done(&cfg, "add-search-filter", 2).unwrap();

        assert_eq!(outcome.change, "add-search-filter");
        assert_eq!(outcome.task_id, 2);
        assert_eq!(outcome.task_desc, "second");
        let tasks_md =
            std::fs::read_to_string(cfg.changes_dir().join("add-search-filter").join("tasks.md"))
                .unwrap();
        assert_eq!(tasks_md, "- [ ] first\n- [x] second\n");
    }

    #[test]
    fn mark_task_done_errors_when_change_does_not_exist() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);

        let err = mark_task_done(&cfg, "does-not-exist", 1).unwrap_err();
        assert_eq!(
            err.to_string(),
            "tasks.md not found for change 'does-not-exist'"
        );
    }

    #[test]
    fn mark_task_done_errors_when_tasks_md_is_missing() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        create(&cfg, "add-search-filter").unwrap();

        let err = mark_task_done(&cfg, "add-search-filter", 1).unwrap_err();
        assert_eq!(
            err.to_string(),
            "tasks.md not found for change 'add-search-filter'"
        );
    }

    #[test]
    fn mark_task_done_records_dirty_files_outside_the_change_dir() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        create(&cfg, "add-search-filter").unwrap();
        write(
            &cfg.changes_dir().join("add-search-filter").join("tasks.md"),
            "- [ ] first\n",
        );
        write(&tmp.join("src.rs"), "fn main() {}\n");

        mark_task_done(&cfg, "add-search-filter", 1).unwrap();

        let recorded = touched::already_recorded(&cfg, "add-search-filter");
        assert!(recorded.contains("src.rs"));
        // The change's own artifact dir (tasks.md itself just got rewritten,
        // and is git-dirty) must never show up as a "touched" file.
        assert!(!recorded.iter().any(|f| f.contains("add-search-filter")));
    }

    #[test]
    fn mark_task_done_never_records_its_own_state_directory() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        create(&cfg, "add-search-filter").unwrap();
        write(
            &cfg.changes_dir().join("add-search-filter").join("tasks.md"),
            "- [ ] first\n- [ ] second\n",
        );
        write(&tmp.join("src.rs"), "fn main() {}\n");

        // task 1 creates .spectra/touched/add-search-filter.json, which is
        // untracked (and un-gitignored in this test fixture) at the moment
        // task 2 runs -- it must never be attributed to task 2 as a "touched" file.
        mark_task_done(&cfg, "add-search-filter", 1).unwrap();
        mark_task_done(&cfg, "add-search-filter", 2).unwrap();

        let recorded = touched::already_recorded(&cfg, "add-search-filter");
        assert!(!recorded.iter().any(|f| f.contains(".spectra")));
    }

    #[test]
    fn mark_task_done_does_not_reattribute_a_file_already_recorded() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        create(&cfg, "add-search-filter").unwrap();
        write(
            &cfg.changes_dir().join("add-search-filter").join("tasks.md"),
            "- [ ] first\n- [ ] second\n",
        );
        write(&tmp.join("src.rs"), "fn main() {}\n");

        mark_task_done(&cfg, "add-search-filter", 1).unwrap();
        // src.rs is still dirty; a second task-done call must not attribute
        // it again to task 2 since it's already recorded under task 1.
        mark_task_done(&cfg, "add-search-filter", 2).unwrap();

        let tracking_json = std::fs::read_to_string(
            tmp.join(".spectra")
                .join("touched")
                .join("add-search-filter.json"),
        )
        .unwrap();
        let tracking: touched::TouchedTracking = serde_json::from_str(&tracking_json).unwrap();
        assert_eq!(tracking.touched.len(), 1);
        assert_eq!(tracking.touched[0].task_id, "1");
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
