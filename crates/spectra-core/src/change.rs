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

pub const NO_ACTIVE_CHANGES_MESSAGE: &str =
    "No active changes. Create one with: spectra new change <name>";

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_specs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retire_capabilities: Option<bool>,
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

/// Remove any `.in-progress`/`.started`/`.touched-baseline.json`/
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
        // 舊版 OpenSpectra（#98）的 per-change baseline，已不再寫入，只負責清掉殘留。
        cfg.root
            .join(".spectra")
            .join("changes")
            .join(format!("{name}.touched-baseline.json")),
    ];
    let mut failures = Vec::new();
    match std::fs::remove_dir_all(crate::touched::baselines_dir(cfg, name)) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => failures.push(format!("removing stale task baselines for '{name}': {e}")),
    }
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
    create_with(cfg, name, CreateOptions::default())
}

/// `new change` 的 `--schema`／`--agent`（oracle 3.0.0）。兩者都**不驗證**，原樣寫進
/// `.openspec.yaml`：oracle 對 `--schema bogus` 寫 `schema: bogus`、對 `--agent nope`
/// 寫 `created_with: nope`，都 exit 0（已 probe）。`--description` 在 oracle 被接受但
/// 不寫入任何地方，所以這裡沒有對應欄位。
#[derive(Debug, Clone, Copy, Default)]
pub struct CreateOptions<'a> {
    pub schema: Option<&'a str>,
    pub agent: Option<&'a str>,
}

pub fn create_with(cfg: &Config, name: &str, options: CreateOptions<'_>) -> Result<Change> {
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
    match create_inner(cfg, name, &dir, options) {
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
            // `.started` is create_inner's last fallible write (the touched
            // baseline written after it is best-effort and never fails the
            // create), so a failure there can leave a (possibly partial)
            // `.started` SHA file with no change directory behind it; clear it
            // along with the change dir.
            if let Err(cleanup_err) = clear_stale_sidecar_state(cfg, name) {
                eprintln!(
                    "warning: failed to remove partial sidecar state for '{name}' after create error: {cleanup_err}"
                );
            }
            Err(e)
        }
    }
}

fn create_inner(
    cfg: &Config,
    name: &str,
    dir: &std::path::Path,
    options: CreateOptions<'_>,
) -> Result<()> {
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
            options
                .schema
                .map(str::to_string)
                .or_else(|| crate::schema::configured_schema_name(cfg))
                .unwrap_or_else(|| crate::schema::SCHEMA_NAME.to_string()),
        ),
        created: Some(today.to_string()),
        created_by: Some(created_by),
        created_with: options.agent.map(str::to_string),
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

    // touched-file 的 baseline 改由 `task start` 逐 task 擷取（oracle 3.0.0，D7），
    // 建立 change 時不再寫 per-change baseline（#98 已移除）。
    Ok(())
}

/// Change directory names under `changes_dir()` that pass the archive/hidden
/// filters, unsorted.
fn walk_change_names(cfg: &Config) -> Vec<String> {
    walk_names_in(&cfg.changes_dir())
}

/// The directory-name filter behind `list_active` and `list_active_sorted`
/// (`list_parked` reads its own store with a different filter).
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
        // oracle 3.0.0 列出 `archive` 以外的每個目錄，包括含大寫或底線的名稱
        // （W7 probe），日期開頭的名稱（`2026-05-05-foo`）也是使用中 change（#219 probe）；
        // 隱藏目錄（archive 交易用的 `.spectra-archive-*.staged`）不算。
        if name == "archive" || name.starts_with('.') {
            continue;
        }
        names.push(name);
    }
    names
}

