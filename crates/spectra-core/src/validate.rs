//! `spectra validate` — validation gate whose rules follow OpenSpec 1.13.2.
//!
//! Unlike `drift`, this command is not reverse-engineered from the closed
//! Spectra binary. Owner ruling D1: which findings exist, their level, the
//! per-item verdict, and the message wording follow OpenSpec 1.13.2
//! (`@fission-ai/openspec`, `dist/core/validation/validator.js`); the Markdown
//! reading it relies on is ported in [`crate::openspec_md`]. See
//! `docs/reverse-engineering/validate.md`.
//!
//! Changes and canonical specs may be validated directly or in bulk;
//! archived validation checks incomplete tasks. Errors always fail, warnings
//! fail only under `--strict`, and informational findings never fail.
//!
//! Two output shapes exist (ruling D3, W9a): the oracle 3.0.0 shape
//! ([`oracle_item`], the default) and the OpenSpec 1.13.2 `--json` report
//! ([`openspec_report`], `--format openspec`). Both are built from the same
//! [`ChangeValidation`] list; only the presentation differs.

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

use crate::change;
use crate::config::Config;
use crate::fsutil::read_optional;
use crate::openspec_md;

/// One validation finding, serialized in OpenSpec's key order: `level`,
/// `path`, then `line` when the rule grounds one, then `message`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Issue {
    pub level: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    pub message: String,
    /// The finding belongs to a delta file under the change's `specs/` (its
    /// `path` is relative to that directory). The oracle format prefixes such
    /// messages with `specs/<path>: ` (ruling D12-2); OpenSpec's JSON has no
    /// such field.
    #[serde(skip)]
    pub delta_file: bool,
}

impl Issue {
    fn new(level: &str, path: String, message: String) -> Self {
        Self {
            level: level.to_string(),
            path,
            line: None,
            message,
            delta_file: false,
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

/// OpenSpec 1.13.2's `{items, passed, failed}` counters, in that key order.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Totals {
    pub items: usize,
    pub passed: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub totals: Totals,
    /// One entry per **requested** type, even when it has no item
    /// (`commands/validate.js` `runBulkValidation`); `change` sorts before
    /// `spec`, matching OpenSpec's insertion order.
    pub by_type: std::collections::BTreeMap<String, Totals>,
}

/// `toRootOutput` (`core/root-selection.js:297-303`). OpenSpectra finds its
/// root by walking up to the nearest `.spectra.yaml`, which is OpenSpec's
/// `nearest` source; store roots do not exist here.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RootInfo {
    pub path: String,
    pub source: String,
}

/// The OpenSpec 1.13.2 `validate --json` report (`--format openspec`).
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

/// `projectValidationFindings`: `--report findings` keeps the items that
/// carry any finding (INFO included) and the full-run summary.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FindingsReport {
    pub report: FindingsMeta,
    pub item_findings: Vec<ChangeValidation>,
    pub summary: Summary,
    pub root: RootInfo,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FindingsMeta {
    pub kind: String,
    pub version: String,
    pub scope: String,
    pub returned_items: usize,
    pub total_items: usize,
}

/// Validates every active change, as the OpenSpec report.
pub fn validate_all_active(cfg: &Config, strict: bool) -> Result<ValidateReport> {
    let items = validate_items(cfg, &change::list_active(cfg), &[], strict)?;
    Ok(openspec_report(cfg, &items, &["change"]))
}

/// Validates the named changes, then the named specs, in the given order.
pub fn validate_items(
    cfg: &Config,
    change_names: &[String],
    spec_names: &[String],
    strict: bool,
) -> Result<Vec<ChangeValidation>> {
    let mut items = Vec::with_capacity(change_names.len() + spec_names.len());
    for name in change_names {
        items.push(validate_change(cfg, name, strict)?);
    }
    for name in spec_names {
        items.push(validate_spec(cfg, name, strict)?);
    }
    Ok(items)
}

/// Builds the OpenSpec 1.13.2 report. `requested_types` are the scopes asked
/// for (`change`, `spec`): each gets a `byType` entry even when empty. Items
/// are ordered like OpenSpec's `results.sort((a, b) => a.id.localeCompare(b.id))`
/// ([`locale_compare`]); the sort is stable, so a change and a spec sharing an
/// id keep the input order (OpenSpec orders that tie by async completion,
/// which is not deterministic).
pub fn openspec_report(
    cfg: &Config,
    items: &[ChangeValidation],
    requested_types: &[&str],
) -> ValidateReport {
    fn totals<'a>(items: impl Iterator<Item = &'a ChangeValidation>) -> Totals {
        let (mut passed, mut failed) = (0, 0);
        for item in items {
            if item.valid {
                passed += 1;
            } else {
                failed += 1;
            }
        }
        Totals {
            items: passed + failed,
            passed,
            failed,
        }
    }

    let mut sorted = items.to_vec();
    sorted.sort_by(|left, right| locale_compare(&left.id, &right.id));
    let by_type = requested_types
        .iter()
        .map(|item_type| {
            (
                item_type.to_string(),
                totals(sorted.iter().filter(|item| item.item_type == *item_type)),
            )
        })
        .collect();
    ValidateReport {
        summary: Summary {
            totals: totals(sorted.iter()),
            by_type,
        },
        items: sorted,
        version: "1.0".to_string(),
        root: RootInfo {
            path: cfg.root.to_string_lossy().to_string(),
            source: "nearest".to_string(),
        },
    }
}

/// `projectValidationFindings` over a full report; `scope` is `all`,
/// `changes`, `specs`, or `archived`.
pub fn findings_report(full: ValidateReport, scope: &str) -> FindingsReport {
    let total_items = full.summary.totals.items;
    let item_findings: Vec<ChangeValidation> = full
        .items
        .into_iter()
        .filter(|item| !item.issues.is_empty())
        .collect();
    FindingsReport {
        report: FindingsMeta {
            kind: "validation-findings".to_string(),
            version: "1.0".to_string(),
            scope: scope.to_string(),
            returned_items: item_findings.len(),
            total_items,
        },
        item_findings,
        summary: full.summary,
        root: full.root,
    }
}

/// One item in the oracle 3.0.0 shape: `{change|spec, errors, valid,
/// warnings}`, keys alphabetical (W9 RE spec A3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleItem {
    pub name: String,
    pub is_spec: bool,
    pub valid: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl Serialize for OracleItem {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(4))?;
        if !self.is_spec {
            map.serialize_entry("change", &self.name)?;
        }
        map.serialize_entry("errors", &self.errors)?;
        if self.is_spec {
            map.serialize_entry("spec", &self.name)?;
        }
        map.serialize_entry("valid", &self.valid)?;
        map.serialize_entry("warnings", &self.warnings)?;
        map.end()
    }
}

/// The INFO that ruling D12-1 keeps: archive's merge would refuse the delta,
/// which the oracle itself reports as a warning.
const ARCHIVE_REFUSAL_PREFIX: &str = "Archive would refuse this delta: ";

/// Maps one item into the oracle shape (ruling D12): ERROR → `errors`,
/// WARNING → `warnings`, INFO → `warnings` only when it is an archive refusal
/// (every other INFO is dropped). Messages keep OpenSpec's wording; a
/// delta-file finding gets the `specs/<path>: ` prefix the oracle uses. The
/// verdict is the item's own (so `--strict` still fails on warnings).
pub fn oracle_item(item: &ChangeValidation) -> OracleItem {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for issue in &item.issues {
        let bucket = match issue.level.as_str() {
            "ERROR" => &mut errors,
            "WARNING" => &mut warnings,
            _ if issue.message.starts_with(ARCHIVE_REFUSAL_PREFIX) => &mut warnings,
            _ => continue,
        };
        bucket.push(if issue.delta_file {
            format!("specs/{}: {}", issue.path, issue.message)
        } else {
            issue.message.clone()
        });
    }
    OracleItem {
        name: item.id.clone(),
        is_spec: item.item_type == "spec",
        valid: item.valid,
        errors,
        warnings,
    }
}

/// `String.prototype.localeCompare` as Node 22 (ICU 78, CLDR root collation,
/// alternate = non-ignorable) orders the ids OpenSpec sorts: punctuation and
/// symbols first in CLDR order, then digits, then letters compared
/// case-insensitively; only when every primary weight ties does case decide,
/// lowercase first. Exact for printable ASCII (checked against
/// `node -e` over all 95 characters); any other character sorts after `z` by
/// code point, which is an approximation (no corpus has non-ASCII ids).
pub fn locale_compare(left: &str, right: &str) -> std::cmp::Ordering {
    const ASCII_ORDER: &str =
        " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789abcdefghijklmnopqrstuvwxyz";
    fn primary(ch: char) -> u32 {
        let lower = ch.to_ascii_lowercase();
        match ASCII_ORDER.find(lower) {
            Some(index) if ch.is_ascii() => index as u32,
            _ => 0x100 + ch as u32,
        }
    }
    fn tertiary(ch: char) -> u8 {
        u8::from(ch.is_ascii_uppercase())
    }
    left.chars()
        .map(primary)
        .cmp(right.chars().map(primary))
        .then_with(|| left.chars().map(tertiary).cmp(right.chars().map(tertiary)))
}

