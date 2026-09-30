//! `spectra scope`（oracle 3.0.0，#165）。規格與實證見 `docs/reverse-engineering/scope.md`。
//!
//! 期望值取自 oracle 探測：base tree、git 身分與日期和 RE harness 相同，所以 base commit
//! 必然是 oracle 實測的 `ab81de44…`。`snapshot_id` 含 `.git/index` 原始位元組的 blob id，
//! 每個新 repo 都不同，所以改在測試裡依規格公式獨立重算（`git hash-object`）後比對。

mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

use common::{spectra, TempDir};

const BASE_COMMIT: &str = "ab81de440d47494d78249705771d9f5f585e7084";
const TASKS: &str = "## 1. Core\n\n- [ ] 1.1 first core task\n- [ ] 1.2 second core task\n\n## 2. Polish\n\n- [ ] 2.1 first polish task\n- [ ] 2.2 second polish task\n";

fn env(cmd: &mut Command) -> &mut Command {
    for k in ["GIT_DIR", "GIT_INDEX_FILE", "GIT_WORK_TREE"] {
        cmd.env_remove(k);
    }
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Golden")
        .env("GIT_AUTHOR_EMAIL", "golden@example.com")
        .env("GIT_COMMITTER_NAME", "Golden")
        .env("GIT_COMMITTER_EMAIL", "golden@example.com")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .env("NO_COLOR", "1")
}

/// 夾具用的 git。`maintenance.auto=false`：`git commit` 預設會 spawn 分離的
/// `git maintenance run --auto --detach`，它取得再釋放 `.git/objects/maintenance.lock`，
/// 高負載下晚於 commit 回傳才跑，改到 `.git/objects` 的 mtime，讓唯讀測試誤判（#222）。
fn git(dir: &Path, args: &[&str]) -> String {
    let out = env(Command::new("git")
        .args(["-c", "maintenance.auto=false"])
        .arg("-C")
        .arg(dir)
        .args(args))
    .output()
    .expect("git runs");
    assert!(out.status.success(), "git {args:?} failed: {out:?}");
    String::from_utf8(out.stdout).unwrap()
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// RE harness 的 golden base（`.gitkeep` 為空檔，所以空 blob 一開始就在 object DB 裡）。
fn base_repo(label: &str) -> TempDir {
    let dir = TempDir::new(label);
    for (rel, content) in [
        (".spectra.yaml", "spec_dir: docs/spectra\n"),
        (".gitignore", ".spectra/\n"),
        ("docs/spectra/config.yaml", "schema: spec-driven\n"),
        ("docs/spectra/specs/.gitkeep", ""),
        ("docs/spectra/changes/archive/.gitkeep", ""),
        (
            "docs/spectra/changes/demo/.openspec.yaml",
            "schema: spec-driven\ncreated: 2026-01-01\ncreated_by: Golden <golden@example.com>\n",
        ),
        ("docs/spectra/changes/demo/tasks.md", TASKS),
        ("src/a.rs", "fn a() {}\n"),
        ("src/b.rs", "fn b() {}\n"),
    ] {
        write(&dir, rel, content);
    }
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "base"]);
    assert_eq!(git(&dir, &["rev-parse", "HEAD"]).trim(), BASE_COMMIT);
    dir
}

fn scope(root: &Path, args: &[&str]) -> Output {
    env(spectra().arg("scope").args(args).current_dir(root))
        .output()
        .unwrap()
}

fn scope_json(root: &Path, args: &[&str]) -> Value {
    let mut all = args.to_vec();
    all.push("--json");
    let out = scope(root, &all);
    assert!(out.status.success(), "scope failed: {out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.ends_with("}\n"), "pretty JSON 需以換行結尾：{text:?}");
    serde_json::from_str(&text).unwrap()
}

