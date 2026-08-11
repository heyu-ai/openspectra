use chrono::{Duration, Local};
use sha2::{Digest, Sha256};
use spectra_core::schema;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn spectra() -> Command {
    Command::new(env!("CARGO_BIN_EXE_spectra"))
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git {args:?} failed: {output:?}");
}

fn git_commit_at(dir: &Path, message: &str, date: &str) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["commit", "-qm", message])
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .expect("git commit runs");
    assert!(output.status.success(), "dated commit failed: {output:?}");
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "spectra-instructions-it-{label}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn init_project_with_change(root: &Path, name: &str) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Howie"]);
    git(root, &["config", "user.email", "howie@example.com"]);
    let init = spectra().arg("init").current_dir(root).output().unwrap();
    assert!(init.status.success(), "init failed: {init:?}");
    let new_change = spectra()
        .args(["new", "change", name])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        new_change.status.success(),
        "new change failed: {new_change:?}"
    );
}

fn add_change(root: &Path, name: &str) {
    let dir = change_dir(root, name);
    std::fs::create_dir_all(&dir).unwrap();
    let today = Local::now().date_naive();
    std::fs::write(
        dir.join(".openspec.yaml"),
        format!("schema: spec-driven\ncreated: {today}\ncreated_by: Test\n"),
    )
    .unwrap();
}

fn change_dir(root: &Path, name: &str) -> PathBuf {
    root.join("openspec").join("changes").join(name)
}

fn run(root: &Path, args: &[&str]) -> Output {
    spectra().args(args).current_dir(root).output().unwrap()
}

fn run_ok(root: &Path, args: &[&str]) -> Output {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "spectra {args:?} failed: {output:?}"
    );
    output
}

fn json_output(root: &Path, args: &[&str]) -> (String, serde_json::Value) {
    let output = run_ok(root, args);
    let text = String::from_utf8(output.stdout).unwrap();
    let value = serde_json::from_str(&text).unwrap();
    (text, value)
}

fn assert_patterns_in_order(text: &str, patterns: &[&str]) {
    let mut offset = 0;
    for pattern in patterns {
        let relative = text[offset..]
            .find(pattern)
            .unwrap_or_else(|| panic!("missing {pattern:?} after byte {offset} in:\n{text}"));
        offset += relative + pattern.len();
    }
}

fn write_artifacts(root: &Path, name: &str, tasks: &str) {
    let dir = change_dir(root, name);
    std::fs::write(dir.join("proposal.md"), "# Proposal\n").unwrap();
    std::fs::write(dir.join("design.md"), "# Design\n").unwrap();
    std::fs::write(dir.join("tasks.md"), tasks).unwrap();
    let specs = dir.join("specs").join("cap");
    std::fs::create_dir_all(&specs).unwrap();
    std::fs::write(specs.join("spec.md"), "# Spec\n").unwrap();
}