/// `nearestMatches` (`utils/match.js`): the `max` candidates with the
/// smallest Levenshtein distance over UTF-16 code units, ties in candidate
/// order (the sort is stable).
pub fn nearest_matches(input: &str, candidates: &[String], max: usize) -> Vec<String> {
    fn levenshtein(a: &[u16], b: &[u16]) -> usize {
        let mut previous: Vec<usize> = (0..=b.len()).collect();
        for (i, left) in a.iter().enumerate() {
            let mut current = vec![i + 1; b.len() + 1];
            for (j, right) in b.iter().enumerate() {
                let cost = usize::from(left != right);
                current[j + 1] = (previous[j + 1] + 1)
                    .min(current[j] + 1)
                    .min(previous[j] + cost);
            }
            previous = current;
        }
        previous[b.len()]
    }
    let input: Vec<u16> = input.encode_utf16().collect();
    let mut scored: Vec<(usize, &String)> = candidates
        .iter()
        .map(|candidate| {
            let units: Vec<u16> = candidate.encode_utf16().collect();
            (levenshtein(&input, &units), candidate)
        })
        .collect();
    scored.sort_by_key(|(distance, _)| *distance);
    scored
        .into_iter()
        .take(max)
        .map(|(_, candidate)| candidate.clone())
        .collect()
}

/// 驗證一個 change：逐條移植 OpenSpec 1.13.2 的
/// `Validator.validateChangeDeltaSpecs`（`dist/core/validation/validator.js`
/// 123-455），連 issue 的順序、path、line 都照搬（決策 D1）。OpenSpec 沒有的
/// 規則不報，唯一例外是 OpenSpectra 獨有的 archive 拒絕理由（trace footer、
/// trace sidecar、capability retirement），依 owner 裁決 D12-6 仍是 ERROR（C20）。
pub fn validate_change(cfg: &Config, name: &str, strict: bool) -> Result<ChangeValidation> {
    let started = std::time::Instant::now();
    let change_dir = cfg.changes_dir().join(name);
    let specs_dir = change_dir.join("specs");
    let loaded_change = change::try_load(cfg, name)?;
    let skip_specs = loaded_change
        .as_ref()
        .and_then(|change| change.metadata.skip_specs)
        == Some(true);
    let retirement_declared = loaded_change
        .as_ref()
        .and_then(|change| change.metadata.retire_capabilities)
        == Some(true);

    let mut issues = Vec::new();
    let discovered = discover_spec_files(&specs_dir)?;
    // 只有「檔案」才算：名為 spec.md 的目錄是一般的 capability 目錄（validator.js:142-150）。
    let has_root_level_spec = std::fs::metadata(specs_dir.join("spec.md"))
        .map(|metadata| metadata.is_file())
        .unwrap_or(false);
    if has_root_level_spec {
        issues.push(Issue::error(
            "spec.md".to_string(),
            "Delta spec found at specs/spec.md. Delta specs must live under a capability path \
             (e.g. specs/<capability-path>/spec.md) — a file at the specs/ root is ignored when \
             the change is applied or archived."
                .to_string(),
        ));
    }

    let mut total_deltas = 0usize;
    let mut missing_header_specs = Vec::new();
    let mut empty_section_specs: Vec<(String, Vec<&'static str>)> = Vec::new();
    let mut plans = Vec::new();
    for (id, file) in &discovered {
        let Ok(bytes) = std::fs::read(file) else {
            continue;
        };
        let plan = openspec_md::parse_delta_spec(&String::from_utf8_lossy(&bytes));
        let entry_path = format!("{id}/spec.md");
        if plan.entry_count() == 0 {
            let sections = present_section_names(&plan);
            if sections.is_empty() {
                missing_header_specs.push(entry_path.clone());
            } else {
                empty_section_specs.push((entry_path.clone(), sections));
            }
        }
        total_deltas += plan.entry_count();
        check_delta_file(cfg, id, &plan, &entry_path, &mut issues)?;
        plans.push((id.clone(), entry_path, plan));
    }

    // 已經有 ERROR 的 delta 檔不再以 archive 的措辭重報一次（validator.js:366-377、
    // 825-829）；其餘 archive 會拒絕的情況只是 INFO，不影響判定（C1、C3、C13）。
    let already_reported: std::collections::HashSet<String> = issues
        .iter()
        .filter(|issue| issue.level == "ERROR")
        .map(|issue| issue.path.clone())
        .chain(missing_header_specs.iter().cloned())
        .chain(empty_section_specs.iter().map(|(path, _)| path.clone()))
        .collect();
    for (id, entry_path, plan) in &plans {
        if already_reported.contains(entry_path) {
            continue;
        }
        if let Some(message) = archive_refusal(cfg, id, plan) {
            issues.push(Issue::info(
                entry_path.clone(),
                format!("Archive would refuse this delta: {message}"),
            ));
        }
    }
    if !skip_specs {
        if let Err(error) = crate::archive::validate_archive_compatibility(
            cfg,
            &change_dir,
            name,
            retirement_declared,
        ) {
            // OpenSpec 有對應規則的 archive 衝突上面已依 OpenSpec 報過；這裡只留
            // OpenSpectra 獨有的拒絕理由。
            if error
                .downcast_ref::<crate::archive::OpenSpectraOnlyRefusal>()
                .is_some()
            {
                issues.push(Issue::error(format!("changes/{name}"), error.to_string()));
            }
        }
    }

    let unread_delta_files = find_unread_delta_files(&specs_dir)?;
    for (path, expected) in &unread_delta_files {
        issues.push(Issue::error(
            path.clone(),
            format!(
                "Delta spec found at specs/{path}. Delta specs must be a spec.md inside a \
                 capability folder — this file is ignored when the change is applied or \
                 archived. Move its requirements into specs/{expected}."
            ),
        ));
    }
    for (path, sections) in &empty_section_specs {
        issues.push(Issue::error(
            path.clone(),
            format!(
                "Delta sections {} were found, but no requirement entries parsed. Ensure each \
                 section includes at least one \"### Requirement:\" block (REMOVED may use \
                 bullet list syntax).",
                format_section_list(sections)
            ),
        ));
    }
    for path in &missing_header_specs {
        issues.push(Issue::error(
            path.clone(),
            "No delta sections found. Add headers such as \"## ADDED Requirements\" or move \
             non-delta notes outside specs/."
                .to_string(),
        ));
    }
    // 宣告 skip_specs 時，specs/ 底下任何非 dot 檔都與宣告矛盾；讀不到 specs/
    // 視同有檔案（fail closed，validator.js:427-436）。
    let specs_dir_has_files = skip_specs && has_any_file_under(&specs_dir).unwrap_or(true);
    if specs_dir_has_files {
        issues.push(Issue::error(
            "file".to_string(),
            CHANGE_SKIP_SPECS_CONFLICT.to_string(),
        ));
    }
    // root spec.md 與非 spec.md 的 delta 檔已經說明了錯在哪，不再疊一個「沒有 delta」。
    if total_deltas == 0 && !has_root_level_spec && unread_delta_files.is_empty() {
        if skip_specs && !specs_dir_has_files {
            issues.push(Issue::info(
                "file".to_string(),
                CHANGE_SKIP_SPECS_ACCEPTED.to_string(),
            ));
        } else if !skip_specs {
            issues.push(Issue::error(
                "file".to_string(),
                format!("Change must have at least one delta. {GUIDE_NO_DELTAS}"),
            ));
        }
    }
    // 到這裡為止，path 相對於 change 的 specs/ 的 finding 都屬於某個 delta 檔
    // （根目錄的 spec.md、非 spec.md 的 delta 檔也算）；task 檔的 finding 在下面才加，
    // path 是相對 change 目錄，不能混進來。
    let delta_paths: std::collections::HashSet<&str> = plans
        .iter()
        .map(|(_, entry_path, _)| entry_path.as_str())
        .chain(has_root_level_spec.then_some("spec.md"))
        .chain(unread_delta_files.iter().map(|(path, _)| path.as_str()))
        .collect();
    for issue in &mut issues {
        issue.delta_file = delta_paths.contains(issue.path.as_str());
    }
    issues.extend(task_file_issues(cfg, &change_dir, loaded_change.as_ref())?);

    Ok(ChangeValidation {
        id: name.to_string(),
        item_type: "change".to_string(),
        valid: issues_are_valid(&issues, strict),
        issues,
        duration_ms: elapsed_ms(started),
    })
}

/// 驗證一份 main spec：移植 `Validator.validateSpec`（validator.js:24-47）。
/// `MarkdownParser.parseSpec` 找不到 Purpose／Requirements 時只報那一個 ERROR，
/// 其他規則都不跑（S7、S8）；否則先報 Zod schema 的 ERROR，再跑 `applySpecRules`。
pub fn validate_spec(cfg: &Config, id: &str, strict: bool) -> Result<ChangeValidation> {
    let started = std::time::Instant::now();
    let spec = crate::spec::load(cfg, id)?;
    let spec_path = spec.spec_md();
    let bytes =
        std::fs::read(&spec_path).with_context(|| format!("reading {}", spec_path.display()))?;
    let content = String::from_utf8_lossy(&bytes);
    let issues = match openspec_md::parse_spec(&content) {
        Err(message) => vec![Issue::error(
            "file".to_string(),
            format!("{message}. {GUIDE_MISSING_SPEC_SECTIONS}"),
        )],
        Ok(parsed) => spec_issues(&parsed, &content),
    };
    Ok(ChangeValidation {
        id: id.to_string(),
        item_type: "spec".to_string(),
        valid: issues_are_valid(&issues, strict),
        issues,
        duration_ms: elapsed_ms(started),
    })
}

fn spec_issues(spec: &openspec_md::ParsedSpec, content: &str) -> Vec<Issue> {
    let mut issues = Vec::new();
    // Zod（spec.schema.js、base.schema.js）：requirements.min(1)、每個 requirement 的
    // text.min(1) 與 scenarios.min(1)。path 是 Zod 的 `a.0.b` 寫法。
    if spec.requirements.is_empty() {
        issues.push(Issue::error(
            "requirements".to_string(),
            "Spec must have at least one requirement".to_string(),
        ));
    }
    for (index, requirement) in spec.requirements.iter().enumerate() {
        if requirement.text.is_empty() {
            issues.push(Issue::error(
                format!("requirements.{index}.text"),
                "Requirement text cannot be empty".to_string(),
            ));
        }
        if requirement.scenarios == 0 {
            issues.push(Issue::error(
                format!("requirements.{index}.scenarios"),
                "Requirement must have at least one scenario".to_string(),
            ));
        }
    }
    // applySpecRules（validator.js:672-743）
    for structural in openspec_md::main_spec_structure_issues(content) {
        issues.push(Issue::error("file".to_string(), structural.message).at_line(structural.line));
    }
    // placeholder 本身長於 50 字，所以兩條只會中一條；placeholder 優先（S9、S10）。
    if let Some(line) = openspec_md::purpose_placeholder(&spec.overview, content) {
        let issue = Issue::warning("overview".to_string(), PURPOSE_IS_PLACEHOLDER.to_string());
        issues.push(match line {
            Some(line) => issue.at_line(line),
            None => issue,
        });
    } else if openspec_md::js_len(&spec.overview) < MIN_PURPOSE_LENGTH {
        issues.push(Issue::warning(
            "overview".to_string(),
            format!("Purpose section is too brief (less than {MIN_PURPOSE_LENGTH} characters)"),
        ));
    }
    for (index, requirement) in spec.requirements.iter().enumerate() {
        if openspec_md::js_len(&requirement.text) > MAX_REQUIREMENT_TEXT_LENGTH {
            issues.push(Issue::info(
                format!("requirements[{index}]"),
                format!(
                    "Requirement text is very long (>{MAX_REQUIREMENT_TEXT_LENGTH} characters). \
                     Consider breaking it down."
                ),
            ));
        }
        if requirement.scenarios == 0 {
            issues.push(Issue::warning(
                format!("requirements[{index}].scenarios"),
                format!("Requirement must have at least one scenario. {GUIDE_SCENARIO_FORMAT}"),
            ));
        }
    }
    // SHALL/MUST 只看第一個 `## Requirements` 底下的 `### Requirement:` block（S12）。
    for (index, block) in openspec_md::requirements_section_blocks(content)
        .iter()
        .enumerate()
    {
        let label = format!("Requirement \"{}\"", block.name);
        let text = openspec_md::block_requirement_text(&block.raw);
        if text.is_empty() {
            issues.push(Issue::error(
                format!("requirements[{index}]"),
                missing_shall_or_must_message(&label, &block.name, false),
            ));
        } else if !openspec_md::contains_shall_or_must(&text) {
            issues.push(Issue::warning(
                format!("requirements[{index}]"),
                missing_shall_or_must_message(&label, &block.name, true),
            ));
        }
    }
    issues
}

const MIN_PURPOSE_LENGTH: usize = 50;
const MAX_REQUIREMENT_TEXT_LENGTH: usize = 500;
const CHANGE_SKIP_SPECS_CONFLICT: &str = "skip_specs is set in .openspec.yaml but spec files exist under specs/. Remove skip_specs or delete the delta spec files";
const CHANGE_SKIP_SPECS_ACCEPTED: &str = "skip_specs is set in .openspec.yaml: change declares no spec-level behavior changes, zero deltas accepted";
const GUIDE_NO_DELTAS: &str = "No deltas found. Ensure your change has a specs/ directory with capability folders (e.g. specs/http-server/spec.md) containing .md files that use delta headers (## ADDED/MODIFIED/REMOVED/RENAMED Requirements) and that each requirement includes at least one \"#### Scenario:\" block. If this change intentionally modifies no specs (pure refactor, tooling, docs), set \"skip_specs: true\" in the change's .openspec.yaml instead. Tip: run \"openspec change show <change-id> --json --deltas-only\" to inspect parsed deltas.";
const GUIDE_MISSING_SPEC_SECTIONS: &str = "Missing required sections. Expected headers: \"## Purpose\" and \"## Requirements\". Example:\n## Purpose\n[brief purpose]\n\n## Requirements\n### Requirement: Clear requirement statement\nUsers SHALL ...\n\n#### Scenario: Descriptive name\n- **WHEN** ...\n- **THEN** ...";
const GUIDE_SCENARIO_FORMAT: &str = "Scenarios must use level-4 headers. Convert bullet lists into:\n#### Scenario: Short name\n- **WHEN** ...\n- **THEN** ...\n- **AND** ...";
const PURPOSE_IS_PLACEHOLDER: &str = "Purpose section is still a placeholder rather than a Purpose anyone wrote (the sentence `openspec archive` writes for a new capability, or a `TBD`/`TODO` marker left in its place). Replace it with what this capability is for, editing the main spec directly: a `## Purpose` in a delta is read only when the capability is created, so it cannot replace this one.";
const TASKS_WITHOUT_CHECKBOXES: &str = "This change counts as 0 tasks: no line in its tracked task files is a checkbox, so \"openspec list\" and \"openspec status\" report no work and \"openspec archive\" has nothing to flag as incomplete. Write each task as \"- [ ] 1.1 Description\".";

/// `buildMissingShallOrMustMessage`（validator.js:892-899）。
fn missing_shall_or_must_message(prefix: &str, name: &str, guidance_only: bool) -> String {
    let base = format!(
        "{prefix} {} contain SHALL or MUST",
        if guidance_only { "should" } else { "must" }
    );
    let suffix = if guidance_only {
        " (RFC 2119 best practice for English specs)"
    } else {
        ""
    };
    if openspec_md::contains_shall_or_must(name) {
        return format!(
            "{base} in the requirement body, not only in the header. Move the SHALL/MUST \
             statement to the line immediately after the \"### Requirement: ...\" header.{suffix}"
        );
    }
    format!("{base}{suffix}")
}

fn present_section_names(plan: &openspec_md::DeltaPlan) -> Vec<&'static str> {
    [
        (plan.added_present, "## ADDED Requirements"),
        (plan.modified_present, "## MODIFIED Requirements"),
        (plan.removed_present, "## REMOVED Requirements"),
        (plan.renamed_present, "## RENAMED Requirements"),
    ]
    .into_iter()
    .filter_map(|(present, name)| present.then_some(name))
    .collect()
}

