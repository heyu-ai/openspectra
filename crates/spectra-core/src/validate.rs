//! `spectra validate` — OpenSpec change validation gate.
//!
//! Unlike `drift`/`archive`, this command is **not** reverse-engineered from
//! the closed-source macOS Spectra binary: Spectra is macOS-only and its
//! `validate` surface can't be probed from Linux CI, so there is no oracle to
//! calibrate against. Instead it matches the documented `@fission-ai/openspec`
//! (OSS 1.5.0) `validate` contract — deliberately *without* OSS's
//! nested-capability blind spot, so `specs/<Epic>/<Feature>/spec.md` layouts
//! validate correctly instead of being reported as "no deltas found". See
//! `docs/reverse-engineering/validate.md` for the rule set and rationale.
//!
//! Validation rules:
//!  - Structural (always an ERROR): a change must contain at least one
//!    requirement delta — an `### Requirement:` under an `## ADDED`/`##
//!    MODIFIED`/`## REMOVED` section, or a `- TO:` entry under `## RENAMED`,
//!    in any `specs/**/spec.md` beneath the change.
//!  - Content quality (an ERROR only under `--strict`): each ADDED/MODIFIED
//!    requirement must state a normative `SHALL`/`MUST` **in its first text
//!    block** (issue #80 — see `extract_requirement_text`) and carry at least
//!    one `#### Scenario:` block. Without `--strict` these are not reported, so
//!    a non-strict run gates purely on structure. (OSS fires the SHALL/MUST
//!    finding unconditionally, not just under `--strict`; the strict gating
//!    here is OpenSpectra's own choice — see `validate.md` "Known divergences".)

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::change;
use crate::config::Config;
use crate::fsutil::read_optional;

/// A word-boundaried RFC 2119 keyword: `\bSHALL\b` matches "The system SHALL"
/// but not "MARSHALL", and case-sensitivity keeps a lowercase "shall" (prose,
/// not a normative clause) from counting.
static SHALL_OR_MUST_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(SHALL|MUST)\b").unwrap());
static TASK_GROUP_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^ {0,3}##\s+(\d+)\.(?:\s|$)").unwrap());
static LEVEL_TWO_HEADING_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^ {0,3}##(?:\s|$)").unwrap());
static TASK_ID_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*[-*]\s*\[[ xX]\]\s*(\d+(?:\.\d+)+(?:[A-Za-z]+)?)(?:\s|$)").unwrap()
});

/// One validation finding. Existing field order remains level/path/message;
/// `line` is additive and omitted when the parser cannot ground it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Issue {
    pub level: String,
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
}

impl Issue {
    fn new(level: &str, path: String, message: String) -> Self {
        Self {
            level: level.to_string(),
            path,
            message,
            line: None,
        }
    }

    fn error(path: String, message: String) -> Self {
        Self::new("ERROR", path, message)
    }

    fn warning(path: String, message: String) -> Self {
        Self::new("WARNING", path, message)
    }

    fn info(path: String, message: String) -> Self {
        Self::new("INFO", path, message)
    }

