//! D13：MODIFIED requirement 的 scenario 身分（slug）與 `removed-scenario` 標記。
//!
//! 每個案例都用同一份 fixture 跑 `validate --strict --format openspec` 與 `archive`，
//! 確認兩條路徑的判定一致：validate 判有效的，archive 也要套用；validate 判無效的，
//! archive 也要拒絕。

mod common;

use common::{init_project_with_change, spectra, TempDir};
use std::process::Stdio;

const MAIN: &str = "# auth Specification\n\n## Purpose\n\nAuthentication of family accounts across every client.\n\n\
## Requirements\n\n### Requirement: Login\nThe system SHALL authenticate users.\n\n\
#### Scenario: password-login -- 密碼登入成功\n- **WHEN** credentials are valid\n- **THEN** access is granted\n\n\
#### Scenario: locked-account -- 帳號鎖定時拒絕\n- **WHEN** an account is locked\n- **THEN** access is denied\n";

fn setup(label: &str, main: &str, delta: &str) -> TempDir {
    let tmp = TempDir::new(label);
    init_project_with_change(&tmp, "feat");
    let change = tmp.join("openspec/changes/feat");
    std::fs::write(
        change.join("proposal.md"),
        "# Proposal\n\n## Why\nBecause.\n\n## What Changes\n- auth\n",
    )
    .unwrap();
    std::fs::write(change.join("tasks.md"), "- [x] 1.1 done\n").unwrap();
    let main_path = tmp.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(main_path.parent().unwrap()).unwrap();
    std::fs::write(main_path, main).unwrap();
    std::fs::create_dir_all(change.join("specs/auth")).unwrap();
    std::fs::write(change.join("specs/auth/spec.md"), delta).unwrap();
    tmp
}

fn validate(tmp: &TempDir) -> serde_json::Value {
    let out = spectra()
        .args([
            "validate", "feat", "--strict", "--json", "--format", "openspec",
        ])
        .current_dir(&**tmp)
        .output()
        .unwrap();
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {out:?}"))
}

fn is_valid(report: &serde_json::Value) -> bool {
    report["items"][0]["valid"].as_bool().unwrap()
}

