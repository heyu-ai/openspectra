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
        yaml.lines().any(|line| line == "name: mycustom"),
        "fork target identity was not written: {yaml}"
    );

    let loaded = ResolvedSchema::load(&schema_dir, "mycustom").unwrap();
    assert_eq!(loaded.name, "mycustom");
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
fn schema_init_force_with_unknown_artifact_preserves_existing_target() {
    let root = TempDir::new("schema-init-force-unknown");
    init_project(&root);
    let target = root.join("openspec/schemas/team-flow");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("sentinel.txt"), "keep me\n").unwrap();

    let output = spectra()
        .args([
            "schema",
            "init",
            "team-flow",
            "--artifacts",
            "unknown",
            "--force",
        ])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("unknown artifact ID 'unknown'"));
    assert_eq!(
        std::fs::read_to_string(target.join("sentinel.txt")).unwrap(),
        "keep me\n"
    );
}

#[test]
#[cfg(unix)]
fn schema_init_config_failure_restores_existing_target_and_config() {
    use std::os::unix::fs::PermissionsExt;

    let root = TempDir::new("schema-init-config-rollback");
    init_project(&root);
    let target = root.join("openspec/schemas/team-flow");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("sentinel.txt"), "keep me\n").unwrap();
    let config_path = root.join("openspec/config.yaml");
    let original_config = std::fs::read_to_string(&config_path).unwrap();
    let spec_dir = root.join("openspec");
    std::fs::set_permissions(&spec_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let output = spectra()
        .args(["schema", "init", "team-flow", "--default", "--force"])
        .current_dir(&*root)
        .output()
        .unwrap();

    std::fs::set_permissions(&spec_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        std::fs::read_to_string(target.join("sentinel.txt")).unwrap(),
        "keep me\n"
    );
    assert_eq!(
        std::fs::read_to_string(config_path).unwrap(),
        original_config
    );
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
fn fork_copies_a_project_schema_tree_and_updates_its_identity() {
    let root = TempDir::new("schema-fork-project");
    init_project(&root);

    let source_dir = root.join("openspec/schemas/original");
    std::fs::create_dir_all(source_dir.join("templates")).unwrap();
    std::fs::write(
        source_dir.join("schema.yaml"),
        "# preserved comment\nname: Project Source\nversion: 1\ndescription: 專案 schema\nartifacts:\n- id: proposal\n  generates: proposal.md\n  description: 提案\n  template: proposal.md\n  instruction: |\n    撰寫提案。\n  requires: []\napply:\n  requires: [proposal]\n  instruction: 套用提案。\n",
    )
    .unwrap();
    std::fs::write(source_dir.join("templates/proposal.md"), "## 專案範本\n").unwrap();
    std::fs::create_dir_all(source_dir.join("notes/nested")).unwrap();
    std::fs::write(source_dir.join("notes/nested/readme.txt"), "keep me\n").unwrap();

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
    assert_eq!(copied.name, "copied");
    assert_eq!(copied.description, "專案 schema");
    assert_eq!(copied.artifacts[0].template, "## 專案範本\n");
    let copied_yaml =
        std::fs::read_to_string(root.join("openspec/schemas/copied/schema.yaml")).unwrap();
    assert!(copied_yaml.starts_with("# preserved comment\nname: copied\n"));
    assert!(copied_yaml.contains("instruction: |\n    撰寫提案。"));
    assert_eq!(
        std::fs::read_to_string(root.join("openspec/schemas/copied/notes/nested/readme.txt"))
            .unwrap(),
        "keep me\n"
    );
}

#[test]
fn fork_rewrites_only_the_top_level_name_and_preserves_yaml_formatting() {
    let root = TempDir::new("schema-fork-name-rewrite");
    init_project(&root);
    let source_dir = root.join("openspec/schemas/original");
    std::fs::create_dir_all(source_dir.join("templates")).unwrap();
    let source = concat!(
        "# header\r\n",
        "description: |\r\n",
        "  name: block scalar content\r\n",
        "name: Project Source  # identity\r\n",
        "version: 1\r\n",
        "artifacts:\r\n",
        "- id: proposal\r\n",
        "  generates: proposal.md\r\n",
        "  description: proposal\r\n",
        "  template: proposal.md\r\n",
        "  instruction: write\r\n",
        "  requires: []\r\n",
        "apply:\r\n",
        "  requires: [proposal]\r\n",
        "  instruction: apply\r\n",
    );
    std::fs::write(source_dir.join("schema.yaml"), source).unwrap();
    std::fs::write(source_dir.join("templates/proposal.md"), "# Proposal\r\n").unwrap();

    let output = spectra()
        .args(["schema", "fork", "original", "copied"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert!(output.status.success(), "fork failed: {output:?}");
    let copied = std::fs::read_to_string(root.join("openspec/schemas/copied/schema.yaml")).unwrap();
    assert_eq!(
        copied,
        source.replacen(
            "name: Project Source  # identity\r\n",
            "name: copied  # identity\r\n",
            1
        )
    );
}

#[test]
fn schema_init_validate_and_which_form_a_complete_management_flow() {
    let root = TempDir::new("schema-management");
    init_project(&root);

    let initialized = spectra()
        .args([
            "schema",
            "init",
            "team-flow",
            "--description",
            "Team workflow",
            "--artifacts",
            "proposal,tasks",
            "--default",
            "--json",
        ])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(initialized.status.success(), "{initialized:?}");
    let created: serde_json::Value = serde_json::from_slice(&initialized.stdout).unwrap();
    assert_eq!(created["schema"], "team-flow");

    let validated = spectra()
        .args(["schema", "validate", "team-flow", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(validated.status.success(), "{validated:?}");
    let checks: serde_json::Value = serde_json::from_slice(&validated.stdout).unwrap();
    assert_eq!(checks[0]["valid"], true);

    let resolved = spectra()
        .args(["schema", "which", "team-flow", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(resolved.status.success(), "{resolved:?}");
    let resolution: serde_json::Value = serde_json::from_slice(&resolved.stdout).unwrap();
    assert_eq!(resolution[0]["source"], "project");

    let config = std::fs::read_to_string(root.join("openspec/config.yaml")).unwrap();
    assert!(config.contains("schema: team-flow"));
}
