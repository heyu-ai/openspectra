//! W11：內建實作切換（`OPENSPECTRA_IMPL`、`.spectra/impl`、shadow、`spectra impl`）。
//! 以一支假的 oracle shell script（`OPENSPECTRA_ORACLE_BIN`）驗證轉交、比對與紀錄。
#![cfg(unix)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{spectra, TempDir};

/// 假 oracle：把 argv 附加到 `calls.log`，stdout 印 `stdout_text`，exit `code`。
fn fake_oracle(dir: &Path, stdout_text: &str, code: i32) -> PathBuf {
    let path = dir.join("fake-oracle.sh");
    let log = dir.join("calls.log");
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nprintf '%s' '{}'\nexit {code}\n",
            log.display(),
            stdout_text
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn calls(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("calls.log")).unwrap_or_default()
}

/// 專案（`spectra init` 後沒有 change），外加獨立的 state／config 目錄。
fn project(label: &str) -> TempDir {
    let dir = TempDir::new(label);
    let init = spectra()
        .args(["init", "--dir", "openspec"])
        .current_dir(&*dir)
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    dir
}

fn run(dir: &Path, mode: Option<&str>, oracle: &Path, args: &[&str]) -> Output {
    let mut cmd: Command = spectra();
    cmd.args(args)
        .current_dir(dir)
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("OPENSPECTRA_ORACLE_BIN", oracle)
        .env_remove("OPENSPECTRA_IMPL")
        .env_remove("OPENSPECTRA_SHADOW_CHILD");
    if let Some(mode) = mode {
        cmd.env("OPENSPECTRA_IMPL", mode);
    }
    cmd.output().unwrap()
}

fn shadow_log(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("state/openspectra/shadow.jsonl")).unwrap_or_default()
}

#[test]
fn oracle_mode_hands_the_whole_call_to_the_oracle() {
    let dir = project("impl-oracle");
    let oracle = fake_oracle(&dir, "from-oracle", 3);
    // 連 OpenSpectra 不認得的子指令也原樣交出去。
    let out = run(
        &dir,
        Some("oracle"),
        &oracle,
        &["decisions", "--weird", "x y"],
    );
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "from-oracle");
    assert_eq!(calls(&dir), "decisions --weird x y\n");
}

#[test]
fn oracle_mode_without_an_oracle_fails_loudly() {
    let dir = project("impl-oracle-missing");
    let missing = dir.join("no-such-oracle");
    let out = run(&dir, Some("oracle"), &missing, &["list"]);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(
        err.starts_with("Error: the oracle binary was not found at "),
        "{err}"
    );
}

#[test]
fn shadow_mode_returns_the_oracle_result_and_logs_only_differences() {
    let dir = project("impl-shadow");
    // 與 OpenSpectra 相同（空專案的 list --json）→ 不寫 log。
    let same = fake_oracle(&dir, "{\"changes\": []}", 0);
    let out = run(&dir, Some("shadow"), &same, &["list", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "{\"changes\": []}");
    assert_eq!(shadow_log(&dir), "");

    // 不同 → 呼叫端仍拿到 oracle 的結果（含 exit code），並寫一筆差異。
    let different = fake_oracle(&dir, "{\"changes\": [1]}", 3);
    let out = run(&dir, Some("shadow"), &different, &["list", "--json"]);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "{\"changes\": [1]}");
    let log = shadow_log(&dir);
    assert_eq!(log.lines().count(), 1, "{log}");
    let record: serde_json::Value = serde_json::from_str(log.trim()).unwrap();
    assert_eq!(record["argv"], serde_json::json!(["list", "--json"]));
    assert_eq!(
        record["diffs"],
        serde_json::json!(["exit 3->0", "stdout .changes[] len 1->0"])
    );
}

#[test]
fn shadow_mode_never_runs_openspectra_for_writing_commands() {
    let dir = project("impl-shadow-write");
    let oracle = fake_oracle(&dir, "", 0);
    let out = run(&dir, Some("shadow"), &oracle, &["new", "change", "demo"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(calls(&dir), "new change demo\n");
    assert!(
        !dir.join("openspec/changes/demo").exists(),
        "OpenSpectra must not also run a writing command"
    );
    assert_eq!(shadow_log(&dir), "");
}

#[test]
fn project_impl_file_selects_the_mode_and_impl_reports_it() {
    let dir = project("impl-project-file");
    let oracle = fake_oracle(&dir, "oracle-list", 0);
    std::fs::create_dir_all(dir.join(".spectra")).unwrap();
    std::fs::write(dir.join(".spectra/impl"), "oracle\n").unwrap();
    let out = run(&dir, None, &oracle, &["list"]);
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "oracle-list");

    // `spectra impl` 一律由 OpenSpectra 回答。
    let out = run(&dir, None, &oracle, &["impl", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["mode"], "oracle");
    assert_eq!(
        v["source"],
        dir.join(".spectra/impl").to_string_lossy().as_ref()
    );
    assert_eq!(v["oracle_found"], true);

    // 環境變數優先於專案檔。
    let out = run(&dir, Some("oss"), &oracle, &["impl", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["mode"], "oss");
    assert_eq!(v["source"], "OPENSPECTRA_IMPL");
}

#[test]
fn invalid_mode_is_an_error() {
    let dir = project("impl-invalid");
    let oracle = fake_oracle(&dir, "", 0);
    let out = run(&dir, Some("fast"), &oracle, &["list"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: invalid implementation mode 'fast' in OPENSPECTRA_IMPL (expected oracle, shadow, or oss)\n"
    );
    assert_eq!(calls(&dir), "");
}

#[test]
fn oss_mode_records_failures_in_errors_jsonl() {
    let dir = project("impl-oss-errors");
    let oracle = fake_oracle(&dir, "", 0);
    let out = run(&dir, None, &oracle, &["show", "nothing-here"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(calls(&dir), "", "oss mode never calls the oracle");
    let log = std::fs::read_to_string(dir.join("state/openspectra/errors.jsonl")).unwrap();
    let record: serde_json::Value = serde_json::from_str(log.trim()).unwrap();
    assert_eq!(record["argv"], serde_json::json!(["show", "nothing-here"]));
    assert_eq!(record["exit"], 1);
    assert_eq!(record["panic"], false);
    assert!(
        record["summary"].as_str().unwrap().contains("nothing-here"),
        "{record}"
    );
}
