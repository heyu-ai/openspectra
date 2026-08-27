mod common;

use std::path::Path;

use common::{spectra, TempDir};

/// Absolute path to a captured oracle golden (relative to the CLI crate's
/// manifest dir), used to pin `spectra schemas` output byte-for-byte.
fn golden(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/reverse-engineering/golden")
        .join(name);
    std::fs::read_to_string(&path).unwrap()
}

#[test]
fn schemas_text_matches_the_oracle_golden() {
    // No project needed: the oracle lists the built-in registry outside an
    // initialized project, so `schemas` never runs `require_initialized`.
    let root = TempDir::new("schemas-text");

    let out = spectra()
        .args(["schemas", "--no-color"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert!(out.status.success(), "schemas failed: {out:?}");
    assert!(out.stderr.is_empty(), "unexpected stderr: {out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        golden("schemas-2.3.1.txt")
    );
}

#[test]
fn schemas_json_matches_the_oracle_golden() {
    let root = TempDir::new("schemas-json");

    let out = spectra()
        .args(["schemas", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert!(out.status.success(), "schemas failed: {out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        golden("schemas-2.3.1.json")
    );
}

#[test]
fn schemas_works_without_an_initialized_project() {
    // Regression pin for the deliberate skip of `require_initialized`: a bare
    // directory (no `.spectra.yaml`) must still list schemas and exit 0.
    let root = TempDir::new("schemas-uninitialized");

    let out = spectra()
        .arg("schemas")
        .current_dir(&*root)
        .output()
        .unwrap();

    assert!(out.status.success(), "schemas failed: {out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("spec-driven"),
        "expected spec-driven in output, got: {stdout}"
    );
}

#[test]
fn schemas_lists_project_schemas_alongside_the_builtin() {
    let root = TempDir::new("schemas-project-listing");
    common::git(&root, &["init", "-q"]);
    common::git(&root, &["config", "user.name", "Test"]);
    common::git(&root, &["config", "user.email", "test@test.com"]);
    let init = spectra().arg("init").current_dir(&*root).output().unwrap();
    assert!(init.status.success(), "init failed: {init:?}");

    let schema_dir = root.join("openspec/schemas/mycustom");
    std::fs::create_dir_all(schema_dir.join("templates")).unwrap();
    std::fs::write(
        schema_dir.join("schema.yaml"),
        "name: Display Name\ndescription: Hidden desc\nartifacts:\n- id: proposal\n  generates: proposal.md\n  description: p\n  template: proposal.md\n  instruction: x\n  requires: []\napply:\n  requires: [proposal]\n  instruction: y\n",
    )
    .unwrap();

    let text_out = spectra()
        .args(["schemas", "--no-color"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(text_out.status.success(), "schemas failed: {text_out:?}");
    let text = String::from_utf8(text_out.stdout).unwrap();
    assert!(
        text.contains("spec-driven (package)"),
        "missing built-in: {text}"
    );
    assert!(
        text.contains("mycustom (project)"),
        "missing project schema: {text}"
    );
    assert!(
        !text.contains("mycustom (project) —"),
        "project schema should have no description suffix: {text}"
    );
}

#[test]
fn schemas_json_lists_project_schemas_with_null_description() {
    let root = TempDir::new("schemas-project-json");
    common::git(&root, &["init", "-q"]);
    common::git(&root, &["config", "user.name", "Test"]);
    common::git(&root, &["config", "user.email", "test@test.com"]);
    let init = spectra().arg("init").current_dir(&*root).output().unwrap();
    assert!(init.status.success(), "init failed: {init:?}");

    let schema_dir = root.join("openspec/schemas/mycustom");
    std::fs::create_dir_all(schema_dir.join("templates")).unwrap();
    std::fs::write(
        schema_dir.join("schema.yaml"),
        "name: Display Name\nartifacts:\n- id: proposal\n  generates: proposal.md\n  description: p\n  template: proposal.md\n  instruction: x\n  requires: []\napply:\n  requires: [proposal]\n  instruction: y\n",
    )
    .unwrap();

    let json_out = spectra()
        .args(["schemas", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(
        json_out.status.success(),
        "schemas --json failed: {json_out:?}"
    );
    let json: serde_json::Value = serde_json::from_slice(&json_out.stdout).unwrap();
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 2, "expected 2 schemas: {json}");
    assert_eq!(arr[1]["name"], "mycustom");
    assert_eq!(arr[1]["source"], "project");
    assert_eq!(arr[1]["description"], serde_json::Value::Null);
    assert_eq!(arr[1]["artifacts"], serde_json::json!(["proposal"]));
}
