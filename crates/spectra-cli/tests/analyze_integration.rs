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

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "spectra-analyze-it-{label}-{}-{seq}",
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

fn init_project(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Howie"]);
    git(root, &["config", "user.email", "howie@example.com"]);
    let output = spectra().arg("init").current_dir(root).output().unwrap();
    assert!(output.status.success(), "init failed: {output:?}");
}

fn change_dir(root: &Path, name: &str) -> PathBuf {
    root.join("openspec").join("changes").join(name)
}

fn add_change(root: &Path, name: &str) {
    std::fs::create_dir_all(change_dir(root, name)).unwrap();
}

fn write_change_file(root: &Path, name: &str, relative: &str, content: &str) {
    let path = change_dir(root, name).join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn write_main_spec(root: &Path, capability: &str, content: &str) {
    let path = root
        .join("openspec")
        .join("specs")
        .join(capability)
        .join("spec.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn run(root: &Path, args: &[&str]) -> Output {
    spectra().args(args).current_dir(root).output().unwrap()
}

fn json_report(root: &Path, name: &str) -> (String, serde_json::Value) {
    let output = run(root, &["analyze", name, "--json"]);
    assert!(
        output.status.success(),
        "analyze must exit 0 even with findings: {output:?}"
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let value = serde_json::from_str(&text).unwrap();
    (text, value)
}

fn finding<'a>(report: &'a serde_json::Value, key: &str) -> &'a serde_json::Value {
    report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["summary_msg"]["key"] == format!("{key}.summary"))
        .unwrap_or_else(|| panic!("missing finding {key} in {report:#}"))
}

