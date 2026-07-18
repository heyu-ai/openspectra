//! Four-dimension artifact analysis for `spectra analyze`.
//!
//! Findings describe artifact quality but do not form a pass/fail gate. The
//! CLI therefore always exits successfully when this module returns a report,
//! regardless of how many Critical, Warning, or Suggestion findings it holds.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

use crate::{change, config::Config, schema};

const DIMENSION_COVERAGE: &str = "Coverage";
const DIMENSION_CONSISTENCY: &str = "Consistency";
const DIMENSION_AMBIGUITY: &str = "Ambiguity";
const DIMENSION_GAPS: &str = "Gaps";

/// A localized message reference. `params` is deliberately always serialized,
/// including when empty, because consumers distinguish an empty object from a
/// missing field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FindingMessage {
    pub key: String,
    pub params: BTreeMap<String, String>,
}

/// One analysis finding. Declaration order pins the JSON key order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub id: String,
    pub dimension: String,
    pub severity: String,
    pub location: String,
    pub summary: String,
    pub recommendation: String,
    pub summary_msg: FindingMessage,
    pub recommendation_msg: FindingMessage,
}

/// The status of one of the four dimensions. Declaration order pins the JSON
/// key order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DimensionReport {
    pub dimension: String,
    pub status: String,
    pub finding_count: usize,
}

/// Complete `spectra analyze --json` payload. Field declaration order is the
/// measured top-level key order, and names intentionally remain snake_case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnalyzeReport {
    pub change_id: String,
    pub dimensions: Vec<DimensionReport>,
    pub findings: Vec<Finding>,
    pub artifacts_analyzed: Vec<String>,
    pub artifacts_missing: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArtifactPresence {
    proposal: bool,
    specs: bool,
    design: bool,
    tasks: bool,
}

impl ArtifactPresence {
    fn from_change_dir(change_dir: &Path) -> Self {
        let present = |id| {
            let artifact = schema::artifacts()
                .iter()
                .find(|artifact| artifact.id == id)
                .expect("all analyze artifacts exist in the built-in schema");
            schema::artifact_done(artifact, change_dir)
        };
        Self {
            proposal: present("proposal"),
            specs: present("specs"),
            design: present("design"),
            tasks: present("tasks"),
        }
    }

    fn gates(self) -> [bool; 4] {
        let coverage_inputs = [self.proposal, self.specs, self.tasks]
            .into_iter()
            .filter(|present| *present)
            .count();
        [
            coverage_inputs >= 2,
            self.design && self.tasks,
            self.specs,
            self.proposal || self.specs || self.design || self.tasks,
        ]
    }

