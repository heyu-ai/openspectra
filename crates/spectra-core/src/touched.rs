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

/// Read-only variant of [`already_recorded`] that never renames or mutates
/// the tracking file. Safe to call during a transaction that may roll back.
pub fn already_recorded_readonly(cfg: &Config, name: &str) -> HashSet<String> {
    load_readonly(cfg, name)
        .touched
        .into_iter()
        .flat_map(|e| e.files)
        .collect()
}

fn load_readonly(cfg: &Config, name: &str) -> TouchedTracking {
    let path = touched_path(cfg, name);
    let empty = || TouchedTracking {
        change: name.to_string(),
        touched: Vec::new(),
    };
    match std::fs::read_to_string(&path) {
        Err(_) => empty(),
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| empty()),
    }
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

/// `.spectra/changes/<name>.touched-baseline.json` — OpenSpectra-only
/// sidecar（oracle 沒有），記錄「上一個檢查點」時每個 dirty 檔案的內容指紋。
/// 檢查點是 `spectra new change`（`change::create`）與每次成功記錄的
/// `task done`；下一次 `task done` 只把指紋與檢查點不同（或檢查點時還不
/// dirty）的檔案算成這個 task 的 touched file。
///
/// 這修的是 oracle v2.3.1 的 session-wide 過度收集（kaochenlong/spectra-app#95、
/// heyu-ai/openspectra#98）：change 開始前就已經 dirty、之後沒再被改過的
/// 無關檔案，不該被灌進 archive 的 `@trace` `code:` 清單。oracle 3.0.0 的
/// `task_baseline` 行為尚未實測。放在 `.spectra/changes/`（`.started` 旁邊）
/// 而不是 `.spectra/touched/`，是讓後者只放 oracle 格式的 tracking 檔。
pub(crate) fn baseline_path(cfg: &Config, name: &str) -> PathBuf {
    cfg.root
        .join(".spectra")
        .join("changes")
        .join(format!("{name}.touched-baseline.json"))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Baseline {
    #[serde(default)]
    /// `None`（JSON `null`）是「檢查點時 dirty，但指紋無法判定」：路徑仍要記下，
    /// 它之後若變乾淨才會被列為候選；比較時一律算「有變」。
    files: std::collections::BTreeMap<String, Option<String>>,
}

/// 所有 dirty 檔案在某一刻的指紋，依 `git status` 的順序。每個 `task done`
/// 只算一次，同時拿來比較與寫成下一個檢查點。
pub struct Snapshot(Vec<(String, Option<String>)>);

/// 檔案目前狀態的指紋：一般檔案是長度加內容的 FNV-1a 64，symlink 是它的
/// 目標，不存在（已刪除）是 `"missing"`。無法判定狀態時（讀不到內容、讀不到
/// symlink 目標、目錄或 submodule 這類非一般檔案、`NotFound` 以外的 stat 錯誤）
/// 回傳 `None`：baseline 以 `null` 記下這個路徑，比較時一律算「有變」——寧可
/// 多記也不要因為兩次都「不知道」就判定沒變而靜默漏記。代價是 change 開始前
/// 就 dirty、之後沒被碰過的 submodule（`git status` 回報成目錄）也會被記進
/// 第一個 task；要精準判定得對 submodule 另跑 git，列為已知限制。
///
/// 不用 `DefaultHasher`：它的演算法不保證跨 Rust 版本穩定，而 baseline 會跨
/// binary 升級保存。碰撞的後果是某個真的被改過的檔案漏記，這份資料本來就是
/// best-effort，可以接受。
fn fingerprint(root: &std::path::Path, rel: &str) -> Option<String> {
    let path = root.join(rel);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if is_gone(&e) => return Some("missing".to_string()),
        Err(_) => return None,
    };
    if meta.file_type().is_symlink() {
        return std::fs::read_link(&path)
            .ok()
            .map(|target| format!("symlink:{}", target.display()));
    }
    if !meta.is_file() {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in &bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(format!("{}:{hash:016x}", bytes.len()))
}

/// stat 錯誤是否代表路徑已不存在。`NotADirectory` 是某個上層目錄已被換成
/// 一般檔案，路徑同樣不存在；權限不足等其他錯誤不代表檔案消失了。
/// 與 `archive` 剔除失效路徑共用，兩邊對「不存在」的判定才不會分岔。
pub(crate) fn is_gone(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory)
}

/// 目前所有 dirty 檔案的指紋。
pub fn snapshot(cfg: &Config, dirty: &[String]) -> Snapshot {
    Snapshot(
        dirty
            .iter()
            .map(|f| (f.clone(), fingerprint(&cfg.root, f)))
            .collect(),
    )
}