/// `list` 的 `summary`（oracle 3.0.0，見 `docs/reverse-engineering/list-show.md`）：
/// 只看 `proposal.md`。原始行（不 trim）以 `## Why`、`## Problem` 或 `## Summary`
/// 開頭（大小寫敏感的前綴比對，不理會 code fence）即開始一個區段，區段到下一個
/// `## ` 開頭的行為止；取區段內第一個 trim 後非空、且不以 `<!--` 開頭的行。
/// 該區段取不到就找下一個符合的標題。超過 30 個字元時截成前 30 個字元加 `…`。
pub fn summary(ch: &Change) -> Option<String> {
    let text = std::fs::read_to_string(ch.proposal_md()).ok()?;
    let mut in_section = false;
    for line in text.lines() {
        if ["## Why", "## Problem", "## Summary"]
            .iter()
            .any(|h| line.starts_with(h))
        {
            in_section = true;
            continue;
        }
        if line.starts_with("## ") {
            in_section = false;
            continue;
        }
        if !in_section {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") {
            continue;
        }
        let chars: Vec<char> = trimmed.chars().collect();
        return Some(if chars.len() > 30 {
            format!("{}…", chars[..30].iter().collect::<String>())
        } else {
            trimmed.to_string()
        });
    }
    None
}

/// 變更清單的排序欄位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Modified,
    Created,
}

/// change 目錄內所有一般檔案（遞迴）的最新 mtime，以整秒計；目錄本身的 mtime
/// 不算（oracle 3.0.0 的 `modified` 排序）。沒有檔案時為 `None`（排最後）。
pub(crate) fn latest_file_mtime(dir: &Path) -> Option<u64> {
    let mut latest: Option<u64> = None;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(entry.path());
            } else if let Some(secs) = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
            {
                latest = Some(latest.map_or(secs, |l| l.max(secs)));
            }
        }
    }
    latest
}

