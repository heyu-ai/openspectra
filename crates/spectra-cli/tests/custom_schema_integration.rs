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
fn new_artifact_uses_custom_schema_template() {
    let root = TempDir::new("new-artifact-custom-template");
    init_project_with_custom_schema(&root);

    let output = spectra()
        .args(["new", "artifact", "proposal", "--change", "test-change"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "new artifact failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let content =
        std::fs::read_to_string(root.join("openspec/changes/test-change/proposal.md")).unwrap();
    assert_eq!(
        content, "## Custom Proposal Template\n",
        "expected custom template content, got: {content}"
    );
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

/// 自訂 schema `ord`（探測 p33／p34）：artifact 宣告順序 x y a c b d，依賴 y←x、a←y、b←c、
/// d←x,a；`apply` 沒有 instruction。
fn init_project_with_order_schema(root: &Path) {
    git(root, &["init", "-q"]);
    let init = spectra().arg("init").current_dir(root).output().unwrap();
    assert!(init.status.success(), "init failed: {init:?}");
    let schema_dir = root.join("openspec").join("schemas").join("ord");
    std::fs::create_dir_all(schema_dir.join("templates")).unwrap();
    let mut yaml = String::from("name: ord\nversion: 1\ndescription: order probe\nartifacts:\n");
    for (id, deps) in [
        ("x", &[][..]),
        ("y", &["x"][..]),
        ("a", &["y"][..]),
        ("c", &[][..]),
        ("b", &["c"][..]),
        ("d", &["x", "a"][..]),
    ] {
        std::fs::write(
            schema_dir.join("templates").join(format!("{id}.md")),
            "# t\n",
        )
        .unwrap();
        yaml.push_str(&format!(
            "- id: {id}\n  generates: {id}.md\n  description: {id} artifact\n  template: {id}.md\n  instruction: Write {id}.\n  requires: [{}]\n",
            deps.join(", ")
        ));
    }
    yaml.push_str("apply:\n  requires: [a]\n");
    std::fs::write(schema_dir.join("schema.yaml"), yaml).unwrap();
    let change = root.join("openspec").join("changes").join("c");
    std::fs::create_dir_all(&change).unwrap();
    std::fs::write(
        change.join(".openspec.yaml"),
        "schema: ord\ncreated: 2026-09-01\n",
    )
    .unwrap();
}

/// oracle 3.0.0：`status` 的 artifact 順序是 Kahn 逐輪＋同輪字母序（`c x b y a d`），
/// `schemas` 列表維持宣告順序（探測 p34）。
#[test]
fn status_orders_custom_artifacts_like_the_oracle() {
    let dir = TempDir::new("custom-schema-order");
    init_project_with_order_schema(&dir);
    let out = spectra()
        .args(["status", "--change", "c", "--json"])
        .current_dir(&*dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let ids: Vec<&str> = v["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["c", "x", "b", "y", "a", "d"]);

    let listing = spectra()
        .args(["schemas", "--json"])
        .current_dir(&*dir)
        .output()
        .unwrap();
    let schemas: serde_json::Value = serde_json::from_slice(&listing.stdout).unwrap();
    let ord = schemas
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "ord")
        .unwrap();
    assert_eq!(
        ord["artifacts"],
        serde_json::json!(["x", "y", "a", "c", "b", "d"])
    );
}

/// oracle 3.0.0：`apply.instruction` 可省略——JSON 的 `instruction` 為 null，human 輸出在
/// `Progress` 行後結束（探測 p33）。
#[test]
fn apply_instruction_is_optional() {
    let dir = TempDir::new("custom-schema-no-apply-instruction");
    init_project_with_order_schema(&dir);
    std::fs::write(dir.join("openspec/changes/c/a.md"), "# a\n").unwrap();
    let json = spectra()
        .args(["instructions", "apply", "--change", "c", "--json"])
        .current_dir(&*dir)
        .output()
        .unwrap();
    assert!(json.status.success(), "{json:?}");
    let v: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["instruction"], serde_json::Value::Null);

    let human = spectra()
        .args(["instructions", "apply", "--change", "c"])
        .current_dir(&*dir)
        .output()
        .unwrap();
    assert!(human.status.success(), "{human:?}");
    let text = String::from_utf8(human.stdout).unwrap();
    assert!(text.ends_with(" complete\n"), "{text:?}");
    assert!(!text.contains("Instruction:"), "{text:?}");
}

/// 自訂 schema `tr`：`plan` → `todo`，apply 需要 `todo`；`apply_block` 決定有沒有 tracks。
fn init_project_with_tracks_schema(root: &Path, apply_block: &str) {
    git(root, &["init", "-q"]);
    let init = spectra().arg("init").current_dir(root).output().unwrap();
    assert!(init.status.success(), "init failed: {init:?}");
    let schema_dir = root.join("openspec").join("schemas").join("tr");
    std::fs::create_dir_all(schema_dir.join("templates")).unwrap();
    for id in ["plan", "todo"] {
        std::fs::write(
            schema_dir.join("templates").join(format!("{id}.md")),
            "# t\n",
        )
        .unwrap();
    }
    std::fs::write(
        schema_dir.join("schema.yaml"),
        format!(
            "name: tr\nversion: 1\ndescription: x\nartifacts:\n- id: plan\n  generates: plan.md\n  description: plan\n  template: plan.md\n  instruction: Plan.\n  requires: []\n- id: todo\n  generates: todo.md\n  description: todo\n  template: todo.md\n  instruction: Todo.\n  requires: [plan]\n{apply_block}"
        ),
    )
    .unwrap();
    let change = root.join("openspec").join("changes").join("c");
    std::fs::create_dir_all(&change).unwrap();
    std::fs::write(
        change.join(".openspec.yaml"),
        "schema: tr\ncreated: 2026-09-01\n",
    )
    .unwrap();
    std::fs::write(change.join("plan.md"), "# p\n").unwrap();
    std::fs::write(change.join("todo.md"), "- [ ] 1.1 a\n- [x] 1.2 b\n").unwrap();
    // tasks.md 不是 tracks 時不影響 apply。
    std::fs::write(change.join("tasks.md"), "- [ ] 9.1 not tracked\n").unwrap();
}

fn apply_json(root: &Path) -> serde_json::Value {
    let out = spectra()
        .args(["instructions", "apply", "--change", "c", "--json"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).unwrap()
}

/// oracle 3.0.0（探測 p37）：apply 的 task 來自 `apply.tracks`；`contextFiles` 以 id 列出
/// 所有已完成的 artifact。
#[test]
fn apply_reads_tasks_from_the_tracks_file() {
    let dir = TempDir::new("custom-schema-tracks");
    init_project_with_tracks_schema(&dir, "apply:\n  requires: [todo]\n  tracks: todo.md\n");
    let v = apply_json(&dir);
    assert_eq!(v["state"], "ready");
    assert_eq!(
        v["progress"],
        serde_json::json!({"total": 2, "complete": 1, "remaining": 1})
    );
    let change = dir.join("openspec/changes/c");
    assert_eq!(
        v["contextFiles"],
        serde_json::json!({
            "plan": change.join("plan.md").to_string_lossy(),
            "todo": change.join("todo.md").to_string_lossy(),
        })
    );
}

/// oracle 3.0.0（探測 p37／p38）：沒有 `apply.tracks` 時不計 task，required artifact 齊全就
/// 是 ready——即使 change 裡有 tasks.md。
#[test]
fn apply_without_tracks_is_ready_with_no_tasks() {
    let dir = TempDir::new("custom-schema-no-tracks");
    init_project_with_tracks_schema(&dir, "apply:\n  requires: [todo]\n");
    let v = apply_json(&dir);
    assert_eq!(v["state"], "ready");
    assert_eq!(
        v["progress"],
        serde_json::json!({"total": 0, "complete": 0, "remaining": 0})
    );
    assert_eq!(v["tasks"], serde_json::json!([]));
}

/// W7g 的 schema fixture（`tests/fixtures/schema_validate/<case>/`）放進
/// `openspec/schemas/m/`，並建一個 `schema: m` 的 change `c1`——與 oracle 探測 p04
/// 的 jail 相同的配置。
fn project_with_fixture_schema(case: &str) -> TempDir {
    let root = TempDir::new(&format!("custom-schema-{case}"));
    git(&root, &["init", "-q"]);
    std::fs::create_dir_all(root.join("openspec/changes/archive")).unwrap();
    std::fs::create_dir_all(root.join("openspec/specs")).unwrap();
    std::fs::create_dir_all(root.join("openspec/changes/c1")).unwrap();
    std::fs::write(root.join(".spectra.yaml"), "spec_dir: openspec\n").unwrap();
    std::fs::write(root.join("openspec/config.yaml"), "schema: spec-driven\n").unwrap();
    std::fs::write(
        root.join("openspec/changes/c1/.openspec.yaml"),
        "schema: m\ncreated: 2026-09-01\n",
    )
    .unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/schema_validate")
        .join(case);
    let target = root.join("openspec/schemas/m");
    std::fs::create_dir_all(target.join("templates")).unwrap();
    std::fs::copy(fixture.join("schema.yaml"), target.join("schema.yaml")).unwrap();
    for entry in std::fs::read_dir(fixture.join("templates"))
        .into_iter()
        .flatten()
    {
        let entry = entry.unwrap();
        std::fs::copy(
            entry.path(),
            target.join("templates").join(entry.file_name()),
        )
        .unwrap();
    }
    root
}

fn stdout_of(root: &Path, args: &[&str]) -> String {
    let output = spectra().args(args).current_dir(root).output().unwrap();
    assert!(output.status.success(), "{args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap()
}

/// oracle 3.0.0（W7g 探測 q01／q03）：artifact 沒有 `instruction` 時，human 輸出整段
/// 省略，`--json` 沒有 `instruction` 這個 key。
#[test]
fn instructions_omit_a_missing_artifact_instruction() {
    let root = project_with_fixture_schema("art-no-instruction");

    assert_eq!(
        stdout_of(
            &root,
            &["instructions", "a", "--change", "c1", "--no-color"]
        ),
        "Artifact: a\nOutput: a.md\nDescription: A\n\nUnlocks:\n  - b\n\nTemplate:\n# A\n\n"
    );
    let json: serde_json::Value = serde_json::from_str(&stdout_of(
        &root,
        &["instructions", "a", "--change", "c1", "--json"],
    ))
    .unwrap();
    assert!(
        !json.as_object().unwrap().contains_key("instruction"),
        "{json}"
    );
}

/// oracle 3.0.0（W7g 探測 q03）：template 缺檔時 human 輸出省略 `Template:` 段落。
#[test]
fn instructions_omit_the_template_section_for_a_missing_template() {
    let root = project_with_fixture_schema("tpl-missing");

    assert_eq!(
        stdout_of(&root, &["instructions", "b", "--change", "c1", "--no-color"]),
        "Artifact: b\nOutput: b.md\nDescription: B\n\nInstruction:\ndo b\n\nDependencies:\n  \u{25cb} a (a.md)\n"
    );
}

/// oracle 3.0.0（W7g 探測 q01）：schema 沒有 `apply` 時 apply 階段要求全部 artifact。
#[test]
fn apply_without_an_apply_section_requires_every_artifact() {
    let root = project_with_fixture_schema("no-apply");

    assert_eq!(
        stdout_of(&root, &["instructions", "apply", "--change", "c1", "--no-color"]),
        "Change: c1\nSchema: m\nState: blocked\nProgress: 0/0 complete\n\nMissing artifacts:\n  - a\n  - b\n"
    );
}