fn messages(report: &serde_json::Value) -> Vec<(String, String)> {
    report["items"][0]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["level"].as_str().unwrap().to_string(),
                i["message"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn archive(tmp: &TempDir) -> std::process::Output {
    spectra()
        .args(["archive", "feat", "--yes", "--no-color"])
        .current_dir(&**tmp)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn main_spec(tmp: &TempDir) -> String {
    std::fs::read_to_string(tmp.join("openspec/specs/auth/spec.md")).unwrap()
}

fn delta(scenarios: &str) -> String {
    format!(
        "## MODIFIED Requirements\n\n### Requirement: Login\nThe system SHALL authenticate users.\n\n{scenarios}"
    )
}

const PASSWORD: &str =
    "#### Scenario: password-login -- 密碼登入成功\n- **WHEN** credentials are valid\n- **THEN** access is granted\n\n";

#[test]
fn retitling_a_scenario_with_the_same_slug_is_valid_and_archives() {
    let tmp = setup(
        "slug-retitle",
        MAIN,
        &delta(&format!(
            "{PASSWORD}#### Scenario: locked-account -- 帳號鎖定或停用時拒絕\n- **WHEN** an account is locked or disabled\n- **THEN** access is denied\n"
        )),
    );
    let report = validate(&tmp);
    assert!(is_valid(&report), "{:#?}", messages(&report));

    let run = archive(&tmp);
    assert!(run.status.success(), "{run:?}");
    let main = main_spec(&tmp);
    assert!(
        main.contains("locked-account -- 帳號鎖定或停用時拒絕"),
        "{main}"
    );
    assert!(!main.contains("locked-account -- 帳號鎖定時拒絕"), "{main}");
}

#[test]
fn dropping_a_scenario_without_a_marker_is_invalid_and_archive_refuses() {
    let tmp = setup("slug-drop", MAIN, &delta(PASSWORD));
    let report = validate(&tmp);
    assert!(!is_valid(&report));
    assert!(
        messages(&report).iter().any(|(level, m)| level == "ERROR"
            && m.starts_with(
                "MODIFIED \"Login\" omits scenario(s) the current spec still has: \"locked-account -- 帳號鎖定時拒絕\"."
            )),
        "{:#?}",
        messages(&report)
    );

    let run = archive(&tmp);
    assert!(!run.status.success(), "{run:?}");
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("omits scenario(s)"),
        "{run:?}"
    );
    assert!(main_spec(&tmp).contains("locked-account -- 帳號鎖定時拒絕"));
}

#[test]
fn a_marker_drops_the_named_scenario_and_is_not_written_to_the_main_spec() {
    let tmp = setup(
        "slug-marker",
        MAIN,
        &delta(&format!(
            "<!-- removed-scenario: locked-account -->\n\n{PASSWORD}"
        )),
    );
    let report = validate(&tmp);
    assert!(is_valid(&report), "{:#?}", messages(&report));

    let run = archive(&tmp);
    assert!(run.status.success(), "{run:?}");
    let main = main_spec(&tmp);
    assert!(!main.contains("locked-account"), "{main}");
    assert!(!main.contains("removed-scenario"), "{main}");
    assert!(main.contains("password-login -- 密碼登入成功"), "{main}");
}

#[test]
fn a_duplicate_slug_does_not_hide_a_real_drop() {
    let main = MAIN.replace(
        "#### Scenario: locked-account -- 帳號鎖定時拒絕",
        "#### Scenario: password-login -- 密碼錯誤時拒絕",
    );
    let tmp = setup(
        "slug-duplicate",
        &main,
        &delta(&format!(
            "{PASSWORD}#### Scenario: password-login -- 改寫後的新 scenario\n- **WHEN** x\n- **THEN** y\n"
        )),
    );
    let report = validate(&tmp);
    assert!(!is_valid(&report), "{:#?}", messages(&report));

    let run = archive(&tmp);
    assert!(!run.status.success(), "{run:?}");
}

const UNSLUGGED_MAIN: &str = "# auth Specification\n\n## Purpose\n\nAuthentication of family accounts across every client.\n\n\
## Requirements\n\n### Requirement: Login\nThe system SHALL authenticate users.\n\n\
#### Scenario: Locked account is denied\n- **WHEN** an account is locked\n- **THEN** access is denied\n";

#[test]
fn adding_a_slug_with_the_same_title_is_valid_and_archives() {
    let tmp = setup(
        "slug-added",
        UNSLUGGED_MAIN,
        &delta("#### Scenario: locked-account -- Locked account is denied\n- **WHEN** an account is locked\n- **THEN** access is denied\n"),
    );
    let report = validate(&tmp);
    assert!(is_valid(&report), "{:#?}", messages(&report));

    let run = archive(&tmp);
    assert!(run.status.success(), "{run:?}");
    assert!(main_spec(&tmp).contains("locked-account -- Locked account is denied"));
}

#[test]
fn adding_a_slug_with_a_different_title_is_invalid_and_archive_refuses() {
    let tmp = setup(
        "slug-added-retitled",
        UNSLUGGED_MAIN,
        &delta("#### Scenario: locked-account -- Locked or disabled account is denied\n- **WHEN** an account is locked\n- **THEN** access is denied\n"),
    );
    let report = validate(&tmp);
    assert!(!is_valid(&report), "{:#?}", messages(&report));

    let run = archive(&tmp);
    assert!(!run.status.success(), "{run:?}");
    assert!(main_spec(&tmp).contains("#### Scenario: Locked account is denied"));
}

#[test]
fn an_unused_marker_is_info_only_and_strict_stays_valid() {
    let tmp = setup(
        "slug-unused-marker",
        MAIN,
        &delta(&format!(
            "<!-- removed-scenario: never-existed -->\n\n{PASSWORD}#### Scenario: locked-account -- 帳號鎖定時拒絕\n- **WHEN** an account is locked\n- **THEN** access is denied\n"
        )),
    );
    let report = validate(&tmp);
    assert!(is_valid(&report), "{:#?}", messages(&report));
    assert!(
        messages(&report).iter().any(|(level, m)| level == "INFO"
            && m.starts_with(
                "MODIFIED \"Login\" has removed-scenario marker(s) that match no scenario dropped from the current spec: \"never-existed\"."
            )),
        "{:#?}",
        messages(&report)
    );
}
