//! `spectra scope`（oracle 3.0.0，#165）：唯讀地列出一個實作的範圍與 patch。
//!
//! 規格與實證見 `docs/reverse-engineering/scope.md`。oracle 以 libgit2 產生 patch；依 D8
//! 裁決，OpenSpectra 使用 git CLI，再把 patch 標頭調成 libgit2 的樣子（typechange 拆段、
//! 含空白路徑不加行尾 tab、空新檔補 `---`/`+++`）。仍有差異的 libgit2 邊角情況（worktree
//! rename 偵測、intent-to-add 拆成兩段、rename 相似度、abbrev 碰撞）記於該文件 §10。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;

use crate::config::Config;
use crate::touched::TouchedTracking;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Committed,
    Staged,
    Unstaged,
    Untracked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopeDiff {
    pub kind: Kind,
    pub status: String,
    pub path: String,
    pub old_path: Option<String>,
    pub patch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopeFile {
    pub path: String,
    pub old_path: Option<String>,
    pub change_kinds: Vec<Kind>,
    pub provenance: Vec<String>,
    pub content_kind: String,
    pub inspectable: bool,
    pub diffs: Vec<ScopeDiff>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Limitation {
    pub path: Option<String>,
    pub code: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopeReport {
    pub schema_version: u32,
    pub snapshot_id: String,
    pub scope_source: String,
    pub status: String,
    pub base_revision: Option<String>,
    pub base_source: Option<String>,
    pub head_revision: Option<String>,
    pub files: Vec<ScopeFile>,
    pub limitations: Vec<Limitation>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ScopeOptions<'a> {
    pub change: Option<&'a str>,
    pub base: Option<&'a str>,
}

const MISSING_BASE: &str = "No validated pre-implementation base; provide --base explicitly. Current differences do not establish complete historical coverage.";
const APPROXIMATED: &str = "Scope is approximated: trusted touched attribution is unavailable; this is not proof of hunk ownership.";
const PREEXISTING: &str =
    "This path was dirty before task start; its hunks have only approximate attribution.";
const ZERO: &str = "0000000000000000000000000000000000000000";

/// oracle 共用的 change ID 驗證（訊息與順序照 probe）。
pub fn validate_change_id(id: &str) -> Result<()> {
    if id.is_empty() {
        bail!("Change ID must not be empty");
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        bail!("Change ID '{id}' contains illegal characters (path separators or '..')");
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!("Change ID '{id}' must contain only lowercase letters, digits, and hyphens");
    }
    if id.starts_with('-') || id.ends_with('-') {
        bail!("Change ID '{id}' must not start or end with a hyphen");
    }
    Ok(())
}

/// 在 `dir` 執行 git，回傳 (成功與否, stdout, stderr)。所有呼叫都不寫 index。
fn git(dir: &Path, args: &[&str], env: &[(&str, &Path)]) -> Result<(bool, Vec<u8>, String)> {
    let mut cmd = Command::new("git");
    cmd.env("GIT_OPTIONAL_LOCKS", "0")
        .args(["-c", "gc.auto=0", "-c", "maintenance.auto=false"])
        .arg("-C")
        .arg(dir)
        .args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().context("running git")?;
    Ok((
        out.status.success(),
        out.stdout,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

fn git_ok(dir: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let (ok, out, err) = git(dir, args, &[])?;
    if !ok {
        bail!("git {} failed: {}", args.join(" "), err.trim());
    }
    Ok(out)
}

fn git_line(dir: &Path, args: &[&str]) -> Option<String> {
    let (ok, out, _) = git(dir, args, &[]).ok()?;
    ok.then(|| String::from_utf8_lossy(&out).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// git blob id（`hash-object --no-filters --stdin`）。
fn blob_id(dir: &Path, data: &[u8]) -> Result<String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["hash-object", "--no-filters", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("running git hash-object")?;
    child.stdin.as_mut().expect("piped stdin").write_all(data)?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("git hash-object failed");
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// repo 的位置資訊。
struct Repo {
    top: PathBuf,
    /// 專案在 repo 內的前綴（專案就在頂層時為空字串）。
    prefix: String,
    /// 專案子樹的 pathspec（專案就在頂層時為 `None`）。
    pathspec: Option<String>,
}

fn open_repo(cfg: &Config) -> Result<Repo> {
    let root = cfg.root.clone();
    if git_line(&root, &["rev-parse", "--is-bare-repository"]).as_deref() == Some("true") {
        bail!("Git scope requires a worktree");
    }
    let Some(top) = git_line(&root, &["rev-parse", "--show-toplevel"]) else {
        bail!(
            "Git unavailable for scope: could not find repository at '{}'; class=Repository (6); code=NotFound (-3)",
            root.display()
        );
    };
    let prefix = git_line(&root, &["rev-parse", "--show-prefix"]).unwrap_or_default();
    let pathspec = (!prefix.is_empty()).then(|| format!(":(top){prefix}"));
    Ok(Repo {
        top: PathBuf::from(top),
        prefix,
        pathspec,
    })
}

fn head_oid(repo: &Repo) -> Option<String> {
    git_line(&repo.top, &["rev-parse", "--verify", "-q", "HEAD"])
}

fn is_ancestor(repo: &Repo, base: &str, head: &str) -> bool {
    git(&repo.top, &["merge-base", "--is-ancestor", base, head], &[])
        .map(|(ok, _, _)| ok)
        .unwrap_or(false)
}

/// `--base` 解析成 commit oid；錯誤訊息照 oracle（libgit2）的文字。
fn resolve_base(repo: &Repo, arg: &str) -> Result<String> {
    if arg.is_empty() {
        bail!("Invalid comparison base \"\": failed to parse revision specifier - Invalid pattern ''; class=Invalid (3); code=InvalidSpec (-12)");
    }
    let Some(oid) = git_line(&repo.top, &["rev-parse", "--verify", "-q", arg]) else {
        bail!("Invalid comparison base \"{arg}\": revspec '{arg}' not found; class=Reference (4); code=NotFound (-3)");
    };
    match git_line(
        &repo.top,
        &["rev-parse", "--verify", "-q", &format!("{arg}^{{commit}}")],
    ) {
        Some(commit) => Ok(commit),
        None => bail!("Invalid comparison base \"{arg}\": the git_object of id '{oid}' can not be successfully peeled into a commit (git_object_t=1).; class=Object (11); code=InvalidSpec (-12)"),
    }
}

/// 一個 git delta（`git diff --raw` 的一筆，或 untracked 檔）。
struct Delta {
    kind: Kind,
    status: String,
    path: String,
    old_path: Option<String>,
    old_mode: String,
    new_mode: String,
    old_oid: String,
    new_oid: String,
}

fn status_name(code: char) -> &'static str {
    match code {
        'A' => "added",
        'D' => "deleted",
        'R' => "renamed",
        _ => "modified",
    }
}

/// 解析 `git diff --raw -z`。typechange（`T`）拆成 deleted 再 added 兩筆（oracle 行為）。
fn parse_raw(kind: Kind, bytes: &[u8]) -> Vec<Delta> {
    let fields: Vec<String> = bytes
        .split(|b| *b == 0)
        .map(|f| String::from_utf8_lossy(f).into_owned())
        .collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < fields.len() {
        let meta = &fields[i];
        if !meta.starts_with(':') {
            i += 1;
            continue;
        }
        let parts: Vec<&str> = meta[1..].split(' ').collect();
        if parts.len() < 5 {
            break;
        }
        let code = parts[4].chars().next().unwrap_or('M');
        let (old_path, path, step) = if code == 'R' || code == 'C' {
            (
                Some(fields.get(i + 1).cloned().unwrap_or_default()),
                fields.get(i + 2).cloned().unwrap_or_default(),
                3,
            )
        } else {
            (None, fields.get(i + 1).cloned().unwrap_or_default(), 2)
        };
        let delta =
            |status: &str, old_mode: &str, new_mode: &str, old_oid: &str, new_oid: &str| Delta {
                kind,
                status: status.to_string(),
                path: path.clone(),
                old_path: old_path.clone(),
                old_mode: old_mode.to_string(),
                new_mode: new_mode.to_string(),
                old_oid: old_oid.to_string(),
                new_oid: new_oid.to_string(),
            };
        if code == 'T' {
            out.push(delta("deleted", parts[0], "000000", parts[2], ZERO));
            out.push(delta("added", "000000", parts[1], ZERO, parts[3]));
        } else {
            out.push(delta(
                status_name(code),
                parts[0],
                parts[1],
                parts[2],
                parts[3],
            ));
        }
        i += step;
    }
    out
}

/// patch 前綴（依 repo config：`diff.noprefix` → 無前綴；`diff.mnemonicPrefix=false` → a/ b/）。
fn prefixes(repo: &Repo, kind: Kind) -> (String, String) {
    let flag = |key: &str| git_line(&repo.top, &["config", "--bool", key]);
    if flag("diff.noprefix").as_deref() == Some("true") {
        return (String::new(), String::new());
    }
    if flag("diff.mnemonicPrefix").as_deref() == Some("false") {
        return ("a/".to_string(), "b/".to_string());
    }
    let (src, dst) = match kind {
        Kind::Committed => ("c/", "c/"),
        Kind::Staged => ("c/", "i/"),
        Kind::Unstaged | Kind::Untracked => ("i/", "w/"),
    };
    (src.to_string(), dst.to_string())
}

fn abbrev(repo: &Repo) -> String {
    git_line(&repo.top, &["config", "core.abbrev"])
        .filter(|v| v.parse::<u32>().is_ok())
        .unwrap_or_else(|| "7".to_string())
}

/// 物件或工作目錄檔案的內容（oid 為全 0 時讀工作目錄；讀不到時為空）。
fn side_bytes(repo: &Repo, oid: &str, path: &str) -> Vec<u8> {
    if oid == ZERO {
        return std::fs::read(repo.top.join(path)).unwrap_or_default();
    }
    git(&repo.top, &["cat-file", "blob", oid], &[])
        .ok()
        .filter(|(ok, _, _)| *ok)
        .map(|(_, out, _)| out)
        .unwrap_or_default()
}

fn is_binary(data: &[u8]) -> bool {
    data.iter().take(8000).any(|b| *b == 0)
}

/// 一個 diff 的內容種類與 patch（binary、非 UTF-8、submodule 沒有 patch）。
fn content_and_patch(
    repo: &Repo,
    delta: &Delta,
    diff_args: &[&str],
    env: &[(&str, &Path)],
) -> Result<(&'static str, Option<String>)> {
    if delta.old_mode == "160000" || delta.new_mode == "160000" {
        return Ok(("submodule", None));
    }
    let old_side = if delta.old_mode == "000000" {
        Vec::new()
    } else {
        side_bytes(
            repo,
            &delta.old_oid,
            delta.old_path.as_deref().unwrap_or(&delta.path),
        )
    };
    let new_side = if delta.new_mode == "000000" {
        Vec::new()
    } else {
        side_bytes(repo, &delta.new_oid, &delta.path)
    };
    if is_binary(&old_side) || is_binary(&new_side) {
        return Ok(("binary", None));
    }
    if std::str::from_utf8(&old_side).is_err() || std::str::from_utf8(&new_side).is_err() {
        return Ok(("unavailable", None));
    }
    let (src, dst) = prefixes(repo, delta.kind);
    let src_prefix = format!("--src-prefix={src}");
    let dst_prefix = format!("--dst-prefix={dst}");
    let abbrev = format!("--abbrev={}", abbrev(repo));
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-color",
        "--no-textconv",
        "-U3",
        "--inter-hunk-context=0",
        "--diff-algorithm=myers",
        "-M50%",
        abbrev.as_str(),
        src_prefix.as_str(),
        dst_prefix.as_str(),
    ];
    if src.is_empty() {
        args.push("--no-prefix");
    }
    args.extend_from_slice(diff_args);
    args.push("--");
    let paths: Vec<String> = std::iter::once(&delta.path)
        .chain(delta.old_path.iter())
        .map(|p| format!(":(top,literal){p}"))
        .collect();
    args.extend(paths.iter().map(String::as_str));
    let (ok, out, err) = git(&repo.top, &args, env)?;
    if !ok {
        bail!("git diff failed: {}", err.trim());
    }
    let patch =
        String::from_utf8(out).map_err(|_| anyhow!("git diff produced non-UTF-8 output"))?;
    let patch = libgit2_headers(typechange_section(patch, &delta.status), &dst, &delta.path);
    let symlink = delta.old_mode == "120000" || delta.new_mode == "120000";
    Ok((if symlink { "symlink" } else { "text" }, Some(patch)))
}

/// typechange（例如檔案變 symlink）時 git 對同一路徑輸出「deleted」與「new」兩段 patch，
/// oracle 拆成兩個 delta、各自只帶自己那段。只有一段時原樣回傳。
fn typechange_section(patch: String, status: &str) -> String {
    let starts: Vec<usize> = patch
        .match_indices("diff --git ")
        .filter(|(i, _)| *i == 0 || patch.as_bytes()[i - 1] == b'\n')
        .map(|(i, _)| i)
        .collect();
    if starts.len() < 2 {
        return patch;
    }
    let marker = match status {
        "deleted" => "\ndeleted file mode ",
        "added" => "\nnew file mode ",
        _ => return patch,
    };
    let ends = starts
        .iter()
        .skip(1)
        .copied()
        .chain(std::iter::once(patch.len()));
    starts
        .iter()
        .zip(ends)
        .map(|(s, e)| &patch[*s..e])
        .find(|section| section.contains(marker))
        .map_or(patch.clone(), str::to_string)
}

/// 把 git 的 patch 標頭調成 oracle（libgit2）的樣子：
/// - 含空白的路徑：git 在 `---`／`+++` 行尾加 `\t`，libgit2 不加。
/// - 新增的空檔：git 省略 `---`／`+++` 兩行，libgit2 會輸出 `--- /dev/null` 與 `+++ <dst><path>`。
///   （刪除空檔的對應情況未經 oracle 驗證，維持 git 的輸出。）
fn libgit2_headers(patch: String, dst: &str, path: &str) -> String {
    let mut out = String::with_capacity(patch.len() + 32);
    for line in patch.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if (body.starts_with("--- ") || body.starts_with("+++ ")) && body.ends_with('\t') {
            out.push_str(&body[..body.len() - 1]);
            out.push('\n');
        } else {
            out.push_str(line);
        }
    }
    let empty_new = out.contains("\nnew file mode ")
        && !out.contains("\n--- ")
        && !out.contains("\nBinary files ")
        && out.lines().last().is_some_and(|l| l.starts_with("index "));
    if empty_new {
        out.push_str(&format!("--- /dev/null\n+++ {dst}{path}\n"));
    }
    out
}

fn content_rank(kind: &str) -> u8 {
    match kind {
        "binary" | "submodule" => 3,
        "unavailable" => 2,
        "symlink" => 1,
        _ => 0,
    }
}

/// untracked 的一般檔案／symlink 與巢狀 repo（repo 相對路徑）。一般檔案必須可讀，否則與
/// tracked 檔相同回報 `Cannot read source`（oracle 在 capture 與 `--check-snapshot` 都檢查）。
fn list_untracked(repo: &Repo) -> Result<(Vec<String>, Vec<String>)> {
    let mut args = vec!["ls-files", "--others", "--exclude-standard", "-z"];
    if let Some(pathspec) = &repo.pathspec {
        args.extend(["--", pathspec.as_str()]);
    }
    let listed = git_ok(&repo.top, &args)?;
    let mut files = Vec::new();
    let mut nested = Vec::new();
    for raw in listed.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let path = String::from_utf8_lossy(raw).into_owned();
        if let Some(dir) = path.strip_suffix('/') {
            nested.push(dir.to_string());
            continue;
        }
        let full = repo.top.join(&path);
        match std::fs::symlink_metadata(&full) {
            Ok(meta) if meta.file_type().is_symlink() => files.push(path),
            Ok(meta) if meta.is_file() => {
                if let Err(e) = std::fs::File::open(&full) {
                    let rel = &path[repo.prefix.len().min(path.len())..];
                    bail!("Cannot read source \"{rel}\": {e}");
                }
                files.push(path);
            }
            _ => {} // FIFO 等非一般檔案略過（oracle 行為）
        }
    }
    Ok((files, nested))
}

/// 把真正的 index 複製到 scratch（沒有 index 時不建檔，git 視為空 index）。
fn copy_index(repo: &Repo, scratch: &Path, name: &str) -> Result<PathBuf> {
    let copy = scratch.join(name);
    let real_index = git_line(&repo.top, &["rev-parse", "--git-path", "index"])
        .map(|p| repo.top.join(p))
        .filter(|p| p.is_file());
    if let Some(real) = real_index {
        std::fs::copy(&real, &copy).context("copying the git index")?;
    }
    Ok(copy)
}

/// index 對工作目錄的 diff 用的 index 複本。`git diff` 會無視 `GIT_OPTIONAL_LOCKS` 把刷新後
/// 的 stat 資料寫回 index（`refresh_index_quietly`），oracle 則完全唯讀（p17g：stat 過期時
/// 真正的 `.git/index` 被改寫）。
const WORKTREE_INDEX: &str = "scope-worktree-index";

/// unstaged 的 raw delta，對 index 複本執行。
fn unstaged_deltas(repo: &Repo, scratch: &Path) -> Result<Vec<Delta>> {
    let index = copy_index(repo, scratch, WORKTREE_INDEX)?;
    let (ok, out, err) = git(
        &repo.top,
        &raw_args(repo, &[]),
        &[("GIT_INDEX_FILE", &index)],
    )?;
    if !ok {
        bail!("git diff failed: {}", err.trim());
    }
    Ok(parse_raw(Kind::Unstaged, &out))
}

/// untracked diff 用的 git 環境：暫存 index，加上 scratch object 目錄（真正的 objects 當
/// alternate 唯讀使用），讓 `add -N` 寫出的空 blob 等物件不落進 `.git/objects`。
type ScratchEnv = Vec<(&'static str, PathBuf)>;

/// untracked 的 diff：在暫存 index 以 intent-to-add 加入後比對（不動真正的 index 與 object DB）。
fn untracked_deltas(repo: &Repo, scratch: &Path) -> Result<(Vec<Delta>, Option<ScratchEnv>)> {
    let (files, nested) = list_untracked(repo)?;
    let untracked = |path: String, mode: &str| Delta {
        kind: Kind::Untracked,
        status: "untracked".to_string(),
        path,
        old_path: None,
        old_mode: "000000".to_string(),
        new_mode: mode.to_string(),
        old_oid: ZERO.to_string(),
        new_oid: ZERO.to_string(),
    };
    let mut deltas: Vec<Delta> = nested.into_iter().map(|p| untracked(p, "040000")).collect();
    if files.is_empty() {
        return Ok((deltas, None));
    }
    let index = copy_index(repo, scratch, "scope-index")?;
    let objects = scratch.join("scope-objects");
    std::fs::create_dir_all(&objects).context("creating scratch object directory")?;
    let real_objects = git_line(&repo.top, &["rev-parse", "--git-path", "objects"])
        .map(|p| repo.top.join(p))
        .ok_or_else(|| anyhow!("could not locate the git object directory"))?;
    let env: ScratchEnv = vec![
        ("GIT_INDEX_FILE", index),
        ("GIT_OBJECT_DIRECTORY", objects),
        ("GIT_ALTERNATE_OBJECT_DIRECTORIES", real_objects),
    ];
    // `add -N` 不帶 alternates：git 寫入已存在的物件時會 freshen（utime）它，即使它在
    // alternate 裡——空 blob 常已存在（例如 `.gitkeep`），會改到 `.git/objects` 的 mtime。
    let env_refs: Vec<(&str, &Path)> = env[..2].iter().map(|(k, v)| (*k, v.as_path())).collect();
    let mut add = vec!["add", "-N", "--"];
    let specs: Vec<String> = files.iter().map(|f| format!(":(top,literal){f}")).collect();
    add.extend(specs.iter().map(String::as_str));
    let (ok, _, err) = git(&repo.top, &add, &env_refs)?;
    if !ok {
        bail!("preparing untracked diffs failed: {}", err.trim());
    }
    for path in files {
        let mode = match std::fs::symlink_metadata(repo.top.join(&path)) {
            Ok(meta) if meta.file_type().is_symlink() => "120000",
            _ => "100644",
        };
        deltas.push(untracked(path, mode));
    }
    Ok((deltas, Some(env)))
}

/// 已解析的參數與 repo 狀態。
struct Plan<'a> {
    change: Option<&'a str>,
    base_arg: Option<&'a str>,
    tracking: Option<TouchedTracking>,
    head: Option<String>,
}

/// snapshot_id：preimage `[change, base_arg, base_candidate, head, index_id, tracking_id,
/// entries]` 的 compact JSON，再取 git blob id（oracle 3.0.0，見 scope.md）。
fn snapshot_id(
    cfg: &Config,
    repo: &Repo,
    plan: &Plan<'_>,
    base_candidate: Option<&str>,
) -> Result<String> {
    let index_id = match git_line(&repo.top, &["rev-parse", "--git-path", "index"])
        .map(|p| repo.top.join(p))
        .and_then(|p| std::fs::read(p).ok())
    {
        Some(bytes) => Some(blob_id(&repo.top, &bytes)?),
        None => None,
    };
    let tracking_id = match plan.change {
        Some(change) => match std::fs::read(crate::touched::touched_path(cfg, change)) {
            Ok(bytes) => Some(blob_id(&repo.top, &bytes)?),
            Err(_) => None,
        },
        None => None,
    };
    let touched: Vec<String> = plan
        .tracking
        .iter()
        .flat_map(|t| t.touched.iter())
        .flat_map(|e| e.files.iter().cloned())
        .collect();
    let entries = crate::fingerprint::snapshot_entries(&cfg.root, &touched)
        .ok_or_else(|| anyhow!("Git unavailable for scope"))?;
    let entries: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|(path, status, id)| serde_json::json!([path, status, id]))
        .collect();
    let preimage = serde_json::to_string(&serde_json::json!([
        plan.change,
        plan.base_arg,
        base_candidate,
        plan.head,
        index_id,
        tracking_id,
        entries,
    ]))?;
    blob_id(&repo.top, preimage.as_bytes())
}

/// 讀 tracking 檔（只在 `--change` 時）；壞掉或屬於別的 change 時失敗（與 `task` 相同訊息）。
fn load_tracking(cfg: &Config, change: Option<&str>) -> Result<Option<TouchedTracking>> {
    match change {
        Some(change) => crate::touched::load_strict(cfg, change),
        None => Ok(None),
    }
}

/// 工作目錄中有變動的追蹤路徑若無法讀取或不是一般檔案，oracle 在擷取 snapshot 時就失敗。
fn check_sources(repo: &Repo, deltas: &[Delta]) -> Result<()> {
    // oracle p17f：dirty 路徑的上層目錄被換成 symlink 時不讀穿，直接失敗（含已刪除的路徑）。
    for delta in deltas {
        let mut ancestor = repo.top.clone();
        let parts: Vec<&str> = delta.path.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            ancestor.push(part);
            let is_link = std::fs::symlink_metadata(&ancestor)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            if is_link {
                let rel = &delta.path[repo.prefix.len().min(delta.path.len())..];
                bail!("Scope path \"{rel}\" traverses a directory symlink; target was not read");
            }
        }
    }
    for delta in deltas
        .iter()
        .filter(|d| d.new_oid == ZERO && d.new_mode != "000000")
    {
        let full = repo.top.join(&delta.path);
        let Ok(meta) = std::fs::symlink_metadata(&full) else {
            continue;
        };
        let rel = &delta.path[repo.prefix.len().min(delta.path.len())..];
        if meta.file_type().is_symlink() || meta.is_dir() {
            continue;
        }
        if !meta.is_file() {
            bail!("Unsupported source file type at \"{rel}\"");
        }
        if let Err(e) = std::fs::File::open(&full) {
            bail!("Cannot read source \"{rel}\": {e}");
        }
    }
    Ok(())
}

fn raw_args<'a>(repo: &'a Repo, extra: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec![
        "diff",
        "--raw",
        "-z",
        "--no-abbrev",
        "--no-ext-diff",
        "-M50%",
        "--ignore-submodules=none",
    ];
    args.extend_from_slice(extra);
    if let Some(pathspec) = &repo.pathspec {
        args.push("--");
        args.push(pathspec);
    }
    args
}

fn limitation(path: Option<&str>, code: &str, reason: &str) -> Limitation {
    Limitation {
        path: path.map(str::to_string),
        code: code.to_string(),
        reason: reason.to_string(),
    }
}

fn tempdir() -> Result<PathBuf> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("spectra-scope-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

pub fn capture(cfg: &Config, opts: ScopeOptions<'_>) -> Result<ScopeReport> {
    if let Some(change) = opts.change {
        validate_change_id(change)?;
    }
    let repo = open_repo(cfg)?;
    if let Some(change) = opts.change {
        if !cfg.changes_dir().join(change).is_dir() {
            bail!("Change {change:?} not found");
        }
    }
    let plan = Plan {
        change: opts.change,
        base_arg: opts.base,
        tracking: load_tracking(cfg, opts.change)?,
        head: head_oid(&repo),
    };
    let scratch = tempdir()?;
    let result = build_report(cfg, &repo, &plan, &scratch);
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

/// 決定比較基準：`--base` 的錯誤是致命的，review_base 的問題只是 limitation。
fn comparison_base(
    repo: &Repo,
    plan: &Plan<'_>,
    limitations: &mut Vec<Limitation>,
) -> Result<(Option<String>, Option<&'static str>, Option<String>)> {
    let explicit = match plan.base_arg {
        Some(arg) => {
            let oid = resolve_base(repo, arg)?;
            let Some(head) = plan.head.as_deref() else {
                bail!("Comparison base requires a committed HEAD");
            };
            if !is_ancestor(repo, &oid, head) {
                bail!("Comparison base \"{arg}\" is not in HEAD's history");
            }
            Some(oid)
        }
        None => None,
    };
    let review_head = plan
        .tracking
        .as_ref()
        .and_then(|t| t.review_base.as_ref())
        .and_then(|r| r.head_revision.clone());
    let base_candidate = explicit
        .clone()
        .or_else(|| plan.change.and(review_head.clone()));
    if let Some(oid) = explicit {
        return Ok((Some(oid), Some("explicit"), base_candidate));
    }
    let Some(review) = review_head else {
        return Ok((None, None, base_candidate));
    };
    match plan.head.as_deref() {
        None => {
            limitations.push(limitation(
                None,
                "invalid_review_base",
                "Comparison base requires a committed HEAD",
            ));
            Ok((None, None, base_candidate))
        }
        Some(head) if is_ancestor(repo, &review, head) => {
            Ok((Some(review), Some("review_base"), base_candidate))
        }
        Some(_) => {
            limitations.push(limitation(
                None,
                "invalid_review_base",
                &format!("Comparison base \"{review}\" is not in HEAD's history"),
            ));
            Ok((None, None, base_candidate))
        }
    }
}

fn build_report(cfg: &Config, repo: &Repo, plan: &Plan<'_>, scratch: &Path) -> Result<ScopeReport> {
    let staged = parse_raw(
        Kind::Staged,
        &git_ok(&repo.top, &raw_args(repo, &["--cached"]))?,
    );
    let unstaged = unstaged_deltas(repo, scratch)?;
    check_sources(repo, &unstaged)?;
    let (untracked, scratch_env) = untracked_deltas(repo, scratch)?;
    let mut limitations = Vec::new();
    let (base_revision, base_source, base_candidate) =
        comparison_base(repo, plan, &mut limitations)?;

    let mut deltas = Vec::new();
    if let (Some(base), Some(head)) = (&base_revision, &plan.head) {
        if base != head {
            deltas.extend(parse_raw(
                Kind::Committed,
                &git_ok(&repo.top, &raw_args(repo, &[base, head]))?,
            ));
        }
    }
    deltas.extend(staged);
    deltas.extend(unstaged);
    deltas.extend(untracked);

    let touched = plan
        .tracking
        .as_ref()
        .map(|t| t.touched.clone())
        .unwrap_or_default();
    let scope_source = match plan.change {
        None if plan.base_arg.is_some() => "explicit_base",
        None => "current_worktree",
        Some(_) if !touched.is_empty() && touched.iter().all(|e| e.provenance.is_some()) => {
            "touched_tracking"
        }
        Some(_) if !touched.is_empty() => "approximated_tracking",
        Some(_) if base_revision.is_some() => "approximated_base",
        Some(_) => "approximated_worktree",
    };
    let tracking_mode = scope_source.ends_with("_tracking");

    let rel = |p: &str| p[repo.prefix.len().min(p.len())..].to_string();
    let mut files: Vec<ScopeFile> = Vec::new();
    let mut content_limits: Vec<(Kind, String, Limitation)> = Vec::new();
    for delta in &deltas {
        let path = rel(&delta.path);
        let old_path = delta.old_path.as_deref().map(rel);
        // 追蹤模式只留 path／old_path 與 touched 檔完全相符的 diff。
        let provenance: Vec<String> = if tracking_mode {
            let mut prov = Vec::new();
            for entry in &touched {
                let named = entry
                    .files
                    .iter()
                    .any(|f| *f == path || Some(f.as_str()) == old_path.as_deref());
                if named {
                    let p = entry
                        .provenance
                        .clone()
                        .unwrap_or_else(|| "legacy_unverified".to_string());
                    if !prov.contains(&p) {
                        prov.push(p);
                    }
                }
            }
            if prov.is_empty() {
                continue;
            }
            prov
        } else {
            vec!["git_diff".to_string()]
        };
        let diff_args: Vec<&str> = match delta.kind {
            Kind::Committed => vec![
                base_revision.as_deref().unwrap_or_default(),
                plan.head.as_deref().unwrap_or_default(),
            ],
            Kind::Staged => vec!["--cached"],
            Kind::Unstaged | Kind::Untracked => vec![],
        };
        let worktree_index = scratch.join(WORKTREE_INDEX);
        let env: Vec<(&str, &Path)> = match (&scratch_env, delta.kind) {
            (Some(env), Kind::Untracked) => env.iter().map(|(k, v)| (*k, v.as_path())).collect(),
            (_, Kind::Unstaged) => vec![("GIT_INDEX_FILE", worktree_index.as_path())],
            _ => vec![],
        };
        let (content, patch) = if delta.new_mode == "040000" {
            ("text", Some(String::new())) // 巢狀 repo：oracle 輸出空 patch
        } else {
            content_and_patch(repo, delta, &diff_args, &env)?
        };
        let code = match content {
            "binary" => Some(("binary_content", "Binary content is not content-verified")),
            "unavailable" => Some((
                "non_utf8_content",
                "Non-UTF-8 content is not content-verified",
            )),
            "submodule" => Some((
                "submodule_content",
                "Submodule contents require separate inspection",
            )),
            _ => None,
        };
        if let Some((code, reason)) = code {
            content_limits.push((
                delta.kind,
                path.clone(),
                limitation(Some(&path), code, reason),
            ));
        }
        let diff = ScopeDiff {
            kind: delta.kind,
            status: delta.status.clone(),
            path: path.clone(),
            old_path: old_path.clone(),
            patch,
        };
        // rename 鏈：前一個 kind 的新路徑正是這個 diff 的舊路徑時合併成同一個 FILE。
        let existing = files
            .iter_mut()
            .position(|f| f.path == path || old_path.as_deref() == Some(f.path.as_str()));
        match existing {
            Some(i) => {
                let file = &mut files[i];
                if file.path != path {
                    if file.old_path.is_none() {
                        file.old_path = Some(file.path.clone());
                    }
                    file.path = path.clone();
                }
                if old_path.is_some() && file.old_path.is_none() {
                    file.old_path = old_path.clone();
                }
                for p in provenance {
                    if !file.provenance.contains(&p) {
                        file.provenance.push(p);
                    }
                }
                if content_rank(content) > content_rank(&file.content_kind) {
                    file.content_kind = content.to_string();
                }
                file.diffs.push(diff);
            }
            None => files.push(ScopeFile {
                path: path.clone(),
                old_path: old_path.clone(),
                change_kinds: Vec::new(),
                provenance,
                content_kind: content.to_string(),
                inspectable: true,
                diffs: vec![diff],
            }),
        }
    }
    for file in &mut files {
        let mut kinds: Vec<Kind> = file.diffs.iter().map(|d| d.kind).collect();
        kinds.sort();
        kinds.dedup();
        file.change_kinds = kinds;
        file.inspectable = file.diffs.iter().all(|d| d.patch.is_some());
    }
    files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));

    content_limits.sort_by(|a, b| (a.0, a.1.as_bytes()).cmp(&(b.0, b.1.as_bytes())));
    limitations.extend(content_limits.into_iter().map(|(_, _, l)| l));
    let missing_base = plan.change.is_some() && base_revision.is_none();
    if missing_base {
        limitations.push(limitation(None, "missing_comparison_base", MISSING_BASE));
    }
    if scope_source.starts_with("approximated_") {
        limitations.push(limitation(None, "approximated_attribution", APPROXIMATED));
    }
    if let Some(review) = plan.tracking.as_ref().and_then(|t| t.review_base.as_ref()) {
        for file in &files {
            if review
                .dirty_fingerprints
                .iter()
                .any(|f| f.path == file.path)
            {
                limitations.push(limitation(
                    Some(&file.path),
                    "preexisting_dirty",
                    PREEXISTING,
                ));
            }
        }
    }
    let status = if missing_base {
        "insufficient"
    } else if files.is_empty() {
        "empty"
    } else {
        "resolved"
    };
    let snapshot_id = snapshot_id(cfg, repo, plan, base_candidate.as_deref())?;
    Ok(ScopeReport {
        schema_version: SCHEMA_VERSION,
        snapshot_id,
        scope_source: scope_source.to_string(),
        status: status.to_string(),
        base_revision,
        base_source: base_source.map(str::to_string),
        head_revision: plan.head.clone(),
        files,
        limitations,
    })
}

/// `--check-snapshot`：以相同的 `--change`／`--base` 重算 snapshot_id 並逐字比對。
/// 不檢查 change 是否存在，base 無法解析時只是讓 preimage 的 base_candidate 為 null。
pub fn check(cfg: &Config, opts: ScopeOptions<'_>, id: &str) -> Result<()> {
    if let Some(change) = opts.change {
        validate_change_id(change)?;
    }
    let repo = open_repo(cfg)?;
    let tracking = load_tracking(cfg, opts.change)?;
    let scratch = tempdir()?;
    let unstaged = unstaged_deltas(&repo, &scratch);
    let _ = std::fs::remove_dir_all(&scratch);
    let unstaged = unstaged?;
    check_sources(&repo, &unstaged)?;
    list_untracked(&repo)?;
    let base_candidate = match opts.base {
        Some(arg) => resolve_base(&repo, arg).ok(),
        None if opts.change.is_some() => tracking
            .as_ref()
            .and_then(|t| t.review_base.as_ref())
            .and_then(|r| r.head_revision.clone()),
        None => None,
    };
    let plan = Plan {
        change: opts.change,
        base_arg: opts.base,
        tracking,
        head: head_oid(&repo),
    };
    if snapshot_id(cfg, &repo, &plan, base_candidate.as_deref())? != id {
        bail!("Scope changed since capture; discard the old snapshot and refresh scope before reporting");
    }
    Ok(())
}

/// 人類輸出：stdout 的標頭與檔案行（路徑用 Rust Debug 引號），limitations 走 stderr。
pub fn render_human(report: &ScopeReport) -> (String, String) {
    let status = match report.status.as_str() {
        "empty" => "Empty",
        "resolved" => "Resolved",
        _ => "Insufficient",
    };
    let mut out = format!("Scope: {} ({status})\n", report.scope_source);
    for file in &report.files {
        let kinds: Vec<&str> = file
            .change_kinds
            .iter()
            .map(|k| match k {
                Kind::Committed => "Committed",
                Kind::Staged => "Staged",
                Kind::Unstaged => "Unstaged",
                Kind::Untracked => "Untracked",
            })
            .collect();
        out.push_str(&format!("  {:?}: [{}]\n", file.path, kinds.join(", ")));
    }
    let err: String = report
        .limitations
        .iter()
        .map(|l| format!("{}: {}\n", l.code, l.reason))
        .collect();
    (out, err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_ids_are_validated_with_the_oracle_messages() {
        let msg = |id: &str| validate_change_id(id).unwrap_err().to_string();
        assert_eq!(msg(""), "Change ID must not be empty");
        assert_eq!(
            msg("BAD"),
            "Change ID 'BAD' must contain only lowercase letters, digits, and hyphens"
        );
        assert_eq!(
            msg("."),
            "Change ID '.' must contain only lowercase letters, digits, and hyphens"
        );
        assert_eq!(
            msg("../specs"),
            "Change ID '../specs' contains illegal characters (path separators or '..')"
        );
        assert_eq!(
            msg("demo/"),
            "Change ID 'demo/' contains illegal characters (path separators or '..')"
        );
        assert_eq!(
            msg("-x"),
            "Change ID '-x' must not start or end with a hyphen"
        );
        assert!(validate_change_id("demo").is_ok());
    }

    #[test]
    fn raw_diff_parsing_splits_typechanges_and_reads_renames() {
        let raw = b":100644 100644 aaa bbb M\0src/a.rs\0:100644 100644 ccc ccc R100\0src/b.rs\0src/b2.rs\0:100644 120000 ddd eee T\0src/l\0";
        let deltas = parse_raw(Kind::Unstaged, raw);
        let shape: Vec<(&str, &str, Option<&str>)> = deltas
            .iter()
            .map(|d| (d.status.as_str(), d.path.as_str(), d.old_path.as_deref()))
            .collect();
        assert_eq!(
            shape,
            vec![
                ("modified", "src/a.rs", None),
                ("renamed", "src/b2.rs", Some("src/b.rs")),
                ("deleted", "src/l", None),
                ("added", "src/l", None),
            ]
        );
    }

    #[test]
    fn human_output_uses_debug_quoted_paths() {
        let report = ScopeReport {
            schema_version: 1,
            snapshot_id: "x".into(),
            scope_source: "touched_tracking".into(),
            status: "resolved".into(),
            base_revision: None,
            base_source: None,
            head_revision: None,
            files: vec![ScopeFile {
                path: "src/q\"b.rs".into(),
                old_path: None,
                change_kinds: vec![Kind::Staged, Kind::Unstaged],
                provenance: vec![],
                content_kind: "text".into(),
                inspectable: true,
                diffs: vec![],
            }],
            limitations: vec![limitation(None, "approximated_attribution", APPROXIMATED)],
        };
        let (out, err) = render_human(&report);
        assert_eq!(
            out,
            "Scope: touched_tracking (Resolved)\n  \"src/q\\\"b.rs\": [Staged, Unstaged]\n"
        );
        assert_eq!(err, format!("approximated_attribution: {APPROXIMATED}\n"));
    }
}
