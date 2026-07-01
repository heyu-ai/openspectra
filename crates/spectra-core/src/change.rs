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

fn parked_marker_path(cfg: &Config, name: &str) -> PathBuf {
    cfg.root
        .join(".spectra")
        .join("changes")
        .join(format!("{name}.parked"))
}

fn is_parked(cfg: &Config, name: &str) -> bool {
    parked_marker_path(cfg, name).exists()
}

/// Remove any `.parked`/`.started` sidecar files left behind by a change
/// directory of the same `name` that was deleted by hand (rather than via
/// `spectra archive`), so a freshly `create`d change never silently inherits
/// stale parked/baseline state. A missing file is not an error.
fn clear_stale_sidecar_state(cfg: &Config, name: &str) -> Result<()> {
    for path in [parked_marker_path(cfg, name), started_sha_path(cfg, name)] {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("removing stale {}", path.display())),
        }
    }
    Ok(())
}

/// Errors unless `name` is both an existing change directory *and* passes
/// the same archive/name filters as `list_active`/`list_parked`
/// (`walk_change_names`) — otherwise parking a non-canonical or
/// archived-prefixed name would silently succeed with no visible effect in
/// any listing.
fn require_parkable(cfg: &Config, name: &str) -> Result<()> {
    if try_load(cfg, name)?.is_none() {
        return Err(anyhow!(
            "change '{name}' not found in {}",
            cfg.changes_dir().display()
        ));
    }
    if !walk_change_names(cfg).iter().any(|n| n == name) {
        return Err(anyhow!(
            "'{name}' is an archived or non-canonical change name and can't be parked"
        ));
    }
    Ok(())
}

/// Mark a change as parked (idempotent: parking an already-parked change is
/// not an error). Errors if `name` doesn't name an existing, active-eligible
/// change. Note: not safe against concurrent deletion of the change
/// directory between the existence check and the marker write.
pub fn park(cfg: &Config, name: &str) -> Result<()> {
    require_parkable(cfg, name)?;
    let marker = parked_marker_path(cfg, name);
    let parent = marker
        .parent()
        .expect("parked_marker_path always has a parent");
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    std::fs::write(&marker, "").with_context(|| format!("writing {}", marker.display()))?;
    Ok(())
}

/// Clear a change's parked marker (idempotent: unparking a change that isn't
/// parked is not an error). Errors if `name` doesn't name an existing,
/// active-eligible change.
pub fn unpark(cfg: &Config, name: &str) -> Result<()> {
    require_parkable(cfg, name)?;
    let marker = parked_marker_path(cfg, name);
    match std::fs::remove_file(&marker) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {}", marker.display())),
    }
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
        return Err(anyhow!(
            "change '{name}' not found in {}",
            cfg.changes_dir().display()
        ));
    }
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
        parked: is_parked(cfg, name),
    })
}

/// Scaffold a new change directory: `.openspec.yaml`, `proposal.md`,
/// `design.md`, `tasks.md`, and (best-effort) a `.spectra/changes/<name>.started`
/// baseline SHA. Errors if `name` isn't kebab-case, is archived-prefixed, is
/// the reserved `archive` name, or the change already exists.
///
/// Note: not safe against a concurrent `create` for the same name racing
/// between the existence check and the writes below (the same TOCTOU class
/// as `park`'s concurrent-deletion race, just triggered by concurrent
/// creation instead). On any failure inside `create_inner`, the partial
/// scaffold directory is removed so a retry doesn't get a misleading
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
    // A prior change with the same name may have been removed by hand,
    // leaving its `.parked`/`.started` sidecar files behind; clear them so
    // this fresh change doesn't silently inherit stale parked/baseline state.
    // Best-effort: a failure here is unrelated to whether the scaffold itself
    // can succeed, so it's logged rather than blocking `create`.
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
            // directory behind it; clear it along with the scaffold dir.
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
    let files: [(&str, String); 4] = [
        (
            ".openspec.yaml",
            format!("schema: spec-driven\ncreated: {today}\ncreated_with: spectra-cli\n"),
        ),
        (
            "proposal.md",
            format!("# {name}\n\ntodo: describe what this change does and why.\n"),
        ),
        // Lowercase-only: `drift`'s Symbol-anchor extraction (anchors.rs)
        // flags any capitalized word not found elsewhere in the repo as a
        // broken reference, so placeholder prose here must avoid it or a
        // freshly-created change scores as heavily drifted immediately.
        (
            "design.md",
            "# design\n\ntodo: describe the technical design here.\n".to_string(),
        ),
        ("tasks.md", "# tasks\n\n- [ ] todo\n".to_string()),
    ];
    for (filename, content) in files {
        let path = dir.join(filename);
        std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
    }

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