#[test]
fn artifact_json_for_all_four_modes_has_canonical_order_constants_and_dag_fields() {
    let root = TempDir::new("artifacts");
    init_project_with_change(&root, "demo-feature");
    let ids = ["proposal", "design", "specs", "tasks"];

    for (index, id) in ids.iter().enumerate() {
        let (text, value) = json_output(
            &root,
            &["instructions", id, "--change", "demo-feature", "--json"],
        );
        assert_patterns_in_order(
            &text,
            &[
                "  \"changeName\":",
                "  \"artifactId\":",
                "  \"schemaName\":",
                "  \"changeDir\":",
                "  \"outputPath\":",
                "  \"description\":",
                "  \"instruction\":",
                "  \"locale\":",
                "  \"template\":",
                "  \"dependencies\":",
                "  \"unlocks\":",
            ],
        );
        let definition = &schema::ARTIFACTS[index];
        assert_eq!(value["artifactId"], *id);
        assert_eq!(value["instruction"], definition.instruction);
        assert_eq!(value["template"], definition.template);
        assert!(Path::new(value["changeDir"].as_str().unwrap()).is_absolute());
    }

    let (_, empty_proposal) = json_output(
        &root,
        &[
            "instructions",
            "proposal",
            "--change",
            "demo-feature",
            "--json",
        ],
    );
    assert_eq!(
        empty_proposal["unlocks"],
        serde_json::json!(["design", "specs"])
    );
    let (design_before_text, design_before) = json_output(
        &root,
        &[
            "instructions",
            "design",
            "--change",
            "demo-feature",
            "--json",
        ],
    );
    assert_eq!(design_before["dependencies"][0]["done"], false);
    assert_patterns_in_order(
        &design_before_text,
        &[
            "      \"id\":",
            "      \"done\":",
            "      \"path\":",
            "      \"description\":",
        ],
    );

    std::fs::write(
        change_dir(&root, "demo-feature").join("proposal.md"),
        "# P\n",
    )
    .unwrap();
    let (_, design_after) = json_output(
        &root,
        &[
            "instructions",
            "design",
            "--change",
            "demo-feature",
            "--json",
        ],
    );
    assert_eq!(design_after["dependencies"][0]["done"], true);

    std::fs::write(
        change_dir(&root, "demo-feature").join("tasks.md"),
        "# Tasks\n",
    )
    .unwrap();
    let (_, specs_after_tasks) = json_output(
        &root,
        &[
            "instructions",
            "specs",
            "--change",
            "demo-feature",
            "--json",
        ],
    );
    assert_eq!(specs_after_tasks["unlocks"], serde_json::json!([]));
}

#[test]
fn project_context_and_artifact_rules_only_extend_artifact_json() {
    let root = TempDir::new("project-instructions");
    init_project_with_change(&root, "demo-feature");
    let context = "PROJECT-CONTEXT-127 first line\nPROJECT-CONTEXT-127 second line";
    let rule = "PROJECT-RULE-127 keep proposals concise";
    std::fs::write(
        root.join("openspec").join("config.yaml"),
        format!(
            "schema: spec-driven\ncontext: |\n  PROJECT-CONTEXT-127 first line\n  PROJECT-CONTEXT-127 second line\nrules:\n  proposal:\n    - {rule}\n"
        ),
    )
    .unwrap();

    let (proposal_text, proposal) = json_output(
        &root,
        &[
            "instructions",
            "proposal",
            "--change",
            "demo-feature",
            "--json",
        ],
    );
    assert_patterns_in_order(
        &proposal_text,
        &[
            "  \"instruction\":",
            "  \"context\":",
            "  \"rules\":",
            "  \"locale\":",
        ],
    );
    assert_eq!(proposal["context"], context);
    assert_eq!(proposal["rules"], serde_json::json!([rule]));

    let (tasks_text, tasks) = json_output(
        &root,
        &[
            "instructions",
            "tasks",
            "--change",
            "demo-feature",
            "--json",
        ],
    );
    assert_eq!(tasks["context"], context);
    assert!(!tasks_text.contains("\"rules\":"));

    let (apply_text, _) = json_output(
        &root,
        &[
            "instructions",
            "apply",
            "--change",
            "demo-feature",
            "--json",
        ],
    );
    assert!(!apply_text.contains("\"context\":"));
    assert!(!apply_text.contains("\"rules\":"));

    let human = run_ok(
        &root,
        &["instructions", "proposal", "--change", "demo-feature"],
    );
    let human_text = String::from_utf8(human.stdout).unwrap();
    assert!(!human_text.contains(context));
    assert!(!human_text.contains(rule));
}

#[test]
fn no_artifact_selects_first_incomplete_then_apply_when_all_are_done() {
    let root = TempDir::new("selection");
    init_project_with_change(&root, "demo-feature");

    let (_, empty) = json_output(
        &root,
        &["instructions", "--change", "demo-feature", "--json"],
    );
    assert_eq!(empty["artifactId"], "proposal");

    write_artifacts(&root, "demo-feature", "- [ ] 1.1 pending\n");
    let (_, complete_artifacts) = json_output(
        &root,
        &["instructions", "--change", "demo-feature", "--json"],
    );
    assert_eq!(complete_artifacts["state"], "ready");
    assert!(complete_artifacts.get("artifactId").is_none());
}

