//! `spectra archive` — freeze a completed change under a hidden sibling,
//! validate and commit its canonical spec deltas, then move it to
//! `<spec_dir>/changes/archive/YYYY-MM-DD-<name>/` and stamp the archived
//! `.openspec.yaml` with `archived_by`/`archived_at`.
//!
//! Reverse-engineered against `/Applications/Spectra.app` v2.3.1 — see
//! `docs/reverse-engineering/archive.md` for the full write-up, including
//! the Phase 2 OpenSpec compatibility delta behavior and other documented
//! gaps (no snapshot/unarchive support).

use anyhow::{anyhow, Context, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::fsutil::read_optional;
use crate::{change, touched};

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

/// `archive --preview` 對單一 capability 的預估（oracle 3.0.0 的 JSON 欄位名稱與順序）。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PreviewSpecUpdate {
    pub capability: String,
    pub exists: bool,
    pub added: usize,
    pub modified: usize,
    pub removed: usize,
    pub renamed: usize,
    /// oracle 在所有已 probe 的情境（包括另一個 active change 修改同一 capability）
    /// 都輸出 `null`，觸發條件未知，這裡固定為 `None`。
    pub conflict_source: Option<String>,
}

/// `archive --preview` 的結果。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ArchivePreview {
    pub change_id: String,
    pub spec_updates: Vec<PreviewSpecUpdate>,
    pub incomplete_tasks: usize,
    /// 同 `conflict_source`：probe 過的情境一律是空陣列。
    pub warnings: Vec<String>,
    pub has_delta_specs: bool,
}

/// 不修改任何檔案，預估 archive 會套用什麼（oracle 3.0.0 probe，見 `archive.md`）。
///
/// - 各計數只算**能成功套用**的操作：MODIFIED／REMOVED 指向不存在的 requirement、
///   ADDED 已存在的 requirement 都不計，也不報錯（真正 archive 時才會失敗）。
/// - `--skip-specs`、`--mark-tasks-complete`、`--no-validate` 都不影響 preview；
///   只有 `.openspec.yaml` 的 `skip_specs: true` 會讓 spec_updates 變空。
/// - delta 檔沒有任何操作 section 時直接報錯，與 oracle 相同。
/// - capability 依名稱排序；oracle 用檔案系統的 readdir 順序（刻意分歧）。
pub fn preview(cfg: &Config, name: &str) -> Result<ArchivePreview> {
    let ch =
        change::try_load(cfg, name)?.ok_or_else(|| anyhow!("Change '{name}' does not exist"))?;
    let metadata = load_change_metadata(&ch.dir)?;
    let mut spec_updates = Vec::new();
    if metadata.skip_specs != Some(true) {
        for (capability, delta) in crate::fsutil::collect_delta_specs(&ch.dir.join("specs"))? {
            let doc = crate::markdown::parse_delta(&delta)
                .map_err(|e| anyhow!("Failed to parse delta spec: {e}"))?;
            if !(doc.added_present
                || doc.modified_present
                || doc.removed_present
                || doc.renamed_present)
            {
                anyhow::bail!(
                    "Failed to parse delta spec: Invalid format: Delta spec must contain at least one operation (ADDED, MODIFIED, REMOVED, or RENAMED)"
                );
            }
            let path = cfg.specs_dir().join(&capability).join("spec.md");
            let existing = read_optional(&path)?;
            spec_updates.push(count_applicable(capability, existing.as_deref(), &doc));
        }
    }
    let incomplete_tasks = read_optional(&ch.dir.join("tasks.md"))?
        .map(|md| crate::tasks::parse(&md).iter().filter(|t| !t.done).count())
        .unwrap_or(0);
    Ok(ArchivePreview {
        change_id: name.to_string(),
        has_delta_specs: !spec_updates.is_empty(),
        spec_updates,
        incomplete_tasks,
        warnings: Vec::new(),
    })
}

/// 依 archive 的套用順序（RENAMED → REMOVED → MODIFIED → ADDED）對照 canonical spec
/// 的 requirement 名稱，只計入能套用的操作。
fn count_applicable(
    capability: String,
    existing: Option<&str>,
    doc: &crate::markdown::DeltaDocument,
) -> PreviewSpecUpdate {
    use crate::markdown::normalize_name;
    let mut names: std::collections::HashSet<String> = existing
        .map(crate::markdown::parse_main_requirements)
        .unwrap_or_default()
        .iter()
        .map(|r| normalize_name(&r.name))
        .collect();
    let mut update = PreviewSpecUpdate {
        capability,
        exists: existing.is_some(),
        added: 0,
        modified: 0,
        removed: 0,
        renamed: 0,
        conflict_source: None,
    };
    for rename in &doc.renamed {
        if names.remove(&normalize_name(&rename.from)) {
            names.insert(normalize_name(&rename.to));
            update.renamed += 1;
        }
    }
    for removed in &doc.removed {
        if names.remove(&normalize_name(removed)) {
            update.removed += 1;
        }
    }
    for modified in &doc.modified {
        if names.contains(&normalize_name(&modified.name)) {
            update.modified += 1;
        }
    }
    for added in &doc.added {
        if names.insert(normalize_name(&added.name)) {
            update.added += 1;
        }
    }
    update
}

/// Archive `name`: first freeze its active directory under a hidden sibling,
/// then validate and prepare every canonical-spec mutation from those frozen
/// bytes. After verifying that the frozen tree has not changed, apply specs,
/// move the frozen directory to `<spec_dir>/changes/archive/<today>-<name>/`,
/// optionally mark tasks complete, and stamp `.openspec.yaml`.
///
/// Metadata, tasks, and canonical specs are rolled back only while their
/// current bytes exactly match this transaction's expected output. An occupied
/// active path or concurrent edit is never overwritten. A verified
/// cross-device archive copy is authoritative even when its hidden staged
/// source cannot be removed; the retained source is reported for recovery.
pub fn archive(
    cfg: &Config,
    name: &str,
    skip_specs: bool,
    no_validate: bool,
    mark_tasks_complete: bool,
) -> Result<ArchiveOutcome> {
    let ch = change::try_load(cfg, name)?.ok_or_else(|| anyhow!("Change '{name}' not found."))?;
    let today = chrono::Local::now().date_naive();
    let archived_name = format!("{today}-{name}");
    let archive_dir = cfg.changes_dir().join("archive");
    let dest = archive_dir.join(&archived_name);
    let staged = ch
        .dir
        .parent()
        .expect("change directory always has a parent")
        .join(format!(".spectra-archive-{archived_name}.staged"));
    if path_entry_exists(&staged)? {
        anyhow::bail!(
            "archive staging path already exists at {}; recover or remove it before retrying",
            staged.display()
        );
    }

    let archive_dir_preexisting = archive_dir
        .try_exists()
        .with_context(|| format!("checking {}", archive_dir.display()))?;
    std::fs::create_dir_all(&archive_dir)
        .with_context(|| format!("creating {}", archive_dir.display()))?;
    let claim = ArchiveClaim::acquire(&archive_dir, &dest, &archived_name)?;
    std::fs::rename(&ch.dir, &staged).with_context(|| {
        format!(
            "freezing active change from {} to {}",
            ch.dir.display(),
            staged.display()
        )
    })?;

    let mut prepared = Vec::new();
    let mut metadata_snapshot = None;
    let mut tasks_snapshot = None;
    let transaction = (|| -> Result<()> {
        let fingerprint = DirectoryFingerprint::capture(&staged)?;
        let metadata = load_change_metadata(&staged)?;
        let declared_skip_specs = metadata.skip_specs == Some(true);
        if declared_skip_specs && has_any_file(&staged.join("specs"))? {
            anyhow::bail!("Change declares skip_specs but also contains files under specs/");
        }
        let skip_specs = skip_specs || declared_skip_specs;
        let retirement_declared = metadata.retire_capabilities == Some(true);
        if !skip_specs && !no_validate {
            validate_archive_compatibility(cfg, &staged, name, retirement_declared)?;
        }
        prepared = if skip_specs {
            Vec::new()
        } else {
            prepare_spec_deltas(
                cfg,
                &staged,
                name,
                today,
                retirement_declared,
                retirement_declared && !no_validate,
                false,
            )?
        };

        let mut metadata_file = FileSnapshot::capture(
            &staged.join(".openspec.yaml"),
            PathBuf::from(".openspec.yaml"),
        )?;
        let metadata_update =
            ArchivedMetadataUpdate::prepare(cfg, metadata_file.original.as_deref(), today)?;
        metadata_file.expect_bytes(metadata_update.bytes.clone());
        metadata_snapshot = Some(metadata_file);

        let mut tasks_file =
            FileSnapshot::capture(&staged.join("tasks.md"), PathBuf::from("tasks.md"))?;
        let tasks_update = if mark_tasks_complete {
            prepare_tasks_update(tasks_file.original.as_deref(), name)?
        } else {
            None
        };
        if let Some(bytes) = &tasks_update {
            tasks_file.expect_bytes(bytes.clone());
        }
        tasks_snapshot = Some(tasks_file);

        fingerprint.verify(&staged)?;
        commit_prepared_specs(&prepared)?;
        if let Some(bytes) = tasks_update {
            write_file_bytes(&staged.join("tasks.md"), &bytes)?;
        }
        metadata_update.apply(&staged)?;
        move_directory(&staged, &dest)
            .with_context(|| format!("moving {} to {}", staged.display(), dest.display()))?;
        Ok(())
    })();

    if let Err(error) = transaction {
        let mut rollback_errors = Vec::new();
        if let Err(rollback) = restore_frozen_change(&ch.dir, &staged, &dest) {
            rollback_errors.push(rollback.to_string());
        }
        if ch.dir.exists() {
            for snapshot in [metadata_snapshot.as_ref(), tasks_snapshot.as_ref()]
                .into_iter()
                .flatten()
            {
                if let Err(rollback) = snapshot.restore_if_expected(&ch.dir) {
                    rollback_errors.push(rollback.to_string());
                }
            }
        }
        if let Err(rollback) = rollback_prepared_specs(&prepared) {
            rollback_errors.push(rollback.to_string());
        }
        drop(claim);
        if !archive_dir_preexisting {
            let _ = std::fs::remove_dir(&archive_dir);
        }
        if rollback_errors.is_empty() {
            return Err(error);
        }
        return Err(anyhow!(
            "{error:#}; rollback also failed: {}",
            rollback_errors.join("; ")
        ));
    }

    if let Err(error) = change::clear_stale_sidecar_state(cfg, name) {
        eprintln!("warning: failed to clear sidecar state for '{name}': {error}");
    }

    Ok(ArchiveOutcome {
        name: name.to_string(),
        archived_name,
        specs_applied: prepared
            .into_iter()
            .filter_map(|spec| spec.result)
            .collect(),
    })
}

struct ArchiveClaim {
    path: PathBuf,
}

impl ArchiveClaim {
    fn acquire(archive_dir: &Path, dest: &Path, archive_name: &str) -> Result<Self> {
        if path_entry_exists(dest)? {
            anyhow::bail!("Archive '{archive_name}' already exists.");
        }
        let path = archive_dir.join(".spectra-archive.lock");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .with_context(|| {
                format!(
                    "archive destination is already claimed; remove stale lock {} if no archive is running",
                    path.display()
                )
            })?;
        writeln!(file, "pid={}", std::process::id())?;
        file.sync_all()?;
        if path_entry_exists(dest)? {
            let _ = std::fs::remove_file(&path);
            anyhow::bail!("Archive '{archive_name}' already exists.");
        }
        Ok(Self { path })
    }
}

impl Drop for ArchiveClaim {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[derive(Debug)]
struct FileSnapshot {
    relative_path: PathBuf,
    original: Option<Vec<u8>>,
    expected: Option<Vec<u8>>,
}

impl FileSnapshot {
    fn capture(source_path: &Path, relative_path: PathBuf) -> Result<Self> {
        Ok(Self {
            relative_path,
            original: read_optional_bytes(source_path)?,
            expected: None,
        })
    }

    fn expect_bytes(&mut self, expected: Vec<u8>) {
        self.expected = Some(expected);
    }

    fn restore_if_expected(&self, active_dir: &Path) -> Result<()> {
        let Some(expected) = self.expected.as_deref() else {
            return Ok(());
        };
        if self.original.as_deref() == Some(expected) {
            return Ok(());
        }
        let path = active_dir.join(&self.relative_path);
        let current = read_optional_bytes(&path)?;
        if current == self.original {
            return Ok(());
        }
        if current.as_deref() != Some(expected) {
            anyhow::bail!(
                "refusing to overwrite concurrent change at {} during rollback",
                path.display()
            );
        }
        restore_file(&path, self.original.as_deref())
    }
}

#[derive(Debug, PartialEq, Eq)]
struct DirectoryFingerprint {
    entries: Vec<FingerprintEntry>,
}

#[derive(Debug, PartialEq, Eq)]
struct FingerprintEntry {
    path: PathBuf,
    value: FingerprintValue,
}

#[derive(Debug, PartialEq, Eq)]
enum FingerprintValue {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

impl DirectoryFingerprint {
    fn capture(root: &Path) -> Result<Self> {
        let metadata = std::fs::symlink_metadata(root)
            .with_context(|| format!("fingerprinting {}", root.display()))?;
        if !metadata.file_type().is_dir() {
            anyhow::bail!("archive source is not a directory: {}", root.display());
        }
        let mut entries = Vec::new();
        Self::capture_dir(root, root, &mut entries)?;
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(Self { entries })
    }