    fn ordered(self) -> [(&'static str, bool); 4] {
        [
            ("proposal", self.proposal),
            ("specs", self.specs),
            ("design", self.design),
            ("tasks", self.tasks),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DeltaSection {
    Added,
    Modified,
    Removed,
    Renamed,
}

impl DeltaSection {
    const ORDERED: [Self; 4] = [Self::Added, Self::Modified, Self::Removed, Self::Renamed];

    fn from_heading(line: &str) -> Option<Self> {
        match line.trim() {
            "## ADDED Requirements" => Some(Self::Added),
            "## MODIFIED Requirements" => Some(Self::Modified),
            "## REMOVED Requirements" => Some(Self::Removed),
            "## RENAMED Requirements" => Some(Self::Renamed),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Added => "ADDED",
            Self::Modified => "MODIFIED",
            Self::Removed => "REMOVED",
            Self::Renamed => "RENAMED",
        }
    }
}

#[derive(Debug)]
struct DeltaRequirement {
    name: String,
    section: DeltaSection,
    order: usize,
}

#[derive(Debug)]
struct ParsedDelta {
    all_requirements: Vec<String>,
    modified_requirements: Vec<String>,
    validation_errors: Vec<String>,
}

#[derive(Debug)]
struct SpecFile {
    relative_path: String,
    capability: String,
    content: String,
    delta: ParsedDelta,
}

fn requirement_name(line: &str) -> Option<String> {
    line.trim()
        .strip_prefix("### Requirement:")
        .map(|rest| rest.trim().to_string())
}

fn parse_delta(content: &str) -> ParsedDelta {
    let mut current_section = None;
    let mut all_requirements = Vec::new();
    let mut delta_requirements = Vec::new();

    for (order, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            current_section = DeltaSection::from_heading(trimmed);
            continue;
        }
        let Some(name) = requirement_name(line) else {
            continue;
        };
        all_requirements.push(name.clone());
        if let Some(section) = current_section {
            delta_requirements.push(DeltaRequirement {
                name,
                section,
                order,
            });
        }
    }

    let modified_requirements = delta_requirements
        .iter()
        .filter(|requirement| requirement.section == DeltaSection::Modified)
        .map(|requirement| requirement.name.clone())
        .collect();

    let mut validation_errors = Vec::new();
    // The oracle did not expose multi-error ordering. OpenSpectra makes it
    // deterministic: duplicates use canonical section order and document
    // order, followed by cross-section conflicts in first-name order.
    for section in DeltaSection::ORDERED {
        let mut seen = HashSet::new();
        for requirement in delta_requirements
            .iter()
            .filter(|requirement| requirement.section == section)
        {
            if !seen.insert(requirement.name.as_str()) {
                validation_errors.push(format!(
                    "Duplicate requirement '{}' in {} section",
                    requirement.name,
                    section.label()
                ));
            }
        }
    }

    let mut names_in_order = Vec::new();
    let mut sections_by_name: HashMap<&str, Vec<(DeltaSection, usize)>> = HashMap::new();
    for requirement in &delta_requirements {
        let sections = sections_by_name
            .entry(&requirement.name)
            .or_insert_with(|| {
                names_in_order.push(requirement.name.as_str());
                Vec::new()
            });
        if !sections
            .iter()
            .any(|(section, _)| *section == requirement.section)
        {
            sections.push((requirement.section, requirement.order));
        }
    }
    for name in names_in_order {
        let sections = sections_by_name
            .get_mut(name)
            .expect("a name recorded in order has collected sections");
        sections.sort_by_key(|(_, order)| *order);
        for left in 0..sections.len() {
            for right in (left + 1)..sections.len() {
                validation_errors.push(format!(
                    "Requirement '{name}' appears in both {} and {} sections",
                    sections[left].0.label(),
                    sections[right].0.label()
                ));
            }
        }
    }

    ParsedDelta {
        all_requirements,
        modified_requirements,
        validation_errors,
    }
}

fn slash_relative(base: &Path, path: &Path) -> Result<String> {
    let relative = path
        .strip_prefix(base)
        .map_err(|_| anyhow!("{} is outside {}", path.display(), base.display()))?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let raw = component.as_os_str();
        let part = raw
            .to_str()
            .ok_or_else(|| anyhow!("path component {raw:?} is not valid UTF-8"))?;
        parts.push(part);
    }
    Ok(parts.join("/"))
}

fn collect_markdown_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
    let Some(entries) = crate::fsutil::read_dir_optional(dir)? else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading {}", path.display()))?;
        if file_type.is_dir() {
            collect_markdown_paths(&path, paths)?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "md") {
            paths.push(path);
        }
    }
    Ok(())
}

fn collect_spec_files(change_dir: &Path) -> Result<Vec<SpecFile>> {
    let specs_dir = change_dir.join("specs");
    let mut paths = Vec::new();
    collect_markdown_paths(&specs_dir, &mut paths)?;

    let mut keyed_paths = Vec::with_capacity(paths.len());
    for path in paths {
        let relative = slash_relative(change_dir, &path)?;
        keyed_paths.push((relative, path));
    }
    // The oracle uses readdir order. Like WP3 contextFiles, OpenSpectra sorts
    // relative paths so identical trees produce stable reports everywhere.
    keyed_paths.sort_by(|left, right| left.0.cmp(&right.0));

    let mut files = Vec::with_capacity(keyed_paths.len());
    for (relative_path, path) in keyed_paths {
        let parent = path
            .parent()
            .expect("a discovered markdown file always has a parent");
        let capability = slash_relative(&specs_dir, parent)?;
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let delta = parse_delta(&content);
        files.push(SpecFile {
            relative_path,
            capability,
            content,
            delta,
        });
    }
    Ok(files)
}