fn blob_id(root: &Path, data: &[u8]) -> String {
    let mut child = env(Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["hash-object", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped()))
    .spawn()
    .unwrap();
    child.stdin.as_mut().unwrap().write_all(data).unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// 規格公式：`[change, base_arg, base_candidate, head, index_id, tracking_id, entries]` 的 compact
/// JSON 取 blob id。
fn expected_snapshot(
    root: &Path,
    preimage_head: [Value; 3],
    tracking: Option<&str>,
    entries: Value,
) -> String {
    let index = std::fs::read(root.join(".git/index")).unwrap();
    let tracking_id = tracking.map(|t| blob_id(root, t.as_bytes()));
    let head = git(root, &["rev-parse", "HEAD"]).trim().to_string();
    let [change, base_arg, base_candidate] = preimage_head;
    let preimage = json!([
        change,
        base_arg,
        base_candidate,
        head,
        blob_id(root, &index),
        tracking_id,
        entries
    ]);
    blob_id(root, serde_json::to_string(&preimage).unwrap().as_bytes())
}

#[test]
fn clean_repo_json_matches_oracle_shape_and_snapshot_formula() {
    let dir = base_repo("scope-clean");
    let out = scope(&dir, &["--json"]);
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let got: Value = serde_json::from_slice(&out.stdout).unwrap();
    let expected_id = expected_snapshot(
        &dir,
        [Value::Null, Value::Null, Value::Null],
        None,
        json!([]),
    );
    let expected = format!(
        "{{\n  \"schema_version\": 1,\n  \"snapshot_id\": \"{expected_id}\",\n  \"scope_source\": \"current_worktree\",\n  \"status\": \"empty\",\n  \"base_revision\": null,\n  \"base_source\": null,\n  \"head_revision\": \"{BASE_COMMIT}\",\n  \"files\": [],\n  \"limitations\": []\n}}\n"
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), expected, "{got}");
}

/// oracle p26：tracking 裡 touched 的 clean 路徑也進 snapshot entries（status 0），
/// 因此已 commit 的 touched 檔之後被改寫時，舊 snapshot 會失效。
#[test]
fn snapshot_entries_include_clean_touched_paths() {
    let dir = base_repo("scope-touched-clean");
    write(&dir, "src/a.rs", "fn a() { 1 }\n");
    git(&dir, &["commit", "-q", "-am", "impl"]);
    let tracking = format!(
        "{{\n  \"change\": \"demo\",\n  \"touched\": [\n    {{\n      \"task_id\": \"1\",\n      \"task_desc\": \"1.1 first core task\",\n      \"files\": [\n        \"src/a.rs\",\n        \"src/b.rs\"\n      ],\n      \"provenance\": \"task_baseline\"\n    }}\n  ],\n  \"review_base\": {{\n    \"head_revision\": \"{BASE_COMMIT}\",\n    \"dirty_fingerprints\": []\n  }}\n}}"
    );
    write(&dir, ".spectra/touched/demo.json", &tracking);

    let got = scope_json(&dir, &["--change", "demo"]);
    let entries = json!([
        ["src/a.rs", 0, blob_id(&dir, b"fn a() { 1 }\n")],
        ["src/b.rs", 0, blob_id(&dir, b"fn b() {}\n")],
    ]);
    let expected = expected_snapshot(
        &dir,
        [json!("demo"), Value::Null, json!(BASE_COMMIT)],
        Some(&tracking),
        entries,
    );
    assert_eq!(got["snapshot_id"], json!(expected));
    assert_eq!(got["scope_source"], json!("touched_tracking"));
    assert_eq!(got["base_source"], json!("review_base"));
    let paths: Vec<&str> = got["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["src/a.rs"]);
}

#[test]
fn check_snapshot_reports_current_then_changed() {
    let dir = base_repo("scope-check");
    write(&dir, "src/a.rs", "fn a() { 1 }\n");
    let id = scope_json(&dir, &[])["snapshot_id"]
        .as_str()
        .unwrap()
        .to_string();

    let human = scope(&dir, &["--check-snapshot", &id]);
    assert!(human.status.success());
    assert_eq!(
        String::from_utf8(human.stdout).unwrap(),
        format!("Scope snapshot is current: {id}\n")
    );
    let as_json = scope(&dir, &["--check-snapshot", &id, "--json"]);
    assert_eq!(
        String::from_utf8(as_json.stdout).unwrap(),
        format!("{{\"snapshot_id\":\"{id}\",\"status\":\"current\"}}\n")
    );

    write(&dir, "src/a.rs", "fn a() { 2 }\n");
    let stale = scope(&dir, &["--check-snapshot", &id, "--json"]);
    assert_eq!(stale.status.code(), Some(1));
    assert!(stale.stdout.is_empty());
    assert_eq!(
        String::from_utf8(stale.stderr).unwrap(),
        "Error: Scope changed since capture; discard the old snapshot and refresh scope before reporting\n"
    );
}

/// 每個檔案（含 `.git/`）的大小、mtime、內容。
fn tree_state(root: &Path) -> Vec<(std::path::PathBuf, u64, std::time::SystemTime, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            if meta.is_dir() {
                stack.push(path.clone());
            }
            let content = if meta.is_file() {
                std::fs::read(&path).unwrap()
            } else {
                Vec::new()
            };
            out.push((path, meta.len(), meta.modified().unwrap(), content));
        }
    }
    out.sort();
    out
}

