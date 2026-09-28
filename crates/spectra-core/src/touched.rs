//! `.spectra/touched/<name>.json` 與 `.spectra/task-baselines/<name>/<id>.json`：
//! oracle 3.0.0 的 per-task touched-file tracking（#190；D7 裁決以它取代 OpenSpectra
//! 原本的 per-change baseline #98）。
//!
//! - `task start` 為一個 task 擷取 baseline 指紋；touched 檔還不存在時一併建立，並記下
//!   `review_base`（當下的 HEAD 與 dirty 指紋），之後不再更新。
//! - `task done` 只記錄自該 task 的 baseline 以來有變動的路徑（`task_baseline`），或
//!   `--file` 明確列出的路徑（`explicit_files`）；兩者皆無時不記錄。
//! - 兩個檔都用 serde_json pretty 格式、結尾不換行；`.spectra/touched/<name>.lock`
//!   是 0 byte 的檔案，建立後從不刪除。
//!
//! 格式與規則的實證見 `docs/reverse-engineering/task.md`。

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::fingerprint::FileFingerprint;

pub const PROVENANCE_TASK_BASELINE: &str = "task_baseline";
pub const PROVENANCE_EXPLICIT_FILES: &str = "explicit_files";

/// 舊版 OpenSpectra 把 `task_id` 寫成數字；讀取時兩種都接受，寫出一律是字串。
fn task_id_string<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<String, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        Text(String),
        Number(u64),
    }
    Ok(match Id::deserialize(d)? {
        Id::Text(s) => s,
        Id::Number(n) => n.to_string(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TouchedEntry {
    #[serde(deserialize_with = "task_id_string")]
    pub task_id: String,
    pub task_desc: String,
    pub files: Vec<String>,
    /// oracle 3.0.0 的每筆條目都有；舊版 OpenSpectra 的條目沒有，原樣保留。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewBase {
    pub head_revision: Option<String>,
    pub dirty_fingerprints: Vec<FileFingerprint>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TouchedTracking {
    pub change: String,
    #[serde(default)]
    pub touched: Vec<TouchedEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_base: Option<ReviewBase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskBaseline {
    pub change: String,
    pub task_id: String,
    pub fingerprints: Vec<FileFingerprint>,
}

pub(crate) fn touched_path(cfg: &Config, name: &str) -> PathBuf {
    cfg.root
        .join(".spectra")
        .join("touched")
        .join(format!("{name}.json"))
}

pub(crate) fn lock_path(cfg: &Config, name: &str) -> PathBuf {
    cfg.root
        .join(".spectra")
        .join("touched")
        .join(format!("{name}.lock"))
}

pub(crate) fn baselines_dir(cfg: &Config, name: &str) -> PathBuf {
    cfg.root.join(".spectra").join("task-baselines").join(name)
}

pub(crate) fn baseline_path(cfg: &Config, name: &str, task_id: &str) -> PathBuf {
    baselines_dir(cfg, name).join(format!("{task_id}.json"))
}

/// 建立 0 byte 的 lock 檔（已存在則不動）。oracle 從不刪除它，連 archive 也會留下。
pub fn touch_lock(cfg: &Config, name: &str) -> Result<()> {
    let path = lock_path(cfg, name);
    let parent = path.parent().expect("lock path always has a parent");
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("Cannot open tracking lock: {}", path.display()))?;
    Ok(())
}

/// 讀取 tracking 檔：不存在回 `None`；無法解析或 change 名稱不符時失敗（oracle 的訊息）。
pub fn load_strict(cfg: &Config, name: &str) -> Result<Option<TouchedTracking>> {
    let path = touched_path(cfg, name);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let tracking: TouchedTracking = serde_json::from_str(&text)
        .map_err(|e| anyhow!("Failed to parse touched tracking {}: {e}", path.display()))?;
    if tracking.change != name {
        anyhow::bail!(
            "Touched tracking belongs to \"{}\", expected \"{name}\"",
            tracking.change
        );
    }
    Ok(Some(tracking))
}

/// pretty JSON、結尾不換行（與 oracle 逐位元組相同）。
fn write_pretty<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().expect("tracking paths always have a parent");
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))
}

pub fn persist(cfg: &Config, tracking: &TouchedTracking) -> Result<()> {
    write_pretty(&touched_path(cfg, &tracking.change), tracking)
}

/// 讀取某個 task 的 baseline；不存在回 `None`，壞掉時失敗。
pub fn load_baseline(cfg: &Config, name: &str, task_id: &str) -> Result<Option<TaskBaseline>> {
    let path = baseline_path(cfg, name, task_id);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| anyhow!("Failed to parse task baseline {}: {e}", path.display()))
}

pub fn baseline_exists(cfg: &Config, name: &str, task_id: &str) -> bool {
    baseline_path(cfg, name, task_id).exists()
}