#[test]
fn apply_ready_has_ordered_context_loose_tasks_progress_and_clean_preflight() {
    let root = TempDir::new("ready");
    init_project_with_change(&root, "demo-feature");
    write_artifacts(
        &root,
        "demo-feature",
        concat!(
            "- [x] 1.1 done\n",
            "- [ ] [P] 1.2 parallel\n",
            "- [ ] 1.3 [P] later\n",
            "+ [z] 1.4 custom\n",
        ),
    );

    let (text, value) = json_output(
        &root,
        &[
            "instructions",
            "apply",
            "--change",
            "demo-feature",
            "--json",
        ],
    );
    assert_patterns_in_order(
        &text,
        &[
            "  \"changeName\":",
            "  \"changeDir\":",
            "  \"schemaName\":",
            "  \"contextFiles\":",
            "  \"progress\":",
            "  \"tasks\":",
            "  \"state\":",
            "  \"locale\":",
            "  \"instruction\":",
            "  \"preflight\":",
        ],
    );
    let context_start = text.find("  \"contextFiles\":").unwrap();
    let context_end = text.find("  \"progress\":").unwrap();
    assert_patterns_in_order(
        &text[context_start..context_end],
        &[
            "    \"proposal\":",
            "    \"design\":",
            "    \"specs\":",
            "    \"tasks\":",
        ],
    );
    let dir = std::fs::canonicalize(change_dir(&root, "demo-feature")).unwrap();
    assert_eq!(
        value["contextFiles"]["proposal"],
        dir.join("proposal.md").to_string_lossy().as_ref()
    );
    assert_eq!(
        value["contextFiles"]["design"],
        dir.join("design.md").to_string_lossy().as_ref()
    );
    assert_eq!(
        value["contextFiles"]["specs"],
        dir.join("specs/**/*.md").to_string_lossy().as_ref()
    );
    assert_eq!(
        value["contextFiles"]["tasks"],
        dir.join("tasks.md").to_string_lossy().as_ref()
    );
    assert_eq!(
        value["progress"],
        serde_json::json!({"total": 4, "complete": 1, "remaining": 3})
    );
    assert_eq!(value["state"], "ready");
    assert_eq!(value["tasks"][0]["id"], "1");
    assert_eq!(value["tasks"][3]["id"], "4");
    assert_eq!(value["tasks"][1]["description"], "1.2 parallel");
    assert_eq!(value["tasks"][1]["parallel"], true);
    assert_eq!(value["tasks"][2]["description"], "1.3 [P] later");
    assert_eq!(value["tasks"][2]["parallel"], false);
    assert_eq!(value["tasks"][3]["done"], false);
    assert_eq!(value["preflight"]["status"], "clean");
    assert_eq!(value["preflight"]["missingFiles"], serde_json::json!([]));
    assert_eq!(value["preflight"]["driftedFiles"], serde_json::json!([]));
    assert_eq!(value["preflight"]["staleness"]["daysOld"], 0);
    assert_eq!(value["preflight"]["staleness"]["isStale"], false);
}

#[test]
fn apply_all_done_omits_conditional_keys() {
    let root = TempDir::new("all-done");
    init_project_with_change(&root, "demo-feature");
    write_artifacts(&root, "demo-feature", "- [x] 1.1 a\n- [X] 1.2 b\n");

    let (_, value) = json_output(
        &root,
        &[
            "instructions",
            "apply",
            "--change",
            "demo-feature",
            "--json",
        ],
    );
    assert_eq!(value["state"], "all_done");
    assert!(value.get("preflight").is_none());
    assert!(value.get("missingArtifacts").is_none());
}

