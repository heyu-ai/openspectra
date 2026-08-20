//! End-to-end integration tests for custom schema loading (#126).
//!
//! These drive the real CLI binary over a project with a custom schema at
//! `<spec_dir>/schemas/<name>/schema.yaml`, verifying that `status --json`,
//! `instructions --json`, and `templates` reflect the custom schema's own
//! name, artifacts, instructions, and templates -- not the built-in
//! `spec-driven` defaults -- and that the pipeline still fails loud when the
//! configured schema has no matching directory on disk.

mod common;

use std::path::Path;

use common::{git, spectra, TempDir};

/// Writes a two-artifact custom schema (`proposal` -> `tasks`) named
/// "My Custom Schema" at `<root>/openspec/schemas/mycustom/`, points the
/// project at it via `config.yaml`, and creates a change under it so the
/// change's own `.openspec.yaml` also records `schema: mycustom` (mirroring
/// how `spectra new change` stamps the configured schema per #117).
fn init_project_with_custom_schema(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Howie"]);
    git(root, &["config", "user.email", "howie@example.com"]);

    let init = spectra().arg("init").current_dir(root).output().unwrap();
    assert!(init.status.success(), "init failed: {init:?}");

    std::fs::write(
        root.join("openspec").join("config.yaml"),
        "schema: mycustom\n",
    )
    .unwrap();

    let schema_dir = root.join("openspec").join("schemas").join("mycustom");
    std::fs::create_dir_all(schema_dir.join("templates")).unwrap();
    std::fs::write(
        schema_dir.join("schema.yaml"),
        r#"name: My Custom Schema
version: 1
description: A test custom schema
artifacts:
- id: proposal
  generates: proposal.md
  description: Custom proposal
  template: proposal.md
  instruction: |
    Write a custom proposal.
  requires: []
- id: tasks
  generates: tasks.md
  description: Custom tasks
  template: tasks.md
  instruction: |
    Write custom tasks.
  requires: [proposal]
apply:
  requires: [tasks]
  tracks: tasks.md
  instruction: |
    Apply custom tasks.
"#,
    )
    .unwrap();
    std::fs::write(
        schema_dir.join("templates").join("proposal.md"),
        "## Custom Proposal Template\n",
    )
    .unwrap();
    std::fs::write(
        schema_dir.join("templates").join("tasks.md"),
        "## Custom Tasks Template\n",
    )
    .unwrap();

    let new_change = spectra()
        .args(["new", "change", "test-change"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        new_change.status.success(),
        "new change failed: {new_change:?}"
    );
}

#[test]
fn status_json_shows_custom_schema_name_and_artifacts() {
    let root = TempDir::new("status-custom");
    init_project_with_custom_schema(&root);

    let output = spectra()
        .args(["status", "--change", "test-change", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schemaName"], "My Custom Schema");
    assert_eq!(json["artifacts"].as_array().unwrap().len(), 2);
    assert_eq!(json["artifacts"][0]["id"], "proposal");
    assert_eq!(json["artifacts"][1]["id"], "tasks");
}

#[test]
fn instructions_json_uses_custom_instruction_and_template() {
    let root = TempDir::new("instructions-custom");
    init_project_with_custom_schema(&root);

    let output = spectra()
        .args([
            "instructions",
            "proposal",
            "--change",
            "test-change",
            "--json",
        ])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schemaName"], "My Custom Schema");
    assert_eq!(json["instruction"], "Write a custom proposal.\n");
    assert_eq!(json["template"], "## Custom Proposal Template\n");
}

/// #126 review follow-up: `templates --schema <custom>` must show the custom
/// schema's own name in the header and its own artifacts in the listing --
/// not the built-in `spec-driven` schema's 4 artifacts.
#[test]
fn templates_schema_flag_shows_custom_schema_name_and_artifacts() {
    let root = TempDir::new("templates-custom");
    init_project_with_custom_schema(&root);

    let text = spectra()
        .args(["templates", "--schema", "mycustom", "--no-color"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(
        text.status.success(),
        "{}",
        String::from_utf8_lossy(&text.stderr)
    );
    let stdout = String::from_utf8(text.stdout).unwrap();
    assert_eq!(
        stdout,
        "Templates (My Custom Schema)\n  \u{2713} proposal \u{2192} proposal.md\n  \u{2713} tasks \u{2192} tasks.md\n"
    );

    let json = spectra()
        .args(["templates", "--schema", "mycustom", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(
        json.status.success(),
        "{}",
        String::from_utf8_lossy(&json.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    let artifacts = value.as_array().unwrap();
    assert_eq!(artifacts.len(), 2, "must not list the built-in 4: {value}");
    assert_eq!(artifacts[0]["artifactId"], "proposal");
    assert_eq!(artifacts[0]["templateName"], "proposal.md");
    assert_eq!(artifacts[1]["artifactId"], "tasks");
    assert_eq!(artifacts[1]["templateName"], "tasks.md");
}

#[test]
fn status_exits_nonzero_when_custom_schema_dir_is_missing() {
    let root = TempDir::new("schema-missing");
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "Howie"]);
    git(&root, &["config", "user.email", "howie@example.com"]);

    let init = spectra().arg("init").current_dir(&*root).output().unwrap();
    assert!(init.status.success(), "init failed: {init:?}");
    std::fs::write(
        root.join("openspec").join("config.yaml"),
        "schema: nosuch\n",
    )
    .unwrap();

    let new_change = spectra()
        .args(["new", "change", "c1"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(
        new_change.status.success(),
        "new change failed: {new_change:?}"
    );

    let output = spectra()
        .args(["status", "--change", "c1", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Schema not found"), "{stderr}");
}

#[test]
fn explicit_schema_flag_overrides_config_yaml() {
    let root = TempDir::new("schema-flag-override");
    init_project_with_custom_schema(&root);

    // --schema spec-driven should use the built-in, ignoring both the
    // change's own recorded `mycustom` schema and config.yaml.
    let output = spectra()
        .args([
            "status",
            "--change",
            "test-change",
            "--schema",
            "spec-driven",
            "--json",
        ])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schemaName"], "spec-driven");
    assert_eq!(json["artifacts"].as_array().unwrap().len(), 4);
}