/// `formatSectionList`：`A`、`A and B`、`A, B and C`。
fn format_section_list(sections: &[&str]) -> String {
    match sections {
        [] => String::new(),
        [only] => (*only).to_string(),
        [head @ .., last] => format!("{} and {last}", head.join(", ")),
    }
}

/// 保留插入順序的名稱集合（JS `Set` 的迭代順序會影響衝突訊息的順序）。
#[derive(Default)]
struct OrderedNames(Vec<String>);

impl OrderedNames {
    /// 已存在時回傳 `false`，與 `Set.has` 後 `add` 的組合相同。
    fn insert(&mut self, name: String) -> bool {
        if self.0.contains(&name) {
            return false;
        }
        self.0.push(name);
        true
    }

    fn contains(&self, name: &str) -> bool {
        self.0.iter().any(|candidate| candidate == name)
    }
}

/// 一個 delta 檔的逐檔規則（validator.js:161-362），依 OpenSpec 的順序寫入 `issues`。
fn check_delta_file(
    cfg: &Config,
    id: &str,
    plan: &openspec_md::DeltaPlan,
    entry_path: &str,
    issues: &mut Vec<Issue>,
) -> Result<()> {
    use openspec_md::normalize_requirement_name as key_of;

    // stray `###` 併入 requirement block，只給 INFO（C4）；沒有名字的
    // `### Requirement:` 另有措辭（C8）。
    for stray in &plan.skipped_headers {
        let nameless = stray.header.eq_ignore_ascii_case("requirement")
            || stray.header.eq_ignore_ascii_case("requirement:");
        let message = if nameless {
            format!(
                "Header \"### {}\" in {} is missing a requirement name and is ignored by \
                 validation. Add a name, e.g. \"### Requirement: <name>\".",
                stray.header, stray.section
            )
        } else {
            format!(
                "Header \"### {}\" in {} is not a \"### Requirement:\" header and is ignored by \
                 validation. Use \"### Requirement: {}\" if it should be validated as a \
                 requirement.",
                stray.header, stray.section, stray.header
            )
        };
        issues.push(Issue::info(entry_path.to_string(), message).at_line(stray.line));
    }
    for unpaired in &plan.unpaired_renames {
        let missing = if unpaired.side == "FROM" {
            "TO"
        } else {
            "FROM"
        };
        issues.push(
            Issue::error(
                entry_path.to_string(),
                format!(
                    "RENAMED {}: \"{}\" has no matching {missing}: line. Write each rename as a \
                     FROM: line followed immediately by its TO: line.",
                    unpaired.side, unpaired.name
                ),
            )
            .at_line(unpaired.line),
        );
    }
    // delta section 以外的 requirement 不會被套用（C10）。
    for orphan in &plan.orphaned_requirements {
        let location = match &orphan.section {
            Some(section) => format!("under \"## {section}\""),
            None => "above the first \"## \" section".to_string(),
        };
        issues.push(
            Issue::warning(
                entry_path.to_string(),
                format!(
                    "Requirement \"{}\" is {location}, which is not a delta section, so it is \
                     ignored. Move it under \"## ADDED Requirements\", \"## MODIFIED \
                     Requirements\", \"## REMOVED Requirements\", or \"## RENAMED Requirements\".",
                    orphan.name
                ),
            )
            .at_line(orphan.line),
        );
    }

    let mut added_names = OrderedNames::default();
    let mut modified_names = OrderedNames::default();
    let mut removed_names = OrderedNames::default();
    let mut renamed_from = OrderedNames::default();
    let mut renamed_to = OrderedNames::default();
    for (operation, blocks, names) in [
        ("ADDED", &plan.added, &mut added_names),
        ("MODIFIED", &plan.modified, &mut modified_names),
    ] {
        for block in blocks {
            if !names.insert(key_of(&block.name)) {
                issues.push(Issue::error(
                    entry_path.to_string(),
                    format!("Duplicate requirement in {operation}: \"{}\"", block.name),
                ));
            }
            let label = format!("{operation} \"{}\"", block.name);
            let text = openspec_md::block_requirement_text(&block.raw);
            if text.is_empty() {
                issues.push(Issue::error(
                    entry_path.to_string(),
                    if openspec_md::contains_shall_or_must(&block.name) {
                        missing_shall_or_must_message(&label, &block.name, false)
                    } else {
                        format!("{label} is missing requirement text")
                    },
                ));
            } else if !openspec_md::contains_shall_or_must(&text) {
                issues.push(Issue::warning(
                    entry_path.to_string(),
                    missing_shall_or_must_message(&label, &block.name, true),
                ));
            }
            // 沒有內文的 `#### ` 不算 scenario（C5）。
            if openspec_md::block_scenario_count(&block.raw) < 1 {
                let hint = if openspec_md::block_empty_scenario_count(&block.raw) > 0 {
                    " (a scenario header with no body under it does not count; add its steps, \
                     e.g. \"- **WHEN** ...\" and \"- **THEN** ...\")"
                } else {
                    ""
                };
                issues.push(Issue::error(
                    entry_path.to_string(),
                    format!("{label} must include at least one scenario{hint}"),
                ));
            }
        }
    }
    if !plan.modified.is_empty() {
        issues.extend(scenario_loss_issues(cfg, id, plan, entry_path)?);
    }
    for name in &plan.removed {
        if !removed_names.insert(key_of(name)) {
            issues.push(Issue::error(
                entry_path.to_string(),
                format!("Duplicate requirement in REMOVED: \"{name}\""),
            ));
        }
    }
    for rename in &plan.renamed {
        if !renamed_from.insert(key_of(&rename.from)) {
            issues.push(Issue::error(
                entry_path.to_string(),
                format!("Duplicate FROM in RENAMED: \"{}\"", rename.from),
            ));
        }
        if !renamed_to.insert(key_of(&rename.to)) {
            issues.push(Issue::error(
                entry_path.to_string(),
                format!("Duplicate TO in RENAMED: \"{}\"", rename.to),
            ));
        }
    }
    // 同一個檔案裡的跨 section 衝突：每個衝突一個 ERROR，檔案其餘部分照常檢查（C6）。
    for name in &modified_names.0 {
        if removed_names.contains(name) {
            issues.push(Issue::error(
                entry_path.to_string(),
                format!("Requirement present in both MODIFIED and REMOVED: \"{name}\""),
            ));
        }
        if added_names.contains(name) {
            issues.push(Issue::error(
                entry_path.to_string(),
                format!("Requirement present in both MODIFIED and ADDED: \"{name}\""),
            ));
        }
    }
    for name in &added_names.0 {
        if removed_names.contains(name) {
            issues.push(Issue::error(
                entry_path.to_string(),
                format!("Requirement present in both ADDED and REMOVED: \"{name}\""),
            ));
        }
    }
    for rename in &plan.renamed {
        let from = key_of(&rename.from);
        if modified_names.contains(&from) {
            issues.push(Issue::error(
                entry_path.to_string(),
                format!(
                    "MODIFIED references old name from RENAMED. Use new header for \"{}\"",
                    rename.to
                ),
            ));
        }
        if added_names.contains(&key_of(&rename.to)) {
            issues.push(Issue::error(
                entry_path.to_string(),
                format!("RENAMED TO collides with ADDED for \"{}\"", rename.to),
            ));
        }
        let folded = openspec_md::fold_requirement_name(&from);
        if let Some(spelled) = removed_names
            .0
            .iter()
            .find(|removed| openspec_md::fold_requirement_name(removed) == folded)
        {
            let variant = if *spelled == from {
                String::new()
            } else {
                format!(" (REMOVED spells it \"{spelled}\")")
            };
            issues.push(Issue::error(
                entry_path.to_string(),
                format!(
                    "Requirement present in both RENAMED and REMOVED: \"{}\"{variant}",
                    rename.from
                ),
            ));
        }
    }
    Ok(())
}

