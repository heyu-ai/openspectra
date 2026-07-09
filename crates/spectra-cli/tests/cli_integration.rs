//! CLI-level integration tests that spawn the built `spectra` binary, for
//! contracts that depend on full dispatch through `run()` (not just clap
//! parsing) -- unit tests in `main.rs` cover the parser; these cover the
//! actual runtime behavior.

use std::path::{Path, PathBuf};
use std::process::Command;

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
}

#[test]
fn init_text_output_reports_root_spec_dir_and_gitignore_update() {
    let tmp = TempDir::new("init-text");
    git(&tmp, &["init", "-q"]);

    let out = spectra().arg("init").current_dir(&*tmp).output().unwrap();
    assert!(out.status.success(), "init failed: {out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();

    // Canonicalize before comparing: on macOS `std::env::temp_dir()` returns
    // a `/var/...` path that's actually a symlink to `/private/var/...`, and
    // the CLI reports whatever `std::env::current_dir()` resolves to after
    // `cd`-ing in, which follows the symlink -- a raw `contains` would only
    // pass by coincidence (see the sibling JSON test for the same issue).
    let canonical_root = tmp.canonicalize().unwrap();
    assert!(stdout.contains(&canonical_root.display().to_string()));
    assert!(stdout.contains("spec_dir: openspec"));
    assert!(stdout.contains("Added '.spectra/' to .gitignore."));
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

/// Init a git repo + spectra project in `tmp` and scaffold change `name`.
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
    assert_eq!(report["items"][0]["issues"][0]["level"], "ERROR");
    assert_eq!(
        out.status.code(),
        Some(1),
        "an invalid change must exit 1 (gate semantics); stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn validate_nonstrict_passes_a_structurally_present_delta() {
    // The same bad delta as above (no SHALL, no scenario) is structurally a
    // delta, so a non-strict run gates only on structure and exits 0.
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
        "## ADDED Requirements\n\n### Requirement: Login\n\nUsers can log in.\n",
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
}

#[test]
fn validate_errors_when_change_has_no_delta() {
    let tmp = TempDir::new("validate-nodelta");
    init_project_with_change(&tmp, "feat");
    // `new change` scaffolds proposal/design/tasks but no specs/ deltas.

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
fn list_help_does_not_mention_changes_as_unimplemented() {
    let out = spectra().args(["list", "--help"]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.contains("not yet implemented"));
}
