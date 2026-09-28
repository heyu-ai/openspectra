//! oracle 3.0.0 的 per-task baseline 指紋（`task start`／`task done`，#190）。
//!
//! oracle 內含 libgit2，以 `git_status_list` 取得 dirty 路徑；這裡用 git CLI 重現
//! 同一組結果（規格與實證見 `docs/reverse-engineering/task.md` 的
//! 「v3.0.0 fingerprints」一節）：
//!
//! - 狀態來自 `git status --porcelain=v2 --no-renames --untracked-files=all
//!   --ignore-submodules=all`，換算成 libgit2 的 `git_status_t` bitmask；同一路徑
//!   出現多筆時取 OR（例如 `git rm --cached` 後的 `D ` 加 `??` 得 132）。
//! - `index_identity` 是 stage-0 index entry 的 `<oid>:<十進位 mode>:<on-disk flags>`，
//!   flags = `min(路徑長度, 0xFFF) | stage<<12 | EXTENDED(0x4000) | ASSUME_VALID(0x8000)`；
//!   沒有 stage-0 entry 時為 `None`。
//! - `worktree_identity` 是工作目錄原始位元組的 blob oid（`hash-object --no-filters`），
//!   symlink 為 `symlink:<目標字串的 oid>`，檔案不存在或是目錄時為 `None`。
//! - 排除整個 spec 目錄與 `.spectra/`（以路徑元件比對）；路徑依位元組排序。

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

// libgit2 `git_status_t`。
const INDEX_NEW: u32 = 1;
const INDEX_MODIFIED: u32 = 2;
const INDEX_DELETED: u32 = 4;
const INDEX_RENAMED: u32 = 8;
const INDEX_TYPECHANGE: u32 = 16;
const WT_NEW: u32 = 128;
const WT_MODIFIED: u32 = 256;
const WT_DELETED: u32 = 512;
const WT_TYPECHANGE: u32 = 1024;
const CONFLICTED: u32 = 32768;

const FLAG_EXTENDED: u32 = 0x4000;
const FLAG_ASSUME_VALID: u32 = 0x8000;
const FLAG_NAME_MASK: u32 = 0x0FFF;

/// 一個 dirty 路徑的指紋；key 順序與 oracle 相同，缺值輸出 `null`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFingerprint {
    pub path: String,
    pub status: u32,
    pub index_identity: Option<String>,
    pub worktree_identity: Option<String>,
}

/// `git -C root <args>`，回傳原始 stdout bytes；失敗回 `None`。
fn git_bytes(root: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

fn nul_records(bytes: &[u8]) -> impl Iterator<Item = String> + '_ {
    bytes
        .split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .map(|r| String::from_utf8_lossy(r).into_owned())
}

fn index_bits(c: char) -> u32 {
    match c {
        'M' => INDEX_MODIFIED,
        'A' => INDEX_NEW,
        'D' => INDEX_DELETED,
        'R' => INDEX_RENAMED,
        'T' => INDEX_TYPECHANGE,
        _ => 0,
    }
}

fn worktree_bits(c: char) -> u32 {
    match c {
        'M' => WT_MODIFIED,
        'D' => WT_DELETED,
        'T' => WT_TYPECHANGE,
        // intent-to-add（`git add -N`）：libgit2 回報 INDEX_NEW | WT_MODIFIED（實測 257）。
        'A' => INDEX_NEW | WT_MODIFIED,
        _ => 0,
    }
}

/// 解析 porcelain v2 的 status，回傳 路徑 → bitmask；路徑相對於 repo root。
fn parse_status(bytes: &[u8]) -> BTreeMap<String, u32> {
    let mut out: BTreeMap<String, u32> = BTreeMap::new();
    for record in nul_records(bytes) {
        let (bits, path) = if let Some(rest) = record.strip_prefix("? ") {
            // 巢狀（非 submodule）repo 以 `inner/` 出現；oracle 記成 `inner`。
            (WT_NEW, rest.trim_end_matches('/').to_string())
        } else if let Some(rest) = record.strip_prefix("u ") {
            // u XY sub m1 m2 m3 mW h1 h2 h3 path
            let path = rest.splitn(10, ' ').nth(9).unwrap_or_default().to_string();
            (CONFLICTED, path)
        } else if let Some(rest) = record.strip_prefix("1 ") {
            // 1 XY sub mH mI mW hH hI path
            let mut fields = rest.splitn(8, ' ');
            let xy: Vec<char> = fields.next().unwrap_or("..").chars().collect();
            let path = fields.nth(6).unwrap_or_default().to_string();
            let x = xy.first().copied().unwrap_or('.');
            let y = xy.get(1).copied().unwrap_or('.');
            (index_bits(x) | worktree_bits(y), path)
        } else {
            continue;
        };
        if !path.is_empty() {
            *out.entry(path).or_default() |= bits;
        }
    }
    out
}

/// 路徑是否落在 `prefix`（以路徑元件比對，`docs/spectrafoo` 不算在 `docs/spectra` 內）。
fn under(path: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    !prefix.is_empty() && (path == prefix || path.starts_with(&format!("{prefix}/")))
}