/// `.openspec.yaml` 的原始 `created` 字串（`created` 排序以字串比較，不解析日期）。
/// 與 `show` 相同，metadata 必須同時有 `schema` 與 `created` 才算解析成功；只有其中
/// 一個時視為沒有 metadata（排最後）。
fn raw_created(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(".openspec.yaml")).ok()?;
    let value: serde_yaml::Value = serde_yaml::from_str(&text).ok()?;
    value.get("schema")?;
    match value.get("created")? {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// oracle 3.0.0 的排序：`modified` 依最新檔案 mtime（整秒）遞減；`created` 依原始
/// `created` 字串遞減、沒有的排最後，同值再依 modified；`name` 依位元組遞增。
/// oracle 的同值順序是檔案系統的 readdir 順序（不可攜），OpenSpectra 改以名稱排序
/// （刻意分歧，同 `instructions` 的 `contextFiles`）。
fn sort_names_by_metadata(names: &mut [String], root: &Path, sort_key: SortKey) {
    match sort_key {
        SortKey::Name => names.sort(),
        SortKey::Modified => {
            names.sort_by_cached_key(|name| {
                (
                    std::cmp::Reverse(latest_file_mtime(&root.join(name))),
                    name.clone(),
                )
            });
        }
        SortKey::Created => {
            names.sort_by_cached_key(|name| {
                let dir = root.join(name);
                let created = raw_created(&dir);
                (
                    created.is_none(),
                    std::cmp::Reverse(created),
                    std::cmp::Reverse(latest_file_mtime(&dir)),
                    name.clone(),
                )
            });
        }
    }
}

/// List active (non-archived) change names, sorted. Parked changes are not
/// under `changes_dir()` at all, so no extra filtering is needed.
pub fn list_active(cfg: &Config) -> Vec<String> {
    let mut names = walk_change_names(cfg);
    names.sort();
    names
}

/// 依指定欄位列出使用中的變更。
pub fn list_active_sorted(cfg: &Config, sort_key: SortKey) -> Vec<String> {
    let mut names = walk_change_names(cfg);
    sort_names_by_metadata(&mut names, &cfg.changes_dir(), sort_key);
    names
}

/// List parked change names — the directories the oracle moved into
/// `<git common dir>/spectra-app/changes/` — sorted.
///
/// Like [`list_active`] it keeps date-prefixed names: the oracle parks and
/// lists `2026-01-01-old` like any other name (probed).
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

/// 依指定欄位列出暫停中的變更。
pub fn list_parked_sorted(cfg: &Config, sort_key: SortKey) -> Vec<String> {
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
    sort_names_by_metadata(&mut names, &dir, sort_key);
    names
}

/// Resolve a change name without treating an empty active-change set as an
/// error. Read/report commands use `None` as their normal empty state; commands
/// that require a change should continue to call [`resolve`].
pub fn resolve_optional(cfg: &Config, explicit: Option<&str>) -> Result<Option<String>> {
    if let Some(name) = explicit {
        return Ok(Some(name.to_string()));
    }
    let active = list_active(cfg);
    match active.len() {
        1 => Ok(active.into_iter().next()),
        0 => Ok(None),
        _ => Err(anyhow!(
            "Multiple changes found. Use a change name to specify one: {}",
            active.join(", ")
        )),
    }
}

/// Resolve a change name: use `explicit` if given, else auto-select when
/// exactly one active change exists (mirrors `spectra drift`'s auto-detect).
pub fn resolve(cfg: &Config, explicit: Option<&str>) -> Result<String> {
    resolve_optional(cfg, explicit)?.ok_or_else(|| anyhow!(NO_ACTIVE_CHANGES_MESSAGE))
}

/// `analyze` 的 change 解析：多個 active change 時用 oracle 3.0.0 `analyze` 自己的
/// 措辭 `Specify one:`（`status`／`instructions` 是 `Use --change to specify one:`，
/// 見 W10 探測 p30）。沒有 active change 時回 `None`，由呼叫端印出提示並成功結束。
pub fn resolve_for_analyze(cfg: &Config, explicit: Option<&str>) -> Result<Option<String>> {
    if explicit.is_none() {
        let active = list_active(cfg);
        if active.len() > 1 {
            anyhow::bail!("Multiple changes found. Specify one: {}", active.join(", "));
        }
    }
    resolve_optional(cfg, explicit)
}

/// `task start`／`task done` 的 change 解析：多個 active change 時用 oracle 3.0.0 的
/// 措辭 `Use --change to specify one:`（其他指令的共用措辭待 #50 裁決）。
pub fn resolve_for_task(cfg: &Config, explicit: Option<&str>) -> Result<String> {
    if explicit.is_none() {
        let active = list_active(cfg);
        if active.len() > 1 {
            anyhow::bail!(
                "Multiple changes found. Use --change to specify one: {}",
                active.join(", ")
            );
        }
    }
    resolve(cfg, explicit)
}

/// `task done` 的結果（oracle 3.0.0 `--json` 的欄位）。
#[derive(Debug)]
pub struct TaskDoneOutcome {
    pub change: String,
    pub task_id: String,
    pub task_desc: String,
    /// `task_baseline`／`explicit_files`；沒有記錄任何 touched 時為 `None`。
    pub provenance: Option<String>,
    pub touched_files: Vec<String>,
    pub warnings: Vec<String>,
}

/// `task start` 的結果（oracle 3.0.0 `--json` 的欄位）。
#[derive(Debug)]
pub struct TaskStartOutcome {
    pub change: String,
    pub task_id: String,
    pub baseline_created: bool,
    pub git_tracking_available: bool,
    pub warnings: Vec<String>,
}

pub const WARNING_NO_BASELINE: &str = "touched_tracking_skipped_no_baseline_or_explicit_files";
pub const WARNING_GIT_UNAVAILABLE: &str = "git_tracking_unavailable";

/// 讀 tasks.md 並把 task ID 參數對應到序號；錯誤訊息與 oracle 3.0.0 相同。
fn read_tasks_for(cfg: &Config, name: &str, task_arg: &str) -> Result<(PathBuf, String, usize)> {
    let not_found = || anyhow!("tasks.md not found for change '{name}'");
    let ch = try_load(cfg, name)?.ok_or_else(not_found)?;
    let tasks_path = ch.tasks_md();
    let md = match std::fs::read_to_string(&tasks_path) {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(not_found()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", tasks_path.display())),
    };
    let task_id = crate::tasks::resolve_task_id(&md, task_arg)
        .ok_or_else(|| anyhow!("Task {task_arg} not found for change '{name}'"))?;
    Ok((tasks_path, md, task_id))
}

/// `--file` 參數正規化成相對於專案 root 的路徑（oracle 3.0.0 規則，見 task.md）：
/// 以字面方式處理 `.`／`..`／重複斜線與 root 內的絕對路徑，不檢查存在與否。
pub(crate) fn normalize_explicit_path(cfg: &Config, arg: &str) -> Result<String> {
    use std::path::Component;
    let outside = || anyhow!("Explicit path '{arg}' is outside the project workspace");
    let raw = Path::new(arg);
    let joined = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cfg.root.join(raw)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(outside());
                }
            }
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    let relative = normalized.strip_prefix(&cfg.root).map_err(|_| outside())?;
    if relative.as_os_str().is_empty() {
        return Err(outside());
    }
    // 經由 symlink 逃出專案：以最長的既存祖先做 canonicalize 後再比對。
    if let Ok(root) = cfg.root.canonicalize() {
        let mut probe = normalized.as_path();
        while !probe.exists() {
            match probe.parent() {
                Some(parent) => probe = parent,
                None => break,
            }
        }
        if let Ok(real) = probe.canonicalize() {
            if !real.starts_with(&root) {
                anyhow::bail!("Explicit path '{arg}' resolves outside the project workspace");
            }
        }
    }
    let relative = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    let under = |prefix: &str| {
        let prefix = prefix.trim_end_matches('/');
        relative == prefix || relative.starts_with(&format!("{prefix}/"))
    };
    if under(cfg.spec_dir.trim_start_matches("./")) {
        anyhow::bail!("Explicit path '{arg}' is inside the configured spec directory");
    }
    if under(".spectra") {
        anyhow::bail!("Explicit path '{arg}' is inside Spectra tracking metadata");
    }
    Ok(relative)
}

