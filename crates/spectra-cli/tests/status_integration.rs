mod common;

use std::path::Path;

use common::{change_dir, init_project_with_change, spectra, TempDir};

#[test]
fn status_empty_change_reports_only_proposal_ready() {
    let root = TempDir::new("empty");
    init_project_with_change(&root, "demo-feature");

    let out = spectra()
        .args(["status", "--change", "demo-feature", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert!(out.status.success(), "status failed: {out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["changeName"], "demo-feature");
    assert_eq!(report["schemaName"], "spec-driven");
    assert_eq!(report["isComplete"], false);
    assert_eq!(report["applyRequires"], serde_json::json!(["tasks"]));
    assert_eq!(report["artifacts"][0]["status"], "ready");
    assert_eq!(report["artifacts"][1]["status"], "blocked");
    assert_eq!(
        report["artifacts"][1]["missingDeps"],
        serde_json::json!(["proposal"])
    );
    assert_eq!(report["artifacts"][2]["status"], "blocked");
    assert_eq!(
        report["artifacts"][2]["missingDeps"],
        serde_json::json!(["proposal"])
    );
    assert_eq!(report["artifacts"][3]["status"], "blocked");
    assert_eq!(
        report["artifacts"][3]["missingDeps"],
        serde_json::json!(["specs"])
    );
}

fn write_all_artifacts(root: &Path, name: &str) {
    let dir = change_dir(root, name);
    std::fs::write(dir.join("proposal.md"), "# Proposal\n").unwrap();
    std::fs::write(dir.join("design.md"), "# Design\n").unwrap();
    std::fs::write(dir.join("tasks.md"), "# Tasks\n").unwrap();
    let spec_dir = dir.join("specs").join("billing").join("invoices");
    std::fs::create_dir_all(&spec_dir).unwrap();
    std::fs::write(spec_dir.join("spec.md"), "# Spec\n").unwrap();
}

#[test]
fn status_file_existence_does_not_cascade_after_specs_are_deleted() {
    let root = TempDir::new("file-existence");
    init_project_with_change(&root, "demo-feature");
    write_all_artifacts(&root, "demo-feature");
    std::fs::remove_dir_all(change_dir(&root, "demo-feature").join("specs")).unwrap();

    let out = spectra()
        .args(["status", "--change", "demo-feature", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert!(out.status.success(), "status failed: {out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["artifacts"][2]["status"], "ready");
    assert_eq!(report["artifacts"][3]["status"], "done");
    assert_eq!(report["isComplete"], false);
}

#[test]
fn status_complete_omits_missing_deps_and_prints_completion_line() {
    let root = TempDir::new("complete");
    init_project_with_change(&root, "demo-feature");
    write_all_artifacts(&root, "demo-feature");

    let json_out = spectra()
        .args(["status", "--change", "demo-feature", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(json_out.status.success(), "status failed: {json_out:?}");
    let report: serde_json::Value = serde_json::from_slice(&json_out.stdout).unwrap();
    assert_eq!(report["isComplete"], true);
    assert_eq!(report["applyRequires"], serde_json::json!(["tasks"]));
    for artifact in report["artifacts"].as_array().unwrap() {
        assert_eq!(artifact["status"], "done");
        assert!(!artifact.as_object().unwrap().contains_key("missingDeps"));
    }

    let human_out = spectra()
        .args(["status", "--change", "demo-feature"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(human_out.status.success(), "status failed: {human_out:?}");
    assert_eq!(
        String::from_utf8(human_out.stdout).unwrap(),
        concat!(
            "Change: demo-feature\n",
            "Schema: spec-driven\n",
            "\n",
            "  ✓ proposal (proposal.md)\n",
            "  ✓ design (design.md)\n",
            "  ✓ specs (specs/**/*.md)\n",
            "  ✓ tasks (tasks.md)\n",
            "\n",
            "  ✓ All artifacts complete\n",
        )
    );
}

#[test]
fn status_json_is_additive_and_human_output_preserves_oracle_contract() {
    let root = TempDir::new("contract");
    init_project_with_change(&root, "demo-feature");

    let json_out = spectra()
        .args(["status", "--schema", "spec-driven", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(json_out.status.success(), "status failed: {json_out:?}");
    let json_text = String::from_utf8(json_out.stdout).unwrap();
    let report: serde_json::Value = serde_json::from_str(&json_text).unwrap();
    assert_eq!(report["changeName"], "demo-feature");
    assert_eq!(report["schemaName"], "spec-driven");
    assert_eq!(report["isComplete"], false);
    assert_eq!(report["isPlanningComplete"], false);
    assert_eq!(report["applyRequires"], serde_json::json!(["tasks"]));
    assert_eq!(report["artifacts"][0]["requires"], serde_json::json!([]));
    for key in [
        "changeName",
        "schemaName",
        "isComplete",
        "applyRequires",
        "artifacts",
    ] {
        assert!(report.as_object().unwrap().contains_key(key));
    }
    let positions: Vec<_> = [
        "\"changeName\"",
        "\"schemaName\"",
        "\"isComplete\"",
        "\"applyRequires\"",
        "\"artifacts\"",
    ]
    .iter()
    .map(|key| json_text.find(key).unwrap())
    .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(!report["artifacts"][0]
        .as_object()
        .unwrap()
        .contains_key("missingDeps"));

    let human_out = spectra()
        .arg("status")
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(human_out.status.success(), "status failed: {human_out:?}");
    assert_eq!(
        String::from_utf8(human_out.stdout).unwrap(),
        concat!(
            "Change: demo-feature\n",
            "Schema: spec-driven\n",
            "\n",
            "  ○ proposal (proposal.md)\n",
            "  ✗ design (design.md)\n",
            "    blocked by: proposal\n",
            "  ✗ specs (specs/**/*.md)\n",
            "    blocked by: proposal\n",
            "  ✗ tasks (tasks.md)\n",
            "    blocked by: specs\n",
            "\n",
        )
    );

    let bad_schema = spectra()
        .args(["status", "--schema", "bogus"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert_eq!(bad_schema.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(bad_schema.stderr).unwrap(),
        "Error: Schema not found: Schema 'bogus' not found in project, user, or built-in locations\n"
    );

    let missing_change = spectra()
        .args(["status", "--change", "nope"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert_eq!(missing_change.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(missing_change.stderr).unwrap(),
        "Error: Change 'nope' not found.\n"
    );
}

#[test]
fn new_change_writes_only_the_three_oracle_metadata_keys() {
    let root = TempDir::new("new-change-metadata");
    init_project_with_change(&root, "demo-feature");
    let dir = change_dir(&root, "demo-feature");

    assert!(!dir.join("proposal.md").exists());
    assert!(!dir.join("design.md").exists());
    assert!(!dir.join("tasks.md").exists());

    let metadata = std::fs::read_to_string(dir.join(".openspec.yaml")).unwrap();
    let lines: Vec<_> = metadata.lines().collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0], "schema: spec-driven");
    assert!(lines[1].starts_with("created: "));
    assert_eq!(lines[1].len(), "created: YYYY-MM-DD".len());
    assert_eq!(lines[2], "created_by: Howie <howie@example.com>");
    assert!(!metadata.contains("created_with"));
}

#[test]
fn status_still_reports_when_openspec_yaml_is_malformed() {
    // Established policy (change::load): a malformed .openspec.yaml warns
    // loudly and falls back to defaults instead of failing. Pins that the
    // new status command inherits that path -- report on stdout, warning on
    // stderr, exit 0.
    let root = TempDir::new("malformed-metadata");
    init_project_with_change(&root, "demo-feature");
    let meta_path = change_dir(&root, "demo-feature").join(".openspec.yaml");
    std::fs::write(&meta_path, "schema: [unclosed\n").unwrap();

    let out = spectra()
        .args(["status", "--change", "demo-feature"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert!(out.status.success(), "status failed: {out:?}");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("warning: ignoring unparseable"),
        "expected malformed-metadata warning, got: {stderr}"
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("proposal"),
        "status report missing artifacts: {stdout}"
    );
}

#[test]
fn status_exposes_dependencies_and_skip_specs_planning_completion() {
    let root = TempDir::new("skip-specs-status");
    init_project_with_change(&root, "demo-feature");
    let dir = change_dir(&root, "demo-feature");
    std::fs::write(
        dir.join(".openspec.yaml"),
        "schema: spec-driven\nskip_specs: true\n",
    )
    .unwrap();
    std::fs::write(dir.join("proposal.md"), "# Proposal\n").unwrap();
    std::fs::write(dir.join("design.md"), "# Design\n").unwrap();
    std::fs::write(dir.join("tasks.md"), "# Tasks\n").unwrap();

    let out = spectra()
        .args(["status", "--change", "demo-feature", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["isPlanningComplete"], true);
    assert_eq!(report["artifacts"][2]["status"], "skipped");
    assert_eq!(
        report["artifacts"][3]["requires"],
        serde_json::json!(["specs"])
    );
}

#[test]
fn status_all_returns_every_active_change_in_one_envelope() {
    let root = TempDir::new("status-all");
    init_project_with_change(&root, "z-last");
    let created = spectra()
        .args(["new", "change", "a-first"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(created.status.success(), "{created:?}");

    let out = spectra()
        .args(["status", "--all", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["changes"].as_array().unwrap().len(), 2);
    assert_eq!(report["changes"][0]["changeName"], "a-first");
    assert_eq!(report["changes"][1]["changeName"], "z-last");
}

#[test]
fn status_all_human_renders_the_full_artifact_dag_for_every_change() {
    let root = TempDir::new("status-all-human");
    init_project_with_change(&root, "z-last");
    let created = spectra()
        .args(["new", "change", "a-first"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(created.status.success(), "{created:?}");

    let out = spectra()
        .args(["status", "--all"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    let first = stdout.find("Change: a-first").unwrap();
    let second = stdout.find("Change: z-last").unwrap();
    assert!(first < second, "{stdout}");
    for section in [&stdout[first..second], &stdout[second..]] {
        assert!(section.contains("Schema: spec-driven"), "{section}");
        assert!(section.contains("proposal (proposal.md)"), "{section}");
        assert!(section.contains("design (design.md)"), "{section}");
        assert!(section.contains("specs (specs/**/*.md)"), "{section}");
        assert!(section.contains("tasks (tasks.md)"), "{section}");
        assert!(section.contains("blocked by:"), "{section}");
    }
}