/// stage-0 index entry：路徑 → (mode, oid, ls-files -v 的 tag)。
fn index_entries(root: &Path, paths: &[&str]) -> HashMap<String, (u32, String, char)> {
    let mut args = vec!["--literal-pathspecs", "ls-files", "-s", "-v", "-z", "--"];
    args.extend_from_slice(paths);
    let Some(bytes) = git_bytes(root, &args) else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for record in nul_records(&bytes) {
        // "<tag> <mode> <oid> <stage>\t<path>"
        let Some((meta, path)) = record.split_once('\t') else {
            continue;
        };
        let mut fields = meta.split(' ');
        let tag = fields.next().and_then(|t| t.chars().next()).unwrap_or('H');
        let mode = fields.next().and_then(|m| u32::from_str_radix(m, 8).ok());
        let oid = fields.next().map(str::to_string);
        let stage = fields.next();
        if let (Some(mode), Some(oid), Some("0")) = (mode, oid, stage) {
            out.insert(path.to_string(), (mode, oid, tag));
        }
    }
    out
}

fn index_identity(path: &str, entry: &(u32, String, char), status: u32) -> String {
    let (mode, oid, tag) = entry;
    let mut flags = (path.len() as u32).min(FLAG_NAME_MASK);
    // `ls-files -v`：小寫 tag 表示 assume-unchanged；`S`／`s` 表示 skip-worktree
    // （EXTENDED）；intent-to-add 同樣設 EXTENDED，由 status 的 INDEX_NEW|WT_MODIFIED 辨識。
    if tag.is_ascii_lowercase() {
        flags |= FLAG_ASSUME_VALID;
    }
    let intent_to_add = status & (INDEX_NEW | WT_MODIFIED) == (INDEX_NEW | WT_MODIFIED)
        && oid == "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391";
    if tag.eq_ignore_ascii_case(&'s') || intent_to_add {
        flags |= FLAG_EXTENDED;
    }
    format!("{oid}:{mode}:{flags}")
}