pub fn write_baseline(cfg: &Config, baseline: &TaskBaseline) -> Result<()> {
    write_pretty(
        &baseline_path(cfg, &baseline.change, &baseline.task_id),
        baseline,
    )
}

pub fn remove_baseline(cfg: &Config, name: &str, task_id: &str) -> Result<()> {
    let path = baseline_path(cfg, name, task_id);
    match std::fs::remove_file(&path) {
        Err(e) if !is_gone(&e) => Err(e).with_context(|| format!("removing {}", path.display())),
        _ => Ok(()),
    }
}

/// `<name>.<ext>.corrupt`, or `<name>.<ext>.corrupt.2`, `.3`, ... if that's
/// already taken — so a second (or third...) corruption event doesn't
/// silently clobber the backup of a previous one via `rename`'s
/// overwrite-the-destination semantics. Used by `archive`'s
/// `stamp_archived_metadata`, which backs up an unparseable `.openspec.yaml`
/// before overwriting it.
pub(crate) fn non_colliding_backup_path(path: &Path) -> PathBuf {
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

/// 這個 change 所有條目記錄過的路徑。唯讀、容錯：讀不到或壞掉都當成空的，
/// 供 archive 在可能回滾的交易中使用。
pub fn already_recorded_readonly(cfg: &Config, name: &str) -> HashSet<String> {
    std::fs::read_to_string(touched_path(cfg, name))
        .ok()
        .and_then(|s| serde_json::from_str::<TouchedTracking>(&s).ok())
        .map(|t| t.touched.into_iter().flat_map(|e| e.files).collect())
        .unwrap_or_default()
}

pub(crate) fn is_gone(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory)
}

/// 測試用：直接附加一筆條目（`explicit_files`）。
#[cfg(test)]
pub fn record(
    cfg: &Config,
    name: &str,
    task_id: usize,
    task_desc: &str,
    files: Vec<String>,
) -> Result<()> {
    let mut tracking = load_strict(cfg, name)?.unwrap_or_else(|| TouchedTracking {
        change: name.to_string(),
        ..Default::default()
    });
    tracking.touched.push(TouchedEntry {
        task_id: task_id.to_string(),
        task_desc: task_desc.to_string(),
        files,
        provenance: Some(PROVENANCE_EXPLICIT_FILES.to_string()),
    });
    persist(cfg, &tracking)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(root: &Path) -> Config {
        Config {
            root: root.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        }
    }

    #[test]
    fn tracking_round_trips_the_oracle_bytes_and_reads_legacy_numeric_ids() {
        let oracle = "{\n  \"change\": \"demo\",\n  \"touched\": [\n    {\n      \"task_id\": \"1\",\n      \"task_desc\": \"1.1 first\",\n      \"files\": [\n        \"src/a.rs\"\n      ],\n      \"provenance\": \"task_baseline\"\n    }\n  ],\n  \"review_base\": {\n    \"head_revision\": \"abc\",\n    \"dirty_fingerprints\": [\n      {\n        \"path\": \"src/old.rs\",\n        \"status\": 128,\n        \"index_identity\": null,\n        \"worktree_identity\": \"def\"\n      }\n    ]\n  }\n}";
        let parsed: TouchedTracking = serde_json::from_str(oracle).unwrap();
        assert_eq!(serde_json::to_string_pretty(&parsed).unwrap(), oracle);

        let legacy = r#"{"change":"demo","touched":[{"task_id":3,"task_desc":"t","files":["a"]}]}"#;
        let parsed: TouchedTracking = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.touched[0].task_id, "3");
        assert!(parsed.review_base.is_none());
        assert_eq!(
            serde_json::to_string(&parsed).unwrap(),
            r#"{"change":"demo","touched":[{"task_id":"3","task_desc":"t","files":["a"]}]}"#
        );
    }

    #[test]
    fn load_strict_rejects_a_foreign_or_corrupt_tracking_file() {
        let dir =
            std::env::temp_dir().join(format!("spectra-touched-strict-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let c = cfg(&dir);
        assert!(load_strict(&c, "demo").unwrap().is_none());
        std::fs::create_dir_all(dir.join(".spectra/touched")).unwrap();
        std::fs::write(
            touched_path(&c, "demo"),
            r#"{"change":"other","touched":[]}"#,
        )
        .unwrap();
        assert_eq!(
            load_strict(&c, "demo").unwrap_err().to_string(),
            "Touched tracking belongs to \"other\", expected \"demo\""
        );
        std::fs::write(touched_path(&c, "demo"), "{bogus").unwrap();
        assert!(load_strict(&c, "demo")
            .unwrap_err()
            .to_string()
            .starts_with("Failed to parse touched tracking "));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
