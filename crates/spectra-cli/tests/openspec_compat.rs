//! End-to-end compatibility coverage for an OpenSpec-style project that has
//! `openspec/` content but no root `.spectra.yaml`.

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
            "spectra-openspec-compat-{label}-{}-{seq}",
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

fn copy_dir_all(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir_all(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

#[test]
fn adopt_list_drift_and_archive_a_vendored_openspec_project() {
    let root = TempDir::new("e2e");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("openspec_project");
    copy_dir_all(&fixture, &root);
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "t@t.co"]);
    git(&root, &["config", "user.name", "t"]);

    let init = spectra()
        .args(["init", "--adopt", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(init.status.success(), "init --adopt failed: {init:?}");
    let init_json: serde_json::Value = serde_json::from_slice(&init.stdout).unwrap();
    assert_eq!(init_json["spec_dir"], "openspec");
    assert_eq!(init_json["adopted"], true);
    assert!(root.join(".spectra.yaml").is_file());

    let list = spectra()
        .args(["list", "--specs", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(list.status.success(), "list --specs failed: {list:?}");
    let list_json: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    assert!(list_json["specs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["name"] == "session-management"));

    let drift = spectra()
        .args(["drift", "tighten-session-security", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(
        matches!(drift.status.code(), Some(0)),
        "successful drift always exits 0 (oracle-pinned), got: {drift:?}"
    );
    let _: serde_json::Value = serde_json::from_slice(&drift.stdout).unwrap();

    let archive = spectra()
        .args(["archive", "tighten-session-security"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(archive.status.success(), "archive failed: {archive:?}");

    let spec = std::fs::read_to_string(
        root.join("openspec")
            .join("specs")
            .join("session-management")
            .join("spec.md"),
    )
    .unwrap();
    assert!(spec.contains("### Requirement: Device Trust"));
    assert!(spec.contains("The system MUST expire sessions after 15 minutes of inactivity."));
    // MODIFIED replaces the whole block: the superseded requirement statement
    // and its old scenario must be gone. (The delta's own `(Previously: 30
    // minutes)` provenance line legitimately survives -- that's OpenSpec's
    // human-facing record of what changed, not stale requirement text.)
    assert!(
        !spec.contains("MUST expire sessions after 30 minutes"),
        "superseded requirement statement must be gone, got:\n{spec}"
    );
    assert!(
        !spec.contains("30 minutes pass without user activity"),
        "superseded scenario must be gone, got:\n{spec}"
    );
    assert!(!spec.contains("### Requirement: Remember Me"));
    assert!(spec.contains("### Requirement: Session Audit Trail"));
    assert!(!spec.contains("### Requirement: Legacy Audit Log"));
    // Every surviving requirement header must remain at the start of its own
    // line. The MODIFIED-glue regression would otherwise concatenate the header
    // following the modified "Session Expiration" block onto its last line,
    // silently dropping it from `^### Requirement:` parsing.
    for header in ["Device Trust", "Session Expiration", "Session Audit Trail"] {
        assert!(
            spec.contains(&format!("\n### Requirement: {header}")),
            "{header} must remain line-anchored after the merge, got:\n{spec}"
        );
    }

    assert!(!root
        .join("openspec")
        .join("changes")
        .join("tighten-session-security")
        .exists());
    let archive_dir = root.join("openspec").join("changes").join("archive");
    let archived_names: Vec<String> = std::fs::read_dir(archive_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert!(
        archived_names
            .iter()
            .any(|name| name.ends_with("-tighten-session-security")),
        "expected archived change directory, got: {archived_names:?}"
    );
}
