//! End-to-end drift over a synthetic git repo.
//!
//! Uses a design with no incidental prose-symbol noise so anchor extraction is
//! deterministic. Covers the broken/unresolved split, baseline-aware FilePath
//! handling, scoring, and JSON compatibility.

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
            ("--directory", "CliFlag", "not in --help"),
            ("MissingWidget", "Symbol", "symbol not found in repo"),
            (
                "jsonb_array_length",
                "Function",
                "function not found in repo"
            ),
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
        vec![("src/planned.rs", "FilePath", "forward reference")]
    );

    // Five anchors are extracted; four are broken and only the not-yet-created
    // FilePath stays unresolved: 4/5 => D2, with one broken CliFlag =>
    // min(2*2+3, 2*2+1) = 5.
    let structure = report
        .dimensions
        .iter()
        .find(|d| matches!(d.kind, drift::DimensionKind::Structure))
        .unwrap();
    assert_eq!(structure.status, "4/5 anchors broken");
    assert_eq!(structure.score, 5);
    assert_eq!(report.total_score, 5);

    // 80% actionable decay exceeds the 30% threshold.
    assert_eq!(report.severity, "heavy");
    assert_eq!(
        report.primary_recommendation,
        "spectra archive synthetic-change --skip-specs"
    );
    assert_eq!(report.last_commit, None);

    let json = serde_json::to_value(&report).unwrap();
    assert_eq!(json["broken_anchors"].as_array().unwrap().len(), 4);
    assert_eq!(json["unresolved_anchors"].as_array().unwrap().len(), 1);

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn drift_reports_a_broken_anchor_past_the_cap_on_an_over_cap_design() {
    // #119: with ANCHOR_CAP applied as truncate(50) this design reported
    // "0/50 anchors broken" -- the one genuinely missing file sits at index 50
    // and was discarded before resolution. The oracle samples instead of
    // truncating, keeps index 50, and reports it.
    const TOTAL: usize = 55;
    const MISSING: usize = 50;

    let root =
        std::env::temp_dir().join(format!("spectra-drift-it-overcap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "t@t.co"]);
    git(&root, &["config", "user.name", "t"]);
    write(
        &root.join(".spectra.yaml"),
        "spec_dir: openspec\nlocale: tw\n",
    );
    for i in (0..TOTAL).filter(|i| *i != MISSING) {
        write(&root.join(format!("src/mod{i:03}.rs")), "pub fn f() {}\n");
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "baseline"]);

    let cd = root.join("openspec/changes/over-cap");
    write(&cd.join("proposal.md"), "# proposal\n");
    write(&cd.join("tasks.md"), "- [x] 1.1 done\n");
    let design: String = (0..TOTAL)
        .map(|i| format!("- `src/mod{i:03}.rs`\n"))
        .collect();
    write(&cd.join("design.md"), &design);

    let cfg = Config::load(&root).unwrap();
    let ch = change::load(&cfg, "over-cap").unwrap();
    let report = drift::analyze(&cfg, &ch).unwrap();

    let structure = report
        .dimensions
        .iter()
        .find(|d| matches!(d.kind, drift::DimensionKind::Structure))
        .unwrap();
    // One category, over the cap => sampled to 12, and index 50 is one of the
    // sampled positions (i * 55 / 12 for i = 11).
    assert_eq!(structure.status, "1/12 anchors broken");
    let broken: Vec<_> = report
        .broken_anchors
        .iter()
        .map(|a| a.anchor.as_str())
        .collect();
    assert_eq!(broken, vec![format!("src/mod{MISSING:03}.rs")]);

    // Probe the downstream chain, not just the recovered constant: 1/12 is 8.3%
    // decay (D0) with no broken CliFlag, so `min(2*0+3, 2*0+0)` is 0 and the
    // change must stay `light` with the apply recommendation. Without this, a
    // sampling change could move the denominator and silently shift severity.
    assert_eq!(structure.score, 0);
    assert_eq!(report.total_score, 0);
    assert_eq!(report.severity, "light");
    assert_eq!(report.primary_recommendation, "/spectra-apply over-cap");

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_committed_design_self_matches_its_own_function_anchors() {
    // Pins the `git grep` self-match blind spot flagged in PR #1 review. It was
    // cosmetic while #83 held Function anchors as `unresolved`; since #119 put
    // them back into `broken` they feed `structure_score`, so a self-match now
    // suppresses real signal and the behavior needs to be locked either way.
    //
    // `drift_separates_broken_and_unresolved_anchors` covers the opposite setup
    // (an untracked `design.md`, where the same anchor IS reported broken), so
    // the two together bracket the blind spot rather than hiding it.
    //
    // The blind spot is the oracle's own: probed on v2.3.1 with this exact
    // fixture, it also reports `0/1 anchors broken`. So this locks faithful
    // reproduction, not a divergence — PR #1's open question ("faithful-repro
    // vs fix") now has the faithful-repro half measured and pinned.
    let root =
        std::env::temp_dir().join(format!("spectra-drift-it-selfmatch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "t@t.co"]);
    git(&root, &["config", "user.name", "t"]);
    write(
        &root.join(".spectra.yaml"),
        "spec_dir: openspec\nlocale: tw\n",
    );

    let cd = root.join("openspec/changes/self-match");
    write(&cd.join("proposal.md"), "# proposal\n");
    write(&cd.join("tasks.md"), "- [x] 1.1 done\n");
    // `absent_helper` exists nowhere in the repo except this design.md.
    write(&cd.join("design.md"), "calls absent_helper()\n");
    // Committing the change dir is what creates the self-match: `git grep`
    // searches every tracked file, design.md included.
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "commit the change dir"]);

    let cfg = Config::load(&root).unwrap();
    let ch = change::load(&cfg, "self-match").unwrap();
    let report = drift::analyze(&cfg, &ch).unwrap();

    let structure = report
        .dimensions
        .iter()
        .find(|d| matches!(d.kind, drift::DimensionKind::Structure))
        .unwrap();
    assert_eq!(
        structure.status, "0/1 anchors broken",
        "a tracked design.md resolves its own Function anchor via git grep, so \
         the anchor reads healthy even though the function does not exist in \
         any implementation file"
    );
    assert!(report.broken_anchors.is_empty());
    assert!(report.unresolved_anchors.is_empty());

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