/// `tree_state` 前後不同時，列出每個有差異的路徑與變動的欄位（size／mtime／內容），
/// 失敗訊息才分得出是 scope 寫了檔還是別的行程動了 `.git`（#222）。
fn assert_tree_unchanged(
    root: &Path,
    before: &[(std::path::PathBuf, u64, std::time::SystemTime, Vec<u8>)],
    what: &str,
) {
    let after = tree_state(root);
    if before == after.as_slice() {
        return;
    }
    let index = |s: &[(std::path::PathBuf, u64, std::time::SystemTime, Vec<u8>)]| {
        s.iter()
            .map(|(p, len, mtime, content)| (p.clone(), (*len, *mtime, content.clone())))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let (b, a) = (index(before), index(&after));
    let mut diffs = Vec::new();
    for path in b
        .keys()
        .chain(a.keys())
        .collect::<std::collections::BTreeSet<_>>()
    {
        let rel = path.strip_prefix(root).unwrap_or(path).display();
        match (b.get(path), a.get(path)) {
            (Some(_), None) => diffs.push(format!("{rel}: 被刪除")),
            (None, Some(_)) => diffs.push(format!("{rel}: 新出現")),
            (Some(x), Some(y)) if x != y => {
                let mut fields = Vec::new();
                if x.0 != y.0 {
                    fields.push(format!("size {} -> {}", x.0, y.0));
                }
                if x.1 != y.1 {
                    fields.push(format!("mtime {:?} -> {:?}", x.1, y.1));
                }
                if x.2 != y.2 {
                    fields.push("內容".to_string());
                }
                diffs.push(format!("{rel}: {}", fields.join(", ")));
            }
            _ => {}
        }
    }
    panic!("{what}：\n{}", diffs.join("\n"));
}

/// oracle 唯讀（p08a、p17g）。空 blob 已在 object DB 裡時，`git add -N` 會 freshen 它的
/// mtime——untracked diff 的暫存 index 必須避開這個寫入。
#[test]
fn scope_is_read_only_even_with_untracked_files() {
    let dir = base_repo("scope-readonly");
    write(&dir, "src/new.rs", "fn n() {}\n");
    write(&dir, "src/empty.rs", "");
    write(&dir, "src/a.rs", "fn a() { 1 }\n");
    let before = tree_state(&dir);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    for args in [
        &["--json"][..],
        &[][..],
        &["--change", "demo", "--base", "HEAD"][..],
    ] {
        let out = scope(&dir, args);
        assert!(out.status.success(), "{args:?}: {out:?}");
    }
    assert_tree_unchanged(&dir, &before, "scope 改動了工作樹或 .git");
}

/// oracle p17g：index 的 stat 資料過期時，`git diff` 會無視 `GIT_OPTIONAL_LOCKS` 把刷新結果
/// 寫回 index；scope 必須改用 index 複本，真正的 `.git/index` 不能動。
#[test]
fn stale_index_stat_is_not_written_back() {
    let dir = base_repo("scope-stale-stat");
    let b = std::fs::File::options()
        .write(true)
        .open(dir.join("src/b.rs"))
        .unwrap();
    b.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(100))
        .unwrap();
    drop(b);
    write(&dir, "src/a.rs", "fn a() { 1 }\n");
    let before = tree_state(&dir);
    for args in [
        &["--json"][..],
        &["--change", "demo"][..],
        &["--base", "HEAD", "--json"][..],
    ] {
        let out = scope(&dir, args);
        assert!(out.status.success(), "{args:?}: {out:?}");
    }
    let id = scope_json(&dir, &[])["snapshot_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(scope(&dir, &["--check-snapshot", &id]).status.success());
    assert_tree_unchanged(&dir, &before, "scope 改寫了 .git/index 或工作樹");
}

/// oracle p17f：dirty 路徑的上層目錄被換成 symlink 時直接失敗，不讀穿。
#[cfg(unix)]
#[test]
fn dirty_path_under_directory_symlink_is_fatal() {
    let dir = base_repo("scope-dirsymlink");
    write(&dir, "src/sub/x.rs", "x\n");
    write(&dir, "other/x.rs", "y\n");
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-q", "-m", "sub"]);
    std::fs::remove_dir_all(dir.join("src/sub")).unwrap();
    std::os::unix::fs::symlink("../other", dir.join("src/sub")).unwrap();
    let out = scope(&dir, &["--json"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: Scope path \"src/sub/x.rs\" traverses a directory symlink; target was not read\n"
    );
}

fn diffs_of<'a>(report: &'a Value, path: &str) -> &'a Vec<Value> {
    report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == json!(path))
        .unwrap_or_else(|| panic!("{path} 不在 files：{report}"))["diffs"]
        .as_array()
        .unwrap()
}

