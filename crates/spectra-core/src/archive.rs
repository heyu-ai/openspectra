//! `spectra archive` — move a completed change to
//! `<spec_dir>/changes/archive/YYYY-MM-DD-<name>/`, stamp its
//! `.openspec.yaml` with `archived_by`/`archived_at`, and (unless
//! `--skip-specs`) merge each capability's proposed requirement deltas into
//! the canonical `<spec_dir>/specs/<capability>/spec.md`.
//!
//! Reverse-engineered against `/Applications/Spectra.app` v2.3.1 — see
//! `docs/reverse-engineering/archive.md` for the full write-up, including
//! the Phase 2 OpenSpec compatibility delta behavior and other documented
//! gaps (no snapshot/unarchive support).

use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::Path;

use crate::config::Config;
use crate::fsutil::read_optional;
use crate::{change, touched};

static ADDED_HEADER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^## ADDED Requirements\s*$").unwrap());
static MODIFIED_HEADER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^## MODIFIED Requirements\s*$").unwrap());
static REMOVED_HEADER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^## REMOVED Requirements\s*$").unwrap());
static RENAMED_HEADER_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?m)^## RENAMED Requirements\s*$").unwrap());
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
    pub modified: usize,
    pub removed: usize,
    pub renamed: usize,
}

/// Outcome of [`archive`].
#[derive(Debug)]
pub struct ArchiveOutcome {
    pub name: String,
    pub archived_name: String,
    pub specs_applied: Vec<SpecApplyResult>,
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
/// Unless `skip_specs`, every capability's spec delta is validated
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
    no_validate: bool,
    mark_tasks_complete: bool,
) -> Result<ArchiveOutcome> {
    let ch = change::try_load(cfg, name)?.ok_or_else(|| anyhow!("Change '{name}' not found."))?;

    if !skip_specs && !no_validate {
        validate_spec_deltas(cfg, &ch.dir, name)?;
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

/// Computes every capability merge without writing it. This catches all
/// requirement-name conflicts before anything about the change has been
/// touched, preserving the same "active and unmoved on validation failure"
/// guarantee the earlier unsupported-header gate provided.
fn validate_spec_deltas(cfg: &Config, change_dir: &Path, source: &str) -> Result<()> {
    let specs_dir = change_dir.join("specs");
    let today = chrono::Local::now().date_naive();
    // Traverses `specs/**/spec.md` recursively via the shared collector, so a
    // nested-capability delta (`specs/<Epic>/<Feature>/spec.md`) is validated
    // here exactly as `apply_spec_deltas` will merge it below -- the two must
    // see the identical delta set, or a change could validate cleanly yet
    // archive with a nested delta silently ignored (issue #39).
    for (capability, delta) in crate::fsutil::collect_delta_specs(&specs_dir)? {
        merge_spec_delta(cfg, &capability, &delta, source, today, true)?;
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
    // Same recursive collector `validate_spec_deltas` used pre-move, so the
    // applied set is exactly the validated set. The capability id may be a
    // `/`-joined nested path (e.g. `Billing/Invoices`); `join`ing it produces
    // the matching nested canonical `specs/<Epic>/<Feature>/spec.md`, and
    // `create_dir_all` below makes the intermediate dirs.
    for (capability, delta) in crate::fsutil::collect_delta_specs(&specs_dir)? {
        let (content, result) = merge_spec_delta(cfg, &capability, &delta, source, today, false)?;
        if let Some(content) = content {
            let spec_path = cfg.specs_dir().join(&capability).join("spec.md");
            let parent = spec_path.parent().expect("spec path always has a parent");
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
            std::fs::write(&spec_path, content)
                .with_context(|| format!("writing {}", spec_path.display()))?;
        }
        results.push(result);
    }
    // No explicit sort: `collect_delta_specs` already yields entries sorted by
    // capability id (pinned by `fsutil`'s `collect_delta_specs_*` unit tests),
    // and results are pushed in that order.
    Ok(results)
}

#[derive(Debug, Clone)]
struct RequirementBlock {
    name: String,
    normalized_name: String,
    start: usize,
    end: usize,
    header_end: usize,
}

#[derive(Debug)]
struct RequirementDelta {
    added: Vec<String>,
    modified: Vec<String>,
    removed: Vec<String>,
    renamed: Vec<RenameDelta>,
}

#[derive(Debug)]
struct RenameDelta {
    from: String,
    to: String,
}

/// Merge one capability's delta into its canonical spec, returning the full
/// replacement content only when the canonical file should be written. The
/// same function is used by pre-move validation and post-move application so
/// conflicts are found against the exact operation order OpenSpec documents:
/// RENAMED, REMOVED, MODIFIED, then ADDED.
fn merge_spec_delta(
    cfg: &Config,
    capability: &str,
    delta: &str,
    source: &str,
    today: chrono::NaiveDate,
    dry_run: bool,
) -> Result<(Option<String>, SpecApplyResult)> {
    let parsed = parse_requirement_delta(capability, delta)?;
    let result = SpecApplyResult {
        capability: capability.to_string(),
        added: parsed.added.len(),
        modified: parsed.modified.len(),
        removed: parsed.removed.len(),
        renamed: parsed.renamed.len(),
    };
    let has_changes = result.added + result.modified + result.removed + result.renamed > 0;
    if !has_changes {
        return Ok((None, result));
    }

    let spec_path = cfg.specs_dir().join(capability).join("spec.md");
    let existing = read_optional(&spec_path)?;
    let mut content = match existing {
        Some(content) => content,
        None if parsed.modified.is_empty()
            && parsed.removed.is_empty()
            && parsed.renamed.is_empty() =>
        {
            format!(
                "# {capability} Specification\n\n\
                 ## Purpose\n\n\
                 TBD - created by archiving change '{source}'. Update Purpose after archive.\n\n\
                 ## Requirements\n"
            )
        }
        None => {
            let first_missing = parsed
                .renamed
                .first()
                .map(|r| ("RENAME", r.from.as_str()))
                .or_else(|| parsed.removed.first().map(|r| ("REMOVE", r.as_str())))
                .or_else(|| {
                    parsed
                        .modified
                        .first()
                        .map(|r| ("MODIFY", requirement_name(r)))
                });
            let (kind, name) = first_missing.expect("non-ADDED delta exists");
            return Err(missing_requirement_error(
                capability, kind, name, &spec_path,
            ));
        }
    };

    for rename in &parsed.renamed {
        if find_requirement_block(&content, &rename.to).is_some() {
            return Err(anyhow!(
                "capability '{capability}': cannot RENAME requirement to '{}' -- a requirement with that name already exists in {}",
                normalize_requirement_name(&rename.to),
                spec_path.display()
            ));
        }
        let block = find_requirement_block(&content, &rename.from).ok_or_else(|| {
            missing_requirement_error(capability, "RENAME", &rename.from, &spec_path)
        })?;
        content.replace_range(
            block.start..block.header_end,
            &format!("### Requirement: {}", rename.to.trim()),
        );
    }

    for removed in &parsed.removed {
        let block = find_requirement_block(&content, removed)
            .ok_or_else(|| missing_requirement_error(capability, "REMOVE", removed, &spec_path))?;
        content.replace_range(block.start..block.end, "");
    }

    for modified in &parsed.modified {
        let name = requirement_name(modified);
        let block = find_requirement_block(&content, name)
            .ok_or_else(|| missing_requirement_error(capability, "MODIFY", name, &spec_path))?;
        // `block.start..block.end` spans the canonical block *including* the
        // trailing separator (blank line / `\n---\n` / EOF) that sits before
        // the next header, but `modified` is `trim_end()`'d. Re-append the
        // original block's trailing whitespace so the following
        // `### Requirement:` stays at the start of its own line -- otherwise it
        // is glued onto the modified block's last line and the multiline
        // `^### Requirement:` regex silently stops matching it, dropping the
        // requirement from every later parse.
        let trailing = {
            let original = &content[block.start..block.end];
            original[original.trim_end().len()..].to_string()
        };
        content.replace_range(block.start..block.end, &format!("{modified}{trailing}"));
    }

    let mut added_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for added in &parsed.added {
        let name = requirement_name(added);
        if find_requirement_block(&content, name).is_some() {
            return Err(anyhow!(
                "capability '{capability}': cannot ADD requirement '{name}' -- it already exists in {}",
                spec_path.display()
            ));
        }
        // The exists check above only compares against the canonical spec, not
        // the delta's own ADDED list -- so two same-named `### Requirement:`
        // blocks in one ADDED section would both append (a duplicate header).
        // Reject the second loudly instead.
        if !added_names.insert(normalize_requirement_name(name)) {
            return Err(anyhow!(
                "capability '{capability}': delta ADDs requirement '{}' more than once",
                normalize_requirement_name(name)
            ));
        }
    }

    // Only application writes; validation (`dry_run`) discards `content`, so it
    // must skip the append -- `append_added_requirements` loads this change's
    // touched-file sidecar, which renames a *corrupt* sidecar aside. Doing that
    // during validation would be a side effect on a run that may still fail on a
    // later capability, breaking the "validation failure leaves the change
    // untouched" guarantee. The conflict checks above are what validation needs;
    // they've all run by this point.
    if !dry_run {
        append_added_requirements(&mut content, &parsed.added, cfg, source, today);
    }

    Ok((Some(content), result))
}

fn parse_requirement_delta(capability: &str, delta: &str) -> Result<RequirementDelta> {
    // Each delta kind is parsed from its *first* matching header, with the
    // section bounded by the next `## ` header -- so a second same-kind header
    // (e.g. from a bad merge) would have its body silently dropped. Reject a
    // duplicate loudly instead, consistent with the empty-section guard below.
    for (header_re, kind) in [
        (&*ADDED_HEADER_RE, "ADDED"),
        (&*MODIFIED_HEADER_RE, "MODIFIED"),
        (&*REMOVED_HEADER_RE, "REMOVED"),
        (&*RENAMED_HEADER_RE, "RENAMED"),
    ] {
        if header_re.find_iter(delta).count() > 1 {
            return Err(anyhow!(
                "capability '{capability}': delta has more than one \
                 `## {kind} Requirements` section -- merge them into one"
            ));
        }
    }

    let added: Vec<String> = requirement_blocks_in_section(delta, &ADDED_HEADER_RE)?
        .into_iter()
        .map(|b| block_text(delta, &b))
        .collect();
    let modified: Vec<String> = requirement_blocks_in_section(delta, &MODIFIED_HEADER_RE)?
        .into_iter()
        .map(|b| block_text(delta, &b))
        .collect();
    let removed: Vec<String> = requirement_blocks_in_section(delta, &REMOVED_HEADER_RE)?
        .into_iter()
        .map(|b| b.name)
        .collect();
    let renamed = parse_renamed_requirements(capability, delta)?;

    // A recognized delta section header that parsed to zero entries is almost
    // always a malformed delta (a prose-only body, the wrong heading level, or
    // a REMOVED/RENAMED entry not written as a `### Requirement:` header).
    // Erroring loudly here preserves the guarantee the old unsupported-header
    // reject gave: `archive` must never silently drop a MODIFIED/REMOVED/RENAMED
    // intent and move the change out of the active set with nothing applied.
    // ADDED keeps its historical lenient no-op for an empty section -- it never
    // had a reject gate to preserve, and an empty ADDED section is harmless.
    ensure_section_non_empty(
        capability,
        delta,
        &MODIFIED_HEADER_RE,
        "MODIFIED",
        modified.len(),
    )?;
    ensure_section_non_empty(
        capability,
        delta,
        &REMOVED_HEADER_RE,
        "REMOVED",
        removed.len(),
    )?;
    ensure_section_non_empty(
        capability,
        delta,
        &RENAMED_HEADER_RE,
        "RENAMED",
        renamed.len(),
    )?;

    Ok(RequirementDelta {
        added,
        modified,
        removed,
        renamed,
    })
}

/// Errors when `header_re` matches `delta` (the section is present) but the
/// section parsed to zero entries -- see [`parse_requirement_delta`] for why a
/// present-but-empty MODIFIED/REMOVED/RENAMED section must fail loudly rather
/// than archive as a silent no-op.
fn ensure_section_non_empty(
    capability: &str,
    delta: &str,
    header_re: &Regex,
    kind: &str,
    parsed_count: usize,
) -> Result<()> {
    if parsed_count == 0 && header_re.is_match(delta) {
        return Err(anyhow!(
            "capability '{capability}': `## {kind} Requirements` section contains no \
             recognizable entries -- fix the delta or re-run with --skip-specs"
        ));
    }
    Ok(())
}

fn requirement_blocks_in_section(delta: &str, header_re: &Regex) -> Result<Vec<RequirementBlock>> {
    let Some(header_match) = header_re.find(delta) else {
        return Ok(Vec::new());
    };
    let after_header = &delta[header_match.end()..];
    let section_end = SECTION_HEADER_RE
        .find(after_header)
        .map_or(after_header.len(), |m| m.start());
    Ok(requirement_blocks(after_header[..section_end].as_ref())
        .into_iter()
        .map(|mut block| {
            block.start += header_match.end();
            block.end += header_match.end();
            block.header_end += header_match.end();
            block
        })
        .collect())
}

fn parse_renamed_requirements(capability: &str, delta: &str) -> Result<Vec<RenameDelta>> {
    let Some(header_match) = RENAMED_HEADER_RE.find(delta) else {
        return Ok(Vec::new());
    };
    let after_header = &delta[header_match.end()..];
    let section_end = SECTION_HEADER_RE
        .find(after_header)
        .map_or(after_header.len(), |m| m.start());
    let section = &after_header[..section_end];

    let mut renames = Vec::new();
    let mut pending_from: Option<String> = None;
    for line in section.lines() {
        // Accept either `-` or `*` as the list bullet: a markdown auto-formatter
        // routinely rewrites one to the other, and OpenSpec's own examples use
        // `-` but don't forbid `*`. A line that isn't a bullet at all is skipped.
        let Some(item) = line
            .trim_start()
            .strip_prefix('-')
            .or_else(|| line.trim_start().strip_prefix('*'))
            .map(str::trim_start)
        else {
            continue;
        };
        if let Some(raw) = item.strip_prefix("FROM:") {
            if pending_from.is_some() {
                return Err(anyhow!(
                    "capability '{capability}': malformed RENAMED Requirements section -- FROM without following TO"
                ));
            }
            pending_from = Some(parse_backticked_requirement_header(
                capability, "FROM", raw,
            )?);
        } else if let Some(raw) = item.strip_prefix("TO:") {
            let Some(from) = pending_from.take() else {
                return Err(anyhow!(
                    "capability '{capability}': malformed RENAMED Requirements section -- TO without preceding FROM"
                ));
            };
            let to = parse_backticked_requirement_header(capability, "TO", raw)?;
            renames.push(RenameDelta { from, to });
        }
    }

    if pending_from.is_some() {
        return Err(anyhow!(
            "capability '{capability}': malformed RENAMED Requirements section -- FROM without following TO"
        ));
    }

    Ok(renames)
}

fn parse_backticked_requirement_header(capability: &str, field: &str, raw: &str) -> Result<String> {
    let Some(start) = raw.find('`') else {
        return Err(anyhow!(
            "capability '{capability}': malformed RENAMED Requirements section -- {field} is missing a backticked requirement header"
        ));
    };
    let rest = &raw[start + 1..];
    let Some(end) = rest.find('`') else {
        return Err(anyhow!(
            "capability '{capability}': malformed RENAMED Requirements section -- {field} is missing a closing backtick"
        ));
    };
    requirement_name_from_header(&rest[..end]).ok_or_else(|| {
        anyhow!(
            "capability '{capability}': malformed RENAMED Requirements section -- {field} must be a `### Requirement: <name>` header"
        )
    })
}

fn block_text(content: &str, block: &RequirementBlock) -> String {
    content[block.start..block.end].trim_end().to_string()
}

fn requirement_blocks(content: &str) -> Vec<RequirementBlock> {
    REQUIREMENT_HEADER_RE
        .find_iter(content)
        .filter_map(|m| {
            let line_end = content[m.start()..]
                .find('\n')
                .map_or(content.len(), |offset| m.start() + offset);
            let header = &content[m.start()..line_end];
            let name = requirement_name_from_header(header)?;
            let after_header = &content[line_end..];
            let next_requirement = REQUIREMENT_HEADER_RE
                .find(after_header)
                .map(|next| line_end + next.start());
            let next_section = SECTION_HEADER_RE
                .find(after_header)
                .map(|next| line_end + next.start());
            let end = match (next_requirement, next_section) {
                (Some(req), Some(section)) => req.min(section),
                (Some(req), None) => req,
                (None, Some(section)) => section,
                (None, None) => content.len(),
            };
            Some(RequirementBlock {
                name: name.clone(),
                normalized_name: normalize_requirement_name(&name),
                start: m.start(),
                end,
                header_end: line_end,
            })
        })
        .collect()
}

fn find_requirement_block(content: &str, name: &str) -> Option<RequirementBlock> {
    let needle = normalize_requirement_name(name);
    requirement_blocks(content)
        .into_iter()
        .find(|block| block.normalized_name == needle)
}

fn requirement_name(block: &str) -> &str {
    let line_end = block.find('\n').unwrap_or(block.len());
    block[..line_end]
        .strip_prefix("### Requirement:")
        .expect("requirement block starts with a requirement header")
        .trim()
}

fn requirement_name_from_header(header: &str) -> Option<String> {
    header
        .strip_prefix("### Requirement:")
        .map(|name| name.trim().to_string())
}

fn normalize_requirement_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn missing_requirement_error(
    capability: &str,
    kind: &str,
    name: &str,
    spec_path: &Path,
) -> anyhow::Error {
    anyhow!(
        "capability '{capability}': cannot {kind} requirement '{}' -- it does not exist in {}",
        normalize_requirement_name(name),
        spec_path.display()
    )
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

/// Append ADDED requirement blocks to `content` using the original
/// reverse-engineered placement and trace-footer rules. The earlier
/// RENAMED/REMOVED/MODIFIED operations mutate `content` first, so ADDED's
/// duplicate checks and insertion point see the post-merge canonical spec.
fn append_added_requirements(
    content: &mut String,
    blocks: &[String],
    cfg: &Config,
    source: &str,
    today: chrono::NaiveDate,
) {
    if blocks.is_empty() {
        return;
    }
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
    for block in blocks {
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

    let mut point = requirements_insertion_point(content);
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
            claude_slash_commands: false,
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

    const CANONICAL_SPEC: &str = "# my-cap Specification\n\n\
        ## Purpose\n\nExisting.\n\n\
        ## Requirements\n\n\
        ### Requirement: First\n\nfirst text\n\n\
        #### Scenario: First scenario\n\n\
        - **WHEN** first happens\n\
        - **THEN** first works\n\n\
        ### Requirement: Second\n\nsecond text\n\n\
        ### Requirement: Third\n\nthird text\n";

    #[test]
    fn archive_moves_the_change_dir_to_dated_archive_subdir() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();

        let outcome = archive(&c, "my-feature", true, false, false).unwrap();

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

        let err = archive(&c, "does-not-exist", true, false, false).unwrap_err();
        assert_eq!(err.to_string(), "Change 'does-not-exist' not found.");
    }

    #[test]
    fn archive_errors_when_change_is_already_archived() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        archive(&c, "my-feature", true, false, false).unwrap();

        let err = archive(&c, "my-feature", true, false, false).unwrap_err();
        assert_eq!(err.to_string(), "Change 'my-feature' not found.");
    }

    #[test]
    fn archive_stamps_archived_by_and_archived_at() {
        let tmp = TempDir::new();
        let c = git_repo_cfg(&tmp);
        change::create(&c, "my-feature").unwrap();

        let outcome = archive(&c, "my-feature", true, false, false).unwrap();

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

        let outcome = archive(&c, "my-feature", true, false, false).unwrap();

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

        let outcome = archive(&c, "my-feature", true, false, false).unwrap();

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

        let outcome = archive(&c, "my-feature", true, false, false).unwrap();

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

        let outcome = archive(&c, "my-feature", true, false, true).unwrap();

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

        let outcome = archive(&c, "my-feature", true, false, true).unwrap();

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

        let outcome = archive(&c, "my-feature", true, false, false).unwrap();

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

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

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
    fn archive_applies_a_nested_capability_delta() {
        // Regression (#39): `archive` must traverse nested-capability layouts
        // (`specs/<Epic>/<Feature>/spec.md`) the same way `validate` does.
        // Before the shared recursive collector, archive's single-level walk
        // silently ignored the nested delta -- the change moved to the archive
        // with the requirement never merged into any canonical spec, and no
        // error reported.
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("Billing")
                .join("Invoices")
                .join("spec.md"),
            DELTA_TEMPLATE,
        );

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

        assert_eq!(outcome.specs_applied.len(), 1);
        assert_eq!(outcome.specs_applied[0].capability, "Billing/Invoices");
        assert_eq!(outcome.specs_applied[0].added, 1);
        // The nested capability id maps to the matching nested canonical path.
        let spec = std::fs::read_to_string(
            c.specs_dir()
                .join("Billing")
                .join("Invoices")
                .join("spec.md"),
        )
        .unwrap();
        assert!(spec.starts_with("# Billing/Invoices Specification"));
        assert!(spec.contains("### Requirement: <!-- requirement name -->"));
        assert!(spec.contains("<!-- @trace\nsource: my-feature\n"));
    }

    #[cfg(unix)]
    #[test]
    fn archive_does_not_follow_a_symlink_cycle_under_specs() {
        // Regression (#39): archive's recursive spec walk must not follow
        // directory symlinks, or a checked-in cycle (`specs/loop -> specs`)
        // recurses without bound -> stack overflow, crashing archive instead of
        // completing. Mirrors validate's symlink guard. (If this regresses it
        // stack-overflows the test process rather than failing an assertion --
        // which is exactly the crash we are guarding against.)
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
        let specs_root = c.changes_dir().join("my-feature").join("specs");
        std::os::unix::fs::symlink(&specs_root, specs_root.join("loop")).unwrap();

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

        assert_eq!(outcome.specs_applied.len(), 1);
        assert_eq!(outcome.specs_applied[0].capability, "my-cap");
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

        archive(&c, "my-feature", false, false, false).unwrap();

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

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

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

        archive(&c, "my-feature", false, false, false).unwrap();

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

        archive(&c, "my-feature", false, false, false).unwrap();

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

        archive(&c, "my-feature", false, false, false).unwrap();

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

        let outcome = archive(&c, "my-feature", true, false, false).unwrap();

        assert!(outcome.specs_applied.is_empty());
        assert!(!c.specs_dir().join("my-cap").join("spec.md").exists());
    }

    #[test]
    fn archive_applies_a_modified_requirements_delta() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            CANONICAL_SPEC,
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## MODIFIED Requirements\n\n\
             ### Requirement: Second\n\n\
             modified text\n\
             (Previously: second text)\n\n\
             #### Scenario: Modified scenario\n\n\
             - **WHEN** second changes\n\
             - **THEN** modified behavior applies\n",
        );

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

        assert_eq!(outcome.specs_applied[0].modified, 1);
        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap").join("spec.md")).unwrap();
        assert!(spec.contains("### Requirement: First"));
        assert!(spec.contains("first text"));
        assert!(spec.contains("### Requirement: Second\n\nmodified text"));
        assert!(spec.contains("(Previously: second text)"));
        assert!(!spec.contains("second text\n\n### Requirement: Third"));
        // Regression: the header following a MODIFIED block must stay at the
        // start of its own line. A `trim_end()`'d replacement used to glue it
        // onto the modified block's last line ("modified text### Requirement:
        // Third"), after which `^### Requirement:` silently dropped it.
        assert!(
            spec.contains("\n### Requirement: Third"),
            "Third must remain at line-start after MODIFY, got:\n{spec}"
        );
        assert_eq!(
            REQUIREMENT_HEADER_RE.find_iter(&spec).count(),
            3,
            "all three requirement headers must remain line-anchored, got:\n{spec}"
        );
    }

    #[test]
    fn archive_applies_a_removed_requirements_delta() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            CANONICAL_SPEC,
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## REMOVED Requirements\n\n### Requirement: Second\n\nDeprecated.\n",
        );

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

        assert_eq!(outcome.specs_applied[0].removed, 1);
        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap").join("spec.md")).unwrap();
        assert!(spec.contains("### Requirement: First"));
        assert!(!spec.contains("### Requirement: Second"));
        assert!(spec.contains("### Requirement: Third"));
    }

    #[test]
    fn archive_applies_rename_before_modify_on_the_new_name() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            CANONICAL_SPEC,
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## RENAMED Requirements\n\
             - FROM: `### Requirement: Second`\n\
             - TO: `### Requirement: Better Second`\n\n\
             ## MODIFIED Requirements\n\n\
             ### Requirement: Better Second\n\n\
             renamed and modified text\n",
        );

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

        assert_eq!(outcome.specs_applied[0].renamed, 1);
        assert_eq!(outcome.specs_applied[0].modified, 1);
        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap").join("spec.md")).unwrap();
        assert!(!spec.contains("### Requirement: Second"));
        assert!(spec.contains("### Requirement: Better Second\n\nrenamed and modified text"));
    }

    #[test]
    fn archive_applies_all_delta_kinds_in_openspec_order() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            CANONICAL_SPEC,
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## ADDED Requirements\n\n\
             ### Requirement: Fourth\n\nfourth text\n\n\
             ## MODIFIED Requirements\n\n\
             ### Requirement: Renamed First\n\nrenamed first modified text\n\n\
             ## REMOVED Requirements\n\n\
             ### Requirement: Third\n\nremove it\n\n\
             ## RENAMED Requirements\n\
             - FROM: `### Requirement: First`\n\
             - TO: `### Requirement: Renamed First`\n",
        );

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

        assert_eq!(outcome.specs_applied[0].added, 1);
        assert_eq!(outcome.specs_applied[0].modified, 1);
        assert_eq!(outcome.specs_applied[0].removed, 1);
        assert_eq!(outcome.specs_applied[0].renamed, 1);
        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap").join("spec.md")).unwrap();
        assert!(!spec.contains("### Requirement: First"));
        assert!(spec.contains("### Requirement: Renamed First\n\nrenamed first modified text"));
        assert!(spec.contains("### Requirement: Second"));
        assert!(!spec.contains("### Requirement: Third"));
        assert!(spec.contains("### Requirement: Fourth"));
        assert!(spec.contains("<!-- @trace\nsource: my-feature\n"));
    }

    #[test]
    fn archive_errors_on_a_modified_delta_for_a_nonexistent_requirement() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            CANONICAL_SPEC,
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## MODIFIED Requirements\n\n### Requirement: Missing\n\nnew text\n",
        );

        let err = archive(&c, "my-feature", false, false, false).unwrap_err();
        assert!(err
            .to_string()
            .contains("capability 'my-cap': cannot MODIFY requirement 'Missing'"));
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
    fn archive_errors_on_a_renamed_delta_for_an_existing_target_requirement() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap").join("spec.md");
        write(&spec_path, CANONICAL_SPEC);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## RENAMED Requirements\n- FROM: `### Requirement: First`\n- TO: `### Requirement: Second`\n",
        );

        let err = archive(&c, "my-feature", false, false, false).unwrap_err();
        assert!(err
            .to_string()
            .contains("capability 'my-cap': cannot RENAME requirement to 'Second'"));
        assert!(
            c.changes_dir().join("my-feature").is_dir(),
            "change directory must still be active"
        );
        assert!(
            !c.changes_dir().join("archive").exists(),
            "nothing should have been moved"
        );
        assert_eq!(std::fs::read_to_string(spec_path).unwrap(), CANONICAL_SPEC);
    }

    #[test]
    fn archive_errors_on_removed_and_renamed_missing_requirements() {
        for (kind, delta, expected) in [
            (
                "removed",
                "## REMOVED Requirements\n\n### Requirement: Missing\n\nold\n",
                "cannot REMOVE requirement 'Missing'",
            ),
            (
                "renamed",
                "## RENAMED Requirements\n- FROM: `### Requirement: Missing`\n- TO: `### Requirement: New`\n",
                "cannot RENAME requirement 'Missing'",
            ),
        ] {
            let tmp = TempDir::new();
            let c = cfg(&tmp);
            write(&c.specs_dir().join("my-cap").join("spec.md"), CANONICAL_SPEC);
            change::create(&c, "my-feature").unwrap();
            write(
                &c.changes_dir()
                    .join("my-feature")
                    .join("specs")
                    .join("my-cap")
                    .join("spec.md"),
                delta,
            );

            let err = archive(&c, "my-feature", false, false, false).unwrap_err();

            assert!(
                err.to_string().contains(expected),
                "{kind} conflict should name the missing requirement, got: {err}"
            );
            assert!(c.changes_dir().join("my-feature").is_dir());
            assert!(!c.changes_dir().join("archive").exists());
        }
    }

    #[test]
    fn archive_errors_on_an_added_requirement_that_already_exists() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            CANONICAL_SPEC,
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## ADDED Requirements\n\n### Requirement: Second\n\nnew duplicate\n",
        );

        let err = archive(&c, "my-feature", false, false, false).unwrap_err();

        assert!(err
            .to_string()
            .contains("capability 'my-cap': cannot ADD requirement 'Second'"));
        assert!(c.changes_dir().join("my-feature").is_dir());
        assert!(!c.changes_dir().join("archive").exists());
    }

    #[test]
    fn archive_matches_requirement_headers_with_collapsed_whitespace() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            "# my-cap Specification\n\n## Requirements\n\n### Requirement: Session Expiration\n\nold\n",
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## MODIFIED Requirements\n\n### Requirement:   Session    Expiration  \n\nnew\n",
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap").join("spec.md")).unwrap();
        assert!(spec.contains("### Requirement:   Session    Expiration  \n\nnew"));
        assert!(!spec.contains("\nold\n"));
    }

    // A canonical spec in the exact shape `archive` itself produces: `---`
    // separators between requirements and a `<!-- @trace -->` footer on each.
    // The hand-written CANONICAL_SPEC lacks both, so MODIFYing a spectra-
    // produced spec is only exercised here.
    const SPECTRA_PRODUCED_SPEC: &str = "# my-cap Specification\n\n\
        ## Purpose\n\nExisting.\n\n\
        ## Requirements\n\n\
        ### Requirement: Alpha\n\nalpha text\n\n\
        <!-- @trace\nsource: old\nupdated: 2026-01-01\ncode: []\n-->\n\n\
        ---\n\
        ### Requirement: Beta\n\nbeta text\n\n\
        <!-- @trace\nsource: old\nupdated: 2026-01-01\ncode: []\n-->\n\n\
        ---\n\
        ### Requirement: Gamma\n\ngamma text\n\n\
        <!-- @trace\nsource: old\nupdated: 2026-01-01\ncode: []\n-->\n";

    #[test]
    fn archive_modify_on_a_spectra_produced_spec_keeps_following_headers_line_anchored() {
        // Regression for the MODIFIED-glue bug: MODIFYing a non-last
        // requirement in a spec that carries `---`/`@trace` footers must not
        // glue the following `### Requirement:` onto the modified block's last
        // line. Before the fix this dropped Beta (count == 2, corrupt output);
        // it still archived "successfully", so a `.contains(header)` assertion
        // wouldn't catch it.
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            SPECTRA_PRODUCED_SPEC,
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## MODIFIED Requirements\n\n### Requirement: Alpha\n\nmodified alpha text\n",
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap").join("spec.md")).unwrap();
        assert!(spec.contains("modified alpha text"));
        assert_eq!(
            REQUIREMENT_HEADER_RE.find_iter(&spec).count(),
            3,
            "all three requirement headers must survive as line-anchored, got:\n{spec}"
        );
        assert!(spec.contains("\n### Requirement: Beta"));
        assert!(spec.contains("\n### Requirement: Gamma"));
        // Beta/Gamma were untouched, so their trace footers must remain.
        assert!(spec.contains("### Requirement: Beta\n\nbeta text"));
    }

    #[test]
    fn archive_errors_on_a_present_but_empty_delta_section() {
        // A recognized MODIFIED/REMOVED/RENAMED header whose body parses to
        // zero entries must fail loudly, not archive as a silent no-op (the
        // guarantee the old unsupported-header reject provided).
        for (delta, kind) in [
            (
                "## MODIFIED Requirements\n\nsome prose but no requirement blocks\n",
                "MODIFIED",
            ),
            (
                "## REMOVED Requirements\n\nsome prose but no requirement blocks\n",
                "REMOVED",
            ),
            (
                "## RENAMED Requirements\n\nsome prose but no from/to bullets\n",
                "RENAMED",
            ),
        ] {
            let tmp = TempDir::new();
            let c = cfg(&tmp);
            write(
                &c.specs_dir().join("my-cap").join("spec.md"),
                CANONICAL_SPEC,
            );
            change::create(&c, "my-feature").unwrap();
            write(
                &c.changes_dir()
                    .join("my-feature")
                    .join("specs")
                    .join("my-cap")
                    .join("spec.md"),
                delta,
            );

            let err = archive(&c, "my-feature", false, false, false).unwrap_err();
            assert!(
                err.to_string()
                    .contains(&format!("`## {kind} Requirements` section contains no")),
                "{kind} empty section should fail loudly, got: {err}"
            );
            assert!(c.changes_dir().join("my-feature").is_dir());
            assert!(!c.changes_dir().join("archive").exists());
        }
    }

    #[test]
    fn archive_errors_on_duplicate_section_headers() {
        // Two `## MODIFIED Requirements` sections: only the first is parsed, so
        // the second would be silently dropped -- reject loudly instead.
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            CANONICAL_SPEC,
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## MODIFIED Requirements\n\n### Requirement: First\n\na\n\n\
             ## MODIFIED Requirements\n\n### Requirement: Second\n\nb\n",
        );

        let err = archive(&c, "my-feature", false, false, false).unwrap_err();
        assert!(
            err.to_string()
                .contains("more than one `## MODIFIED Requirements` section"),
            "got: {err}"
        );
        assert!(c.changes_dir().join("my-feature").is_dir());
        assert!(!c.changes_dir().join("archive").exists());
    }

    #[test]
    fn archive_errors_on_a_delta_that_adds_the_same_requirement_twice() {
        // The canonical-spec exists check can't catch an intra-delta duplicate
        // (neither block is in the canonical spec yet), so a dedicated guard
        // must reject it rather than append a duplicate header.
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## ADDED Requirements\n\n\
             ### Requirement: Dup\n\na\n\n\
             ### Requirement: Dup\n\nb\n",
        );

        let err = archive(&c, "my-feature", false, false, false).unwrap_err();
        assert!(
            err.to_string()
                .contains("delta ADDs requirement 'Dup' more than once"),
            "got: {err}"
        );
        assert!(c.changes_dir().join("my-feature").is_dir());
        assert!(!c.changes_dir().join("archive").exists());
    }

    #[test]
    fn merge_validation_is_side_effect_free_on_the_touched_sidecar() {
        // Regression: pre-move validation runs `merge_spec_delta` in `dry_run`
        // mode, which must NOT run the ADDED append -- the append loads this
        // change's touched sidecar and renames a *corrupt* one aside. Doing that
        // during validation would mutate `.spectra/touched/` even on a run that
        // later fails another capability, breaking the "validation failure
        // leaves the change untouched" guarantee.
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        let touched = crate::touched::touched_path(&c, "my-feature");
        write(&touched, "not valid json");
        let corrupt_backup = touched.with_extension("json.corrupt");
        let today = chrono::Local::now().date_naive();
        let added_delta = "## ADDED Requirements\n\n### Requirement: New\n\ntext\n";

        // dry_run (validation): no append, so the corrupt sidecar is untouched.
        merge_spec_delta(&c, "my-cap", added_delta, "my-feature", today, true).unwrap();
        assert!(
            touched.is_file() && !corrupt_backup.exists(),
            "validation must not read or rename the touched sidecar"
        );

        // application (dry_run = false) is where the sidecar is read, so a
        // corrupt one is renamed aside there -- confirming the load lives only
        // in the apply path, not validation.
        merge_spec_delta(&c, "my-cap", added_delta, "my-feature", today, false).unwrap();
        assert!(
            corrupt_backup.is_file(),
            "application should have read and backed up the corrupt sidecar"
        );
    }

    #[test]
    fn archive_accepts_asterisk_bullets_in_renamed() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            CANONICAL_SPEC,
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## RENAMED Requirements\n\
             * FROM: `### Requirement: First`\n\
             * TO: `### Requirement: Primero`\n",
        );

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();
        assert_eq!(outcome.specs_applied[0].renamed, 1);
        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap").join("spec.md")).unwrap();
        assert!(spec.contains("### Requirement: Primero"));
        assert!(!spec.contains("### Requirement: First"));
    }

    #[test]
    fn archive_matches_collapsed_whitespace_for_removed_and_renamed() {
        // REMOVED with a whitespace-noisy header.
        {
            let tmp = TempDir::new();
            let c = cfg(&tmp);
            write(
                &c.specs_dir().join("my-cap").join("spec.md"),
                CANONICAL_SPEC,
            );
            change::create(&c, "my-feature").unwrap();
            write(
                &c.changes_dir()
                    .join("my-feature")
                    .join("specs")
                    .join("my-cap")
                    .join("spec.md"),
                "## REMOVED Requirements\n\n### Requirement:   Second  \n\ngone\n",
            );
            archive(&c, "my-feature", false, false, false).unwrap();
            let spec =
                std::fs::read_to_string(c.specs_dir().join("my-cap").join("spec.md")).unwrap();
            assert!(!spec.contains("### Requirement: Second"));
        }
        // RENAMED whose FROM header has collapsed whitespace.
        {
            let tmp = TempDir::new();
            let c = cfg(&tmp);
            write(
                &c.specs_dir().join("my-cap").join("spec.md"),
                CANONICAL_SPEC,
            );
            change::create(&c, "my-feature").unwrap();
            write(
                &c.changes_dir()
                    .join("my-feature")
                    .join("specs")
                    .join("my-cap")
                    .join("spec.md"),
                "## RENAMED Requirements\n\
                 - FROM: `### Requirement:   Second  `\n\
                 - TO: `### Requirement: Segundo`\n",
            );
            archive(&c, "my-feature", false, false, false).unwrap();
            let spec =
                std::fs::read_to_string(c.specs_dir().join("my-cap").join("spec.md")).unwrap();
            assert!(spec.contains("### Requirement: Segundo"));
            assert!(!spec.contains("### Requirement: Second"));
        }
    }

    #[test]
    fn archive_errors_on_malformed_renamed_sections() {
        for (delta, needle) in [
            (
                "## RENAMED Requirements\n- FROM: `### Requirement: First`\n- FROM: `### Requirement: Second`\n",
                "FROM without following TO",
            ),
            (
                "## RENAMED Requirements\n- TO: `### Requirement: New`\n",
                "TO without preceding FROM",
            ),
            (
                "## RENAMED Requirements\n- FROM: ### Requirement: First\n- TO: `### Requirement: New`\n",
                "missing a backticked requirement header",
            ),
            (
                "## RENAMED Requirements\n- FROM: `### Requirement: First\n- TO: `### Requirement: New`\n",
                "missing a closing backtick",
            ),
            (
                "## RENAMED Requirements\n- FROM: `not a requirement header`\n- TO: `### Requirement: New`\n",
                "must be a `### Requirement: <name>` header",
            ),
        ] {
            let tmp = TempDir::new();
            let c = cfg(&tmp);
            write(&c.specs_dir().join("my-cap").join("spec.md"), CANONICAL_SPEC);
            change::create(&c, "my-feature").unwrap();
            write(
                &c.changes_dir()
                    .join("my-feature")
                    .join("specs")
                    .join("my-cap")
                    .join("spec.md"),
                delta,
            );

            let err = archive(&c, "my-feature", false, false, false).unwrap_err();
            assert!(
                err.to_string().contains(needle),
                "malformed RENAMED should error with '{needle}', got: {err}"
            );
            assert!(c.changes_dir().join("my-feature").is_dir());
            assert!(!c.changes_dir().join("archive").exists());
        }
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

        assert!(archive(&c, "my-feature", false, false, false).is_err());

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

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

        assert_eq!(outcome.specs_applied.len(), 1);
        assert_eq!(outcome.specs_applied[0].added, 0);
        assert!(!c.specs_dir().join("my-cap").join("spec.md").exists());
    }

    #[test]
    fn archive_clears_sidecar_state_on_success() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        change::mark_in_progress(&c, "my-feature").unwrap();

        archive(&c, "my-feature", true, false, false).unwrap();

        assert!(!c
            .root
            .join(".spectra")
            .join("changes")
            .join("my-feature.in-progress")
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

        archive(&c, "my-feature", true, false, false).unwrap();

        assert!(!touched::touched_path(&c, "my-feature").exists());
    }

    /// After chmod(0o000), root (or a container with CAP_DAC_OVERRIDE) can
    /// still read the file, so the permission-denied scenario these tests
    /// need is unconstructible; skip rather than fail in that case.
    ///
    /// Kept in sync with the identical helper in `touched.rs`'s test module.
    #[cfg(unix)]
    fn permission_denied_is_constructible(path: &std::path::Path) -> bool {
        std::fs::read(path).is_err()
    }

    #[cfg(unix)]
    #[test]
    fn archive_fails_loudly_on_an_unreadable_spec_delta_instead_of_silently_dropping_it() {
        use std::os::unix::fs::PermissionsExt;

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

        if !permission_denied_is_constructible(&spec_path) {
            eprintln!(
                "skipping archive_fails_loudly_on_an_unreadable_spec_delta_instead_of_silently_dropping_it: \
                 running as root (chmod 0o000 not enforced)"
            );
            std::fs::set_permissions(&spec_path, std::fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }

        let result = archive(&c, "my-feature", false, false, false);
        std::fs::set_permissions(&spec_path, std::fs::Permissions::from_mode(0o644)).unwrap();

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

        let err = archive(&c, "my-feature", false, false, false).unwrap_err();

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

        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        let meta_path = c.changes_dir().join("my-feature").join(".openspec.yaml");
        std::fs::set_permissions(&meta_path, std::fs::Permissions::from_mode(0o000)).unwrap();

        if !permission_denied_is_constructible(&meta_path) {
            eprintln!(
                "skipping archive_preserves_the_underlying_error_cause_after_a_post_rename_failure: \
                 running as root (chmod 0o000 not enforced)"
            );
            std::fs::set_permissions(&meta_path, std::fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }

        let err = archive(&c, "my-feature", true, false, false).unwrap_err();
        std::fs::set_permissions(&meta_path, std::fs::Permissions::from_mode(0o644)).unwrap();

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
