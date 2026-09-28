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
        "Error: Not initialized. Run 'spectra init' to initialize.\n"
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
    // oracle 3.0.0 的形狀：{artifactCount, name, valid}。
    assert_eq!(
        String::from_utf8(validated.stdout).unwrap(),
        "{\n  \"artifactCount\": 2,\n  \"name\": \"team-flow\",\n  \"valid\": true\n}\n"
    );

    let resolved = spectra()
        .args(["schema", "which", "team-flow", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(resolved.status.success(), "{resolved:?}");
    let resolution: serde_json::Value = serde_json::from_slice(&resolved.stdout).unwrap();
    // oracle 3.0.0 的形狀：{name, resolved, sources: [{path, source}]}。
    assert_eq!(resolution["name"], "team-flow");
    assert_eq!(resolution["resolved"], "project");
    assert_eq!(resolution["sources"][0]["source"], "project");
    assert!(resolution["sources"][0]["path"]
        .as_str()
        .unwrap()
        .ends_with("openspec/schemas/team-flow/schema.yaml"));

    let config = std::fs::read_to_string(root.join("openspec/config.yaml")).unwrap();
    assert!(config.contains("schema: team-flow"));
}

#[test]
fn schema_fork_rejects_reserved_transaction_names() {
    let root = TempDir::new("schema-private-name");
    init_project(&root);

    let output = spectra()
        .args(["schema", "fork", "spec-driven", ".team.stage-123-0"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("reserved transaction name"));
    assert!(!root.join("openspec/schemas/.team.stage-123-0").exists());
}

/// 最小的專案 schema：沒有 `apply`、artifact 沒有 `instruction`（oracle 3.0.0 接受，
/// W7g 探測 p15；D11-5 要求 fork 也接受）。
const MINIMAL_SCHEMA: &str = "# keep this comment\nname: m\nversion: 1\nartifacts:\n  - id: a\n    generates: a.md\n    description: A\n    template: a.md\n  - id: b\n    generates: b.md\n    description: B\n    template: b.md\n    requires: [a]\n";

fn write_project_schema(root: &Path, name: &str, yaml: &str, templates: &[(&str, &str)]) {
    let dir = root.join("openspec/schemas").join(name);
    std::fs::create_dir_all(dir.join("templates")).unwrap();
    std::fs::write(dir.join("schema.yaml"), yaml).unwrap();
    for (file, content) in templates {
        std::fs::write(dir.join("templates").join(file), content).unwrap();
    }
}

fn fork_output(root: &Path, args: &[&str]) -> std::process::Output {
    spectra()
        .args(["schema", "fork"])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

/// D11-5：必填欄位跟 oracle 放寬，但複製維持原樣——註解保留，只改寫 `name:`，
/// 不像 oracle 那樣重新序列化成 `apply: null`、`instruction: null`。
#[test]
fn fork_accepts_a_minimal_project_schema_and_copies_it_verbatim() {
    let root = TempDir::new("schema-fork-minimal");
    init_project(&root);
    write_project_schema(
        &root,
        "m",
        MINIMAL_SCHEMA,
        &[("a.md", "# A\n"), ("b.md", "# B\n")],
    );

    let output = fork_output(&root, &["m", "m2"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "\u{2713} Forked 'm' \u{2192} 'm2'\n"
    );
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    assert_eq!(
        std::fs::read_to_string(root.join("openspec/schemas/m2/schema.yaml")).unwrap(),
        MINIMAL_SCHEMA.replacen("name: m\n", "name: m2\n", 1)
    );
}

/// D11-2：缺檔的 template 略過、空檔照樣複製，兩者都在 stderr 警告，exit 0
/// （oracle 3.0.0 同樣略過／複製，但完全無聲，W7g 探測 p13）。
#[test]
fn fork_skips_missing_templates_copies_empty_ones_and_warns() {
    let root = TempDir::new("schema-fork-template-warnings");
    init_project(&root);
    write_project_schema(&root, "m", MINIMAL_SCHEMA, &[("b.md", "")]);

    let output = fork_output(&root, &["m", "m2"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "\u{2713} Forked 'm' \u{2192} 'm2'\n"
    );
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "Warning: Template 'a.md' for artifact 'a' is missing\n\
         Warning: Template 'b.md' for artifact 'b' is empty\n"
    );
    let templates = root.join("openspec/schemas/m2/templates");
    assert_eq!(entries(&templates), vec!["b.md"]);
    assert_eq!(std::fs::read(templates.join("b.md")).unwrap(), b"");
}

/// D11-4：`--force` 以原子替換取代整個目錄，舊目錄多出的檔案不會殘留（oracle 3.0.0
/// 就地覆寫、舊檔保留，W7g 探測 p10 A）。
#[test]
fn fork_force_replaces_the_whole_target_directory() {
    let root = TempDir::new("schema-fork-force-replace");
    init_project(&root);
    let target = root.join("openspec/schemas/t");
    std::fs::create_dir_all(target.join("templates")).unwrap();
    std::fs::write(target.join("EXTRA.txt"), "stale\n").unwrap();
    std::fs::write(target.join("templates/old.md"), "stale\n").unwrap();

    let output = fork_output(&root, &["no-spec", "t", "--force"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(entries(&target), vec!["schema.yaml", "templates"]);
    assert_eq!(
        entries(&target.join("templates")),
        vec!["design.md", "proposal.md", "tasks.md"]
    );
    assert_eq!(entries(&root.join("openspec/schemas")), vec!["t"]);
}

/// D11-4：目標是一般檔案時 `--force` 同樣替換成 schema 目錄（oracle 3.0.0：
/// `File exists (os error 17)`，W7g 探測 q02），而且不留下 `.tf.backup-*`。
#[test]
fn fork_force_replaces_a_regular_file_target_without_leaking_a_backup() {
    let root = TempDir::new("schema-fork-force-file");
    init_project(&root);
    let schemas = root.join("openspec/schemas");
    std::fs::create_dir_all(&schemas).unwrap();
    std::fs::write(schemas.join("tf"), "x\n").unwrap();

    let output = fork_output(&root, &["no-spec", "tf", "--force"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(schemas.join("tf/schema.yaml").is_file());
    assert_eq!(entries(&schemas), vec!["tf"]);
}

/// D11-3：只接受 `schemas/` 底下一層的目錄名稱。oracle 3.0.0 不檢查：`../esc` 寫到
/// `schemas/` 外、`""` 與 `.` 指向 `schemas/` 本身（W7g 探測 p10 E、p15、q02）。
/// 被拒絕時什麼都不寫，也不留下暫存目錄。
#[test]
fn fork_rejects_targets_outside_the_schemas_directory() {
    let root = TempDir::new("schema-fork-target-names");
    init_project(&root);
    let schemas = root.join("openspec/schemas");
    std::fs::create_dir_all(&schemas).unwrap();

    for (target, error) in [
        (
            "",
            "Error: schema fork target '' must name a directory inside schemas/\n",
        ),
        (
            ".",
            "Error: schema fork target '.' must name a directory inside schemas/\n",
        ),
        (
            "../esc",
            "Error: schema fork target '../esc' must not contain '..'\n",
        ),
        (
            "a/b",
            "Error: schema fork target 'a/b' must not contain path separators\n",
        ),
    ] {
        let output = fork_output(&root, &["no-spec", target]);
        assert_eq!(output.status.code(), Some(1), "{target:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{target:?}");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            error,
            "{target:?}"
        );
        assert!(
            entries(&schemas).is_empty(),
            "{target:?}: {:?}",
            entries(&schemas)
        );
        assert!(!root.join("openspec/esc").exists());
    }
}

/// oracle 3.0.0（W7g 探測 p10 D）：來源 schema 無效時在寫入任何東西之前失敗。
#[test]
fn fork_of_an_invalid_source_writes_nothing() {
    let root = TempDir::new("schema-fork-invalid-source");
    init_project(&root);
    write_project_schema(
        &root,
        "m",
        "name: m\nversion: 1\nartifacts:\n  - id: a\n    generates: a.md\n    description: A\n    template: a.md\n    requires: [zzz]\n",
        &[("a.md", "# A\n")],
    );

    let output = fork_output(&root, &["m", "m2"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "Error: Invalid schema: Artifact 'a' requires unknown artifact 'zzz'\n"
    );
    assert_eq!(entries(&root.join("openspec/schemas")), vec!["m"]);
}
