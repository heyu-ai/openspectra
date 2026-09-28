//! Coverage dimension（oracle `analyzer::coverage::analyze_coverage`）。
//! 輸出依種類分組：先全部 `covMissingSpec`（proposal 順序），再全部
//! `covMissingTask`（spec 檔、requirement 順序），最後全部 `covDeltaValidation`。

use std::collections::HashSet;

use super::extract::{self, DeltaSection};
use super::{params, Artifacts, FindingText};

pub(super) fn findings(artifacts: &Artifacts) -> Vec<FindingText> {
    let mut findings = Vec::new();

    if let Some(proposal) = artifacts.proposal {
        // 缺漏判準是「不在 spec 目錄名稱集合裡」的精確字串比對：`nested/cap` 與
        // 大小寫不同的 `x`／`X` 都算缺漏（oracle 3.0.0 p02a）。
        let spec_dirs: HashSet<&str> = artifacts
            .spec_files
            .iter()
            .map(|file| file.capability.as_str())
            .collect();
        for capability in extract::proposal_capabilities(proposal) {
            if spec_dirs.contains(capability.as_str()) {
                continue;
            }
            findings.push(FindingText {
                severity: "Critical",
                location: "proposal.md → Capabilities".to_string(),
                summary: format!("Capability `{capability}` has no corresponding spec file"),
                recommendation: format!("Create specs/{capability}/spec.md with requirements"),
                key: "covMissingSpec",
                summary_params: params(&[("cap", &capability)]),
                recommendation_params: params(&[("cap", &capability)]),
            });
        }
    }

    if let Some(tasks) = artifacts.tasks {
        // 只比對 task 行（不含 heading、續行與一般 bullet）；REMOVED 的 requirement
        // 不檢查，RENAMED 檢查 FROM 的舊名稱（oracle 的行為，照搬）。
        let task_lines = extract::task_lines(tasks);
        for file in artifacts.spec_files {
            for requirement in file.requirements() {
                if requirement.section == Some(DeltaSection::Removed) {
                    continue;
                }
                let needle = requirement.name.to_lowercase();
                if task_lines.iter().any(|line| line.contains(&needle)) {
                    continue;
                }
                let name = requirement.name;
                findings.push(FindingText {
                    severity: "Warning",
                    location: file.relative_path.clone(),
                    summary: format!("Requirement '{name}' has no matching task"),
                    recommendation: format!("Add a task in tasks.md that references '{name}'"),
                    key: "covMissingTask",
                    summary_params: params(&[("req", &name)]),
                    recommendation_params: params(&[("req", &name)]),
                });
            }
        }
    }

    for file in artifacts.spec_files {
        for error in file.delta_sections().validation_errors() {
            findings.push(FindingText {
                severity: "Critical",
                location: file.relative_path.clone(),
                summary: format!("Delta spec validation error: {error}"),
                recommendation: "Fix the delta spec structure".to_string(),
                key: "covDeltaValidation",
                summary_params: params(&[("error", &error)]),
                recommendation_params: params(&[]),
            });
        }
    }
    findings
}
