//! End-to-end `init` -> `new change` -> `drift` over a synthetic git repo,
//! proving `init`'s scaffold is sufficient for the rest of the pipeline to
//! run without any hand-written setup.

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

#[test]
fn init_then_new_change_then_drift_runs_end_to_end() {
    let root = std::env::temp_dir().join(format!("spectra-init-it-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

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
    // A brand-new change with lowercase-only placeholder prose has no broken
    // anchors and no pending-task collisions, so drift is minor.
    assert_eq!(report.severity, "light");
    assert!(report.broken_anchors.is_empty());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn init_is_idempotent_refusal_not_silent_reinit() {
    let root = std::env::temp_dir().join(format!("spectra-init-it-reinit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    init::init(&root).unwrap();
    let err = init::init(&root).unwrap_err();
    assert!(err.to_string().contains("already initialized"));

    let _ = std::fs::remove_dir_all(&root);
}
