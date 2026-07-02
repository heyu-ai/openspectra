//! `.spectra/touched/<name>.json` — per-task file-touch tracking written by
//! `spectra task done`, read by the (AI-agent-only) `/spectra:commit` skill to
//! group a commit's dirty files by the task that produced them.
//!
//! JSON schema (reverse-engineered against `/Applications/Spectra.app`
//! v2.3.1's bundled `/spectra:commit` skill doc):
//! ```json
//! {
//!   "change": "<change-name>",
//!   "touched": [
//!     { "task_id": "1", "task_desc": "Task description", "files": ["src/file1.ts"] }
//!   ]
//! }
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::PathBuf;

use crate::config::Config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TouchedEntry {
    pub task_id: String,
    pub task_desc: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TouchedTracking {
    pub change: String,
    #[serde(default)]
    pub touched: Vec<TouchedEntry>,
}

pub(crate) fn touched_path(cfg: &Config, name: &str) -> PathBuf {
    cfg.root
        .join(".spectra")
        .join("touched")
        .join(format!("{name}.json"))
}

/// Load the existing tracking file for `name`, or an empty one if it's
/// absent — a missing file is the expected first-run state. Two other cases
/// are loud instead of silent, since resetting either one in place and then
/// having a later write overwrite it would permanently discard prior
/// task→file history with no trace or recovery path:
/// - present but unreadable for a reason other than absence (permission
///   denied, etc.) — warns and starts empty rather than silently masking a
///   real I/O problem as "no history yet";
/// - present but unparseable (corruption, a partial write, a future schema)
///   — renamed aside to `<name>.json.corrupt` (so the original bytes survive
///   for inspection/recovery) before being treated as empty.
fn load(cfg: &Config, name: &str) -> TouchedTracking {
    let path = touched_path(cfg, name);
    let empty = || TouchedTracking {
        change: name.to_string(),
        touched: Vec::new(),
    };
    match std::fs::read_to_string(&path) {
        Err(e) if e.kind() == ErrorKind::NotFound => empty(),
        Err(e) => {
            eprintln!(
                "warning: couldn't read {} ({e}); starting fresh touched-file tracking for '{name}'",
                path.display()
            );
            empty()
        }
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
            let backup = non_colliding_backup_path(&path);
            let recovery_hint = match std::fs::rename(&path, &backup) {
                Ok(()) => format!("the original file was preserved at {}", backup.display()),
                Err(rename_err) => format!("failed to preserve the original file too ({rename_err})"),
            };
            eprintln!(
                "warning: {} is corrupt ({e}); resetting touched-file tracking for '{name}' -- {recovery_hint}",
                path.display()
            );
            empty()
        }),
    }
}

/// `<name>.<ext>.corrupt`, or `<name>.<ext>.corrupt.2`, `.3`, ... if that's
/// already taken — so a second (or third...) corruption event doesn't
/// silently clobber the backup of a previous one via `rename`'s
/// overwrite-the-destination semantics. Shared with `archive`'s
/// `stamp_archived_metadata`, which backs up an unparseable `.openspec.yaml`
/// the same way before overwriting it.
pub(crate) fn non_colliding_backup_path(path: &std::path::Path) -> PathBuf {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("bak");
    let base = path.with_extension(format!("{ext}.corrupt"));
    if !base.exists() {
        return base;
    }
    (2..)
        .map(|n| PathBuf::from(format!("{}.{n}", base.display())))
        .find(|p| !p.exists())
        .expect("infinite range")
}

/// File paths already recorded against any task for this change, across all
/// existing entries — used so a file isn't attributed to more than one task.
pub fn already_recorded(cfg: &Config, name: &str) -> HashSet<String> {
    load(cfg, name)
        .touched
        .into_iter()
        .flat_map(|e| e.files)
        .collect()
}

fn persist(cfg: &Config, name: &str, tracking: &TouchedTracking) -> Result<()> {
    let path = touched_path(cfg, name);
    let parent = path.parent().expect("touched path always has a parent");
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let json = serde_json::to_string_pretty(tracking)?;
    std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Append a new entry for `task_id`/`task_desc`/`files`, creating the
/// tracking dir/file if needed. A no-op when `files` is empty (matches the
/// reference CLI: no tracking file is written when a task touched nothing).
pub fn record(
    cfg: &Config,
    name: &str,
    task_id: usize,
    task_desc: &str,
    files: Vec<String>,
) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let mut tracking = load(cfg, name);
    tracking.change = name.to_string();
    tracking.touched.push(TouchedEntry {
        task_id: task_id.to_string(),
        task_desc: task_desc.to_string(),
        files,
    });
    persist(cfg, name, &tracking)
}