    fn capture_dir(root: &Path, dir: &Path, entries: &mut Vec<FingerprintEntry>) -> Result<()> {
        for entry in
            std::fs::read_dir(dir).with_context(|| format!("fingerprinting {}", dir.display()))?
        {
            let entry = entry.with_context(|| format!("fingerprinting {}", dir.display()))?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .expect("fingerprinted entry is beneath root")
                .to_path_buf();
            let metadata = std::fs::symlink_metadata(&path)
                .with_context(|| format!("fingerprinting {}", path.display()))?;
            let value = if metadata.file_type().is_dir() {
                Self::capture_dir(root, &path, entries)?;
                FingerprintValue::Directory
            } else if metadata.file_type().is_file() {
                FingerprintValue::File(
                    std::fs::read(&path)
                        .with_context(|| format!("fingerprinting {}", path.display()))?,
                )
            } else if metadata.file_type().is_symlink() {
                FingerprintValue::Symlink(
                    std::fs::read_link(&path)
                        .with_context(|| format!("fingerprinting {}", path.display()))?,
                )
            } else {
                anyhow::bail!("unsupported archive entry {}", path.display());
            };
            entries.push(FingerprintEntry {
                path: relative,
                value,
            });
        }
        Ok(())
    }

    fn verify(&self, root: &Path) -> Result<()> {
        let current = Self::capture(root)?;
        if current != *self {
            anyhow::bail!(
                "frozen change changed while archive was preparing: {}",
                root.display()
            );
        }
        Ok(())
    }
}

fn path_entry_exists_io(path: &Path) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn path_entry_exists(path: &Path) -> Result<bool> {
    path_entry_exists_io(path).with_context(|| format!("checking {}", path.display()))
}

fn restore_frozen_change(active: &Path, staged: &Path, dest: &Path) -> Result<()> {
    if path_entry_exists(active)? {
        anyhow::bail!(
            "refusing to overwrite occupied active change path {} during rollback",
            active.display()
        );
    }
    let staged_exists = path_entry_exists(staged)?;
    let dest_exists = path_entry_exists(dest)?;
    if staged_exists {
        std::fs::rename(staged, active).with_context(|| {
            format!(
                "restoring frozen change from {} to {}",
                staged.display(),
                active.display()
            )
        })?;
        if dest_exists {
            anyhow::bail!(
                "restored active change from retained stage, but archive destination remains at {}",
                dest.display()
            );
        }
        return Ok(());
    }
    if dest_exists {
        move_directory(dest, active).with_context(|| {
            format!(
                "restoring archived change from {} to {}",
                dest.display(),
                active.display()
            )
        })?;
        return Ok(());
    }
    anyhow::bail!(
        "cannot restore active change {}; neither {} nor {} exists",
        active.display(),
        staged.display(),
        dest.display()
    )
}

fn read_optional_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

fn restore_file(path: &Path, content: Option<&[u8]>) -> Result<()> {
    match content {
        Some(content) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            std::fs::write(path, content).with_context(|| format!("restoring {}", path.display()))
        }
        None => match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error).with_context(|| format!("removing {}", path.display())),
        },
    }
}
fn has_any_file(dir: &Path) -> Result<bool> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error).with_context(|| format!("reading {}", dir.display())),
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        let metadata = std::fs::symlink_metadata(entry.path())
            .with_context(|| format!("reading {}", entry.path().display()))?;
        if metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Ok(true);
        }
        if metadata.file_type().is_dir() && has_any_file(&entry.path())? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn move_directory(source: &Path, destination: &Path) -> std::io::Result<()> {
    move_directory_with(
        source,
        destination,
        |source, destination| std::fs::rename(source, destination),
        |source| std::fs::remove_dir_all(source),
    )
}

fn move_directory_with<R, D>(
    source: &Path,
    destination: &Path,
    rename: R,
    remove_source: D,
) -> std::io::Result<()>
where
    R: FnOnce(&Path, &Path) -> std::io::Result<()>,
    D: FnOnce(&Path) -> std::io::Result<()>,
{
    match rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if is_cross_device_error(&error) => {
            copy_directory_exclusive(source, destination)?;
            if let Err(remove_error) = remove_source(source) {
                eprintln!(
                    "warning: archived copy at {} verified, but hidden staged source {} could not \
                     be removed ({remove_error}); the destination is authoritative and the staged \
                     source is retained for recovery",
                    destination.display(),
                    source.display()
                );
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn is_cross_device_error(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        error.raw_os_error() == Some(libc::EXDEV)
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}

fn copy_directory_exclusive(source: &Path, destination: &Path) -> std::io::Result<()> {
    copy_directory_exclusive_with(source, destination, directories_equal)
}

fn copy_directory_exclusive_with<V>(
    source: &Path,
    destination: &Path,
    verify: V,
) -> std::io::Result<()>
where
    V: FnOnce(&Path, &Path) -> std::io::Result<bool>,
{
    if path_entry_exists_io(destination)? {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("{} already exists", destination.display()),
        ));
    }
    if let Err(error) = copy_directory_contents(source, destination) {
        return Err(clean_failed_copy(destination, error));
    }
    match verify(source, destination) {
        Ok(true) => Ok(()),
        Ok(false) => Err(clean_failed_copy(
            destination,
            std::io::Error::other("archive fallback copy verification failed"),
        )),
        Err(error) => Err(clean_failed_copy(destination, error)),
    }
}

fn clean_failed_copy(destination: &Path, error: std::io::Error) -> std::io::Error {
    match std::fs::remove_dir_all(destination) {
        Ok(()) => error,
        Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => error,
        Err(cleanup) => std::io::Error::new(
            error.kind(),
            format!(
                "{error}; also failed to clean incomplete archive destination {}: {cleanup}",
                destination.display()
            ),
        ),
    }
}

fn copy_directory_contents(source: &Path, destination: &Path) -> std::io::Result<()> {
    let source_metadata = std::fs::symlink_metadata(source)?;
    if !source_metadata.file_type().is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "archive source is not a directory",
        ));
    }
    std::fs::create_dir(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&source_path)?;
        if metadata.file_type().is_dir() {
            copy_directory_contents(&source_path, &destination_path)?;
        } else if metadata.file_type().is_file() {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            let mut input = std::fs::File::open(&source_path)?;
            let mut output = options.open(&destination_path)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
            std::fs::set_permissions(&destination_path, metadata.permissions())?;
        } else if metadata.file_type().is_symlink() {
            copy_symlink(&source_path, &destination_path)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported archive entry {}", source_path.display()),
            ));
        }
    }
    std::fs::set_permissions(destination, source_metadata.permissions())?;
    Ok(())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(std::fs::read_link(source)?, destination)
}

#[cfg(not(unix))]
fn copy_symlink(_source: &Path, _destination: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "archive fallback cannot copy symbolic links on this platform",
    ))
}

