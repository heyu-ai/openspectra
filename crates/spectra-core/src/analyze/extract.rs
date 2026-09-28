//! Markdown 解析：delta spec 的 requirement／scenario／Purpose、proposal 的
//! capability 與 tasks.md 的 task 行（oracle 3.0.0 `analyzer::extract` 與
//! delta-spec parser 的行為，逐條見 `docs/reverse-engineering/analyze.md`）。

use std::collections::HashSet;
use std::path::Path;

use anyhow::{anyhow, Context, Result};

/// fence 分隔行：`trim_start` 後以 ```` ``` ```` 或 `~~~` 開頭（可帶 info string）。
/// 各處都以「任一分隔行切換」處理，不比對開頭與結尾的字元是否相同。
pub(super) fn is_fence_delimiter(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum DeltaSection {
    Added,
    Modified,
    Removed,
    Renamed,
}

impl DeltaSection {
    /// 只認第 0 欄起算的 `## ` heading，比對 `trim_end` 後的全文：
    /// `## ADDED Requirements  ` 算，`  ## MODIFIED Requirements` 與
    /// `## added requirements` 不算（golden `delta-validation-multi` v7）。
    fn from_heading(line: &str) -> Option<Self> {
        match line.trim_end() {
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

/// `### Requirement: <name>`（trim 後比對，名稱 trim；空名稱不算）。
pub(super) fn requirement_name(line: &str) -> Option<&str> {
    let name = line.trim().strip_prefix("### Requirement:")?.trim();
    (!name.is_empty()).then_some(name)
}

/// RENAMED 區段的 `- FROM:` 行取出舊名稱：`- FROM:` 必須大小寫相符且用 `-` bullet
/// （可縮排），其後可選的反引號裡必須是 `### Requirement: <name>`；
/// `- FROM: Bare` 與 `` - FROM: `Requirement: X` `` 都不算（golden `coverage-tasks`）。
fn renamed_from_name(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix("- FROM:")?.trim();
    let rest = rest.strip_prefix('`').unwrap_or(rest);
    let rest = rest.strip_suffix('`').unwrap_or(rest);
    requirement_name(rest)
}

/// 一個 requirement header（或 RENAMED 的 FROM 名稱）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Requirement {
    pub name: String,
    /// 所在的 delta 區段；其他 `## ` heading 之下或第一個 heading 之前為 `None`。
    pub section: Option<DeltaSection>,
    /// 0-based 行號。
    pub line: usize,
    /// 來自 RENAMED 的 `- FROM:` 行，而不是 `### Requirement:` header。
    pub renamed_from: bool,
}

/// 一個 `#### Scenario:` 區塊。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Scenario {
    pub name: String,
    pub body: Vec<String>,
}

/// delta-spec parser 的結果：各操作區段的 requirement 名稱與 `## Purpose` 內容。
/// 同名區段 heading 重複時後者整段取代前者（oracle 以 map 存；golden v5／v6）。
#[derive(Debug, Default)]
pub(super) struct DeltaSections {
    added: Vec<String>,
    modified: Vec<String>,
    removed: Vec<String>,
    /// `## Purpose` 之後到下一個 `## ` heading 為止的內容；沒有 Purpose 為 `None`。
    pub purpose: Option<String>,
}

impl DeltaSections {
    fn list_mut(&mut self, section: DeltaSection) -> Option<&mut Vec<String>> {
        match section {
            DeltaSection::Added => Some(&mut self.added),
            DeltaSection::Modified => Some(&mut self.modified),
            DeltaSection::Removed => Some(&mut self.removed),
            DeltaSection::Renamed => None,
        }
    }

    pub fn modified(&self) -> &[String] {
        &self.modified
    }

    pub fn has_operation_requirements(&self) -> bool {
        !(self.added.is_empty() && self.modified.is_empty() && self.removed.is_empty())
    }

    /// `## Purpose` 存在且去掉空白後有內容。
    pub fn has_nonempty_purpose(&self) -> bool {
        self.purpose
            .as_deref()
            .is_some_and(|purpose| !purpose.trim().is_empty())
    }

    /// analyze 回報的 delta 驗證錯誤：先是各區段內的重複（ADDED、MODIFIED、
    /// REMOVED，每多出現一次一筆），再是跨區段衝突（(ADDED,MODIFIED)、
    /// (ADDED,REMOVED)、(MODIFIED,REMOVED) 依序），最後是 Purpose 檢查。
    /// RENAMED 不參與。同一對區段內 oracle 以 HashSet 走訪（每次執行順序不同），
    /// OpenSpectra 固定為前一個區段的文件順序。
    pub fn validation_errors(&self) -> Vec<String> {
        let ordered = [
            (DeltaSection::Added, &self.added),
            (DeltaSection::Modified, &self.modified),
            (DeltaSection::Removed, &self.removed),
        ];
        let mut errors = Vec::new();
        for (section, names) in ordered {
            let mut seen = HashSet::new();
            for name in names {
                if !seen.insert(name.as_str()) {
                    errors.push(format!(
                        "Duplicate requirement '{name}' in {} section",
                        section.label()
                    ));
                }
            }
        }
        for (left, right) in [(0, 1), (0, 2), (1, 2)] {
            let (left_section, left_names) = ordered[left];
            let (right_section, right_names) = ordered[right];
            let mut reported = HashSet::new();
            for name in left_names {
                if right_names.contains(name) && reported.insert(name.as_str()) {
                    errors.push(format!(
                        "Requirement '{name}' appears in both {} and {} sections",
                        left_section.label(),
                        right_section.label()
                    ));
                }
            }
        }
        if let Some(purpose) = &self.purpose {
            if purpose.trim().is_empty() {
                errors.push(
                    "Invalid format: Purpose section is empty; add 1-3 sentences describing the capability or remove the section"
                        .to_string(),
                );
            } else if purpose.contains("TBD") || purpose.contains("TODO") {
                // 大小寫敏感：oracle 不擋 `tbd`／`todo`（golden p9、c10）。
                errors.push(
                    "Invalid format: Purpose section contains placeholder text (TBD/TODO); write the actual capability purpose"
                        .to_string(),
                );
            }
        }
        errors
    }
}

/// 一個 change 的 delta spec：`specs/<dir>/spec.md`（恰好一層）。
#[derive(Debug)]
pub(super) struct SpecFile {
    /// change 目錄起算的路徑，例如 `specs/cap/spec.md`。
    pub relative_path: String,
    /// `<dir>`。
    pub capability: String,
    pub content: String,
}

impl SpecFile {
    /// fence-aware 掃描出的 requirement header，以及 RENAMED 區段裡的 FROM 名稱，
    /// 依文件順序。
    pub fn requirements(&self) -> Vec<Requirement> {
        let mut requirements = Vec::new();
        let mut section = None;
        let mut in_fence = false;
        for (line_index, line) in self.content.lines().enumerate() {
            if is_fence_delimiter(line) {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                continue;
            }
            if line.starts_with("## ") {
                section = DeltaSection::from_heading(line);
                continue;
            }
            if let Some(name) = requirement_name(line) {
                requirements.push(Requirement {
                    name: name.to_string(),
                    section,
                    line: line_index,
                    renamed_from: false,
                });
            } else if section == Some(DeltaSection::Renamed) {
                if let Some(name) = renamed_from_name(line) {
                    requirements.push(Requirement {
                        name: name.to_string(),
                        section,
                        line: line_index,
                        renamed_from: true,
                    });
                }
            }
        }
        requirements
    }

    /// delta-spec parser（fence-aware）：各操作區段的 requirement 與 Purpose。
    pub fn delta_sections(&self) -> DeltaSections {
        let mut sections = DeltaSections::default();
        let mut current: Option<DeltaSection> = None;
        let mut in_purpose = false;
        let mut in_fence = false;
        for line in self.content.lines() {
            if is_fence_delimiter(line) {
                in_fence = !in_fence;
            } else if !in_fence && line.starts_with("## ") {
                in_purpose = line.trim_end() == "## Purpose";
                if in_purpose {
                    sections.purpose = Some(String::new());
                }
                current = DeltaSection::from_heading(line);
                if let Some(list) = current.and_then(|section| sections.list_mut(section)) {
                    list.clear();
                }
                continue;
            }
            if in_purpose {
                let purpose = sections
                    .purpose
                    .as_mut()
                    .expect("in_purpose implies a Purpose section");
                purpose.push_str(line);
                purpose.push('\n');
                continue;
            }
            if in_fence {
                continue;
            }
            if let Some(name) = requirement_name(line) {
                if let Some(list) = current.and_then(|section| sections.list_mut(section)) {
                    list.push(name.to_string());
                }
            }
        }
        sections
    }

    /// requirement 區塊（header 之後到下一個 requirement header 之前，`end` 為其行號；
    /// 最後一個到檔尾）是否有 `#### Scenario:` 行。其他 `### `、`## ` heading 都不結束
    /// 區塊（語料 yibi-mvp 0098 與 golden `ambiguity` req-blocks），掃描也不看 fence：
    /// fence 裡的 scenario header 同樣算（golden `delta-validation-multi` f3）。
    pub fn has_scenario(&self, requirement: &Requirement, end: Option<usize>) -> bool {
        self.content
            .lines()
            .enumerate()
            .skip(requirement.line + 1)
            .take_while(|(index, _)| end.is_none_or(|end| *index < end))
            .any(|(_, line)| line.trim().starts_with("#### Scenario:"))
    }

    /// fence-aware 掃描出的 `#### Scenario:` 區塊；區塊在下一個 `## `／`### `／
    /// `#### ` heading 結束（`#####`、`# ` 與 `####Note` 都不結束，golden
    /// `ambiguity` concrete）。區塊內容保留 fence 分隔行，供 [`scenario_has_concrete_data`]
    /// 自己切換 fence 狀態。
    pub fn scenarios(&self) -> Vec<Scenario> {
        let mut scenarios: Vec<Scenario> = Vec::new();
        let mut open = false;
        let mut in_fence = false;
        for line in self.content.lines() {
            if is_fence_delimiter(line) {
                in_fence = !in_fence;
            } else if !in_fence {
                let trimmed = line.trim();
                if trimmed.starts_with("## ")
                    || trimmed.starts_with("### ")
                    || trimmed.starts_with("#### ")
                {
                    open = false;
                    if let Some(name) = trimmed.strip_prefix("#### Scenario:") {
                        scenarios.push(Scenario {
                            name: name.trim().to_string(),
                            body: Vec::new(),
                        });
                        open = true;
                    }
                    continue;
                }
            }
            if open {
                scenarios
                    .last_mut()
                    .expect("an open block has a scenario")
                    .body
                    .push(line.to_string());
            }
        }
        scenarios
    }
}

/// scenario 區塊是否含具體資料（oracle `scenario_has_concrete_data`）：跳過 fence
/// 內的行；每行 `trim` 後以 `##### Example:` 或 `- **GIVEN**` 開頭、以 `|` 開頭且含
/// 至少 3 個 `|`，或以 `- **WHEN**`／`- **THEN**`／`- **AND**` 開頭且含 `"`、反引號
/// 或 ASCII 數字。全部大小寫敏感。
pub(super) fn scenario_has_concrete_data(body: &[String]) -> bool {
    let mut in_fence = false;
    for raw in body {
        if is_fence_delimiter(raw) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let line = raw.trim();
        if line.starts_with("##### Example:") || line.starts_with("- **GIVEN**") {
            return true;
        }
        if line.starts_with('|') && line.matches('|').count() >= 3 {
            return true;
        }
        if (line.starts_with("- **WHEN**")
            || line.starts_with("- **THEN**")
            || line.starts_with("- **AND**"))
            && line
                .chars()
                .any(|c| c == '"' || c == '`' || c.is_ascii_digit())
        {
            return true;
        }
    }
    false
}

fn utf8_name(name: &std::ffi::OsStr) -> Result<String> {
    name.to_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("path component {name:?} is not valid UTF-8"))
}

/// `specs/<dir>/spec.md` 是一般檔案（可經 symlink）的每個 `<dir>`。oracle 依
/// readdir 順序走訪；OpenSpectra 依名稱 byte 排序，讓相同的樹在任何檔案系統上
/// 產生相同報表（刻意分歧，同 W7a `list`）。巢狀的 `specs/a/b/spec.md`、平放的
/// `specs/x.md` 與 `specs/x/other.md` 都不算（oracle 3.0.0 p23）。
pub(super) fn collect_spec_files(change_dir: &Path) -> Result<Vec<SpecFile>> {
    let specs_dir = change_dir.join("specs");
    let Some(entries) = crate::fsutil::read_dir_optional(&specs_dir)? else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", specs_dir.display()))?;
        if entry.path().join("spec.md").is_file() {
            names.push(utf8_name(&entry.file_name())?);
        }
    }
    names.sort();
    names
        .into_iter()
        .map(|capability| {
            let path = specs_dir.join(&capability).join("spec.md");
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            Ok(SpecFile {
                relative_path: format!("specs/{capability}/spec.md"),
                capability,
                content,
            })
        })
        .collect()
}