#[test]
fn apply_blocked_distinguishes_a_missing_tasks_artifact_from_zero_checkboxes() {
    let root = TempDir::new("blocked");
    init_project_with_change(&root, "missing-tasks");
    let dir = change_dir(&root, "missing-tasks");
    std::fs::write(dir.join("proposal.md"), "# Proposal\n").unwrap();
    let specs = dir.join("specs").join("cap");
    std::fs::create_dir_all(&specs).unwrap();
    std::fs::write(specs.join("spec.md"), "# Spec\n").unwrap();

    let (_, missing) = json_output(
        &root,
        &[
            "instructions",
            "apply",
            "--change",
            "missing-tasks",
            "--json",
        ],
    );
    assert_eq!(missing["state"], "blocked");
    assert_eq!(missing["missingArtifacts"], serde_json::json!(["tasks"]));
    assert!(missing["contextFiles"].get("tasks").is_none());
    assert!(missing.get("preflight").is_none());

    add_change(&root, "zero-checkboxes");
    std::fs::write(
        change_dir(&root, "zero-checkboxes").join("tasks.md"),
        "# No checkbox lines\n",
    )
    .unwrap();
    let (_, zero) = json_output(
        &root,
        &[
            "instructions",
            "apply",
            "--change",
            "zero-checkboxes",
            "--json",
        ],
    );
    assert_eq!(zero["state"], "blocked");
    assert!(zero.get("missingArtifacts").is_none());
    assert!(zero.get("preflight").is_none());
}

#[test]
fn preflight_reports_staleness_missing_files_and_drift_in_priority_order() {
    let root = TempDir::new("preflight");
    init_project_with_change(&root, "demo-feature");
    let dir = change_dir(&root, "demo-feature");
    let created = Local::now().date_naive() - Duration::days(30);
    std::fs::write(
        dir.join(".openspec.yaml"),
        format!("schema: spec-driven\ncreated: {created}\ncreated_by: Test\n"),
    )
    .unwrap();
    write_artifacts(&root, "demo-feature", "- [ ] 1.1 pending\n");

    let (_, stale) = json_output(
        &root,
        &[
            "instructions",
            "apply",
            "--change",
            "demo-feature",
            "--json",
        ],
    );
    assert_eq!(stale["preflight"]["status"], "warnings");
    assert_eq!(stale["preflight"]["staleness"]["daysOld"], 30);
    assert_eq!(stale["preflight"]["staleness"]["isStale"], true);

    std::fs::write(
        dir.join("proposal.md"),
        "# Proposal\nAffected code:\n- src/nonexistent.rs\n",
    )
    .unwrap();
    let (_, missing) = json_output(
        &root,
        &[
            "instructions",
            "apply",
            "--change",
            "demo-feature",
            "--json",
        ],
    );
    assert_eq!(missing["preflight"]["status"], "critical");
    assert_eq!(
        missing["preflight"]["missingFiles"],
        serde_json::json!([{"path": "src/nonexistent.rs", "referencedIn": "proposal"}])
    );
}

#[test]
fn preflight_omits_staleness_when_created_is_missing() {
    let root = TempDir::new("no-created");
    init_project_with_change(&root, "demo-feature");
    write_artifacts(&root, "demo-feature", "- [ ] 1.1 pending\n");
    std::fs::write(
        change_dir(&root, "demo-feature").join(".openspec.yaml"),
        "schema: spec-driven\ncreated_by: Test\n",
    )
    .unwrap();

    let (_, value) = json_output(
        &root,
        &[
            "instructions",
            "apply",
            "--change",
            "demo-feature",
            "--json",
        ],
    );
    assert_eq!(value["preflight"]["status"], "clean");
    assert!(value["preflight"].get("staleness").is_none());
    assert_eq!(value["preflight"]["driftedFiles"], serde_json::json!([]));
}