fn directories_equal(left: &Path, right: &Path) -> std::io::Result<bool> {
    let mut left_entries = std::fs::read_dir(left)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    let mut right_entries = std::fs::read_dir(right)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    left_entries.sort();
    right_entries.sort();
    if left_entries != right_entries {
        return Ok(false);
    }
    for name in left_entries {
        let left_path = left.join(&name);
        let right_path = right.join(&name);
        let left_metadata = std::fs::symlink_metadata(&left_path)?;
        let right_metadata = std::fs::symlink_metadata(&right_path)?;
        if left_metadata.file_type() != right_metadata.file_type() {
            return Ok(false);
        }
        if left_metadata.file_type().is_dir() {
            if !directories_equal(&left_path, &right_path)? {
                return Ok(false);
            }
        } else if left_metadata.file_type().is_file() {
            if std::fs::read(&left_path)? != std::fs::read(&right_path)? {
                return Ok(false);
            }
        } else if left_metadata.file_type().is_symlink()
            && std::fs::read_link(&left_path)? != std::fs::read_link(&right_path)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Compute the exact task bytes before the transaction writes them. A missing
/// tasks file remains a no-op, including during rollback.
fn prepare_tasks_update(original: Option<&[u8]>, name: &str) -> Result<Option<Vec<u8>>> {
    let Some(original) = original else {
        eprintln!("warning: --mark-tasks-complete requested but no tasks.md found for '{name}'");
        return Ok(None);
    };
    let markdown = std::str::from_utf8(original).context("tasks.md is not valid UTF-8")?;
    Ok(Some(crate::tasks::mark_all_done(markdown).into_bytes()))
}

fn write_file_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let content = std::str::from_utf8(bytes)
        .with_context(|| format!("writing {}: content is not UTF-8", path.display()))?;
    crate::fsutil::write_atomically(path, content)
        .with_context(|| format!("writing {}", path.display()))
}

fn load_change_metadata(change_dir: &Path) -> Result<change::ChangeMetadata> {
    let path = change_dir.join(".openspec.yaml");
    let Some(bytes) = read_optional_bytes(&path)? else {
        return Ok(change::ChangeMetadata::default());
    };
    Ok(serde_yaml::from_slice(&bytes).unwrap_or_else(|error| {
        eprintln!(
            "warning: ignoring unparseable {} ({error}); \
             metadata fields (skip_specs, retire_capabilities) will use defaults",
            path.display()
        );
        change::ChangeMetadata::default()
    }))
}

struct ArchivedMetadataUpdate {
    bytes: Vec<u8>,
    parse_error: Option<String>,
}

impl ArchivedMetadataUpdate {
    fn prepare(cfg: &Config, original: Option<&[u8]>, today: chrono::NaiveDate) -> Result<Self> {
        let (mut metadata, parse_error) = match original {
            None => (change::ChangeMetadata::default(), None),
            Some(bytes) => match serde_yaml::from_slice(bytes) {
                Ok(metadata) => (metadata, None),
                Err(error) => (change::ChangeMetadata::default(), Some(error.to_string())),
            },
        };
        metadata.archived_by = crate::git::user_identity(&cfg.root);
        if metadata.archived_by.is_none() {
            eprintln!(
                "note: git user.name/user.email not configured; archived_by will be omitted from .openspec.yaml"
            );
        }
        metadata.archived_at = Some(today.to_string());
        Ok(Self {
            bytes: serde_yaml::to_string(&metadata)?.into_bytes(),
            parse_error,
        })
    }

    fn apply(self, change_dir: &Path) -> Result<()> {
        let meta_path = change_dir.join(".openspec.yaml");
        if let Some(error) = self.parse_error {
            let backup = touched::non_colliding_backup_path(&meta_path);
            let recovery_hint = match std::fs::copy(&meta_path, &backup) {
                Ok(_) => format!("the original file was preserved at {}", backup.display()),
                Err(copy_error) => {
                    format!("failed to preserve the original file too ({copy_error})")
                }
            };
            eprintln!(
                "warning: {} is unparseable ({error}); resetting its metadata -- {recovery_hint}",
                meta_path.display()
            );
        }
        write_file_bytes(&meta_path, &self.bytes)
    }
}

/// 一個要寫入（或 retire 時刪除）的 canonical 檔案：`spec.md`，或它的
/// `spec.trace.yaml` sidecar（`result` 為 `None`）。兩者共用同一套
/// commit／rollback，sidecar 因此跟 spec.md 同進同退。
struct PreparedSpec {
    path: PathBuf,
    original: Option<Vec<u8>>,
    content: Option<String>,
    result: Option<SpecApplyResult>,
    retire: bool,
}

/// 一個 capability 這次 archive 對追溯資料的影響，交給 `prepare_spec_deltas`
/// 寫成 sidecar。
#[derive(Debug, Default)]
struct SpecTraceDelta {
    /// 套 delta 前從既有 spec.md 剝出來的 inline footer（oracle 寫的，或
    /// sidecar 之前的舊版）；記的是改名前的名稱，要先吸收再套 RENAMED。
    footers: Vec<crate::trace::InlineFooter>,
    /// 套完 delta 後才剝出來的 footer：delta 的 ADDED／MODIFIED 內容自己帶進來
    /// 的（例如從 oracle 產出的 spec 整塊複製）；記的是新名稱，要在 RENAMED 之後吸收。
    late_footers: Vec<crate::trace::InlineFooter>,
    added: Vec<String>,
    modified: Vec<String>,
    removed: Vec<String>,
    renamed: Vec<crate::trace::RenamedRequirement>,
}

/// Validate every delta merge and capability retirement without writing specs,
/// reading touched sidecars, or mutating the change directory.
pub(crate) fn validate_archive_compatibility(
    cfg: &Config,
    change_dir: &Path,
    source: &str,
    retirement_declared: bool,
) -> Result<()> {
    prepare_spec_deltas(
        cfg,
        change_dir,
        source,
        chrono::Local::now().date_naive(),
        retirement_declared,
        retirement_declared,
        true,
    )
    .map(|_| ())
}

fn prepare_spec_deltas(
    cfg: &Config,
    change_dir: &Path,
    source: &str,
    today: chrono::NaiveDate,
    retirement_declared: bool,
    retirement_allowed: bool,
    dry_run: bool,
) -> Result<Vec<PreparedSpec>> {
    let mut prepared = Vec::new();
    // 這次 archive 的 `code` 清單只跟 change 有關；第一個需要它的 capability
    // 才算，之後共用（touched 只讀一次、警告只印一次）。
    let mut code_files: Option<Vec<String>> = None;
    for (capability, delta) in crate::fsutil::collect_delta_specs(&change_dir.join("specs"))? {
        let path = cfg.specs_dir().join(&capability).join("spec.md");
        let original = read_optional_bytes(&path)?;
        let (mut content, result, trace_delta) = merge_spec_delta(
            cfg,
            &capability,
            &delta,
            source,
            dry_run,
            retirement_declared,
        )?;
        let emptied = result.removed > 0
            && content.as_deref().is_some_and(|rebuilt| {
                crate::markdown::parse_main_requirements(rebuilt).is_empty()
            });
        let retire = if emptied {
            let rebuilt = content.as_deref().expect("emptied content exists");
            if !can_retire_spec(rebuilt) {
                anyhow::bail!(
                    "capability '{capability}' cannot be retired because its spec contains content outside Purpose and Requirements"
                );
            }
            if !retirement_allowed {
                if retirement_declared {
                    anyhow::bail!(
                        "capability '{capability}' retirement is disabled by --no-validate"
                    );
                }
                anyhow::bail!(
                    "capability '{capability}' would have no requirements; add retire_capabilities: true to the change metadata to retire it"
                );
            }
            content = None;
            true
        } else {
            false
        };

        // 驗證（dry_run，`spectra validate` 也走這裡）同樣解析 sidecar，所以壞掉
        // 的 sidecar 在寫入任何 canonical 檔案之前就失敗，archive 會把凍結的
        // change 還原。比對「寫入前沒被改過」與解析用的是同一份 bytes。
        let sidecar_path = crate::trace::sidecar_path(&path);
        let sidecar_original = read_optional_bytes(&sidecar_path)?;
        let existing_trace = sidecar_original
            .as_deref()
            .map(|bytes| {
                // 不能 lossy 解碼：壞掉的 bytes 會被換成 U+FFFD 後寫回，原始內容就沒了。
                let text = std::str::from_utf8(bytes).with_context(|| {
                    format!("{} is not a valid trace sidecar", sidecar_path.display())
                })?;
                crate::trace::TraceFile::parse(text, &sidecar_path)
            })
            .transpose()?;
        if existing_trace.is_none()
            && original
                .as_deref()
                .is_some_and(|bytes| crate::trace::has_pointer(&String::from_utf8_lossy(bytes)))
        {
            return Err(crate::trace::missing_sidecar_error(&sidecar_path));
        }
        let sidecar = if retire {
            // capability retire 時 sidecar 一併移除。archive 目錄不保存 sidecar，
            // 已移除的追溯紀錄只能從 git history 找回。
            sidecar_original.is_some().then_some(PreparedSpec {
                path: sidecar_path,
                original: sidecar_original,
                content: None,
                result: None,
                retire: true,
            })
        } else if let (Some(rebuilt), false) = (content.as_deref(), dry_run) {
            let mut trace = existing_trace.unwrap_or_default();
            trace.absorb(&trace_delta.footers);
            trace.apply_renames(&trace_delta.renamed);
            trace.absorb(&trace_delta.late_footers);
            trace.traces.push(crate::trace::TraceEntry {
                source: source.to_string(),
                updated: today.to_string(),
                added: trace_delta.added,
                modified: trace_delta.modified,
                removed: trace_delta.removed,
                renamed: trace_delta.renamed,
                code: code_files
                    .get_or_insert_with(|| trace_code_files(cfg, source))
                    .clone(),
                ..Default::default()
            });
            let rebuilt = crate::trace::ensure_pointer(rebuilt);
            // 行號以實際要寫出的內容計算，使用者照著找得到。
            let unparsed = crate::trace::extract_inline(&rebuilt).unparsed_lines;
            if !unparsed.is_empty() {
                eprintln!(
                    "warning: {}: left {} unrecognized `<!-- @trace` footer(s) in place ({}); move them into {} by hand",
                    path.display(),
                    unparsed.len(),
                    crate::trace::describe_lines(&unparsed),
                    crate::trace::SIDECAR_FILE
                );
            }
            content = Some(rebuilt);
            Some(PreparedSpec {
                path: sidecar_path,
                original: sidecar_original,
                content: Some(trace.to_yaml()?),
                result: None,
                retire: false,
            })
        } else {
            None
        };
        prepared.push(PreparedSpec {
            path,
            original,
            content,
            result: Some(result),
            retire,
        });
        prepared.extend(sidecar);
    }
    Ok(prepared)
}

/// 這次 archive 的 `code:` 清單：這個 change 的 touched file，排除確定已不在
/// 磁碟上的路徑（#98 D 項）。只讀 touched sidecar、不動它：`/spectra:commit`
/// 仍需要知道哪些刪除是哪個 task 造成的。`symlink_metadata` 讓斷掉的 symlink
/// 仍算存在；只有「確定不存在」才剔除，權限不足等其他 stat 錯誤保留並警告。
fn trace_code_files(cfg: &Config, source: &str) -> Vec<String> {
    let mut code_files: Vec<String> = touched::already_recorded_readonly(cfg, source)
        .into_iter()
        .filter(|f| match std::fs::symlink_metadata(cfg.root.join(f)) {
            Ok(_) => true,
            Err(e) if touched::is_gone(&e) => false,
            Err(e) => {
                eprintln!("warning: couldn't check {f} ({e}); keeping it in the trace code list");
                true
            }
        })
        .collect();
    code_files.sort();
    code_files
}

fn can_retire_spec(content: &str) -> bool {
    enum Section {
        Outside,
        Purpose,
        Requirements,
    }
    let normalized = crate::markdown::normalize_markdown(content);
    let lines: Vec<&str> = normalized.lines().collect();
    let mask = crate::markdown::fenced_line_mask(&lines);
    let mut section = Section::Outside;
    for (index, line) in lines.iter().enumerate() {
        if mask[index] {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("# ") && matches!(section, Section::Outside) {
            continue;
        }
        // archive 自己加的 sidecar 指標不算「Purpose 與 Requirements 以外的內容」。
        if crate::trace::is_pointer_line(trimmed) && matches!(section, Section::Outside) {
            continue;
        }
        if trimmed.eq_ignore_ascii_case("## Purpose") {
            section = Section::Purpose;
            continue;
        }
        if trimmed.eq_ignore_ascii_case("## Requirements") {
            section = Section::Requirements;
            continue;
        }
        if trimmed.starts_with('#') {
            return false;
        }
        if !matches!(section, Section::Purpose) {
            return false;
        }
    }
    true
}

fn commit_prepared_specs(prepared: &[PreparedSpec]) -> Result<()> {
    for spec in prepared {
        if read_optional_bytes(&spec.path)? != spec.original {
            anyhow::bail!(
                "main spec changed while archive was preparing: {}",
                spec.path.display()
            );
        }
        if spec.retire {
            std::fs::remove_file(&spec.path)
                .with_context(|| format!("retiring {}", spec.path.display()))?;
            if let Some(parent) = spec.path.parent() {
                let _ = std::fs::remove_dir(parent);
            }
            continue;
        }
        let Some(content) = &spec.content else {
            continue;
        };
        let parent = spec.path.parent().expect("spec path always has a parent");
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
        crate::fsutil::write_atomically(&spec.path, content)
            .with_context(|| format!("writing {}", spec.path.display()))?;
    }
    Ok(())
}

fn rollback_prepared_specs(prepared: &[PreparedSpec]) -> Result<()> {
    let mut failures = Vec::new();
    for spec in prepared.iter().rev() {
        if spec.retire {
            match read_optional_bytes(&spec.path) {
                Ok(current) if current == spec.original => {}
                Ok(None) => {
                    if let Err(error) = restore_file(&spec.path, spec.original.as_deref()) {
                        failures.push(error.to_string());
                    }
                }
                Ok(Some(_)) => failures.push(format!(
                    "refusing to overwrite concurrent change at {} during retirement rollback",
                    spec.path.display()
                )),
                Err(error) => failures.push(error.to_string()),
            }
            continue;
        }
        let Some(expected) = spec.content.as_ref().map(String::as_bytes) else {
            continue;
        };
        match read_optional_bytes(&spec.path) {
            Ok(current) if current == spec.original => {}
            Ok(Some(current)) if current.as_slice() == expected => {
                if let Err(error) = restore_file(&spec.path, spec.original.as_deref()) {
                    failures.push(error.to_string());
                }
            }
            Ok(None) if spec.original.is_none() => {}
            Ok(_) => failures.push(format!(
                "refusing to overwrite concurrent change at {} during rollback",
                spec.path.display()
            )),
            Err(error) => failures.push(error.to_string()),
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(failures.join("; ")))
    }
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
    added: Vec<crate::markdown::Requirement>,
    modified: Vec<crate::markdown::Requirement>,
    removed: Vec<String>,
    renamed: Vec<RenameDelta>,
    purpose: Option<String>,
}

#[derive(Debug)]
struct RenameDelta {
    from: String,
    to: String,
}

/// Merge one capability's parsed delta into its canonical spec. Parsed
/// [`crate::markdown::Requirement`] values remain authoritative for names,
/// raw blocks, and spans, including accepted header casing.
///
/// Operations run in OpenSpec order: RENAMED, REMOVED, MODIFIED, then ADDED.
fn merge_spec_delta(
    cfg: &Config,
    capability: &str,
    delta: &str,
    source: &str,
    dry_run: bool,
    retirement_declared: bool,
) -> Result<(Option<String>, SpecApplyResult, SpecTraceDelta)> {
    let parsed = parse_requirement_delta(capability, delta)?;
    let planned =
        parsed.added.len() + parsed.modified.len() + parsed.removed.len() + parsed.renamed.len();
    let mut result = SpecApplyResult {
        capability: capability.to_string(),
        added: 0,
        modified: 0,
        removed: 0,
        renamed: 0,
    };
    let mut trace = SpecTraceDelta::default();
    if planned == 0 {
        return Ok((None, result, trace));
    }

    let spec_path = cfg.specs_dir().join(capability).join("spec.md");
    let existing = read_optional(&spec_path)?;
    if existing.is_none()
        && retirement_declared
        && parsed.added.is_empty()
        && parsed.modified.is_empty()
        && parsed.renamed.is_empty()
        && !parsed.removed.is_empty()
    {
        return Ok((None, result, trace));
    }
    let mut content = match existing {
        Some(content) => {
            if parsed.purpose.is_some() && !dry_run {
                eprintln!(
                    "warning: capability '{capability}' already exists; ignoring delta Purpose and preserving {}",
                    spec_path.display()
                );
            }
            // 先把既有的 inline footer 剝出來，再套 delta：MODIFIED／REMOVED
            // 會整塊替換或刪除 requirement，先剝才不會連同 footer 一起丟掉。
            let normalized = crate::markdown::normalize_markdown(&content);
            let extracted = crate::trace::extract_inline(&normalized);
            trace.footers = extracted.footers;
            extracted.content
        }
        None if parsed.modified.is_empty()
            && parsed.removed.is_empty()
            && parsed.renamed.is_empty() =>
        {
            let purpose = parsed.purpose.clone().unwrap_or_else(|| {
                format!(
                    "TBD - created by archiving change '{source}'. Update Purpose after archive."
                )
            });
            format!(
                "# {capability} Specification\n\n\
                 ## Purpose\n\n\
                 {purpose}\n\n\
                 ## Requirements\n"
            )
        }
        None => {
            let first_missing = parsed
                .renamed
                .first()
                .map(|rename| ("RENAME", rename.from.as_str()))
                .or_else(|| parsed.removed.first().map(|name| ("REMOVE", name.as_str())))
                .or_else(|| {
                    parsed
                        .modified
                        .first()
                        .map(|requirement| ("MODIFY", requirement.name.as_str()))
                });
            let (kind, name) = first_missing.expect("non-ADDED delta exists");
            return Err(missing_requirement_error(
                capability, kind, name, &spec_path,
            ));
        }
    };

    for rename in &parsed.renamed {
        if let Some(block) = find_requirement_block(&content, &rename.from) {
            if let Some(existing_target) = find_folded_requirement_block(&content, &rename.to) {
                return Err(anyhow!(
                    "capability '{capability}': cannot RENAME requirement to '{}' -- requirement '{}' already exists in {}",
                    normalize_requirement_name(&rename.to),
                    existing_target.name,
                    spec_path.display()
                ));
            }
            content.replace_range(
                block.start..block.header_end,
                &format!("### Requirement: {}", rename.to.trim()),
            );
            result.renamed += 1;
            trace.renamed.push(crate::trace::RenamedRequirement {
                from: block.name.clone(),
                to: rename.to.trim().to_string(),
            });
        } else if let Some(variant) = find_folded_requirement_block(&content, &rename.from) {
            return Err(requirement_spelling_error(
                capability,
                "RENAME",
                &rename.from,
                &variant.name,
                &spec_path,
            ));
        } else if find_requirement_block(&content, &rename.to).is_none() {
            if let Some(variant) = find_folded_requirement_block(&content, &rename.to) {
                return Err(requirement_spelling_error(
                    capability,
                    "RENAME target",
                    &rename.to,
                    &variant.name,
                    &spec_path,
                ));
            }
            return Err(missing_requirement_error(
                capability,
                "RENAME",
                &rename.from,
                &spec_path,
            ));
        }
    }

    for removed in &parsed.removed {
        if let Some(block) = find_requirement_block(&content, removed) {
            refuse_to_discard_unrecognized_footer(capability, "REMOVE", &content, &block, None)?;
            content.replace_range(block.start..block.end, "");
            result.removed += 1;
            trace.removed.push(block.name.clone());
        } else if let Some(variant) = find_folded_requirement_block(&content, removed) {
            return Err(requirement_spelling_error(
                capability,
                "REMOVE",
                removed,
                &variant.name,
                &spec_path,
            ));
        } else {
            return Err(missing_requirement_error(
                capability, "REMOVE", removed, &spec_path,
            ));
        }
    }

    for modified in &parsed.modified {
        let name = modified.name.as_str();
        let block = find_requirement_block(&content, name)
            .ok_or_else(|| missing_requirement_error(capability, "MODIFY", name, &spec_path))?;
        let original = block_text(&content, &block);
        let missing = missing_scenarios_in_modified(&original, &modified.scenarios);
        if !missing.is_empty() {
            anyhow::bail!(
                "capability '{capability}': MODIFIED requirement '{name}' omits scenario(s) the current spec still has: {}",
                missing
                    .iter()
                    .map(|scenario| format!("\"{scenario}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if requirement_content_eq(&original, &modified.raw) {
            continue;
        }
        refuse_to_discard_unrecognized_footer(
            capability,
            "MODIFY",
            &content,
            &block,
            Some(&modified.raw),
        )?;
        let trailing = {
            let original = &content[block.start..block.end];
            original[original.trim_end().len()..].to_string()
        };
        content.replace_range(
            block.start..block.end,
            &format!("{}{trailing}", modified.raw),
        );
        result.modified += 1;
        trace.modified.push(modified.name.clone());
    }

    let mut added_to_apply = Vec::new();
    for added in &parsed.added {
        let name = added.name.as_str();
        if let Some(block) = find_requirement_block(&content, name) {
            let original = block_text(&content, &block);
            if requirement_content_eq(&original, &added.raw) {
                continue;
            }
            return Err(anyhow!(
                "capability '{capability}': cannot ADD requirement '{name}' -- it already exists in {}",
                spec_path.display()
            ));
        }
        if let Some(variant) = find_folded_requirement_block(&content, name) {
            return Err(requirement_spelling_error(
                capability,
                "ADD",
                name,
                &variant.name,
                &spec_path,
            ));
        }
        added_to_apply.push(added.raw.clone());
        result.added += 1;
        trace.added.push(added.name.clone());
    }
    append_added_requirements(&mut content, &added_to_apply);
    // delta 的 ADDED／MODIFIED 內容本身可能帶著 inline footer（依慣例 MODIFIED
    // 要貼整個 requirement，從 oracle 產出的 spec 複製時就會帶上）；它們也要
    // 進 sidecar，不能原樣寫回 spec.md。
    let late = crate::trace::extract_inline(&content);
    content = late.content;
    trace.late_footers = late.footers;

    let applied = result.added + result.modified + result.removed + result.renamed;
    if applied > 0 {
        content.truncate(content.trim_end_matches('\n').len());
        content.push('\n');
    }
    Ok(((applied > 0).then_some(content), result, trace))
}

/// MODIFIED／REMOVED 會整塊替換或刪除 requirement。可解析的 footer 在這之前
/// 已經剝進 sidecar，此時 block 裡若還有 `<!-- @trace`，就是認不得、無法搬
/// 的那種：不猜怎麼保留，直接失敗（驗證階段就會擋下），請人先處理。
/// `replacement` 是 MODIFIED 貼進來的新內容（REMOVED 為 `None`）；認不得的
/// footer 若原樣出現在新內容裡，就沒有東西會遺失，不擋。
fn refuse_to_discard_unrecognized_footer(
    capability: &str,
    operation: &str,
    content: &str,
    block: &RequirementBlock,
    replacement: Option<&str>,
) -> Result<()> {
    let discarded = crate::trace::unrecognized_footers(&content[block.start..block.end])
        .into_iter()
        .any(|footer| !replacement.is_some_and(|new| new.contains(&footer)));
    if !discarded {
        return Ok(());
    }
    anyhow::bail!(
        "capability '{capability}': cannot {operation} requirement '{}' -- it holds an unrecognized `<!-- @trace` footer that would be discarded; move it into {} or delete it by hand, then archive again",
        block.name,
        crate::trace::SIDECAR_FILE
    )
}

fn parse_requirement_delta(capability: &str, delta: &str) -> Result<RequirementDelta> {
    let parsed = crate::markdown::parse_delta(delta)
        .map_err(|error| anyhow!("capability '{capability}': parsing delta: {error:#}"))?;

    for (present, count, kind) in [
        (parsed.modified_present, parsed.modified.len(), "MODIFIED"),
        (parsed.removed_present, parsed.removed.len(), "REMOVED"),
        (parsed.renamed_present, parsed.renamed.len(), "RENAMED"),
    ] {
        if present && count == 0 {
            return Err(anyhow!(
                "capability '{capability}': `## {kind} Requirements` section contains no \
                 recognizable entries -- fix the delta or re-run with --skip-specs"
            ));
        }
    }

    Ok(RequirementDelta {
        purpose: parsed.purpose,
        added: parsed.added,
        modified: parsed.modified,
        removed: parsed.removed,
        renamed: parsed
            .renamed
            .into_iter()
            .map(|rename| RenameDelta {
                from: rename.from,
                to: rename.to,
            })
            .collect(),
    })
}

fn block_text(content: &str, block: &RequirementBlock) -> String {
    content[block.start..block.end].trim_end().to_string()
}

/// 兩段 requirement 內容是否相同，忽略多行 footer 與尾端空白、`---`。footer
/// 開頭用與 [`crate::trace::extract_inline`] 相同的判斷（`footer_opener`，
/// 排除 code fence 與縮排 code block），否則 `<!--@trace` 這類寫法會讓相同內容
/// 被判成不同，縮排 code block 裡的 footer 範例則會被當成 footer 忽略。
fn requirement_content_eq(left: &str, right: &str) -> bool {
    fn normalize(content: &str) -> String {
        let source: Vec<&str> = content.lines().collect();
        let mask = crate::markdown::fenced_line_mask(&source);
        let mut lines = Vec::new();
        let mut in_trace = false;
        for (index, line) in source.iter().enumerate() {
            let trimmed = line.trim();
            if !in_trace && !mask[index] && crate::trace::footer_opener(line) == Some(true) {
                in_trace = true;
                continue;
            }
            if in_trace {
                if trimmed == "-->" {
                    in_trace = false;
                }
                continue;
            }
            lines.push(line.trim_end());
        }
        while lines
            .last()
            .is_some_and(|line| line.trim().is_empty() || line.trim() == "---")
        {
            lines.pop();
        }
        lines.join("\n").trim().to_string()
    }

    normalize(left) == normalize(right)
}

fn missing_scenarios_in_modified(current: &str, modified: &[String]) -> Vec<String> {
    let mut proposed = modified.to_vec();
    let mut missing = Vec::new();
    let current_scenarios =
        crate::markdown::parse_main_requirements(&format!("## Requirements\n{current}"))
            .into_iter()
            .next()
            .map(|requirement| requirement.scenarios)
            .unwrap_or_default();
    for scenario in current_scenarios {
        if let Some(index) = proposed.iter().position(|candidate| candidate == &scenario) {
            proposed.remove(index);
        } else {
            missing.push(scenario);
        }
    }
    missing
}

fn requirement_blocks(content: &str) -> Vec<RequirementBlock> {
    crate::markdown::parse_main_requirements(content)
        .into_iter()
        .map(|requirement| RequirementBlock {
            name: requirement.name.clone(),
            normalized_name: normalize_requirement_name(&requirement.name),
            start: requirement.start,
            end: requirement.end,
            header_end: requirement.header_end,
        })
        .collect()
}

fn find_requirement_block(content: &str, name: &str) -> Option<RequirementBlock> {
    let needle = normalize_requirement_name(name);
    requirement_blocks(content)
        .into_iter()
        .find(|block| block.normalized_name == needle)
}

fn find_folded_requirement_block(content: &str, name: &str) -> Option<RequirementBlock> {
    let needle = normalize_requirement_name(name).to_lowercase();
    requirement_blocks(content)
        .into_iter()
        .find(|block| block.normalized_name.to_lowercase() == needle)
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

fn requirement_spelling_error(
    capability: &str,
    kind: &str,
    requested: &str,
    existing: &str,
    spec_path: &Path,
) -> anyhow::Error {
    anyhow!(
        "capability '{capability}': cannot {kind} requirement '{}' -- it differs only in case or whitespace from '{}' in {}",
        normalize_requirement_name(requested),
        normalize_requirement_name(existing),
        spec_path.display()
    )
}

/// Where new requirement blocks belong in the canonical spec's existing
/// content: right after the `## Requirements` header, before whatever `##`
/// section (if any) follows it -- never blindly at the very end of the file,
/// which would incorrectly nest new requirements under an unrelated trailing
/// section (e.g. a human-added `## Notes`/`## Appendix`).
///
/// A newly created canonical spec always has a `## Requirements` header, but
/// an existing canonical spec.md predating
/// that convention (or hand-edited to drop it) might not. Falling back to
/// `content.len()` in that case would silently reproduce the exact
/// trailing-section bug this function exists to avoid, so it falls back one
/// more step to right after `## Purpose` (using the same before-the-next-
/// section logic) before finally giving up and using the end of the file --
/// which is only reachable when the spec has no recognizable section
/// structure at all, so there's no trailing section left to nest under.
fn requirements_insertion_point(content: &str) -> usize {
    crate::markdown::main_requirements_insertion_point(content)
        .or_else(|| crate::markdown::main_purpose_insertion_point(content))
        .unwrap_or(content.len())
}

/// Append ADDED requirement blocks to `content` using the reverse-engineered
/// placement rules. The earlier RENAMED/REMOVED/MODIFIED operations mutate
/// `content` first, so ADDED's duplicate checks and insertion point see the
/// post-merge canonical spec. Validation and application share this: trace
/// data lives in the `spec.trace.yaml` sidecar (#98), so no inline `@trace`
/// footer is appended here.
fn append_added_requirements(content: &mut String, blocks: &[String]) {
    if blocks.is_empty() {
        return;
    }
    let mut has_existing_requirement =
        !crate::markdown::parse_main_requirements(content).is_empty();
    let mut insertion = String::new();
    for block in blocks {
        insertion.push_str(if has_existing_requirement {
            "\n---\n"
        } else {
            "\n"
        });
        insertion.push_str(block);
        insertion.push('\n');
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
        // between the new requirement and that section's header, matching
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

    /// 讀 capability 的 `spec.trace.yaml`；不存在時 panic。
    fn read_trace(c: &Config, capability: &str) -> crate::trace::TraceFile {
        let path = crate::trace::sidecar_path(&c.specs_dir().join(capability).join("spec.md"));
        crate::trace::TraceFile::load(&path)
            .unwrap()
            .unwrap_or_else(|| panic!("{} should exist", path.display()))
    }

    /// archive 後的 spec.md：追溯資料只在 sidecar，spec.md 只有一行指標。
    fn assert_trace_lives_in_the_sidecar(spec: &str) {
        assert!(
            !spec.contains("<!-- @trace\n"),
            "no inline footer expected:\n{spec}"
        );
        assert_eq!(
            spec.matches(crate::trace::POINTER).count(),
            1,
            "exactly one sidecar pointer expected:\n{spec}"
        );
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

    /// 建一個 change，`deltas` 為 (capability, delta 內容)，並寫入 canonical auth spec。
    fn preview_fixture(tmp: &TempDir, tasks: &str, deltas: &[(&str, &str)]) -> Config {
        let c = cfg(tmp);
        write(
            &c.specs_dir().join("auth/spec.md"),
            "# auth Specification\n\n## Purpose\n\nAuth.\n\n## Requirements\n\n\
             ### Requirement: Login\nThe system SHALL log in.\n\n#### Scenario: ok\n- **WHEN** a\n- **THEN** b\n\n\
             ### Requirement: Logout\nThe system SHALL log out.\n\n#### Scenario: ok\n- **WHEN** a\n- **THEN** b\n\n\
             ### Requirement: Session\nThe system SHALL keep sessions.\n\n#### Scenario: ok\n- **WHEN** a\n- **THEN** b\n",
        );
        let dir = c.changes_dir().join("demo");
        write(&dir.join(".openspec.yaml"), "schema: spec-driven\n");
        write(&dir.join("tasks.md"), tasks);
        for (capability, delta) in deltas {
            write(&dir.join("specs").join(capability).join("spec.md"), delta);
        }
        c
    }

    const SCENARIO: &str = "\n\n#### Scenario: s\n- **WHEN** a\n- **THEN** b\n";

    #[test]
    fn preview_counts_only_operations_that_would_apply_like_the_oracle() {
        let tmp = TempDir::new();
        let auth = format!(
            "## REMOVED Requirements\n\n### Requirement: Logout\n**Reason**: x\n\n### Requirement: Ghost\n**Reason**: y\n\n\
             ## MODIFIED Requirements\n\n### Requirement: Login\nThe system SHALL v2.{SCENARIO}\n\
             ### Requirement: Missing\nThe system SHALL m.{SCENARIO}\n\
             ## ADDED Requirements\n\n### Requirement: Session\nThe system SHALL dup.{SCENARIO}\n\
             ### Requirement: Fresh\nThe system SHALL f.{SCENARIO}"
        );
        let billing = format!(
            "## ADDED Requirements\n\n### Requirement: Invoice\nThe system SHALL i.{SCENARIO}"
        );
        let c = preview_fixture(
            &tmp,
            "- [ ] 1.1 open\n- [x] 1.2 done\n- [~] 1.3 partial\n",
            &[("billing", &billing), ("auth", &auth)],
        );
        let before = fs::read_to_string(c.specs_dir().join("auth/spec.md")).unwrap();

        let p = preview(&c, "demo").unwrap();

        assert_eq!(p.change_id, "demo");
        assert_eq!(p.incomplete_tasks, 2, "[ ] and [~] are both pending");
        assert!(p.has_delta_specs);
        assert!(p.warnings.is_empty());
        let shape: Vec<_> = p
            .spec_updates
            .iter()
            .map(|u| {
                (
                    u.capability.as_str(),
                    u.exists,
                    u.added,
                    u.modified,
                    u.removed,
                    u.renamed,
                )
            })
            .collect();
        // 名稱排序（oracle 用 readdir 順序，刻意分歧）。
        assert_eq!(
            shape,
            vec![("auth", true, 1, 1, 1, 0), ("billing", false, 1, 0, 0, 0)]
        );
        assert_eq!(
            fs::read_to_string(c.specs_dir().join("auth/spec.md")).unwrap(),
            before,
            "preview must not modify files"
        );
        assert!(c.changes_dir().join("demo").is_dir());
        assert_eq!(
            serde_json::to_string(&p.spec_updates[1]).unwrap(),
            r#"{"capability":"billing","exists":false,"added":1,"modified":0,"removed":0,"renamed":0,"conflict_source":null}"#
        );
    }

    #[test]
    fn preview_honors_skip_specs_metadata_and_rejects_empty_deltas() {
        let tmp = TempDir::new();
        let c = preview_fixture(&tmp, "", &[]);
        write(
            &c.changes_dir().join("demo/.openspec.yaml"),
            "schema: spec-driven\nskip_specs: true\n",
        );
        let p = preview(&c, "demo").unwrap();
        assert!(p.spec_updates.is_empty());
        assert!(!p.has_delta_specs);

        let tmp = TempDir::new();
        let c = preview_fixture(&tmp, "", &[("auth", "just prose\n")]);
        assert_eq!(
            preview(&c, "demo").unwrap_err().to_string(),
            "Failed to parse delta spec: Invalid format: Delta spec must contain at least one operation (ADDED, MODIFIED, REMOVED, or RENAMED)"
        );
        assert_eq!(
            preview(&c, "nope").unwrap_err().to_string(),
            "Change 'nope' does not exist"
        );
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

    #[cfg(unix)]
    #[test]
    fn fallback_copy_preserves_nested_files_and_symlinks() {
        let tmp = TempDir::new();
        let source = tmp.join("source");
        let destination = tmp.join("destination");
        write(&source.join("nested/file.txt"), "content\n");
        std::os::unix::fs::symlink("nested/file.txt", source.join("link")).unwrap();

        copy_directory_exclusive(&source, &destination).unwrap();

        assert!(source.is_dir());
        assert!(directories_equal(&source, &destination).unwrap());
        assert_eq!(
            std::fs::read_link(destination.join("link")).unwrap(),
            PathBuf::from("nested/file.txt")
        );
    }

    #[test]
    fn fallback_copy_cleans_destination_when_verification_errors() {
        let tmp = TempDir::new();
        let source = tmp.join("source");
        let destination = tmp.join("destination");
        write(&source.join("file.txt"), "content\n");

        let error = copy_directory_exclusive_with(&source, &destination, |_, _| {
            Err(std::io::Error::other("verification read failed"))
        })
        .unwrap_err();

        assert!(error.to_string().contains("verification read failed"));
        assert!(!destination.exists());
        assert!(source.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn verified_cross_device_copy_retains_staged_source_when_cleanup_fails() {
        let tmp = TempDir::new();
        let staged = tmp.join(".change.staged");
        let destination = tmp.join("archive");
        write(&staged.join("file.txt"), "content\n");

        move_directory_with(
            &staged,
            &destination,
            |_, _| Err(std::io::Error::from_raw_os_error(libc::EXDEV)),
            |_| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "simulated cleanup failure",
                ))
            },
        )
        .unwrap();

        assert!(staged.is_dir());
        assert!(destination.is_dir());
        assert!(directories_equal(&staged, &destination).unwrap());
    }

    #[test]
    fn frozen_source_fingerprint_detects_content_changes() {
        let tmp = TempDir::new();
        let staged = tmp.join(".change.staged");
        let file = staged.join("specs/cap/spec.md");
        write(&file, "before\n");
        let fingerprint = DirectoryFingerprint::capture(&staged).unwrap();

        write(&file, "after\n");

        let error = fingerprint.verify(&staged).unwrap_err();
        assert!(error
            .to_string()
            .contains("frozen change changed while archive was preparing"));
    }

    #[cfg(unix)]
    #[test]
    fn archive_rolls_back_an_earlier_spec_when_a_later_write_fails() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        let first_path = c.specs_dir().join("a-first").join("spec.md");
        let original = "# a-first Specification\n\n## Purpose\n\nFirst.\n\n## Requirements\n\n\
            ### Requirement: Existing\nold text\n";
        write(&first_path, original);
        // sidecar 與 spec.md 同進同退：已存在的要還原成原本的 bytes，
        // 原本不存在的（b-mid）要被移除。
        let first_sidecar = crate::trace::sidecar_path(&first_path);
        let original_sidecar = "version: 1\ntraces:\n- source: earlier\n  updated: 2026-01-01\n  added:\n  - Existing\n  code: []\n";
        write(&first_sidecar, original_sidecar);
        let mid_path = c.specs_dir().join("b-mid").join("spec.md");
        write(
            &mid_path,
            "# b-mid Specification\n\n## Purpose\n\nMid.\n\n## Requirements\n\n\
            ### Requirement: Existing\nold text\n",
        );
        write(
            &c.changes_dir().join("my-feature/specs/a-first/spec.md"),
            "## MODIFIED Requirements\n\n### Requirement: Existing\nnew text\n",
        );
        write(
            &c.changes_dir().join("my-feature/specs/b-mid/spec.md"),
            "## MODIFIED Requirements\n\n### Requirement: Existing\nnew text\n",
        );
        write(
            &c.changes_dir().join("my-feature/specs/z-last/spec.md"),
            DELTA_TEMPLATE,
        );
        let blocked = c.specs_dir().join("z-last");
        std::fs::create_dir_all(&blocked).unwrap();
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o555)).unwrap();

        let result = archive(&c, "my-feature", false, false, false);
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o755)).unwrap();

        result.unwrap_err();
        assert_eq!(std::fs::read_to_string(first_path).unwrap(), original);
        assert_eq!(
            std::fs::read_to_string(&first_sidecar).unwrap(),
            original_sidecar
        );
        assert!(!crate::trace::sidecar_path(&mid_path).exists());
        assert!(c.changes_dir().join("my-feature").is_dir());
        let archive_dir = c.changes_dir().join("archive");
        assert!(
            !archive_dir.exists()
                || !archive_dir
                    .read_dir()
                    .unwrap()
                    .any(|entry| entry.is_ok_and(|entry| entry
                        .file_name()
                        .to_string_lossy()
                        .ends_with("-my-feature")))
        );
        // 前提對照：同一個 change 在不被擋的情況下確實會寫出 b-mid 的 sidecar，
        // 所以上面「不存在」證明的是 rollback 移除了它，而不是它從沒被寫出。
        archive(&c, "my-feature", false, false, false).unwrap();
        assert!(crate::trace::sidecar_path(&mid_path).exists());
    }

    #[test]
    fn archive_claim_is_exclusive_and_released_on_drop() {
        let tmp = TempDir::new();
        let archive_dir = tmp.join("archive");
        let destination = archive_dir.join("2026-09-05-change");
        std::fs::create_dir_all(&archive_dir).unwrap();

        let claim = ArchiveClaim::acquire(&archive_dir, &destination, "change").unwrap();
        assert!(ArchiveClaim::acquire(&archive_dir, &destination, "change").is_err());
        drop(claim);
        assert!(ArchiveClaim::acquire(&archive_dir, &destination, "change").is_ok());
    }

    #[test]
    fn rollback_refuses_to_overwrite_a_concurrent_spec_edit() {
        let tmp = TempDir::new();
        let path = tmp.join("spec.md");
        std::fs::write(&path, "concurrent").unwrap();
        let prepared = vec![PreparedSpec {
            path: path.clone(),
            original: Some(b"old".to_vec()),
            content: Some("new".to_string()),
            result: Some(SpecApplyResult {
                capability: "cap".to_string(),
                added: 0,
                modified: 1,
                removed: 0,
                renamed: 0,
            }),
            retire: false,
        }];

        assert!(rollback_prepared_specs(&prepared).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "concurrent");
    }

    #[test]
    fn metadata_rollback_preserves_a_concurrent_edit() {
        let tmp = TempDir::new();
        let active = tmp.join("active");
        let path = active.join(".openspec.yaml");
        write(&path, "concurrent: true\n");
        let snapshot = FileSnapshot {
            relative_path: PathBuf::from(".openspec.yaml"),
            original: Some(b"created: 2026-09-05\n".to_vec()),
            expected: Some(b"created: 2026-09-05\narchived_at: 2026-09-05\n".to_vec()),
        };

        assert!(snapshot.restore_if_expected(&active).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "concurrent: true\n");
    }

    #[test]
    fn change_rollback_preserves_an_occupied_active_path() {
        let tmp = TempDir::new();
        let active = tmp.join("active");
        let staged = tmp.join(".active.staged");
        let destination = tmp.join("archive");
        write(&active.join("concurrent.txt"), "concurrent\n");
        write(&staged.join("original.txt"), "original\n");

        let error = restore_frozen_change(&active, &staged, &destination).unwrap_err();

        assert!(error.to_string().contains("occupied active change path"));
        assert_eq!(
            std::fs::read_to_string(active.join("concurrent.txt")).unwrap(),
            "concurrent\n"
        );
        assert!(staged.join("original.txt").is_file());
    }

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
        // Raw string appending would mis-handle YAML-significant ':' and '#';
        // metadata planning goes through serde_yaml so the value round-trips.
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
        // Metadata is deserialized and reserialized before the move; a field
        // this struct does not model by name must survive via
        // ChangeMetadata::extra's #[serde(flatten)] mapping.
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
    }

    #[test]
    fn archive_backs_up_an_unparseable_openspec_yaml_instead_of_overwriting_it_in_place() {
        // An unparseable .openspec.yaml is renamed aside at apply time before
        // the planned metadata replaces it, so its original frozen bytes stay
        // recoverable rather than being destroyed.
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
        assert_trace_lives_in_the_sidecar(&spec);
        assert!(spec.starts_with(&format!(
            "# my-cap Specification\n\n{}\n\n## Purpose",
            crate::trace::POINTER
        )));
        // The very first requirement in a fresh spec has no "---" separator.
        assert!(!spec.contains("---"));
        let trace = read_trace(&c, "my-cap");
        assert_eq!(trace.traces.len(), 1);
        assert_eq!(trace.traces[0].source, "my-feature");
        assert_eq!(trace.traces[0].added, vec!["<!-- requirement name -->"]);
    }

    /// 以 `files` 當這個 change 的 touched file 跑一次 archive，回傳 sidecar
    /// 這次紀錄的 `code` 清單。
    fn archive_with_touched(c: &Config, files: &[&str]) -> Vec<String> {
        change::create(c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            DELTA_TEMPLATE,
        );
        let files = files.iter().map(|f| f.to_string()).collect();
        touched::record(c, "my-feature", 1, "t1", files).unwrap();
        archive(c, "my-feature", false, false, false).unwrap();
        read_trace(c, "my-cap").traces.pop().unwrap().code
    }

    #[test]
    fn archive_trace_code_omits_touched_paths_that_no_longer_exist() {
        // #98：已從磁碟消失的路徑不寫進 `code`，但 touched sidecar 本身不動。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(&tmp.join("src/kept.rs"), "// kept\n");

        let code = archive_with_touched(&c, &["src/kept.rs", "src/gone.rs"]);

        assert_eq!(code, vec!["src/kept.rs"]);
    }

    #[cfg(unix)]
    #[test]
    fn archive_trace_code_keeps_a_dangling_symlink() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        std::os::unix::fs::symlink("nowhere", tmp.join("link")).unwrap();

        let code = archive_with_touched(&c, &["link"]);

        assert_eq!(code, vec!["link"]);
    }

    #[cfg(unix)]
    #[test]
    fn archive_trace_code_keeps_a_path_it_cannot_stat() {
        use std::os::unix::fs::PermissionsExt;
        /// 測試結束（含 panic）時把目錄權限改回來，讓 TempDir 刪得掉。
        struct RestoreSearchable(PathBuf);
        impl Drop for RestoreSearchable {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
            }
        }
        // #173 review：權限不足不代表檔案消失，不能從 `code:` 剔除。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(&tmp.join("private/secret.rs"), "// secret\n");
        let private = tmp.join("private");
        let _restore = RestoreSearchable(private.clone());
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o600)).unwrap();
        if std::fs::symlink_metadata(private.join("secret.rs")).is_ok() {
            eprintln!("skipping: running as root (directory search permission not enforced)");
            return;
        }

        let code = archive_with_touched(&c, &["private/secret.rs"]);

        assert_eq!(code, vec!["private/secret.rs"]);
    }

    #[cfg(unix)]
    #[test]
    fn archive_trace_code_omits_a_path_whose_parent_became_a_file() {
        // #173 round 2：上層目錄被換成一般檔案（NotADirectory），路徑同樣已不存在。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(&tmp.join("a"), "now a file\n");
        write(&tmp.join("kept.rs"), "// kept\n");

        let code = archive_with_touched(&c, &["a/b.rs", "kept.rs"]);

        assert_eq!(code, vec!["kept.rs"]);
    }

    #[test]
    fn archive_preserves_an_authored_purpose_for_a_new_capability() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            &format!("## Purpose\n\nLets users export portable account data.\n\n{DELTA_TEMPLATE}"),
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap/spec.md")).unwrap();
        assert!(spec.contains("## Purpose\n\nLets users export portable account data."));
        assert!(!spec.contains("TBD - created by archiving"));
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
        assert_trace_lives_in_the_sidecar(&spec);
        assert_eq!(
            read_trace(&c, "Billing/Invoices").traces[0].source,
            "my-feature"
        );
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
            "a blank line must separate the inserted requirement from '## Notes', got:\n{spec}"
        );
    }

    #[test]
    fn archive_ignores_fenced_requirements_heading_when_placing_added_blocks() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap/spec.md"),
            "# my-cap Specification\n\n## Purpose\n\nExisting.\n\n\
             ```markdown\n## requirements\nquoted\n```\n\n\
             ## Requirements\n\n### Requirement: Existing\ntext\n",
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            DELTA_TEMPLATE,
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap/spec.md")).unwrap();
        let real_heading = spec.rfind("## Requirements").unwrap();
        let added = spec
            .find("### Requirement: <!-- requirement name -->")
            .unwrap();
        assert!(
            added > real_heading,
            "added block landed outside real section:\n{spec}"
        );
        assert!(crate::markdown::parse_main_requirements(&spec)
            .iter()
            .any(|requirement| requirement.name == "<!-- requirement name -->"));
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
            crate::markdown::parse_main_requirements(&spec).len(),
            3,
            "all three requirement headers must remain line-anchored, got:\n{spec}"
        );
    }

    #[test]
    fn archive_accepts_lowercase_section_and_requirement_headers() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap").join("spec.md");
        write(
            &spec_path,
            "# my-cap Specification\n\n## purpose\n\nExisting.\n\n## requirements\n\n\
             ### requirement: First\n\nold text\n\n## Notes\n\nKeep me.\n",
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## modified requirements\n\n### requirement: First\n\nnew text\n\n\
             ## added requirements\n\n### requirement: Second\n\nsecond text\n",
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        let rebuilt = std::fs::read_to_string(spec_path).unwrap();
        assert!(rebuilt.contains("### requirement: First\n\nnew text"));
        let added = rebuilt.find("### requirement: Second").unwrap();
        let notes = rebuilt.find("## Notes").unwrap();
        assert!(
            added < notes,
            "ADDED requirement escaped its lowercase section"
        );
    }

    #[test]
    fn archive_uses_lowercase_purpose_as_the_added_requirement_fallback() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap").join("spec.md");
        write(
            &spec_path,
            "# my-cap Specification\n\n## purpose\n\nExisting.\n\n## Notes\n\nKeep me.\n",
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## added requirements\n\n### requirement: First\n\ntext\n",
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        let rebuilt = std::fs::read_to_string(spec_path).unwrap();
        assert!(
            rebuilt.find("### requirement: First").unwrap() < rebuilt.find("## Notes").unwrap()
        );
    }

    #[test]
    fn archive_canonicalizes_rebuilt_specs_to_one_final_newline() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap").join("spec.md");
        write(&spec_path, &format!("{CANONICAL_SPEC}\n\n"));
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir()
                .join("my-feature")
                .join("specs")
                .join("my-cap")
                .join("spec.md"),
            "## MODIFIED Requirements\n\n\
             ### Requirement: Second\n\nupdated text\n",
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        let rebuilt = std::fs::read_to_string(spec_path).unwrap();
        assert!(rebuilt.ends_with('\n'));
        assert!(!rebuilt.ends_with("\n\n"));
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
    fn archive_rejects_a_typo_in_a_removed_requirement() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap").join("spec.md"),
            CANONICAL_SPEC,
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## REMOVED Requirements\n\n### Requirement: Secnod\n",
        );

        let error = archive(&c, "my-feature", false, false, false).unwrap_err();

        assert!(error
            .to_string()
            .contains("cannot REMOVE requirement 'Secnod'"));
        assert!(c.changes_dir().join("my-feature").is_dir());
    }

    #[test]
    fn explicit_retirement_allows_an_already_absent_capability() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/.openspec.yaml"),
            "retire_capabilities: true\n",
        );
        write(
            &c.changes_dir().join("my-feature/specs/gone/spec.md"),
            "## REMOVED Requirements\n\n### Requirement: Former\n",
        );

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

        assert_eq!(outcome.specs_applied[0].removed, 0);
        assert!(!c.specs_dir().join("gone/spec.md").exists());
    }

    #[test]
    fn capability_retirement_requires_marker_and_deletes_the_spec_when_declared() {
        let make = |label: &str, declared: bool| {
            let tmp = TempDir::new();
            let c = cfg(&tmp);
            let spec_path = c.specs_dir().join("my-cap/spec.md");
            write(
                &spec_path,
                "# my-cap Specification\n\n## Purpose\n\nCapability purpose.\n\n## Requirements\n\n\
                 ### Requirement: Only\nThe system SHALL exist.\n\n\
                 #### Scenario: Exists\n- **WHEN** used\n- **THEN** it works\n",
            );
            change::create(&c, label).unwrap();
            if declared {
                write(
                    &c.changes_dir().join(label).join(".openspec.yaml"),
                    "schema: spec-driven\nretire_capabilities: true\n",
                );
            }
            write(
                &c.changes_dir().join(label).join("specs/my-cap/spec.md"),
                "## REMOVED Requirements\n\n### Requirement: Only\n",
            );
            (tmp, c, spec_path)
        };

        let (_blocked_tmp, blocked_cfg, blocked_path) = make("blocked", false);
        assert!(archive(&blocked_cfg, "blocked", false, false, false).is_err());
        assert!(blocked_path.is_file());
        assert!(blocked_cfg.changes_dir().join("blocked").is_dir());

        let (_retired_tmp, retired_cfg, retired_path) = make("retired", true);
        let retired_sidecar = crate::trace::sidecar_path(&retired_path);
        write(&retired_sidecar, "version: 1\ntraces: []\n");
        archive(&retired_cfg, "retired", false, false, false).unwrap();
        assert!(!retired_path.exists());
        // sidecar 一併移除，capability 目錄才不會只剩一個 yaml。
        assert!(!retired_sidecar.exists());
        assert!(!retired_path.parent().unwrap().exists());
    }

    #[test]
    fn a_spec_with_a_sidecar_pointer_can_still_be_retired() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap/spec.md");
        write(
            &spec_path,
            &format!(
                "# my-cap Specification\n\n{}\n\n## Purpose\n\nP.\n\n## Requirements\n\n\
                 ### Requirement: Only\nThe system SHALL exist.\n",
                crate::trace::POINTER
            ),
        );
        // 有指標就要有 sidecar（指標對到不存在的檔案會讓 archive 失敗，#179）。
        let sidecar = crate::trace::sidecar_path(&spec_path);
        write(
            &sidecar,
            "version: 1\ntraces:\n- source: s\n  updated: d\n  added:\n  - Only\n  code: []\n",
        );
        change::create(&c, "retire").unwrap();
        write(
            &c.changes_dir().join("retire/.openspec.yaml"),
            "schema: spec-driven\nretire_capabilities: true\n",
        );
        write(
            &c.changes_dir().join("retire/specs/my-cap/spec.md"),
            "## REMOVED Requirements\n\n### Requirement: Only\n",
        );

        archive(&c, "retire", false, false, false).unwrap();

        assert!(!spec_path.exists());
        assert!(!sidecar.exists());
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
             #### Scenario: First scenario\n\n\
             - **WHEN** first changes\n\
             - **THEN** modified behavior applies\n\n\
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
        assert_trace_lives_in_the_sidecar(&spec);
        let trace = read_trace(&c, "my-cap");
        assert_eq!(trace.traces.len(), 1);
        let entry = &trace.traces[0];
        assert_eq!(entry.source, "my-feature");
        assert_eq!(entry.added, vec!["Fourth"]);
        assert_eq!(entry.modified, vec!["Renamed First"]);
        assert_eq!(entry.removed, vec!["Third"]);
        assert_eq!(
            entry.renamed,
            vec![crate::trace::RenamedRequirement {
                from: "First".into(),
                to: "Renamed First".into(),
            }]
        );
    }

    /// oracle 3.0.0 實際 archive 出來的 canonical spec 形狀（2026-09-26 實測）：
    /// 每個 requirement 各一份 inline footer，第一份前面有兩個空行。
    const ORACLE_ARCHIVED_SPEC: &str = "# my-cap Specification\n\n## Purpose\n\nP.\n\n## Requirements\n\n\
### Requirement: Alpha\n\nThe system SHALL alpha.\n\n#### Scenario: a\n\n- **WHEN** x\n- **THEN** y\n\n\n\
<!-- @trace\nsource: oracle-change\nupdated: 2026-09-01\ncode:\n  - pre.txt\n  - a.rs\n-->\n\n---\n\
### Requirement: Beta\n\nThe system SHALL beta.\n\n#### Scenario: b\n\n- **WHEN** x\n- **THEN** y\n\n\
<!-- @trace\nsource: oracle-change\nupdated: 2026-09-01\ncode:\n  - pre.txt\n  - a.rs\n-->";

    #[test]
    fn archive_absorbs_oracle_inline_footers_into_the_sidecar() {
        // 混用情境：oracle 寫出的 inline footer，下一次 openspectra archive 要
        // 吸收進 sidecar；MODIFIED 的 requirement 其 footer 必須在整塊替換前先剝出來。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(&c.specs_dir().join("my-cap/spec.md"), ORACLE_ARCHIVED_SPEC);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## MODIFIED Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha, again.\n\n\
             #### Scenario: a\n\n- **WHEN** x\n- **THEN** y2\n",
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap/spec.md")).unwrap();
        assert_trace_lives_in_the_sidecar(&spec);
        assert!(spec.contains("The system SHALL alpha, again."));
        // 沒被修改的 Beta：footer 連同前面的空行剝乾淨，檔尾只留一個換行。
        assert!(
            spec.ends_with(
                "The system SHALL beta.\n\n#### Scenario: b\n\n- **WHEN** x\n- **THEN** y\n"
            ),
            "{spec}"
        );
        let trace = read_trace(&c, "my-cap");
        assert_eq!(trace.traces.len(), 2, "{trace:?}");
        assert_eq!(trace.traces[0].source, "oracle-change");
        assert_eq!(trace.traces[0].imported, vec!["Alpha", "Beta"]);
        assert_eq!(trace.traces[0].code, vec!["pre.txt", "a.rs"]);
        assert_eq!(trace.traces[1].source, "my-feature");
        assert_eq!(trace.traces[1].modified, vec!["Alpha"]);
    }

    #[test]
    fn a_second_archive_appends_an_entry_and_keeps_one_pointer() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        for (name, requirement) in [("first", "One"), ("second", "Two")] {
            change::create(&c, name).unwrap();
            write(
                &c.changes_dir().join(name).join("specs/my-cap/spec.md"),
                &format!("## ADDED Requirements\n\n### Requirement: {requirement}\n\ntext\n"),
            );
            archive(&c, name, false, false, false).unwrap();
        }

        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap/spec.md")).unwrap();
        assert_trace_lives_in_the_sidecar(&spec);
        let trace = read_trace(&c, "my-cap");
        let sources: Vec<&str> = trace.traces.iter().map(|t| t.source.as_str()).collect();
        assert_eq!(sources, vec!["first", "second"]);
        assert_eq!(trace.traces[1].added, vec!["Two"]);
    }

    #[test]
    fn archive_renames_requirements_recorded_by_earlier_entries() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "first").unwrap();
        write(
            &c.changes_dir().join("first/specs/my-cap/spec.md"),
            "## ADDED Requirements\n\n### Requirement: Old\n\ntext\n",
        );
        archive(&c, "first", false, false, false).unwrap();
        change::create(&c, "second").unwrap();
        write(
            &c.changes_dir().join("second/specs/my-cap/spec.md"),
            "## RENAMED Requirements\n- FROM: `### Requirement: Old`\n- TO: `### Requirement: New`\n",
        );

        archive(&c, "second", false, false, false).unwrap();

        let trace = read_trace(&c, "my-cap");
        assert_eq!(trace.traces[0].added, vec!["New"]);
        assert_eq!(trace.traces[1].renamed[0].from, "Old");
        assert_eq!(trace.traces[1].renamed[0].to, "New");
    }

    #[test]
    fn archive_refuses_to_overwrite_a_corrupt_sidecar() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap/spec.md");
        write(&spec_path, CANONICAL_SPEC);
        let sidecar = crate::trace::sidecar_path(&spec_path);
        write(&sidecar, "traces: [");
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## ADDED Requirements\n\n### Requirement: Fourth\n\ntext\n",
        );

        let error = archive(&c, "my-feature", false, false, false).unwrap_err();

        assert!(
            format!("{error:#}").contains("is not a valid trace sidecar"),
            "{error:#}"
        );
        assert_eq!(std::fs::read_to_string(&sidecar).unwrap(), "traces: [");
        assert_eq!(std::fs::read_to_string(&spec_path).unwrap(), CANONICAL_SPEC);
        assert!(c.changes_dir().join("my-feature").is_dir());
    }

    #[test]
    fn archive_leaves_an_unrecognized_footer_in_place() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let footer = "<!-- @trace\nsource: x\nupdated: y\nowner: someone\n-->";
        write(
            &c.specs_dir().join("my-cap/spec.md"),
            &format!(
                "# my-cap Specification\n\n## Purpose\n\nP.\n\n## Requirements\n\n\
                 ### Requirement: Alpha\n\ntext\n\n{footer}\n"
            ),
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## ADDED Requirements\n\n### Requirement: Beta\n\ntext\n",
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap/spec.md")).unwrap();
        assert!(
            spec.contains(footer),
            "unrecognized footer must survive:\n{spec}"
        );
        assert_eq!(read_trace(&c, "my-cap").traces.len(), 1);
    }

    /// 一份 Alpha 裡帶著認不得 footer（未知欄位 `owner:`）的 canonical spec。
    const SPEC_WITH_UNRECOGNIZED_FOOTER: &str =
        "# my-cap Specification\n\n## Purpose\n\nP.\n\n## Requirements\n\n\
### Requirement: Alpha\n\ntext\n\n<!-- @trace\nsource: x\nupdated: y\nowner: someone\n-->\n\n---\n\
### Requirement: Beta\n\nbeta\n";

    #[test]
    fn archive_refuses_to_modify_or_remove_a_requirement_holding_an_unrecognized_footer() {
        // #175 review：認不得的 footer 會隨 MODIFIED／REMOVED 整塊消失，不能先刪再說「保留」。
        for delta in [
            "## MODIFIED Requirements\n\n### Requirement: Alpha\n\nnew text\n",
            "## REMOVED Requirements\n\n### Requirement: Alpha\n",
        ] {
            let tmp = TempDir::new();
            let c = cfg(&tmp);
            let spec_path = c.specs_dir().join("my-cap/spec.md");
            write(&spec_path, SPEC_WITH_UNRECOGNIZED_FOOTER);
            change::create(&c, "my-feature").unwrap();
            write(
                &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
                delta,
            );

            let error = archive(&c, "my-feature", false, false, false).unwrap_err();

            let message = format!("{error:#}");
            assert!(
                message.contains("unrecognized `<!-- @trace` footer")
                    && message.contains("'Alpha'"),
                "{message}"
            );
            assert_eq!(
                std::fs::read_to_string(&spec_path).unwrap(),
                SPEC_WITH_UNRECOGNIZED_FOOTER
            );
            assert!(c.changes_dir().join("my-feature").is_dir());
        }
    }

    #[test]
    fn archive_absorbs_footers_that_arrive_inside_delta_blocks() {
        // #175 review：MODIFIED 依慣例貼整個 requirement，從 oracle 產出的 spec 複製時
        // 會連 footer 一起帶進來；它們也要進 sidecar，不能原樣寫回 spec.md。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(&c.specs_dir().join("my-cap/spec.md"), CANONICAL_SPEC);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## ADDED Requirements\n\n### Requirement: Fourth\n\nfourth text\n\n\
             <!-- @trace\nsource: copied\nupdated: 2026-09-01\ncode:\n  - c.rs\n-->\n",
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        let spec = std::fs::read_to_string(c.specs_dir().join("my-cap/spec.md")).unwrap();
        assert_trace_lives_in_the_sidecar(&spec);
        let trace = read_trace(&c, "my-cap");
        let copied = trace
            .traces
            .iter()
            .find(|entry| entry.source == "copied")
            .unwrap_or_else(|| panic!("{trace:?}"));
        assert_eq!(copied.imported, vec!["Fourth"]);
        assert_eq!(copied.code, vec!["c.rs"]);
    }

    #[test]
    fn archive_renames_names_absorbed_from_inline_footers() {
        // 吸收要在套 RENAMED 之前：footer 記的是改名前的名稱。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(&c.specs_dir().join("my-cap/spec.md"), ORACLE_ARCHIVED_SPEC);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## RENAMED Requirements\n- FROM: `### Requirement: Alpha`\n- TO: `### Requirement: Gamma`\n",
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        assert_eq!(
            read_trace(&c, "my-cap").traces[0].imported,
            vec!["Gamma", "Beta"]
        );
    }

    #[test]
    fn footers_carried_in_by_the_delta_are_not_renamed() {
        // delta 帶進來的 footer 記的是新名稱，要在套 RENAMED 之後才吸收：
        // 這裡 Alpha 改名成 Gamma，同時又 ADD 一個新的 Alpha。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write(
            &c.specs_dir().join("my-cap/spec.md"),
            "# my-cap Specification\n\n## Purpose\n\nP.\n\n## Requirements\n\n### Requirement: Alpha\n\nold\n",
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## ADDED Requirements\n\n### Requirement: Alpha\n\nnew\n\n\
             <!-- @trace\nsource: copied\nupdated: 2026-09-01\ncode: []\n-->\n\n\
             ## RENAMED Requirements\n- FROM: `### Requirement: Alpha`\n- TO: `### Requirement: Gamma`\n",
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        let trace = read_trace(&c, "my-cap");
        let copied = trace
            .traces
            .iter()
            .find(|entry| entry.source == "copied")
            .unwrap();
        assert_eq!(copied.imported, vec!["Alpha"]);
    }

    #[test]
    fn archive_refuses_a_non_utf8_sidecar_and_leaves_its_bytes_alone() {
        // #175 round 2：以 lossy 解碼會把壞掉的 bytes 換成 U+FFFD 後寫回，原始內容就沒了。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap/spec.md");
        write(&spec_path, CANONICAL_SPEC);
        let sidecar = crate::trace::sidecar_path(&spec_path);
        let bytes =
            b"version: 1\ntraces:\n- source: \"a\xFFb\"\n  updated: d\n  code: []\n".to_vec();
        std::fs::write(&sidecar, &bytes).unwrap();
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## ADDED Requirements\n\n### Requirement: Fourth\n\ntext\n",
        );

        let error = archive(&c, "my-feature", false, false, false).unwrap_err();

        assert!(
            format!("{error:#}").contains("is not a valid trace sidecar"),
            "{error:#}"
        );
        assert_eq!(std::fs::read(&sidecar).unwrap(), bytes);
        assert_eq!(std::fs::read_to_string(&spec_path).unwrap(), CANONICAL_SPEC);
    }

    #[test]
    fn archive_allows_modifying_a_requirement_whose_pasted_block_keeps_its_unrecognized_footer() {
        // #175 round 2：依慣例 MODIFIED 貼整個 requirement；認不得的 footer 若原樣
        // 跟著貼過來，就沒有東西會遺失，不該拒絕。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap/spec.md");
        write(&spec_path, SPEC_WITH_UNRECOGNIZED_FOOTER);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## MODIFIED Requirements\n\n### Requirement: Alpha\n\nnew text\n\n\
             <!-- @trace\nsource: x\nupdated: y\nowner: someone\n-->\n",
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        let spec = std::fs::read_to_string(&spec_path).unwrap();
        assert!(spec.contains("new text"), "{spec}");
        assert!(spec.contains("owner: someone"), "{spec}");
    }

    #[test]
    fn validation_rejects_a_corrupt_trace_sidecar() {
        // `spectra validate` 走同一個相容性檢查，壞掉的 sidecar 要在這裡就被擋下。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap/spec.md");
        write(&spec_path, CANONICAL_SPEC);
        write(&crate::trace::sidecar_path(&spec_path), "traces: [");
        change::create(&c, "my-feature").unwrap();
        let change_dir = c.changes_dir().join("my-feature");
        write(
            &change_dir.join("specs/my-cap/spec.md"),
            "## ADDED Requirements\n\n### Requirement: Fourth\n\ntext\n",
        );

        let error =
            validate_archive_compatibility(&c, &change_dir, "my-feature", false).unwrap_err();

        assert!(
            format!("{error:#}").contains("is not a valid trace sidecar"),
            "{error:#}"
        );
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
    fn archive_errors_when_both_rename_source_and_target_are_missing() {
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
            "## RENAMED Requirements\n- FROM: `### Requirement: Missing`\n- TO: `### Requirement: New`\n",
        );

        let err = archive(&c, "my-feature", false, false, false).unwrap_err();

        assert!(
            err.to_string()
                .contains("cannot RENAME requirement 'Missing'"),
            "rename conflict should name the missing requirement, got: {err}"
        );
        assert!(c.changes_dir().join("my-feature").is_dir());
        assert!(!c.changes_dir().join("archive").exists());
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
            crate::markdown::parse_main_requirements(&spec).len(),
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
    }

    #[test]
    fn preparing_specs_is_side_effect_free_on_the_touched_sidecar_in_both_modes() {
        // Validation (dry_run) never reads the touched sidecar, and application
        // reads it only through the read-only loader, so a corrupt touched file
        // is never renamed aside during a transaction that may roll back.
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        let change_dir = c.changes_dir().join("my-feature");
        write(
            &change_dir.join("specs/my-cap/spec.md"),
            "## ADDED Requirements\n\n### Requirement: New\n\ntext\n",
        );
        let touched = crate::touched::touched_path(&c, "my-feature");
        write(&touched, "not valid json");
        let corrupt_backup = touched.with_extension("json.corrupt");
        let today = chrono::Local::now().date_naive();

        prepare_spec_deltas(&c, &change_dir, "my-feature", today, false, false, true).unwrap();
        assert!(
            touched.is_file() && !corrupt_backup.exists(),
            "validation must not read or rename the touched sidecar"
        );

        prepare_spec_deltas(&c, &change_dir, "my-feature", today, false, false, false).unwrap();
        assert!(
            touched.is_file() && !corrupt_backup.exists(),
            "application must not rename the touched sidecar either"
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
        // Regression: compatibility failure after freezing must restore the
        // staged directory to its active name without committing any spec.
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
        write(&tmp.join("src/lib.rs"), "// lib\n");
        let snapshot = touched::snapshot(&c, &["src/lib.rs".to_string()]);
        touched::write_baseline(&c, "my-feature", &snapshot).unwrap();
        assert!(touched::baseline_path(&c, "my-feature").is_file());

        archive(&c, "my-feature", true, false, false).unwrap();

        assert!(!touched::touched_path(&c, "my-feature").exists());
        assert!(!touched::baseline_path(&c, "my-feature").exists());
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
        // Validation hit the permission error after freezing; rollback restores the active name.
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
    fn archive_preserves_the_error_cause_from_a_frozen_source_read_failure() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new();
        let c = cfg(&tmp);
        change::create(&c, "my-feature").unwrap();
        let meta_path = c.changes_dir().join("my-feature").join(".openspec.yaml");
        std::fs::set_permissions(&meta_path, std::fs::Permissions::from_mode(0o000)).unwrap();

        if !permission_denied_is_constructible(&meta_path) {
            eprintln!(
                "skipping archive_preserves_the_error_cause_from_a_frozen_source_read_failure: \
                 running as root (chmod 0o000 not enforced)"
            );
            std::fs::set_permissions(&meta_path, std::fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }

        let err = archive(&c, "my-feature", true, false, false).unwrap_err();
        std::fs::set_permissions(&meta_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        // The underlying I/O cause must survive the fingerprinting and
        // rollback context rather than being flattened away.
        let full_chain = format!("{err:#}");
        assert!(
            full_chain.to_lowercase().contains("permission denied"),
            "expected the permission-denied cause in the error chain, got: {full_chain}"
        );
    }

    // ---- #179 ----

    #[test]
    fn archive_refuses_a_pointer_whose_sidecar_is_missing() {
        // spec.md 有指標、sidecar 卻不見了（例如沒被 git add）：不能從空白重建，
        // 否則舊紀錄就此遺失。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap/spec.md");
        let spec = format!(
            "# my-cap Specification\n\n{}\n\n## Purpose\n\nP.\n\n## Requirements\n\n\
             ### Requirement: Existing\nold text\n",
            crate::trace::POINTER
        );
        write(&spec_path, &spec);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## MODIFIED Requirements\n\n### Requirement: Existing\nnew text\n",
        );

        let error = archive(&c, "my-feature", false, false, false).unwrap_err();

        assert!(
            format!("{error:#}").contains(crate::trace::SIDECAR_FILE),
            "{error:#}"
        );
        assert_eq!(fs::read_to_string(&spec_path).unwrap(), spec);
        assert!(!crate::trace::sidecar_path(&spec_path).exists());
        assert!(c.changes_dir().join("my-feature").is_dir());
    }

    #[test]
    fn archive_absorbs_a_footer_with_a_nonstandard_opener_before_removing_its_requirement() {
        // `<!--@trace`（沒有空白）以前完全不被偵測：REMOVED 會把它連同
        // requirement 一起丟掉，也不警告（#179）。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap/spec.md");
        write(
            &spec_path,
            "# my-cap Specification\n\n## Purpose\n\nP.\n\n## Requirements\n\n\
             ### Requirement: Alpha\nThe system SHALL alpha.\n\n\
             <!--@trace\nsource: oracle-change\nupdated: 2026-09-01\ncode:\n  - a.rs\n-->\n\n\
             ### Requirement: Beta\nThe system SHALL beta.\n",
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## REMOVED Requirements\n\n### Requirement: Alpha\n",
        );

        archive(&c, "my-feature", false, false, false).unwrap();

        let trace = read_trace(&c, "my-cap");
        assert_eq!(trace.traces[0].source, "oracle-change");
        assert_eq!(trace.traces[0].imported, vec!["Alpha"]);
        assert_eq!(trace.traces[0].code, vec!["a.rs"]);
        assert_eq!(trace.traces[1].removed, vec!["Alpha"]);
    }

    #[cfg(unix)]
    #[test]
    fn archive_rolls_back_the_sidecar_of_a_retired_capability() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("a-cap/spec.md");
        let spec = "# a-cap Specification\n\n## Purpose\n\nP.\n\n## Requirements\n\n\
                    ### Requirement: Only\nThe system SHALL exist.\n";
        write(&spec_path, spec);
        let sidecar = crate::trace::sidecar_path(&spec_path);
        let sidecar_content =
            "version: 1\ntraces:\n- source: s\n  updated: d\n  added:\n  - Only\n  code:\n  - x.rs\n";
        write(&sidecar, sidecar_content);
        change::create(&c, "retire").unwrap();
        write(
            &c.changes_dir().join("retire/.openspec.yaml"),
            "schema: spec-driven\nretire_capabilities: true\n",
        );
        write(
            &c.changes_dir().join("retire/specs/a-cap/spec.md"),
            "## REMOVED Requirements\n\n### Requirement: Only\n",
        );
        write(
            &c.changes_dir().join("retire/specs/z-last/spec.md"),
            DELTA_TEMPLATE,
        );
        let blocked = c.specs_dir().join("z-last");
        fs::create_dir_all(&blocked).unwrap();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o555)).unwrap();

        let result = archive(&c, "retire", false, false, false);
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755)).unwrap();

        result.unwrap_err();
        assert_eq!(fs::read_to_string(&spec_path).unwrap(), spec);
        assert_eq!(fs::read_to_string(&sidecar).unwrap(), sidecar_content);
    }

    #[test]
    fn commit_refuses_a_sidecar_changed_since_it_was_prepared() {
        let tmp = TempDir::new();
        let sidecar = tmp.join("my-cap").join(crate::trace::SIDECAR_FILE);
        write(&sidecar, "concurrent edit\n");
        let prepared = [PreparedSpec {
            path: sidecar.clone(),
            original: Some(b"prepared from this\n".to_vec()),
            content: Some("new sidecar\n".to_string()),
            result: None,
            retire: false,
        }];

        let error = commit_prepared_specs(&prepared).unwrap_err();

        assert!(format!("{error:#}").contains("changed while archive was preparing"));
        assert_eq!(fs::read_to_string(&sidecar).unwrap(), "concurrent edit\n");
    }

    // ---- PR #180 review：no-op 比對與 footer 偵測用同一條 opener 規則 ----

    const ALPHA_SPEC: &str = "# my-cap Specification\n\n## Purpose\n\nP.\n\n## Requirements\n\n\
                              ### Requirement: Alpha\nThe system SHALL alpha.\n";
    const NONSTANDARD_FOOTER: &str = "<!--@trace\nsource: x\nupdated: y\ncode: []\n-->\n";

    #[test]
    fn archive_treats_an_identical_added_requirement_with_a_nonstandard_footer_as_a_noop() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap/spec.md");
        write(&spec_path, ALPHA_SPEC);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            &format!(
                "## ADDED Requirements\n\n### Requirement: Alpha\nThe system SHALL alpha.\n\n{NONSTANDARD_FOOTER}"
            ),
        );

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

        assert_eq!(outcome.specs_applied[0].added, 0);
        assert_eq!(fs::read_to_string(&spec_path).unwrap(), ALPHA_SPEC);
    }

    #[test]
    fn archive_treats_an_identical_modified_requirement_with_a_nonstandard_footer_as_a_noop() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap/spec.md");
        write(&spec_path, ALPHA_SPEC);
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            &format!(
                "## MODIFIED Requirements\n\n### Requirement: Alpha\nThe system SHALL alpha.\n\n{NONSTANDARD_FOOTER}"
            ),
        );

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

        assert_eq!(outcome.specs_applied[0].modified, 0);
        assert_eq!(fs::read_to_string(&spec_path).unwrap(), ALPHA_SPEC);
        assert!(
            !crate::trace::sidecar_path(&spec_path).exists(),
            "a no-op must not leave a sidecar entry"
        );
    }

    #[test]
    fn archive_applies_a_modified_requirement_that_changes_text_after_a_single_line_footer() {
        // 開頭行帶內容的 footer（含單行 `<!-- @trace ... -->`）無法解析，不是多行
        // footer 的開頭：它後面的內容仍要參與比對，否則之後的改動會被當成 no-op
        // 而靜默丟掉。
        let requirement = |detail: &str| {
            format!(
                "### Requirement: Alpha\nThe system SHALL alpha.\n\n<!-- @trace source: x -->\n\n{detail}\n"
            )
        };
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap/spec.md");
        write(
            &spec_path,
            &format!(
                "# my-cap Specification\n\n## Purpose\n\nP.\n\n## Requirements\n\n{}",
                requirement("Old detail.")
            ),
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            &format!("## MODIFIED Requirements\n\n{}", requirement("New detail.")),
        );

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

        assert_eq!(outcome.specs_applied[0].modified, 1);
        let spec = fs::read_to_string(&spec_path).unwrap();
        assert!(spec.contains("New detail."), "{spec}");
        assert!(!spec.contains("Old detail."), "{spec}");
    }

    #[test]
    fn archive_applies_a_modified_requirement_that_only_changes_a_code_example() {
        // 縮排 code block 或 code fence 裡的 footer 範例不是 footer：只改範例的
        // MODIFIED 要真的套用，不能被當成「內容相同」而靜默丟掉。
        // `SOURCE` 會換成實際的 source 值。
        let forms = [
            (
                "indented",
                "    <!-- @trace\n    source: SOURCE\n    updated: d\n    code: []\n    -->\n",
            ),
            (
                "fenced",
                "```\n<!-- @trace\nsource: SOURCE\nupdated: d\ncode: []\n-->\n```\n",
            ),
        ];
        for (form, example) in forms {
            let requirement = |source: &str| {
                format!(
                    "### Requirement: Alpha\nThe system SHALL alpha.\n\nFooter example:\n\n{}",
                    example.replace("SOURCE", source)
                )
            };
            let tmp = TempDir::new();
            let c = cfg(&tmp);
            let spec_path = c.specs_dir().join("my-cap/spec.md");
            write(
                &spec_path,
                &format!(
                    "# my-cap Specification\n\n## Purpose\n\nP.\n\n## Requirements\n\n{}",
                    requirement("old-example")
                ),
            );
            change::create(&c, "my-feature").unwrap();
            write(
                &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
                &format!("## MODIFIED Requirements\n\n{}", requirement("new-example")),
            );

            let outcome = archive(&c, "my-feature", false, false, false).unwrap();

            assert_eq!(outcome.specs_applied[0].modified, 1, "{form}");
            let spec = fs::read_to_string(&spec_path).unwrap();
            assert!(spec.contains("source: new-example"), "{form}: {spec}");
            assert!(!spec.contains("old-example"), "{form}: {spec}");
        }
    }

    #[test]
    fn archive_does_not_treat_an_indented_pointer_example_as_a_missing_sidecar() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap/spec.md");
        write(
            &spec_path,
            &format!(
                "# my-cap Specification\n\n## Purpose\n\nThe pointer looks like:\n\n    {}\n\n\
                 ## Requirements\n\n### Requirement: Alpha\nThe system SHALL alpha.\n",
                crate::trace::POINTER
            ),
        );
        change::create(&c, "my-feature").unwrap();
        write(
            &c.changes_dir().join("my-feature/specs/my-cap/spec.md"),
            "## ADDED Requirements\n\n### Requirement: Beta\nThe system SHALL beta.\n",
        );

        let outcome = archive(&c, "my-feature", false, false, false).unwrap();

        assert_eq!(outcome.specs_applied[0].added, 1);
        let spec = fs::read_to_string(&spec_path).unwrap();
        assert!(
            spec.split('\n').any(|line| line == crate::trace::POINTER),
            "a real pointer must be inserted:\n{spec}"
        );
        assert!(crate::trace::sidecar_path(&spec_path).exists());
    }

    #[test]
    fn validation_rejects_a_pointer_whose_sidecar_is_missing() {
        // `spectra validate`（dry_run）走同一個相容性檢查；有指標、沒有 sidecar
        // 要在這裡就失敗，不能只在實際 archive 時才擋（#179 AC-4）。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let spec_path = c.specs_dir().join("my-cap/spec.md");
        write(
            &spec_path,
            &format!(
                "# my-cap Specification\n\n{}\n\n## Purpose\n\nP.\n\n## Requirements\n\n\
                 ### Requirement: Alpha\nThe system SHALL alpha.\n",
                crate::trace::POINTER
            ),
        );
        change::create(&c, "my-feature").unwrap();
        let change_dir = c.changes_dir().join("my-feature");
        write(
            &change_dir.join("specs/my-cap/spec.md"),
            "## ADDED Requirements\n\n### Requirement: Beta\nThe system SHALL beta.\n",
        );

        let error =
            validate_archive_compatibility(&c, &change_dir, "my-feature", false).unwrap_err();

        assert!(
            format!("{error:#}").contains(crate::trace::SIDECAR_FILE),
            "{error:#}"
        );
    }
}
