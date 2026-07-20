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
