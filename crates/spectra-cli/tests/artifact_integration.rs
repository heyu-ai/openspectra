mod common;

use std::io::Write;
use std::path::Path;
use std::process::{Output, Stdio};

use common::{change_dir, init_project_with_change, spectra, TempDir};

fn run_with_stdin(root: &Path, args: &[&str], content: &str) -> Output {
    let mut child = spectra()
        .args(args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(content.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn proposal_stdin_writes_exact_bytes_and_compact_json() {
    let root = TempDir::new("proposal");
    init_project_with_change(&root, "demo-feature");
    let content = "intro ## Why this matters";

    let out = run_with_stdin(
        &root,
        &[
            "new",
            "artifact",
            "proposal",
            "--change",
            "demo-feature",
            "--stdin",
            "--json",
        ],
        content,
    );

    assert!(out.status.success(), "new artifact failed: {out:?}");
    let path = change_dir(&root, "demo-feature").join("proposal.md");
    assert_eq!(std::fs::read(&path).unwrap(), content.as_bytes());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!(
            "{{\"artifact\":\"proposal\",\"change\":\"demo-feature\",\"path\":\"{}\",\"status\":\"created\",\"validated\":true,\"warnings\":[]}}\n",
            path.display()
        )
    );
}

#[test]
fn design_template_uses_schema_constant_and_is_not_validated() {
    let root = TempDir::new("design-template");
    init_project_with_change(&root, "demo-feature");

    let out = spectra()
        .args([
            "new",
            "artifact",
            "design",
            "--change",
            "demo-feature",
            "--json",
        ])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert!(out.status.success(), "new artifact failed: {out:?}");
    let path = change_dir(&root, "demo-feature").join("design.md");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        spectra_core::schema::DESIGN_TEMPLATE
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["artifact"], "design");
    assert_eq!(report["path"], path.to_string_lossy().as_ref());
    assert_eq!(report["validated"], false);
}

#[test]
fn tasks_stdin_with_checkbox_is_validated() {
    let root = TempDir::new("tasks");
    init_project_with_change(&root, "demo-feature");
    let content = "## 1. Work\n\n- [ ] 1.1 Implement it";

    let out = run_with_stdin(
        &root,
        &[
            "new",
            "artifact",
            "tasks",
            "--change",
            "demo-feature",
            "--stdin",
            "--json",
        ],
        content,
    );

    assert!(out.status.success(), "new artifact failed: {out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["validated"], true);
    assert_eq!(
        std::fs::read_to_string(change_dir(&root, "demo-feature").join("tasks.md")).unwrap(),
        content
    );
}

#[test]
fn spec_stdin_lands_under_capability_directory() {
    let root = TempDir::new("spec");
    init_project_with_change(&root, "demo-feature");
    let content = "## ADDED Requirements";

    let out = run_with_stdin(
        &root,
        &[
            "new",
            "artifact",
            "spec",
            "user-auth",
            "--change",
            "demo-feature",
            "--stdin",
            "--json",
        ],
        content,
    );

    assert!(out.status.success(), "new artifact failed: {out:?}");
    let path = change_dir(&root, "demo-feature")
        .join("specs")
        .join("user-auth")
        .join("spec.md");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["artifact"], "spec");
    assert_eq!(report["path"], path.to_string_lossy().as_ref());
    assert_eq!(report["validated"], true);
}

#[test]
fn unknown_type_reports_the_oracle_error() {
    let root = TempDir::new("unknown-type");
    init_project_with_change(&root, "demo-feature");

    let out = spectra()
        .args(["new", "artifact", "bogus", "--change", "demo-feature"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: Unknown artifact type 'bogus'. Valid types: proposal, design, tasks, spec\n"
    );
}

#[test]
fn spec_without_capability_reports_the_oracle_error() {
    let root = TempDir::new("missing-capability");
    init_project_with_change(&root, "demo-feature");

    let out = spectra()
        .args(["new", "artifact", "spec", "--change", "demo-feature"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: Capability name is required for spec type. Usage: spectra new artifact spec <capability> --change <name>\n"
    );
}

#[test]
fn already_exists_errors_then_force_overwrites() {
    let root = TempDir::new("force");
    init_project_with_change(&root, "demo-feature");
    let path = change_dir(&root, "demo-feature").join("proposal.md");

    let first = run_with_stdin(
        &root,
        &[
            "new",
            "artifact",
            "proposal",
            "--change",
            "demo-feature",
            "--stdin",
        ],
        "## Why first",
    );
    assert!(first.status.success(), "first create failed: {first:?}");

    let second = run_with_stdin(
        &root,
        &[
            "new",
            "artifact",
            "proposal",
            "--change",
            "demo-feature",
            "--stdin",
        ],
        "## Why second",
    );
    assert_eq!(second.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(second.stderr).unwrap(),
        format!(
            "Error: Artifact already exists: {}. Use --force to overwrite\n",
            path.display()
        )
    );

    let forced = run_with_stdin(
        &root,
        &[
            "new",
            "artifact",
            "proposal",
            "--change",
            "demo-feature",
            "--stdin",
            "--force",
        ],
        "## Why replacement",
    );
    assert!(forced.status.success(), "forced create failed: {forced:?}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "## Why replacement");
}

#[test]
fn proposal_validation_failure_does_not_create_file() {
    let root = TempDir::new("validation-failure");
    init_project_with_change(&root, "demo-feature");
    let path = change_dir(&root, "demo-feature").join("proposal.md");

    let out = run_with_stdin(
        &root,
        &[
            "new",
            "artifact",
            "proposal",
            "--change",
            "demo-feature",
            "--stdin",
        ],
        "## Motivation",
    );

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: Proposal must contain a ## Why, ## Problem, or ## Summary section\n"
    );
    assert!(!path.exists());
}

#[test]
fn nonexistent_explicit_change_has_no_trailing_period_in_error() {
    let root = TempDir::new("missing-change");
    init_project_with_change(&root, "demo-feature");

    let out = spectra()
        .args(["new", "artifact", "proposal", "--change", "no-such"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: Change 'no-such' not found\n"
    );
}

#[test]
fn human_output_includes_validation_line_only_for_stdin() {
    let root = TempDir::new("human");
    init_project_with_change(&root, "demo-feature");

    let out = run_with_stdin(
        &root,
        &[
            "new",
            "artifact",
            "proposal",
            "--change",
            "demo-feature",
            "--stdin",
        ],
        "## Why human output",
    );

    assert!(out.status.success(), "new artifact failed: {out:?}");
    let path = change_dir(&root, "demo-feature").join("proposal.md");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!(
            "✓ Created proposal: {}\n  Content validated ✓\n",
            path.display()
        )
    );
}

#[test]
fn force_with_invalid_content_exits_1_and_preserves_the_original_file() {
    // Probed contract (design.md): --force does NOT skip content validation;
    // invalid stdin + --force exits 1 and the existing artifact is untouched.
    // Previously true only by the accident of validation-before-write
    // ordering -- this pins it against refactors that would silently clobber
    // a valid artifact.
    let root = TempDir::new("force-no-clobber");
    init_project_with_change(&root, "demo-feature");
    let path = change_dir(&root, "demo-feature").join("proposal.md");

    let first = run_with_stdin(
        &root,
        &[
            "new",
            "artifact",
            "proposal",
            "--change",
            "demo-feature",
            "--stdin",
        ],
        "## Why original",
    );
    assert!(first.status.success(), "first create failed: {first:?}");

    let clobber_attempt = run_with_stdin(
        &root,
        &[
            "new",
            "artifact",
            "proposal",
            "--change",
            "demo-feature",
            "--stdin",
            "--force",
        ],
        "no required heading here",
    );
    assert_eq!(clobber_attempt.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(clobber_attempt.stderr).unwrap(),
        "Error: Proposal must contain a ## Why, ## Problem, or ## Summary section\n"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "## Why original");
}

#[test]
fn extra_capability_positional_is_ignored_for_non_spec_types() {
    // Probed against Spectra.app 2.3.1 (2026-07-18): the oracle also accepts
    // and silently ignores a capability positional for non-spec types
    // (`new artifact proposal extra-arg --change X --stdin` exits 0 and
    // creates the file). Pinned so a future "reject it" change is a
    // deliberate divergence, not an accident.
    let root = TempDir::new("extra-positional");
    init_project_with_change(&root, "demo-feature");

    let out = run_with_stdin(
        &root,
        &[
            "new",
            "artifact",
            "proposal",
            "extra-arg",
            "--change",
            "demo-feature",
            "--stdin",
        ],
        "## Why ignored positional",
    );

    assert!(out.status.success(), "new artifact failed: {out:?}");
    let path = change_dir(&root, "demo-feature").join("proposal.md");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "## Why ignored positional"
    );
}

#[test]
fn json_mode_errors_stay_on_stderr_with_no_partial_json() {
    // Errors must not change shape under --json: same plain-text stderr,
    // exit 1, and nothing (not even partial JSON) on stdout.
    let root = TempDir::new("json-error");
    init_project_with_change(&root, "demo-feature");

    let out = run_with_stdin(
        &root,
        &[
            "new",
            "artifact",
            "nonsense",
            "--change",
            "demo-feature",
            "--stdin",
            "--json",
        ],
        "## Why",
    );

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: Unknown artifact type 'nonsense'. Valid types: proposal, design, tasks, spec\n"
    );
    assert_eq!(out.stdout, b"");
}
