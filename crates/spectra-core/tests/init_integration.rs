//! End-to-end `init` -> `new change` -> `drift` over a synthetic git repo,
//! proving a metadata-only new change is sufficient for drift to run.

use std::path::Path;
use std::process::Command;

use spectra_core::{change, config::Config, drift, init};

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

/// RAII scratch directory: removes itself on drop even if a test panics
/// partway through, so a failed assertion doesn't leak a directory in the
/// system temp dir.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "spectra-init-it-{label}-{}-{seq}",
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
fn init_then_new_change_then_drift_runs_end_to_end() {
    let root = TempDir::new("e2e");

    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "t@t.co"]);
    git(&root, &["config", "user.name", "t"]);

    let outcome = init::init(&root).unwrap();
    assert_eq!(outcome.spec_dir, "openspec");
    assert!(root.join("openspec/changes").is_dir());
    assert!(root.join("openspec/specs").is_dir());
    assert!(root.join(".gitignore").is_file());

    let cfg = Config::load(&root).unwrap();
    let ch = change::create(&cfg, "add-search-filter").unwrap();
    assert_eq!(ch.name, "add-search-filter");

    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "init"]);

    let report = drift::analyze(&cfg, &ch).unwrap();
    // A metadata-only new change has no broken anchors or task collisions.
    assert_eq!(report.severity, "light");
    assert!(report.broken_anchors.is_empty());
    assert!(report.unresolved_anchors.is_empty());
}

#[test]
fn init_is_idempotent_refusal_not_silent_reinit() {
    let root = TempDir::new("reinit");

    init::init(&root).unwrap();
    let err = init::init(&root).unwrap_err();
    assert!(err.to_string().contains("already initialized"));
}

#[test]
fn init_adopt_then_list_specs_sees_preexisting_capability() {
    let root = TempDir::new("adopt-list");
    std::fs::create_dir_all(root.join("openspec/specs/search")).unwrap();
    std::fs::write(
        root.join("openspec/specs/search/spec.md"),
        "# Search Specification\n\n## Purpose\n\nExisting.\n",
    )
    .unwrap();

    let outcome = init::init_with_options(&root, true).unwrap();

    assert!(outcome.adopted);
    let cfg = Config::load(&root).unwrap();
    assert_eq!(spectra_core::spec::list(&cfg).unwrap(), vec!["search"]);
}
