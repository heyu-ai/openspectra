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
//!    requirement must state a normative `SHALL`/`MUST` and carry at least one
//!    `#### Scenario:` block. Without `--strict` these are not reported, so a
//!    non-strict run gates purely on structure (matching OSS, where `--strict`
//!    is what turns content-quality findings into hard failures).

use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::change;
use crate::config::Config;

/// A word-boundaried RFC 2119 keyword: `\bSHALL\b` matches "The system SHALL"
/// but not "MARSHALL", and case-sensitivity keeps a lowercase "shall" (prose,
/// not a normative clause) from counting.
static SHALL_OR_MUST_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(SHALL|MUST)\b").unwrap());

/// One validation finding. Field order is the serialized `--json` order and is
/// the documented contract the downstream gate parses (`level`/`path`/`message`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Issue {
    pub level: String,
    pub path: String,
    pub message: String,
}

impl Issue {
    fn error(path: String, message: String) -> Self {
        Issue {
            level: "ERROR".to_string(),
            path,
            message,
        }
    }
}

/// Per-change result. `valid` is `true` exactly when `issues` is empty.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChangeValidation {
    pub id: String,
    pub valid: bool,
    pub issues: Vec<Issue>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Totals {
    pub passed: usize,
    pub failed: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Summary {
    pub totals: Totals,
}

/// Top-level `--json` shape, matching the downstream gate's parser:
/// `{ "items": [...], "summary": { "totals": { "failed": N } } }`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ValidateReport {
    pub items: Vec<ChangeValidation>,
    pub summary: Summary,
}

impl ValidateReport {
    /// Whether any change failed validation. The CLI maps this to a non-zero
    /// exit so `validate` behaves like a linter/gate (unlike `drift`, whose
    /// exit code never encodes analysis severity).
    pub fn any_failed(&self) -> bool {
        self.summary.totals.failed > 0
    }
}

/// Validate every active (non-parked, non-archived) change.
pub fn validate_all_active(cfg: &Config, strict: bool) -> Result<ValidateReport> {
    let names = change::list_active(cfg);
    build_report(cfg, &names, strict)
}

/// Validate an explicit set of change names, assembling the summary totals.
pub fn build_report(cfg: &Config, names: &[String], strict: bool) -> Result<ValidateReport> {
    let mut items = Vec::with_capacity(names.len());
    for name in names {
        items.push(validate_change(cfg, name, strict)?);
    }
    let failed = items.iter().filter(|i| !i.valid).count();
    let total = items.len();
    let passed = total - failed;
    Ok(ValidateReport {
        items,
        summary: Summary {
            totals: Totals {
                passed,
                failed,
                total,
            },
        },
    })
}

/// Validate one change by name. Traverses `changes/<name>/specs/` recursively
/// (so nested `<Epic>/<Feature>/spec.md` layouts are covered), enforcing the
/// structural rule always and the content-quality rules only under `strict`.
pub fn validate_change(cfg: &Config, name: &str, strict: bool) -> Result<ChangeValidation> {
    let specs_root = cfg.changes_dir().join(name).join("specs");
    let files = crate::fsutil::collect_delta_specs(&specs_root)?;

    let mut issues = Vec::new();
    let mut total_operations = 0usize;
    for (cap, content) in &files {
        let parsed = parse_delta(content);
        total_operations += parsed.operations;
        if strict {
            for req in &parsed.body_reqs {
                if !req.has_shall_or_must {
                    issues.push(Issue::error(
                        spec_rel_path(cap),
                        format!(
                            "Requirement '{}' must state a normative SHALL or MUST",
                            req.name
                        ),
                    ));
                }
                if !req.has_scenario {
                    issues.push(Issue::error(
                        spec_rel_path(cap),
                        format!(
                            "Requirement '{}' must have at least one `#### Scenario:` block",
                            req.name
                        ),
                    ));
                }
            }
        }
    }
    if total_operations == 0 {
        issues.push(Issue::error(
            format!("changes/{name}"),
            "Change must contain at least one delta (an ADDED/MODIFIED/REMOVED/RENAMED \
             requirement under specs/**/spec.md)"
                .to_string(),
        ));
    }

    Ok(ChangeValidation {
        id: name.to_string(),
        valid: issues.is_empty(),
        issues,
    })
}