#[test]
fn preflight_reports_a_file_committed_after_the_change_date_as_drifted() {
    let root = TempDir::new("drifted");
    init_project_with_change(&root, "demo-feature");
    let dir = change_dir(&root, "demo-feature");
    let source = root.join("src");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("drift.rs"), "before\n").unwrap();
    std::fs::write(dir.join("proposal.md"), "Affected code: src/drift.rs\n").unwrap();
    std::fs::write(dir.join("tasks.md"), "- [ ] 1.1 pending\n").unwrap();
    let specs = dir.join("specs").join("cap");
    std::fs::create_dir_all(&specs).unwrap();
    std::fs::write(specs.join("spec.md"), "# Spec\n").unwrap();
    std::fs::write(
        dir.join(".openspec.yaml"),
        "schema: spec-driven\ncreated: 2026-07-10\ncreated_by: Test\n",
    )
    .unwrap();
    git(&root, &["add", "."]);
    git_commit_at(&root, "before", "2026-07-01T12:00:00+0000");
    std::fs::write(source.join("drift.rs"), "after\n").unwrap();
    git(&root, &["add", "src/drift.rs"]);
    git_commit_at(&root, "after", "2026-07-15T12:00:00+0000");

    let (_, value) = json_output(
        &root,
        &[
            "instructions",
            "apply",
            "--change",
            "demo-feature",
            "--json",
        ],
    );
    assert_eq!(value["preflight"]["status"], "warnings");
    assert_eq!(
        value["preflight"]["driftedFiles"],
        serde_json::json!([{
            "path": "src/drift.rs",
            "lastCommit": "2026-07-15",
            "changeCreated": "2026-07-10"
        }])
    );
}

fn skill_asset(name: &str) -> Vec<u8> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    std::fs::read(
        repo.join("crates/spectra-core/assets/skills")
            .join(format!("{name}.md")),
    )
    .unwrap()
}

#[derive(Debug)]
struct SkillManifestRow {
    name: String,
    bytes: usize,
    sha256: String,
}

fn skill_manifest() -> Vec<SkillManifestRow> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/reverse-engineering/golden/skills-2.3.1.tsv");
    let text = std::fs::read_to_string(path).unwrap();
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("skill\tbytes\tsha256"));
    lines
        .map(|line| {
            let mut fields = line.split('\t');
            let row = SkillManifestRow {
                name: fields.next().unwrap().to_string(),
                bytes: fields.next().unwrap().parse().unwrap(),
                sha256: fields.next().unwrap().to_string(),
            };
            assert!(fields.next().is_none(), "extra columns in row: {line}");
            row
        })
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn embedded_skills_match_the_assets_and_oracle_manifest_outside_a_project() {
    let root = TempDir::new("skills");
    let names = [
        "tdd", "audit", "apply", "archive", "ask", "commit", "debug", "discuss", "drift", "ingest",
        "propose", "analyze", "verify", "sync", "clarify",
    ];
    let manifest = skill_manifest();
    assert_eq!(
        manifest
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        names
    );

    for (name, row) in names.into_iter().zip(manifest) {
        let output = run(&root, &["instructions", "--skill", name]);
        assert!(output.status.success(), "skill {name} failed: {output:?}");
        assert!(output.stderr.is_empty(), "skill {name} wrote to stderr");
        assert_eq!(output.stdout, skill_asset(name), "skill {name} drifted");
        assert_eq!(
            output.stdout.len(),
            row.bytes,
            "skill {name} length drifted"
        );
        assert_eq!(
            sha256_hex(&output.stdout),
            row.sha256,
            "skill {name} digest drifted"
        );
    }
}

