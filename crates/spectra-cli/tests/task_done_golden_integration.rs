//! `task done` 對 oracle 3.0.0 golden 的重播比對（#110）。
//!
//! `docs/reverse-engineering/golden/task-done-3.0.0.json` 由
//! `scripts/capture-task-done.py` 產生，自帶 base tree 與每個 setup 步驟；本測試
//! 依樣重播，逐步比對 exit、stdout、stderr、`tasks.md` bytes 與
//! `.spectra/touched/<change>.json` bytes。
//!
//! 已知分歧分三層處理，任何一層失準都會讓測試失敗：
//! 1. 系統性分歧以 transform 從 oracle 值推導 OpenSpectra 的期望值（JSON 只保留
//!    v2.3.1 的四個 key、人類輸出不帶 `✓ `、不輸出無 baseline 警告）；每個
//!    transform 至少要被用到一次，否則視為過期。
//! 2. 個別分歧逐筆記在 `task-done-3.0.0.divergences.json`；未列的分歧失敗，
//!    值已與 oracle 一致的條目與沒被用到的條目也失敗。
//! 3. 用到 `task start` 或 `--file` 的情境整段不重播（OpenSpectra 尚未實作），
//!    另以測試鎖住「這兩個介面仍被拒絕」，實作後必須回來改這裡。

mod common;

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{json, Value};

use common::{spectra, TempDir};

const NO_BASELINE_WARNING: &str = "! touched_tracking_skipped_no_baseline_or_explicit_files\n";
const V2_JSON_KEYS: [&str; 4] = ["change", "status", "task_desc", "task_id"];
const V3_ONLY_SCENARIOS: [&str; 4] = [
    "dirty-path-kinds",
    "task-start-baseline",
    "baseline-exclusions",
    "explicit-file-attribution",
];

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

fn with_git_env(cmd: &mut Command, golden: &Value) {
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

fn args_of(step: &Value) -> Vec<&str> {
    step["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect()
}

fn uses_v3_only_surface(step: &Value) -> bool {
    if step["op"] != "spectra" {
        return false;
    }
    let args = args_of(step);
    args.starts_with(&["task", "start"]) || args.contains(&"--file")
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

#[derive(Default)]
struct TransformUse(HashMap<&'static str, usize>);

impl TransformUse {
    /// Derive OpenSpectra's expected value from the oracle's for the
    /// systematic divergences (see module doc, layer 1).
    fn apply(&mut self, field: &str, oracle: &Value) -> Value {
        let Some(text) = oracle.as_str() else {
            return oracle.clone();
        };
        let mut hit = |name: &'static str| *self.0.entry(name).or_default() += 1;
        match field {
            "stdout" if text.starts_with('{') => {
                hit("json-shape");
                let full: Value = serde_json::from_str(text).unwrap();
                let subset: serde_json::Map<String, Value> = V2_JSON_KEYS
                    .iter()
                    .map(|k| (k.to_string(), full[*k].clone()))
                    .collect();
                Value::String(serde_json::to_string_pretty(&Value::Object(subset)).unwrap() + "\n")
            }
            "stdout" if text.starts_with("✓ ") => {
                hit("checkmark");
                Value::String(text["✓ ".len()..].to_string())
            }
            "stderr" if text.contains(NO_BASELINE_WARNING) => {
                hit("no-baseline-warning");
                Value::String(text.replace(NO_BASELINE_WARNING, ""))
            }
            _ => oracle.clone(),
        }
    }
}

#[test]
fn task_done_replays_the_oracle_golden_modulo_the_pinned_divergences() {
    let golden = load("task-done-3.0.0.json");
    let ledger = load("task-done-3.0.0.divergences.json");
    assert_eq!(str_of(&golden, "oracle_version"), "3.0.0");
    let root_token = str_of(&golden, "root_token");

    let mut divergences: HashMap<(String, u64, String), Value> = HashMap::new();
    for e in ledger["entries"].as_array().unwrap() {
        assert!(
            ledger["classes"].get(str_of(e, "class")).is_some(),
            "ledger 條目的 class 未定義：{e}"
        );
        let key = (
            str_of(e, "scenario").to_string(),
            e["step"].as_u64().unwrap(),
            str_of(e, "field").to_string(),
        );
        let dup = divergences.insert(key, e["openspectra"].clone());
        assert!(dup.is_none(), "ledger 條目重複：{e}");
    }

    let mut used = BTreeSet::new();
    let mut transforms = TransformUse::default();
    let mut failures = Vec::new();
    let mut compared = 0;

    for scenario in golden["scenarios"].as_array().unwrap() {
        let name = str_of(scenario, "name");
        let v3_only = scenario["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(uses_v3_only_surface);
        assert_eq!(
            v3_only,
            V3_ONLY_SCENARIOS.contains(&name),
            "情境 {name} 的 v3-only 分類與 V3_ONLY_SCENARIOS 不一致"
        );
        if v3_only {
            continue;
        }

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
                    ];
                    for (field, actual) in got {
                        compared += 1;
                        let derived = transforms.apply(field, &step["expect"][field]);
                        let key = (name.to_string(), ordinal, field.to_string());
                        let expected = match divergences.get(&key) {
                            Some(pinned) => {
                                used.insert(key.clone());
                                if *pinned == derived {
                                    failures.push(format!(
                                        "{name} #{ordinal} {args:?} {field}：ledger 條目已與 oracle 一致，請移除"
                                    ));
                                }
                                pinned
                            }
                            None => &derived,
                        };
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

    for key in divergences.keys() {
        if !used.contains(key) {
            failures.push(format!("ledger 條目沒有對應的 step：{key:?}"));
        }
    }
    for name in ["json-shape", "checkmark", "no-baseline-warning"] {
        if transforms.0.get(name).copied().unwrap_or(0) == 0 {
            failures.push(format!("transform {name} 從未套用，已過期"));
        }
    }
    assert!(
        compared >= 150,
        "只比對了 {compared} 個欄位，golden 可能被截短"
    );
    assert!(
        failures.is_empty(),
        "{} 處與 golden 不符：\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn v3_only_task_start_and_explicit_files_are_still_rejected() {
    // 這兩個介面實作後，V3_ONLY_SCENARIOS 的情境就該納入重播；此測試失敗即提醒。
    let golden = load("task-done-3.0.0.json");
    let scenario = golden["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "task-start-baseline")
        .unwrap();
    let repo = seed(scenario, &golden);
    for args in [
        vec!["task", "start", "1"],
        vec!["task", "done", "1", "--file", "src/a.rs"],
    ] {
        let out = run_spectra(&repo, &golden, &args);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?} 應被 clap 拒絕：{out:?}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(repo.join(str_of(&golden, "tasks_md"))).unwrap(),
        scenario["base"][str_of(&golden, "tasks_md")]
            .as_str()
            .unwrap(),
        "被拒絕的指令不可改動 tasks.md"
    );
}
