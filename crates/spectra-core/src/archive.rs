//! `spectra archive` — move a completed change to
//! `<spec_dir>/changes/archive/YYYY-MM-DD-<name>/`, stamp its
//! `.openspec.yaml` with `archived_by`/`archived_at`, and (unless
//! `--skip-specs`) merge each capability's proposed `## ADDED Requirements`
//! delta into the canonical `<spec_dir>/specs/<capability>/spec.md`.
//!
//! Reverse-engineered against `/Applications/Spectra.app` v2.3.1 — see
//! `docs/reverse-engineering/archive.md` for the full write-up, including
//! the deliberately narrowed spec-merge scope (only `## ADDED Requirements`
//! is applied; a delta containing `MODIFIED`/`REMOVED`/`RENAMED` sections
//! errors, asking for `--skip-specs`) and other documented gaps (no
//! snapshot/unarchive support).

use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use std::io::ErrorKind;
use std::path::Path;

use crate::config::Config;
use crate::{change, touched};

static ADDED_HEADER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^## ADDED Requirements\s*$").unwrap());
static UNSUPPORTED_HEADER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^## (MODIFIED|REMOVED|RENAMED) Requirements\s*$").unwrap());
static REQUIREMENT_HEADER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^### Requirement:").unwrap());
static SECTION_HEADER_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^## \S").unwrap());

/// How many requirements were appended to one capability's canonical spec.
#[derive(Debug, Clone)]
pub struct SpecApplyResult {
    pub capability: String,
    pub added: usize,
}

/// Outcome of [`archive`].
#[derive(Debug)]
pub struct ArchiveOutcome {
    pub name: String,
    pub archived_name: String,
    pub specs_applied: Vec<SpecApplyResult>,
}