/// proposal 的 capability（oracle `extract_proposal_capabilities`）：`trim` 後以
/// `## Capabilities`、`### New Capabilities` 或 `### Modified Capabilities` 開頭的行
/// 開啟區段，下一個 `## ` 行關閉（不看 fence）；區段內每一行取第一個反引號 token，
/// 空 token 與含 ASCII 空白的 token 不算。
pub(super) fn proposal_capabilities(proposal: &str) -> Vec<String> {
    let mut capabilities = Vec::new();
    let mut in_section = false;
    for line in proposal.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## Capabilities")
            || trimmed.starts_with("### New Capabilities")
            || trimmed.starts_with("### Modified Capabilities")
        {
            in_section = true;
            continue;
        }
        if trimmed.starts_with("## ") {
            in_section = false;
            continue;
        }
        if !in_section {
            continue;
        }
        let Some((_, rest)) = trimmed.split_once('`') else {
            continue;
        };
        let Some((token, _)) = rest.split_once('`') else {
            continue;
        };
        if !token.is_empty() && !token.contains(' ') {
            capabilities.push(token.to_string());
        }
    }
    capabilities
}

/// tasks.md 的 task 行（小寫）：`trim_start` 後以 `- [`、`* [` 或 `+ [` 開頭，接任一個
/// 字元再接 `]`（`- [ ]1.8` 算；`- []`、`- [link](url)`、`1. [ ]`、`-\t[ ]` 不算）。
pub(super) fn task_lines(tasks: &str) -> Vec<String> {
    tasks
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            let Some(rest) = ["- [", "* [", "+ ["]
                .iter()
                .find_map(|prefix| trimmed.strip_prefix(prefix))
            else {
                return false;
            };
            let mut chars = rest.chars();
            chars.next().is_some() && chars.next() == Some(']')
        })
        .map(str::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(content: &str) -> SpecFile {
        SpecFile {
            relative_path: "specs/cap/spec.md".to_string(),
            capability: "cap".to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn capabilities_take_the_first_backtick_token_of_every_section_line() {
        let proposal = "## Why\n- `before`\n## Capabilities (v2)\n\
            - `alpha`: first `ignored`\n\
            continuation `beta` here\n\
            | `gamma` | x |\n\
            - `Has Space`\n- ``\n\
            #### deeper\n- `delta`\n\
            ## Impact\n- `after`\n\
            ### Modified Capabilities\n- `epsilon`\n";
        assert_eq!(
            proposal_capabilities(proposal),
            ["alpha", "beta", "gamma", "delta", "epsilon"]
        );
    }

    #[test]
    fn task_lines_need_a_single_character_checkbox_after_the_bullet() {
        let tasks = "- [ ] a\n* [x] b\n+ [~] c\n  - [ ]d\n- [] e\n- [link](u) f\n1. [ ] g\n-\t[ ] h\n## i\n";
        assert_eq!(
            task_lines(tasks),
            ["- [ ] a", "* [x] b", "+ [~] c", "  - [ ]d"]
        );
    }

    #[test]
    fn requirements_skip_fences_and_collect_renamed_from_names() {
        let file = spec(
            "### Requirement: Before\n\
             ## ADDED Requirements\n\
             ```\n### Requirement: Fenced\n```\n\
             ### Requirement:\n\
             ## RENAMED Requirements\n\
             - FROM: `### Requirement: Old`\n\
             - FROM: Bare\n\
             ### Requirement: Inside Renamed\n\
             ## REMOVED Requirements\n\
             ### Requirement: Gone\n",
        );
        let found: Vec<_> = file
            .requirements()
            .into_iter()
            .map(|r| (r.name, r.section, r.renamed_from))
            .collect();
        assert_eq!(
            found,
            [
                ("Before".to_string(), None, false),
                ("Old".to_string(), Some(DeltaSection::Renamed), true),
                (
                    "Inside Renamed".to_string(),
                    Some(DeltaSection::Renamed),
                    false
                ),
                ("Gone".to_string(), Some(DeltaSection::Removed), false),
            ]
        );
    }

    #[test]
    fn validation_reports_duplicates_then_cross_sections_then_purpose() {
        let file = spec(
            "## Purpose\n\n## ADDED Requirements\n### Requirement: A\n### Requirement: A\n### Requirement: A\n\
             ### Requirement: B\n### Requirement: C\n\
             ## MODIFIED Requirements\n### Requirement: C\n### Requirement: B\n\
             ## REMOVED Requirements\n### Requirement: B\n",
        );
        assert_eq!(
            file.delta_sections().validation_errors(),
            [
                "Duplicate requirement 'A' in ADDED section",
                "Duplicate requirement 'A' in ADDED section",
                "Requirement 'B' appears in both ADDED and MODIFIED sections",
                "Requirement 'C' appears in both ADDED and MODIFIED sections",
                "Requirement 'B' appears in both ADDED and REMOVED sections",
                "Requirement 'B' appears in both MODIFIED and REMOVED sections",
                "Invalid format: Purpose section is empty; add 1-3 sentences describing the capability or remove the section",
            ]
        );
    }

    #[test]
    fn a_repeated_section_heading_replaces_the_earlier_section() {
        let file = spec(
            "## ADDED Requirements\n### Requirement: X\n### Requirement: X\n\
             ## ADDED Requirements\n### Requirement: Y\n",
        );
        let sections = file.delta_sections();
        assert!(sections.validation_errors().is_empty());
        assert!(sections.has_operation_requirements());
    }

    #[test]
    fn purpose_content_runs_to_the_next_level_two_heading() {
        let with_h3 = spec("## Purpose\n\n### Sub\n\n## ADDED Requirements\n");
        assert!(with_h3.delta_sections().has_nonempty_purpose());
        let blank = spec("## Purpose  \r\n \t\r\n## ADDED Requirements\n");
        assert_eq!(
            blank.delta_sections().purpose.as_deref().map(str::trim),
            Some("")
        );
        let indented = spec("  ## Purpose\n\nP.\n");
        assert_eq!(indented.delta_sections().purpose, None);
        let fenced = spec("```\n## Purpose\n\nP.\n```\n");
        assert_eq!(fenced.delta_sections().purpose, None);
        let placeholder = spec("## Purpose\n\nwork todo, TBDX\n");
        assert_eq!(
            placeholder.delta_sections().validation_errors(),
            ["Invalid format: Purpose section contains placeholder text (TBD/TODO); write the actual capability purpose"]
        );
    }

    #[test]
    fn scenario_blocks_end_at_level_two_to_four_headings() {
        let file = spec(
            "#### Scenario: a\n##### Example: e\n\
             #### Scenario: b\n# Title\n- **GIVEN** g\n\
             #### Scenario: c\n#### Note\n- **GIVEN** g\n\
             ```\n#### Scenario: fenced\n```\n",
        );
        let found: Vec<_> = file
            .scenarios()
            .into_iter()
            .map(|s| (s.name.clone(), scenario_has_concrete_data(&s.body)))
            .collect();
        assert_eq!(
            found,
            [
                ("a".to_string(), true),
                ("b".to_string(), true),
                ("c".to_string(), false),
            ]
        );
    }

    #[test]
    fn concrete_data_rules_are_case_sensitive_prefixes() {
        let concrete = |line: &str| scenario_has_concrete_data(&[line.to_string()]);
        assert!(concrete("- **GIVEN** a"));
        assert!(concrete("    - **WHEN** 3 things"));
        assert!(concrete("- **WHEN**3"));
        assert!(concrete("- **THEN** \"c\""));
        assert!(concrete("- **AND** `c`"));
        assert!(concrete("| a | b |"));
        assert!(!concrete("| a b |"));
        assert!(!concrete("- **When** 3 things"));
        assert!(!concrete("* **WHEN** 3 things"));
        assert!(!concrete("- **GIVEN:** x"));
        assert!(!concrete("- **THEN** 顯示３個"));
        assert!(!concrete("- **THEN** ok"));
        assert!(!scenario_has_concrete_data(&[
            "```".to_string(),
            "- **GIVEN** a".to_string(),
            "```".to_string(),
        ]));
    }
}
