//! `task start`／`task done` 對 oracle 3.0.0 golden 的完整重播比對（#110、#190）。
//!
//! `docs/reverse-engineering/golden/task-done-3.0.0.json` 由
//! `scripts/capture-task-done.py` 產生，自帶 base tree 與每個 setup 步驟；本測試
//! 依樣重播全部情境，逐步比對 exit、stdout、stderr、`tasks.md` bytes、
//! `.spectra/touched/<change>.json` bytes，以及 `.spectra/` 底下的檔案清單
//! （含 `touched/<change>.lock` 與 `task-baselines/`）。
//!
//! D7（docs/migration-plan.md）裁決以 v3.0.0 的 per-task baseline 取代 OpenSpectra
//! 原本的 per-change baseline（#98），所以這裡不再有 transform 或分歧 ledger：
//! 每個欄位都必須與 oracle 逐位元組相同。
//!
//! 截短保護：golden 的情境名稱必須恰好是 `SCENARIOS`（依 golden 順序），且實際
//! 比對的欄位數必須等於 `EXPECTED_COMPARED_FIELDS`；每個 step 的 `expect` 缺少
//! 欄位即 panic，不會靜默當成 Null。

mod common;

use std::path::Path;
use std::process::{Command, Output};

use serde_json::{json, Value};

use common::{spectra, TempDir};

/// golden 的情境，依 golden 中的順序。
const SCENARIOS: [&str; 14] = [
    "grouped-numbering-ignores-headers-and-labels",
    "invalid-task-ids",
    "evaluation-order-id-vs-change",
    "marker-variants",
    "line-endings-and-final-newline",
    "dirty-path-kinds",
    "task-start-baseline",
    "baseline-exclusions",
    "explicit-file-attribution",
    "first-task-wins-attribution",
    "spectra-dir-not-gitignored",
    "spectra-dir-committed",
    "not-a-git-repo",
    "change-autodetect",
];
/// 共 60 個 spectra step，每個比對 6 個欄位。
const EXPECTED_COMPARED_FIELDS: usize = 60 * 6;

fn load(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/reverse-engineering/golden")
        .join(name);
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("讀取 {path:?}：{e}"));
    serde_json::from_str(&raw).unwrap()
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} 不是字串：{v}"))
}

/// 取出 step 的 oracle 期望值；缺欄位即 panic，避免被截短的 golden 靜默變成 Null。
fn expect_of<'a>(step: &'a Value, field: &str) -> &'a Value {
    step["expect"]
        .as_object()
        .unwrap_or_else(|| panic!("step 沒有 expect 物件：{step}"))
        .get(field)
        .unwrap_or_else(|| panic!("step 的 expect 缺少 {field}：{step}"))
}

fn with_git_env(cmd: &mut Command, golden: &Value) {
    // 繼承來的 repo 位置覆寫會讓 git 指向別的 repo，而不是暫存 repo。
    for k in ["GIT_DIR", "GIT_INDEX_FILE", "GIT_WORK_TREE"] {
        cmd.env_remove(k);
    }
    for (k, v) in golden["git_env"].as_object().unwrap() {
        cmd.env(k, v.as_str().unwrap());
    }
    cmd.env("NO_COLOR", "1");
}

fn git(repo: &Path, golden: &Value, args: &[&str]) {
    let mut cmd = Command::new("git");
    with_git_env(&mut cmd, golden);
    let out = cmd.arg("-C").arg(repo).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?} 失敗：{out:?}");
}

fn run_spectra(repo: &Path, golden: &Value, args: &[&str]) -> Output {
    let mut cmd = spectra();
    with_git_env(&mut cmd, golden);
    cmd.args(args).current_dir(repo).output().unwrap()
}

fn write(repo: &Path, rel: &str, content: &str) {
    let path = repo.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn read_opt(path: &Path) -> Value {
    match std::fs::read_to_string(path) {
        Ok(s) => Value::String(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(e) => panic!("讀取 {path:?}：{e}"),
    }
}

/// `.spectra/` 底下所有檔案的相對路徑（排序後），與 capture 腳本的 `state_files` 相同。
fn state_files(repo: &Path) -> Value {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut files = Vec::new();
    walk(repo, &repo.join(".spectra"), &mut files);
    files.sort();
    json!(files)
}

fn args_of(step: &Value) -> Vec<&str> {
    step["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect()
}

/// Build the scenario's base tree (and initial commit) in a fresh temp dir.
fn seed(scenario: &Value, golden: &Value) -> TempDir {
    let repo = TempDir::new("task-done-golden");
    for (rel, content) in scenario["base"].as_object().unwrap() {
        write(&repo, rel, content.as_str().unwrap());
    }
    if scenario["git"].as_bool().unwrap() {
        git(&repo, golden, &["init", "-q", "-b", "main"]);
        git(&repo, golden, &["add", "-A"]);
        git(&repo, golden, &["commit", "-q", "-m", "base"]);
    }
    repo
}

#[test]
fn task_start_and_done_replay_the_oracle_golden_byte_for_byte() {
    let golden = load("task-done-3.0.0.json");
    assert_eq!(str_of(&golden, "oracle_version"), "3.0.0");
    let root_token = str_of(&golden, "root_token");

    let names: Vec<&str> = golden["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .map(|sc| str_of(sc, "name"))
        .collect();
    assert_eq!(
        names, SCENARIOS,
        "golden 的情境與 SCENARIOS 不符（缺漏、多出或順序不同）"
    );

    let mut failures = Vec::new();
    let mut compared = 0;
    for scenario in golden["scenarios"].as_array().unwrap() {
        let name = str_of(scenario, "name");
        let repo = seed(scenario, &golden);
        let repo_str = repo.to_string_lossy().to_string();
        let mut ordinal = 0;
        for step in scenario["steps"].as_array().unwrap() {
            match step["op"].as_str().unwrap() {
                "write" => write(&repo, str_of(step, "path"), str_of(step, "content")),
                "remove" => std::fs::remove_file(repo.join(str_of(step, "path"))).unwrap(),
                "git" => git(&repo, &golden, &args_of(step)),
                "spectra" => {
                    ordinal += 1;
                    let args = args_of(step);
                    let out = run_spectra(&repo, &golden, &args);
                    let normalize = |b: &[u8]| {
                        Value::String(String::from_utf8_lossy(b).replace(&repo_str, root_token))
                    };
                    let got = [
                        ("exit", json!(out.status.code())),
                        ("stdout", normalize(&out.stdout)),
                        ("stderr", normalize(&out.stderr)),
                        (
                            "tasks_md",
                            read_opt(&repo.join(str_of(&golden, "tasks_md"))),
                        ),
                        (
                            "touched",
                            read_opt(&repo.join(str_of(&golden, "touched_json"))),
                        ),
                        ("spectra_state_files", state_files(&repo)),
                    ];
                    for (field, actual) in got {
                        compared += 1;
                        let expected = expect_of(step, field);
                        if actual != *expected {
                            failures.push(format!(
                                "{name} #{ordinal} {args:?} {field}：\n  期望 {expected}\n  實際 {actual}"
                            ));
                        }
                    }
                }
                op => panic!("未知的 step op：{op}"),
            }
        }
    }

    assert_eq!(
        compared, EXPECTED_COMPARED_FIELDS,
        "比對欄位數與預期不符，golden 可能被截短或擴充"
    );
    assert!(
        failures.is_empty(),
        "{} 處與 golden 不符：\n{}",
        failures.len(),
        failures.join("\n")
    );
}
