//! `spectra analyze` 對 oracle 3.0.0 golden 的完整重播比對（W10）。
//!
//! `docs/reverse-engineering/golden/analyze-3.0.0.json` 由
//! `scripts/capture-analyze.py` 產生，自帶每個專案的檔案；本測試依樣建出專案，
//! 逐次執行比對 exit code、stdout、stderr。
//!
//! golden 已套用兩個刻意分歧的正規化（見 capture 腳本與 analyze.md）：`params`
//! 的 key 依字母序，以及 fixture 目錄名稱挑成 readdir 順序等於名稱排序。其餘每個
//! 位元組都必須與 oracle 相同。`tty` 的執行在 pseudo-terminal 上跑（`script(1)`），
//! 驗證顏色；PTY 的 `\r\n` 折回 `\n` 後比對。
//!
//! 截短保護：情境名稱必須恰好是 `SCENARIOS`，實際比對的執行數必須等於
//! `EXPECTED_RUNS`。

mod common;

use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

use common::{spectra, TempDir};

const SCENARIOS: [&str; 18] = [
    "presence-layouts",
    "coverage-capabilities",
    "coverage-tasks",
    "coverage-order-and-gating",
    "delta-validation",
    "delta-validation-multi",
    "consistency-design-topics",
    "consistency-gating",
    "consistency-goals-overlap",
    "ambiguity",
    "gaps",
    "gaps-purpose-headings",
    "localization-gating-and-files",
    "localization-detection",
    "localization-locales",
    "human-output",
    "numeric-claims",
    "change-resolution",
];
const EXPECTED_RUNS: usize = 240;

fn load() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/reverse-engineering/golden/analyze-3.0.0.json");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("讀取 {path:?}：{e}"));
    serde_json::from_str(&raw).unwrap()
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} 不是字串：{v}"))
}

/// 取出 run 的 oracle 期望值；缺欄位即 panic，避免被截短的 golden 靜默變成 Null。
fn expect_of<'a>(run: &'a Value, field: &str) -> &'a Value {
    run["expect"]
        .as_object()
        .unwrap_or_else(|| panic!("run 沒有 expect 物件：{run}"))
        .get(field)
        .unwrap_or_else(|| panic!("run 的 expect 缺少 {field}：{run}"))
}

fn plain(root: &Path, args: &[&str]) -> Output {
    let mut cmd = spectra();
    for key in ["NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE"] {
        cmd.env_remove(key);
    }
    cmd.args(args).current_dir(root).output().unwrap()
}

/// stdout 接到 PTY（`script`）；stdin 保持開啟到子行程結束，避免 `script` 對 PTY 送 ^D。
fn on_terminal(root: &Path, args: &[&str]) -> Output {
    let mut command = Command::new("script");
    command.env("OPENSPECTRA_IMPL", "oss");
    for key in ["NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE"] {
        command.env_remove(key);
    }
    #[cfg(target_os = "macos")]
    {
        command.args(["-q", "/dev/null", env!("CARGO_BIN_EXE_spectra")]);
        command.args(args);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let script_command = format!("{} {}", env!("CARGO_BIN_EXE_spectra"), args.join(" "));
        command.args(["-q", "-e", "-c", &script_command, "/dev/null"]);
    }
    let mut child = command
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take();
    let output = child.wait_with_output().unwrap();
    drop(stdin);
    output
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn analyze_replays_the_oracle_golden_byte_for_byte() {
    let golden = load();
    assert_eq!(str_of(&golden, "oracle_version"), "3.0.0");
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
    let mut runs = 0;
    for scenario in golden["scenarios"].as_array().unwrap() {
        let name = str_of(scenario, "name");
        for project in scenario["projects"].as_array().unwrap() {
            let root = TempDir::new("analyze-golden");
            for (rel, content) in project["files"].as_object().unwrap() {
                write(&root, rel, content.as_str().unwrap());
            }
            for run in project["runs"].as_array().unwrap() {
                runs += 1;
                let args: Vec<&str> = run["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_str().unwrap())
                    .collect();
                let tty = run["tty"].as_bool().unwrap();
                let out = if tty {
                    on_terminal(&root, &args)
                } else {
                    plain(&root, &args)
                };
                let text = |bytes: &[u8]| {
                    let s = String::from_utf8_lossy(bytes).to_string();
                    if tty {
                        s.replace("\r\n", "\n")
                    } else {
                        s
                    }
                };
                let got = [
                    ("exit", Value::from(out.status.code())),
                    ("stdout", Value::from(text(&out.stdout))),
                    ("stderr", Value::from(text(&out.stderr))),
                ];
                for (field, actual) in got {
                    let expected = expect_of(run, field);
                    // `script` 會把子行程的 stderr 也接到 PTY（併進 stdout），所以 tty
                    // 執行只比對 exit 與 stdout；golden 的 tty 執行 stderr 全為空。
                    if tty && field == "stderr" {
                        assert_eq!(expected, "", "{name} {args:?}：tty 執行的 stderr 應為空");
                        continue;
                    }
                    if actual != *expected {
                        failures.push(format!(
                            "{name} (locale {}) {args:?} {field}：\n  期望 {expected}\n  實際 {actual}",
                            project["locale"]
                        ));
                    }
                }
            }
        }
    }

    assert_eq!(
        runs, EXPECTED_RUNS,
        "執行數與預期不符，golden 可能被截短或擴充"
    );
    assert!(
        failures.is_empty(),
        "{} 處與 golden 不符：\n{}",
        failures.len(),
        failures.join("\n")
    );
}
