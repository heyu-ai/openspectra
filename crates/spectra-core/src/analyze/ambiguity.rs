//! Ambiguity dimension：每個 spec 檔依序輸出全部 `ambNoScenario`、全部
//! `ambAbstractScenario`、全部 `ambWeakLanguage`（oracle 3.0.0 p20）。

use super::extract::{self, DeltaSection, SpecFile};
use super::{params, FindingText};

/// 弱語氣字詞與優先序：每行只回報第一個命中的字（不分大小寫的純子字串，
/// `mayhem` 也算 `may`），回報清單上的標準寫法。
const WEAK_WORDS: [(&str, &str); 9] = [
    ("should", "should"),
    ("may", "may"),
    ("might", "might"),
    ("consider", "consider"),
    ("possibly", "possibly"),
    ("tbd", "TBD"),
    ("todo", "TODO"),
    ("???", "???"),
    ("tktk", "TKTK"),
];

/// 3.0.0 起 `trim_start` 後以 `#` 開頭的行（heading）不檢查；URL、inline code 與
/// fence 內的行照樣檢查（oracle 的行為，照搬）。
fn weak_language_pattern(line: &str) -> Option<&'static str> {
    if line.trim_start().starts_with('#') {
        return None;
    }
    let lowercase = line.to_lowercase();
    WEAK_WORDS
        .into_iter()
        .find_map(|(needle, canonical)| lowercase.contains(needle).then_some(canonical))
}

pub(super) fn findings(spec_files: &[SpecFile]) -> Vec<FindingText> {
    let mut findings = Vec::new();
    for file in spec_files {
        // REMOVED 與 RENAMED 區段的 requirement 不要求 scenario。
        let headers: Vec<_> = file
            .requirements()
            .into_iter()
            .filter(|requirement| !requirement.renamed_from)
            .collect();
        for (index, requirement) in headers.iter().enumerate() {
            let end = headers.get(index + 1).map(|next| next.line);
            if matches!(
                requirement.section,
                Some(DeltaSection::Removed | DeltaSection::Renamed)
            ) || file.has_scenario(requirement, end)
            {
                continue;
            }
            let name = &requirement.name;
            findings.push(FindingText {
                severity: "Warning",
                location: file.relative_path.clone(),
                summary: format!("Requirement '{name}' has no scenarios"),
                recommendation: format!("Add #### Scenario: sections with WHEN/THEN for '{name}'"),
                key: "ambNoScenario",
                summary_params: params(&[("req", name)]),
                recommendation_params: params(&[("req", name)]),
            });
        }
        for scenario in file.scenarios() {
            if extract::scenario_has_concrete_data(&scenario.body) {
                continue;
            }
            let name = scenario.name;
            findings.push(FindingText {
                severity: "Suggestion",
                location: file.relative_path.clone(),
                summary: format!("Scenario '{name}' has no concrete examples"),
                recommendation: "Add ##### Example: with concrete GIVEN/WHEN/THEN data".to_string(),
                key: "ambAbstractScenario",
                summary_params: params(&[("scenario", &name)]),
                recommendation_params: params(&[("scenario", &name)]),
            });
        }
        for (line_index, line) in file.content.lines().enumerate() {
            let Some(pattern) = weak_language_pattern(line) else {
                continue;
            };
            findings.push(FindingText {
                severity: "Suggestion",
                location: format!("{}:{}", file.relative_path, line_index + 1),
                summary: format!("Vague language '{pattern}' found"),
                recommendation: format!("Replace '{pattern}' with SHALL/SHALL NOT for clarity"),
                key: "ambWeakLanguage",
                summary_params: params(&[("pattern", pattern)]),
                recommendation_params: params(&[("pattern", pattern)]),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weak_language_is_case_insensitive_substring_priority_and_canonicalized() {
        assert_eq!(
            weak_language_pattern("TBD first then SHOULD later"),
            Some("should")
        );
        assert_eq!(weak_language_pattern("MAYBE this works"), Some("may"));
        assert_eq!(weak_language_pattern("status tbd"), Some("TBD"));
        assert_eq!(weak_language_pattern("fully specified"), None);
        assert_eq!(
            weak_language_pattern("we could consider this"),
            Some("consider")
        );
        assert_eq!(weak_language_pattern("POSSIBLY later"), Some("possibly"));
        assert_eq!(weak_language_pattern("todo: wire it up"), Some("TODO"));
        assert_eq!(weak_language_pattern("placeholder TKTK"), Some("TKTK"));
        assert_eq!(weak_language_pattern("really ???"), Some("???"));
        assert_eq!(weak_language_pattern("it might"), Some("might"));
        assert_eq!(
            weak_language_pattern("consider possibly doing a todo"),
            Some("consider")
        );
        assert_eq!(weak_language_pattern("todo then tktk"), Some("TODO"));
    }

    #[test]
    fn weak_language_skips_headings_but_not_urls_or_code() {
        assert_eq!(weak_language_pattern("### Requirement: A should"), None);
        assert_eq!(weak_language_pattern("  # indented might"), None);
        assert_eq!(weak_language_pattern("\t# tab possibly"), None);
        assert_eq!(
            weak_language_pattern("see https://example.com/should"),
            Some("should")
        );
        assert_eq!(weak_language_pattern("inline `may` code"), Some("may"));
    }
}
