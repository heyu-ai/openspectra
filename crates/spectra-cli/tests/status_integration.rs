use std::path::Path;
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
            "spectra-status-it-{label}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
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

#[cfg(unix)]
#[test]
fn status_and_analyze_fail_loudly_on_an_unreadable_spec_directory() {
    use std::os::unix::fs::PermissionsExt;

    let root = TempDir::new("unreadable-specs");
    init_project_with_change(&root, "demo-feature");
    let specs = change_dir(&root, "demo-feature").join("specs");
    std::fs::create_dir_all(&specs).unwrap();
    std::fs::set_permissions(&specs, std::fs::Permissions::from_mode(0o000)).unwrap();

    if std::fs::read_dir(&specs).is_ok() {
        eprintln!(
            "skipping status_and_analyze_fail_loudly_on_an_unreadable_spec_directory: \
             running as root (chmod 0o000 not enforced)"
        );
        std::fs::set_permissions(&specs, std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }

    for (command, args) in [
        ("status", vec!["status", "--change", "demo-feature"]),
        ("analyze", vec!["analyze", "demo-feature"]),
    ] {
        let output = spectra().args(args).current_dir(&*root).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{command}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("Permission denied"),
            "{command}: {output:?}"
        );
    }

    std::fs::set_permissions(&specs, std::fs::Permissions::from_mode(0o755)).unwrap();
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
fn status_json_and_human_contracts_match_the_oracle() {
    let root = TempDir::new("contract");
    init_project_with_change(&root, "demo-feature");

    let json_out = spectra()
        .args(["status", "--schema", "spec-driven", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(json_out.status.success(), "status failed: {json_out:?}");
    let json_text = String::from_utf8(json_out.stdout).unwrap();
    assert_eq!(
        json_text,
        concat!(
            "{\n",
            "  \"changeName\": \"demo-feature\",\n",
            "  \"schemaName\": \"spec-driven\",\n",
            "  \"isComplete\": false,\n",
            "  \"applyRequires\": [\n",
            "    \"tasks\"\n",
            "  ],\n",
            "  \"artifacts\": [\n",
            "    {\n",
            "      \"id\": \"proposal\",\n",
            "      \"outputPath\": \"proposal.md\",\n",
            "      \"status\": \"ready\"\n",
            "    },\n",
            "    {\n",
            "      \"id\": \"design\",\n",
            "      \"outputPath\": \"design.md\",\n",
            "      \"status\": \"blocked\",\n",
            "      \"missingDeps\": [\n",
            "        \"proposal\"\n",
            "      ]\n",
            "    },\n",
            "    {\n",
            "      \"id\": \"specs\",\n",
            "      \"outputPath\": \"specs/**/*.md\",\n",
            "      \"status\": \"blocked\",\n",
            "      \"missingDeps\": [\n",
            "        \"proposal\"\n",
            "      ]\n",
            "    },\n",
            "    {\n",
            "      \"id\": \"tasks\",\n",
            "      \"outputPath\": \"tasks.md\",\n",
            "      \"status\": \"blocked\",\n",
            "      \"missingDeps\": [\n",
            "        \"specs\"\n",
            "      ]\n",
            "    }\n",
            "  ]\n",
            "}\n",
        )
    );
    let report: serde_json::Value = serde_json::from_str(&json_text).unwrap();
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
