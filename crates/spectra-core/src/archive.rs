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
static REQUIREMENTS_HEADER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^## Requirements\s*$").unwrap());
static PURPOSE_HEADER_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?m)^## Purpose\s*$").unwrap());
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
/// capability's spec delta. Clears the change's `.spectra/` sidecar state
/// (parked/baseline/touched-file markers) on success -- best-effort: if
/// that cleanup itself fails (e.g. a permission error), it's reported as a
/// warning rather than failing the whole archive, since the move and spec
/// application have already succeeded by that point.
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
    .with_context(|| {
        format!(
            "change '{name}' was moved to {} but archiving could not be completed. \
             The change is at that location -- fix the issue, then either finish archiving by \
             hand or move it back to keep working on it",
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

/// Sets `archived_by`/`archived_at` on `.openspec.yaml` by deserializing it
/// into `ChangeMetadata`, updating those two fields, and reserializing --
/// rather than appending raw `key: value` text lines, which (a) could
/// produce a YAML value needing quoting/escaping that a raw string literal
/// wouldn't get (e.g. a git user.name containing `:` or `#`), and (b) would
/// produce a duplicate key if the file somehow already had `archived_by`/
/// `archived_at` (this shouldn't happen in the normal flow, but a raw
/// string append has no way to notice or correct it either way).
fn stamp_archived_metadata(cfg: &Config, dest: &Path, today: chrono::NaiveDate) -> Result<()> {
    let meta_path = dest.join(".openspec.yaml");
    let mut metadata: change::ChangeMetadata = match read_optional(&meta_path)? {
        None => change::ChangeMetadata::default(),
        Some(text) => serde_yaml::from_str(&text).unwrap_or_else(|e| {
            // Mirrors touched.rs::load: an unparseable file is renamed aside
            // (never silently overwritten in place) before falling back to
            // an empty default, so the original bytes survive for recovery
            // even though this function is about to write a fresh file here.
            let backup = touched::non_colliding_backup_path(&meta_path);
            let recovery_hint = match std::fs::rename(&meta_path, &backup) {
                Ok(()) => format!("the original file was preserved at {}", backup.display()),
                Err(rename_err) => {
                    format!("failed to preserve the original file too ({rename_err})")
                }
            };
            eprintln!(
                "warning: {} is unparseable ({e}); resetting its metadata -- {recovery_hint}",
                meta_path.display()
            );
            change::ChangeMetadata::default()
        }),
    };
    metadata.archived_by = crate::git::user_identity(&cfg.root);
    if metadata.archived_by.is_none() {
        eprintln!(
            "note: git user.name/user.email not configured; archived_by will be omitted from .openspec.yaml"
        );
    }
    metadata.archived_at = Some(today.to_string());
    let yaml = serde_yaml::to_string(&metadata)?;
    std::fs::write(&meta_path, yaml).with_context(|| format!("writing {}", meta_path.display()))
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

/// Where new requirement blocks belong in the canonical spec's existing
/// content: right after the `## Requirements` header, before whatever `##`
/// section (if any) follows it -- never blindly at the very end of the file,
/// which would incorrectly nest new requirements under an unrelated trailing
/// section (e.g. a human-added `## Notes`/`## Appendix`).
///
/// A spec freshly created by [`apply_added_requirements`] always has a
/// `## Requirements` header, but an existing canonical spec.md predating
/// that convention (or hand-edited to drop it) might not. Falling back to
/// `content.len()` in that case would silently reproduce the exact
/// trailing-section bug this function exists to avoid, so it falls back one
/// more step to right after `## Purpose` (using the same before-the-next-
/// section logic) before finally giving up and using the end of the file --
/// which is only reachable when the spec has no recognizable section
/// structure at all, so there's no trailing section left to nest under.
fn requirements_insertion_point(content: &str) -> usize {
    if let Some(req_match) = REQUIREMENTS_HEADER_RE.find(content) {
        let after = &content[req_match.end()..];
        return SECTION_HEADER_RE
            .find(after)
            .map_or(content.len(), |m| req_match.end() + m.start());
    }
    if let Some(purpose_match) = PURPOSE_HEADER_RE.find(content) {
        let after = &content[purpose_match.end()..];
        return SECTION_HEADER_RE
            .find(after)
            .map_or(content.len(), |m| purpose_match.end() + m.start());
    }
    content.len()
}

/// Extract every `### Requirement:` block from `delta`'s `## ADDED
/// Requirements` section (if present) and insert each, verbatim, into
/// `<spec_dir>/specs/<capability>/spec.md`'s own `## Requirements` section
/// (creating the file with a placeholder Purpose if it doesn't exist yet),
/// followed by a `<!-- @trace ... -->` footer recording
/// `source`/`updated`/`code` (the latter from this change's
/// `.spectra/touched/<name>.json`, if any). New blocks are inserted via
/// [`requirements_insertion_point`], not blindly appended to the end of the
/// file. Returns the number of requirements appended — 0 both when the delta
/// has no `## ADDED Requirements` section at all, and when that section is
/// present but empty (no `### Requirement:` blocks).
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
    let mut insertion = String::new();
    for block in &blocks {
        insertion.push_str(if has_existing_requirement {
            "\n---\n"
        } else {
            "\n"
        });
        insertion.push_str(&format!(
            "{block}\n\n<!-- @trace\nsource: {source}\nupdated: {today}\n{code_yaml}\n-->\n"
        ));
        has_existing_requirement = true;
    }

    let mut point = requirements_insertion_point(&content);
    if point == content.len() {
        if !content.ends_with('\n') {
            content.push('\n');
            point = content.len();
        }
    } else {
        // Inserting before an existing trailing section: leave a blank line
        // between the new trace footer and that section's header, matching
        // normal markdown spacing between sections.
        insertion.push('\n');
    }
    content.insert_str(point, &insertion);

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
        git_repo_cfg_with_identity(tmp, "Ada Lovelace", "ada@example.com")
    }

    fn git_repo_cfg_with_identity(tmp: &TempDir, name: &str, email: &str) -> Config {
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
        run(&["config", "user.name", name]);
        run(&["config", "user.email", email]);
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
    fn archive_stamps_archived_by_containing_yaml_special_characters_round_trips() {
        // A git user.name containing ':' or '#' would corrupt a raw
        // string-append into .openspec.yaml (unquoted ':' starts a new
        // mapping key, '#' starts a comment); stamp_archived_metadata must
        // go through serde_yaml so the value round-trips instead.
        let tmp = TempDir::new();
        let c = git_repo_cfg_with_identity(&tmp, "Weird: Name #1", "weird@example.com");
        change::create(&c, "my-feature").unwrap();

        let outcome = archive(&c, "my-feature", true, false).unwrap();

        let meta_path = c
            .changes_dir()
            .join("archive")
            .join(&outcome.archived_name)
            .join(".openspec.yaml");
        let meta = std::fs::read_to_string(&meta_path).unwrap();
        let parsed: change::ChangeMetadata = serde_yaml::from_str(&meta).unwrap();
        assert_eq!(
            parsed.archived_by.as_deref(),
            Some("Weird: Name #1 <weird@example.com>")
        );
    }

    #[test]
    fn archive_preserves_unknown_openspec_yaml_fields_through_the_metadata_round_trip() {
        // stamp_archived_metadata deserializes .openspec.yaml into
        // ChangeMetadata and reserializes it; a field this struct doesn't
        // model by name (e.g. from a newer reference-CLI version, or one a
        // human added) must survive via ChangeMetadata::extra's #[serde(flatten)]
        // rather than being silently dropped on every archive.
        let tmp = TempDir::new();
        let c = git_repo_cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature").join(".openspec.yaml"),
            "schema: v1\ncreated: 2024-01-01\nfuture_field: some-value-not-yet-modeled\n",
        );

        let outcome = archive(&c, "my-feature", true, false).unwrap();

        let meta = std::fs::read_to_string(
            c.changes_dir()
                .join("archive")
                .join(&outcome.archived_name)
                .join(".openspec.yaml"),
        )
        .unwrap();
        assert!(
            meta.contains("future_field: some-value-not-yet-modeled"),
            "unknown field must round-trip, got:\n{meta}"
        );
        assert!(meta.contains("created: 2024-01-01"));
        assert!(meta.contains("archived_at: "));
    }

    #[test]
    fn archive_backs_up_an_unparseable_openspec_yaml_instead_of_overwriting_it_in_place() {
        // Mirrors touched.rs's corrupt-file handling: an unparseable
        // .openspec.yaml must be renamed aside before stamp_archived_metadata
        // resets it to a fresh default, so the original bytes are still
        // recoverable instead of being silently destroyed by the write that
        // follows.
        let tmp = TempDir::new();
        let c = git_repo_cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature").join(".openspec.yaml"),
            "this: [is, not, : valid yaml\n",
        );

        let outcome = archive(&c, "my-feature", true, false).unwrap();

        let archived_dir = c.changes_dir().join("archive").join(&outcome.archived_name);
        let backup = std::fs::read_to_string(archived_dir.join(".openspec.yaml.corrupt")).unwrap();
        assert!(backup.contains("this: [is, not, : valid yaml"));
        let meta = std::fs::read_to_string(archived_dir.join(".openspec.yaml")).unwrap();
        assert!(meta.contains("archived_at: "));
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
    fn archive_with_mark_tasks_complete_succeeds_even_when_tasks_md_is_missing() {
        // --mark-tasks-complete was explicitly requested but has nothing to
        // do; this must not fail the whole archive (only warn on stderr).
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        std::fs::remove_file(c.changes_dir().join("my-feature").join("tasks.md")).unwrap();

        let outcome = archive(&c, "my-feature", true, true).unwrap();

        assert!(!c
            .changes_dir()
            .join("archive")
            .join(&outcome.archived_name)
            .join("tasks.md")
            .exists());
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
    fn archive_inserts_new_requirements_before_a_trailing_section_in_the_canonical_spec() {
        // The *canonical* spec (not the delta) has grown a human-added
        // section after "## Requirements" (e.g. "## Notes"). A newly
        // archived requirement must land inside "## Requirements", before
        // that trailing section -- not appended after it at the file's end.
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            "# my-cap Specification\n\n\
             ## Purpose\n\nExisting.\n\n\
             ## Requirements\n\n\
             ### Requirement: First\n\ntext\n\n\
             ## Notes\n\nSome human-added trailing notes.\n",
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
        let new_req_idx = spec
            .find("### Requirement: <!-- requirement name -->")
            .unwrap();
        let notes_idx = spec.find("## Notes").unwrap();
        assert!(
            new_req_idx < notes_idx,
            "new requirement must be inserted before the trailing '## Notes' section, got:\n{spec}"
        );
        assert!(spec.contains("Some human-added trailing notes."));
        assert!(
            spec[..notes_idx].ends_with("\n\n"),
            "a blank line must separate the inserted trace footer from '## Notes', got:\n{spec}"
        );
    }

    #[test]
    fn archive_inserts_new_requirements_before_a_trailing_section_when_the_canonical_spec_has_no_requirements_header(
    ) {
        // Regression: a canonical spec.md predating the "## Requirements"
        // convention (or hand-edited to drop it) must not fall all the way
        // back to a blind end-of-file append -- that would reproduce the
        // exact trailing-section bug `requirements_insertion_point` exists
        // to avoid. It should fall back to right after "## Purpose" instead.
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            "# my-cap Specification\n\n\
             ## Purpose\n\nExisting.\n\n\
             ## Notes\n\nSome human-added trailing notes.\n",
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
        let new_req_idx = spec
            .find("### Requirement: <!-- requirement name -->")
            .unwrap();
        let notes_idx = spec.find("## Notes").unwrap();
        assert!(
            new_req_idx < notes_idx,
            "new requirement must be inserted before the trailing '## Notes' section \
             even without a '## Requirements' header, got:\n{spec}"
        );
        assert!(spec.contains("Some human-added trailing notes."));
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

    #[test]
    fn archive_clears_touched_json_on_success() {
        // A change recreated with the same name after archiving must not
        // inherit stale touched-file history from before it was archived.
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        touched::record(
            &c,
            "my-feature",
            1,
            "did something",
            vec!["src/lib.rs".to_string()],
        )
        .unwrap();
        assert!(touched::touched_path(&c, "my-feature").is_file());

        archive(&c, "my-feature", true, false).unwrap();

        assert!(!touched::touched_path(&c, "my-feature").exists());
    }

    #[cfg(unix)]
    #[test]
    fn archive_fails_loudly_on_an_unreadable_spec_delta_instead_of_silently_dropping_it() {
        use std::os::unix::fs::PermissionsExt;

        if !crate::testutil::permissions_enforced() {
            eprintln!(
                "skipping archive_fails_loudly_on_an_unreadable_spec_delta_instead_of_silently_dropping_it: \
                 running as root or on a permission-ignoring filesystem, so a 0o000 read \
                 wouldn't be denied"
            );
            return;
        }

        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        let spec_path = c
            .changes_dir()
            .join("my-feature")
            .join("specs")
            .join("my-cap")
            .join("spec.md");
        write(&spec_path, DELTA_TEMPLATE);
        std::fs::set_permissions(&spec_path, std::fs::Permissions::from_mode(0o000)).unwrap();

        let result = archive(&c, "my-feature", false, false);

        assert!(result.is_err());
        // Validation (which hit the permission error) runs before the move.
        assert!(
            c.changes_dir().join("my-feature").is_dir(),
            "change must still be active"
        );
    }

    // macOS (APFS/HFS+) rejects non-UTF-8 filenames at the syscall level, so
    // this is only constructible on Linux (ext4 et al. allow arbitrary bytes)
    // -- matches the same platform-gating already used by spec.rs's
    // equivalent non-UTF-8-name test.
    #[cfg(target_os = "linux")]
    #[test]
    fn archive_errors_on_a_non_utf8_capability_directory_name() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        let specs_dir = c.changes_dir().join("my-feature").join("specs");
        let cap_dir = specs_dir.join(OsStr::from_bytes(b"bad-\xFF-cap"));
        std::fs::create_dir_all(&cap_dir).unwrap();
        std::fs::write(cap_dir.join("spec.md"), DELTA_TEMPLATE).unwrap();

        let err = archive(&c, "my-feature", false, false).unwrap_err();

        assert!(err.to_string().contains("not valid UTF-8"));
        assert!(
            c.changes_dir().join("my-feature").is_dir(),
            "change must still be active"
        );
    }

    #[cfg(unix)]
    #[test]
    fn archive_preserves_the_underlying_error_cause_after_a_post_rename_failure() {
        use std::os::unix::fs::PermissionsExt;

        if !crate::testutil::permissions_enforced() {
            eprintln!(
                "skipping archive_preserves_the_underlying_error_cause_after_a_post_rename_failure: \
                 running as root or on a permission-ignoring filesystem, so a 0o000 read \
                 wouldn't be denied"
            );
            return;
        }

        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        let meta_path = c.changes_dir().join("my-feature").join(".openspec.yaml");
        std::fs::set_permissions(&meta_path, std::fs::Permissions::from_mode(0o000)).unwrap();

        let err = archive(&c, "my-feature", true, false).unwrap_err();

        // {:#} is what main.rs actually prints; the underlying io::Error's
        // message must survive the "where did the change go" wrapper, not
        // be flattened away by it.
        let full_chain = format!("{err:#}");
        assert!(
            full_chain.to_lowercase().contains("permission denied"),
            "expected the permission-denied cause in the error chain, got: {full_chain}"
        );
    }
}
