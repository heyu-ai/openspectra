//! End-to-end drift over a synthetic git repo.
//!
//! Uses a design with no prose-symbol noise (lowercase prose only) so anchor
//! extraction is fully deterministic and matches the reference exactly. Asserts
//! only time-independent outputs (broken set, Structure score, severity forced
//! by >30% decay, recommendation) so the test is stable on any run date.

use std::path::Path;
use std::process::Command;

use spectra_core::{change, config::Config, drift};

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

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

#[test]
fn drift_over_synthetic_repo_matches_recovered_rules() {
    let root = std::env::temp_dir().join(format!("spectra-drift-it-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "t@t.co"]);
    git(&root, &["config", "user.name", "t"]);

    write(&root.join(".spectra.yaml"), "spec_dir: openspec\nlocale: tw\n");
    // helper_fn is defined here so the Function anchor resolves; src/gone.rs is
    // referenced by the design but never created, so its FilePath anchor breaks.
    write(&root.join("src/lib.rs"), "pub fn helper_fn() {}\n");

    let cd = root.join("openspec/changes/synthetic-change");
    write(&cd.join(".openspec.yaml"), "schema: spec-driven\ncreated: 2026-01-01\n");
    write(&cd.join("proposal.md"), "# proposal\n");
    write(&cd.join("tasks.md"), "- [x] 1.1 done\n");
    write(
        &cd.join("design.md"),
        "uses `src/lib.rs` and `src/gone.rs`\nflags --json --verbose\ncalls helper_fn()\n",
    );
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "init"]);

    let cfg = Config::load(&root).unwrap();
    let ch = change::load(&cfg, "synthetic-change").unwrap();
    let report = drift::analyze(&cfg, &ch).unwrap();

    // Broken anchors: the missing file plus both CLI flags (always "not in --help").
    let mut broken: Vec<_> = report.broken_anchors.iter().map(|b| b.anchor.clone()).collect();
    broken.sort();
    assert_eq!(broken, vec!["--json", "--verbose", "src/gone.rs"]);

    // 5 anchors total (2 paths + 2 flags + 1 function), 3 broken => 60% decay.
    let structure = report
        .dimensions
        .iter()
        .find(|d| matches!(d.kind, drift::DimensionKind::Structure))
        .unwrap();
    assert_eq!(structure.status, "3/5 anchors broken");
    assert_eq!(structure.score, 7);

    // 60% decay exceeds the 30% threshold => heavy, regardless of run date.
    assert_eq!(report.severity, "heavy");
    assert_eq!(report.primary_recommendation, "spectra archive synthetic-change --skip-specs");
    assert_eq!(report.last_commit, None);

    let _ = std::fs::remove_dir_all(&root);
}