/// Read `path` as UTF-8 text: `Ok(None)` when it's genuinely absent, `Err`
/// for any other I/O failure (permission denied, invalid UTF-8, etc.) —
/// callers must not fold a real read failure into "doesn't exist yet",
/// since doing so before a subsequent write (as this module's several
/// create-if-absent spots do) would silently clobber unreadable content.
fn read_optional(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// `Ok(None)` when `path` is genuinely absent; `Err` for any other failure
/// (mirrors [`read_optional`]'s NotFound-only collapse).
fn read_dir_optional(path: &Path) -> Result<Option<std::fs::ReadDir>> {
    match std::fs::read_dir(path) {
        Ok(entries) => Ok(Some(entries)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Whether `path` is a directory, via `fs::metadata` (not the boolean
/// `Path::is_dir()`, which returns `false` on *any* stat error including
/// permission-denied — indistinguishable from "genuinely absent").
fn is_dir(path: &Path) -> Result<bool> {
    match std::fs::metadata(path) {
        Ok(m) => Ok(m.is_dir()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Archive `name`: move its directory to
/// `<spec_dir>/changes/archive/<today>-<name>/`, then (in order) optionally
/// mark all pending tasks done, stamp `.openspec.yaml` with
/// `archived_by`/`archived_at`, and (unless `skip_specs`) apply each
/// capability's spec delta. Clears the change's `.spectra/` sidecar
/// (parked/baseline) markers on success.
///
/// Errors with "Change '<name>' not found." when `name` doesn't name an
/// existing active change — matching the reference CLI, which reports the
/// same message whether the name never existed or was already archived
/// (archived changes live under `changes/archive/`, outside the active
/// namespace `try_load` searches).
///
/// Unless `skip_specs`, every capability's spec delta is validated (checked
/// for an unsupported `MODIFIED`/`REMOVED`/`RENAMED Requirements` section)
/// *before* the change directory is moved or anything else is written, so a
/// validation failure leaves the change exactly as it was — still active,
/// not stuck half-archived with the directory already moved but its specs
/// never applied. `--mark-tasks-complete` similarly runs *after* the
/// directory move (operating on the now-archived `tasks.md`), not before —
/// if the move itself fails (e.g. a same-day archive-name collision), the
/// still-active change is left completely untouched rather than carrying
/// prematurely-flipped checkboxes. Any failure *after* the move still
/// leaves the change moved but incomplete; the resulting error names the
/// new location so it isn't a mystery where the change went.
pub fn archive(
    cfg: &Config,
    name: &str,
    skip_specs: bool,
    mark_tasks_complete: bool,
) -> Result<ArchiveOutcome> {
    let ch = change::try_load(cfg, name)?.ok_or_else(|| anyhow!("Change '{name}' not found."))?;

    if !skip_specs {
        validate_spec_deltas(&ch.dir)?;
    }

    let today = chrono::Local::now().date_naive();
    let archived_name = format!("{today}-{name}");
    let archive_dir = cfg.changes_dir().join("archive");
    std::fs::create_dir_all(&archive_dir)
        .with_context(|| format!("creating {}", archive_dir.display()))?;
    let dest = archive_dir.join(&archived_name);
    std::fs::rename(&ch.dir, &dest)
        .with_context(|| format!("moving {} to {}", ch.dir.display(), dest.display()))?;

    let specs_applied = (|| -> Result<Vec<SpecApplyResult>> {
        if mark_tasks_complete {
            mark_tasks_complete_at(&dest, name)?;
        }
        stamp_archived_metadata(cfg, &dest, today)?;
        if skip_specs {
            Ok(Vec::new())
        } else {
            apply_spec_deltas(cfg, &dest, name, today)
        }
    })()
    .map_err(|e| {
        anyhow!(
            "change '{name}' was moved to {} but archiving could not be completed: {e}. \
             The change is at that location -- fix the issue, then either finish archiving by \
             hand or move it back to keep working on it.",
            dest.display()
        )
    })?;

    if let Err(e) = change::clear_stale_sidecar_state(cfg, name) {
        eprintln!("warning: failed to clear sidecar state for '{name}': {e}");
    }

    Ok(ArchiveOutcome {
        name: name.to_string(),
        archived_name,
        specs_applied,
    })
}

/// Errors if any capability's `specs/<cap>/spec.md` under `change_dir`
/// contains a `MODIFIED`/`REMOVED`/`RENAMED Requirements` section, or has a
/// non-UTF-8 capability directory name, before anything about the change
/// has been touched.
fn validate_spec_deltas(change_dir: &Path) -> Result<()> {
    let specs_dir = change_dir.join("specs");
    let Some(entries) = read_dir_optional(&specs_dir)? else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", specs_dir.display()))?;
        let path = entry.path();
        if !is_dir(&path)? {
            continue;
        }
        let Some(delta) = read_optional(&path.join("spec.md"))? else {
            continue;
        };
        let capability = entry
            .file_name()
            .into_string()
            .map_err(|raw| anyhow!("capability directory name {raw:?} is not valid UTF-8"))?;
        if let Some(caps) = UNSUPPORTED_HEADER_RE.captures(&delta) {
            let kind = &caps[1];
            return Err(anyhow!(
                "capability '{capability}': {kind} requirement deltas aren't supported yet -- \
                 re-run with --skip-specs and apply this spec change by hand"
            ));
        }
    }
    Ok(())
}

/// Flip every pending checkbox in the archived change's `tasks.md`. A
/// missing `tasks.md` is not an error (nothing to mark), but is worth a
/// warning since `--mark-tasks-complete` was explicitly requested and would
/// otherwise silently have no effect.
fn mark_tasks_complete_at(dest: &Path, name: &str) -> Result<()> {
    let tasks_path = dest.join("tasks.md");
    let Some(md) = read_optional(&tasks_path)? else {
        eprintln!("warning: --mark-tasks-complete requested but no tasks.md found for '{name}'");
        return Ok(());
    };
    std::fs::write(&tasks_path, crate::tasks::mark_all_done(&md))
        .with_context(|| format!("writing {}", tasks_path.display()))
}

fn stamp_archived_metadata(cfg: &Config, dest: &Path, today: chrono::NaiveDate) -> Result<()> {
    let meta_path = dest.join(".openspec.yaml");
    let mut content = read_optional(&meta_path)?.unwrap_or_default();
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    match crate::git::user_identity(&cfg.root) {
        Some(identity) => content.push_str(&format!("archived_by: {identity}\n")),
        None => eprintln!(
            "note: git user.name/user.email not configured; archived_by will be omitted from .openspec.yaml"
        ),
    }
    content.push_str(&format!("archived_at: {today}\n"));
    std::fs::write(&meta_path, content).with_context(|| format!("writing {}", meta_path.display()))
}

fn apply_spec_deltas(
    cfg: &Config,
    archived_dir: &Path,
    source: &str,
    today: chrono::NaiveDate,
) -> Result<Vec<SpecApplyResult>> {
    let specs_dir = archived_dir.join("specs");
    let mut results = Vec::new();
    let Some(entries) = read_dir_optional(&specs_dir)? else {
        return Ok(results);
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", specs_dir.display()))?;
        let path = entry.path();
        if !is_dir(&path)? {
            continue;
        }
        let Some(delta) = read_optional(&path.join("spec.md"))? else {
            continue;
        };
        let capability = entry
            .file_name()
            .into_string()
            .map_err(|raw| anyhow!("capability directory name {raw:?} is not valid UTF-8"))?;
        let added = apply_added_requirements(cfg, &capability, &delta, source, today)?;
        results.push(SpecApplyResult { capability, added });
    }
    results.sort_by(|a, b| a.capability.cmp(&b.capability));
    Ok(results)
}

/// Extract every `### Requirement:` block from `delta`'s `## ADDED
/// Requirements` section (if present) and append each, verbatim, to
/// `<spec_dir>/specs/<capability>/spec.md` (creating it with a placeholder
/// Purpose if it doesn't exist yet), followed by a `<!-- @trace ... -->`
/// footer recording `source`/`updated`/`code` (the latter from this change's
/// `.spectra/touched/<name>.json`, if any). Returns the number of
/// requirements appended — 0 both when the delta has no `## ADDED
/// Requirements` section at all, and when that section is present but
/// empty (no `### Requirement:` blocks).
///
/// Assumes `delta` already passed [`validate_spec_deltas`] (called by
/// `archive` before the change directory is moved) and so has no
/// `MODIFIED`/`REMOVED`/`RENAMED Requirements` section.
fn apply_added_requirements(
    cfg: &Config,
    capability: &str,
    delta: &str,
    source: &str,
    today: chrono::NaiveDate,
) -> Result<usize> {
    let Some(added_match) = ADDED_HEADER_RE.find(delta) else {
        return Ok(0);
    };
    let after_header = &delta[added_match.end()..];
    let section_end = SECTION_HEADER_RE
        .find(after_header)
        .map_or(after_header.len(), |m| m.start());
    let section = &after_header[..section_end];

    let boundaries: Vec<usize> = REQUIREMENT_HEADER_RE
        .find_iter(section)
        .map(|m| m.start())
        .collect();
    if boundaries.is_empty() {
        return Ok(0);
    }
    let blocks: Vec<String> = boundaries
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let end = boundaries.get(i + 1).copied().unwrap_or(section.len());
            section[start..end].trim_end().to_string()
        })
        .collect();

    let spec_path = cfg.specs_dir().join(capability).join("spec.md");
    let mut content = read_optional(&spec_path)?.unwrap_or_else(|| {
        format!(
            "# {capability} Specification\n\n\
             ## Purpose\n\n\
             TBD - created by archiving change '{source}'. Update Purpose after archive.\n\n\
             ## Requirements\n"
        )
    });

    let mut code_files: Vec<String> = touched::already_recorded(cfg, source).into_iter().collect();
    code_files.sort();
    let code_yaml = if code_files.is_empty() {
        "code: []".to_string()
    } else {
        let mut s = "code:".to_string();
        for f in &code_files {
            s.push_str(&format!("\n  - {f}"));
        }
        s
    };

    let mut has_existing_requirement = content.contains("### Requirement:");
    for block in &blocks {
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(if has_existing_requirement {
            "\n---\n"
        } else {
            "\n"
        });
        content.push_str(&format!(
            "{block}\n\n<!-- @trace\nsource: {source}\nupdated: {today}\n{code_yaml}\n-->\n"
        ));
        has_existing_requirement = true;
    }

    let parent = spec_path.parent().expect("spec path always has a parent");
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    std::fs::write(&spec_path, content)
        .with_context(|| format!("writing {}", spec_path.display()))?;

    Ok(blocks.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "spectra-archive-test-{}-{seq}-{}",
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
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn cfg(tmp: &TempDir) -> Config {
        Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        }
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
        run(&["config", "user.name", "Ada Lovelace"]);
        run(&["config", "user.email", "ada@example.com"]);
        cfg(tmp)
    }

    const DELTA_TEMPLATE: &str = "## ADDED Requirements\n\n\
        ### Requirement: <!-- requirement name -->\n\n\
        <!-- requirement text -->\n\n\
        #### Scenario: <!-- scenario name -->\n\n\
        - **WHEN** <!-- condition -->\n\
        - **THEN** <!-- expected outcome -->\n";

    #[test]
    fn archive_moves_the_change_dir_to_dated_archive_subdir() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();

        let outcome = archive(&c, "my-feature", true, false).unwrap();

        assert_eq!(outcome.name, "my-feature");
        assert!(outcome.archived_name.ends_with("-my-feature"));
        assert!(c
            .changes_dir()
            .join("archive")
            .join(&outcome.archived_name)
            .join(".openspec.yaml")
            .is_file());
        assert!(!c.changes_dir().join("my-feature").exists());
    }

    #[test]
    fn archive_errors_when_change_does_not_exist() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);

        let err = archive(&c, "does-not-exist", true, false).unwrap_err();
        assert_eq!(err.to_string(), "Change 'does-not-exist' not found.");
    }

    #[test]
    fn archive_errors_when_change_is_already_archived() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        archive(&c, "my-feature", true, false).unwrap();

        let err = archive(&c, "my-feature", true, false).unwrap_err();
        assert_eq!(err.to_string(), "Change 'my-feature' not found.");
    }

    #[test]
    fn archive_stamps_archived_by_and_archived_at() {
        let tmp = TempDir::new();
        let c = git_repo_cfg(&tmp);
        change::create(&c, "my-feature").unwrap();

        let outcome = archive(&c, "my-feature", true, false).unwrap();

        let meta = std::fs::read_to_string(
            c.changes_dir()
                .join("archive")
                .join(&outcome.archived_name)
                .join(".openspec.yaml"),
        )
        .unwrap();
        assert!(meta.contains("archived_at: "));
        assert!(meta.contains("archived_by: Ada Lovelace <ada@example.com>"));
    }

    #[test]
    fn archive_with_mark_tasks_complete_flips_all_checkboxes() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature").join("tasks.md"),
            "- [ ] a\n- [ ] b\n",
        );

        let outcome = archive(&c, "my-feature", true, true).unwrap();

        let tasks = std::fs::read_to_string(
            c.changes_dir()
                .join("archive")
                .join(&outcome.archived_name)
                .join("tasks.md"),
        )
        .unwrap();
        assert_eq!(tasks, "- [x] a\n- [x] b\n");
    }

    #[test]
    fn archive_without_mark_tasks_complete_leaves_tasks_pending() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature").join("tasks.md"),
            "- [ ] a\n",
        );

        let outcome = archive(&c, "my-feature", true, false).unwrap();

        let tasks = std::fs::read_to_string(
            c.changes_dir()
                .join("archive")
                .join(&outcome.archived_name)
                .join("tasks.md"),
        )
        .unwrap();
        assert_eq!(tasks, "- [ ] a\n");
    }

    #[test]
    fn archive_creates_a_new_capability_spec_from_an_added_delta() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            DELTA_TEMPLATE,
        );

        let outcome = archive(&c, "my-feature", false, false).unwrap();

        assert_eq!(outcome.specs_applied.len(), 1);
        assert_eq!(outcome.specs_applied[0].capability, "my-cap");
        assert_eq!(outcome.specs_applied[0].added, 1);
        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap").join("spec.md")).unwrap();
        assert!(spec.starts_with("# my-cap Specification"));
        assert!(spec.contains("## Purpose"));
        assert!(spec.contains("TBD - created by archiving change 'my-feature'"));
        assert!(spec.contains("### Requirement: <!-- requirement name -->"));
        assert!(spec.contains("<!-- @trace\nsource: my-feature\n"));
        // The very first requirement in a fresh spec has no "---" separator.
        assert!(!spec.contains("---"));
    }

    #[test]
    fn archive_appends_to_an_existing_capability_spec_with_a_separator() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            "# my-cap Specification\n\n## Purpose\n\nExisting.\n\n## Requirements\n\n### Requirement: First\n\ntext\n",
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            DELTA_TEMPLATE,
        );

        archive(&c, "my-feature", false, false).unwrap();

        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap").join("spec.md")).unwrap();
        assert!(spec.contains("### Requirement: First"));
        assert!(spec.contains("---\n### Requirement: <!-- requirement name -->"));
    }

    #[test]
    fn archive_handles_multiple_requirements_in_one_added_section() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        let delta = "## ADDED Requirements\n\n\
            ### Requirement: First\n\ntext one\n\n\
            ### Requirement: Second\n\ntext two\n";
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            delta,
        );

        let outcome = archive(&c, "my-feature", false, false).unwrap();

        assert_eq!(outcome.specs_applied[0].added, 2);
        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap").join("spec.md")).unwrap();
        // Each requirement's own text stays with its own block, not bled into the other.
        let first_idx = spec.find("### Requirement: First").unwrap();
        let second_idx = spec.find("### Requirement: Second").unwrap();
        assert!(first_idx < second_idx);
        let between = &spec[first_idx..second_idx];
        assert!(between.contains("text one"));
        assert!(!between.contains("text two"));
        assert!(spec.contains("---\n### Requirement: Second"));
    }

    #[test]
    fn archive_stops_at_a_section_header_following_added_requirements() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        let delta = "## ADDED Requirements\n\n\
            ### Requirement: First\n\ntext one\n\n\
            ## Some Other Section\n\nunrelated trailing content\n";
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            delta,
        );

        archive(&c, "my-feature", false, false).unwrap();

        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap").join("spec.md")).unwrap();
        assert!(spec.contains("text one"));
        assert!(!spec.contains("unrelated trailing content"));
    }

    #[test]
    fn archive_skips_specs_when_skip_specs_is_true() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            DELTA_TEMPLATE,
        );

        let outcome = archive(&c, "my-feature", true, false).unwrap();

        assert!(outcome.specs_applied.is_empty());
        assert!(!c.specs_dir().join("my-cap").join("spec.md").exists());
    }

    #[test]
    fn archive_errors_on_a_modified_requirements_delta_without_skip_specs() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## MODIFIED Requirements\n\n### Requirement: First\n\nnew text\n",
        );

        let err = archive(&c, "my-feature", false, false).unwrap_err();
        assert!(err.to_string().contains("--skip-specs"));
    }

    #[test]
    fn archive_leaves_the_change_active_when_spec_validation_fails() {
        // Regression: validation must run *before* the directory move, so a
        // MODIFIED-delta error doesn't leave the change stuck half-archived
        // (directory already moved, but never fully processed).
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## MODIFIED Requirements\n\n### Requirement: First\n\nnew text\n",
        );

        assert!(archive(&c, "my-feature", false, false).is_err());

        assert!(
            c.changes_dir().join("my-feature").is_dir(),
            "change directory must still be active"
        );
        assert!(
            !c.changes_dir().join("archive").exists(),
            "nothing should have been moved"
        );
    }

    #[test]
    fn archive_is_a_noop_for_a_delta_with_no_added_section() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "# just some notes, no delta headers\n",
        );

        let outcome = archive(&c, "my-feature", false, false).unwrap();

        assert_eq!(outcome.specs_applied.len(), 1);
        assert_eq!(outcome.specs_applied[0].added, 0);
        assert!(!c.specs_dir().join("my-cap").join("spec.md").exists());
    }

    #[test]
    fn archive_clears_sidecar_state_on_success() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        change::park(&c, "my-feature").unwrap();

        archive(&c, "my-feature", true, false).unwrap();

        assert!(!c
            .root
            .join(".spectra")
            .join("changes")
            .join("my-feature.parked")
            .exists());
    }
}