/// `task start`：為一個 task 擷取 baseline 指紋，不修改 tasks.md（oracle 3.0.0）。
///
/// - baseline 已存在時什麼都不寫（`baseline_created: false`）。
/// - touched 檔不存在時才建立，並記下 `review_base`；已存在就不動它。
/// - 不在 git repo 內時不寫 baseline，回報 `git_tracking_unavailable`。
/// - 成功才建立 lock（找不到 change 或 task 時不建立，與 `task done` 不同）。
pub fn start_task(cfg: &Config, name: &str, task_arg: &str) -> Result<TaskStartOutcome> {
    let (_, _, _) = read_tasks_for(cfg, name, task_arg)?;
    let tracking = crate::touched::load_strict(cfg, name)?;
    crate::touched::touch_lock(cfg, name)?;
    let mut outcome = TaskStartOutcome {
        change: name.to_string(),
        task_id: task_arg.to_string(),
        baseline_created: false,
        git_tracking_available: true,
        warnings: Vec::new(),
    };
    if crate::touched::baseline_exists(cfg, name, task_arg) {
        return Ok(outcome);
    }
    let Some(fingerprints) = crate::fingerprint::dirty_fingerprints(&cfg.root, &cfg.spec_dir)
    else {
        outcome.git_tracking_available = false;
        outcome.warnings.push(WARNING_GIT_UNAVAILABLE.to_string());
        return Ok(outcome);
    };
    crate::touched::write_baseline(
        cfg,
        &crate::touched::TaskBaseline {
            change: name.to_string(),
            task_id: task_arg.to_string(),
            fingerprints: fingerprints.clone(),
        },
    )?;
    if tracking.is_none() {
        crate::touched::persist(
            cfg,
            &crate::touched::TouchedTracking {
                change: name.to_string(),
                touched: Vec::new(),
                review_base: Some(crate::touched::ReviewBase {
                    head_revision: crate::fingerprint::head_revision(&cfg.root),
                    dirty_fingerprints: fingerprints,
                }),
            },
        )?;
    }
    outcome.baseline_created = true;
    Ok(outcome)
}

