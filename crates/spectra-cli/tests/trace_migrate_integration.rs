//! `spectra trace migrate`（OpenSpectra-only，#98）在 CLI 層的契約：
//! dry-run 不寫檔、實際遷移後冪等、壞掉的 sidecar 以狀態碼 1 回報。

mod common;

use std::path::Path;

use common::{git, spectra, TempDir};

/// oracle 3.0.0 實際 archive 出來的形狀（2026-09-26 實測）。
const ORACLE_SPEC: &str = "# cap Specification\n\n## Purpose\n\nP.\n\n## Requirements\n\n\
### Requirement: Alpha\n\nThe system SHALL alpha.\n\n#### Scenario: a\n\n- **WHEN** x\n- **THEN** y\n\n\n\
<!-- @trace\nsource: demo\nupdated: 2026-09-26\ncode:\n  - a.rs\n-->\n\n---\n\
### Requirement: Beta\n\nThe system SHALL beta.\n\n#### Scenario: b\n\n- **WHEN** x\n- **THEN** y\n\n\
<!-- @trace\nsource: demo\nupdated: 2026-09-26\ncode:\n  - a.rs\n-->";

fn project() -> (TempDir, std::path::PathBuf) {
    let root = TempDir::new("trace-migrate");
    git(&root, &["init", "-q"]);
    let output = spectra().arg("init").current_dir(&*root).output().unwrap();
    assert!(output.status.success(), "init 失敗：{output:?}");
    let spec = root.join("openspec/specs/cap/spec.md");
    std::fs::create_dir_all(spec.parent().unwrap()).unwrap();
    std::fs::write(&spec, ORACLE_SPEC).unwrap();
    (root, spec)
}

fn migrate(root: &Path, args: &[&str]) -> std::process::Output {
    spectra()
        .args(["trace", "migrate"])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
}

#[test]
fn trace_migrate_dry_run_reports_without_writing() {
    let (root, spec) = project();

    let output = migrate(&root, &["--dry-run"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "cap: would move 2 inline trace footer(s) into spec.trace.yaml\n"
    );
    assert_eq!(std::fs::read_to_string(&spec).unwrap(), ORACLE_SPEC);
    assert!(!spec.with_file_name("spec.trace.yaml").exists());
}

#[test]
fn trace_migrate_moves_footers_and_a_rerun_finds_nothing() {
    let (root, spec) = project();

    let first = migrate(&root, &[]);
    assert_eq!(first.status.code(), Some(0), "{first:?}");
    assert_eq!(
        String::from_utf8_lossy(&first.stdout),
        "cap: moved 2 inline trace footer(s) into spec.trace.yaml\n"
    );
    let migrated = std::fs::read_to_string(&spec).unwrap();
    assert!(!migrated.contains("<!-- @trace\n"), "{migrated}");
    assert!(migrated.contains("<!-- @trace-sidecar: spec.trace.yaml -->"));
    let sidecar = std::fs::read_to_string(spec.with_file_name("spec.trace.yaml")).unwrap();
    assert!(sidecar.contains("source: demo"), "{sidecar}");

    let second = migrate(&root, &["--json"]);
    assert_eq!(second.status.code(), Some(0), "{second:?}");
    let json: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(json["dry_run"], false);
    assert_eq!(json["specs"], serde_json::json!([]));
}

#[test]
fn trace_migrate_check_fails_on_inline_footers_without_writing_and_passes_once_clean() {
    let (root, spec) = project();

    let dirty = migrate(&root, &["--check"]);
    assert_eq!(dirty.status.code(), Some(1), "{dirty:?}");
    assert_eq!(
        String::from_utf8_lossy(&dirty.stdout),
        "cap: has 2 inline trace footer(s) not yet in spec.trace.yaml\n"
    );
    assert_eq!(std::fs::read_to_string(&spec).unwrap(), ORACLE_SPEC);
    assert!(!spec.with_file_name("spec.trace.yaml").exists());

    assert_eq!(migrate(&root, &[]).status.code(), Some(0));

    let clean = migrate(&root, &["--check"]);
    assert_eq!(clean.status.code(), Some(0), "{clean:?}");
}

#[test]
fn trace_migrate_check_fails_on_a_stale_name_left_by_an_outside_rename() {
    // oracle 做 RENAMED 只改 spec.md 標題、不動 sidecar：舊名稱要被報出來。
    let (root, spec) = project();
    assert_eq!(migrate(&root, &[]).status.code(), Some(0));
    let renamed = std::fs::read_to_string(&spec)
        .unwrap()
        .replace("### Requirement: Alpha", "### Requirement: Alpha Renamed");
    std::fs::write(&spec, renamed).unwrap();

    let output = migrate(&root, &["--check"]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning: cap: spec.trace.yaml names requirement(s) not in spec.md: Alpha"),
        "{stderr}"
    );
    // 一般模式只警告，不當成失敗。
    assert_eq!(migrate(&root, &[]).status.code(), Some(0));
}

#[test]
fn trace_migrate_check_fails_on_an_unrecognized_footer_and_on_a_corrupt_sidecar_alone() {
    // #175 review：只有認不得的 footer、或沒有 footer 但 sidecar 壞掉，`--check` 都要失敗。
    let (root, spec) = project();
    std::fs::write(
        &spec,
        "# cap Specification\n\n## Requirements\n\n### Requirement: A\n\ntext\n\n<!-- @trace\nsource: x\nupdated: y\nowner: someone\n-->\n",
    )
    .unwrap();

    let unparsed = migrate(&root, &["--check"]);
    assert_eq!(unparsed.status.code(), Some(1), "{unparsed:?}");
    let plain = migrate(&root, &[]);
    assert!(
        String::from_utf8_lossy(&plain.stderr)
            .contains("left 1 unrecognized `<!-- @trace` footer(s) in place (line 9)"),
        "{plain:?}"
    );

    std::fs::write(
        &spec,
        "# cap Specification\n\n## Requirements\n\n### Requirement: A\n\ntext\n",
    )
    .unwrap();
    std::fs::write(spec.with_file_name("spec.trace.yaml"), "traces: [").unwrap();
    let corrupt = migrate(&root, &["--check"]);
    assert_eq!(corrupt.status.code(), Some(1), "{corrupt:?}");
}

#[test]
fn trace_migrate_check_json_reports_the_flags_the_user_passed() {
    let (root, _spec) = project();
    let output = migrate(&root, &["--check", "--json"]);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["check"], true);
    assert_eq!(json["dry_run"], false);
}

#[test]
fn trace_migrate_exits_1_on_a_corrupt_sidecar_and_leaves_the_spec_alone() {
    let (root, spec) = project();
    std::fs::write(spec.with_file_name("spec.trace.yaml"), "traces: [").unwrap();

    let output = migrate(&root, &[]);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error: cap:") && stderr.contains("is not a valid trace sidecar"),
        "{stderr}"
    );
    assert_eq!(std::fs::read_to_string(&spec).unwrap(), ORACLE_SPEC);
}
