//! W7d：`drift` 的 task 碰撞（blocked／maybe-resolved）與 `instructions apply` 的 preflight，
//! 規則見 docs/reverse-engineering/drift.md「3. Tasks」與 artifact-workflow.md「Preflight」
//! （oracle 3.0.0）。commit 都落在 `created` 之後好幾天，避開 git `--since` 在 created
//! 當天依執行時刻而變的 cutoff。

mod common;

use std::path::Path;
use std::process::Command;

use serde_json::{json, Value};

use common::{spectra, TempDir};

fn git(dir: &Path, args: &[&str], date: &str) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@x")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@x")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn rev(dir: &Path, name: &str) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", name])
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim()[..7].to_string()
}

const TASKS: &str = "## 1. Work\n\n- [ ] 1.1 Check `docs/a.md` [est: 1h]\n- [ ] 1.2 Add login flow\n- [x] 1.3 Fix docs/a.md parser\n- [ ] 1.4 Update lib/x.ts\n- [ ] 1.5 Fix login parser\n";

/// created 2026-01-01；base commit 在那之前，之後三個 commit：碰 `docs/a.md`（與 `lib/`、
/// `mobile/`）、改 README（subject 含 add／login）、再碰一次 `docs/a.md`。
fn project(label: &str) -> TempDir {
    let dir = TempDir::new(label);
    write(&dir, ".spectra.yaml", "spec_dir: openspec\n");
    write(&dir, "openspec/config.yaml", "schema: spec-driven\n");
    write(
        &dir,
        "openspec/changes/c/.openspec.yaml",
        "schema: spec-driven\ncreated: 2026-01-01\n",
    );
    write(&dir, "openspec/changes/c/tasks.md", TASKS);
    write(&dir, "docs/a.md", "a\n");
    write(&dir, "lib/x.ts", "x\n");
    write(&dir, "mobile/y.ts", "y\n");
    write(&dir, "README.md", "r\n");
    git(&dir, &["init", "-q", "-b", "main"], "2025-12-01T00:00:00Z");
    git(&dir, &["add", "-A"], "2025-12-01T00:00:00Z");
    git(&dir, &["commit", "-qm", "base"], "2025-12-01T00:00:00Z");
    write(&dir, "docs/a.md", "a2\n");
    write(&dir, "lib/x.ts", "x2\n");
    write(&dir, "mobile/y.ts", "y2\n");
    git(&dir, &["commit", "-qam", "touch a"], "2026-02-10T12:00:00Z");
    write(&dir, "README.md", "r2\n");
    git(
        &dir,
        &["commit", "-qam", "add login \"q\" flow"],
        "2026-02-11T12:00:00Z",
    );
    // log 順序中第一個（最新）碰 docs/a.md 的 commit 才是 blocked 的回報對象。
    write(&dir, "docs/a.md", "a3\n");
    git(
        &dir,
        &["commit", "-qam", "touch a again"],
        "2026-02-12T12:00:00Z",
    );
    dir
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    spectra()
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

#[test]
fn drift_reports_blocked_and_maybe_resolved_tasks() {
    let dir = project("collisions");
    let touch = rev(&dir, "HEAD");
    let login = rev(&dir, "HEAD~1");
    let out = run(&dir, &["drift", "c", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        v["tasks_blocked_external"],
        json!([{
            "task_description": "1.1 Check `docs/a.md` [est: 1h]",
            "commit_sha": touch,
            "commit_subject": "touch a again",
            "commit_date": "2026-02-12",
        }])
    );
    assert_eq!(
        v["tasks_maybe_resolved"],
        json!([{
            "task_description": "1.2 Add login flow",
            "commit_sha": login,
            "commit_subject": "add login \"q\" flow",
            "commit_date": "2026-02-11",
        }])
    );
    assert_eq!(
        v["dimensions"][2],
        json!({"kind": "Tasks", "status": "1 blocked, 1 maybe-done", "score": 2, "contributes_to_total": true})
    );
    // 1.5 Fix login parser：`add login …` 含關鍵字 login 但不含動詞 fix，不算。
    // 鍵的順序：task_description、commit_sha、commit_subject、commit_date。
    let text = String::from_utf8(out.stdout).unwrap();
    let order: Vec<usize> = [
        "\"task_description\"",
        "\"commit_sha\"",
        "\"commit_subject\"",
        "\"commit_date\"",
    ]
    .iter()
    .map(|k| text.find(k).unwrap())
    .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");

    let human = String::from_utf8(run(&dir, &["drift", "c"]).stdout).unwrap();
    assert!(
        human.contains(&format!(
            "\nTasks blocked by external changes\n  - 1.1 Check `docs/a.md` [est: 1h] → {touch} \"touch a again\" (2026-02-12)\n\nTasks possibly resolved elsewhere\n  - 1.2 Add login flow → {login} \"add login \"q\" flow\" (2026-02-11)\n\nSeverity: "
        )),
        "{human}"
    );
}

#[test]
fn drift_without_git_reports_git_unavailable() {
    let dir = TempDir::new("collisions-nogit");
    write(&dir, ".spectra.yaml", "spec_dir: openspec\n");
    write(
        &dir,
        "openspec/changes/c/.openspec.yaml",
        "schema: spec-driven\ncreated: 2026-01-01\n",
    );
    write(&dir, "openspec/changes/c/tasks.md", TASKS);
    let v: Value = serde_json::from_slice(&run(&dir, &["drift", "c", "--json"]).stdout).unwrap();
    assert_eq!(v["dimensions"][2]["status"], "git unavailable");
    assert_eq!(v["dimensions"][2]["score"], 0);
}

#[test]
fn preflight_follows_the_affected_code_grammar_and_string_dates() {
    let dir = project("preflight");
    write(
        &dir,
        "openspec/changes/c/proposal.md",
        "## Why\n\nx.\n\n## Impact\n\nAffected code:\n- `docs/a.md`, `src/missing.rs`\n- New: `src/new.rs`\n- `plugins/p/SKILL.md` (修改)\n",
    );
    // `mobile/y.ts` 存在且在 created 之後改過，但 `mobile/` 不在白名單，不是候選。
    write(
        &dir,
        "openspec/changes/c/design.md",
        "Touches `lib/x.ts` and `mobile/y.ts`.\n",
    );
    let apply = |dir: &Path| -> Value {
        let out = run(dir, &["instructions", "apply", "--change", "c", "--json"]);
        assert!(out.status.success(), "{out:?}");
        serde_json::from_slice(&out.stdout).unwrap()
    };
    let v = apply(&dir);
    assert_eq!(
        v["preflight"]["missingFiles"],
        json!([
            {"path": "src/missing.rs", "referencedIn": "proposal"},
            {"path": "plugins/p/SKILL.md", "referencedIn": "proposal"},
        ])
    );
    // AC 項目先、再來是 design 的反引號（`lib/` 在白名單，`mobile/` 不在）。
    assert_eq!(
        v["preflight"]["driftedFiles"],
        json!([
            {"path": "docs/a.md", "lastCommit": "2026-02-12", "changeCreated": "2026-01-01"},
            {"path": "lib/x.ts", "lastCommit": "2026-02-10", "changeCreated": "2026-01-01"},
        ])
    );

    // 字串比較：`"2026-02-10" > "2026-2-1"` 不成立，所以沒有 drifted。
    write(
        &dir,
        "openspec/changes/c/.openspec.yaml",
        "schema: spec-driven\ncreated: 2026-2-1\n",
    );
    assert_eq!(apply(&dir)["preflight"]["driftedFiles"], json!([]));
}