    fn at_line(mut self, line: usize) -> Self {
        self.line = Some(line);
        self
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChangeValidation {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub valid: bool,
    pub issues: Vec<Issue>,
    #[serde(rename = "durationMs")]
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Totals {
    pub passed: usize,
    pub failed: usize,
    pub total: usize,
    pub items: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub totals: Totals,
    pub by_type: std::collections::BTreeMap<String, Totals>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RootInfo {
    pub path: String,
    pub spec_dir: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ValidateReport {
    pub items: Vec<ChangeValidation>,
    pub summary: Summary,
    pub version: String,
    pub root: RootInfo,
}

impl ValidateReport {
    pub fn any_failed(&self) -> bool {
        self.summary.totals.failed > 0
    }
}

pub fn validate_all_active(cfg: &Config, strict: bool) -> Result<ValidateReport> {
    let names = change::list_active(cfg);
    build_report(cfg, &names, strict)
}

pub fn build_report(cfg: &Config, names: &[String], strict: bool) -> Result<ValidateReport> {
    let mut items = Vec::with_capacity(names.len());
    for name in names {
        items.push(validate_change(cfg, name, strict)?);
    }
    Ok(report_from_items(cfg, items))
}

pub fn build_mixed_report(
    cfg: &Config,
    change_names: &[String],
    spec_names: &[String],
    strict: bool,
) -> Result<ValidateReport> {
    let mut items = Vec::with_capacity(change_names.len() + spec_names.len());
    for name in change_names {
        items.push(validate_change(cfg, name, strict)?);
    }
    for name in spec_names {
        items.push(validate_spec(cfg, name, strict)?);
    }
    Ok(report_from_items(cfg, items))
}

pub fn report_from_items(cfg: &Config, items: Vec<ChangeValidation>) -> ValidateReport {
    fn totals<'a>(items: impl Iterator<Item = &'a ChangeValidation>) -> Totals {
        let items: Vec<_> = items.collect();
        let failed = items.iter().filter(|item| !item.valid).count();
        let total = items.len();
        Totals {
            passed: total - failed,
            failed,
            total,
            items: total,
        }
    }

    let mut by_type = std::collections::BTreeMap::new();
    for item_type in ["change", "spec"] {
        let selected: Vec<_> = items
            .iter()
            .filter(|item| item.item_type == item_type)
            .collect();
        if !selected.is_empty() {
            by_type.insert(item_type.to_string(), totals(selected.into_iter()));
        }
    }
    ValidateReport {
        summary: Summary {
            totals: totals(items.iter()),
            by_type,
        },
        items,
        version: "2.0".to_string(),
        root: RootInfo {
            path: cfg.root.to_string_lossy().to_string(),
            spec_dir: cfg.spec_dir.clone(),
        },
    }
}

/// Validate one change by name. Traverses `changes/<name>/specs/` recursively
/// (so nested `<Epic>/<Feature>/spec.md` layouts are covered), enforcing the
/// structural rule always and the content-quality rules only under `strict`.
pub fn validate_change(cfg: &Config, name: &str, strict: bool) -> Result<ChangeValidation> {
    let started = std::time::Instant::now();
    let specs_root = cfg.changes_dir().join(name).join("specs");
    let files = crate::fsutil::collect_delta_specs(&specs_root)?;
    let skip_specs =
        change::try_load(cfg, name)?.and_then(|change| change.metadata.skip_specs) == Some(true);

    let mut issues = Vec::new();
    if skip_specs && !files.is_empty() {
        issues.push(Issue::error(
            ".openspec.yaml".to_string(),
            "Change declares skip_specs but also contains delta spec files".to_string(),
        ));
    } else if skip_specs {
        issues.push(Issue::info(
            ".openspec.yaml".to_string(),
            "Change declares skip_specs and contains no delta specs".to_string(),
        ));
    }
    let mut total_operations = 0usize;
    for (cap, content) in &files {
        let path = spec_rel_path(cap);
        let parsed = match crate::markdown::parse_delta(content) {
            Ok(parsed) => parsed,
            Err(error) => {
                issues.push(Issue::error(path, error.to_string()));
                continue;
            }
        };
        total_operations += parsed.added.len()
            + parsed.modified.len()
            + parsed.removed.len()
            + parsed.renamed.len();

        for req in parsed.added.iter().chain(&parsed.modified) {
            if req.text.is_empty() {
                issues.push(
                    Issue::error(
                        path.clone(),
                        format!("Requirement '{}' is missing requirement text", req.name),
                    )
                    .at_line(req.line),
                );
            } else if !SHALL_OR_MUST_RE.is_match(&req.text) {
                issues.push(
                    Issue::warning(
                        path.clone(),
                        format!(
                            "Requirement '{}' should state a normative SHALL or MUST",
                            req.name
                        ),
                    )
                    .at_line(req.line),
                );
            }
            if req.scenarios.is_empty() {
                issues.push(
                    Issue::error(
                        path.clone(),
                        format!(
                            "Requirement '{}' must have at least one `#### Scenario:` or other level-4 scenario block",
                            req.name
                        ),
                    )
                    .at_line(req.line),
                );
            }
        }

        let main_path = cfg.specs_dir().join(cap).join("spec.md");
        if let Ok(main_content) = std::fs::read_to_string(&main_path) {
            let current = crate::markdown::parse_main_requirements(&main_content);
            for modified in &parsed.modified {
                let base_name = parsed
                    .renamed
                    .iter()
                    .find(|rename| normalize_name(&rename.to) == normalize_name(&modified.name))
                    .map_or(modified.name.as_str(), |rename| rename.from.as_str());
                let Some(base) = current.iter().find(|requirement| {
                    normalize_name(&requirement.name) == normalize_name(base_name)
                }) else {
                    continue;
                };
                let missing = missing_scenarios(&base.scenarios, &modified.scenarios);
                if !missing.is_empty() {
                    issues.push(
                        Issue::error(
                            path.clone(),
                            format!(
                                "MODIFIED \"{}\" omits scenario(s) the current spec still has: {}",
                                modified.name,
                                missing
                                    .iter()
                                    .map(|scenario| format!("\"{scenario}\""))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        )
                        .at_line(modified.line),
                    );
                }
            }
        }
    }
    issues.extend(task_numbering_issues(
        &cfg.changes_dir().join(name).join("tasks.md"),
    )?);
    if total_operations == 0 && !skip_specs {
        issues.push(Issue::error(
            format!("changes/{name}"),
            "Change must contain at least one delta (an ADDED/MODIFIED/REMOVED/RENAMED \
             requirement under specs/**/spec.md)"
                .to_string(),
        ));
    }

    Ok(ChangeValidation {
        id: name.to_string(),
        item_type: "change".to_string(),
        valid: issues_are_valid(&issues, strict),
        issues,
        duration_ms: elapsed_ms(started),
    })
}

pub fn validate_spec(cfg: &Config, id: &str, strict: bool) -> Result<ChangeValidation> {
    let started = std::time::Instant::now();
    let spec = crate::spec::load(cfg, id)?;
    let content = std::fs::read_to_string(spec.spec_md())?;
    let mut issues = Vec::new();
    let purpose = crate::markdown::parse_main_purpose(&content);
    match &purpose {
        None => issues.push(Issue::error(
            "spec.md".to_string(),
            "Spec must contain a non-empty ## Purpose section".to_string(),
        )),
        Some(purpose) if crate::markdown::is_placeholder_purpose(purpose) => {
            issues.push(Issue::warning(
                "spec.md".to_string(),
                "Purpose is still a TBD/TODO placeholder".to_string(),
            ));
        }
        Some(_) => {}
    }
    let requirements = crate::markdown::parse_main_requirements(&content);
    if requirements.is_empty() {
        issues.push(Issue::error(
            "spec.md".to_string(),
            "Spec must contain at least one requirement under ## Requirements".to_string(),
        ));
    }
    for requirement in requirements {
        if requirement.text.is_empty() {
            issues.push(
                Issue::error(
                    "spec.md".to_string(),
                    format!(
                        "Requirement '{}' is missing requirement text",
                        requirement.name
                    ),
                )
                .at_line(requirement.line),
            );
        } else if !SHALL_OR_MUST_RE.is_match(&requirement.text) {
            issues.push(
                Issue::warning(
                    "spec.md".to_string(),
                    format!(
                        "Requirement '{}' should state a normative SHALL or MUST",
                        requirement.name
                    ),
                )
                .at_line(requirement.line),
            );
        }
        if requirement.scenarios.is_empty() {
            issues.push(
                Issue::error(
                    "spec.md".to_string(),
                    format!("Requirement '{}' must include a scenario", requirement.name),
                )
                .at_line(requirement.line),
            );
        }
    }
    Ok(ChangeValidation {
        id: id.to_string(),
        item_type: "spec".to_string(),
        valid: issues_are_valid(&issues, strict),
        issues,
        duration_ms: elapsed_ms(started),
    })
}

pub fn validate_archived(cfg: &Config) -> Result<ValidateReport> {
    let archive_dir = cfg.changes_dir().join("archive");
    let entries = match std::fs::read_dir(&archive_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(report_from_items(cfg, Vec::new()));
        }
        Err(error) => {
            return Err(error).with_context(|| format!("reading {}", archive_dir.display()));
        }
    };
    let mut dirs = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    dirs.sort();
    let mut items = Vec::new();
    for dir in dirs {
        let started = std::time::Instant::now();
        if !dir.is_dir() {
            continue;
        }

        let id = dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();
        let tasks = read_optional(&dir.join("tasks.md"))?
            .map(|content| crate::tasks::parse(&content))
            .unwrap_or_default();
        let incomplete = tasks.iter().filter(|task| !task.done).count();
        let issues = if incomplete == 0 {
            Vec::new()
        } else {
            vec![Issue::error(
                "tasks.md".to_string(),
                format!("{incomplete} incomplete archived task(s)"),
            )]
        };
        items.push(ChangeValidation {
            id,
            item_type: "change".to_string(),
            valid: issues.is_empty(),
            issues,
            duration_ms: elapsed_ms(started),
        });
    }
    Ok(report_from_items(cfg, items))
}
fn elapsed_ms(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn task_numbering_issues(path: &std::path::Path) -> Result<Vec<Issue>> {
    let Some(content) = read_optional(path)? else {
        return Ok(Vec::new());
    };
    let mut issues = Vec::new();
    let mut current_group: Option<String> = None;
    let mut first_by_id = std::collections::HashMap::new();
    for (index, line) in content.lines().enumerate() {
        if LEVEL_TWO_HEADING_RE.is_match(line) {
            current_group = TASK_GROUP_RE
                .captures(line)
                .map(|captures| captures[1].trim_start_matches('0').to_string());
        }
        let Some(captures) = TASK_ID_RE.captures(line) else {
            continue;
        };
        let id = captures[1].to_string();
        let line_number = index + 1;
        if let Some(group) = &current_group {
            let task_group = id
                .split('.')
                .next()
                .unwrap_or_default()
                .trim_start_matches('0');
            if task_group != group {
                issues.push(
                    Issue::warning(
                        "tasks.md".to_string(),
                        format!("Task \"{id}\" is under group {group}, but points to group {task_group}"),
                    )
                    .at_line(line_number),
                );
            }
        }
        if let Some(first) = first_by_id.insert(id.clone(), line_number) {
            issues.push(
                Issue::warning(
                    "tasks.md".to_string(),
                    format!("Task ID \"{id}\" is duplicated; first declared on line {first}"),
                )
                .at_line(line_number),
            );
        }
    }
    Ok(issues)
}

fn issues_are_valid(issues: &[Issue], strict: bool) -> bool {
    !issues
        .iter()
        .any(|issue| issue.level == "ERROR" || (strict && issue.level == "WARNING"))
}

fn normalize_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn missing_scenarios(current: &[String], modified: &[String]) -> Vec<String> {
    let mut remaining = modified.to_vec();
    let mut missing = Vec::new();
    for scenario in current {
        if let Some(index) = remaining.iter().position(|candidate| candidate == scenario) {
            remaining.remove(index);
        } else {
            missing.push(scenario.clone());
        }
    }
    missing
}

/// Human-readable relative path to a capability's delta spec, used in issue
/// `path` fields. `cap` is the capability id relative to the change's `specs/`
/// dir (e.g. `auth` or `Epic/Feature`); it is never empty, because
/// `fsutil::collect_delta_specs` rejects a stray `specs/spec.md` (no capability
/// directory) with a hard error upstream rather than yielding an empty id here.
fn spec_rel_path(cap: &str) -> String {
    format!("specs/{cap}/spec.md")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    const GOOD_ADDED: &str = "## ADDED Requirements\n\n\
        ### Requirement: Login\n\n\
        The system SHALL authenticate users.\n\n\
        #### Scenario: Valid credentials\n\n\
        - **WHEN** a user submits valid credentials\n\
        - **THEN** they are logged in\n";

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "spectra-validate-test-{}-{seq}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
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

    fn cfg(tmp: &TempDir) -> Config {
        Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        }
    }

    fn write_delta(cfg: &Config, change: &str, cap_path: &str, content: &str) {
        let mut dir = cfg.changes_dir().join(change).join("specs");
        for part in cap_path.split('/') {
            dir = dir.join(part);
        }
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("spec.md"), content).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn validate_change_does_not_follow_a_symlink_cycle_under_specs() {
        // Regression (mob review, all 3 voices): the recursive walk must not
        // follow directory symlinks, or a checked-in cycle (`specs/loop -> .`)
        // recurses without bound -> stack overflow, crashing the gate. With
        // symlink-not-following descent this terminates cleanly and still finds
        // the real `auth` delta. (If this ever regresses it stack-overflows the
        // test process rather than failing an assertion -- which is exactly the
        // crash we are guarding against.)
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write_delta(&c, "feat", "auth", GOOD_ADDED);
        let specs_root = c.changes_dir().join("feat").join("specs");
        // A directory symlink pointing back at its own parent: the classic
        // walk cycle.
        std::os::unix::fs::symlink(&specs_root, specs_root.join("loop")).unwrap();

        let result = validate_change(&c, "feat", true).unwrap();
        assert!(
            result.valid,
            "the real delta must still be found; got: {:?}",
            result.issues
        );
    }

    #[test]
    fn validate_change_passes_a_well_formed_added_delta() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write_delta(&c, "feat", "auth", GOOD_ADDED);

        let result = validate_change(&c, "feat", true).unwrap();
        assert!(
            result.valid,
            "expected valid, got issues: {:?}",
            result.issues
        );
        assert!(result.issues.is_empty());
    }

    #[test]
    fn validate_change_errors_when_no_deltas_exist() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        // A change directory with no specs/ at all.
        fs::create_dir_all(c.changes_dir().join("empty")).unwrap();

        let result = validate_change(&c, "empty", true).unwrap();
        assert!(!result.valid);
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].level, "ERROR");
        assert!(result.issues[0].message.contains("at least one delta"));
    }