/// `task done`：把 task 標為完成並記錄 touched files（oracle 3.0.0）。
///
/// 檢查順序：解析 change → task ID → 是否已完成 → `--file` → tracking → 寫 tasks.md。
/// 只要 change 名稱已解析就建立 lock（即使之後失敗）。tracking：
/// - 有 `--file`：記錄這些路徑（`explicit_files`），並刪除這個 task 的 baseline；
/// - 否則有 baseline：記錄自 baseline 以來變動的路徑（`task_baseline`），空的就不新增條目；
/// - 兩者皆無：不記錄，警告 `touched_tracking_skipped_no_baseline_or_explicit_files`；
/// - 不在 git repo 內：`--file` 也忽略，警告 `git_tracking_unavailable`。
///
/// tracking 先寫，tasks.md 寫入失敗時還原 tracking 並保留 baseline。
pub fn mark_task_done(
    cfg: &Config,
    name: &str,
    task_arg: &str,
    explicit_files: &[String],
) -> Result<TaskDoneOutcome> {
    crate::touched::touch_lock(cfg, name)?;
    let (tasks_path, md, task_id) = read_tasks_for(cfg, name, task_arg)?;
    let (new_md, task_desc) = crate::tasks::mark_done(&md, task_id)?;
    let mut files = explicit_files
        .iter()
        .map(|arg| normalize_explicit_path(cfg, arg))
        .collect::<Result<Vec<_>>>()?;
    files.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    files.dedup();

    let tracking = crate::touched::load_strict(cfg, name)?;
    let baseline = crate::touched::load_baseline(cfg, name, task_arg)?;
    let mut outcome = TaskDoneOutcome {
        change: name.to_string(),
        task_id: task_arg.to_string(),
        task_desc: task_desc.clone(),
        provenance: None,
        touched_files: Vec::new(),
        warnings: Vec::new(),
    };

    let now = crate::fingerprint::dirty_fingerprints(&cfg.root, &cfg.spec_dir);
    let recorded: Option<(Vec<String>, &str)> = match (&now, baseline.as_ref()) {
        (None, _) => {
            outcome.warnings.push(WARNING_GIT_UNAVAILABLE.to_string());
            None
        }
        (Some(_), _) if !files.is_empty() => {
            Some((files, crate::touched::PROVENANCE_EXPLICIT_FILES))
        }
        (Some(now), Some(baseline)) => Some((
            crate::fingerprint::changed_paths(&baseline.fingerprints, now),
            crate::touched::PROVENANCE_TASK_BASELINE,
        )),
        (Some(_), None) => {
            outcome.warnings.push(WARNING_NO_BASELINE.to_string());
            None
        }
    };

    let touched_path = crate::touched::touched_path(cfg, name);
    let original_touched = std::fs::read(&touched_path).ok();
    let mut wrote_touched = false;
    if let Some((paths, provenance)) = recorded {
        outcome.provenance = Some(provenance.to_string());
        outcome.touched_files = paths.clone();
        if !paths.is_empty() {
            let mut tracking = tracking.unwrap_or_else(|| crate::touched::TouchedTracking {
                change: name.to_string(),
                ..Default::default()
            });
            tracking.touched.push(crate::touched::TouchedEntry {
                task_id: task_arg.to_string(),
                task_desc,
                files: paths,
                provenance: Some(provenance.to_string()),
            });
            crate::touched::persist(cfg, &tracking)?;
            wrote_touched = true;
        }
    }

    if let Err(e) = std::fs::write(&tasks_path, &new_md) {
        if wrote_touched {
            let restored = match &original_touched {
                Some(bytes) => std::fs::write(&touched_path, bytes),
                None => std::fs::remove_file(&touched_path),
            };
            if let Err(rollback) = restored {
                anyhow::bail!(
                    "Failed to write tasks.md: {e}; failed to rollback touched tracking: {rollback}"
                );
            }
        }
        anyhow::bail!("Failed to write tasks.md: {e}");
    }
    // baseline 在 tasks.md 寫入成功後才刪除；git 不可用時同樣刪除（oracle 實測）。
    if baseline.is_some() || crate::touched::baseline_exists(cfg, name, task_arg) {
        crate::touched::remove_baseline(cfg, name, task_arg)?;
    }
    Ok(outcome)
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
    fn resolve_optional_exposes_the_empty_state_without_changing_required_resolution() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };

        assert_eq!(resolve_optional(&cfg, None).unwrap(), None);
        assert_eq!(
            resolve(&cfg, None).unwrap_err().to_string(),
            NO_ACTIVE_CHANGES_MESSAGE
        );
        assert_eq!(
            resolve_optional(&cfg, Some("explicit-change")).unwrap(),
            Some("explicit-change".to_string())
        );

        write(
            &cfg.changes_dir().join("one-change").join("proposal.md"),
            "# One\n",
        );
        assert_eq!(
            resolve_optional(&cfg, None).unwrap(),
            Some("one-change".to_string())
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
    fn list_active_includes_date_prefixed_changes_but_not_archive_or_hidden_dirs() {
        // oracle 3.0.0 把 `changes/2026-05-05-dated/` 當使用中 change 列出（#219 探測）；
        // 只有 `archive/` 與隱藏目錄（archive 交易的暫存目錄）不算。
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        let changes = cfg.changes_dir();
        for dir in [
            "2026-05-05-dated",
            "plain-change",
            "archive/2026-01-01-old",
            ".spectra-archive-x.staged",
        ] {
            write(&changes.join(dir).join("proposal.md"), "# P\n");
        }

        assert_eq!(
            list_active(&cfg),
            vec!["2026-05-05-dated".to_string(), "plain-change".to_string()]
        );
        assert_eq!(
            list_active_sorted(&cfg, SortKey::Name),
            vec!["2026-05-05-dated".to_string(), "plain-change".to_string()]
        );
    }

    #[test]
    fn resolve_auto_selects_a_lone_date_prefixed_active_change() {
        let tmp = TempDir::new();
        let cfg = git_repo_cfg(&tmp);
        write(
            &cfg.changes_dir()
                .join("2026-05-05-dated")
                .join("proposal.md"),
            "# P\n",
        );
        write(
            &cfg.changes_dir()
                .join("archive/2026-01-01-old")
                .join("proposal.md"),
            "# P\n",
        );

        assert_eq!(resolve(&cfg, None).unwrap(), "2026-05-05-dated");
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
        // The oracle parks and lists them, as it does for active changes (#219).
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

    /// Issue #118 的 worktree 半邊：parked store 必須落在**共用**的 git dir。
    /// 若 `common_dir` 誤用 `--git-dir`，從 linked worktree park 會寫進
    /// `.git/worktrees/<name>/spectra-app/`，main checkout 就看不到它。
    #[test]
    fn parking_from_a_linked_worktree_lands_in_the_shared_store() {
        let tmp = TempDir::new();
        let main_cfg = git_repo_cfg(&tmp);
        let wt_parent = TempDir::new();
        let wt = wt_parent.join("wt");
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&*tmp)
            .args(["worktree", "add", "-q", "--detach"])
            .arg(&wt)
            .output()
            .unwrap()
            .status
            .success());
        let wt_cfg = Config {
            root: wt.clone(),
            ..main_cfg.clone()
        };
        write(
            &wt_cfg.changes_dir().join("from-wt").join("proposal.md"),
            "# From worktree\n",
        );

        park(&wt_cfg, "from-wt").unwrap();

        let shared = tmp
            .canonicalize()
            .unwrap()
            .join(".git")
            .join("spectra-app")
            .join("changes")
            .join("from-wt")
            .join("proposal.md");
        assert_eq!(
            std::fs::read_to_string(&shared).unwrap(),
            "# From worktree\n"
        );
        assert_eq!(list_parked(&main_cfg), vec!["from-wt".to_string()]);
        assert_eq!(list_parked(&wt_cfg), vec!["from-wt".to_string()]);
    }

    #[test]
    fn list_sorting_follows_the_oracle_rules() {
        // oracle 3.0.0：modified 取最新「檔案」mtime（整秒、目錄 mtime 不算）遞減；
        // created 依原始字串遞減、沒有 metadata 的排最後；同值依名稱（刻意分歧）。
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        let set_mtime = |path: &std::path::Path, secs: u64| {
            let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
            std::fs::File::options()
                .append(true)
                .open(path)
                .unwrap()
                .set_modified(t)
                .unwrap();
        };
        let dir = cfg.changes_dir();
        write(
            &dir.join("old/.openspec.yaml"),
            "schema: spec-driven\ncreated: 2026-09-01\n",
        );
        write(
            &dir.join("new/.openspec.yaml"),
            "schema: spec-driven\ncreated: 2026-02-11\n",
        );
        write(&dir.join("new/deep/er/f.bin"), "x");
        write(&dir.join("bare/notes.txt"), "x");
        write(
            &dir.join("Upper-Case/.openspec.yaml"),
            "created: notadate\n",
        );
        set_mtime(&dir.join("old/.openspec.yaml"), 1_000);
        set_mtime(&dir.join("new/.openspec.yaml"), 1_000);
        set_mtime(&dir.join("new/deep/er/f.bin"), 3_000);
        set_mtime(&dir.join("bare/notes.txt"), 2_000);
        set_mtime(&dir.join("Upper-Case/.openspec.yaml"), 1_000);

        assert_eq!(
            list_active_sorted(&cfg, SortKey::Modified),
            vec!["new", "bare", "Upper-Case", "old"],
            "latest nested file wins; equal seconds fall back to name"
        );
        assert_eq!(
            list_active_sorted(&cfg, SortKey::Created),
            vec!["old", "new", "bare", "Upper-Case"],
            "raw created strings descending; metadata without `schema` counts as none \
             and sorts last, where ties fall back to modified"
        );
        assert_eq!(
            list_active_sorted(&cfg, SortKey::Name),
            vec!["Upper-Case", "bare", "new", "old"]
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
    fn create_with_writes_schema_and_agent_unvalidated_like_the_oracle() {
        let tmp = TempDir::new();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        };
        // oracle 3.0.0：--schema 與 --agent 都不驗證，原樣寫入；created_with 排在 created_by 之後。
        let ch = create_with(
            &cfg,
            "demo",
            CreateOptions {
                schema: Some("bogus"),
                agent: Some("nope"),
            },
        )
        .unwrap();
        let created = ch.metadata.created.as_deref().unwrap();
        let created_by = ch.metadata.created_by.as_deref().unwrap();
        assert_eq!(
            std::fs::read_to_string(ch.dir.join(".openspec.yaml")).unwrap(),
            format!(
                "schema: bogus\ncreated: {created}\ncreated_by: {created_by}\ncreated_with: nope\n"
            )
        );
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

        let outcome = mark_task_done(&cfg, "add-search-filter", "2", &[]).unwrap();

        assert_eq!(outcome.change, "add-search-filter");
        assert_eq!(outcome.task_id, "2");
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

        let err = mark_task_done(&cfg, "does-not-exist", "1", &[]).unwrap_err();
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

        let err = mark_task_done(&cfg, "add-search-filter", "1", &[]).unwrap_err();
        assert_eq!(
            err.to_string(),
            "tasks.md not found for change 'add-search-filter'"
        );
    }

    fn git_in(tmp: &TempDir, args: &[&str]) {
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&**tmp)
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }

    /// 已 commit 的專案：change `demo`、兩個 task，以及 `src/a.rs`、`src/old.rs`。
    fn tracked_project(tmp: &TempDir) -> Config {
        let cfg = git_repo_cfg(tmp);
        write(&tmp.join(".gitignore"), ".spectra/\n");
        write(&tmp.join("src/a.rs"), "fn a() {}\n");
        write(&tmp.join("src/old.rs"), "fn old() {}\n");
        write(
            &cfg.changes_dir().join("demo/.openspec.yaml"),
            "schema: spec-driven\n",
        );
        write(
            &cfg.changes_dir().join("demo/tasks.md"),
            "- [ ] 1.1 first\n- [ ] 1.2 second\n",
        );
        git_in(tmp, &["add", "-A"]);
        git_in(tmp, &["commit", "-q", "-m", "base"]);
        cfg
    }

    #[test]
    fn task_done_after_task_start_records_only_paths_changed_since_the_baseline() {
        let tmp = TempDir::new();
        let cfg = tracked_project(&tmp);
        write(&tmp.join("src/old.rs"), "fn old() { /* dirty before */ }\n");

        let started = start_task(&cfg, "demo", "1").unwrap();
        assert!(started.baseline_created && started.git_tracking_available);
        let again = start_task(&cfg, "demo", "1").unwrap();
        assert!(!again.baseline_created, "an existing baseline is preserved");
        assert!(touched::lock_path(&cfg, "demo").is_file());

        write(&tmp.join("src/a.rs"), "fn a() { 1 }\n");
        write(&tmp.join("src/new.rs"), "fn new() {}\n");
        // spec 目錄內的變動不算。
        write(&cfg.changes_dir().join("demo/design.md"), "d\n");

        let done = mark_task_done(&cfg, "demo", "1", &[]).unwrap();
        assert_eq!(done.provenance.as_deref(), Some("task_baseline"));
        assert_eq!(done.touched_files, vec!["src/a.rs", "src/new.rs"]);
        assert!(done.warnings.is_empty());
        assert!(!touched::baseline_path(&cfg, "demo", "1").exists());

        let tracking = touched::load_strict(&cfg, "demo").unwrap().unwrap();
        let review = tracking
            .review_base
            .expect("created by the first task start");
        assert_eq!(
            review
                .dirty_fingerprints
                .iter()
                .map(|f| f.path.as_str())
                .collect::<Vec<_>>(),
            vec![".gitignore", "src/old.rs"]
                .into_iter()
                .filter(|p| *p != ".gitignore")
                .collect::<Vec<_>>(),
            "only the pre-dirty file outside the spec dir"
        );
        assert_eq!(tracking.touched.len(), 1);
        assert_eq!(tracking.touched[0].task_id, "1");
    }

    #[test]
    fn task_done_without_a_baseline_warns_and_records_nothing() {
        let tmp = TempDir::new();
        let cfg = tracked_project(&tmp);
        write(&tmp.join("src/a.rs"), "fn a() { 1 }\n");

        let done = mark_task_done(&cfg, "demo", "2", &[]).unwrap();
        assert_eq!(done.task_desc, "1.2 second");
        assert_eq!(done.provenance, None);
        assert!(done.touched_files.is_empty());
        assert_eq!(done.warnings, vec![WARNING_NO_BASELINE]);
        assert!(!touched::touched_path(&cfg, "demo").exists());
        assert!(touched::lock_path(&cfg, "demo").is_file());
    }

    #[test]
    fn task_done_with_explicit_files_records_them_normalized() {
        let tmp = TempDir::new();
        let cfg = tracked_project(&tmp);
        let files = [
            "./src//b.rs".to_string(),
            "src/../src/a.rs".to_string(),
            "src/b.rs".to_string(),
        ];
        let done = mark_task_done(&cfg, "demo", "1", &files).unwrap();
        assert_eq!(done.provenance.as_deref(), Some("explicit_files"));
        assert_eq!(done.touched_files, vec!["src/a.rs", "src/b.rs"]);
    }

    #[test]
    fn explicit_paths_are_rejected_with_the_oracle_messages() {
        let tmp = TempDir::new();
        let cfg = tracked_project(&tmp);
        let err = |arg: &str| normalize_explicit_path(&cfg, arg).unwrap_err().to_string();
        assert_eq!(
            err("../outside.rs"),
            "Explicit path '../outside.rs' is outside the project workspace"
        );
        assert_eq!(
            err("."),
            "Explicit path '.' is outside the project workspace"
        );
        assert_eq!(
            err("openspec/changes/demo/design.md"),
            "Explicit path 'openspec/changes/demo/design.md' is inside the configured spec directory"
        );
        assert_eq!(
            err(".spectra/x"),
            "Explicit path '.spectra/x' is inside Spectra tracking metadata"
        );
        assert_eq!(normalize_explicit_path(&cfg, "src/").unwrap(), "src");
        assert_eq!(
            normalize_explicit_path(&cfg, "no/such/f.rs").unwrap(),
            "no/such/f.rs"
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(std::env::temp_dir(), tmp.join("escape")).unwrap();
            assert_eq!(
                err("escape/x.rs"),
                "Explicit path 'escape/x.rs' resolves outside the project workspace"
            );
        }
        // 任何 --file 錯誤都不能改動 tasks.md。
        let before = std::fs::read_to_string(cfg.changes_dir().join("demo/tasks.md")).unwrap();
        assert!(mark_task_done(&cfg, "demo", "1", &["../x".to_string()]).is_err());
        assert_eq!(
            std::fs::read_to_string(cfg.changes_dir().join("demo/tasks.md")).unwrap(),
            before
        );
    }

    #[test]
    fn task_ids_must_match_the_decimal_index_exactly() {
        let tmp = TempDir::new();
        let cfg = tracked_project(&tmp);
        for arg in ["0", "01", "+1", "1.1", "3", "abc", " 1"] {
            assert_eq!(
                mark_task_done(&cfg, "demo", arg, &[])
                    .unwrap_err()
                    .to_string(),
                format!("Task {arg} not found for change 'demo'")
            );
            assert_eq!(
                start_task(&cfg, "demo", arg).unwrap_err().to_string(),
                format!("Task {arg} not found for change 'demo'")
            );
        }
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