/// `findScenarioLossIssues`（validator.js:578-655）：MODIFIED 漏掉 main spec 仍有的
/// scenario 時每個 requirement 一個 ERROR（C3）。main spec 不存在、或找不到該
/// requirement 時不報（archive 的判斷交給下面的 INFO）。
fn scenario_loss_issues(
    cfg: &Config,
    id: &str,
    plan: &openspec_md::DeltaPlan,
    entry_path: &str,
) -> Result<Vec<Issue>> {
    use openspec_md::normalize_requirement_name as key_of;

    let main_spec = main_spec_path(cfg, id);
    let content = match std::fs::read(&main_spec) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) => {
            // 只有「這個檔案本身不能用」的錯誤才報；不存在與暫時性的錯誤都不算。
            let code = match error.kind() {
                std::io::ErrorKind::PermissionDenied => "EACCES",
                std::io::ErrorKind::IsADirectory => "EISDIR",
                _ => return Ok(Vec::new()),
            };
            return Ok(vec![Issue::error(
                entry_path.to_string(),
                format!(
                    "Could not read {} to check the MODIFIED requirements against it ({code}). \
                     Archive reads the same file, so fix the file before archiving.",
                    main_spec.display()
                ),
            )]);
        }
    };
    let mut current = std::collections::HashMap::new();
    for block in openspec_md::requirements_section_blocks(&content) {
        current.insert(key_of(&block.name), block);
    }
    // archive 先套 RENAMED 再套 MODIFIED，所以 MODIFIED 用新名字時要回頭找舊名字的
    // block；rename 可以串接，visited 擋掉循環。
    let renamed_from: std::collections::HashMap<String, String> = plan
        .renamed
        .iter()
        .map(|rename| (key_of(&rename.to), key_of(&rename.from)))
        .collect();
    let renamed_away: std::collections::HashSet<String> = plan
        .renamed
        .iter()
        .map(|rename| key_of(&rename.from))
        .collect();
    let mut issues = Vec::new();
    for block in &plan.modified {
        let key = key_of(&block.name);
        if renamed_away.contains(&key) {
            continue;
        }
        let mut visited = std::collections::HashSet::new();
        let mut cursor = Some(key);
        let mut base = None;
        while let Some(name) = cursor {
            if !visited.insert(name.clone()) {
                break;
            }
            if let Some(found) = current.get(&name) {
                base = Some(found);
                break;
            }
            cursor = renamed_from.get(&name).cloned();
        }
        let Some(base) = base else {
            continue;
        };
        let diff = openspec_md::diff_scenario_names(&base.raw, &block.raw);
        if diff.missing.is_empty() {
            continue;
        }
        issues.push(Issue::error(
            entry_path.to_string(),
            format!(
                "MODIFIED \"{}\" omits scenario(s) the current spec still has: {}. {} Copy the \
                 omitted scenarios into the MODIFIED block (a MODIFIED requirement replaces the \
                 whole block, so archive refuses to drop them).",
                block.name,
                quoted_list(&diff.missing),
                openspec_md::describe_scenario_balance(&diff)
            ),
        ));
    }
    Ok(issues)
}

