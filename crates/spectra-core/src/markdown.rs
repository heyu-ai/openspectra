//! Shared OpenSpec Markdown structure parsing.

use std::borrow::Cow;

pub(crate) fn normalize_markdown(content: &str) -> Cow<'_, str> {
    let without_bom = content.strip_prefix('\u{feff}').unwrap_or(content);
    if !without_bom.contains('\r') {
        return if without_bom.len() == content.len() {
            Cow::Borrowed(content)
        } else {
            Cow::Borrowed(without_bom)
        };
    }

    let mut normalized = String::with_capacity(without_bom.len());
    let mut chars = without_bom.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(ch);
        }
    }
    Cow::Owned(normalized)
}

#[derive(Clone, Copy)]
struct Fence {
    marker: u8,
    len: usize,
}

fn fence_content(line: &str) -> Option<&str> {
    let indent = line
        .as_bytes()
        .iter()
        .take_while(|byte| **byte == b' ')
        .count();
    (indent <= 3).then(|| &line[indent..])
}

fn fence_marker(line: &str) -> Option<Fence> {
    let bytes = fence_content(line)?.as_bytes();
    let marker = *bytes.first()?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let len = bytes.iter().take_while(|byte| **byte == marker).count();
    (len >= 3).then_some(Fence { marker, len })
}

fn closes_fence(line: &str, active: Fence) -> bool {
    let content = match fence_content(line) {
        Some(content) => content,
        None => return false,
    };
    let bytes = content.as_bytes();
    let len = bytes
        .iter()
        .take_while(|byte| **byte == active.marker)
        .count();
    len >= active.len && content[len..].trim().is_empty()
}