fn extract_capabilities(proposal: &str) -> Vec<String> {
    let mut in_capabilities = false;
    let mut capabilities = Vec::new();
    for line in proposal.lines() {
        let trimmed = line.trim();
        if !in_capabilities {
            if trimmed == "## Capabilities" {
                in_capabilities = true;
            }
            continue;
        }
        if trimmed.starts_with("## ") {
            break;
        }
        if !trimmed.starts_with('-') {
            continue;
        }
        let Some(open) = trimmed.find('`') else {
            continue;
        };
        let rest = &trimmed[(open + 1)..];
        let Some(close) = rest.find('`') else {
            continue;
        };
        capabilities.push(rest[..close].to_string());
    }
    capabilities
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StructureIssue {
    NoScenario(String),
    AbstractScenario(String),
}

#[derive(Debug)]
struct RequirementStructure {
    name: String,
    order: usize,
    scenario_count: usize,
}

#[derive(Debug)]
struct ScenarioStructure {
    name: String,
    order: usize,
    has_example: bool,
}

fn structure_issues(content: &str) -> Vec<StructureIssue> {
    let mut requirements: Vec<RequirementStructure> = Vec::new();
    let mut scenarios: Vec<ScenarioStructure> = Vec::new();
    let mut current_requirement = None;
    let mut current_scenario = None;

    for (order, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if DeltaSection::from_heading(trimmed).is_some() {
            current_requirement = None;
            current_scenario = None;
            continue;
        }
        if let Some(name) = requirement_name(line) {
            requirements.push(RequirementStructure {
                name,
                order,
                scenario_count: 0,
            });
            current_requirement = Some(requirements.len() - 1);
            current_scenario = None;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("#### Scenario:") {
            scenarios.push(ScenarioStructure {
                name: rest.trim().to_string(),
                order,
                has_example: false,
            });
            current_scenario = Some(scenarios.len() - 1);
            if let Some(index) = current_requirement {
                requirements[index].scenario_count += 1;
            }
            continue;
        }
        if trimmed.starts_with("##### Example:") {
            if let Some(index) = current_scenario {
                scenarios[index].has_example = true;
            }
        }
    }

    let mut ordered = Vec::new();
    for requirement in requirements {
        if requirement.scenario_count == 0 {
            ordered.push((
                requirement.order,
                StructureIssue::NoScenario(requirement.name),
            ));
        }
    }
    for scenario in scenarios {
        if !scenario.has_example {
            ordered.push((
                scenario.order,
                StructureIssue::AbstractScenario(scenario.name),
            ));
        }
    }
    ordered.sort_by_key(|(order, _)| *order);
    ordered.into_iter().map(|(_, issue)| issue).collect()
}

fn weak_language_pattern(line: &str) -> Option<&'static str> {
    let lowercase = line.to_lowercase();
    [
        ("should", "should"),
        ("may", "may"),
        ("might", "might"),
        ("tbd", "TBD"),
        ("???", "???"),
    ]
    .into_iter()
    .find_map(|(needle, canonical)| lowercase.contains(needle).then_some(canonical))
}

fn params(values: &[(&str, &str)]) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

struct FindingText {
    severity: &'static str,
    location: String,
    summary: String,
    recommendation: String,
    key: &'static str,
    summary_params: BTreeMap<String, String>,
    recommendation_params: BTreeMap<String, String>,
}

fn push_finding(findings: &mut Vec<Finding>, prefix: &str, dimension: &str, text: FindingText) {
    findings.push(Finding {
        id: format!("{prefix}-{}", findings.len() + 1),
        dimension: dimension.to_string(),
        severity: text.severity.to_string(),
        location: text.location,
        summary: text.summary,
        recommendation: text.recommendation,
        summary_msg: FindingMessage {
            key: format!("{}.summary", text.key),
            params: text.summary_params,
        },
        recommendation_msg: FindingMessage {
            key: format!("{}.recommendation", text.key),
            params: text.recommendation_params,
        },
    });
}

fn coverage_findings(
    change_dir: &Path,
    presence: ArtifactPresence,
    proposal: Option<&str>,
    tasks: Option<&str>,
    spec_files: &[SpecFile],
) -> Vec<Finding> {
    let mut findings = Vec::new();
    if presence.proposal {
        for capability in extract_capabilities(proposal.expect("present proposal was loaded")) {
            if !change_dir
                .join("specs")
                .join(&capability)
                .join("spec.md")
                .is_file()
            {
                push_finding(
                    &mut findings,
                    "COV",
                    DIMENSION_COVERAGE,
                    FindingText {
                        severity: "Critical",
                        location: "proposal.md → Capabilities".to_string(),
                        summary: format!(
                            "Capability `{capability}` has no corresponding spec file"
                        ),
                        recommendation: format!(
                            "Create specs/{capability}/spec.md with requirements"
                        ),
                        key: "covMissingSpec",
                        summary_params: params(&[("cap", &capability)]),
                        recommendation_params: params(&[("cap", &capability)]),
                    },
                );
            }
        }
    }

    let tasks_lowercase = tasks.map(str::to_lowercase);
    for spec_file in spec_files {
        if presence.tasks && presence.specs {
            let tasks_lowercase = tasks_lowercase
                .as_deref()
                .expect("present tasks were loaded for Coverage");
            for requirement in &spec_file.delta.all_requirements {
                if !tasks_lowercase.contains(&requirement.to_lowercase()) {
                    push_finding(
                        &mut findings,
                        "COV",
                        DIMENSION_COVERAGE,
                        FindingText {
                            severity: "Warning",
                            location: spec_file.relative_path.clone(),
                            summary: format!("Requirement '{requirement}' has no matching task"),
                            recommendation: format!(
                                "Add a task in tasks.md that references '{requirement}'"
                            ),
                            key: "covMissingTask",
                            summary_params: params(&[("req", requirement)]),
                            recommendation_params: params(&[("req", requirement)]),
                        },
                    );
                }
            }
        }
        for error in &spec_file.delta.validation_errors {
            push_finding(
                &mut findings,
                "COV",
                DIMENSION_COVERAGE,
                FindingText {
                    severity: "Critical",
                    location: spec_file.relative_path.clone(),
                    summary: format!("Delta spec validation error: {error}"),
                    recommendation: "Fix the delta spec structure".to_string(),
                    key: "covDeltaValidation",
                    summary_params: params(&[("error", error)]),
                    recommendation_params: params(&[]),
                },
            );
        }
    }
    findings
}

fn consistency_findings(design: &str, tasks: &str) -> Vec<Finding> {
    let tasks_lowercase = tasks.to_lowercase();
    let mut findings = Vec::new();
    for line in design.lines() {
        let Some(topic) = line.trim_start().strip_prefix("### ") else {
            continue;
        };
        let keyword = topic.trim().to_lowercase();
        if !tasks_lowercase.contains(&keyword) {
            push_finding(
                &mut findings,
                "CON",
                DIMENSION_CONSISTENCY,
                FindingText {
                    severity: "Warning",
                    location: "design.md".to_string(),
                    summary: format!("Design topic '{keyword}' not referenced in tasks"),
                    recommendation: "Verify tasks cover this design decision".to_string(),
                    key: "conDesignNotInTasks",
                    summary_params: params(&[("keyword", &keyword)]),
                    recommendation_params: params(&[]),
                },
            );
        }
    }
    findings
}

fn ambiguity_findings(spec_files: &[SpecFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for spec_file in spec_files {
        for issue in structure_issues(&spec_file.content) {
            match issue {
                StructureIssue::NoScenario(requirement) => push_finding(
                    &mut findings,
                    "AMB",
                    DIMENSION_AMBIGUITY,
                    FindingText {
                        severity: "Warning",
                        location: spec_file.relative_path.clone(),
                        summary: format!("Requirement '{requirement}' has no scenarios"),
                        recommendation: format!(
                            "Add #### Scenario: sections with WHEN/THEN for '{requirement}'"
                        ),
                        key: "ambNoScenario",
                        summary_params: params(&[("req", &requirement)]),
                        recommendation_params: params(&[("req", &requirement)]),
                    },
                ),
                StructureIssue::AbstractScenario(scenario) => push_finding(
                    &mut findings,
                    "AMB",
                    DIMENSION_AMBIGUITY,
                    FindingText {
                        severity: "Suggestion",
                        location: spec_file.relative_path.clone(),
                        summary: format!("Scenario '{scenario}' has no concrete examples"),
                        recommendation: "Add ##### Example: with concrete GIVEN/WHEN/THEN data"
                            .to_string(),
                        key: "ambAbstractScenario",
                        summary_params: params(&[("scenario", &scenario)]),
                        recommendation_params: params(&[("scenario", &scenario)]),
                    },
                ),
            }
        }
        for (line_index, line) in spec_file.content.lines().enumerate() {
            let Some(pattern) = weak_language_pattern(line) else {
                continue;
            };
            push_finding(
                &mut findings,
                "AMB",
                DIMENSION_AMBIGUITY,
                FindingText {
                    severity: "Suggestion",
                    location: format!("{}:{}", spec_file.relative_path, line_index + 1),
                    summary: format!("Vague language '{pattern}' found"),
                    recommendation: format!("Replace '{pattern}' with SHALL/SHALL NOT for clarity"),
                    key: "ambWeakLanguage",
                    summary_params: params(&[("pattern", pattern)]),
                    recommendation_params: params(&[("pattern", pattern)]),
                },
            );
        }
    }
    findings
}

fn gaps_findings(
    cfg: &Config,
    presence: ArtifactPresence,
    spec_files: &[SpecFile],
) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    if presence.specs && !presence.proposal {
        push_finding(
            &mut findings,
            "GAP",
            DIMENSION_GAPS,
            FindingText {
                severity: "Critical",
                location: "change directory".to_string(),
                summary: "Specs exist but no proposal.md found".to_string(),
                recommendation: "Create proposal.md describing the change purpose".to_string(),
                key: "gapNoProposal",
                summary_params: params(&[]),
                recommendation_params: params(&[]),
            },
        );
    }

    for spec_file in spec_files {
        if spec_file.delta.modified_requirements.is_empty() {
            continue;
        }
        let main_spec_path = cfg.specs_dir().join(&spec_file.capability).join("spec.md");
        if !main_spec_path.is_file() {
            push_finding(
                &mut findings,
                "GAP",
                DIMENSION_GAPS,
                FindingText {
                    severity: "Warning",
                    location: spec_file.relative_path.clone(),
                    summary: format!(
                        "MODIFIED requirements reference capability '{}' but no main spec found",
                        spec_file.capability
                    ),
                    recommendation: format!(
                        "Check if openspec/specs/{}/spec.md exists",
                        spec_file.capability
                    ),
                    key: "gapNoMainSpec",
                    summary_params: params(&[("spec", &spec_file.capability)]),
                    recommendation_params: params(&[("spec", &spec_file.capability)]),
                },
            );
            continue;
        }

        let main_spec = std::fs::read_to_string(&main_spec_path)
            .with_context(|| format!("reading {}", main_spec_path.display()))?;
        let main_requirements: HashSet<String> =
            main_spec.lines().filter_map(requirement_name).collect();
        for requirement in &spec_file.delta.modified_requirements {
            if !main_requirements.contains(requirement) {
                push_finding(
                    &mut findings,
                    "GAP",
                    DIMENSION_GAPS,
                    FindingText {
                        severity: "Warning",
                        location: spec_file.relative_path.clone(),
                        summary: format!(
                            "MODIFIED requirement '{requirement}' not found in main spec"
                        ),
                        recommendation: format!(
                            "Verify requirement '{requirement}' exists in openspec/specs/{}/spec.md",
                            spec_file.capability
                        ),
                        key: "gapModifiedNotFound",
                        summary_params: params(&[("name", requirement)]),
                        recommendation_params: params(&[
                            ("name", requirement),
                            ("spec", &spec_file.capability),
                        ]),
                    },
                );
            }
        }
    }
    Ok(findings)
}

fn dimension_report(dimension: &str, ran: bool, finding_count: usize) -> DimensionReport {
    let status = if !ran {
        "Skipped (insufficient artifacts)".to_string()
    } else if finding_count == 0 {
        "Clean".to_string()
    } else {
        format!("{finding_count} issue(s) found")
    };
    DimensionReport {
        dimension: dimension.to_string(),
        status,
        finding_count,
    }
}

/// Analyze one already-resolved change name. Missing changes use the same
/// exact operational error as `status`.
pub fn analyze(cfg: &Config, change_name: &str) -> Result<AnalyzeReport> {
    let change = change::try_load(cfg, change_name)?
        .ok_or_else(|| anyhow!("Change '{change_name}' not found."))?;
    let presence = ArtifactPresence::from_change_dir(&change.dir);
    let gates = presence.gates();
    let spec_files = if presence.specs {
        collect_spec_files(&change.dir)?
    } else {
        Vec::new()
    };

    let proposal = if gates[0] && presence.proposal {
        Some(
            std::fs::read_to_string(change.proposal_md())
                .with_context(|| format!("reading {}", change.proposal_md().display()))?,
        )
    } else {
        None
    };
    let tasks_needed = (gates[0] && presence.specs && presence.tasks) || gates[1];
    let tasks = if tasks_needed {
        Some(
            std::fs::read_to_string(change.tasks_md())
                .with_context(|| format!("reading {}", change.tasks_md().display()))?,
        )
    } else {
        None
    };
    let design = if gates[1] {
        Some(
            std::fs::read_to_string(change.design_md())
                .with_context(|| format!("reading {}", change.design_md().display()))?,
        )
    } else {
        None
    };

    let coverage = if gates[0] {
        coverage_findings(
            &change.dir,
            presence,
            proposal.as_deref(),
            tasks.as_deref(),
            &spec_files,
        )
    } else {
        Vec::new()
    };
    let consistency = if gates[1] {
        consistency_findings(
            design.as_deref().expect("gated design was loaded"),
            tasks.as_deref().expect("gated tasks were loaded"),
        )
    } else {
        Vec::new()
    };
    let ambiguity = if gates[2] {
        ambiguity_findings(&spec_files)
    } else {
        Vec::new()
    };
    let gaps = if gates[3] {
        gaps_findings(cfg, presence, &spec_files)?
    } else {
        Vec::new()
    };

    let dimensions = vec![
        dimension_report(DIMENSION_COVERAGE, gates[0], coverage.len()),
        dimension_report(DIMENSION_CONSISTENCY, gates[1], consistency.len()),
        dimension_report(DIMENSION_AMBIGUITY, gates[2], ambiguity.len()),
        dimension_report(DIMENSION_GAPS, gates[3], gaps.len()),
    ];
    let findings = coverage
        .into_iter()
        .chain(consistency)
        .chain(ambiguity)
        .chain(gaps)
        .collect();

    let mut artifacts_analyzed = Vec::new();
    let mut artifacts_missing = Vec::new();
    for (artifact, present) in presence.ordered() {
        if present {
            artifacts_analyzed.push(artifact.to_string());
        } else {
            artifacts_missing.push(artifact.to_string());
        }
    }

    Ok(AnalyzeReport {
        change_id: change.name,
        dimensions,
        findings,
        artifacts_analyzed,
        artifacts_missing,
    })
}

/// Render the measured plain-text report. Analyze output intentionally ignores
/// color settings, matching `status` and `instructions` human output.
pub fn format_human(report: &AnalyzeReport) -> String {
    let mut output = format!("Change: {}\n\n", report.change_id);
    for dimension in &report.dimensions {
        let glyph = if dimension.finding_count == 0 {
            "✓"
        } else {
            "●"
        };
        output.push_str(&format!(
            "  {glyph} {:<15}{} ({} findings)\n",
            dimension.dimension, dimension.status, dimension.finding_count
        ));
    }
    if !report.artifacts_analyzed.is_empty() {
        output.push('\n');
        output.push_str(&format!(
            "  Analyzed: {}\n",
            report.artifacts_analyzed.join(", ")
        ));
    }
    if !report.artifacts_missing.is_empty() {
        output.push_str(&format!(
            "  Missing: {}\n",
            report.artifacts_missing.join(", ")
        ));
    }
    output.push('\n');
    if report.findings.is_empty() {
        output.push_str("  ✓ No issues found\n");
        return output;
    }

    output.push_str(&format!("  Findings ({}):\n\n", report.findings.len()));
    for finding in &report.findings {
        let severity = match finding.severity.as_str() {
            "Critical" => "CRITICAL",
            "Warning" => "WARNING",
            "Suggestion" => "SUGGEST",
            other => other,
        };
        output.push_str(&format!("  [{severity}] {}\n", finding.summary));
        output.push_str(&format!("    at: {}\n", finding.location));
        output.push_str(&format!("    → {}\n", finding.recommendation));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_first_backtick_token_from_capability_bullets_only() {
        let proposal = "# Proposal\n\
            ## Capabilities\n\
            ### New Capabilities\n\
            - `alpha`: first `ignored`\n\
            - nobacktick: ignored\n\
            prose mentions `not-a-bullet`\n\
            ### Modified Capabilities\n\
            - `<name>`: template placeholder\n\
            ## Impact\n\
            - `after-section`: ignored\n";

        assert_eq!(extract_capabilities(proposal), ["alpha", "<name>"]);
    }

    #[test]
    fn parses_requirements_from_all_sections_and_reports_structure_in_document_order() {
        let spec = "#### Scenario: orphan\n\
            ## ADDED Requirements\n\
            ### Requirement: Added without scenario\n\
            ### Requirement: Added with example\n\
            #### Scenario: concrete\n\
            ##### Example: one\n\
            ## REMOVED Requirements\n\
            ### Requirement: Removed with abstract scenario\n\
            #### Scenario: abstract\n\
            ## RENAMED Requirements\n\
            ### Requirement: Rename body\n";
        let delta = parse_delta(spec);
        assert_eq!(
            delta.all_requirements,
            [
                "Added without scenario",
                "Added with example",
                "Removed with abstract scenario",
                "Rename body",
            ]
        );
        assert_eq!(
            structure_issues(spec),
            [
                StructureIssue::AbstractScenario("orphan".to_string()),
                StructureIssue::NoScenario("Added without scenario".to_string()),
                StructureIssue::AbstractScenario("abstract".to_string()),
                StructureIssue::NoScenario("Rename body".to_string()),
            ]
        );
    }

    #[test]
    fn weak_language_is_case_insensitive_substring_priority_and_canonicalized() {
        assert_eq!(
            weak_language_pattern("TBD first then SHOULD later"),
            Some("should")
        );
        assert_eq!(weak_language_pattern("MAYBE this works"), Some("may"));
        assert_eq!(weak_language_pattern("status tbd"), Some("TBD"));
        assert_eq!(weak_language_pattern("fully specified"), None);
    }

    #[test]
    fn delta_validation_orders_duplicates_then_cross_section_conflicts() {
        let spec = "## MODIFIED Requirements\n\
            ### Requirement: Shared\n\
            ## ADDED Requirements\n\
            ### Requirement: Duplicate\n\
            ### Requirement: Duplicate\n\
            ### Requirement: Shared\n";

        assert_eq!(
            parse_delta(spec).validation_errors,
            [
                "Duplicate requirement 'Duplicate' in ADDED section",
                "Requirement 'Shared' appears in both MODIFIED and ADDED sections",
            ]
        );
    }

    #[test]
    fn gating_matrix_matches_each_dimension_contract() {
        let presence = |proposal, specs, design, tasks| ArtifactPresence {
            proposal,
            specs,
            design,
            tasks,
        };

        assert_eq!(presence(false, false, false, false).gates(), [false; 4]);
        assert_eq!(
            presence(true, false, false, false).gates(),
            [false, false, false, true]
        );
        assert_eq!(
            presence(false, true, false, false).gates(),
            [false, false, true, true]
        );
        assert_eq!(
            presence(false, false, true, false).gates(),
            [false, false, false, true]
        );
        assert_eq!(
            presence(false, false, false, true).gates(),
            [false, false, false, true]
        );
        assert_eq!(
            presence(true, false, false, true).gates(),
            [true, false, false, true]
        );
        assert_eq!(
            presence(false, true, false, true).gates(),
            [true, false, true, true]
        );
        assert_eq!(
            presence(true, true, false, false).gates(),
            [true, false, true, true]
        );
        assert_eq!(
            presence(false, false, true, true).gates(),
            [false, true, false, true]
        );
    }
}