/// 與 oracle（libgit2）逐位元組相同的 patch：untracked 新檔、空新檔的 `---`／`+++`、
/// 含空白路徑沒有行尾 tab。
#[test]
fn untracked_patches_match_libgit2() {
    let dir = base_repo("scope-untracked");
    write(&dir, "src/new.rs", "fn n() {}\n");
    write(&dir, "src/empty.rs", "");
    write(&dir, "src/a b.rs", "x\n");
    let got = scope_json(&dir, &[]);
    let new_oid = &blob_id(&dir, b"fn n() {}\n")[..7];
    let x_oid = &blob_id(&dir, b"x\n")[..7];
    assert_eq!(
        diffs_of(&got, "src/new.rs")[0]["patch"],
        json!(format!(
            "diff --git i/src/new.rs w/src/new.rs\nnew file mode 100644\nindex 0000000..{new_oid}\n--- /dev/null\n+++ w/src/new.rs\n@@ -0,0 +1 @@\n+fn n() {{}}\n"
        ))
    );
    assert_eq!(
        diffs_of(&got, "src/empty.rs")[0]["patch"],
        json!("diff --git i/src/empty.rs w/src/empty.rs\nnew file mode 100644\nindex 0000000..e69de29\n--- /dev/null\n+++ w/src/empty.rs\n")
    );
    assert_eq!(
        diffs_of(&got, "src/a b.rs")[0]["patch"],
        json!(format!(
            "diff --git i/src/a b.rs w/src/a b.rs\nnew file mode 100644\nindex 0000000..{x_oid}\n--- /dev/null\n+++ w/src/a b.rs\n@@ -0,0 +1 @@\n+x\n"
        ))
    );
}