fn assert_no_finding(report: &serde_json::Value, key: &str) {
    assert!(
        report["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| finding["summary_msg"]["key"] != format!("{key}.summary")),
        "unexpected finding {key} in {report:#}"
    );
}

fn proposal_with_capability(capability: &str) -> String {
    format!("## Capabilities\n\n### New Capabilities\n\n- `{capability}`: behavior\n\n## Impact\n")
}

fn complete_delta(section: &str, requirement: &str, scenario: &str) -> String {
    format!(
        "## {section} Requirements\n\n\
         ### Requirement: {requirement}\n\n\
         The system SHALL provide the behavior.\n\n\
         #### Scenario: {scenario}\n\n\
         ##### Example: concrete\n\n\
         - **GIVEN** concrete input\n\
         - **WHEN** the operation runs\n\
         - **THEN** concrete output is returned\n"
    )
}

#[test]
fn empty_change_skips_all_dimensions_pins_snake_case_json_and_human_output() {
    let root = TempDir::new("empty");
    init_project(&root);
    add_change(&root, "demo");

    let (text, report) = json_report(&root, "demo");
    assert!(text.ends_with("\n"));
    assert!(!text.ends_with("\n\n"));
    let key_positions: Vec<_> = [
        "\"change_id\"",
        "\"dimensions\"",
        "\"findings\"",
        "\"artifacts_analyzed\"",
        "\"artifacts_missing\"",
    ]
    .iter()
    .map(|key| text.find(key).unwrap())
    .collect();
    assert!(key_positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(!text.contains("changeId"));
    assert_eq!(report["change_id"], "demo");
    assert_eq!(report["findings"], serde_json::json!([]));
    assert_eq!(report["artifacts_analyzed"], serde_json::json!([]));
    assert_eq!(
        report["artifacts_missing"],
        serde_json::json!(["proposal", "specs", "design", "tasks"])
    );
    let dimensions = report["dimensions"].as_array().unwrap();
    assert_eq!(dimensions.len(), 4);
    for (index, name) in ["Coverage", "Consistency", "Ambiguity", "Gaps"]
        .iter()
        .enumerate()
    {
        assert_eq!(dimensions[index]["dimension"], *name);
        assert_eq!(
            dimensions[index]["status"],
            "Skipped (insufficient artifacts)"
        );
        assert_eq!(dimensions[index]["finding_count"], 0);
    }

    let human = run(&root, &["analyze", "demo"]);
    assert!(human.status.success());
    assert_eq!(
        String::from_utf8(human.stdout).unwrap(),
        concat!(
            "Change: demo\n",
            "\n",
            "  ✓ Coverage       Skipped (insufficient artifacts) (0 findings)\n",
            "  ✓ Consistency    Skipped (insufficient artifacts) (0 findings)\n",
            "  ✓ Ambiguity      Skipped (insufficient artifacts) (0 findings)\n",
            "  ✓ Gaps           Skipped (insufficient artifacts) (0 findings)\n",
            "  Missing: proposal, specs, design, tasks\n",
            "\n",
            "  ✓ No issues found\n",
        )
    );
}

#[test]
fn cov_missing_spec_positive_and_negative_contract() {
    let root = TempDir::new("cov-missing-spec");
    init_project(&root);
    add_change(&root, "positive");
    write_change_file(
        &root,
        "positive",
        "proposal.md",
        &proposal_with_capability("alpha"),
    );
    write_change_file(&root, "positive", "tasks.md", "# Tasks\n");

    let (_, positive) = json_report(&root, "positive");
    assert_eq!(
        finding(&positive, "covMissingSpec"),
        &serde_json::json!({
            "id": "COV-1",
            "dimension": "Coverage",
            "severity": "Critical",
            "location": "proposal.md → Capabilities",
            "summary": "Capability `alpha` has no corresponding spec file",
            "recommendation": "Create specs/alpha/spec.md with requirements",
            "summary_msg": {"key": "covMissingSpec.summary", "params": {"cap": "alpha"}},
            "recommendation_msg": {"key": "covMissingSpec.recommendation", "params": {"cap": "alpha"}}
        })
    );

    add_change(&root, "negative");
    write_change_file(
        &root,
        "negative",
        "proposal.md",
        &proposal_with_capability("alpha"),
    );
    write_change_file(&root, "negative", "tasks.md", "# Tasks\n");
    write_change_file(&root, "negative", "specs/alpha/spec.md", "# Spec\n");
    let (_, negative) = json_report(&root, "negative");
    assert_no_finding(&negative, "covMissingSpec");
}

#[test]
fn cov_missing_task_positive_and_negative_contract() {
    let root = TempDir::new("cov-missing-task");
    init_project(&root);
    for name in ["positive", "negative"] {
        add_change(&root, name);
        write_change_file(&root, name, "proposal.md", "# Proposal\n");
        write_change_file(
            &root,
            name,
            "specs/alpha/spec.md",
            &complete_delta("ADDED", "Alpha works", "alpha happy"),
        );
    }
    write_change_file(&root, "positive", "tasks.md", "- [ ] unrelated work\n");
    write_change_file(
        &root,
        "negative",
        "tasks.md",
        "- [ ] implement ALPHA WORKS now\n",
    );

    let (text, positive) = json_report(&root, "positive");
    let finding_start = text.find("  \"findings\": [").unwrap();
    let finding_text = &text[finding_start..];
    let finding_key_positions: Vec<_> = [
        "\"id\"",
        "\"dimension\"",
        "\"severity\"",
        "\"location\"",
        "\"summary\"",
        "\"recommendation\"",
        "\"summary_msg\"",
        "\"recommendation_msg\"",
    ]
    .iter()
    .map(|key| finding_text.find(key).unwrap())
    .collect();
    assert!(finding_key_positions
        .windows(2)
        .all(|pair| pair[0] < pair[1]));
    assert!(!finding_text.contains("summaryMsg"));
    assert!(!finding_text.contains("recommendationMsg"));
    assert_eq!(
        finding(&positive, "covMissingTask"),
        &serde_json::json!({
            "id": "COV-1",
            "dimension": "Coverage",
            "severity": "Warning",
            "location": "specs/alpha/spec.md",
            "summary": "Requirement 'Alpha works' has no matching task",
            "recommendation": "Add a task in tasks.md that references 'Alpha works'",
            "summary_msg": {"key": "covMissingTask.summary", "params": {"req": "Alpha works"}},
            "recommendation_msg": {"key": "covMissingTask.recommendation", "params": {"req": "Alpha works"}}
        })
    );
    let (_, negative) = json_report(&root, "negative");
    assert_no_finding(&negative, "covMissingTask");
}

#[test]
fn cov_delta_validation_positive_and_negative_contract() {
    let root = TempDir::new("cov-delta-validation");
    init_project(&root);
    for name in ["positive", "negative"] {
        add_change(&root, name);
        write_change_file(&root, name, "proposal.md", "# Proposal\n");
    }
    write_change_file(
        &root,
        "positive",
        "specs/alpha/spec.md",
        &format!(
            "{}{}",
            complete_delta("ADDED", "Duplicate", "first"),
            complete_delta("ADDED", "Duplicate", "second")
        ),
    );
    write_change_file(
        &root,
        "negative",
        "specs/alpha/spec.md",
        &format!(
            "{}{}",
            complete_delta("ADDED", "First", "first"),
            complete_delta("ADDED", "Second", "second")
        ),
    );

    let (_, positive) = json_report(&root, "positive");
    assert_eq!(
        finding(&positive, "covDeltaValidation"),
        &serde_json::json!({
            "id": "COV-1",
            "dimension": "Coverage",
            "severity": "Critical",
            "location": "specs/alpha/spec.md",
            "summary": "Delta spec validation error: Duplicate requirement 'Duplicate' in ADDED section",
            "recommendation": "Fix the delta spec structure",
            "summary_msg": {
                "key": "covDeltaValidation.summary",
                "params": {"error": "Duplicate requirement 'Duplicate' in ADDED section"}
            },
            "recommendation_msg": {"key": "covDeltaValidation.recommendation", "params": {}}
        })
    );
    let (_, negative) = json_report(&root, "negative");
    assert_no_finding(&negative, "covDeltaValidation");
}

#[test]
fn con_design_not_in_tasks_positive_and_negative_contract() {
    let root = TempDir::new("con-design-tasks");
    init_project(&root);
    for name in ["positive", "negative"] {
        add_change(&root, name);
        write_change_file(
            &root,
            name,
            "design.md",
            "## Decisions\n\n### Cache Strategy\n",
        );
    }
    write_change_file(&root, "positive", "tasks.md", "- [ ] unrelated work\n");
    write_change_file(
        &root,
        "negative",
        "tasks.md",
        "- [ ] implement CACHE STRATEGY\n",
    );

    let (_, positive) = json_report(&root, "positive");
    assert_eq!(
        finding(&positive, "conDesignNotInTasks"),
        &serde_json::json!({
            "id": "CON-1",
            "dimension": "Consistency",
            "severity": "Warning",
            "location": "design.md",
            "summary": "Design topic 'cache strategy' not referenced in tasks",
            "recommendation": "Verify tasks cover this design decision",
            "summary_msg": {"key": "conDesignNotInTasks.summary", "params": {"keyword": "cache strategy"}},
            "recommendation_msg": {"key": "conDesignNotInTasks.recommendation", "params": {}}
        })
    );
    let (_, negative) = json_report(&root, "negative");
    assert_no_finding(&negative, "conDesignNotInTasks");
}

#[test]
fn amb_no_scenario_positive_and_negative_contract() {
    let root = TempDir::new("amb-no-scenario");
    init_project(&root);
    for name in ["positive", "negative"] {
        add_change(&root, name);
        write_change_file(&root, name, "proposal.md", "# Proposal\n");
    }
    write_change_file(
        &root,
        "positive",
        "specs/alpha/spec.md",
        "## ADDED Requirements\n\n### Requirement: Alpha works\n\nThe system SHALL work.\n",
    );
    write_change_file(
        &root,
        "negative",
        "specs/alpha/spec.md",
        &complete_delta("ADDED", "Alpha works", "alpha happy"),
    );

    let (_, positive) = json_report(&root, "positive");
    assert_eq!(
        finding(&positive, "ambNoScenario"),
        &serde_json::json!({
            "id": "AMB-1",
            "dimension": "Ambiguity",
            "severity": "Warning",
            "location": "specs/alpha/spec.md",
            "summary": "Requirement 'Alpha works' has no scenarios",
            "recommendation": "Add #### Scenario: sections with WHEN/THEN for 'Alpha works'",
            "summary_msg": {"key": "ambNoScenario.summary", "params": {"req": "Alpha works"}},
            "recommendation_msg": {"key": "ambNoScenario.recommendation", "params": {"req": "Alpha works"}}
        })
    );
    let (_, negative) = json_report(&root, "negative");
    assert_no_finding(&negative, "ambNoScenario");
}

#[test]
fn amb_abstract_scenario_positive_and_negative_contract() {
    let root = TempDir::new("amb-abstract-scenario");
    init_project(&root);
    for name in ["positive", "negative"] {
        add_change(&root, name);
        write_change_file(&root, name, "proposal.md", "# Proposal\n");
    }
    write_change_file(
        &root,
        "positive",
        "specs/alpha/spec.md",
        "## ADDED Requirements\n\n### Requirement: Alpha works\n\nThe system SHALL work.\n\n#### Scenario: alpha happy\n",
    );
    write_change_file(
        &root,
        "negative",
        "specs/alpha/spec.md",
        &complete_delta("ADDED", "Alpha works", "alpha happy"),
    );

    let (_, positive) = json_report(&root, "positive");
    assert_eq!(
        finding(&positive, "ambAbstractScenario"),
        &serde_json::json!({
            "id": "AMB-1",
            "dimension": "Ambiguity",
            "severity": "Suggestion",
            "location": "specs/alpha/spec.md",
            "summary": "Scenario 'alpha happy' has no concrete examples",
            "recommendation": "Add ##### Example: with concrete GIVEN/WHEN/THEN data",
            "summary_msg": {"key": "ambAbstractScenario.summary", "params": {"scenario": "alpha happy"}},
            "recommendation_msg": {"key": "ambAbstractScenario.recommendation", "params": {"scenario": "alpha happy"}}
        })
    );
    let (_, negative) = json_report(&root, "negative");
    assert_no_finding(&negative, "ambAbstractScenario");
}

#[test]
fn amb_weak_language_positive_and_negative_contract() {
    let root = TempDir::new("amb-weak-language");
    init_project(&root);
    for name in ["positive", "negative"] {
        add_change(&root, name);
        write_change_file(&root, name, "proposal.md", "# Proposal\n");
    }
    write_change_file(
        &root,
        "positive",
        "specs/alpha/spec.md",
        "The system Should work.\n",
    );
    write_change_file(
        &root,
        "negative",
        "specs/alpha/spec.md",
        "The system SHALL work.\n",
    );

    let (_, positive) = json_report(&root, "positive");
    assert_eq!(
        finding(&positive, "ambWeakLanguage"),
        &serde_json::json!({
            "id": "AMB-1",
            "dimension": "Ambiguity",
            "severity": "Suggestion",
            "location": "specs/alpha/spec.md:1",
            "summary": "Vague language 'should' found",
            "recommendation": "Replace 'should' with SHALL/SHALL NOT for clarity",
            "summary_msg": {"key": "ambWeakLanguage.summary", "params": {"pattern": "should"}},
            "recommendation_msg": {"key": "ambWeakLanguage.recommendation", "params": {"pattern": "should"}}
        })
    );
    let (_, negative) = json_report(&root, "negative");
    assert_no_finding(&negative, "ambWeakLanguage");
}

#[test]
fn gap_no_proposal_positive_and_negative_contract() {
    let root = TempDir::new("gap-no-proposal");
    init_project(&root);
    for name in ["positive", "negative"] {
        add_change(&root, name);
        write_change_file(&root, name, "specs/alpha/spec.md", "# Spec\n");
    }
    write_change_file(&root, "negative", "proposal.md", "# Proposal\n");

    let (_, positive) = json_report(&root, "positive");
    assert_eq!(
        finding(&positive, "gapNoProposal"),
        &serde_json::json!({
            "id": "GAP-1",
            "dimension": "Gaps",
            "severity": "Critical",
            "location": "change directory",
            "summary": "Specs exist but no proposal.md found",
            "recommendation": "Create proposal.md describing the change purpose",
            "summary_msg": {"key": "gapNoProposal.summary", "params": {}},
            "recommendation_msg": {"key": "gapNoProposal.recommendation", "params": {}}
        })
    );
    let (_, negative) = json_report(&root, "negative");
    assert_no_finding(&negative, "gapNoProposal");
}

#[test]
fn gap_no_main_spec_positive_and_negative_contract() {
    let root = TempDir::new("gap-no-main-spec");
    init_project(&root);
    for (name, capability) in [("positive", "missing-main"), ("negative", "present-main")] {
        add_change(&root, name);
        write_change_file(&root, name, "proposal.md", "# Proposal\n");
        write_change_file(
            &root,
            name,
            &format!("specs/{capability}/spec.md"),
            &complete_delta("MODIFIED", "Login flow", "login succeeds"),
        );
    }
    write_main_spec(
        &root,
        "present-main",
        "# Spec\n\n### Requirement: Login flow\n",
    );

    let (_, positive) = json_report(&root, "positive");
    assert_eq!(
        finding(&positive, "gapNoMainSpec"),
        &serde_json::json!({
            "id": "GAP-1",
            "dimension": "Gaps",
            "severity": "Warning",
            "location": "specs/missing-main/spec.md",
            "summary": "MODIFIED requirements reference capability 'missing-main' but no main spec found",
            "recommendation": "Check if openspec/specs/missing-main/spec.md exists",
            "summary_msg": {"key": "gapNoMainSpec.summary", "params": {"spec": "missing-main"}},
            "recommendation_msg": {"key": "gapNoMainSpec.recommendation", "params": {"spec": "missing-main"}}
        })
    );
    let (_, negative) = json_report(&root, "negative");
    assert_no_finding(&negative, "gapNoMainSpec");
}

#[test]
fn gap_modified_not_found_positive_and_negative_contract() {
    let root = TempDir::new("gap-modified-not-found");
    init_project(&root);
    for (name, capability) in [("positive", "mismatch"), ("negative", "exact")] {
        add_change(&root, name);
        write_change_file(&root, name, "proposal.md", "# Proposal\n");
        write_change_file(
            &root,
            name,
            &format!("specs/{capability}/spec.md"),
            &complete_delta("MODIFIED", "Login flow", "login succeeds"),
        );
    }
    write_main_spec(
        &root,
        "mismatch",
        "# Spec\n\n### Requirement: Login flow extended\n",
    );
    write_main_spec(&root, "exact", "# Spec\n\n### Requirement: Login flow\n");

    let (_, positive) = json_report(&root, "positive");
    assert_eq!(
        finding(&positive, "gapModifiedNotFound"),
        &serde_json::json!({
            "id": "GAP-1",
            "dimension": "Gaps",
            "severity": "Warning",
            "location": "specs/mismatch/spec.md",
            "summary": "MODIFIED requirement 'Login flow' not found in main spec",
            "recommendation": "Verify requirement 'Login flow' exists in openspec/specs/mismatch/spec.md",
            "summary_msg": {"key": "gapModifiedNotFound.summary", "params": {"name": "Login flow"}},
            "recommendation_msg": {
                "key": "gapModifiedNotFound.recommendation",
                "params": {"name": "Login flow", "spec": "mismatch"}
            }
        })
    );
    let (_, negative) = json_report(&root, "negative");
    assert_no_finding(&negative, "gapModifiedNotFound");
}

#[test]
fn human_findings_output_matches_the_measured_contract() {
    let root = TempDir::new("human-findings");
    init_project(&root);
    add_change(&root, "c5");
    write_change_file(
        &root,
        "c5",
        "proposal.md",
        &proposal_with_capability("alpha"),
    );
    write_change_file(&root, "c5", "tasks.md", "- [ ] unrelated work\n");
    write_change_file(
        &root,
        "c5",
        "specs/alpha/spec.md",
        "## ADDED Requirements\n\n### Requirement: Alpha works\n\nThe system SHALL work.\n\n#### Scenario: alpha happy\n",
    );

    let output = run(&root, &["analyze", "c5"]);
    assert!(
        output.status.success(),
        "findings must not affect exit status: {output:?}"
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!(
            "Change: c5\n",
            "\n",
            "  ● Coverage       1 issue(s) found (1 findings)\n",
            "  ✓ Consistency    Skipped (insufficient artifacts) (0 findings)\n",
            "  ● Ambiguity      1 issue(s) found (1 findings)\n",
            "  ✓ Gaps           Clean (0 findings)\n",
            "\n",
            "  Analyzed: proposal, specs, tasks\n",
            "  Missing: design\n",
            "\n",
            "  Findings (2):\n",
            "\n",
            "  [WARNING] Requirement 'Alpha works' has no matching task\n",
            "    at: specs/alpha/spec.md\n",
            "    → Add a task in tasks.md that references 'Alpha works'\n",
            "  [SUGGEST] Scenario 'alpha happy' has no concrete examples\n",
            "    at: specs/alpha/spec.md\n",
            "    → Add ##### Example: with concrete GIVEN/WHEN/THEN data\n",
        )
    );
}

#[test]
fn unknown_change_exits_one_with_status_compatible_error() {
    let root = TempDir::new("unknown");
    init_project(&root);

    let output = run(&root, &["analyze", "nope"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "Error: Change 'nope' not found.\n"
    );
}
