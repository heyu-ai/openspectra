//! W7b：oracle 3.0.0 的 `dormancy`、`recommended_action`、apply `tasks[]` 的前置依賴欄位，
//! 以及 `task done` 回報去掉 `[P] `／`[after: …]` 的 description。期望值取自 oracle 探測
//! （docs/reverse-engineering/drift.md、artifact-workflow.md）。

mod common;

use std::path::Path;
use std::process::{Command, Output};

use chrono::{Duration, Local};
use serde_json::{json, Value};

use common::{spectra, TempDir};

fn env(cmd: &mut Command) -> &mut Command {
    for k in ["GIT_DIR", "GIT_INDEX_FILE", "GIT_WORK_TREE"] {
        cmd.env_remove(k);
    }
    cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "T")
        .env("GIT_AUTHOR_EMAIL", "t@x")
        .env("GIT_COMMITTER_NAME", "T")
        .env("GIT_COMMITTER_EMAIL", "t@x")
        .env("NO_COLOR", "1")
}

fn git(dir: &Path, args: &[&str], author_date: Option<&str>) {
    let mut cmd = Command::new("git");
    env(cmd.arg("-C").arg(dir).args(args));
    // 只設 author 時間（committer 維持現在），才分辨得出 oracle 用的是 author 時間。
    if let Some(date) = author_date {
        cmd.env("GIT_AUTHOR_DATE", date);
    }
    let out = cmd.output().expect("git runs");
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// `openspec/` 專案，change `c` 的 created 為今天往前 `age_days` 天。
fn project(label: &str, age_days: i64, tasks: Option<&str>) -> TempDir {
    let dir = TempDir::new(label);
    write(&dir, ".spectra.yaml", "spec_dir: openspec\n");
    write(&dir, "openspec/config.yaml", "schema: spec-driven\n");
    write(&dir, "openspec/specs/.gitkeep", "");
    let created = (Local::now().date_naive() - Duration::days(age_days)).format("%Y-%m-%d");
    write(
        &dir,
        "openspec/changes/c/.openspec.yaml",
        &format!("schema: spec-driven\ncreated: {created}\n"),
    );
    write(&dir, "openspec/changes/c/proposal.md", "## Why\n\nx.\n");
    if let Some(tasks) = tasks {
        write(&dir, "openspec/changes/c/tasks.md", tasks);
    }
    dir
}

fn run(root: &Path, args: &[&str]) -> Output {
    env(spectra().args(args).current_dir(root))
        .output()
        .unwrap()
}

fn json_of(root: &Path, args: &[&str]) -> Value {
    let out = run(root, args);
    assert!(out.status.success(), "{args:?}: {out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

fn keys(v: &Value) -> Vec<&str> {
    v.as_object().unwrap().keys().map(String::as_str).collect()
}

/// 以 `preserve_order` 以外的方式取得 key 順序：直接掃 pretty JSON 的頂層 key。
fn top_level_keys(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.starts_with("  \"") && !l.starts_with("   "))
        .map(|l| l[3..l[3..].find('"').unwrap() + 3].to_string())
        .collect()
}

#[test]
fn drift_json_has_oracle_dormancy_and_recommended_action() {
    let dir = project("w7b-drift-nogit", 27, None);
    let out = run(&dir, &["drift", "c", "--json"]);
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        top_level_keys(&text),
        [
            "dormancy",
            "change_id",
            "created",
            "last_commit",
            "dimensions",
            "broken_anchors",
            "unresolved_anchors",
            "tasks_maybe_resolved",
            "tasks_blocked_external",
            "commits_since_created",
            "total_score",
            "severity",
            "recommended_action",
            "primary_recommendation",
        ]
    );
    let v: Value = serde_json::from_str(&text).unwrap();
    // 不是 git repo：dormancy 為 Git unavailable；time 維度加上後綴；沒有 tasks.md。
    assert_eq!(
        v["dormancy"],
        json!({"status": "unknown", "reason": "Git unavailable", "age_days": 27, "idle_days": null})
    );
    assert_eq!(
        v["dimensions"][0]["status"],
        json!("stale (27d), git unavailable")
    );
    assert_eq!(v["dimensions"][2]["status"], json!("no tasks.md"));
    assert_eq!(
        v["recommended_action"],
        json!({"action_kind": "apply", "change_name": "c", "flags": []})
    );
}

#[test]
fn created_without_schema_counts_as_missing() {
    let dir = project("w7b-created-only", 27, None);
    write(
        &dir,
        "openspec/changes/c/.openspec.yaml",
        "created: 2026-01-01\n",
    );
    let v = json_of(&dir, &["drift", "c", "--json"]);
    assert_eq!(v["created"], Value::Null);
    assert_eq!(
        v["dimensions"][0]["status"],
        json!("no created date, git unavailable")
    );
    assert_eq!(
        v["dormancy"]["reason"],
        json!("created date missing or invalid")
    );
}

#[test]
fn dormancy_uses_the_last_directory_commit_author_date() {
    let dir = project("w7b-dormant", 20, Some("- [ ] 1.1 a\n"));
    git(&dir, &["init", "-q", "-b", "main"], None);
    git(&dir, &["add", "-A"], None);
    let ten_days_ago = (chrono::Utc::now() - Duration::days(10)).to_rfc3339();
    git(&dir, &["commit", "-q", "-m", "c"], Some(&ten_days_ago));
    // 之後與 change 目錄無關的 commit 不影響 idle。
    write(&dir, "README.md", "x\n");
    git(&dir, &["add", "README.md"], None);
    git(&dir, &["commit", "-q", "-m", "other"], None);

    let expected = json!({
        "status": "triggered",
        "reason": "older than five days; no directory commit in three days",
        "age_days": 20,
        "idle_days": 10,
    });
    let drift = json_of(&dir, &["drift", "c", "--json"]);
    assert_eq!(drift["dormancy"], expected);
    let apply = run(&dir, &["instructions", "apply", "--change", "c", "--json"]);
    let text = String::from_utf8(apply.stdout).unwrap();
    assert_eq!(top_level_keys(&text)[0], "dormancy");
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["dormancy"],
        expected
    );
    let compact = json_of(
        &dir,
        &[
            "instructions",
            "apply",
            "--change",
            "c",
            "--json",
            "--compact",
        ],
    );
    assert_eq!(compact["dormancy"], expected);
    assert!(keys(&compact).contains(&"dormancy"));
}