#[test]
fn embedded_skill_takes_precedence_over_artifact_change_and_schema() {
    let root = TempDir::new("skill-precedence");
    let output = run(
        &root,
        &[
            "instructions",
            "proposal",
            "--skill",
            "tdd",
            "--change",
            "whatever",
            "--schema",
            "spec-driven",
        ],
    );

    assert!(output.status.success(), "skill failed: {output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, skill_asset("tdd"));
}

#[test]
fn embedded_skill_wins_inside_an_initialized_project() {
    let root = TempDir::new("skill-in-project");
    init_project_with_change(&root, "demo-feature");

    for args in [
        vec!["instructions", "--skill", "tdd"],
        vec![
            "instructions",
            "proposal",
            "--skill",
            "tdd",
            "--change",
            "demo-feature",
        ],
    ] {
        let output = run(&root, &args);
        assert!(output.status.success(), "skill failed: {output:?}");
        assert!(output.stderr.is_empty());
        assert_eq!(output.stdout, skill_asset("tdd"));
    }
}

#[test]
fn json_is_inert_when_an_embedded_skill_is_requested() {
    let root = TempDir::new("skill-json");
    let output = run(&root, &["instructions", "--json", "--skill", "tdd"]);

    assert!(output.status.success(), "skill failed: {output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, skill_asset("tdd"));
}

#[test]
fn unknown_embedded_skill_fails_outside_a_project() {
    let root = TempDir::new("skill-unknown");
    let output = run(&root, &["instructions", "--skill", "bogus"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"Error: Unknown skill: bogus\n");
}

#[test]
fn instructions_errors_match_the_oracle_contract() {
    let root = TempDir::new("errors");
    init_project_with_change(&root, "demo-feature");
    let cases = [
        (
            vec![
                "instructions",
                "bogus",
                "--change",
                "demo-feature",
            ],
            "Error: Artifact 'bogus' not found in schema\n",
        ),
        (
            vec!["instructions", "proposal", "--change", "nope"],
            "Error: Change 'nope' not found.\n",
        ),
        (
            vec![
                "instructions",
                "proposal",
                "--change",
                "demo-feature",
                "--schema",
                "bogus",
            ],
            "Error: Schema not found: Schema 'bogus' not found in project, user, or built-in locations\n",
        ),
        (
            vec![
                "instructions",
                "bogus",
                "--change",
                "nope",
                "--schema",
                "bogus",
                "--skill",
                "anything",
            ],
            "Error: Unknown skill: anything\n",
        ),
    ];

    for (args, expected) in cases {
        let output = run(&root, &args);
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(String::from_utf8(output.stderr).unwrap(), expected);
    }
}

#[test]
fn human_artifact_and_apply_outputs_are_byte_exact() {
    let root = TempDir::new("human");
    init_project_with_change(&root, "demo-feature");

    let artifact = run_ok(
        &root,
        &["instructions", "proposal", "--change", "demo-feature"],
    );
    assert_eq!(
        String::from_utf8(artifact.stdout).unwrap(),
        format!(
            "Artifact: proposal\nOutput: proposal.md\nDescription: {}\n\nInstruction:\n{}\n\nUnlocks:\n  - design\n  - specs\n\nTemplate:\n{}\n",
            schema::PROPOSAL_DESCRIPTION,
            schema::PROPOSAL_INSTRUCTION,
            schema::PROPOSAL_TEMPLATE
        )
    );

    let tasks = run_ok(
        &root,
        &["instructions", "tasks", "--change", "demo-feature"],
    );
    let tasks_stdout = String::from_utf8(tasks.stdout).unwrap();
    assert!(
        !tasks_stdout.contains("\nUnlocks:\n"),
        "empty unlocks must omit the section:\n{tasks_stdout}"
    );

    write_artifacts(
        &root,
        "demo-feature",
        "- [x] 1.1 done one\n- [ ] [P] 1.2 pending two\n",
    );
    let apply = run_ok(
        &root,
        &["instructions", "apply", "--change", "demo-feature"],
    );
    assert_eq!(
        String::from_utf8(apply.stdout).unwrap(),
        concat!(
            "Change: demo-feature\n",
            "Schema: spec-driven\n",
            "State: ready\n",
            "Progress: 1/2 complete\n",
            "\n",
            "Tasks:\n",
            "  ✓ 1.1 done one\n",
            "  ○ 1.2 pending two\n",
            "\n",
            "Instruction:\n",
            "Read context files, work through pending tasks, mark complete as you go.\n",
            "Pause if you hit blockers or need clarification.\n",
            "\n",
        )
    );
}
