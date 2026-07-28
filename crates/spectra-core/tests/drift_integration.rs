//! End-to-end drift over a synthetic git repo.
//!
//! Uses a design with no incidental prose-symbol noise so anchor extraction is
//! deterministic. Covers the deliberate unresolved classification divergence,
//! baseline-aware FilePath handling, scoring, and JSON compatibility.

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
fn drift_separates_broken_and_unresolved_anchors() {
    let root = std::env::temp_dir().join(format!("spectra-drift-it-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "t@t.co"]);
    git(&root, &["config", "user.name", "t"]);

    write(
        &root.join(".spectra.yaml"),
        "spec_dir: openspec\nlocale: tw\n",
    );
    write(&root.join("src/deleted.rs"), "pub fn old_code() {}\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "baseline"]);
    let baseline = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(baseline.status.success());
    let baseline = String::from_utf8(baseline.stdout).unwrap();

    git(&root, &["rm", "-q", "src/deleted.rs"]);
    git(&root, &["commit", "-qm", "delete old code"]);

    let cd = root.join("openspec/changes/synthetic-change");
    write(&cd.join(".openspec.yaml"), "schema: spec-driven\n");
    write(&cd.join("proposal.md"), "# proposal\n");
    write(&cd.join("tasks.md"), "- [x] 1.1 done\n");
    write(
        &cd.join("design.md"),
        "paths `src/deleted.rs` and `src/planned.rs`\n\
         flag --directory\n\
         call jsonb_array_length()\n\
         type MissingWidget\n",
    );
    write(
        &root.join(".spectra/changes/synthetic-change.started"),
        baseline.trim(),
    );

    let cfg = Config::load(&root).unwrap();
    let ch = change::load(&cfg, "synthetic-change").unwrap();
    let report = drift::analyze(&cfg, &ch).unwrap();

    let broken: Vec<_> = report
        .broken_anchors
        .iter()
        .map(|anchor| {
            (
                anchor.anchor.as_str(),
                anchor.category.as_str(),
                anchor.reason.as_str(),
            )
        })
        .collect();
    assert_eq!(
        broken,
        vec![
            ("MissingWidget", "Symbol", "symbol not found in repo"),
            ("src/deleted.rs", "FilePath", "file does not exist")
        ]
    );
    let unresolved: Vec<_> = report
        .unresolved_anchors
        .iter()
        .map(|anchor| {
            (
                anchor.anchor.as_str(),
                anchor.category.as_str(),
                anchor.reason.as_str(),
            )
        })
        .collect();
    assert_eq!(
        unresolved,
        vec![
            ("--directory", "CliFlag", "no target --help"),
            ("jsonb_array_length", "Function", "not first-party"),
            ("src/planned.rs", "FilePath", "forward reference")
        ]
    );

    // Five anchors are extracted, but only the deleted FilePath and missing
    // Symbol are broken: 2/5 => D2, with no broken CliFlag => score 4. Counting
    // the three unresolved anchors as the oracle does would produce score 5.
    let structure = report
        .dimensions
        .iter()
        .find(|d| matches!(d.kind, drift::DimensionKind::Structure))
        .unwrap();
    assert_eq!(structure.status, "2/5 anchors broken");
    assert_eq!(structure.score, 4);
    assert_eq!(report.total_score, 4);

    // 40% actionable decay exceeds the 30% threshold.
    assert_eq!(report.severity, "heavy");
    assert_eq!(
        report.primary_recommendation,
        "spectra archive synthetic-change --skip-specs"
    );
    assert_eq!(report.last_commit, None);

    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["broken_anchors"].as_array().unwrap().len(), 2);
    assert_eq!(json["unresolved_anchors"].as_array().unwrap().len(), 3);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn drift_falls_back_to_broken_when_the_baseline_is_missing_or_malformed() {
    // PR #104 review（Codex Important）：baseline fallback 只有 resolver 單元
    // 測試涵蓋，缺少貫穿 change loading 與 drift::analyze 的整合案例——若
    // 回歸讓不可用的 baseline 誤用 HEAD 之類的值，缺失 FilePath 會被誤判成
    // forward reference（unresolved）而不再擋人。兩個變體都必須落在 broken。
    for (label, started) in [("missing", None), ("malformed", Some("not-a-sha at all\n"))] {
        let root = std::env::temp_dir().join(format!(
            "spectra-drift-it-nobase-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "t@t.co"]);
        git(&root, &["config", "user.name", "t"]);
        write(
            &root.join(".spectra.yaml"),
            "spec_dir: openspec\nlocale: tw\n",
        );
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-qm", "baseline"]);

        let cd = root.join("openspec/changes/synthetic-change");
        write(&cd.join(".openspec.yaml"), "schema: spec-driven\n");
        write(&cd.join("proposal.md"), "# proposal\n");
        write(&cd.join("tasks.md"), "- [x] 1.1 done\n");
        write(&cd.join("design.md"), "path `src/never-existed.rs`\n");
        if let Some(contents) = started {
            write(
                &root.join(".spectra/changes/synthetic-change.started"),
                contents,
            );
        }

        let cfg = Config::load(&root).unwrap();
        let ch = change::load(&cfg, "synthetic-change").unwrap();
        let report = drift::analyze(&cfg, &ch).unwrap();

        let broken: Vec<_> = report
            .broken_anchors
            .iter()
            .map(|a| (a.anchor.as_str(), a.reason.as_str()))
            .collect();
        assert_eq!(
            broken,
            vec![("src/never-existed.rs", "file does not exist")],
            "{label} baseline: a missing FilePath must fall back to broken, \
             not be classified as a forward reference"
        );
        assert!(
            report
                .unresolved_anchors
                .iter()
                .all(|a| a.anchor != "src/never-existed.rs"),
            "{label} baseline: the path must not appear as unresolved"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