    #[test]
    fn validate_change_missing_scenario_is_always_an_error() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write_delta(
            &c,
            "feat",
            "auth",
            "## ADDED Requirements\n\n### Requirement: Login\n\nThe system SHALL log in.\n",
        );

        let lenient = validate_change(&c, "feat", false).unwrap();
        assert!(!lenient.valid);
        assert!(lenient
            .issues
            .iter()
            .any(|issue| issue.message.contains("#### Scenario:")));

        let strict = validate_change(&c, "feat", true).unwrap();
        assert!(!strict.valid);
        assert!(strict
            .issues
            .iter()
            .any(|issue| issue.message.contains("#### Scenario:")));
    }

    #[test]
    fn validate_change_traverses_nested_epic_feature_layouts() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        // A nested `specs/<Epic>/<Feature>/spec.md` — the layout OSS reports as
        // "no deltas found". A good delta lives two levels deep.
        write_delta(&c, "feat", "Billing/Invoices", GOOD_ADDED);

        let result = validate_change(&c, "feat", true).unwrap();
        assert!(
            result.valid,
            "nested capability delta must be discovered, got: {:?}",
            result.issues
        );
    }

    #[test]
    fn validate_change_reports_nested_path_in_issue() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write_delta(
            &c,
            "feat",
            "Billing/Invoices",
            "## ADDED Requirements\n\n### Requirement: Bad\n\nNo normative keyword.\n",
        );

        let result = validate_change(&c, "feat", true).unwrap();
        assert!(!result.valid);
        assert!(
            result
                .issues
                .iter()
                .all(|i| i.path == "specs/Billing/Invoices/spec.md"),
            "issue path must name the nested capability, got: {:?}",
            result.issues
        );
    }

    #[test]
    fn build_report_summary_totals_reflect_pass_and_fail() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write_delta(&c, "good", "auth", GOOD_ADDED);
        fs::create_dir_all(c.changes_dir().join("bad")).unwrap();

        let report = build_report(&c, &["good".to_string(), "bad".to_string()], true).unwrap();
        assert_eq!(report.summary.totals.total, 2);
        assert_eq!(report.summary.totals.passed, 1);
        assert_eq!(report.summary.totals.failed, 1);
        assert!(report.any_failed());
    }

    #[test]
    fn task_numbering_warnings_fail_only_in_strict_mode() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write_delta(&c, "feat", "auth", GOOD_ADDED);
        fs::write(
            c.changes_dir().join("feat/tasks.md"),
            "## 1. Group\n- [ ] 2.1 wrong group\n- [ ] 2.1 duplicate\n",
        )
        .unwrap();

        let normal = validate_change(&c, "feat", false).unwrap();
        assert!(normal.valid);
        assert!(normal.issues.iter().all(|issue| issue.level == "WARNING"));

        let strict = validate_change(&c, "feat", true).unwrap();
        assert!(!strict.valid);
        assert!(strict
            .issues
            .iter()
            .any(|issue| issue.message.contains("duplicated") && issue.line == Some(3)));
    }

    #[test]
    fn report_json_shape_matches_the_downstream_gate_contract() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let report = report_from_items(
            &c,
            vec![ChangeValidation {
                id: "feat".to_string(),
                item_type: "change".to_string(),
                valid: false,
                issues: vec![Issue::error(
                    "specs/auth/spec.md".to_string(),
                    "boom".to_string(),
                )],
                duration_ms: 0,
            }],
        );
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["items"][0]["id"], "feat");
        assert_eq!(value["items"][0]["valid"], false);
        assert_eq!(value["items"][0]["issues"][0]["level"], "ERROR");
        assert_eq!(value["items"][0]["issues"][0]["path"], "specs/auth/spec.md");
        assert_eq!(value["items"][0]["issues"][0]["message"], "boom");
        assert_eq!(value["summary"]["totals"]["failed"], 1);
    }
}