/// Combines [`already_recorded`] and [`record`] into a single load: given
/// `candidate_files`, filters out anything already recorded against an
/// earlier task and persists the rest as a new entry (a no-op if nothing's
/// left). Loading the tracking file only once here (instead of the two
/// independent loads the split functions would require) means a corrupt
/// tracking file only warns once per `task done` call, not twice.
pub fn record_new(
    cfg: &Config,
    name: &str,
    task_id: usize,
    task_desc: &str,
    candidate_files: Vec<String>,
) -> Result<()> {
    let mut tracking = load(cfg, name);
    let already: HashSet<&str> = tracking
        .touched
        .iter()
        .flat_map(|e| e.files.iter().map(String::as_str))
        .collect();
    let new_files: Vec<String> = candidate_files
        .into_iter()
        .filter(|f| !already.contains(f.as_str()))
        .collect();
    if new_files.is_empty() {
        return Ok(());
    }
    tracking.change = name.to_string();
    tracking.touched.push(TouchedEntry {
        task_id: task_id.to_string(),
        task_desc: task_desc.to_string(),
        files: new_files,
    });
    persist(cfg, name, &tracking)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "spectra-touched-test-{}-{seq}-{}",
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

    fn cfg(tmp: &TempDir) -> Config {
        Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        }
    }

    #[test]
    fn record_is_a_noop_when_files_is_empty() {
        let tmp = TempDir::new();
        record(&cfg(&tmp), "my-change", 1, "desc", Vec::new()).unwrap();
        assert!(!touched_path(&cfg(&tmp), "my-change").exists());
    }

    #[test]
    fn record_writes_and_accumulates_entries() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        record(&c, "my-change", 1, "first task", vec!["a.rs".to_string()]).unwrap();
        record(
            &c,
            "my-change",
            2,
            "second task",
            vec!["b.rs".to_string(), "c.rs".to_string()],
        )
        .unwrap();

        let tracking = load(&c, "my-change");
        assert_eq!(tracking.change, "my-change");
        assert_eq!(tracking.touched.len(), 2);
        assert_eq!(tracking.touched[0].task_id, "1");
        assert_eq!(tracking.touched[0].files, vec!["a.rs".to_string()]);
        assert_eq!(tracking.touched[1].task_id, "2");
        assert_eq!(
            tracking.touched[1].files,
            vec!["b.rs".to_string(), "c.rs".to_string()]
        );
    }

    #[test]
    fn already_recorded_collects_files_across_all_entries() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        record(&c, "my-change", 1, "t1", vec!["a.rs".to_string()]).unwrap();
        record(&c, "my-change", 2, "t2", vec!["b.rs".to_string()]).unwrap();

        let recorded = already_recorded(&c, "my-change");
        assert!(recorded.contains("a.rs"));
        assert!(recorded.contains("b.rs"));
        assert_eq!(recorded.len(), 2);
    }

    #[test]
    fn already_recorded_is_empty_when_no_tracking_file_exists() {
        let tmp = TempDir::new();
        assert!(already_recorded(&cfg(&tmp), "no-such-change").is_empty());
    }

    #[test]
    fn record_new_filters_out_already_recorded_files_with_a_single_load() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        record(&c, "my-change", 1, "t1", vec!["a.rs".to_string()]).unwrap();

        record_new(
            &c,
            "my-change",
            2,
            "t2",
            vec!["a.rs".to_string(), "b.rs".to_string()],
        )
        .unwrap();

        let tracking = load(&c, "my-change");
        assert_eq!(tracking.touched.len(), 2);
        // Only the genuinely-new file (b.rs) is attributed to task 2; a.rs
        // stays attributed to task 1, not duplicated or reattributed.
        assert_eq!(tracking.touched[1].task_id, "2");
        assert_eq!(tracking.touched[1].files, vec!["b.rs".to_string()]);
    }

    #[test]
    fn record_new_is_a_noop_when_every_candidate_is_already_recorded() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        record(&c, "my-change", 1, "t1", vec!["a.rs".to_string()]).unwrap();

        record_new(&c, "my-change", 2, "t2", vec!["a.rs".to_string()]).unwrap();

        assert_eq!(load(&c, "my-change").touched.len(), 1);
    }

    #[test]
    fn load_backs_up_a_corrupt_tracking_file_instead_of_discarding_it() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let path = touched_path(&c, "my-change");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not valid json").unwrap();

        let tracking = load(&c, "my-change");

        assert!(tracking.touched.is_empty());
        let backup = path.with_extension("json.corrupt");
        assert!(
            backup.is_file(),
            "corrupt file should be preserved at {}",
            backup.display()
        );
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "not valid json");
        assert!(
            !path.exists(),
            "the corrupt path itself should have been renamed away"
        );
    }

    #[test]
    fn load_does_not_clobber_a_prior_corrupt_backup_on_a_second_corruption() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let path = touched_path(&c, "my-change");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        std::fs::write(&path, "first corruption").unwrap();
        load(&c, "my-change"); // creates <name>.json.corrupt

        std::fs::write(&path, "second corruption").unwrap();
        load(&c, "my-change"); // must not overwrite the first backup

        let first_backup = path.with_extension("json.corrupt");
        let second_backup = PathBuf::from(format!("{}.2", first_backup.display()));
        assert_eq!(
            std::fs::read_to_string(&first_backup).unwrap(),
            "first corruption"
        );
        assert_eq!(
            std::fs::read_to_string(&second_backup).unwrap(),
            "second corruption"
        );
    }

    /// After chmod(0o000), root (or a container with CAP_DAC_OVERRIDE) can
    /// still read the file, so the permission-denied scenario this test needs
    /// is unconstructible; skip rather than fail in that case.
    #[cfg(unix)]
    fn permission_denied_is_constructible(path: &std::path::Path) -> bool {
        std::fs::read(path).is_err()
    }

    #[cfg(unix)]
    #[test]
    fn load_warns_but_does_not_panic_on_a_permission_denied_read() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let path = touched_path(&c, "my-change");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        if !permission_denied_is_constructible(&path) {
            eprintln!(
                "skipping load_warns_but_does_not_panic_on_a_permission_denied_read: \
                 running as root (chmod 0o000 not enforced)"
            );
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }

        let tracking = load(&c, "my-change");

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(tracking.touched.is_empty());
    }
}
