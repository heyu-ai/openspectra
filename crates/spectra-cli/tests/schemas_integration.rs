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
        golden("schemas-3.0.0.txt")
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
        golden("schemas-3.0.0.json")
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
    let init = spectra()
        .args(["init", "--dir", "openspec"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(init.status.success(), "init failed: {init:?}");

    let schema_dir = root.join("openspec/schemas/mycustom");
    std::fs::create_dir_all(schema_dir.join("templates")).unwrap();
    std::fs::write(
        schema_dir.join("schema.yaml"),
        "name: Display Name\nversion: 1\ndescription: Hidden desc\nartifacts:\n- id: proposal\n  generates: proposal.md\n  description: p\n  template: proposal.md\n  instruction: x\n  requires: []\napply:\n  requires: [proposal]\n  instruction: y\n",
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
    let init = spectra()
        .args(["init", "--dir", "openspec"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(init.status.success(), "init failed: {init:?}");

    let schema_dir = root.join("openspec/schemas/mycustom");
    std::fs::create_dir_all(schema_dir.join("templates")).unwrap();
    std::fs::write(
        schema_dir.join("schema.yaml"),
        "name: Display Name\nversion: 1\nartifacts:\n- id: proposal\n  generates: proposal.md\n  description: p\n  template: proposal.md\n  instruction: x\n  requires: []\napply:\n  requires: [proposal]\n  instruction: y\n",
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
    assert_eq!(
        arr.len(),
        3,
        "expected 2 built-ins + 1 project schema: {json}"
    );
    assert_eq!(arr[2]["name"], "mycustom");
    assert_eq!(arr[2]["source"], "project");
    assert_eq!(arr[2]["description"], serde_json::Value::Null);
    assert_eq!(arr[2]["artifacts"], serde_json::json!(["proposal"]));
}

/// 在 `root/p` 建立最小的已初始化專案。
fn isolated_project(root: &Path) -> std::path::PathBuf {
    let project = root.join("p");
    std::fs::create_dir_all(project.join("openspec/changes/archive")).unwrap();
    std::fs::create_dir_all(project.join("openspec/specs")).unwrap();
    std::fs::write(project.join(".spectra.yaml"), "spec_dir: openspec\n").unwrap();
    std::fs::write(
        project.join("openspec/config.yaml"),
        "schema: spec-driven\n",
    )
    .unwrap();
    project
}

/// `HOME`／`XDG_DATA_HOME` 指向 `root` 底下，開發者自己的使用者層 schema 不會混進輸出。
fn schemas_isolated(root: &Path, project: &Path, args: &[&str]) -> std::process::Output {
    spectra()
        .args(args)
        .current_dir(project)
        .env("HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("xdg"))
        .output()
        .unwrap()
}

fn write_schema(dir: &Path, yaml: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("schema.yaml"), yaml).unwrap();
}

/// #226：oracle 3.0.0 只要 `schema.yaml` 存在就列出專案 schema，即使它載入失敗
/// （缺 `version`、YAML 壞掉、`schema.yaml` 是目錄）；`artifacts` 是從檔案寬鬆
/// 讀出的字串 `id`，讀不出來就是 `[]`。期望值是 oracle 3.0.0 對同一組 schema 的
/// 輸出（見 `docs/reverse-engineering/schemas.md`「Schemas that fail to load」），
/// 只有列出順序不同：oracle 依 readdir 順序，OpenSpectra 依名稱排序。
#[test]
fn schemas_lists_project_schemas_that_fail_to_load() {
    let root = TempDir::new("schemas-load-failures");
    let project = isolated_project(&root);
    let schemas = project.join("openspec/schemas");
    write_schema(
        &schemas.join("no-version"),
        "name: no-version\nartifacts:\n  - id: a\n    generates: a.md\n    description: A\n    template: a.md\n    instruction: do a\n    requires: []\n  - id: b\n    generates: b.md\n    description: B\n    template: a.md\n    instruction: do b\n    requires: [a]\napply:\n  requires: [b]\n  instruction: go\n",
    );
    write_schema(
        &schemas.join("broken-yaml"),
        "name: broken-yaml\nartifacts: [\n",
    );
    write_schema(
        &schemas.join("odd-entries"),
        "artifacts:\n  - id: 1\n  - id: true\n  - plain\n  - id: '01'\n  - description: no id\n  - id: b\n  - id: b\n",
    );
    write_schema(
        &schemas.join("valid"),
        "name: valid\nversion: 1\nartifacts:\n  - id: a\n    generates: a.md\n    description: A\n    template: a.md\n    instruction: do a\n    requires: []\napply:\n  requires: [a]\n  instruction: go\n",
    );
    std::fs::create_dir_all(schemas.join("yaml-is-dir/schema.yaml")).unwrap();
    std::fs::create_dir_all(schemas.join("no-schema-yaml")).unwrap();

    let text = schemas_isolated(&root, &project, &["schemas", "--no-color"]);
    assert!(text.status.success(), "schemas failed: {text:?}");
    assert_eq!(String::from_utf8(text.stderr).unwrap(), "");
    assert_eq!(
        String::from_utf8(text.stdout).unwrap(),
        "Available schemas:\n  spec-driven (package) — Default OpenSpec workflow - proposal → specs → tasks (design optional)\n  no-spec (package) — No-spec workflow - proposal -> tasks (design optional)\n  broken-yaml (project)\n  no-version (project)\n  odd-entries (project)\n  valid (project)\n  yaml-is-dir (project)\n"
    );

    let json = schemas_isolated(&root, &project, &["schemas", "--json"]);
    assert!(json.status.success(), "schemas --json failed: {json:?}");
    assert_eq!(String::from_utf8(json.stderr).unwrap(), "");
    let listed: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(
        listed.as_array().unwrap()[2..],
        serde_json::json!([
            {"artifacts": [], "description": null, "name": "broken-yaml", "source": "project"},
            {"artifacts": ["a", "b"], "description": null, "name": "no-version", "source": "project"},
            {"artifacts": ["01", "b", "b"], "description": null, "name": "odd-entries", "source": "project"},
            {"artifacts": ["a"], "description": null, "name": "valid", "source": "project"},
            {"artifacts": [], "description": null, "name": "yaml-is-dir", "source": "project"},
        ])
        .as_array()
        .unwrap()[..]
    );
}

/// #226：使用者層 schema 同樣寬鬆列出（oracle 3.0.0 以 `HOME` 指向臨時目錄探測
/// `~/Library/Application Support/openspec/schemas/u`；OpenSpectra 另外先看
/// `XDG_DATA_HOME`，這裡用它讓 Linux 與 macOS 走同一條路徑）。
#[test]
fn schemas_lists_user_schemas_that_fail_to_load() {
    let root = TempDir::new("schemas-user-load-failure");
    let project = isolated_project(&root);
    write_schema(&root.join("xdg/openspec/schemas/u"), "artifacts: [\n");

    let json = schemas_isolated(&root, &project, &["schemas", "--json"]);
    assert!(json.status.success(), "schemas --json failed: {json:?}");
    let listed: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(
        listed.as_array().unwrap()[2..],
        serde_json::json!([
            {"artifacts": [], "description": null, "name": "u", "source": "user"},
        ])
        .as_array()
        .unwrap()[..]
    );
}
