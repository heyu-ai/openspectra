use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

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
            "spectra-artifact-it-{label}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(std::fs::canonicalize(dir).unwrap())
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn init_project_with_change(root: &Path, name: &str) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Howie"]);
    git(root, &["config", "user.email", "howie@example.com"]);

    let init = spectra().arg("init").current_dir(root).output().unwrap();
    assert!(init.status.success(), "init failed: {init:?}");
    let new_change = spectra()
        .args(["new", "change", name])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        new_change.status.success(),
        "new change failed: {new_change:?}"
    );
}

fn change_dir(root: &Path, name: &str) -> std::path::PathBuf {
    root.join("openspec").join("changes").join(name)
}

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
