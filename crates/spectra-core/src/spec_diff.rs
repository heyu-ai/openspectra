//! Requirement-level change diffs for review.

use anyhow::{Context, Result};
use serde::Serialize;

use crate::config::Config;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffEntry {
    pub capability: String,
    pub operation: String,
    pub name: String,
    pub diff: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

pub fn change_diff(cfg: &Config, change: &str) -> Result<Vec<DiffEntry>> {
    let change = crate::change::load(cfg, change)?;
    let mut entries = Vec::new();
    for (capability, content) in crate::fsutil::collect_delta_specs(&change.dir.join("specs"))? {
        let delta = crate::markdown::parse_delta(&content)
            .with_context(|| format!("parsing specs/{capability}/spec.md"))?;
        let main_path = cfg.specs_dir().join(&capability).join("spec.md");
        let main = match std::fs::read_to_string(&main_path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", main_path.display()));
            }
        };
        let current = crate::markdown::parse_main_requirements(&main);
        for requirement in delta.added {
            entries.push(DiffEntry {
                capability: capability.clone(),
                operation: "ADDED".to_string(),
                name: requirement.name,
                diff: requirement
                    .raw
                    .lines()
                    .map(|line| format!("+{line}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                warning: None,
            });
        }
        for requirement in delta.modified {
            let base_name = original_requirement_name(&requirement.name, &delta.renamed);
            let base = current
                .iter()
                .find(|candidate| crate::markdown::normalize_name(&candidate.name) == base_name);
            entries.push(DiffEntry {
                capability: capability.clone(),
                operation: "MODIFIED".to_string(),
                name: requirement.name,
                diff: line_diff(
                    base.map(|item| item.raw.as_str()).unwrap_or(""),
                    &requirement.raw,
                ),
                warning: base
                    .is_none()
                    .then(|| format!("Requirement '{base_name}' was not found in the main spec")),
            });
        }
        for name in delta.removed {
            entries.push(DiffEntry {
                capability: capability.clone(),
                operation: "REMOVED".to_string(),
                name: name.clone(),
                diff: current
                    .iter()
                    .find(|item| {
                        crate::markdown::normalize_name(&item.name)
                            == crate::markdown::normalize_name(&name)
                    })
                    .map(|item| {
                        item.raw
                            .lines()
                            .map(|line| format!("-{line}"))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default(),
                warning: None,
            });
        }
        for rename in delta.renamed {
            entries.push(DiffEntry {
                capability: capability.clone(),
                operation: "RENAMED".to_string(),
                name: rename.to.clone(),
                diff: format!(
                    "-### Requirement: {}\n+### Requirement: {}",
                    rename.from, rename.to
                ),
                warning: None,
            });
        }
    }
    Ok(entries)
}

fn original_requirement_name(target: &str, renames: &[crate::markdown::Rename]) -> String {
    let mut current = crate::markdown::normalize_name(target);
    let mut visited = std::collections::HashSet::new();
    while visited.insert(current.clone()) {
        let Some(rename) = renames
            .iter()
            .find(|rename| crate::markdown::normalize_name(&rename.to) == current)
        else {
            break;
        };
        current = crate::markdown::normalize_name(&rename.from);
    }
    current
}

fn line_diff(before: &str, after: &str) -> String {
    let before: Vec<_> = before.lines().collect();
    let after: Vec<_> = after.lines().collect();
    let mut lengths = vec![vec![0usize; after.len() + 1]; before.len() + 1];
    for left in (0..before.len()).rev() {
        for right in (0..after.len()).rev() {
            lengths[left][right] = if before[left] == after[right] {
                lengths[left + 1][right + 1] + 1
            } else {
                lengths[left + 1][right].max(lengths[left][right + 1])
            };
        }
    }
    let mut output = Vec::new();
    let (mut left, mut right) = (0, 0);
    while left < before.len() || right < after.len() {
        if left < before.len() && right < after.len() && before[left] == after[right] {
            output.push(format!(" {}", before[left]));
            left += 1;
            right += 1;
        } else if right < after.len()
            && (left == before.len() || lengths[left][right + 1] >= lengths[left + 1][right])
        {
            output.push(format!("+{}", after[right]));
            right += 1;
        } else {
            output.push(format!("-{}", before[left]));
            left += 1;
        }
    }
    output.join("\n")
}

#[cfg(test)]
mod tests {
    use super::line_diff;

    #[test]
    fn line_diff_preserves_context_and_marks_changes() {
        assert_eq!(line_diff("a\nb\n", "a\nc\n"), " a\n+c\n-b");
    }
}