/// 一批內容的 blob oid（`git hash-object --no-filters --stdin-paths` 或 `--stdin`）。
fn hash_paths(root: &Path, files: &[String]) -> Option<Vec<String>> {
    if files.is_empty() {
        return Some(Vec::new());
    }
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["hash-object", "--no-filters", "--stdin-paths"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    {
        let stdin = child.stdin.as_mut()?;
        for f in files {
            writeln!(stdin, "{f}").ok()?;
        }
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    let oids: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect();
    (oids.len() == files.len()).then_some(oids)
}

fn hash_bytes(root: &Path, data: &[u8]) -> Option<String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["hash-object", "--no-filters", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    child.stdin.as_mut()?.write_all(data).ok()?;
    let out = child.wait_with_output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// 目前 HEAD 的 commit oid；未出生的分支為 `None`。
pub fn head_revision(root: &Path) -> Option<String> {
    git_bytes(root, &["rev-parse", "--verify", "-q", "HEAD"])
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 計算 `root` 所在 repo 的 dirty 指紋，排除 `spec_dir` 與 `.spectra/`（皆相對於
/// 專案 root）。不在 git repo 內或 git 無法執行時回 `None`。
pub fn dirty_fingerprints(root: &Path, spec_dir: &str) -> Option<Vec<FileFingerprint>> {
    let toplevel = git_bytes(root, &["rev-parse", "--show-toplevel"])?;
    let toplevel = std::path::PathBuf::from(String::from_utf8_lossy(&toplevel).trim());
    let prefix = git_bytes(root, &["rev-parse", "--show-prefix"])
        .map(|b| String::from_utf8_lossy(&b).trim().to_string())
        .unwrap_or_default();
    let spec_prefix = format!("{prefix}{}", spec_dir.trim_start_matches("./"));
    let state_prefix = format!("{prefix}.spectra");

    let status_bytes = git_bytes(
        &toplevel,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--no-renames",
            "--untracked-files=all",
            "--ignore-submodules=all",
        ],
    )?;
    let statuses: Vec<(String, u32)> = parse_status(&status_bytes)
        .into_iter()
        .filter(|(p, _)| !under(p, &spec_prefix) && !under(p, &state_prefix))
        .collect();

    let paths: Vec<&str> = statuses.iter().map(|(p, _)| p.as_str()).collect();
    let index = if paths.is_empty() {
        HashMap::new()
    } else {
        index_entries(&toplevel, &paths)
    };

    // 工作目錄身分：一般檔案批次 hash，symlink 另外 hash 目標字串。
    let mut regular = Vec::new();
    let mut worktree: HashMap<String, Option<String>> = HashMap::new();
    for (path, _) in &statuses {
        let full = toplevel.join(path);
        match std::fs::symlink_metadata(&full) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = std::fs::read_link(&full).ok();
                let id = target.and_then(|t| {
                    hash_bytes(&toplevel, t.as_os_str().as_encoded_bytes())
                        .map(|oid| format!("symlink:{oid}"))
                });
                worktree.insert(path.clone(), id);
            }
            Ok(meta) if meta.is_file() => regular.push(path.clone()),
            _ => {
                worktree.insert(path.clone(), None);
            }
        }
    }
    let oids = hash_paths(&toplevel, &regular)?;
    for (path, oid) in regular.into_iter().zip(oids) {
        worktree.insert(path, Some(oid));
    }

    let mut out: Vec<FileFingerprint> = statuses
        .into_iter()
        .map(|(path, status)| FileFingerprint {
            index_identity: index
                .get(&path)
                .map(|entry| index_identity(&path, entry, status)),
            worktree_identity: worktree.remove(&path).flatten(),
            status,
            path,
        })
        .collect();
    out.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    Some(out)
}

/// 兩組指紋間有差異的路徑（任一邊出現、且三元組不同），依位元組排序。
pub fn changed_paths(before: &[FileFingerprint], now: &[FileFingerprint]) -> Vec<String> {
    let key = |f: &FileFingerprint| {
        (
            f.status,
            f.index_identity.clone(),
            f.worktree_identity.clone(),
        )
    };
    let b: HashMap<&str, _> = before.iter().map(|f| (f.path.as_str(), key(f))).collect();
    let n: HashMap<&str, _> = now.iter().map(|f| (f.path.as_str(), key(f))).collect();
    let mut changed: Vec<String> = b
        .keys()
        .chain(n.keys())
        .filter(|p| b.get(*p) != n.get(*p))
        .map(|p| p.to_string())
        .collect();
    changed.sort_by(|x, y| x.as_bytes().cmp(y.as_bytes()));
    changed.dedup();
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_v2_codes_map_to_libgit2_bits() {
        let raw = b"1 .M N... 100644 100644 100644 aa aa src/a.rs\0\
1 MM N... 100644 100644 100644 aa bb src/mm.rs\0\
1 AD N... 000000 100644 000000 00 cc src/ad.rs\0\
1 .A N... 000000 000000 100644 00 00 src/ita.rs\0\
1 D. N... 100644 000000 000000 dd 00 src/rm.rs\0\
? src/rm.rs\0\
? inner/\0\
u UU N... 100644 100644 100644 100644 e1 e2 e3 src/conflict.rs\0";
        let parsed = parse_status(raw);
        let get = |p: &str| parsed.get(p).copied();
        assert_eq!(get("src/a.rs"), Some(256));
        assert_eq!(get("src/mm.rs"), Some(258));
        assert_eq!(get("src/ad.rs"), Some(513));
        assert_eq!(get("src/ita.rs"), Some(257));
        assert_eq!(get("src/rm.rs"), Some(132));
        assert_eq!(get("inner"), Some(128));
        assert_eq!(get("src/conflict.rs"), Some(32768));
    }

    #[test]
    fn exclusion_is_by_path_component() {
        assert!(under("docs/spectra/config.yaml", "docs/spectra"));
        assert!(under("docs/spectra", "docs/spectra/"));
        assert!(!under("docs/spectrafoo/x.md", "docs/spectra"));
        assert!(under(".spectra/touched/demo.json", ".spectra"));
        assert!(!under(".spectra.yaml", ".spectra"));
        assert!(!under(".spectrafoo/y", ".spectra"));
    }

    #[test]
    fn index_identity_flags_follow_the_on_disk_layout() {
        let oid = "b95d8a92c213f38c10ed0311e619fd017556e304".to_string();
        assert_eq!(
            index_identity("src/old.rs", &(0o100644, oid.clone(), 'H'), 256),
            format!("{oid}:33188:10")
        );
        assert_eq!(
            index_identity("src/old.rs", &(0o100644, oid.clone(), 'h'), 256),
            format!("{oid}:33188:{}", 0x8000 | 10)
        );
        let empty = "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391".to_string();
        assert_eq!(
            index_identity("src/ita.rs", &(0o100644, empty.clone(), 'H'), 257),
            format!("{empty}:33188:16394")
        );
        let long = "x".repeat(5000);
        assert!(index_identity(&long, &(0o100644, oid.clone(), 'H'), 256).ends_with(":4095"));
    }

    #[test]
    fn changed_paths_compares_the_full_tuple_on_either_side() {
        let fp = |p: &str, s: u32, w: &str| FileFingerprint {
            path: p.to_string(),
            status: s,
            index_identity: Some("i".to_string()),
            worktree_identity: Some(w.to_string()),
        };
        let before = vec![
            fp("same.rs", 256, "a"),
            fp("edited.rs", 256, "a"),
            fp("gone.rs", 256, "a"),
        ];
        let now = vec![
            fp("same.rs", 256, "a"),
            fp("edited.rs", 256, "b"),
            fp("Z.rs", 128, "c"),
        ];
        assert_eq!(
            changed_paths(&before, &now),
            vec!["Z.rs", "edited.rs", "gone.rs"]
        );
    }
}
