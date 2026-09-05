//! CLI-level integration tests that spawn the built `spectra` binary, for
//! contracts that depend on full dispatch through `run()` (not just clap
//! parsing) -- unit tests in `main.rs` cover the parser; these cover the
//! actual runtime behavior.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn spectra() -> Command {
    Command::new(env!("CARGO_BIN_EXE_spectra"))
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs")
        .status
        .success();
    assert!(ok, "git {args:?} failed");
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "spectra-cli-it-{label}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn list_changes_flag_output_is_byte_identical_to_the_default() {
    let tmp = TempDir::new("list-changes");
    git(&tmp, &["init", "-q"]);
    git(&tmp, &["config", "user.email", "t@t.co"]);
    git(&tmp, &["config", "user.name", "t"]);

    let init = spectra().arg("init").current_dir(&*tmp).output().unwrap();
    assert!(init.status.success(), "init failed: {init:?}");
    let new_change = spectra()
        .args(["new", "change", "add-search-filter"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(
        new_change.status.success(),
        "new change failed: {new_change:?}"
    );

    let default_human = spectra().arg("list").current_dir(&*tmp).output().unwrap();
    let changes_human = spectra()
        .args(["list", "--changes"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(
        default_human.status.success(),
        "list failed: {default_human:?}"
    );
    assert!(
        changes_human.status.success(),
        "list --changes failed: {changes_human:?}"
    );
    assert_eq!(default_human.stdout, changes_human.stdout);
    assert_eq!(
        String::from_utf8(default_human.stdout).unwrap(),
        "Changes:\n  • add-search-filter\n"
    );

    let default_json = spectra()
        .args(["list", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    let changes_json = spectra()
        .args(["list", "--changes", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(
        default_json.status.success(),
        "list --json failed: {default_json:?}"
    );
    assert!(
        changes_json.status.success(),
        "list --changes --json failed: {changes_json:?}"
    );
    assert_eq!(default_json.stdout, changes_json.stdout);
    assert!(!default_json.stdout.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&default_json.stdout).unwrap();
    let item = &value["changes"][0];
    assert_eq!(item["name"], "add-search-filter");
    assert_eq!(item["status"], "in-progress");
    assert_eq!(item["completedTasks"], 0);
    assert_eq!(item["totalTasks"], 0);
    assert!(item.get("summary").is_none());
}

#[test]
fn list_sorts_changes_by_name_modified_and_created() {
    let tmp = TempDir::new("list-sort");
    init_project_with_change(&tmp, "middle");
    std::thread::sleep(std::time::Duration::from_millis(20));
    for name in ["z-last", "a-first"] {
        let out = spectra()
            .args(["new", "change", name])
            .current_dir(&*tmp)
            .output()
            .unwrap();
        assert!(out.status.success(), "new change failed: {out:?}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let listed_names = |sort: &str| {
        let out = spectra()
            .args(["list", "--sort", sort, "--json"])
            .current_dir(&*tmp)
            .output()
            .unwrap();
        assert!(out.status.success(), "list --sort {sort} failed: {out:?}");
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        value["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };

    assert_eq!(listed_names("name"), ["a-first", "middle", "z-last"]);
    assert_eq!(listed_names("modified"), ["a-first", "z-last", "middle"]);
    assert_eq!(listed_names("created"), ["a-first", "z-last", "middle"]);
}

#[test]
fn list_parked_human_output_uses_the_oracle_header_and_bullets() {
    let tmp = TempDir::new("list-parked-human");
    init_project_with_change(&tmp, "on-hold");
    let park = spectra()
        .args(["park", "on-hold"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(park.status.success(), "park failed: {park:?}");

    let out = spectra()
        .args(["list", "--parked"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert!(out.status.success(), "list --parked failed: {out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "Parked:\n  • on-hold\n"
    );
}

#[test]
fn init_text_output_matches_the_oracle() {
    let tmp = TempDir::new("init-text");
    git(&tmp, &["init", "-q"]);

    let out = spectra().arg("init").current_dir(&*tmp).output().unwrap();
    assert!(out.status.success(), "init failed: {out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();

    // Canonicalize before comparing: on macOS `std::env::temp_dir()` returns
    // `/var/...`, a symlink to `/private/var/...`, and the binary reports the
    // canonical form -- comparing the raw temp path would fail spuriously.
    let canonical_root = tmp.canonicalize().unwrap();
    assert_eq!(
        stdout,
        format!(
            "✓ Initialized at {}\n",
            canonical_root.join("openspec").display()
        )
    );
    assert!(out.stderr.is_empty());
}

#[test]
fn init_json_output_matches_the_documented_shape() {
    let tmp = TempDir::new("init-json");
    git(&tmp, &["init", "-q"]);

    let out = spectra()
        .args(["init", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(out.status.success(), "init --json failed: {out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["spec_dir"], "openspec");
    assert_eq!(value["adopted"], false);
    assert_eq!(value["gitignore_updated"], true);
    // Compare canonicalized paths: on macOS `std::env::temp_dir()` returns a
    // `/var/...` path that's actually a symlink to `/private/var/...`, and
    // the CLI reports whatever `std::env::current_dir()` resolves to after
    // `cd`-ing in, which follows the symlink.
    let reported_root = PathBuf::from(value["root"].as_str().unwrap());
    assert_eq!(
        reported_root.canonicalize().unwrap(),
        tmp.canonicalize().unwrap()
    );
}

#[test]
fn init_creates_and_uses_a_missing_explicit_path() {
    let tmp = TempDir::new("init-missing-path");
    let target = tmp.join("new/project");

    let out = spectra()
        .arg("init")
        .arg(&target)
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert!(out.status.success(), "init PATH failed: {out:?}");
    assert!(target.join(".spectra.yaml").is_file());
    assert!(target.join("openspec/changes/archive").is_dir());
}

#[test]
fn init_uses_an_existing_explicit_path_directly() {
    let tmp = TempDir::new("init-existing-path");
    let target = tmp.join("existing");
    std::fs::create_dir_all(&target).unwrap();

    let out = spectra()
        .arg("init")
        .arg(&target)
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert!(out.status.success(), "init PATH failed: {out:?}");
    assert!(target.join(".spectra.yaml").is_file());
    assert!(!tmp.join(".spectra.yaml").exists());
}

#[test]
fn init_force_reinitializes_an_existing_project() {
    let tmp = TempDir::new("init-force");
    let first = spectra().arg("init").current_dir(&*tmp).output().unwrap();
    assert!(first.status.success(), "initial init failed: {first:?}");

    let out = spectra()
        .args(["init", "--force"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert!(out.status.success(), "init --force failed: {out:?}");
    assert!(tmp.join("openspec/config.yaml").is_file());
}

#[test]
fn init_dir_uses_the_custom_spec_directory() {
    let tmp = TempDir::new("init-dir");

    let out = spectra()
        .args(["init", "--dir", "custom-dir"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert!(out.status.success(), "init --dir failed: {out:?}");
    assert!(tmp.join("custom-dir/changes/archive").is_dir());
    assert!(tmp.join("custom-dir/specs").is_dir());
    let config = std::fs::read_to_string(tmp.join(".spectra.yaml")).unwrap();
    assert_eq!(config.lines().nth(5), Some("spec_dir: custom-dir"));
}

#[test]
fn init_without_force_reports_the_oracle_reinitialize_message() {
    let tmp = TempDir::new("init-already");
    let first = spectra().arg("init").current_dir(&*tmp).output().unwrap();
    assert!(first.status.success(), "initial init failed: {first:?}");

    let out = spectra().arg("init").current_dir(&*tmp).output().unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: Already initialized. Use --force to reinitialize.\n"
    );
}

#[test]
fn drift_exits_zero_even_when_severity_is_medium_or_higher() {
    // Regression for issue #37: a successful drift analysis must always exit 0
    // regardless of severity (matching the reference binary and the README's
    // documented contract). v0.1.0 mapped severity to the exit code (light->0,
    // medium->1, heavy->2), which reddened downstream CI on the `spectra`
    // process itself before the caller could gate on the JSON `severity` field.
    let tmp = TempDir::new("drift-exit-zero");
    git(&tmp, &["init", "-q"]);
    git(&tmp, &["config", "user.email", "t@t.co"]);
    git(&tmp, &["config", "user.name", "t"]);

    let init = spectra().arg("init").current_dir(&*tmp).output().unwrap();
    assert!(init.status.success(), "init failed: {init:?}");
    let new_change = spectra()
        .args(["new", "change", "aged-out"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(
        new_change.status.success(),
        "new change failed: {new_change:?}"
    );

    // A `created` date far in the past lands the Time dimension in the
    // "abandoned" bucket (score 4), which alone reaches `medium` severity.
    std::fs::write(
        tmp.join("openspec")
            .join("changes")
            .join("aged-out")
            .join(".openspec.yaml"),
        "schema: spec-driven\ncreated: 2020-01-01\n",
    )
    .unwrap();

    let out = spectra()
        .args(["drift", "aged-out", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        report["severity"], "medium",
        "test setup must actually produce a medium severity, got:\n{stdout}"
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "drift must exit 0 on a medium-severity change, got {:?}; stderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Init a git repo + spectra project in `tmp` and create change `name`.
fn init_project_with_change(tmp: &Path, name: &str) {
    git(tmp, &["init", "-q"]);
    git(tmp, &["config", "user.email", "t@t.co"]);
    git(tmp, &["config", "user.name", "t"]);
    let init = spectra().arg("init").current_dir(tmp).output().unwrap();
    assert!(init.status.success(), "init failed: {init:?}");
    let nc = spectra()
        .args(["new", "change", name])
        .current_dir(tmp)
        .output()
        .unwrap();
    assert!(nc.status.success(), "new change failed: {nc:?}");
}

#[test]
fn archive_yes_skips_confirmation_and_archives() {
    let tmp = TempDir::new("archive-yes");
    init_project_with_change(&tmp, "ready");

    let out = spectra()
        .args(["archive", "ready", "-y", "--skip-specs"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert!(out.status.success(), "archive -y failed: {out:?}");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("Archive 'ready'?"));
    assert!(!tmp.join("openspec/changes/ready").exists());
}

#[test]
fn archive_no_validate_still_keeps_change_active_when_merge_fails() {
    let tmp = TempDir::new("archive-no-validate");
    init_project_with_change(&tmp, "unsafe-change");
    let delta = tmp.join("openspec/changes/unsafe-change/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(
        delta,
        "## MODIFIED Requirements\n\n### Requirement: Missing\n\nnew text\n",
    )
    .unwrap();

    let out = spectra()
        .args(["archive", "unsafe-change", "-y", "--no-validate"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(tmp.join("openspec/changes/unsafe-change").is_dir());
    assert!(!tmp
        .join("openspec/changes/archive")
        .read_dir()
        .unwrap()
        .any(|entry| {
            entry.is_ok_and(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .ends_with("-unsafe-change")
            })
        }));
}

#[test]
fn archive_with_piped_stdin_skips_confirmation() {
    let tmp = TempDir::new("archive-piped");
    init_project_with_change(&tmp, "piped-change");

    let mut child = spectra()
        .args(["archive", "piped-change", "--skip-specs"])
        .current_dir(&*tmp)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), b"n\n").unwrap();
    let out = child.wait_with_output().unwrap();

    assert!(out.status.success(), "piped archive failed: {out:?}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("Aborted."));
    assert!(!tmp.join("openspec/changes/piped-change").exists());
}

#[cfg(unix)]
#[test]
fn archive_prompts_and_aborts_on_a_terminal() {
    let tmp = TempDir::new("archive-prompt");
    init_project_with_change(&tmp, "prompt-change");

    let mut command = Command::new("script");
    #[cfg(target_os = "macos")]
    command.args([
        "-q",
        "/dev/null",
        env!("CARGO_BIN_EXE_spectra"),
        "archive",
        "prompt-change",
        "--skip-specs",
    ]);
    #[cfg(not(target_os = "macos"))]
    let script_command = format!(
        "{} archive prompt-change --skip-specs",
        env!("CARGO_BIN_EXE_spectra")
    );
    #[cfg(not(target_os = "macos"))]
    command.args(["-q", "-c", &script_command, "/dev/null"]);

    let mut child = command
        .current_dir(&*tmp)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), b"n\n").unwrap();
    drop(child.stdin.take());
    let out = child.wait_with_output().unwrap();
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(out.status.success(), "終端機提示測試失敗：{out:?}");
    assert!(output.contains("Archive 'prompt-change'? (y/N) "));
    assert!(output.contains("Aborted."));
    assert!(tmp.join("openspec/changes/prompt-change").is_dir());
}

#[test]
fn validate_accepts_a_well_formed_nested_capability_delta() {
    // The nested `specs/<Epic>/<Feature>/spec.md` layout OSS reports as "no
    // deltas found"; validate must traverse it and pass a good delta -- exit 0.
    let tmp = TempDir::new("validate-nested-ok");
    init_project_with_change(&tmp, "billing");
    let cap = tmp
        .join("openspec")
        .join("changes")
        .join("billing")
        .join("specs")
        .join("Billing")
        .join("Invoices");
    std::fs::create_dir_all(&cap).unwrap();
    std::fs::write(
        cap.join("spec.md"),
        "## ADDED Requirements\n\n\
         ### Requirement: Invoice export\n\n\
         The system SHALL export invoices as PDF.\n\n\
         #### Scenario: Export succeeds\n\n\
         - **WHEN** a user requests a PDF\n\
         - **THEN** a PDF is produced\n",
    )
    .unwrap();

    let out = spectra()
        .args(["validate", "--changes", "--strict", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["summary"]["totals"]["failed"], 0, "got:\n{stdout}");
    assert_eq!(report["items"][0]["id"], "billing");
    assert_eq!(report["items"][0]["valid"], true);
    assert_eq!(
        out.status.code(),
        Some(0),
        "a valid change must exit 0; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn validate_strict_fails_and_exits_nonzero_on_a_bad_delta() {
    let tmp = TempDir::new("validate-bad");
    init_project_with_change(&tmp, "feat");
    let cap = tmp
        .join("openspec")
        .join("changes")
        .join("feat")
        .join("specs")
        .join("auth");
    std::fs::create_dir_all(&cap).unwrap();
    // A requirement with neither a normative keyword nor a scenario.
    std::fs::write(
        cap.join("spec.md"),
        "## ADDED Requirements\n\n### Requirement: Login\n\nUsers can log in.\n",
    )
    .unwrap();

    let out = spectra()
        .args(["validate", "feat", "--strict", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(report["summary"]["totals"]["failed"], 1, "got:\n{stdout}");
    assert_eq!(report["items"][0]["valid"], false);
    assert!(report["items"][0]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["level"] == "ERROR"));
    assert_eq!(
        out.status.code(),
        Some(1),
        "an invalid change must exit 1 (gate semantics); stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn validate_nonstrict_reports_keyword_guidance_without_failing() {
    let tmp = TempDir::new("validate-nonstrict");
    init_project_with_change(&tmp, "feat");
    let cap = tmp
        .join("openspec")
        .join("changes")
        .join("feat")
        .join("specs")
        .join("auth");
    std::fs::create_dir_all(&cap).unwrap();
    std::fs::write(
        cap.join("spec.md"),
        "## ADDED Requirements\n\n### Requirement: Login\n\nUsers can log in.\n\n\
         #### Scenario: Login\n- **WHEN** credentials are valid\n- **THEN** access is granted\n",
    )
    .unwrap();

    let out = spectra()
        .args(["validate", "feat", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let report: serde_json::Value =
        serde_json::from_str(&String::from_utf8(out.stdout).unwrap()).unwrap();
    assert_eq!(report["items"][0]["valid"], true);
    assert_eq!(report["items"][0]["issues"][0]["level"], "WARNING");
}

#[test]
fn validate_rejects_a_modified_requirement_that_drops_a_current_scenario() {
    let tmp = TempDir::new("validate-scenario-loss");
    init_project_with_change(&tmp, "feat");
    let main = tmp.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(main.parent().unwrap()).unwrap();
    std::fs::write(
        &main,
        "# auth Specification\n\n## Purpose\n\nAuthentication.\n\n## Requirements\n\n\
         ### Requirement: Login\nThe system SHALL authenticate users.\n\n\
         #### Scenario: Password login\n- **WHEN** credentials are valid\n- **THEN** access is granted\n\n\
         #### Scenario: Locked account\n- **WHEN** an account is locked\n- **THEN** access is denied\n",
    )
    .unwrap();
    let delta = tmp.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(
        delta,
        "## MODIFIED Requirements\n\n\
         ### Requirement: Login\nThe system SHALL authenticate users.\n\n\
         #### Scenario: Password login\n- **WHEN** credentials are valid\n- **THEN** access is granted\n",
    )
    .unwrap();

    let out = spectra()
        .args(["validate", "feat", "--strict", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(report["items"][0]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["message"]
            .as_str()
            .is_some_and(|message| message.contains("Locked account"))));
    let archived = spectra()
        .args(["archive", "feat", "--yes"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert_eq!(archived.status.code(), Some(1));
    assert!(tmp.join("openspec/changes/feat").is_dir());
    assert!(std::fs::read_to_string(main)
        .unwrap()
        .contains("#### Scenario: Locked account"));
}

#[test]
fn archive_ignores_requirement_headers_inside_fenced_examples() {
    let tmp = TempDir::new("archive-fenced-requirement");
    init_project_with_change(&tmp, "feat");
    let main = tmp.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(main.parent().unwrap()).unwrap();
    std::fs::write(
        &main,
        "# auth Specification\n\n## Purpose\n\nAuthentication.\n\n## Requirements\n\n\
         ```markdown\n### Requirement: Login\nExample only.\n```\n\n\
         ### Requirement: Login\nThe system SHALL use passwords.\n\n\
         #### Scenario: Password\n- **WHEN** a password is valid\n- **THEN** access is granted\n",
    )
    .unwrap();
    let delta = tmp.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(
        delta,
        "## MODIFIED Requirements\n\n\
         ### Requirement: Login\nThe system SHALL use passkeys.\n\n\
         #### Scenario: Password\n- **WHEN** a passkey is valid\n- **THEN** access is granted\n",
    )
    .unwrap();

    let out = spectra()
        .args(["archive", "feat", "--yes"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let updated = std::fs::read_to_string(main).unwrap();
    assert!(!updated.contains("The system SHALL use passwords."));
    assert!(updated.contains("The system SHALL use passkeys."));
    assert_eq!(updated.matches("```").count(), 2);
}

#[test]
fn archive_treats_identical_added_and_modified_requirements_as_already_synced() {
    let tmp = TempDir::new("archive-early-sync");
    init_project_with_change(&tmp, "feat");
    let main = tmp.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(main.parent().unwrap()).unwrap();
    let added = "### Requirement: Added\nThe system SHALL add behavior.\n\n\
        #### Scenario: Added\n- **WHEN** requested\n- **THEN** it is added";
    let modified = "### Requirement: Existing\nThe system SHALL keep behavior.\n\n\
        #### Scenario: Existing\n- **WHEN** requested\n- **THEN** it is kept";
    let original = format!(
        "# auth Specification\n\n## Purpose\n\nAuthentication.\n\n## Requirements\n\n{added}\n\n{modified}\n"
    );
    std::fs::write(&main, &original).unwrap();
    let delta = tmp.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(
        delta,
        format!("## ADDED Requirements\n\n{added}\n\n## MODIFIED Requirements\n\n{modified}\n"),
    )
    .unwrap();

    let out = spectra()
        .args(["archive", "feat", "--yes"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(std::fs::read_to_string(main).unwrap(), original);
}

#[test]
fn archive_rejects_case_variant_and_missing_removal_targets() {
    let typo = TempDir::new("archive-removal-typo");
    init_project_with_change(&typo, "feat");
    let main = typo.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(main.parent().unwrap()).unwrap();
    std::fs::write(
        &main,
        "# auth Specification\n\n## Purpose\n\nAuthentication.\n\n## Requirements\n\n\
         ### Requirement: Login\nThe system SHALL authenticate.\n\n\
         #### Scenario: Login\n- **WHEN** requested\n- **THEN** access is granted\n",
    )
    .unwrap();
    let delta = typo.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(
        &delta,
        "## REMOVED Requirements\n\n### Requirement: login\n",
    )
    .unwrap();
    let typo_out = spectra()
        .args(["archive", "feat", "--yes"])
        .current_dir(&*typo)
        .output()
        .unwrap();
    assert_eq!(typo_out.status.code(), Some(1));
    assert!(typo.join("openspec/changes/feat").is_dir());

    std::fs::write(
        &delta,
        "## REMOVED Requirements\n\n### Requirement: Missing\n",
    )
    .unwrap();
    let missing_out = spectra()
        .args(["archive", "feat", "--yes"])
        .current_dir(&*typo)
        .output()
        .unwrap();
    assert_eq!(missing_out.status.code(), Some(1));
    assert!(typo.join("openspec/changes/feat").is_dir());
    assert!(std::fs::read_to_string(main)
        .unwrap()
        .contains("### Requirement: Login"));
}

#[test]
fn archive_treats_an_existing_rename_target_as_already_synced() {
    let tmp = TempDir::new("archive-rename-synced");
    init_project_with_change(&tmp, "feat");
    let main = tmp.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(main.parent().unwrap()).unwrap();
    let original = "# auth Specification\n\n## Purpose\n\nAuthentication.\n\n## Requirements\n\n\
        ### Requirement: New login\nThe system SHALL authenticate.\n\n\
        #### Scenario: Login\n- **WHEN** requested\n- **THEN** access is granted\n";
    std::fs::write(&main, original).unwrap();
    let delta = tmp.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(
        delta,
        "## RENAMED Requirements\n\
         - FROM: `### Requirement: Old login`\n\
         - TO: `### Requirement: New login`\n",
    )
    .unwrap();

    let out = spectra()
        .args(["archive", "feat", "--yes"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(std::fs::read_to_string(main).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn archive_write_failure_keeps_the_change_active() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new("archive-rollback");
    init_project_with_change(&tmp, "feat");
    let delta = tmp.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(
        delta,
        "## ADDED Requirements\n\n### Requirement: Login\n\
         The system SHALL authenticate.\n\n#### Scenario: Login\n\
         - **WHEN** requested\n- **THEN** access is granted\n",
    )
    .unwrap();
    let target_dir = tmp.join("openspec/specs/auth");
    std::fs::create_dir_all(&target_dir).unwrap();
    std::fs::set_permissions(&target_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let out = spectra()
        .args(["archive", "feat", "--yes"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    std::fs::set_permissions(&target_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(tmp.join("openspec/changes/feat").is_dir());
    assert!(!tmp
        .join("openspec/changes/archive")
        .read_dir()
        .unwrap()
        .any(|entry| {
            entry.is_ok_and(|entry| entry.file_name().to_string_lossy().ends_with("-feat"))
        }));
    assert!(!target_dir.join("spec.md").exists());
}

#[test]
fn validate_errors_when_change_has_no_delta() {
    let tmp = TempDir::new("validate-nodelta");
    init_project_with_change(&tmp, "feat");
    // `new change` creates metadata only, so there are no specs/ deltas.

    let out = spectra()
        .args(["validate", "feat", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let report: serde_json::Value =
        serde_json::from_str(&String::from_utf8(out.stdout).unwrap()).unwrap();
    assert_eq!(report["items"][0]["valid"], false);
    let msg = report["items"][0]["issues"][0]["message"].as_str().unwrap();
    assert!(msg.contains("at least one delta"), "got: {msg}");
}

#[test]
fn skip_specs_validates_and_archives_a_behavior_neutral_change() {
    let tmp = TempDir::new("skip-specs");
    init_project_with_change(&tmp, "feat");
    std::fs::write(
        tmp.join("openspec/changes/feat/.openspec.yaml"),
        "schema: spec-driven\nskip_specs: true\n",
    )
    .unwrap();

    let validated = spectra()
        .args(["validate", "feat", "--strict", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(validated.status.success(), "{validated:?}");
    let report: serde_json::Value = serde_json::from_slice(&validated.stdout).unwrap();
    assert_eq!(report["summary"]["totals"]["failed"], 0);

    let archived = spectra()
        .args(["archive", "feat", "--yes"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(archived.status.success(), "{archived:?}");
}

#[test]
fn skip_specs_conflicts_with_delta_files() {
    let tmp = TempDir::new("skip-specs-conflict");
    init_project_with_change(&tmp, "feat");
    std::fs::write(
        tmp.join("openspec/changes/feat/.openspec.yaml"),
        "schema: spec-driven\nskip_specs: true\n",
    )
    .unwrap();
    let delta = tmp.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(&delta, "## ADDED Requirements\n\n### Requirement: Login\n").unwrap();

    let validated = spectra()
        .args(["validate", "feat", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert_eq!(validated.status.code(), Some(1));

    let archived = spectra()
        .args(["archive", "feat", "--yes"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert_eq!(archived.status.code(), Some(1));
    assert!(tmp.join("openspec/changes/feat").is_dir());
}

#[test]
fn strict_spec_validation_rejects_an_archived_purpose_placeholder() {
    let tmp = TempDir::new("validate-purpose-placeholder");
    init_project_with_change(&tmp, "feat");
    let spec = tmp.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(spec.parent().unwrap()).unwrap();
    std::fs::write(
        spec,
        "# auth Specification\n\n## Purpose\n\n\
         TBD - created by archiving change 'old'. Update Purpose after archive.\n\n\
         ## Requirements\n\n### Requirement: Login\n\
         The system SHALL authenticate.\n\n#### Scenario: Login\n\
         - **WHEN** requested\n- **THEN** access is granted\n",
    )
    .unwrap();

    let normal = spectra()
        .args(["validate", "auth", "--type", "spec", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(normal.status.success(), "{normal:?}");

    let strict = spectra()
        .args(["validate", "auth", "--type", "spec", "--strict", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert_eq!(strict.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&strict.stdout).unwrap();
    assert!(report["items"][0]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["message"]
            .as_str()
            .is_some_and(|message| message.contains("placeholder"))));
}

#[test]
fn validation_bulk_scopes_emit_versioned_additive_reports() {
    let tmp = TempDir::new("validate-bulk-v2");
    init_project_with_change(&tmp, "feat");
    std::fs::write(
        tmp.join("openspec/changes/feat/.openspec.yaml"),
        "schema: spec-driven\nskip_specs: true\n",
    )
    .unwrap();
    let spec = tmp.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(spec.parent().unwrap()).unwrap();
    std::fs::write(
        spec,
        "# auth Specification\n\n## Purpose\n\nAuthentication behavior.\n\n\
         ## Requirements\n\n### Requirement: Login\nThe system SHALL authenticate.\n\n\
         #### Scenario: Login\n- **WHEN** requested\n- **THEN** access is granted\n",
    )
    .unwrap();

    let all = spectra()
        .args(["validate", "--all", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(all.status.success(), "{all:?}");
    let report: serde_json::Value = serde_json::from_slice(&all.stdout).unwrap();
    assert_eq!(report["version"], "2.0");
    assert_eq!(report["summary"]["totals"]["failed"], 0);
    assert_eq!(report["summary"]["totals"]["items"], 2);
    assert_eq!(report["summary"]["byType"]["change"]["items"], 1);
    assert_eq!(report["summary"]["byType"]["spec"]["items"], 1);
    assert!(report["root"]["path"].is_string());
    assert!(report["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["type"] == "spec"));

    let findings = spectra()
        .args(["validate", "--all", "--report", "findings", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(findings.status.success(), "{findings:?}");
    let findings: serde_json::Value = serde_json::from_slice(&findings.stdout).unwrap();
    assert_eq!(findings["report"]["kind"], "validation-findings");
    assert_eq!(findings["report"]["totalItems"], 2);
    assert_eq!(findings["report"]["returnedItems"], 1);
}

#[test]
fn validate_archived_fails_on_incomplete_tasks() {
    let tmp = TempDir::new("validate-archived");
    init_project_with_change(&tmp, "feat");
    std::fs::write(
        tmp.join("openspec/changes/feat/tasks.md"),
        "## 1. Work\n- [ ] 1.1 unfinished\n",
    )
    .unwrap();
    let archived = spectra()
        .args(["archive", "feat", "--yes", "--skip-specs"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(archived.status.success(), "{archived:?}");

    let validated = spectra()
        .args(["validate", "--archived", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert_eq!(validated.status.code(), Some(1));
    let report: serde_json::Value = serde_json::from_slice(&validated.stdout).unwrap();
    assert_eq!(report["summary"]["totals"]["failed"], 1);
    assert!(report["items"][0]["issues"][0]["message"]
        .as_str()
        .unwrap()
        .contains("incomplete"));
}

#[test]
fn validate_errors_change_not_found_for_a_nonexistent_explicit_name() {
    // Regression (mob review): an explicit typo'd / archived name must report

    // "Change '<name>' not found." (like `archive`), not a misleading
    // "must contain at least one delta" validation failure.
    let tmp = TempDir::new("validate-notfound");
    init_project_with_change(&tmp, "real-change");

    let out = spectra()
        .args(["validate", "does-not-exist", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(1),
        "a missing change must exit 1 via the error path"
    );
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("Change 'does-not-exist' not found."),
        "expected not-found error, got stderr:\n{stderr}\nstdout:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    // Must be the error path, not a JSON validation report.
    assert!(
        String::from_utf8(out.stdout).unwrap().trim().is_empty(),
        "no JSON report should be emitted for a not-found change"
    );
}
#[test]
fn list_and_show_support_nested_canonical_spec_ids() {
    let tmp = TempDir::new("nested-canonical-spec");
    init_project_with_change(&tmp, "feat");
    let spec = tmp.join("openspec/specs/identity/auth/spec.md");
    std::fs::create_dir_all(spec.parent().unwrap()).unwrap();
    std::fs::write(&spec, "# Auth\n\nNested authentication.\n").unwrap();

    let listed = spectra()
        .args(["list", "--specs", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(listed.status.success(), "{listed:?}");
    let list_json: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(list_json["specs"][0]["name"], "identity/auth");

    let shown = spectra()
        .args(["show", "identity/auth", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(shown.status.success(), "{shown:?}");
    let show_json: serde_json::Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(show_json["spec"], "# Auth\n\nNested authentication.\n");
}

#[test]
fn archive_warns_and_preserves_existing_purpose() {
    let tmp = TempDir::new("archive-purpose-existing");
    init_project_with_change(&tmp, "feat");
    let main = tmp.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(main.parent().unwrap()).unwrap();
    std::fs::write(
        &main,
        "# auth Specification\n\n## Purpose\n\nExisting purpose.\n\n## Requirements\n",
    )
    .unwrap();
    let delta = tmp.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(
        delta,
        "## Purpose\n\nReplacement purpose.\n\n## ADDED Requirements\n\n\
         ### Requirement: Login\nThe system SHALL authenticate.\n\n\
         #### Scenario: Login\n- **WHEN** requested\n- **THEN** access is granted\n",
    )
    .unwrap();

    let out = spectra()
        .args(["archive", "feat", "--yes"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("ignoring delta Purpose"));
    let updated = std::fs::read_to_string(main).unwrap();
    assert!(updated.contains("Existing purpose."));
    assert!(!updated.contains("Replacement purpose."));
}

#[test]
fn list_help_does_not_mention_changes_as_unimplemented() {
    let out = spectra().args(["list", "--help"]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.contains("not yet implemented"));
    assert!(stdout.contains("--sort <SORT>"));
    assert!(stdout.contains("Sort by: name, modified, created"));
    assert!(stdout.contains("[default: modified]"));
}

/// The marker path spelled out literally, on purpose: every other assertion
/// in this file resolves it through `change::in_progress_marker_path`, the
/// same production helper that writes it, so those assertions only prove
/// "the file is wherever the code put it". Renaming the suffix or moving the
/// directory left the whole suite green until this literal landed here.
fn in_progress_marker(root: &Path, name: &str) -> PathBuf {
    root.join(".spectra")
        .join("changes")
        .join(format!("{name}.in-progress"))
}

#[test]
fn in_progress_add_marks_an_existing_change_without_output() {
    let tmp = TempDir::new("in-progress-existing");
    init_project_with_change(&tmp, "shipping");

    let out = spectra()
        .args(["in-progress", "add", "shipping"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "in-progress add failed: {out:?}"
    );
    assert!(out.stdout.is_empty(), "stdout must be exactly empty");
    // Exit 0 with empty stdout is also what doing nothing at all looks like:
    // without this the whole CLI-to-core wiring can be deleted and every test
    // still passes.
    assert!(
        in_progress_marker(&tmp, "shipping").is_file(),
        "marker must land at the documented .spectra/changes/<name>.in-progress"
    );

    // Idempotency at the CLI layer, not just in the core unit tests.
    let again = spectra()
        .args(["in-progress", "add", "shipping"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert_eq!(
        again.status.code(),
        Some(0),
        "repeated in-progress add failed: {again:?}"
    );
    assert!(again.stdout.is_empty(), "stdout must be exactly empty");
    assert!(
        in_progress_marker(&tmp, "shipping").is_file(),
        "marker must survive a repeated add"
    );
}

#[test]
fn in_progress_add_accepts_a_ghost_change() {
    let tmp = TempDir::new("in-progress-ghost");
    init_project_with_change(&tmp, "real-change");

    let out = spectra()
        .args(["in-progress", "add", "ghost-change"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert_eq!(
        out.status.code(),
        Some(0),
        "ghost-change add failed: {out:?}"
    );
    // The ghost marker is written, not skipped -- exit 0 alone cannot tell
    // "recorded a marker for a change that does not exist" (the oracle's
    // behavior) apart from "silently did nothing".
    assert!(
        in_progress_marker(&tmp, "ghost-change").is_file(),
        "ghost marker must be written despite the change not existing"
    );
    assert!(
        !tmp.join("openspec")
            .join("changes")
            .join("ghost-change")
            .exists(),
        "the change itself must not be created as a side effect"
    );
}

#[test]
fn in_progress_marker_does_not_change_list_or_status_output() {
    let tmp = TempDir::new("in-progress-write-only");
    init_project_with_change(&tmp, "shipping");

    // The fixture MUST report `"status": "done"` before the marker is added.
    // `list_change_items` derives `"in-progress"` for any change without a
    // fully-completed tasks.md (main.rs: `total > 0 && done == total`), and a
    // fresh `new change` has no tasks.md at all -- so on the default fixture
    // the pre-add JSON already says "in-progress", and a mutation wiring the
    // marker into that field produces byte-identical output. The lock would
    // silently prove nothing. Completing the tasks is what gives the two
    // states different bytes, and therefore gives this assertion teeth.
    std::fs::write(
        tmp.join("openspec")
            .join("changes")
            .join("shipping")
            .join("tasks.md"),
        "# Tasks\n\n- [x] 1. done\n",
    )
    .unwrap();
    let baseline = spectra()
        .args(["list", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&baseline.stdout).contains("\"status\": \"done\""),
        "fixture precondition: list --json must read \"done\" before the add, \
         else a marker leak into that field is invisible: {baseline:?}"
    );

    // `list --json` carries a *derived* task status that is also spelled
    // "in-progress" (main.rs `list_change_items`). It is unrelated to this
    // marker, and it is the surface most likely to be wired to it by mistake
    // -- so it has to be in the lock, not just the human-readable listing.
    // `analyze` is here because CHANGELOG names it among the locked surfaces.
    let read_paths: [&[&str]; 6] = [
        &["list"],
        &["list", "--json"],
        &["list", "--parked"],
        &["status"],
        &["analyze", "shipping"],
        &["show", "shipping"],
    ];

    let capture = |args: &[&str]| {
        let out = spectra().args(args).current_dir(&*tmp).output().unwrap();
        assert!(out.status.success(), "{args:?} failed: {out:?}");
        out.stdout
    };

    let before: Vec<Vec<u8>> = read_paths.iter().map(|a| capture(a)).collect();

    let add = spectra()
        .args(["in-progress", "add", "shipping"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(add.status.success(), "in-progress add failed: {add:?}");
    assert!(
        in_progress_marker(&tmp, "shipping").is_file(),
        "marker must exist, else this lock proves nothing"
    );

    for (args, expected) in read_paths.iter().zip(before) {
        assert_eq!(
            capture(args),
            expected,
            "{args:?} output changed after in-progress add; the marker must stay write-only"
        );
    }
}

#[test]
fn in_progress_add_rejects_json_output() {
    let tmp = TempDir::new("in-progress-json");
    init_project_with_change(&tmp, "shipping");

    let out = spectra()
        .args(["in-progress", "add", "shipping", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn in_progress_rejects_remove_subcommand() {
    // Runs in a TempDir like its siblings: clap rejects `remove` before any
    // root resolution today, but this test exists precisely for the day a
    // removal subcommand is added, and it must not touch the real checkout then.
    let tmp = TempDir::new("in-progress-remove");

    let out = spectra()
        .args(["in-progress", "remove", "shipping"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("remove"),
        "clap must name the rejected subcommand, else this passes for any error: {stderr}"
    );
}

#[test]
fn in_progress_add_requires_an_initialized_project() {
    let tmp = TempDir::new("in-progress-uninitialized");

    let out = spectra()
        .args(["in-progress", "add", "shipping"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("Not initialized"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn show_diff_isolates_modified_requirement_lines() {
    let tmp = TempDir::new("show-diff");
    init_project_with_change(&tmp, "feat");
    let main = tmp.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(main.parent().unwrap()).unwrap();
    std::fs::write(
        main,
        "# auth Specification\n\n## Purpose\n\nAuthentication.\n\n## Requirements\n\n\
         ### Requirement: Login\nThe system SHALL use passwords.\n\n\
         #### Scenario: Login\n- **WHEN** a password is valid\n- **THEN** access is granted\n",
    )
    .unwrap();
    let delta = tmp.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(
        delta,
        "## MODIFIED Requirements\n\n### Requirement: Login\n\
         The system SHALL use passkeys.\n\n#### Scenario: Login\n\
         - **WHEN** a passkey is valid\n- **THEN** access is granted\n",
    )
    .unwrap();

    let out = spectra()
        .args(["show", "feat", "--diff", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let diff = report["diff"][0]["diff"].as_str().unwrap();
    assert!(diff.contains("-The system SHALL use passwords."));
    assert!(diff.contains("+The system SHALL use passkeys."));
}

#[test]
fn validate_rejects_a_modified_requirement_missing_from_the_canonical_spec() {
    let tmp = TempDir::new("validate-missing-modified");
    init_project_with_change(&tmp, "feat");
    let main = tmp.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(main.parent().unwrap()).unwrap();
    std::fs::write(
        main,
        "# auth Specification\n\n## Purpose\n\nAuthentication.\n\n## Requirements\n\n\
         ### Requirement: Existing\nThe system SHALL keep existing behavior.\n\n\
         #### Scenario: Existing\n- **WHEN** requested\n- **THEN** it remains\n",
    )
    .unwrap();
    let delta = tmp.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(
        delta,
        "## MODIFIED Requirements\n\n### Requirement: Missing\n\
         The system SHALL modify behavior.\n\n#### Scenario: Modified\n\
         - **WHEN** requested\n- **THEN** it changes\n",
    )
    .unwrap();

    let out = spectra()
        .args(["validate", "feat", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["summary"]["totals"]["failed"], 1);
    assert!(report["items"][0]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["message"]
            .as_str()
            .is_some_and(|message| message.contains("MODIFY") && message.contains("Missing"))));
}

#[test]
fn validate_follows_a_transitive_rename_chain_when_checking_scenario_loss() {
    let tmp = TempDir::new("validate-rename-chain");
    init_project_with_change(&tmp, "feat");
    let main = tmp.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(main.parent().unwrap()).unwrap();
    std::fs::write(
        main,
        "# auth Specification\n\n## Purpose\n\nAuthentication.\n\n## Requirements\n\n\
         ### Requirement: Original login\nThe system SHALL authenticate users.\n\n\
         #### Scenario: Password login\n- **WHEN** credentials are valid\n- **THEN** access is granted\n\n\
         #### Scenario: Locked account\n- **WHEN** an account is locked\n- **THEN** access is denied\n",
    )
    .unwrap();
    let delta = tmp.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(
        delta,
        "## MODIFIED Requirements\n\n\
         ### Requirement: Final login\nThe system SHALL authenticate users.\n\n\
         #### Scenario: Password login\n- **WHEN** credentials are valid\n- **THEN** access is granted\n\n\
         ## RENAMED Requirements\n\
         - FROM: `### Requirement: Original login`\n\
         - TO: `### Requirement: Intermediate login`\n\
         - FROM: `### Requirement: Intermediate login`\n\
         - TO: `### Requirement: Final login`\n",
    )
    .unwrap();

    let out = spectra()
        .args(["validate", "feat", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(report["items"][0]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["message"]
            .as_str()
            .is_some_and(|message| message.contains("Locked account"))));
}

#[test]
fn validate_reports_archive_retirement_preflight_failures_as_findings() {
    let tmp = TempDir::new("validate-retirement-preflight");
    init_project_with_change(&tmp, "feat");
    std::fs::write(
        tmp.join("openspec/changes/feat/.openspec.yaml"),
        "schema: spec-driven\nretire_capabilities: true\n",
    )
    .unwrap();
    let main = tmp.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(main.parent().unwrap()).unwrap();
    std::fs::write(
        main,
        "# auth Specification\n\n## Purpose\n\nAuthentication.\n\n## Requirements\n\n\
         ### Requirement: Login\nThe system SHALL authenticate.\n\n\
         #### Scenario: Login\n- **WHEN** requested\n- **THEN** access is granted\n\n\
         ## Operational Notes\n\nThis content prevents whole-capability retirement.\n",
    )
    .unwrap();
    let delta = tmp.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(delta, "## REMOVED Requirements\n\n### Requirement: Login\n").unwrap();

    let out = spectra()
        .args(["validate", "feat", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(report["items"][0]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["message"]
            .as_str()
            .is_some_and(|message| message.contains("outside Purpose and Requirements"))));
}

#[test]
fn validate_type_requires_an_item_and_conflicts_with_bulk_scopes() {
    let tmp = TempDir::new("validate-type-arguments");
    for args in [
        &["validate", "--type", "change"][..],
        &["validate", "--changes", "--type", "change"][..],
        &["validate", "--specs", "--type", "spec"][..],
        &["validate", "--all", "--type", "change"][..],
        &["validate", "--archived", "--type", "change"][..],
    ] {
        let out = spectra().args(args).current_dir(&*tmp).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "{args:?} must be rejected by clap: {out:?}"
        );
    }
}

#[test]
fn schema_validate_without_a_name_includes_malformed_project_schemas() {
    let tmp = TempDir::new("schema-validate-malformed-bulk");
    init_project_with_change(&tmp, "feat");
    let schema = tmp.join("openspec/schemas/broken/schema.yaml");
    std::fs::create_dir_all(schema.parent().unwrap()).unwrap();
    std::fs::write(schema, "name: [unclosed\n").unwrap();

    let out = spectra()
        .args(["schema", "validate", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let checks: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let broken = checks
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["name"] == "broken")
        .expect("malformed on-disk schema must remain enumerable");
    assert_eq!(broken["valid"], false);
}

#[test]
fn validate_human_output_prints_warning_findings_for_a_valid_item() {
    let tmp = TempDir::new("validate-human-warning");
    init_project_with_change(&tmp, "feat");
    let delta = tmp.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(
        delta,
        "## ADDED Requirements\n\n### Requirement: Login\nUsers can log in.\n\n\
         #### Scenario: Login\n- **WHEN** credentials are valid\n- **THEN** access is granted\n",
    )
    .unwrap();

    let out = spectra()
        .args(["validate", "feat"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("feat"), "{stdout}");
    assert!(stdout.contains("OK"), "{stdout}");
    assert!(stdout.contains("WARNING specs/auth/spec.md:"), "{stdout}");
    assert!(stdout.contains("SHALL or MUST"), "{stdout}");
}

#[test]
fn validate_human_output_prints_info_findings_for_a_valid_item() {
    let tmp = TempDir::new("validate-human-info");
    init_project_with_change(&tmp, "feat");
    std::fs::write(
        tmp.join("openspec/changes/feat/.openspec.yaml"),
        "schema: spec-driven\nskip_specs: true\n",
    )
    .unwrap();

    let out = spectra()
        .args(["validate", "feat"])
        .current_dir(&*tmp)
        .output()
        .unwrap();

    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("OK"), "{stdout}");
    assert!(stdout.contains("INFO .openspec.yaml:"), "{stdout}");
    assert!(stdout.contains("declares skip_specs"), "{stdout}");
}

#[cfg(unix)]
#[test]
fn show_diff_and_validation_propagate_unreadable_canonical_spec_errors() {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new("unreadable-canonical-spec");
    init_project_with_change(&tmp, "feat");
    let main = tmp.join("openspec/specs/auth/spec.md");
    std::fs::create_dir_all(main.parent().unwrap()).unwrap();
    std::fs::write(
        &main,
        "# auth Specification\n\n## Purpose\n\nAuthentication.\n\n## Requirements\n\n\
         ### Requirement: Login\nThe system SHALL authenticate.\n\n\
         #### Scenario: Login\n- **WHEN** requested\n- **THEN** access is granted\n",
    )
    .unwrap();
    let delta = tmp.join("openspec/changes/feat/specs/auth/spec.md");
    std::fs::create_dir_all(delta.parent().unwrap()).unwrap();
    std::fs::write(
        delta,
        "## MODIFIED Requirements\n\n### Requirement: Login\n\
         The system SHALL authenticate safely.\n\n#### Scenario: Login\n\
         - **WHEN** requested\n- **THEN** access is granted\n",
    )
    .unwrap();
    std::fs::set_permissions(&main, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_to_string(&main).is_ok() {
        std::fs::set_permissions(&main, std::fs::Permissions::from_mode(0o644)).unwrap();
        return;
    }

    let diff = spectra()
        .args(["show", "feat", "--diff"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    let validation = spectra()
        .args(["validate", "feat"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    std::fs::set_permissions(&main, std::fs::Permissions::from_mode(0o644)).unwrap();

    for out in [diff, validation] {
        assert_eq!(out.status.code(), Some(1), "{out:?}");
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.contains("reading"), "{stderr}");
        assert!(stderr.contains("openspec/specs/auth/spec.md"), "{stderr}");
    }
}