/// Human-readable relative path to a capability's delta spec, used in issue
/// `path` fields. `cap` is the capability id relative to the change's `specs/`
/// dir (e.g. `auth` or `Epic/Feature`); it is never empty, because
/// `fsutil::collect_delta_specs` rejects a stray `specs/spec.md` (no capability
/// directory) with a hard error upstream rather than yielding an empty id here.
fn spec_rel_path(cap: &str) -> String {
    format!("specs/{cap}/spec.md")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Added,
    Modified,
    Removed,
    Renamed,
    Other,
}

/// A requirement that carries a body (ADDED or MODIFIED), with the two
/// content-quality signals pre-computed from that body.
struct BodyRequirement {
    name: String,
    has_shall_or_must: bool,
    has_scenario: bool,
}

struct DeltaParse {
    /// Total delta operations: ADDED/MODIFIED/REMOVED requirement headers plus
    /// RENAMED `TO:` entries. Drives the "at least one delta" structural rule.
    operations: usize,
    body_reqs: Vec<BodyRequirement>,
}

/// A requirement block being accumulated line-by-line until its terminator
/// (the next `### Requirement:` or `## ` header). `has_scenario` is tracked
/// during the scan (not derived from `body` afterwards) so it can be made
/// fenced-code-block aware — a `#### Scenario:` line inside a ``` fence is code,
/// not a real scenario heading.
struct PendingReq {
    name: String,
    body: String,
    has_scenario: bool,
}

impl PendingReq {
    fn finish(self) -> BodyRequirement {
        BodyRequirement {
            // A normative keyword inside a fenced code block is an unusual but
            // harmless false-*pass* (it only relaxes the gate), so SHALL/MUST
            // stays a whole-body scan; only the false-*fail* case (a scenario
            // hidden by a fence) is made fence-aware via `has_scenario`.
            has_shall_or_must: SHALL_OR_MUST_RE.is_match(&self.body),
            has_scenario: self.has_scenario,
            name: self.name,
        }
    }
}

/// Whether `line` opens or closes a fenced code block (```` ``` ```` or `~~~`,
/// possibly indented under a list item). Used to suppress markdown-structure
/// matching inside code fences.
fn is_code_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// Classify a `## ` header line into a delta section, or `None` if it isn't a
/// level-2 header at all. Trailing whitespace is tolerated (matching archive's
/// `^## ADDED Requirements\s*$` regex); the header must start at column 0, as
/// all archive/OpenSpec structural headers do — a leading-indented `## ` is
/// treated as body, not a section boundary. An unrecognized `## ` header is
/// `Section::Other`, which ends any requirement block without counting.
fn parse_section_header(line: &str) -> Option<Section> {
    let trimmed = line.trim_end();
    if !trimmed.starts_with("## ") {
        return None;
    }
    Some(match trimmed {
        "## ADDED Requirements" => Section::Added,
        "## MODIFIED Requirements" => Section::Modified,
        "## REMOVED Requirements" => Section::Removed,
        "## RENAMED Requirements" => Section::Renamed,
        _ => Section::Other,
    })
}

