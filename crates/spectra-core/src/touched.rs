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

fn touched_path(cfg: &Config, name: &str) -> PathBuf {
    cfg.root
        .join(".spectra")
        .join("touched")
        .join(format!("{name}.json"))
}

/// Load the existing tracking file for `name`, or an empty one if it's
/// absent — a missing file is the expected first-run state. A *present but
/// unparseable* file (corruption, a partial write, a future schema) is a
/// different situation: silently treating it as empty and then having
/// `record` overwrite it would permanently discard prior task→file history
/// with no trace, so that case is loud instead.
fn load(cfg: &Config, name: &str) -> TouchedTracking {
    let path = touched_path(cfg, name);
    match std::fs::read_to_string(&path) {
        Err(_) => TouchedTracking {
            change: name.to_string(),
            touched: Vec::new(),
        },
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
            eprintln!(
                "warning: {} is corrupt ({e}); resetting touched-file tracking for '{name}' \
                 -- prior touched-file history for this change may be lost",
                path.display()
            );
            TouchedTracking {
                change: name.to_string(),
                touched: Vec::new(),
            }
        }),
    }
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

    let path = touched_path(cfg, name);
    let parent = path.parent().expect("touched path always has a parent");
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let json = serde_json::to_string_pretty(&tracking)?;
    std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
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
}