fn quoted_list(names: &[String]) -> String {
    names
        .iter()
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

fn main_spec_path(cfg: &Config, id: &str) -> std::path::PathBuf {
    let mut path = cfg.specs_dir();
    for segment in id.split('/') {
        path = path.join(segment);
    }
    path.join("spec.md")
}

/// JS `Map` 語意的 requirement 表：同名再 set 時原位取代，新名字加在尾端。
#[derive(Default)]
struct BlockMap(Vec<(String, openspec_md::Block)>);

impl BlockMap {
    fn get(&self, key: &str) -> Option<&openspec_md::Block> {
        self.0
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, block)| block)
    }

    fn set(&mut self, key: String, block: openspec_md::Block) {
        match self.0.iter_mut().find(|(candidate, _)| *candidate == key) {
            Some(entry) => entry.1 = block,
            None => self.0.push((key, block)),
        }
    }

    fn delete(&mut self, key: &str) {
        self.0.retain(|(candidate, _)| candidate != key);
    }

    /// 第一個 fold 後相同、且不是 `except` 的 key（`[...map.keys()].find(...)`）。
    fn near_miss(&self, name: &str, except: Option<&str>) -> Option<&openspec_md::Block> {
        let folded = openspec_md::fold_requirement_name(name);
        self.0
            .iter()
            .find(|(key, _)| {
                Some(key.as_str()) != except && openspec_md::fold_requirement_name(key) == folded
            })
            .map(|(_, block)| block)
    }
}

/// OpenSpec 以 archive 的 merge builder 乾跑一次（`buildUpdatedSpec`，
/// specs-apply.js:109-425），把它會丟出的前置條件錯誤當成 INFO（C1、C13）。
/// 回傳該錯誤訊息；會套用成功時回傳 `None`。
///
/// 同名重複、跨 section 衝突與「沒有任何 operation」在 `check_delta_file` 已報成
/// ERROR，帶著 ERROR 的檔案不會走到這裡，所以不重做那些檢查。檔案系統錯誤
/// （讀不到 main spec）在 OpenSpec 也不報。
fn archive_refusal(cfg: &Config, id: &str, plan: &openspec_md::DeltaPlan) -> Option<String> {
    use openspec_md::normalize_requirement_name as key_of;

    if let Some(first) = plan.unpaired_renames.first() {
        let missing = if first.side == "FROM" { "TO" } else { "FROM" };
        return Some(format!(
            "{id} validation failed - RENAMED entry on line {} has no matching {missing}: for \
             header \"### Requirement: {}\". Write each rename as a FROM: line followed \
             immediately by its TO: line.",
            first.line, first.name
        ));
    }
    let target = main_spec_path(cfg, id);
    let (content, is_new_spec) = match std::fs::read(&target) {
        Ok(bytes) => (String::from_utf8_lossy(&bytes).into_owned(), false),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            if !plan.modified.is_empty() || !plan.renamed.is_empty() {
                return Some(format!(
                    "{id}: target spec does not exist; only ADDED requirements are allowed for \
                     new specs. MODIFIED and RENAMED operations require an existing spec."
                ));
            }
            (String::new(), true)
        }
        Err(_) => return None,
    };
    if !is_new_spec {
        let structure = openspec_md::main_spec_structure_issues(&content);
        if !structure.is_empty() {
            let details = structure
                .iter()
                .map(|issue| format!("line {}: {}", issue.line, issue.message))
                .collect::<Vec<_>>()
                .join("\n");
            return Some(format!(
                "{id}: target spec is structurally invalid and cannot be updated until \
                 fixed:\n{details}"
            ));
        }
    }
    let mut blocks = BlockMap::default();
    for block in openspec_md::requirements_section_blocks(&content) {
        blocks.set(key_of(&block.name), block);
    }

    for rename in &plan.renamed {
        let from = key_of(&rename.from);
        let to = key_of(&rename.to);
        if blocks.get(&from).is_none() {
            // 來源不在、目標已在：rename 已提前同步，不算失敗；除非還有大小寫不同的來源。
            if blocks.get(&to).is_some() {
                if let Some(near) = blocks.near_miss(&from, Some(&to)) {
                    return Some(format!(
                        "{id} RENAMED failed for header \"### Requirement: {}\" - source not \
                         found, but \"### Requirement: {}\" exists; fix the header to match it \
                         exactly",
                        rename.from, near.name
                    ));
                }
                continue;
            }
            return Some(format!(
                "{id} RENAMED failed for header \"### Requirement: {}\" - source not found",
                rename.from
            ));
        }
        if blocks.get(&to).is_some() {
            return Some(format!(
                "{id} RENAMED failed for header \"### Requirement: {}\" - target already exists",
                rename.to
            ));
        }
        if let Some(near) = blocks.near_miss(&to, Some(&from)) {
            return Some(format!(
                "{id} RENAMED failed for header \"### Requirement: {}\" - \"### Requirement: {}\" \
                 already exists and differs only in case or spacing; choose a distinct name",
                rename.to, near.name
            ));
        }
        let mut block = blocks.get(&from).cloned().expect("checked above");
        let mut lines: Vec<&str> = block.raw.split('\n').collect();
        let header = format!("### Requirement: {to}");
        lines[0] = &header;
        block.raw = lines.join("\n");
        block.name = to.clone();
        // JS Map 的 delete + set 會把 key 移到尾端。
        blocks.delete(&from);
        blocks.set(to, block);
    }
    for name in &plan.removed {
        let key = key_of(name);
        if blocks.get(&key).is_none() {
            // 已經不在 main spec：視為早已移除（C2），除非有大小寫不同的同名 requirement。
            if !is_new_spec {
                if let Some(near) = blocks.near_miss(&key, None) {
                    return Some(format!(
                        "{id} REMOVED failed for header \"### Requirement: {name}\" - not found, \
                         but \"### Requirement: {}\" exists; fix the header to match it exactly",
                        near.name
                    ));
                }
            }
            continue;
        }
        blocks.delete(&key);
    }
    for modified in &plan.modified {
        let key = key_of(&modified.name);
        let Some(current) = blocks.get(&key) else {
            return Some(format!(
                "{id} MODIFIED failed for header \"### Requirement: {}\" - not found",
                modified.name
            ));
        };
        let diff = openspec_md::diff_scenario_names(&current.raw, &modified.raw);
        if !diff.missing.is_empty() {
            return Some(format!(
                "{id} MODIFIED failed for header \"### Requirement: {}\" - current spec contains \
                 scenario(s) not present in the modified block: {}. {} Refresh the change spec \
                 before archiving to avoid dropping scenarios.",
                modified.name,
                quoted_list(&diff.missing),
                openspec_md::describe_scenario_balance(&diff)
            ));
        }
        blocks.set(key, modified.clone());
    }
    for added in &plan.added {
        let key = key_of(&added.name);
        if let Some(existing) = blocks.get(&key) {
            // 內容相同代表已提前同步，不是衝突。
            if openspec_md::normalize(&existing.raw).trim()
                == openspec_md::normalize(&added.raw).trim()
            {
                continue;
            }
            return Some(format!(
                "{id} ADDED failed for header \"### Requirement: {}\" - already exists",
                added.name
            ));
        }
        if let Some(near) = blocks.near_miss(&key, None) {
            return Some(format!(
                "{id} ADDED failed for header \"### Requirement: {}\" - \"### Requirement: {}\" \
                 already exists and differs only in case or spacing; use MODIFIED with that exact \
                 header to change it, or choose a distinct name",
                added.name, near.name
            ));
        }
        blocks.set(key, added.clone());
    }
    None
}

/// `discoverSpecFiles`（utils/spec-discovery.js）：`specs/` 底下任何深度的
/// `spec.md`（根目錄那個除外），id 是相對 `specs/` 的目錄路徑，依 code point
/// 排序。略過 dot 項目、不跟隨目錄 symlink；`spec.md` 本身是 symlink 時跟隨，
/// 斷掉的略過。`specs/` 不存在（或不是目錄）時沒有 delta。
fn discover_spec_files(specs_root: &std::path::Path) -> Result<Vec<(String, std::path::PathBuf)>> {
    fn walk(
        dir: &std::path::Path,
        segments: &mut Vec<String>,
        out: &mut Vec<(String, std::path::PathBuf)>,
    ) -> Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                return Ok(())
            }
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", dir.display()));
            }
        };
        for entry in entries {
            let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let file_type = entry
                .file_type()
                .with_context(|| format!("checking {}", entry.path().display()))?;
            if file_type.is_dir() {
                segments.push(name);
                walk(&entry.path(), segments, out)?;
                segments.pop();
            } else if name == "spec.md" && !segments.is_empty() {
                let path = entry.path();
                let is_file = if file_type.is_symlink() {
                    match std::fs::metadata(&path) {
                        Ok(metadata) => metadata.is_file(),
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                        Err(error) => {
                            return Err(error)
                                .with_context(|| format!("checking {}", path.display()));
                        }
                    }
                } else {
                    file_type.is_file()
                };
                if is_file {
                    out.push((segments.join("/"), path));
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(specs_root, &mut Vec::new(), &mut out)?;
    out.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(out)
}

/// `findUnreadDeltaFiles`：`specs/` 底下帶 delta section、卻不是 `spec.md` 的
/// `.md` 檔（C12）。回傳 `(相對 specs/ 的路徑, 應該搬去的 <capability>/spec.md)`。
fn find_unread_delta_files(specs_root: &std::path::Path) -> Result<Vec<(String, String)>> {
    fn walk(
        dir: &std::path::Path,
        segments: &mut Vec<String>,
        out: &mut Vec<(String, String)>,
    ) -> Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                return Ok(())
            }
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", dir.display()));
            }
        };
        for entry in entries {
            let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("checking {}", path.display()))?;
            if file_type.is_dir() {
                segments.push(name);
                walk(&path, segments, out)?;
                segments.pop();
                continue;
            }
            if !file_type.is_file() && !file_type.is_symlink() {
                continue;
            }
            if name == "spec.md" || !name.to_lowercase().ends_with(".md") {
                continue;
            }
            if file_type.is_symlink() {
                match std::fs::metadata(&path) {
                    Ok(metadata) if metadata.is_file() => {}
                    Ok(_) => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        return Err(error).with_context(|| format!("checking {}", path.display()));
                    }
                }
            }
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| format!("reading {}", path.display()));
                }
            };
            if !openspec_md::parse_delta_spec(&String::from_utf8_lossy(&bytes))
                .any_section_present()
            {
                continue;
            }
            let capability = if segments.is_empty() {
                name[..name.len() - ".md".len()].to_string()
            } else {
                segments.join("/")
            };
            let mut relative = segments.clone();
            relative.push(name);
            out.push((relative.join("/"), format!("{capability}/spec.md")));
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(specs_root, &mut Vec::new(), &mut out)?;
    out.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(out)
}