fn apply_tasks(tasks_md: &str) -> Vec<Value> {
    let dir = project("w7b-apply", 0, Some(tasks_md));
    let v = json_of(&dir, &["instructions", "apply", "--change", "c", "--json"]);
    v["tasks"].as_array().unwrap().clone()
}

fn row(t: &Value) -> Value {
    json!([
        t["number"],
        t["prerequisites"],
        t["unresolved_prerequisites"],
        t["cycle_member"],
        t["mixed_format"],
        t["parallel"],
        t["done"],
        t["description"]
    ])
}

/// oracle 3.0.0 的 apply tasks（探測 g05、g08、g10–g12、h02、h03）。
#[test]
fn apply_tasks_follow_the_oracle_dependency_graph() {
    let tasks = apply_tasks(
        "- [x] 1.1 a\n- [ ] 1.2 [after: 1.1] b\n- [ ] 1.3 [after: 1.1] c\n- [ ] 1.4 d\n",
    );
    let rows: Vec<Value> = tasks.iter().map(row).collect();
    assert_eq!(
        rows,
        [
            json!(["1.1", [], [], false, false, false, true, "1.1 a"]),
            json!(["1.2", ["1.1"], [], false, false, true, false, "1.2 b"]),
            json!(["1.3", ["1.1"], [], false, false, true, false, "1.3 c"]),
            json!(["1.4", [], [], false, false, true, false, "1.4 d"]),
        ]
    );
    // 完整欄位順序與 snake_case key。
    let first = serde_json::to_string(&tasks[1]).unwrap();
    assert!(first.contains("\"unresolved_prerequisites\""), "{first}");

    let cycle: Vec<Value> = apply_tasks(
        "- [ ] 1.1 [after: 1.2] a\n- [ ] 1.2 [after: 1.1] b\n- [ ] 1.3 [after: 1.2] c\n- [ ] 1.4 [after: 1.4] self\n- [ ] 1.5 e\n",
    )
    .iter()
    .map(|t| json!([t["cycle_member"], t["parallel"]]))
    .collect();
    assert_eq!(
        cycle,
        [
            json!([true, false]),
            json!([true, false]),
            json!([false, false]),
            json!([true, false]),
            json!([false, false])
        ]
    );

    let mixed: Vec<Value> =
        apply_tasks("- [ ] [P] 1.1 a\n- [ ] [P] 1.2 [after: 1.1] b\n- [ ] 1.3 c\n")
            .iter()
            .map(|t| json!([t["mixed_format"], t["parallel"], t["description"]]))
            .collect();
    assert_eq!(
        mixed,
        [
            json!([false, true, "1.1 a"]),
            json!([true, false, "1.2 b"]),
            json!([false, true, "1.3 c"])
        ]
    );

    let unresolved: Vec<Value> =
        apply_tasks("- [ ] 1.1 a\n- [ ] 1.2 [after: 9.9] b\n- [ ] 1.3 c\n")
            .iter()
            .map(|t| json!([t["unresolved_prerequisites"], t["parallel"]]))
            .collect();
    assert_eq!(
        unresolved,
        [
            json!([[], true]),
            json!([["9.9"], false]),
            json!([[], true])
        ]
    );

    // 沒有任何前置宣告（含只有 `[after: ]`）：沿用 legacy `[P]`。
    let legacy: Vec<Value> = apply_tasks("- [ ] [P] 1.1 [after: ] a\n- [ ] 1.2 b\n")
        .iter()
        .map(|t| t["parallel"].clone())
        .collect();
    assert_eq!(legacy, [json!(true), json!(false)]);
}

