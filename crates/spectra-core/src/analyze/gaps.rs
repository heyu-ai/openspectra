//! Gaps dimension：`gapNoProposal`，接著逐檔 `gapNewCapabilityNoPurpose`，再逐檔
//! `gapNoMainSpec`／`gapModifiedNotFound`（三趟輸出，oracle 3.0.0 p22／p25b）。

use std::collections::HashSet;

use anyhow::{Context, Result};

use super::extract;
use super::{params, Artifacts, FindingText};
use crate::config::Config;

pub(super) fn findings(cfg: &Config, artifacts: &Artifacts) -> Result<Vec<FindingText>> {
    let mut findings = Vec::new();
    if !artifacts.spec_files.is_empty() && artifacts.proposal.is_none() {
        findings.push(FindingText {
            severity: "Critical",
            location: "change directory".to_string(),
            summary: "Specs exist but no proposal.md found".to_string(),
            recommendation: "Create proposal.md describing the change purpose".to_string(),
            key: "gapNoProposal",
            summary_params: params(&[]),
            recommendation_params: params(&[]),
        });
    }

    let sections: Vec<_> = artifacts
        .spec_files
        .iter()
        .map(|file| file.delta_sections())
        .collect();
    let main_spec = |capability: &str| cfg.specs_dir().join(capability).join("spec.md");

    // 新 capability（主 spec 不存在）有 ADDED／MODIFIED／REMOVED requirement 卻沒有
    // 非空的 `## Purpose`：archive 會以 placeholder Purpose 建立主 spec。
    for (file, delta) in artifacts.spec_files.iter().zip(&sections) {
        if main_spec(&file.capability).is_file()
            || !delta.has_operation_requirements()
            || delta.has_nonempty_purpose()
        {
            continue;
        }
        let capability = &file.capability;
        findings.push(FindingText {
            severity: "Warning",
            location: file.relative_path.clone(),
            summary: format!(
                "New capability '{capability}' delta has no ## Purpose section; the spec will be created with a placeholder Purpose"
            ),
            recommendation: format!(
                "Add a ## Purpose section (1-3 sentences) at the top of specs/{capability}/spec.md"
            ),
            key: "gapNewCapabilityNoPurpose",
            summary_params: params(&[("spec", capability)]),
            recommendation_params: params(&[("spec", capability)]),
        });
    }

    for (file, delta) in artifacts.spec_files.iter().zip(&sections) {
        let modified = delta.modified();
        if modified.is_empty() {
            continue;
        }
        let capability = &file.capability;
        let main_spec_path = main_spec(capability);
        if !main_spec_path.is_file() {
            findings.push(FindingText {
                severity: "Warning",
                location: file.relative_path.clone(),
                summary: format!(
                    "MODIFIED requirements reference capability '{capability}' but no main spec found"
                ),
                recommendation: format!("Check if openspec/specs/{capability}/spec.md exists"),
                key: "gapNoMainSpec",
                summary_params: params(&[("spec", capability)]),
                recommendation_params: params(&[("spec", capability)]),
            });
            continue;
        }
        let main_content = std::fs::read_to_string(&main_spec_path)
            .with_context(|| format!("reading {}", main_spec_path.display()))?;
        let main_requirements: HashSet<&str> = main_content
            .lines()
            .filter_map(extract::requirement_name)
            .collect();
        for requirement in modified {
            if main_requirements.contains(requirement.as_str()) {
                continue;
            }
            findings.push(FindingText {
                severity: "Warning",
                location: file.relative_path.clone(),
                summary: format!("MODIFIED requirement '{requirement}' not found in main spec"),
                recommendation: format!(
                    "Verify requirement '{requirement}' exists in openspec/specs/{capability}/spec.md"
                ),
                key: "gapModifiedNotFound",
                summary_params: params(&[("name", requirement)]),
                recommendation_params: params(&[("name", requirement), ("spec", capability)]),
            });
        }
    }
    Ok(findings)
}
