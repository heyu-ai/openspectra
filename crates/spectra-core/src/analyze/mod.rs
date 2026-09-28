//! Five-dimension artifact analysis for `spectra analyze`（對齊 oracle 3.0.0）。
//!
//! Findings describe artifact quality but do not form a pass/fail gate. The
//! CLI therefore always exits successfully when this module returns a report,
//! regardless of how many Critical, Warning, or Suggestion findings it holds.
//!
//! 子模組對應 oracle 的 `spectra_core::analyzer::*`（見
//! `docs/reverse-engineering/analyze.md`）：`extract` 解析 markdown，
//! `coverage`／`consistency`／`ambiguity`／`gaps`／`localization` 各自產生一個
//! dimension 的 findings。

mod ambiguity;
mod consistency;
mod coverage;
mod extract;
mod gaps;
mod localization;
mod numeric;

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

use crate::{change, config::Config};

use extract::SpecFile;

const DIMENSION_COVERAGE: &str = "Coverage";
const DIMENSION_CONSISTENCY: &str = "Consistency";
const DIMENSION_AMBIGUITY: &str = "Ambiguity";
const DIMENSION_GAPS: &str = "Gaps";
const DIMENSION_LOCALIZATION: &str = "Localization";

/// A localized message reference. `params` is deliberately always serialized,
/// including when empty, because consumers distinguish an empty object from a
/// missing field. oracle 以 HashMap 序列化（多 key 時每次執行順序不同）；
/// OpenSpectra 固定為字母序（刻意分歧，見 analyze.md）。
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

/// The status of one of the five dimensions. Declaration order pins the JSON
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

/// 四個 artifact 是否存在。analyze 的 dimension 寫死 spec-driven 的
/// proposal/specs/design/tasks 形狀，不隨專案的 schema 改變（與 oracle 相同）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ArtifactPresence {
    proposal: bool,
    specs: bool,
    design: bool,
    tasks: bool,
}

/// 各 dimension 是否執行（oracle 3.0.0 探測 p03 的 16 種組合）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Gates {
    coverage: bool,
    consistency: bool,
    ambiguity: bool,
    gaps: bool,
}

impl ArtifactPresence {
    fn count(self) -> usize {
        [self.proposal, self.specs, self.design, self.tasks]
            .into_iter()
            .filter(|present| *present)
            .count()
    }