/// oracle 3.0.0 `task done`（探測 p26）：JSON、human 訊息與 touched 紀錄都用正規化後的描述。
#[test]
fn task_done_reports_the_normalized_description() {
    let dir = project(
        "w7b-task-done",
        0,
        Some("## 1. G\n\n- [ ] 1.1 first\n- [ ] 1.2 [after: 1.1] second\n- [ ] [P] 1.3 [after: 1.1] third\n"),
    );
    write(&dir, "a.rs", "fn a() {}\n");
    git(&dir, &["init", "-q", "-b", "main"], None);
    git(&dir, &["add", "-A"], None);
    git(&dir, &["commit", "-q", "-m", "init"], None);

    let done = json_of(
        &dir,
        &[
            "task", "done", "2", "--change", "c", "--file", "a.rs", "--json",
        ],
    );
    assert_eq!(done["task_desc"], json!("1.2 second"));
    let human = run(
        &dir,
        &["task", "done", "3", "--change", "c", "--file", "a.rs"],
    );
    assert_eq!(
        String::from_utf8(human.stdout).unwrap(),
        "✓ Task 3 marked as done: 1.3 third\n"
    );
    let touched: Value =
        serde_json::from_slice(&std::fs::read(dir.join(".spectra/touched/c.json")).unwrap())
            .unwrap();
    let descs: Vec<&str> = touched["touched"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["task_desc"].as_str().unwrap())
        .collect();
    assert_eq!(descs, ["1.2 second", "1.3 third"]);
    // tasks.md 本身保留原文。
    assert_eq!(
        std::fs::read_to_string(dir.join("openspec/changes/c/tasks.md")).unwrap(),
        "## 1. G\n\n- [ ] 1.1 first\n- [x] 1.2 [after: 1.1] second\n- [x] [P] 1.3 [after: 1.1] third\n"
    );
}