/// oracle p03h：worktree typechange 拆成 deleted 與 added 兩個 diff，各自只帶自己那段 patch。
#[cfg(unix)]
#[test]
fn typechange_splits_into_two_patches() {
    let dir = base_repo("scope-typechange");
    std::fs::remove_file(dir.join("src/a.rs")).unwrap();
    std::os::unix::fs::symlink("b.rs", dir.join("src/a.rs")).unwrap();
    let got = scope_json(&dir, &[]);
    let diffs = diffs_of(&got, "src/a.rs");
    assert_eq!(diffs.len(), 2);
    assert_eq!(diffs[0]["status"], json!("deleted"));
    assert_eq!(
        diffs[0]["patch"],
        json!("diff --git i/src/a.rs w/src/a.rs\ndeleted file mode 100644\nindex ca05282..0000000\n--- i/src/a.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-fn a() {}\n")
    );
    assert_eq!(diffs[1]["status"], json!("added"));
    assert_eq!(
        diffs[1]["patch"],
        json!("diff --git i/src/a.rs w/src/a.rs\nnew file mode 120000\nindex 0000000..1541615\n--- /dev/null\n+++ w/src/a.rs\n@@ -0,0 +1 @@\n+b.rs\n\\ No newline at end of file\n")
    );
    assert_eq!(got["files"][0]["content_kind"], json!("symlink"));
}

/// oracle p04i／p13c：不可讀的 untracked 檔在 capture 與 `--check-snapshot` 都是致命錯誤。
#[cfg(unix)]
#[test]
fn unreadable_untracked_file_is_fatal() {
    use std::os::unix::fs::PermissionsExt;
    let dir = base_repo("scope-unreadable");
    write(&dir, "src/u.rs", "secret\n");
    let path = dir.join("src/u.rs");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&path).is_ok() {
        // 以 root 執行時權限檢查無效，這個情境無法重現。
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        return;
    }
    for args in [&["--json"][..], &["--check-snapshot", "abc", "--json"][..]] {
        let out = scope(&dir, args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert_eq!(
            String::from_utf8(out.stderr).unwrap(),
            "Error: Cannot read source \"src/u.rs\": Permission denied (os error 13)\n",
            "{args:?}"
        );
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn human_output_and_limitations_go_to_stderr() {
    let dir = base_repo("scope-human");
    write(&dir, "src/a.rs", "fn a() { 1 }\n");
    git(&dir, &["add", "src/a.rs"]);
    write(&dir, "src/a.rs", "fn a() { 2 }\n");
    let out = scope(&dir, &["--change", "demo"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "Scope: approximated_worktree (Insufficient)\n  \"src/a.rs\": [Staged, Unstaged]\n"
    );
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "missing_comparison_base: No validated pre-implementation base; provide --base explicitly. Current differences do not establish complete historical coverage.\napproximated_attribution: Scope is approximated: trusted touched attribution is unavailable; this is not proof of hunk ownership.\n"
    );
}

#[test]
fn argument_errors_match_oracle() {
    let dir = base_repo("scope-errors");
    for (args, stderr) in [
        (&["--change", "BAD"][..], "Error: Change ID 'BAD' must contain only lowercase letters, digits, and hyphens\n"),
        (&["--change", "../specs"][..], "Error: Change ID '../specs' contains illegal characters (path separators or '..')\n"),
        (&["--change=-x"][..], "Error: Change ID '-x' must not start or end with a hyphen\n"),
        (&["--change", "nope"][..], "Error: Change \"nope\" not found\n"),
        (&["--base", "nosuchrev"][..], "Error: Invalid comparison base \"nosuchrev\": revspec 'nosuchrev' not found; class=Reference (4); code=NotFound (-3)\n"),
    ] {
        let out = scope(&dir, args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert_eq!(String::from_utf8(out.stderr).unwrap(), stderr, "{args:?}");
    }
}