/// `hasAnyFileUnder`：任何非 dot 的檔案或 symlink（不跟隨）。
fn has_any_file_under(dir: &std::path::Path) -> std::io::Result<bool> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_file() || file_type.is_symlink() {
            return Ok(true);
        }
        if file_type.is_dir() && has_any_file_under(&entry.path())? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `collectTaskFileIssues`（validator.js:470-498）：tracked task 檔都沒有
/// checkbox 時的 WARNING（C19），以及只在內建 `spec-driven` schema 才跑的 task
/// 編號 WARNING（C18）。schema 解析失敗時兩者都不報。
///
/// tracked task 檔是 schema `apply.tracks` 指到的 artifact 輸出（沒有 tracks 時是
/// id 為 `tasks` 的 artifact）。OpenSpec 會展開 glob；這裡只支援字面路徑（內建
/// schema 與目前所見的專案 schema 都是 `tasks.md`），glob 視為沒有 tracked 檔。
fn task_file_issues(
    cfg: &Config,
    change_dir: &std::path::Path,
    loaded: Option<&change::Change>,
) -> Result<Vec<Issue>> {
    let Ok(schema) = crate::schema::resolve_schema(cfg, None, loaded) else {
        return Ok(Vec::new());
    };
    let generates = match &schema.apply_tracks {
        Some(tracks) => schema
            .artifacts
            .iter()
            .find(|artifact| artifact.output_path == *tracks),
        None => schema
            .artifacts
            .iter()
            .find(|artifact| artifact.id == "tasks"),
    }
    .map(|artifact| artifact.output_path.clone());
    let tracked: Vec<String> = generates
        .filter(|path| !path.contains(['*', '?', '[', '{']))
        .filter(|path| change_dir.join(path).is_file())
        .into_iter()
        .collect();
    let files = if tracked.is_empty() {
        vec!["tasks.md".to_string()]
    } else {
        tracked.clone()
    };
    let mut documents = Vec::new();
    let mut unreadable = 0usize;
    for file in files {
        match std::fs::read(change_dir.join(&file)) {
            Ok(bytes) => documents.push((file, String::from_utf8_lossy(&bytes).into_owned())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => unreadable += 1,
        }
    }
    documents.sort_by(|left, right| left.0.cmp(&right.0));
    let mut issues = Vec::new();
    if !tracked.is_empty() && unreadable == 0 {
        issues.extend(missing_checkbox_issues(&documents));
    }
    if schema.name == crate::schema::SCHEMA_NAME
        && schema.source == crate::schema::SchemaSource::Package
    {
        issues.extend(task_numbering_issues(&documents));
    }
    Ok(issues)
}

/// `parseTaskLines` 的單行版本：
/// `^\s*(?:[-*+]|\d{1,9}[.)])\s*\[(?:\s*([^\]\s]?)\s*\](?![([])|\s+\])\s*(.*)`。
/// Rust `regex` 沒有前瞻，這裡手寫；回傳 description（已 trim）。
fn task_line_description(line: &str) -> Option<String> {
    let rest = line.trim_start();
    let rest = if let Some(rest) = rest.strip_prefix(['-', '*', '+']) {
        rest
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if !(1..=9).contains(&digits) {
            return None;
        }
        rest[digits..].strip_prefix(['.', ')'])?
    };
    let inner = rest.trim_start().strip_prefix('[')?;
    let after_ws = inner.trim_start();
    let leading_ws = inner.len() - after_ws.len();
    // 第一種：`\s*([^\]\s]?)\s*\]`，後面不能緊接 `(` 或 `[`。
    let mut candidate = after_ws;
    if let Some(ch) = candidate.chars().next() {
        if ch != ']' && !ch.is_whitespace() {
            candidate = &candidate[ch.len_utf8()..];
        }
    }
    if let Some(after) = candidate.trim_start().strip_prefix(']') {
        if !after.starts_with(['(', '[']) {
            return Some(after.trim().to_string());
        }
    }
    // 第二種：`\s+\]`，沒有前瞻限制。
    if leading_ws > 0 {
        if let Some(after) = after_ws.strip_prefix(']') {
            return Some(after.trim().to_string());
        }
    }
    None
}

/// `findTaskNumberingIssues`（validation/task-numbering.js）。
fn task_numbering_issues(documents: &[(String, String)]) -> Vec<Issue> {
    static NUMBERED_GROUP: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^ {0,3}##[ \t]+(\d+)\.(?:[ \t]|\r?$)").unwrap());
    static TASK_ID: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^(\d+(?:\.\d+)+(?:[A-Za-z]+)?)").unwrap());
    // `^ {0,3}##(?!#)(?:[ \t]+|[ \t]*\r?$)`
    fn is_level_two_heading(line: &str) -> bool {
        let indent = line.bytes().take_while(|byte| *byte == b' ').count();
        if indent > 3 {
            return false;
        }
        let Some(rest) = line[indent..].strip_prefix("##") else {
            return false;
        };
        rest.starts_with([' ', '\t']) || rest.is_empty() || rest == "\r"
    }
    fn strip_leading_zeros(value: &str) -> &str {
        let trimmed = value.trim_start_matches('0');
        if trimmed.is_empty() {
            &value[value.len() - 1..]
        } else {
            trimmed
        }
    }

    let mut issues = Vec::new();
    let mut first: std::collections::HashMap<String, (String, usize)> =
        std::collections::HashMap::new();
    for (path, content) in documents {
        let lines: Vec<&str> = content.split('\n').collect();
        if !lines.iter().any(|line| NUMBERED_GROUP.is_match(line)) {
            continue;
        }
        let mut group: Option<String> = None;
        for (index, line) in lines.iter().enumerate() {
            if is_level_two_heading(line) {
                group = NUMBERED_GROUP
                    .captures(line)
                    .map(|captures| captures[1].to_string());
            }
            let Some(current) = &group else {
                continue;
            };
            let Some(description) = task_line_description(line) else {
                continue;
            };
            let Some(id) = TASK_ID.captures(&description).and_then(|captures| {
                let id = captures.get(1)?;
                let next = description[id.end()..].chars().next();
                next.is_none_or(char::is_whitespace)
                    .then(|| id.as_str().to_string())
            }) else {
                continue;
            };
            let line_number = index + 1;
            let task_group = id.split('.').next().unwrap_or_default().to_string();
            if strip_leading_zeros(&task_group) != strip_leading_zeros(current) {
                issues.push(
                    Issue::warning(
                        path.clone(),
                        format!(
                            "Task \"{id}\" is under group {current}, but its leading number \
                             points to group {task_group}. Move it to group {task_group} or \
                             renumber it."
                        ),
                    )
                    .at_line(line_number),
                );
            }
            match first.get(&id) {
                Some((first_path, first_line)) => {
                    let declared = if first_path == path {
                        format!("on line {first_line}")
                    } else {
                        format!("in {first_path} on line {first_line}")
                    };
                    issues.push(
                        Issue::warning(
                            path.clone(),
                            format!(
                                "Task ID \"{id}\" is duplicated; it was first declared {declared}."
                            ),
                        )
                        .at_line(line_number),
                    );
                }
                None => {
                    first.insert(id, (path.clone(), line_number));
                }
            }
        }
    }
    issues
}

/// `findMissingTaskCheckboxIssues`（validation/task-checkboxes.js）：整組 tracked
/// task 檔都沒有 checkbox 時，每個檔案指出第一個清單項目（C19）。
fn missing_checkbox_issues(documents: &[(String, String)]) -> Vec<Issue> {
    static LIST_ITEM: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^\s*(?:[-*+]|\d{1,9}[.)])\s+\S").unwrap());
    static FENCE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^ {0,3}(`{3,}|~{3,})(.*)").unwrap());
    fn is_front_matter(line: &str) -> bool {
        let line = line.trim_end();
        line == "---"
    }
    fn indent_columns(line: &str) -> usize {
        let mut column = 0;
        for ch in line.chars() {
            match ch {
                ' ' => column += 1,
                '\t' => column += 4 - (column % 4),
                _ => break,
            }
        }
        column
    }
    fn first_list_item_line(content: &str) -> Option<usize> {
        let lines: Vec<&str> = content.split('\n').collect();
        let mut index = 0;
        if lines
            .first()
            .is_some_and(|line| is_front_matter(line.trim_start_matches('\u{feff}')))
        {
            if let Some(close) = (1..lines.len()).find(|&index| is_front_matter(lines[index])) {
                index = close + 1;
            }
        }
        let mut open_fence: Option<(char, usize)> = None;
        let mut in_comment = false;
        while index < lines.len() {
            let line = lines[index];
            index += 1;
            if in_comment {
                if line.contains("-->") {
                    in_comment = false;
                }
                continue;
            }
            if let Some(captures) = FENCE.captures(line) {
                let run = &captures[1];
                let marker = run.chars().next().unwrap();
                match open_fence {
                    None => open_fence = Some((marker, run.len())),
                    Some((open, length))
                        if marker == open
                            && run.len() >= length
                            && captures[2].trim().is_empty() =>
                    {
                        open_fence = None
                    }
                    Some(_) => {}
                }
                continue;
            }
            if open_fence.is_some() {
                continue;
            }
            if line.trim_start().starts_with("<!--") {
                let open = line.find("<!--").unwrap() + "<!--".len();
                if !line[open..].contains("-->") {
                    in_comment = true;
                }
                continue;
            }
            if LIST_ITEM.is_match(line) && indent_columns(line) < 4 {
                return Some(index);
            }
        }
        None
    }

    if documents.is_empty()
        || documents.iter().any(|(_, content)| {
            content
                .split('\n')
                .any(|line| task_line_description(line).is_some())
        })
    {
        return Vec::new();
    }
    documents
        .iter()
        .filter_map(|(path, content)| {
            first_list_item_line(content).map(|line| {
                Issue::warning(path.clone(), TASKS_WITHOUT_CHECKBOXES.to_string()).at_line(line)
            })
        })
        .collect()
}

/// `--archived`: one item per archived change directory, in name order.
pub fn validate_archived(cfg: &Config) -> Result<Vec<ChangeValidation>> {
    let archive_dir = cfg.changes_dir().join("archive");
    let entries = match std::fs::read_dir(&archive_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Vec::new());
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
        let metadata = std::fs::symlink_metadata(&dir)
            .with_context(|| format!("checking {}", dir.display()))?;
        if !metadata.file_type().is_dir() {
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
    Ok(items)
}
fn elapsed_ms(started: std::time::Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn issues_are_valid(issues: &[Issue], strict: bool) -> bool {
    !issues
        .iter()
        .any(|issue| issue.level == "ERROR" || (strict && issue.level == "WARNING"))
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

    /// Issue #80 的後續：PR #82 曾把 SHALL/MUST 限縮到第一個文字區塊，
    /// OpenSpec 1.12 的 `extractRequirementBody` 改回讀取第一個 header 之前的
    /// 整段 body，PR #161 跟進。這裡釘住 1.12 語義的兩條邊界，任一邊回到
    /// #82 的形狀或放寬到 scenario 內都會失敗。
    #[test]
    fn a_goal_first_requirement_passes_because_the_whole_body_is_normative_text() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write_delta(
            &c,
            "feat",
            "auth",
            "## ADDED Requirements\n\n\
             ### Requirement: Login\n\n\
             > **Goal**: users can sign in\n\n\
             The system SHALL authenticate users.\n\n\
             #### Scenario: Valid credentials\n\n\
             - **WHEN** a user submits valid credentials\n\
             - **THEN** they are logged in\n",
        );

        let result = validate_change(&c, "feat", true).unwrap();
        assert!(result.valid, "got: {:?}", result.issues);
    }

    #[test]
    fn a_shall_that_appears_only_inside_a_scenario_is_not_normative() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write_delta(
            &c,
            "feat",
            "auth",
            "## ADDED Requirements\n\n\
             ### Requirement: Login\n\n\
             Users sign in with a password.\n\n\
             #### Scenario: Valid credentials\n\n\
             - **WHEN** a user submits valid credentials\n\
             - **THEN** the system SHALL log them in\n",
        );

        let result = validate_change(&c, "feat", true).unwrap();
        assert!(!result.valid);
        assert!(
            result
                .issues
                .iter()
                .any(|i| i.message == "ADDED \"Login\" should contain SHALL or MUST (RFC 2119 best practice for English specs)"),
            "got: {:?}",
            result.issues
        );
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
            .any(|issue| issue.message == "ADDED \"Login\" must include at least one scenario"));

        let strict = validate_change(&c, "feat", true).unwrap();
        assert!(!strict.valid);
        assert!(strict
            .issues
            .iter()
            .any(|issue| issue.message == "ADDED \"Login\" must include at least one scenario"));
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
                .all(|i| i.path == "Billing/Invoices/spec.md"),
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

        let items =
            validate_items(&c, &["good".to_string(), "bad".to_string()], &[], true).unwrap();
        let report = openspec_report(&c, &items, &["change", "spec"]);
        assert_eq!(report.summary.totals.items, 2);
        assert_eq!(report.summary.totals.passed, 1);
        assert_eq!(report.summary.totals.failed, 1);
        assert!(report.any_failed());
        // 要求的 type 即使沒有 item 也有一格 byType（commands/validate.js）。
        assert_eq!(
            report.summary.by_type["spec"],
            Totals {
                items: 0,
                passed: 0,
                failed: 0
            }
        );
        assert_eq!(report.summary.by_type["change"].failed, 1);
    }

    /// Node 22／ICU 78 的 `localeCompare` 實測順序（`node -e`，W9a）。
    #[test]
    fn locale_compare_matches_node_for_ascii_ids() {
        let mut ids: Vec<&str> = vec![
            "zeta",
            "Upper_Case",
            "only",
            "alpha",
            "B-upper",
            "a-b",
            "a.b",
            "a/b",
            "aa",
            "parentless/child",
            "a",
            "A",
            "ab",
            "Ab",
            "aB",
            "a1",
            "a10",
            "a2",
            "a-",
            "a_",
            "a b",
        ];
        ids.sort_by(|a, b| locale_compare(a, b));
        assert_eq!(
            ids,
            [
                "a",
                "A",
                "a b",
                "a_",
                "a-",
                "a-b",
                "a.b",
                "a/b",
                "a1",
                "a10",
                "a2",
                "aa",
                "ab",
                "aB",
                "Ab",
                "alpha",
                "B-upper",
                "only",
                "parentless/child",
                "Upper_Case",
                "zeta",
            ]
        );
    }

    #[test]
    fn nearest_matches_ranks_by_levenshtein_keeping_candidate_order_on_ties() {
        let candidates: Vec<String> = [
            "Upper_Case",
            "alpha",
            "only",
            "zeta",
            "B-upper",
            "a-b",
            "a.b",
            "a/b",
            "aa",
            "only",
            "parentless/child",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            nearest_matches("nope", &candidates, 5),
            ["alpha", "only", "zeta", "a-b", "a.b"]
        );
        assert_eq!(nearest_matches("zetta", &candidates, 1), ["zeta"]);
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

    /// C18：OpenSpec 1.13.2 只對「內建」的 spec-driven schema 檢查 task 編號；專案
    /// 自己放一份同名的 `schemas/spec-driven/` 時不檢查（probe_c18b.sh 實測兩邊都
    /// 沒有 finding；改名為 custom-driven 的專案 schema 亦同，probe_c18.sh）。
    #[test]
    fn task_numbering_is_skipped_for_a_project_schema_that_shadows_spec_driven() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write_delta(&c, "feat", "auth", GOOD_ADDED);
        fs::write(
            c.changes_dir().join("feat/tasks.md"),
            "## 1. Group\n- [ ] 2.1 wrong group\n- [ ] 2.1 duplicate\n",
        )
        .unwrap();
        let schema_dir = tmp.join("openspec/schemas/spec-driven");
        fs::create_dir_all(schema_dir.join("templates")).unwrap();
        fs::write(
            schema_dir.join("schema.yaml"),
            "name: spec-driven\nversion: 1\nartifacts:\n- id: tasks\n  generates: tasks.md\n  \
             description: Tasks\n  template: tasks.md\n  instruction: Write them.\n  requires: []\n\
             apply:\n  requires: [tasks]\n  tracks: tasks.md\n  instruction: Do it.\n",
        )
        .unwrap();
        fs::write(schema_dir.join("templates/tasks.md"), "## 1. Group\n").unwrap();

        let result = validate_change(&c, "feat", true).unwrap();
        assert!(result.valid, "got: {:?}", result.issues);
        assert!(result.issues.is_empty(), "got: {:?}", result.issues);
    }

    /// archive 先套 RENAMED 再套 MODIFIED：MODIFIED 用串接 rename 後的新名字時，
    /// 仍要對到 main spec 裡舊名字的 block 檢查 scenario 遺失（validator.js:611-628）。
    #[test]
    fn scenario_loss_follows_a_chain_of_renames_back_to_the_main_spec() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let main = c.specs_dir().join("auth");
        fs::create_dir_all(&main).unwrap();
        fs::write(
            main.join("spec.md"),
            "# auth\n\n## Purpose\n\nAuthentication of family accounts across every client.\n\n\
             ## Requirements\n\n### Requirement: Original\n\nThe system SHALL log in.\n\n\
             #### Scenario: a1\n\n- **WHEN** x\n\n#### Scenario: a2\n\n- **WHEN** y\n",
        )
        .unwrap();
        write_delta(
            &c,
            "feat",
            "auth",
            "## RENAMED Requirements\n\n\
             - FROM: `### Requirement: Original`\n- TO: `### Requirement: Middle`\n\
             - FROM: `### Requirement: Middle`\n- TO: `### Requirement: Final`\n\n\
             ## MODIFIED Requirements\n\n### Requirement: Final\n\nThe system SHALL log in.\n\n\
             #### Scenario: a1\n\n- **WHEN** x\n",
        );

        let result = validate_change(&c, "feat", false).unwrap();
        assert!(
            result.issues.iter().any(|issue| issue.level == "ERROR"
                && issue.message.starts_with(
                    "MODIFIED \"Final\" omits scenario(s) the current spec still has: \"a2\"."
                )),
            "got: {:?}",
            result.issues
        );
    }

    #[test]
    fn report_json_shape_matches_the_downstream_gate_contract() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        let report = openspec_report(
            &c,
            &[ChangeValidation {
                id: "feat".to_string(),
                item_type: "change".to_string(),
                valid: false,
                issues: vec![Issue::error(
                    "specs/auth/spec.md".to_string(),
                    "boom".to_string(),
                )],
                duration_ms: 0,
            }],
            &["change"],
        );
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["items"][0]["id"], "feat");
        assert_eq!(value["items"][0]["valid"], false);
        assert_eq!(value["items"][0]["issues"][0]["level"], "ERROR");
        assert_eq!(value["items"][0]["issues"][0]["path"], "specs/auth/spec.md");
        assert_eq!(value["items"][0]["issues"][0]["message"], "boom");
        assert_eq!(value["summary"]["totals"]["failed"], 1);
    }

    fn item(item_type: &str, issues: Vec<Issue>) -> ChangeValidation {
        ChangeValidation {
            id: "x".to_string(),
            item_type: item_type.to_string(),
            valid: !issues.iter().any(|issue| issue.level == "ERROR"),
            issues,
            duration_ms: 0,
        }
    }

    /// D12-1／D12-2：ERROR→errors、WARNING→warnings、只有 archive 拒絕的 INFO 進
    /// warnings；delta 檔的訊息加 `specs/<path>: `。
    #[test]
    fn oracle_item_buckets_levels_and_prefixes_delta_file_messages() {
        let mut delta_info = Issue::info(
            "cap/spec.md".to_string(),
            "Archive would refuse this delta: cap MODIFIED failed".to_string(),
        );
        delta_info.delta_file = true;
        let mut stray = Issue::info("cap/spec.md".to_string(), "Header ignored".to_string());
        stray.delta_file = true;
        let mut delta_error = Issue::error("cap/spec.md".to_string(), "bad".to_string());
        delta_error.delta_file = true;
        let got = oracle_item(&item(
            "change",
            vec![
                delta_info,
                stray,
                Issue::info("file".to_string(), "skip_specs accepted".to_string()),
                delta_error,
                Issue::warning("tasks.md".to_string(), "dup".to_string()).at_line(3),
            ],
        ));
        assert_eq!(got.errors, ["specs/cap/spec.md: bad"]);
        assert_eq!(
            got.warnings,
            [
                "specs/cap/spec.md: Archive would refuse this delta: cap MODIFIED failed",
                "dup"
            ]
        );
        assert!(!got.valid);
    }

    #[test]
    fn oracle_item_serializes_alphabetical_keys_for_both_types() {
        let change = serde_json::to_string(&oracle_item(&item("change", vec![]))).unwrap();
        assert_eq!(
            change,
            r#"{"change":"x","errors":[],"valid":true,"warnings":[]}"#
        );
        let spec = serde_json::to_string(&oracle_item(&item("spec", vec![]))).unwrap();
        assert_eq!(
            spec,
            r#"{"errors":[],"spec":"x","valid":true,"warnings":[]}"#
        );
    }

    /// delta 檔以外的 change finding（`file`、task 檔）不標 delta_file。
    #[test]
    fn validate_change_marks_only_delta_file_findings() {
        let tmp = TempDir::new();
        let c = cfg(&tmp);
        write_delta(
            &c,
            "feat",
            "auth",
            "## ADDED Requirements\n\n### Requirement: Login\n\nUsers log in.\n\n\
             #### Scenario: ok\n\n- **WHEN** x\n",
        );
        fs::write(
            c.changes_dir().join("feat/tasks.md"),
            "## 1. Group\n- [ ] 1.1 a\n- [ ] 1.1 b\n",
        )
        .unwrap();
        let result = validate_change(&c, "feat", false).unwrap();
        let flags: Vec<(&str, bool)> = result
            .issues
            .iter()
            .map(|issue| (issue.path.as_str(), issue.delta_file))
            .collect();
        assert!(flags.contains(&("auth/spec.md", true)), "{flags:?}");
        assert!(flags.contains(&("tasks.md", false)), "{flags:?}");
    }

    fn write_canonical_spec(cfg: &Config, capability: &str, content: &str) {
        let dir = cfg.specs_dir().join(capability);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("spec.md"), content).unwrap();
    }

    #[test]
    fn validate_spec_passes_a_well_formed_spec() {
        let tmp = TempDir::new();
        let cfg = cfg(&tmp);
        fs::create_dir_all(cfg.specs_dir()).unwrap();
        write_canonical_spec(
            &cfg,
            "auth",
            "# auth Specification\n\n## Purpose\n\nAuthentication of family accounts across every client surface.\n\n\
             ## Requirements\n\n### Requirement: Login\n\
             The system SHALL authenticate users.\n\n\
             #### Scenario: Valid\n- **WHEN** valid\n- **THEN** ok\n",
        );
        let result = validate_spec(&cfg, "auth", false).unwrap();
        assert!(result.valid, "a well-formed spec must be valid: {result:?}");
        assert!(result.issues.is_empty(), "no issues expected: {result:?}");
    }

    #[test]
    fn validate_spec_rejects_missing_purpose() {
        let tmp = TempDir::new();
        let cfg = cfg(&tmp);
        fs::create_dir_all(cfg.specs_dir()).unwrap();
        write_canonical_spec(
            &cfg,
            "auth",
            "# auth Specification\n\n## Requirements\n\n### Requirement: Login\n\
             The system SHALL authenticate users.\n\n\
             #### Scenario: Valid\n- **WHEN** valid\n- **THEN** ok\n",
        );
        let result = validate_spec(&cfg, "auth", false).unwrap();
        assert!(!result.valid, "missing Purpose must fail: {result:?}");
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.message.contains("Purpose")),
            "must report missing Purpose: {result:?}"
        );
    }

    #[test]
    fn validate_spec_rejects_empty_requirements() {
        let tmp = TempDir::new();
        let cfg = cfg(&tmp);
        fs::create_dir_all(cfg.specs_dir()).unwrap();
        write_canonical_spec(
            &cfg,
            "auth",
            "# auth Specification\n\n## Purpose\n\nAuthentication.\n\n## Requirements\n",
        );
        let result = validate_spec(&cfg, "auth", false).unwrap();
        assert!(!result.valid, "empty requirements must fail: {result:?}");
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.message.contains("at least one requirement")),
            "must report empty requirements: {result:?}"
        );
    }

    #[test]
    fn validate_spec_placeholder_purpose_is_warning_not_error() {
        let tmp = TempDir::new();
        let cfg = cfg(&tmp);
        fs::create_dir_all(cfg.specs_dir()).unwrap();
        write_canonical_spec(
            &cfg,
            "auth",
            "# auth Specification\n\n## Purpose\n\nTBD\n\n\
             ## Requirements\n\n### Requirement: Login\n\
             The system SHALL authenticate users.\n\n\
             #### Scenario: Valid\n- **WHEN** valid\n- **THEN** ok\n",
        );
        let result = validate_spec(&cfg, "auth", false).unwrap();
        assert!(
            result.valid,
            "placeholder purpose is WARNING, not ERROR in non-strict: {result:?}"
        );
        assert!(
            result
                .issues
                .iter()
                .any(|issue| issue.message.contains("placeholder")),
            "must warn about placeholder: {result:?}"
        );
        let strict_result = validate_spec(&cfg, "auth", true).unwrap();
        assert!(
            !strict_result.valid,
            "placeholder purpose must fail in strict: {strict_result:?}"
        );
    }
}