fn parse_delta(content: &str) -> DeltaParse {
    let mut operations = 0usize;
    let mut body_reqs = Vec::new();
    let mut section = Section::Other;
    let mut pending: Option<PendingReq> = None;
    let mut in_fence = false;

    for line in content.lines() {
        // Fenced code inside a requirement body must not be parsed as markdown
        // structure: a `## foo` bash comment or a `#### Scenario:` in an example
        // is code, not a heading. The fence delimiter and its contents are still
        // accumulated into `body`, just never classified.
        if is_code_fence(line) {
            in_fence = !in_fence;
            if let Some(req) = pending.as_mut() {
                req.body.push_str(line);
                req.body.push('\n');
            }
            continue;
        }

        if !in_fence {
            if let Some(sec) = parse_section_header(line) {
                if let Some(req) = pending.take() {
                    body_reqs.push(req.finish());
                }
                section = sec;
                continue;
            }

            // Structural headers are matched at column 0 (like archive.rs's
            // `^### Requirement:` / `^#### ...` anchored regexes and the
            // OpenSpec convention). Only list bullets under RENAMED are
            // trim_start'd, since those are legitimately indented list items.
            if let Some(rest) = line.strip_prefix("### Requirement:") {
                if let Some(req) = pending.take() {
                    body_reqs.push(req.finish());
                }
                match section {
                    Section::Added | Section::Modified | Section::Removed => operations += 1,
                    Section::Renamed | Section::Other => {}
                }
                if matches!(section, Section::Added | Section::Modified) {
                    pending = Some(PendingReq {
                        name: rest.trim().to_string(),
                        body: String::new(),
                        has_scenario: false,
                    });
                }
                continue;
            }

            if section == Section::Renamed {
                let bullet = line.trim_start();
                if let Some(rest) = bullet
                    .strip_prefix('-')
                    .or_else(|| bullet.strip_prefix('*'))
                {
                    if rest.trim_start().starts_with("TO:") {
                        operations += 1;
                    }
                }
            }

            if line.starts_with("#### Scenario:") {
                if let Some(req) = pending.as_mut() {
                    req.has_scenario = true;
                }
            }
        }

        if let Some(req) = pending.as_mut() {
            req.body.push_str(line);
            req.body.push('\n');
        }
    }

    if let Some(req) = pending.take() {
        body_reqs.push(req.finish());
    }
    DeltaParse {
        operations,
        body_reqs,
    }
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

    #[test]
    fn parse_delta_counts_added_modified_removed_and_renamed_operations() {
        let delta = "## ADDED Requirements\n\n\
            ### Requirement: A\n\nThe system MUST do a.\n\n#### Scenario: s\n\n- **WHEN** x\n\n\
            ## MODIFIED Requirements\n\n\
            ### Requirement: B\n\nThe system SHALL do b.\n\n#### Scenario: s\n\n- **WHEN** y\n\n\
            ## REMOVED Requirements\n\n\
            ### Requirement: C\n\nDeprecated.\n\n\
            ## RENAMED Requirements\n\n\
            - FROM: `### Requirement: D`\n- TO: `### Requirement: E`\n";
        let parsed = parse_delta(delta);
        // A (added) + B (modified) + C (removed) + one rename TO = 4.
        assert_eq!(parsed.operations, 4);
        // Only ADDED/MODIFIED carry bodies to quality-check.
        assert_eq!(parsed.body_reqs.len(), 2);
    }

    #[test]
    fn parse_delta_ignores_requirement_headers_outside_delta_sections() {
        // A `### Requirement:` under a non-delta `## ` section (or with no
        // section at all) is not a delta operation.
        let delta = "## Some Prose Section\n\n### Requirement: Stray\n\nThe system SHALL x.\n";
        let parsed = parse_delta(delta);
        assert_eq!(parsed.operations, 0);
        assert!(parsed.body_reqs.is_empty());
    }

    #[test]
    fn parse_delta_flags_missing_shall_and_missing_scenario() {
        let delta = "## ADDED Requirements\n\n\
            ### Requirement: NoNormative\n\nThe system does a thing.\n";
        let parsed = parse_delta(delta);
        assert_eq!(parsed.body_reqs.len(), 1);
        assert!(!parsed.body_reqs[0].has_shall_or_must);
        assert!(!parsed.body_reqs[0].has_scenario);
    }

    #[test]
    fn parse_delta_ignores_markdown_structure_inside_a_fenced_code_block() {
        // Regression (mob review): a `## ` bash comment and a `#### Scenario:`
        // line inside a fenced code block are code, not markdown structure. The
        // requirement's real scenario (after the fence) must still be detected,
        // and the in-fence `## Run migrations` must NOT flush the requirement.
        let delta = "## ADDED Requirements\n\n\
            ### Requirement: Deploy\n\n\
            The system SHALL run migrations on deploy.\n\n\
            ```bash\n\
            ## Run migrations\n\
            #### Scenario: not-a-real-heading\n\
            ./migrate.sh\n\
            ```\n\n\
            #### Scenario: Migrations run\n\n\
            - **WHEN** deploy starts\n\
            - **THEN** migrations run\n";
        let parsed = parse_delta(delta);
        assert_eq!(
            parsed.operations, 1,
            "the fenced `## ` must not split the delta"
        );
        assert_eq!(parsed.body_reqs.len(), 1);
        assert!(
            parsed.body_reqs[0].has_scenario,
            "the real post-fence scenario must be detected"
        );
        assert!(parsed.body_reqs[0].has_shall_or_must);
    }

    #[test]
    fn parse_delta_matches_structural_headers_at_column_zero_only() {
        // Structural headers are matched at column 0, consistent with
        // archive.rs's anchored `^### Requirement:` regex and the OpenSpec
        // convention -- an indented `### Requirement:` is body text (e.g. an
        // example inside a scenario), not a new requirement.
        let delta = "## ADDED Requirements\n\n\
            ### Requirement: Real\n\n\
            The system MUST work.\n\n\
            #### Scenario: Real scenario\n\n\
            - **WHEN** x\n\
            - **THEN** y, e.g.\n    \
            ### Requirement: Not A Real One\n";
        let parsed = parse_delta(delta);
        assert_eq!(
            parsed.operations, 1,
            "the indented `### Requirement:` must not count as a second delta"
        );
        assert_eq!(parsed.body_reqs.len(), 1);
        assert!(parsed.body_reqs[0].has_scenario);
        assert!(parsed.body_reqs[0].has_shall_or_must);
    }

    #[test]
    fn parse_delta_shall_is_word_boundaried_and_case_sensitive() {
        // "MARSHALL" contains the substring "SHALL" but is not the keyword, and
        // a lowercase "shall" is prose, not a normative clause.
        let delta = "## ADDED Requirements\n\n\
            ### Requirement: R\n\nThe MARSHALL shall maybe do it.\n\n#### Scenario: s\n\n- **WHEN** x\n";
        let parsed = parse_delta(delta);
        assert!(!parsed.body_reqs[0].has_shall_or_must);
        assert!(parsed.body_reqs[0].has_scenario);
    }

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
    fn validate_change_missing_scenario_errors_only_under_strict() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write_delta(
            &c,
            "feat",
            "auth",
            "## ADDED Requirements\n\n### Requirement: Login\n\nThe system SHALL log in.\n",
        );

        // Structural rule satisfied (one ADDED requirement), so non-strict passes
        // even though the requirement has no scenario.
        let lenient = validate_change(&c, "feat", false).unwrap();
        assert!(lenient.valid, "non-strict must ignore the missing scenario");

        let strict = validate_change(&c, "feat", true).unwrap();
        assert!(!strict.valid);
        assert!(strict
            .issues
            .iter()
            .any(|i| i.message.contains("#### Scenario:")));
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
    fn report_json_shape_matches_the_downstream_gate_contract() {
        let report = ValidateReport {
            items: vec![ChangeValidation {
                id: "feat".to_string(),
                valid: false,
                issues: vec![Issue::error(
                    "specs/auth/spec.md".to_string(),
                    "boom".to_string(),
                )],
            }],
            summary: Summary {
                totals: Totals {
                    passed: 0,
                    failed: 1,
                    total: 1,
                },
            },
        };
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["items"][0]["id"], "feat");
        assert_eq!(value["items"][0]["valid"], false);
        assert_eq!(value["items"][0]["issues"][0]["level"], "ERROR");
        assert_eq!(value["items"][0]["issues"][0]["path"], "specs/auth/spec.md");
        assert_eq!(value["items"][0]["issues"][0]["message"], "boom");
        assert_eq!(value["summary"]["totals"]["failed"], 1);
    }
}