    fn gates(self) -> Gates {
        Gates {
            coverage: self.count() >= 2,
            consistency: self.design || (self.proposal && self.tasks),
            ambiguity: self.specs,
            gaps: self.count() >= 1,
        }
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

/// 單一 finding 的內容；id 與 dimension 由 [`number`] 依序補上。
struct FindingText {
    severity: &'static str,
    location: String,
    summary: String,
    recommendation: String,
    key: &'static str,
    summary_params: BTreeMap<String, String>,
    recommendation_params: BTreeMap<String, String>,
}

fn params(values: &[(&str, &str)]) -> BTreeMap<String, String> {
    values
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

/// 依 dimension 內的順序編號（`<PREFIX>-1` 起）。
fn number(prefix: &str, dimension: &str, texts: Vec<FindingText>) -> Vec<Finding> {
    texts
        .into_iter()
        .enumerate()
        .map(|(index, text)| Finding {
            id: format!("{prefix}-{}", index + 1),
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
        })
        .collect()
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

fn read_optional(path: &Path) -> Result<Option<String>> {
    if !path.is_file() {
        return Ok(None);
    }
    std::fs::read_to_string(path)
        .map(Some)
        .with_context(|| format!("reading {}", path.display()))
}

/// Analyze one already-resolved change name. Missing changes use the same
/// exact operational error as `status`.
pub fn analyze(cfg: &Config, change_name: &str) -> Result<AnalyzeReport> {
    let change = change::try_load(cfg, change_name)?
        .ok_or_else(|| anyhow!("Change '{change_name}' not found."))?;
    let proposal = read_optional(&change.proposal_md())?;
    let design = read_optional(&change.design_md())?;
    let tasks = read_optional(&change.tasks_md())?;
    let spec_files = extract::collect_spec_files(&change.dir)?;
    let presence = ArtifactPresence {
        proposal: proposal.is_some(),
        specs: !spec_files.is_empty(),
        design: design.is_some(),
        tasks: tasks.is_some(),
    };
    let gates = presence.gates();
    let artifacts = Artifacts {
        proposal: proposal.as_deref(),
        design: design.as_deref(),
        tasks: tasks.as_deref(),
        spec_files: &spec_files,
    };

    let localization_ran = localization::runs(cfg.locale.as_deref(), &artifacts);
    let localization = if localization_ran {
        number(
            "LOC",
            DIMENSION_LOCALIZATION,
            localization::findings(cfg.locale.as_deref().unwrap_or_default(), &artifacts),
        )
    } else {
        Vec::new()
    };
    let coverage = if gates.coverage {
        number("COV", DIMENSION_COVERAGE, coverage::findings(&artifacts))
    } else {
        Vec::new()
    };
    let consistency = if gates.consistency {
        number(
            "CON",
            DIMENSION_CONSISTENCY,
            consistency::findings(&artifacts),
        )
    } else {
        Vec::new()
    };
    let ambiguity = if gates.ambiguity {
        number("AMB", DIMENSION_AMBIGUITY, ambiguity::findings(&spec_files))
    } else {
        Vec::new()
    };
    let gaps = if gates.gaps {
        number("GAP", DIMENSION_GAPS, gaps::findings(cfg, &artifacts)?)
    } else {
        Vec::new()
    };

    let dimensions = vec![
        dimension_report(DIMENSION_COVERAGE, gates.coverage, coverage.len()),
        dimension_report(DIMENSION_CONSISTENCY, gates.consistency, consistency.len()),
        dimension_report(DIMENSION_AMBIGUITY, gates.ambiguity, ambiguity.len()),
        dimension_report(DIMENSION_GAPS, gates.gaps, gaps.len()),
        dimension_report(DIMENSION_LOCALIZATION, localization_ran, localization.len()),
    ];
    // Localization 在 dimensions 排最後，findings 卻排最前（oracle 3.0.0 p26b／p64）。
    let findings = localization
        .into_iter()
        .chain(coverage)
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

/// 各規則共用的 artifact 內容；缺少的 artifact 為 `None`（`spec_files` 為空）。
struct Artifacts<'a> {
    proposal: Option<&'a str>,
    design: Option<&'a str>,
    tasks: Option<&'a str>,
    spec_files: &'a [SpecFile],
}

/// 開啟顏色時包上 SGR 碼。
fn paint(text: &str, sgr: &str, use_color: bool) -> String {
    if use_color {
        format!("\x1b[{sgr}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

/// Human 報表（oracle 3.0.0 p26／p27／p28）。`use_color` 時照 oracle 的 SGR
/// 配色：標題粗體、dimension 名稱粗體、狀態 dim、✓ 綠、● 一律黃（即使有
/// Critical）、`Missing:` 黃、嚴重度 CRITICAL 粗紅／WARNING 黃／SUGGEST dim。
pub fn format_human(report: &AnalyzeReport, use_color: bool) -> String {
    let mut output = format!(
        "{}: {}\n\n",
        paint("Change", "1", use_color),
        report.change_id
    );
    for dimension in &report.dimensions {
        let glyph = if dimension.finding_count == 0 {
            paint("✓", "32", use_color)
        } else {
            paint("●", "33", use_color)
        };
        if use_color {
            output.push_str(&format!(
                "  {glyph} {} {} ({} findings)\n",
                paint(&format!("{:<14}", dimension.dimension), "1", true),
                paint(&dimension.status, "2", true),
                dimension.finding_count
            ));
        } else {
            output.push_str(&format!(
                "  {glyph} {:<15}{} ({} findings)\n",
                dimension.dimension, dimension.status, dimension.finding_count
            ));
        }
    }
    if !report.artifacts_analyzed.is_empty() {
        output.push('\n');
        output.push_str(&format!(
            "  {} {}\n",
            paint("Analyzed:", "2", use_color),
            report.artifacts_analyzed.join(", ")
        ));
    }
    if !report.artifacts_missing.is_empty() {
        output.push_str(&format!(
            "  {} {}\n",
            paint("Missing:", "33", use_color),
            report.artifacts_missing.join(", ")
        ));
    }
    output.push('\n');
    if report.findings.is_empty() {
        output.push_str(&format!(
            "  {} No issues found\n",
            paint("✓", "32", use_color)
        ));
        return output;
    }

    output.push_str(&format!(
        "  {} ({}):\n\n",
        paint("Findings", "1", use_color),
        report.findings.len()
    ));
    for finding in &report.findings {
        let severity = match finding.severity.as_str() {
            "Critical" => paint("CRITICAL", "1;31", use_color),
            "Warning" => paint("WARNING", "33", use_color),
            "Suggestion" => paint("SUGGEST", "2", use_color),
            other => other.to_string(),
        };
        output.push_str(&format!("  [{severity}] {}\n", finding.summary));
        output.push_str(&format!(
            "    {} {}\n",
            paint("at:", "2", use_color),
            paint(&finding.location, "2", use_color)
        ));
        output.push_str(&format!(
            "    {} {}\n",
            paint("→", "2", use_color),
            paint(&finding.recommendation, "2", use_color)
        ));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gating_matrix_matches_each_dimension_contract() {
        // oracle 3.0.0 p03：Coverage 需四個 artifact 中任兩個；Consistency 需 design，
        // 或 proposal 加 tasks；Ambiguity 需 specs；Gaps 需任一個。
        let gates = |proposal, specs, design, tasks| {
            let g = ArtifactPresence {
                proposal,
                specs,
                design,
                tasks,
            }
            .gates();
            [g.coverage, g.consistency, g.ambiguity, g.gaps]
        };
        assert_eq!(gates(false, false, false, false), [false; 4]);
        assert_eq!(
            gates(true, false, false, false),
            [false, false, false, true]
        );
        assert_eq!(gates(false, true, false, false), [false, false, true, true]);
        assert_eq!(gates(false, false, true, false), [false, true, false, true]);
        assert_eq!(
            gates(false, false, false, true),
            [false, false, false, true]
        );
        assert_eq!(gates(true, false, false, true), [true, true, false, true]);
        assert_eq!(gates(false, true, false, true), [true, false, true, true]);
        assert_eq!(gates(true, true, false, false), [true, false, true, true]);
        assert_eq!(gates(false, false, true, true), [true, true, false, true]);
        assert_eq!(gates(true, false, true, false), [true, true, false, true]);
        assert_eq!(gates(false, true, true, false), [true, true, true, true]);
    }

    #[test]
    fn colored_human_output_uses_the_oracle_sgr_codes() {
        // 期望值取自 oracle 3.0.0 在 PTY 上的輸出（golden 的 human-output 情境）。
        let report = AnalyzeReport {
            change_id: "c".to_string(),
            dimensions: vec![
                dimension_report(DIMENSION_COVERAGE, true, 1),
                dimension_report(DIMENSION_LOCALIZATION, false, 0),
            ],
            findings: number(
                "COV",
                DIMENSION_COVERAGE,
                vec![FindingText {
                    severity: "Critical",
                    location: "proposal.md → Capabilities".to_string(),
                    summary: "S".to_string(),
                    recommendation: "R".to_string(),
                    key: "covMissingSpec",
                    summary_params: params(&[]),
                    recommendation_params: params(&[]),
                }],
            ),
            artifacts_analyzed: vec!["proposal".to_string()],
            artifacts_missing: vec!["tasks".to_string()],
        };
        assert_eq!(
            format_human(&report, true),
            concat!(
                "\x1b[1mChange\x1b[0m: c\n\n",
                "  \x1b[33m●\x1b[0m \x1b[1mCoverage      \x1b[0m \x1b[2m1 issue(s) found\x1b[0m (1 findings)\n",
                "  \x1b[32m✓\x1b[0m \x1b[1mLocalization  \x1b[0m \x1b[2mSkipped (insufficient artifacts)\x1b[0m (0 findings)\n",
                "\n",
                "  \x1b[2mAnalyzed:\x1b[0m proposal\n",
                "  \x1b[33mMissing:\x1b[0m tasks\n",
                "\n",
                "  \x1b[1mFindings\x1b[0m (1):\n\n",
                "  [\x1b[1;31mCRITICAL\x1b[0m] S\n",
                "    \x1b[2mat:\x1b[0m \x1b[2mproposal.md → Capabilities\x1b[0m\n",
                "    \x1b[2m→\x1b[0m \x1b[2mR\x1b[0m\n",
            )
        );
    }
}