/// List active (non-archived, non-parked) change names, sorted.
pub fn list_active(cfg: &Config) -> Vec<String> {
    let mut names: Vec<String> = walk_change_names(cfg)
        .into_iter()
        .filter(|name| !is_parked(cfg, name))
        .collect();
    names.sort();
    names
}

/// List parked change names (those with a `.spectra/changes/<name>.parked`
/// marker), sorted.
pub fn list_parked(cfg: &Config) -> Vec<String> {
    let mut names: Vec<String> = walk_change_names(cfg)
        .into_iter()
        .filter(|name| is_parked(cfg, name))
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

    #[test]
    fn list_parked_finds_only_marked_changes() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        write(
            &cfg.changes_dir().join("shipped").join("proposal.md"),
            "# Shipped\n",
        );
        write(
            &cfg.changes_dir().join("on-hold").join("proposal.md"),
            "# On hold\n",
        );
        write(
            &tmp.join(".spectra").join("changes").join("on-hold.parked"),
            "",
        );

        assert_eq!(list_parked(&cfg), vec!["on-hold".to_string()]);
        assert_eq!(list_active(&cfg), vec!["shipped".to_string()]);
    }

    #[test]
    fn list_parked_is_empty_when_changes_dir_missing() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };

        assert_eq!(list_parked(&cfg), Vec::<String>::new());
    }

    #[test]
    fn list_parked_sorts_multiple_entries() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        write(
            &cfg.changes_dir().join("zeta").join("proposal.md"),
            "# Zeta\n",
        );
        write(
            &cfg.changes_dir().join("alpha").join("proposal.md"),
            "# Alpha\n",
        );
        write(
            &tmp.join(".spectra").join("changes").join("zeta.parked"),
            "",
        );
        write(
            &tmp.join(".spectra").join("changes").join("alpha.parked"),
            "",
        );

        assert_eq!(
            list_parked(&cfg),
            vec!["alpha".to_string(), "zeta".to_string()]
        );
    }

    #[test]
    fn list_parked_excludes_archived_prefixed_names_even_when_marked() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        write(
            &cfg.changes_dir()
                .join("2026-01-01-old-change")
                .join("proposal.md"),
            "# Old\n",
        );
        write(
            &tmp.join(".spectra")
                .join("changes")
                .join("2026-01-01-old-change.parked"),
            "",
        );

        assert_eq!(list_parked(&cfg), Vec::<String>::new());
    }

    #[test]
    fn try_load_rejects_path_traversal_names() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
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
    fn park_marks_an_existing_change_and_is_idempotent() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        write(
            &cfg.changes_dir().join("shipped").join("proposal.md"),
            "# Shipped\n",
        );

        park(&cfg, "shipped").unwrap();
        assert_eq!(list_parked(&cfg), vec!["shipped".to_string()]);

        // Parking an already-parked change is not an error.
        park(&cfg, "shipped").unwrap();
        assert_eq!(list_parked(&cfg), vec!["shipped".to_string()]);
    }

    #[test]
    fn park_errors_when_change_does_not_exist() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };

        assert!(park(&cfg, "ghost").is_err());
    }

    #[test]
    fn unpark_clears_the_marker_and_is_idempotent() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        write(
            &cfg.changes_dir().join("on-hold").join("proposal.md"),
            "# On hold\n",
        );
        write(
            &tmp.join(".spectra").join("changes").join("on-hold.parked"),
            "",
        );

        unpark(&cfg, "on-hold").unwrap();
        assert_eq!(list_active(&cfg), vec!["on-hold".to_string()]);

        // Unparking an already-active change is not an error.
        unpark(&cfg, "on-hold").unwrap();
        assert_eq!(list_active(&cfg), vec!["on-hold".to_string()]);
    }

    #[test]
    fn unpark_errors_when_change_does_not_exist() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };

        assert!(unpark(&cfg, "ghost").is_err());
    }

    #[test]
    fn park_and_unpark_reject_archived_prefixed_names() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };
        write(
            &cfg.changes_dir()
                .join("2026-01-01-old-change")
                .join("proposal.md"),
            "# Old\n",
        );

        assert!(park(&cfg, "2026-01-01-old-change").is_err());
        assert!(unpark(&cfg, "2026-01-01-old-change").is_err());
        assert_eq!(list_parked(&cfg), Vec::<String>::new());
    }

    #[test]
    fn create_scaffolds_all_expected_files() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };

        let ch = create(&cfg, "add-search-filter").unwrap();

        assert_eq!(ch.name, "add-search-filter");
        assert!(ch.dir.join(".openspec.yaml").is_file());
        assert!(ch.proposal_md().is_file());
        assert!(ch.design_md().is_file());
        assert!(ch.tasks_md().is_file());
        assert_eq!(ch.metadata.schema.as_deref(), Some("spec-driven"));
        assert!(ch.metadata.created.is_some());
        assert_eq!(list_active(&cfg), vec!["add-search-filter".to_string()]);
        assert_eq!(ch.started_sha, None);
    }

    #[test]
    fn create_does_not_inherit_stale_sidecar_state_from_a_deleted_change() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };

        create(&cfg, "reused-name").unwrap();
        park(&cfg, "reused-name").unwrap();
        // Simulate a user manually deleting the change dir (not via `archive`),
        // leaving the `.parked` marker behind on disk.
        std::fs::remove_dir_all(cfg.changes_dir().join("reused-name")).unwrap();
        assert!(parked_marker_path(&cfg, "reused-name").is_file());

        let ch = create(&cfg, "reused-name").unwrap();

        assert!(
            !ch.parked,
            "a freshly created change must not inherit a stale parked marker"
        );
        assert_eq!(list_active(&cfg), vec!["reused-name".to_string()]);
        assert_eq!(list_parked(&cfg), Vec::<String>::new());
    }

    #[test]
    fn create_rejects_reserved_archive_name() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
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
        // baseline write fail after the 4 scaffold files already succeeded,
        // forcing create() down the partial-failure cleanup path.
        write(&tmp.join(".spectra"), "");

        assert!(create(&cfg, "add-search-filter").is_err());
        assert!(
            !dir.exists(),
            "failed create() must not leave a partial change directory behind"
        );
    }

    #[test]
    fn create_scaffolds_design_md_with_no_broken_symbol_anchors() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        };

        let ch = create(&cfg, "add-search-filter").unwrap();

        let design = std::fs::read_to_string(ch.design_md()).unwrap();
        let symbol_count = crate::anchors::extract(&design)
            .iter()
            .filter(|a| a.kind == crate::anchors::AnchorKind::Symbol)
            .count();
        assert_eq!(
            symbol_count, 0,
            "scaffolded design.md must not contain capitalized Symbol anchors"
        );
    }

    #[test]
    fn create_writes_started_sha_when_root_is_a_git_repo() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
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
        std::fs::remove_file(cfg.changes_dir().join("add-search-filter").join("tasks.md")).unwrap();

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