/// 把 `snapshot` 寫成新的檢查點（無法判定指紋的路徑記為 `null`）。
pub fn write_baseline(cfg: &Config, name: &str, snapshot: &Snapshot) -> Result<()> {
    let baseline = Baseline {
        files: snapshot
            .0
            .iter()
            .map(|(f, fp)| (f.clone(), fp.clone()))
            .collect(),
    };
    let path = baseline_path(cfg, name);
    let parent = path.parent().expect("baseline path always has a parent");
    std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    let json = serde_json::to_string_pretty(&baseline)?;
    std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// 這個 task 的 touched file：`current` 裡的 dirty 檔案，加上檢查點時 dirty、
/// 現在已經乾淨的檔案（例如被改回 commit 的內容），先以 `is_candidate` 排除
/// change 目錄這類不算的路徑，再只留下指紋與檢查點不同的。
///
/// 沒有 baseline（這個功能上線前建立的 change）、baseline 讀不到或無法解析時，
/// 回傳 `current` 裡所有符合 `is_candidate` 的檔案——也就是舊的 session-wide
/// 行為；後兩者代表檔案出了問題，會發出警告。
pub fn touched_since_baseline(
    cfg: &Config,
    name: &str,
    current: &Snapshot,
    is_candidate: impl Fn(&str) -> bool,
) -> Vec<String> {
    let every_dirty = || {
        current
            .0
            .iter()
            .map(|(f, _)| f.clone())
            .filter(|f| is_candidate(f))
            .collect()
    };
    let path = baseline_path(cfg, name);
    let baseline: Baseline = match std::fs::read_to_string(&path) {
        Err(e) if e.kind() == ErrorKind::NotFound => return every_dirty(),
        Err(e) => {
            eprintln!(
                "warning: couldn't read {} ({e}); recording every dirty file for '{name}'",
                path.display()
            );
            return every_dirty();
        }
        Ok(s) => match serde_json::from_str(&s) {
            Ok(b) => b,
            Err(e) => {
                eprintln!(
                    "warning: {} is corrupt ({e}); recording every dirty file for '{name}'",
                    path.display()
                );
                return every_dirty();
            }
        },
    };
    let dirty: HashSet<&str> = current.0.iter().map(|(f, _)| f.as_str()).collect();
    let now_clean = baseline
        .files
        .keys()
        .filter(|f| !dirty.contains(f.as_str()))
        .map(|f| (f.clone(), fingerprint(&cfg.root, f)));
    current
        .0
        .iter()
        .cloned()
        .chain(now_clean)
        .filter(|(f, _)| is_candidate(f))
        .filter(|(f, now)| match (baseline.files.get(f), now) {
            (Some(Some(before)), Some(now)) => before != now,
            _ => true,
        })
        .map(|(f, _)| f)
        .collect()
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
            claude_slash_commands: false,
        }
    }

    #[test]
    fn a_path_whose_state_cannot_be_fingerprinted_always_counts_as_touched() {
        // #173 review：目錄／submodule 這類非一般檔案兩次都「不知道」，不能判定為沒變。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        std::fs::create_dir_all(tmp.join("vendor/lib")).unwrap();
        let dirty = vec!["vendor/lib".to_string()];
        write_baseline(&c, "my-change", &snapshot(&c, &dirty)).unwrap();

        let touched = touched_since_baseline(&c, "my-change", &snapshot(&c, &dirty), |_| true);

        assert_eq!(touched, dirty);
    }

    #[cfg(unix)]
    #[test]
    fn a_path_that_cannot_be_stated_always_counts_as_touched() {
        use std::os::unix::fs::PermissionsExt;
        /// 測試結束（含 panic）時把目錄權限改回來，讓 TempDir 刪得掉。
        struct RestoreSearchable(PathBuf);
        impl Drop for RestoreSearchable {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
            }
        }
        // NotFound 以外的 stat 錯誤（這裡是上層目錄沒有搜尋權限）不能當成
        // `missing`：兩次都是 `missing` 會被判成沒變。
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        std::fs::create_dir_all(tmp.join("private")).unwrap();
        std::fs::write(tmp.join("private/secret.rs"), "// v1\n").unwrap();
        let _restore = RestoreSearchable(tmp.join("private"));
        std::fs::set_permissions(tmp.join("private"), std::fs::Permissions::from_mode(0o600))
            .unwrap();
        if std::fs::symlink_metadata(tmp.join("private/secret.rs")).is_ok() {
            eprintln!("skipping: running as root (directory search permission not enforced)");
            return;
        }
        let dirty = vec!["private/secret.rs".to_string()];
        write_baseline(&c, "my-change", &snapshot(&c, &dirty)).unwrap();

        let touched = touched_since_baseline(&c, "my-change", &snapshot(&c, &dirty), |_| true);

        assert_eq!(touched, dirty);
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
    ///
    /// Kept in sync with the identical helper in `archive.rs`'s test module.
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
