//! Schema-selector resolution through the real CLI (#117).
//!
//! These cases exist because the unit tests in `schema.rs` call
//! `require_supported` directly and never through a project with a real
//! `.openspec.yaml` on disk. That gap let a whole resolution layer ship
//! missing: an earlier revision read only `<spec_dir>/config.yaml` and
//! hard-failed changes the oracle runs fine.
//!
//! **Oracle-side expectations here were probed against v2.3.1; one
//! OpenSpectra-only divergence is marked as such inline** — the
//! built-in-template body of an ungated `new artifact` has no oracle
//! counterpart (see the Accepted Residual Risks in the PR and
//! `docs/reverse-engineering/schemas.md`). A present custom schema at
//! `<spec_dir>/schemas/<name>/schema.yaml` used to be rejected with a
//! `#126`-tagged error; #126 is now implemented (`schema::resolve_schema`),
//! so a well-formed custom schema loads and runs instead.

mod common;

use std::path::Path;
use std::process::Output;

use common::{git, spectra, TempDir};

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A project with one change `c1`. `change_schema` becomes the change's own
/// `.openspec.yaml` `schema:` (the key is omitted entirely when `None`);
/// `project_schema` becomes `<spec_dir>/config.yaml`'s.
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

/// Both gated commands, so a gate added to one and not the other cannot pass.
const GATED: [&[&str]; 2] = [
    &["status", "--change", "c1"],
    &["instructions", "tasks", "--change", "c1"],
];

#[test]
fn a_change_recording_the_builtin_schema_ignores_an_unknown_project_schema() {
    // The regression this file exists for. `spectra new change` stamps the
    // project's configured schema into every change, so reading only
    // config.yaml made openspectra refuse work the oracle performs: probed,
    // the oracle exits 0 and prints `Schema: spec-driven` for this layout.
    let tmp = project("change-wins", Some("spec-driven"), "no-such-schema");

    for args in GATED {
        let out = run(&tmp, args);
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

    for args in GATED {
        let out = run(&tmp, args);
        assert_eq!(out.status.code(), Some(1), "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains(
                "Schema not found: Schema 'no-such-schema' not found in project, user, \
                 or built-in locations"
            ),
            "{args:?}: {}",
            stderr(&out)
        );
    }
}

#[test]
fn the_project_config_selects_only_when_the_change_records_no_schema() {
    let tmp = project("config-fallback", None, "no-such-schema");

    for args in GATED {
        let out = run(&tmp, args);
        assert_eq!(out.status.code(), Some(1), "{args:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("Schema not found: Schema 'no-such-schema'"),
            "{args:?}: {}",
            stderr(&out)
        );
    }
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

/// #126: a present, well-formed custom schema now loads and runs instead of
/// being rejected, regardless of which layer (change / `--schema` / project
/// config) supplied its name. Supersedes the old
/// `the_rejection_message_names_the_layer_that_actually_selected_the_schema`
/// test, which pinned the pre-#126 reject-with-remedy behavior for a present
/// schema -- that behavior no longer exists now that `schema::resolve_schema`
/// actually loads it.
#[test]
fn a_present_custom_schema_loads_and_runs_regardless_of_the_selecting_layer() {
    let tmp = project("custom-schema-loads", Some("mycustom"), "spec-driven");
    let schema_dir = tmp.join("openspec/schemas/mycustom");
    write(
        &schema_dir.join("schema.yaml"),
        "name: mycustom\nversion: 1\nartifacts:\n- id: proposal\n  generates: proposal.md\n  description: A proposal\n  template: proposal.md\n  instruction: Write it.\n  requires: []\napply:\n  requires: [proposal]\n  tracks: tasks.md\n  instruction: Do it.\n",
    );
    write(&schema_dir.join("templates/proposal.md"), "## Template\n");

    // Change layer: the change's own `.openspec.yaml` names the schema.
    let out = run(&tmp, &["status", "--change", "c1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Schema: mycustom"), "{stdout}");

    // Explicit layer: --schema also loads it directly.
    let out = run(&tmp, &["status", "--change", "c1", "--schema", "mycustom"]);
    assert!(out.status.success(), "{}", stderr(&out));

    // Project layer: config.yaml selects it once the change records none.
    write(
        &tmp.join("openspec/changes/c1/.openspec.yaml"),
        "created: 2026-08-01\n",
    );
    write(&tmp.join("openspec/config.yaml"), "schema: mycustom\n");
    let out = run(&tmp, &["status", "--change", "c1"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Schema: mycustom"), "{stdout}");
}

#[test]
fn new_change_stamps_the_configured_schema_so_the_gate_stays_reachable() {
    // Hardcoding `spec-driven` here silently re-opened the fallback #117
    // closes: the change-level key outranks config.yaml, so every change
    // OpenSpectra created in a custom-schema project passed the gate.
    // Probed: the oracle writes `schema: mycustom` for this layout.
    let tmp = TempDir::new("new-change-stamp");
    git(&tmp, &["init", "-q"]);
    git(&tmp, &["config", "user.name", "Howie"]);
    git(&tmp, &["config", "user.email", "howie@example.com"]);
    assert!(spectra()
        .arg("init")
        .current_dir(&*tmp)
        .output()
        .unwrap()
        .status
        .success());
    write(&tmp.join("openspec/config.yaml"), "schema: mycustom\n");

    let out = run(&tmp, &["new", "change", "c9"]);
    assert!(out.status.success(), "{}", stderr(&out));

    let metadata = std::fs::read_to_string(tmp.join("openspec/changes/c9/.openspec.yaml")).unwrap();
    assert!(
        metadata.contains("schema: mycustom"),
        "new change must stamp the configured schema, got:\n{metadata}"
    );

    // ...and the gate now fires on that change instead of silently running the
    // built-in workflow.
    let out = run(&tmp, &["status", "--change", "c9"]);
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stderr(&out).contains("Schema not found: Schema 'mycustom'"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn new_artifact_is_not_gated_by_the_schema_selector() {
    // Probed: with the change recording an unresolvable schema, the oracle's
    // `new artifact design` still exits 0 -- the command never *errors* on the
    // selector and has no `--schema` flag. An earlier revision of #117 gated
    // it, which both diverged from the oracle and inserted a check ahead of
    // this command's probed sequence in artifact-workflow.md.
    //
    // Divergence, deliberate and asserted here so it cannot drift silently:
    // the oracle resolves the selector for *template lookup* and writes a
    // **0-byte** file when it cannot resolve it. OpenSpectra writes the
    // built-in template instead (a usable artifact rather than an empty one).
    let tmp = project("artifact-ungated", Some("no-such-schema"), "no-such-schema");

    let out = run(&tmp, &["new", "artifact", "design", "--change", "c1"]);
    assert!(
        out.status.success(),
        "new artifact must not be schema-gated, got {:?}: {}",
        out.status.code(),
        stderr(&out)
    );
    let written = std::fs::read_to_string(tmp.join("openspec/changes/c1/design.md")).unwrap();
    assert!(
        written.contains("## Context"),
        "OpenSpectra writes the built-in template where the oracle writes an \
         empty file; if this ever becomes empty, the divergence flipped: \
         {written:?}"
    );
}
