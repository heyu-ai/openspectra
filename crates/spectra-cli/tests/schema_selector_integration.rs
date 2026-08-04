//! Schema-selector resolution through the real CLI (#117).
//!
//! These cases exist because the unit tests in `schema.rs` call
//! `require_supported` on a bare `Config` and never through a project with a
//! change that has its own `.openspec.yaml`. That gap let a whole missing
//! resolution layer ship: an earlier revision read only
//! `<spec_dir>/config.yaml` and hard-failed changes the oracle runs fine.
//!
//! Every expectation below was probed against the closed-source v2.3.1 oracle.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn spectra() -> Command {
    Command::new(env!("CARGO_BIN_EXE_spectra"))
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "spectra-schemasel-it-{label}-{}-{seq}",
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

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A project with one change `c1`. `change_schema` becomes the change's own
/// `.openspec.yaml` `schema:` (omitted entirely when `None`); `project_schema`
/// becomes `<spec_dir>/config.yaml`'s.
fn project(label: &str, change_schema: Option<&str>, project_schema: &str) -> TempDir {
    let tmp = TempDir::new(label);
    write(&tmp.join(".spectra.yaml"), "spec_dir: openspec\n");
    write(
        &tmp.join("openspec/config.yaml"),
        &format!("schema: {project_schema}\n"),
    );
    let cd = tmp.join("openspec/changes/c1");
    write(&cd.join("proposal.md"), "## Why\nbecause\n");
    write(&cd.join("tasks.md"), "- [ ] 1.1 do it\n");
    let metadata = match change_schema {
        Some(schema) => format!("schema: {schema}\ncreated: 2026-08-01\n"),
        None => "created: 2026-08-01\n".to_string(),
    };
    write(&cd.join(".openspec.yaml"), &metadata);
    tmp
}

fn run(tmp: &Path, args: &[&str]) -> Output {
    spectra()
        .args(args)
        .current_dir(tmp)
        .output()
        .expect("spectra runs")
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn a_change_recording_the_builtin_schema_ignores_an_unknown_project_schema() {
    // The regression this file exists for. `spectra new change` stamps
    // `schema: spec-driven` into every change, so reading only config.yaml made
    // openspectra refuse work the oracle performs: probed, the oracle exits 0
    // and prints `Schema: spec-driven` for exactly this layout.
    let tmp = project("change-wins", Some("spec-driven"), "no-such-schema");

    for args in [
        vec!["status", "--change", "c1"],
        vec!["instructions", "tasks", "--change", "c1"],
    ] {
        let out = run(&tmp, &args);
        assert!(
            out.status.success(),
            "{args:?} must succeed when the change records the built-in schema, \
             got {:?}: {}",
            out.status.code(),
            stderr(&out)
        );
    }
}

#[test]
fn a_change_recording_an_unknown_schema_fails_even_with_a_healthy_project_config() {
    let tmp = project("change-blocks", Some("no-such-schema"), "spec-driven");

    let out = run(&tmp, &["status", "--change", "c1"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains(
            "Schema not found: Schema 'no-such-schema' not found in project, user, \
             or built-in locations"
        ),
        "{}",
        stderr(&out)
    );
}

#[test]
fn the_project_config_selects_only_when_the_change_records_no_schema() {
    let tmp = project("config-fallback", None, "no-such-schema");

    let out = run(&tmp, &["status", "--change", "c1"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("Schema not found: Schema 'no-such-schema'"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn an_explicit_flag_overrides_the_changes_own_schema() {
    let tmp = project("flag-wins", Some("no-such-schema"), "no-such-schema");

    let out = run(
        &tmp,
        &["status", "--change", "c1", "--schema", "spec-driven"],
    );
    assert!(
        out.status.success(),
        "--schema spec-driven must override both lower layers: {}",
        stderr(&out)
    );
}

#[test]
fn a_missing_change_is_reported_before_the_schema_error() {
    // Probed: the oracle emits `Change 'ghost' not found.` even when the
    // selector is also unresolvable, so the gate must run after change loading.
    let tmp = project("order", Some("no-such-schema"), "no-such-schema");

    let out = run(&tmp, &["status", "--change", "ghost"]);
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(err.contains("Change 'ghost' not found."), "{err}");
    assert!(!err.contains("Schema not found"), "{err}");
}

#[test]
fn a_present_custom_schema_names_its_file_rather_than_claiming_it_is_missing() {
    let tmp = project("present-custom", Some("mycustom"), "spec-driven");
    let definition = tmp.join("openspec/schemas/mycustom/schema.yaml");
    write(&definition, "name: mycustom\nartifacts: []\n");

    let out = run(&tmp, &["status", "--change", "c1"]);
    assert_eq!(out.status.code(), Some(1));
    let err = stderr(&out);
    assert!(err.contains(&definition.display().to_string()), "{err}");
    assert!(err.contains("#126"), "{err}");
    assert!(
        !err.contains("not found in project"),
        "the schema is in the project, so the oracle's not-found wording would \
         be a false statement: {err}"
    );
}

#[test]
fn new_artifact_is_not_gated_by_the_schema_selector() {
    // Probed: with the change recording an unresolvable schema, the oracle's
    // `new artifact design` still exits 0 and writes the built-in template --
    // `new artifact` resolves no schema and has no `--schema` flag. An earlier
    // revision of #117 gated it, which both diverged from the oracle and
    // reordered this command's probed check sequence.
    let tmp = project("artifact-ungated", Some("no-such-schema"), "no-such-schema");

    let out = run(&tmp, &["new", "artifact", "design", "--change", "c1"]);
    assert!(
        out.status.success(),
        "new artifact must not be schema-gated, got {:?}: {}",
        out.status.code(),
        stderr(&out)
    );
    assert!(tmp.join("openspec/changes/c1/design.md").is_file());
}