pub(crate) fn fenced_line_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut active = None;
    for (index, line) in lines.iter().enumerate() {
        if let Some(fence) = active {
            mask[index] = true;
            if closes_fence(line, fence) {
                active = None;
            }
        } else if let Some(fence) = fence_marker(line) {
            mask[index] = true;
            active = Some(fence);
        }
    }
    mask
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Requirement {
    pub name: String,
    pub raw: String,
    pub text: String,
    pub scenarios: Vec<String>,
    pub start: usize,
    pub end: usize,
    pub header_end: usize,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rename {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DeltaDocument {
    pub purpose: Option<String>,
    pub added: Vec<Requirement>,
    pub modified: Vec<Requirement>,
    pub removed: Vec<String>,
    pub renamed: Vec<Rename>,
    pub added_present: bool,
    pub modified_present: bool,
    pub removed_present: bool,
    pub renamed_present: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeltaSection {
    Purpose,
    Added,
    Modified,
    Removed,
    Renamed,
    Other,
}

struct SectionRange {
    kind: DeltaSection,
    start: usize,
    end: usize,
}

fn heading_text(line: &str, level: usize) -> Option<&str> {
    let trimmed = line.trim_start();
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let hashes = trimmed
        .as_bytes()
        .iter()
        .take_while(|byte| **byte == b'#')
        .count();
    if hashes != level || !trimmed[hashes..].starts_with([' ', '\t']) {
        return None;
    }
    Some(trimmed[hashes..].trim().trim_end_matches('#').trim_end())
}

fn requirement_name(line: &str) -> Option<String> {
    let text = heading_text(line, 3)?;
    let (prefix, name) = text.split_once(':')?;
    prefix
        .eq_ignore_ascii_case("Requirement")
        .then(|| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn normalized_scenario_name(text: &str) -> String {
    text.strip_prefix("Scenario:")
        .or_else(|| text.strip_prefix("scenario:"))
        .unwrap_or(text)
        .trim()
        .to_string()
}

fn requirement_blocks(lines: &[&str], mask: &[bool], start: usize, end: usize) -> Vec<Requirement> {
    let mut headers = Vec::new();
    for index in start..end {
        if !mask[index] && heading_text(lines[index], 3).is_some() {
            headers.push(index);
        }
    }

    let mut offsets = Vec::with_capacity(lines.len() + 1);
    let mut offset = 0;
    for line in lines {
        offsets.push(offset);
        offset += line.len() + 1;
    }
    offsets.push(offset.saturating_sub(1));

    let mut requirements = Vec::new();
    for (position, header) in headers.iter().enumerate() {
        let Some(name) = requirement_name(lines[*header]) else {
            continue;
        };
        let block_end = headers.get(position + 1).copied().unwrap_or(end);
        let mut text_lines = Vec::new();
        let mut metadata_lines = Vec::new();
        let mut scenarios = Vec::new();
        for index in (*header + 1)..block_end {
            if mask[index] {
                continue;
            }
            if let Some(scenario) = heading_text(lines[index], 4) {
                scenarios.push(normalized_scenario_name(scenario));
                continue;
            }
            if heading_text(lines[index], 1).is_some()
                || heading_text(lines[index], 2).is_some()
                || heading_text(lines[index], 3).is_some()
            {
                break;
            }
            let trimmed = lines[index].trim();
            if scenarios.is_empty() && !trimmed.is_empty() {
                let is_metadata = trimmed
                    .strip_prefix("**")
                    .and_then(|line| line.split_once("**:"))
                    .is_some_and(|(key, _)| !key.trim().is_empty());
                if is_metadata {
                    metadata_lines.push(trimmed);
                } else {
                    text_lines.push(trimmed);
                }
            }
        }
        requirements.push(Requirement {
            name,
            raw: lines[*header..block_end].join("\n").trim_end().to_string(),
            text: if text_lines.is_empty() {
                metadata_lines.join("\n")
            } else {
                text_lines.join("\n")
            },
            scenarios,
            line: *header + 1,
            start: offsets[*header],
            end: offsets[block_end],
            header_end: offsets[*header] + lines[*header].len(),
        });
    }
    requirements
}

fn rename_pairs(
    lines: &[&str],
    mask: &[bool],
    start: usize,
    end: usize,
) -> anyhow::Result<Vec<Rename>> {
    let mut pending = None;
    let mut renamed = Vec::new();
    for index in start..end {
        if mask[index] {
            continue;
        }
        let line = lines[index].trim_start();
        let Some(item) = line
            .strip_prefix('-')
            .or_else(|| line.strip_prefix('*'))
            .map(str::trim_start)
        else {
            continue;
        };
        if let Some(value) = item.strip_prefix("FROM:") {
            if pending.is_some() {
                anyhow::bail!("RENAMED Requirements contains FROM without following TO");
            }
            pending = Some(parse_rename_header(value, "FROM")?);
        } else if let Some(value) = item.strip_prefix("TO:") {
            let from = pending.take().ok_or_else(|| {
                anyhow::anyhow!("RENAMED Requirements contains TO without preceding FROM")
            })?;
            renamed.push(Rename {
                from,
                to: parse_rename_header(value, "TO")?,
            });
        }
    }
    if pending.is_some() {
        anyhow::bail!("RENAMED Requirements contains FROM without following TO");
    }
    Ok(renamed)
}

fn parse_rename_header(value: &str, field: &str) -> anyhow::Result<String> {
    let start = value
        .find('`')
        .ok_or_else(|| anyhow::anyhow!("{field} is missing a backticked requirement header"))?;
    let rest = &value[start + 1..];
    let end = rest
        .find('`')
        .ok_or_else(|| anyhow::anyhow!("{field} is missing a closing backtick"))?;
    requirement_name(&rest[..end])
        .ok_or_else(|| anyhow::anyhow!("{field} must be a `### Requirement: <name>` header"))
}

pub(crate) fn normalize_name(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn duplicate_name<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    names
        .into_iter()
        .map(normalize_name)
        .find(|name| !seen.insert(name.clone()))
}

fn validate_delta_conflicts(delta: &DeltaDocument) -> anyhow::Result<()> {
    for (kind, names) in [
        (
            "ADDED",
            delta
                .added
                .iter()
                .map(|item| item.name.as_str())
                .collect::<Vec<_>>(),
        ),
        (
            "MODIFIED",
            delta
                .modified
                .iter()
                .map(|item| item.name.as_str())
                .collect(),
        ),
        (
            "REMOVED",
            delta.removed.iter().map(String::as_str).collect(),
        ),
    ] {
        if let Some(name) = duplicate_name(names) {
            if kind == "ADDED" {
                anyhow::bail!("delta ADDs requirement '{name}' more than once");
            }
            anyhow::bail!("Duplicate requirement in {kind}: \"{name}\"");
        }
    }

    if let Some(name) = duplicate_name(delta.renamed.iter().map(|rename| rename.from.as_str())) {
        anyhow::bail!("Duplicate requirement in RENAMED FROM: \"{name}\"");
    }
    if let Some(name) = duplicate_name(delta.renamed.iter().map(|rename| rename.to.as_str())) {
        anyhow::bail!("Duplicate requirement in RENAMED TO: \"{name}\"");
    }

    let added: std::collections::HashSet<_> = delta
        .added
        .iter()
        .map(|item| normalize_name(&item.name))
        .collect();
    let modified: std::collections::HashSet<_> = delta
        .modified
        .iter()
        .map(|item| normalize_name(&item.name))
        .collect();
    let removed: std::collections::HashSet<_> = delta
        .removed
        .iter()
        .map(|name| normalize_name(name))
        .collect();
    let renamed_from: std::collections::HashSet<_> = delta
        .renamed
        .iter()
        .map(|rename| normalize_name(&rename.from))
        .collect();
    let renamed_to: std::collections::HashSet<_> = delta
        .renamed
        .iter()
        .map(|rename| normalize_name(&rename.to))
        .collect();

    for name in &modified {
        if removed.contains(name) {
            anyhow::bail!("Requirement present in both MODIFIED and REMOVED: \"{name}\"");
        }
        if added.contains(name) {
            anyhow::bail!("Requirement present in both MODIFIED and ADDED: \"{name}\"");
        }
    }
    for name in &renamed_from {
        if removed.contains(name) {
            anyhow::bail!("Requirement present in both RENAMED FROM and REMOVED: \"{name}\"");
        }
        if modified.contains(name) {
            anyhow::bail!("Requirement present in both RENAMED FROM and MODIFIED: \"{name}\"");
        }
    }
    for name in &renamed_to {
        if added.contains(name) {
            anyhow::bail!("Requirement present in both RENAMED TO and ADDED: \"{name}\"");
        }
    }
    for name in &added {
        if removed.contains(name) {
            anyhow::bail!("Requirement present in both ADDED and REMOVED: \"{name}\"");
        }
    }
    Ok(())
}

pub(crate) fn parse_delta(content: &str) -> anyhow::Result<DeltaDocument> {
    let normalized = normalize_markdown(content);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mask = fenced_line_mask(&lines);
    let mut headers = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if mask[index] {
            continue;
        }
        let Some(text) = heading_text(line, 2) else {
            continue;
        };
        let kind = if text.eq_ignore_ascii_case("Purpose") {
            DeltaSection::Purpose
        } else if text.eq_ignore_ascii_case("ADDED Requirements") {
            DeltaSection::Added
        } else if text.eq_ignore_ascii_case("MODIFIED Requirements") {
            DeltaSection::Modified
        } else if text.eq_ignore_ascii_case("REMOVED Requirements") {
            DeltaSection::Removed
        } else if text.eq_ignore_ascii_case("RENAMED Requirements") {
            DeltaSection::Renamed
        } else {
            DeltaSection::Other
        };
        headers.push((index, kind));
    }

    let sections: Vec<_> = headers
        .iter()
        .enumerate()
        .map(|(position, (header, kind))| SectionRange {
            kind: *kind,
            start: header + 1,
            end: headers.get(position + 1).map_or(lines.len(), |next| next.0),
        })
        .collect();

    for (kind, label) in [
        (DeltaSection::Purpose, "Purpose"),
        (DeltaSection::Added, "ADDED Requirements"),
        (DeltaSection::Modified, "MODIFIED Requirements"),
        (DeltaSection::Removed, "REMOVED Requirements"),
        (DeltaSection::Renamed, "RENAMED Requirements"),
    ] {
        if sections
            .iter()
            .filter(|section| section.kind == kind)
            .count()
            > 1
        {
            anyhow::bail!("delta has more than one `## {label}` section");
        }
    }

    let mut parsed = DeltaDocument::default();
    for section in sections {
        match section.kind {
            DeltaSection::Purpose => {
                let purpose = lines[section.start..section.end]
                    .join("\n")
                    .trim()
                    .to_string();
                if purpose.is_empty() {
                    anyhow::bail!("delta `## Purpose` section contains no content");
                }
                parsed.purpose = Some(purpose);
            }
            DeltaSection::Added => {
                parsed.added_present = true;
                parsed.added = requirement_blocks(&lines, &mask, section.start, section.end);
                if parsed.added.is_empty() {
                    anyhow::bail!(
                        "delta `## ADDED Requirements` section contains no recognizable entries"
                    );
                }
            }
            DeltaSection::Modified => {
                parsed.modified_present = true;
                parsed.modified = requirement_blocks(&lines, &mask, section.start, section.end);
                if parsed.modified.is_empty() {
                    anyhow::bail!(
                        "delta `## MODIFIED Requirements` section contains no recognizable entries"
                    );
                }
            }
            DeltaSection::Removed => {
                parsed.removed_present = true;
                parsed.removed = requirement_blocks(&lines, &mask, section.start, section.end)
                    .into_iter()
                    .map(|requirement| requirement.name)
                    .collect();
                if parsed.removed.is_empty() {
                    anyhow::bail!(
                        "delta `## REMOVED Requirements` section contains no recognizable entries"
                    );
                }
            }
            DeltaSection::Renamed => {
                parsed.renamed_present = true;
                parsed.renamed = rename_pairs(&lines, &mask, section.start, section.end)?;
                if parsed.renamed.is_empty() {
                    anyhow::bail!(
                        "delta `## RENAMED Requirements` section contains no recognizable entries"
                    );
                }
            }
            DeltaSection::Other => {}
        }
    }
    validate_delta_conflicts(&parsed)?;
    Ok(parsed)
}

pub(crate) fn parse_main_requirements(content: &str) -> Vec<Requirement> {
    let normalized = normalize_markdown(content);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mask = fenced_line_mask(&lines);
    let Some(header) = lines.iter().enumerate().find_map(|(index, line)| {
        (!mask[index]
            && heading_text(line, 2).is_some_and(|text| text.eq_ignore_ascii_case("Requirements")))
        .then_some(index)
    }) else {
        return Vec::new();
    };
    let end = ((header + 1)..lines.len())
        .find(|index| !mask[*index] && heading_text(lines[*index], 2).is_some())
        .unwrap_or(lines.len());
    requirement_blocks(&lines, &mask, header + 1, end)
}

pub(crate) fn parse_main_purpose(content: &str) -> Option<String> {
    let normalized = normalize_markdown(content);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mask = fenced_line_mask(&lines);
    let header = lines.iter().enumerate().find_map(|(index, line)| {
        (!mask[index]
            && heading_text(line, 2).is_some_and(|text| text.eq_ignore_ascii_case("Purpose")))
        .then_some(index)
    })?;
    let end = ((header + 1)..lines.len())
        .find(|index| !mask[*index] && heading_text(lines[*index], 2).is_some())
        .unwrap_or(lines.len());
    let purpose = lines[header + 1..end].join("\n").trim().to_string();
    (!purpose.is_empty()).then_some(purpose)
}

fn main_section_end(content: &str, section_name: &str) -> Option<usize> {
    let normalized = normalize_markdown(content);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mask = fenced_line_mask(&lines);
    let header = lines.iter().enumerate().find_map(|(index, line)| {
        (!mask[index]
            && heading_text(line, 2).is_some_and(|text| text.eq_ignore_ascii_case(section_name)))
        .then_some(index)
    })?;
    let end = ((header + 1)..lines.len())
        .find(|index| !mask[*index] && heading_text(lines[*index], 2).is_some())
        .unwrap_or(lines.len());
    Some(
        lines
            .iter()
            .take(end)
            .map(|line| line.len() + 1)
            .sum::<usize>()
            .min(normalized.len()),
    )
}

pub(crate) fn main_requirements_insertion_point(content: &str) -> Option<usize> {
    main_section_end(content, "Requirements")
}

pub(crate) fn main_purpose_insertion_point(content: &str) -> Option<usize> {
    main_section_end(content, "Purpose")
}

pub(crate) fn is_placeholder_purpose(purpose: &str) -> bool {
    let trimmed = purpose.trim();
    let first = trimmed.split_whitespace().next().unwrap_or_default();
    matches!(first.trim_end_matches([':', '-']), "TBD" | "TODO")
        || (trimmed.contains("TBD - created by archiving change")
            && trimmed.contains("Update Purpose after archive."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_utf8_bom_and_all_supported_line_endings() {
        assert_eq!(
            normalize_markdown("\u{feff}## ADDED Requirements\r\nA\rB\n"),
            "## ADDED Requirements\nA\nB\n"
        );
    }

    #[test]
    fn masks_fences_by_marker_and_opening_length() {
        let lines = [
            "````markdown",
            "### Requirement: quoted",
            "```",
            "still quoted",
            "````",
            "real",
            "~~~text",
            "```",
            "~~~",
        ];
        assert_eq!(
            fenced_line_mask(&lines),
            vec![true, true, true, true, true, false, true, true, true]
        );
    }

    #[test]
    fn parses_purpose_and_every_delta_operation() {
        let delta = parse_delta(
            "## Purpose\n\nPortable exports.\n\n\
             ## ADDED Requirements\n\n\
             ### Requirement: Added\nA SHALL.\n#### Scenario: A\n\n\
             ## MODIFIED Requirements\n\n\
             ### Requirement: Modified\nM MUST.\n#### Edge case\n\n\
             ## REMOVED Requirements\n\n\
             ### Requirement: Removed\n\n\
             ## RENAMED Requirements\n\
             - FROM: `### Requirement: Old`\n\
             - TO: `### Requirement: New`\n",
        )
        .unwrap();

        assert_eq!(delta.purpose.as_deref(), Some("Portable exports."));
        assert_eq!(delta.added[0].name, "Added");
        assert_eq!(delta.modified[0].scenarios, vec!["Edge case"]);
        assert_eq!(delta.removed, vec!["Removed"]);
        assert_eq!(delta.renamed[0].from, "Old");
        assert_eq!(delta.renamed[0].to, "New");
    }

    #[test]
    fn rejects_duplicate_and_cross_section_requirement_conflicts() {
        let duplicate = "## ADDED Requirements\n\
            ### Requirement: Same\nA SHALL.\n#### Scenario: A\n\
            ### Requirement: Same\nB SHALL.\n#### Scenario: B\n";
        assert!(parse_delta(duplicate)
            .unwrap_err()
            .to_string()
            .contains("delta ADDs requirement 'Same' more than once"));

        let cross_section = "## MODIFIED Requirements\n\
            ### Requirement: Same\nA SHALL.\n#### Scenario: A\n\
            ## REMOVED Requirements\n\
            ### Requirement: Same\n";
        assert!(parse_delta(cross_section)
            .unwrap_err()
            .to_string()
            .contains("both MODIFIED and REMOVED"));
    }

    #[test]
    fn fences_require_at_most_three_leading_spaces_to_open_or_close() {
        let lines = [
            "   ```markdown",
            "inside",
            "    `````",
            "still inside",
            "   ```",
            "outside",
            "    ```",
            "not fenced",
        ];
        assert_eq!(
            fenced_line_mask(&lines),
            vec![true, true, true, true, true, false, false, false]
        );
    }

    #[test]
    fn an_over_indented_closer_does_not_expose_destructive_delta_operations() {
        let delta = parse_delta(concat!(
            "```markdown\n",
            "    `````\n",
            "## REMOVED Requirements\n",
            "### Requirement: Keep\n",
            "```\n",
            "## ADDED Requirements\n",
            "### Requirement: Add\n",
            "The system SHALL add safely.\n",
            "#### Scenario: Safe addition\n",
        ))
        .unwrap();

        assert!(!delta.removed_present);
        assert!(delta.removed.is_empty());
        assert_eq!(delta.added[0].name, "Add");
    }

    #[test]
    fn metadata_lines_supply_text_only_when_ordinary_prose_is_absent() {
        let delta = parse_delta(
            "## ADDED Requirements\n\
             ### Requirement: Metadata only\n\
             **Priority**: High\n\
             **Contract**: The system SHALL retain metadata.\n\
             #### Scenario: Metadata survives\n\
             ### Requirement: Ordinary prose\n\
             **Priority**: Low\n\
             The system MUST prefer this prose.\n\
             #### Scenario: Prose wins\n",
        )
        .unwrap();

        assert_eq!(
            delta.added[0].text,
            "**Priority**: High\n**Contract**: The system SHALL retain metadata."
        );
        assert_eq!(delta.added[1].text, "The system MUST prefer this prose.");
    }

    #[test]
    fn rejects_every_present_but_empty_recognized_delta_section() {
        for (source, section) in [
            ("## Purpose\n\n", "Purpose"),
            (
                "## ADDED Requirements\n\nprose without a requirement\n",
                "ADDED Requirements",
            ),
            (
                "## MODIFIED Requirements\n\nprose without a requirement\n",
                "MODIFIED Requirements",
            ),
            (
                "## REMOVED Requirements\n\nprose without a requirement\n",
                "REMOVED Requirements",
            ),
            (
                "## RENAMED Requirements\n\nprose without a rename pair\n",
                "RENAMED Requirements",
            ),
        ] {
            let error = parse_delta(source).unwrap_err().to_string();
            assert!(
                error.contains(&format!("`## {section}` section contains no")),
                "empty {section} section should be rejected, got: {error}"
            );
        }
    }

    #[test]
    fn rejects_duplicate_normalized_rename_sources_and_targets() {
        for (source, operation, name) in [
            (
                "## RENAMED Requirements\n\
                 - FROM: `### Requirement: Old   Name`\n\
                 - TO: `### Requirement: First`\n\
                 - FROM: `### Requirement: Old Name`\n\
                 - TO: `### Requirement: Second`\n",
                "FROM",
                "Old Name",
            ),
            (
                "## RENAMED Requirements\n\
                 - FROM: `### Requirement: First`\n\
                 - TO: `### Requirement: New   Name`\n\
                 - FROM: `### Requirement: Second`\n\
                 - TO: `### Requirement: New Name`\n",
                "TO",
                "New Name",
            ),
        ] {
            let error = parse_delta(source).unwrap_err().to_string();
            assert!(
                error.contains(operation) && error.contains(name),
                "duplicate RENAMED {operation} should name '{name}', got: {error}"
            );
        }
    }

    #[test]
    fn rejects_normalized_rename_conflicts_with_other_operations() {
        for (source, rename_side, operation, name) in [
            (
                "## RENAMED Requirements\n\
                 - FROM: `### Requirement: Old   Name`\n\
                 - TO: `### Requirement: New`\n\
                 ## REMOVED Requirements\n\
                 ### Requirement: Old Name\n",
                "FROM",
                "REMOVED",
                "Old Name",
            ),
            (
                "## RENAMED Requirements\n\
                 - FROM: `### Requirement: Old Name`\n\
                 - TO: `### Requirement: New`\n\
                 ## MODIFIED Requirements\n\
                 ### Requirement: Old   Name\n",
                "FROM",
                "MODIFIED",
                "Old Name",
            ),
            (
                "## RENAMED Requirements\n\
                 - FROM: `### Requirement: Old`\n\
                 - TO: `### Requirement: New   Name`\n\
                 ## ADDED Requirements\n\
                 ### Requirement: New Name\n",
                "TO",
                "ADDED",
                "New Name",
            ),
        ] {
            let error = parse_delta(source).unwrap_err().to_string();
            assert!(
                error.contains(rename_side) && error.contains(operation) && error.contains(name),
                "RENAMED {rename_side}/{operation} conflict should name '{name}', got: {error}"
            );
        }
    }
}
