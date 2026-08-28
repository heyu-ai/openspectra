//! `spectra schema fork` 的端對端整合測試。

mod common;

use std::path::Path;

use common::{spectra, TempDir};
use spectra_core::schema::{ResolvedSchema, SchemaSource};

fn init_project(root: &Path) {
    let output = spectra().arg("init").current_dir(root).output().unwrap();
    assert!(
        output.status.success(),
        "初始化失敗：{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn fork_builtin_creates_loadable_schema_and_lists_it() {
    let root = TempDir::new("schema-fork-builtin");
    init_project(&root);

    let output = spectra()
        .args(["schema", "fork", "spec-driven", "mycustom"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "fork 失敗：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "\u{2713} Forked 'spec-driven' \u{2192} 'mycustom'\n"
    );
    assert!(output.stderr.is_empty());

    let schema_dir = root.join("openspec/schemas/mycustom");
    let schema_yaml = schema_dir.join("schema.yaml");
    assert!(schema_yaml.is_file());
    for template in ["proposal.md", "spec.md", "design.md", "tasks.md"] {
        assert!(
            schema_dir.join("templates").join(template).is_file(),
            "缺少範本：{template}"
        );
    }

    let yaml = std::fs::read_to_string(schema_yaml).unwrap();
    assert!(
        yaml.lines().any(|line| line == "name: spec-driven"),
        "來源名稱未保留：{yaml}"
    );

    let loaded = ResolvedSchema::load(&schema_dir, "mycustom").unwrap();
    assert_eq!(loaded.name, "spec-driven");
    assert_eq!(loaded.source, SchemaSource::Project);
    assert_eq!(loaded.artifacts.len(), 4);
    assert!(loaded
        .artifacts
        .iter()
        .all(|artifact| !artifact.template.is_empty()));

    let listing = spectra()
        .args(["schemas", "--no-color"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(listing.status.success(), "schemas 失敗：{listing:?}");
    assert!(String::from_utf8(listing.stdout)
        .unwrap()
        .contains("mycustom (project)"));
}

#[test]
fn fork_defaults_the_target_name_and_accepts_inert_json_flag() {
    let root = TempDir::new("schema-fork-default");
    init_project(&root);

    let output = spectra()
        .args(["schema", "fork", "spec-driven", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert!(output.status.success(), "fork 失敗：{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "\u{2713} Forked 'spec-driven' \u{2192} 'spec-driven-custom'\n"
    );
    assert!(root
        .join("openspec/schemas/spec-driven-custom/schema.yaml")
        .is_file());
}

#[test]
fn fork_rejects_existing_target_unless_force_is_used() {
    let root = TempDir::new("schema-fork-force");
    init_project(&root);

    let first = spectra()
        .args(["schema", "fork", "spec-driven", "mycustom"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(first.status.success(), "首次 fork 失敗：{first:?}");

    let duplicate = spectra()
        .args(["schema", "fork", "spec-driven", "mycustom"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert_eq!(duplicate.status.code(), Some(1));
    assert!(duplicate.stdout.is_empty());
    assert_eq!(
        String::from_utf8(duplicate.stderr).unwrap(),
        "Error: Schema 'mycustom' already exists. Use --force to overwrite.\n"
    );

    let schema_dir = root.join("openspec/schemas/mycustom");
    std::fs::write(schema_dir.join("schema.yaml"), "已損毀\n").unwrap();
    std::fs::write(schema_dir.join("templates/proposal.md"), "已損毀的範本\n").unwrap();

    let forced = spectra()
        .args(["schema", "fork", "spec-driven", "mycustom", "--force"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(forced.status.success(), "強制 fork 失敗：{forced:?}");
    assert_eq!(
        String::from_utf8(forced.stdout).unwrap(),
        "\u{2713} Forked 'spec-driven' \u{2192} 'mycustom'\n"
    );
    let loaded = ResolvedSchema::load(&schema_dir, "mycustom").unwrap();
    assert_ne!(loaded.artifacts[0].template, "已損毀的範本\n");
}

#[test]
fn fork_reports_missing_source_and_requires_initialized_project() {
    let root = TempDir::new("schema-fork-errors");

    let uninitialized = spectra()
        .args(["schema", "fork", "spec-driven", "mycustom"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert_eq!(uninitialized.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(uninitialized.stderr).unwrap(),
        "Error: Not initialized. Run 'spectra init' first.\n"
    );

    init_project(&root);
    let missing = spectra()
        .args(["schema", "fork", "nosuch", "mycopy"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());
    assert_eq!(
        String::from_utf8(missing.stderr).unwrap(),
        "Error: Schema not found: Schema 'nosuch' not found in project, user, or built-in locations\n"
    );
}

#[test]
fn fork_can_copy_a_project_schema_without_rewriting_its_name() {
    let root = TempDir::new("schema-fork-project");
    init_project(&root);

    let source_dir = root.join("openspec/schemas/original");
    std::fs::create_dir_all(source_dir.join("templates")).unwrap();
    std::fs::write(
        source_dir.join("schema.yaml"),
        "name: Project Source\nversion: 1\ndescription: 專案 schema\nartifacts:\n- id: proposal\n  generates: proposal.md\n  description: 提案\n  template: proposal.md\n  instruction: 撰寫提案。\n  requires: []\napply:\n  requires: [proposal]\n  instruction: 套用提案。\n",
    )
    .unwrap();
    std::fs::write(source_dir.join("templates/proposal.md"), "## 專案範本\n").unwrap();

    let output = spectra()
        .args(["schema", "fork", "original", "copied"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(output.status.success(), "fork 失敗：{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "\u{2713} Forked 'original' \u{2192} 'copied'\n"
    );

    let copied = ResolvedSchema::load(&root.join("openspec/schemas/copied"), "copied").unwrap();
    assert_eq!(copied.name, "Project Source");
    assert_eq!(copied.description, "專案 schema");
    assert_eq!(copied.artifacts[0].template, "## 專案範本\n");
}
