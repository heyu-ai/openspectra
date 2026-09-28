//! OpenSpec 1.13.2 `validate` 讀 Markdown 的方式，逐函式移植自
//! `@fission-ai/openspec` 的 `dist/core/parsers/`（`code-fence.js`、
//! `requirement-blocks.js`、`requirement-text.js`、`markdown-parser.js`、
//! `spec-structure.js`）與 `dist/core/validation/purpose-placeholder.js`。
//!
//! 決策 D1：`spectra validate` 的規則以 OpenSpec 1.13.2 為準。這裡刻意不共用
//! [`crate::markdown`]：那是 archive 的解析器（oracle 行為），兩者對 stray
//! `###`、重複的 `## ADDED Requirements`、`#####` scenario 等的解讀不同，而
//! archive 的拒絕條件不能因為 validate 改規則而跟著變。
//!
//! 移植時保留 JavaScript 的語意差異：
//! - 長度一律用 UTF-16 code unit 計（JS `.length`），中文 Purpose 才會與 OpenSpec
//!   同樣判斷「太短」。
//! - `\b(SHALL|MUST)\b` 是 ASCII 字界（JS 不帶 `u` 旗標的 `\b`），所以「系統SHALL」
//!   也算有 SHALL；Rust `regex` 預設的 Unicode 字界會把中文字當成字元而不算。
//! - 各 regex 的 `\s` 在 JS 與 Rust 都是 Unicode 空白，差別只在 U+FEFF，這裡不處理。

use once_cell::sync::Lazy;
use regex::Regex;

/// `requirement-blocks.js` 的 `REQUIREMENT_HEADER_REGEX`：第 0 欄、`###` 後的空白可省略。
static REQUIREMENT_HEADER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^###\s*Requirement:\s*(.+)\s*$").unwrap());
/// `## ` 章節標題（`splitTopLevelSections`／`findOrphanedRequirements`）。
static TOP_LEVEL_SECTION: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(##)\s+(.+)$").unwrap());
/// `isTopLevelHeader`：requirement block 在這裡結束。
static TOP_LEVEL_HEADER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^##\s+").unwrap());
static STRAY_H3: Lazy<Regex> = Lazy::new(|| Regex::new(r"^###\s+(.+?)\s*$").unwrap());
static REMOVED_BULLET: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*[-*+]\s*`?###\s*Requirement:\s*(.+?)`?\s*$").unwrap());
static RENAMED_FROM: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*[-*+]?\s*FROM:\s*`?###\s*Requirement:\s*(.+?)`?\s*$").unwrap());
static RENAMED_TO: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*[-*+]?\s*TO:\s*`?###\s*Requirement:\s*(.+?)`?\s*$").unwrap());
static REQUIREMENTS_HEADER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^##\s+Requirements\s*$").unwrap());
static CLOSING_HASHES: Lazy<Regex> = Lazy::new(|| Regex::new(r"[ \t]+#+[ \t]*$").unwrap());
static WHITESPACE_RUN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

// requirement-text.js
static METADATA_LINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\*\*[^*]+\*\*:").unwrap());
static HEADER_LINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^#{1,6}\s").unwrap());
static SCENARIO_HEADER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^####\s+").unwrap());
static SCENARIO_BODY_END: Lazy<Regex> = Lazy::new(|| Regex::new(r"^#{1,4}\s").unwrap());
static SCENARIO_PREFIX: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^Scenario:\s*").unwrap());
/// ASCII 字界：見模組說明。
static SHALL_OR_MUST: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?-u:\b)(SHALL|MUST)(?-u:\b)").unwrap());

// code-fence.js
static FENCE_OPEN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(`{3,}|~{3,})").unwrap());
static FENCE_CLOSE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(`{3,}|~{3,})\s*$").unwrap());

// markdown-parser.js
static SPEC_HEADER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(#{1,6})\s+(.+)$").unwrap());
static SPEC_HEADER_LEVEL: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(#{1,6})\s+").unwrap());

// spec-structure.js（requirement header 這裡要求 `###` 後有空白）
static STRUCTURE_DELTA_HEADER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^##\s+(ADDED|MODIFIED|REMOVED|RENAMED)\s+Requirements\s*$").unwrap()
});
static STRUCTURE_REQUIREMENT_HEADER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^###\s+Requirement:\s*(.+)\s*$").unwrap());

// purpose-placeholder.js
static PURPOSE_HEADER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^ {0,3}##[ \t]+Purpose[ \t]*$").unwrap());
/// `^ {0,3}#{1,2}(?!#)[ \t]+`：`[ \t]` 本身不是 `#`，所以負向前瞻可省略。
static PURPOSE_END_HEADER: Lazy<Regex> = Lazy::new(|| Regex::new(r"^ {0,3}#{1,2}[ \t]+").unwrap());
static WORD_CHAR: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\p{L}\p{N}\p{M}_]").unwrap());

pub(crate) const PURPOSE_PLACEHOLDER_PREFIX: &str = "TBD - created by archiving change ";
pub(crate) const PURPOSE_PLACEHOLDER_SUFFIX: &str = ". Update Purpose after archive.";

/// 去掉 UTF-8 BOM，`\r\n`／`\r` 換成 `\n`（`normalizeLineEndings`）。
pub(crate) fn normalize(content: &str) -> String {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    content.replace("\r\n", "\n").replace('\r', "\n")
}

/// JS 字串的 `.length`（UTF-16 code unit 數）。
pub(crate) fn js_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `buildCodeFenceMask`：`true` 表示該行在 fenced code block 內（含開關 fence 行）。
/// 開 fence 不限縮排（`^\s*`），與 [`crate::markdown`] 的「最多三格」不同。
pub(crate) fn fence_mask(lines: &[&str]) -> Vec<bool> {
    let mut mask = vec![false; lines.len()];
    let mut active: Option<(char, usize)> = None;
    for (index, line) in lines.iter().enumerate() {
        match active {
            None => {
                if let Some(captures) = FENCE_OPEN.captures(line) {
                    let run = &captures[1];
                    active = Some((run.chars().next().unwrap(), run.len()));
                    mask[index] = true;
                }
            }
            Some((marker, length)) => {
                mask[index] = true;
                if let Some(captures) = FENCE_CLOSE.captures(line) {
                    let run = &captures[1];
                    if run.starts_with(marker) && run.len() >= length {
                        active = None;
                    }
                }
            }
        }
    }
    mask
}

/// `normalizeRequirementName`：去掉 ATX 結尾的 `#` 串再 trim。
pub(crate) fn normalize_requirement_name(name: &str) -> String {
    CLOSING_HASHES.replace(name, "").trim().to_string()
}

/// `foldRequirementName`：只用來抓大小寫／空白不同的近似名稱。
pub(crate) fn fold_requirement_name(name: &str) -> String {
    WHITESPACE_RUN
        .replace_all(&normalize_requirement_name(name).to_lowercase(), " ")
        .into_owned()
}

pub(crate) fn contains_shall_or_must(text: &str) -> bool {
    SHALL_OR_MUST.is_match(text)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Block {
    pub name: String,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SkippedHeader {
    pub header: String,
    pub section: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnpairedRename {
    pub side: &'static str,
    pub name: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrphanedRequirement {
    pub name: String,
    pub section: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rename {
    pub from: String,
    pub to: String,
}

/// `parseDeltaSpec` 的結果（`DeltaPlan`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DeltaPlan {
    pub added: Vec<Block>,
    pub modified: Vec<Block>,
    pub removed: Vec<String>,
    pub renamed: Vec<Rename>,
    pub unpaired_renames: Vec<UnpairedRename>,
    pub orphaned_requirements: Vec<OrphanedRequirement>,
    pub skipped_headers: Vec<SkippedHeader>,
    pub added_present: bool,
    pub modified_present: bool,
    pub removed_present: bool,
    pub renamed_present: bool,
}

impl DeltaPlan {
    pub(crate) fn any_section_present(&self) -> bool {
        self.added_present || self.modified_present || self.removed_present || self.renamed_present
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.added.len() + self.modified.len() + self.removed.len() + self.renamed.len()
    }
}

struct SectionBody<'a> {
    lines: &'a [&'a str],
    mask: &'a [bool],
    /// section 標題的下一行（1-based）。
    body_start_line: usize,
}

struct Section<'a> {
    title: String,
    body: SectionBody<'a>,
}

fn split_top_level_sections<'a>(lines: &'a [&'a str], mask: &'a [bool]) -> Vec<Section<'a>> {
    let mut headers = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if mask[index] {
            continue;
        }
        if let Some(captures) = TOP_LEVEL_SECTION.captures(line) {
            headers.push((captures[2].trim().to_string(), index));
        }
    }
    headers
        .iter()
        .enumerate()
        .map(|(position, (title, index))| {
            let end = headers.get(position + 1).map_or(lines.len(), |next| next.1);
            Section {
                title: title.clone(),
                body: SectionBody {
                    lines: &lines[index + 1..end],
                    mask: &mask[index + 1..end],
                    body_start_line: index + 2,
                },
            }
        })
        .collect()
}

/// `getSectionsCaseInsensitive`：同名（不分大小寫）的 section 全部合併，標題取第一個。
fn sections_named<'s, 'a>(
    sections: &'s [Section<'a>],
    desired: &str,
) -> (String, Vec<&'s SectionBody<'a>>, bool) {
    let target = desired.to_lowercase();
    let matches: Vec<_> = sections
        .iter()
        .filter(|section| section.title.to_lowercase() == target)
        .collect();
    match matches.first() {
        None => (desired.to_string(), Vec::new(), false),
        Some(first) => (
            first.title.clone(),
            matches.iter().map(|section| &section.body).collect(),
            true,
        ),
    }
}

fn is_requirement_header(body: &SectionBody<'_>, index: usize) -> bool {
    !body.mask[index] && REQUIREMENT_HEADER.is_match(body.lines[index])
}

fn is_top_level_header(body: &SectionBody<'_>, index: usize) -> bool {
    !body.mask[index] && TOP_LEVEL_HEADER.is_match(body.lines[index])
}

/// `parseRequirementBlocksFromSection`：block 從 `### Requirement:` 延伸到下一個
/// `### Requirement:` 或 `## `，中間其他的 `###` 併入 block（C4），並記進
/// `skipped`（給 INFO 用）。
fn parse_requirement_blocks(
    body: &SectionBody<'_>,
    mut skipped: Option<(&str, &mut Vec<SkippedHeader>)>,
) -> Vec<Block> {
    let record = |index: usize, skipped: &mut Option<(&str, &mut Vec<SkippedHeader>)>| {
        let Some((section, sink)) = skipped.as_mut() else {
            return;
        };
        if body.mask[index] {
            return;
        }
        if let Some(captures) = STRAY_H3.captures(body.lines[index]) {
            if !REQUIREMENT_HEADER.is_match(body.lines[index]) {
                sink.push(SkippedHeader {
                    header: captures[1].trim().to_string(),
                    section: section.to_string(),
                    line: body.body_start_line + index,
                });
            }
        }
    };
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < body.lines.len() {
        while index < body.lines.len() && !is_requirement_header(body, index) {
            record(index, &mut skipped);
            index += 1;
        }
        if index >= body.lines.len() {
            break;
        }
        let header = body.lines[index];
        let name = normalize_requirement_name(&REQUIREMENT_HEADER.captures(header).unwrap()[1]);
        let mut buffer = vec![header];
        index += 1;
        while index < body.lines.len()
            && !is_requirement_header(body, index)
            && !is_top_level_header(body, index)
        {
            record(index, &mut skipped);
            buffer.push(body.lines[index]);
            index += 1;
        }
        blocks.push(Block {
            name,
            raw: buffer.join("\n").trim_end().to_string(),
        });
    }
    blocks
}

fn parse_removed_names(body: &SectionBody<'_>) -> Vec<String> {
    let mut names = Vec::new();
    for (index, line) in body.lines.iter().enumerate() {
        if body.mask[index] {
            continue;
        }
        if let Some(captures) = REQUIREMENT_HEADER.captures(line) {
            names.push(normalize_requirement_name(&captures[1]));
        } else if let Some(captures) = REMOVED_BULLET.captures(line) {
            names.push(normalize_requirement_name(&captures[1]));
        }
    }
    names
}

fn parse_renamed_pairs(body: &SectionBody<'_>, unpaired: &mut Vec<UnpairedRename>) -> Vec<Rename> {
    let mut pairs = Vec::new();
    let mut pending: Option<(String, usize)> = None;
    for (index, line) in body.lines.iter().enumerate() {
        if body.mask[index] {
            continue;
        }
        if let Some(captures) = RENAMED_FROM.captures(line) {
            if let Some((name, line)) = pending.take() {
                unpaired.push(UnpairedRename {
                    side: "FROM",
                    name,
                    line,
                });
            }
            pending = Some((
                normalize_requirement_name(&captures[1]),
                body.body_start_line + index,
            ));
        } else if let Some(captures) = RENAMED_TO.captures(line) {
            let to = normalize_requirement_name(&captures[1]);
            match pending.take() {
                Some((from, _)) => pairs.push(Rename { from, to }),
                None => unpaired.push(UnpairedRename {
                    side: "TO",
                    name: to,
                    line: body.body_start_line + index,
                }),
            }
        }
    }
    if let Some((name, line)) = pending {
        unpaired.push(UnpairedRename {
            side: "FROM",
            name,
            line,
        });
    }
    pairs
}

const DELTA_SECTION_TITLES: [&str; 4] = [
    "added requirements",
    "modified requirements",
    "removed requirements",
    "renamed requirements",
];

/// `findOrphanedRequirements`：不在任何 delta section 裡的 `### Requirement:`（C10）。
fn find_orphaned_requirements(lines: &[&str], mask: &[bool]) -> Vec<OrphanedRequirement> {
    let mut orphans = Vec::new();
    let mut section: Option<String> = None;
    for (index, line) in lines.iter().enumerate() {
        if mask[index] {
            continue;
        }
        if let Some(captures) = TOP_LEVEL_SECTION.captures(line) {
            section = Some(captures[2].trim().to_string());
            continue;
        }
        if section
            .as_ref()
            .is_some_and(|title| DELTA_SECTION_TITLES.contains(&title.to_lowercase().as_str()))
        {
            continue;
        }
        if let Some(captures) = REQUIREMENT_HEADER.captures(line) {
            orphans.push(OrphanedRequirement {
                name: normalize_requirement_name(&captures[1]),
                section: section.clone(),
                line: index + 1,
            });
        }
    }
    orphans
}

/// `parseDeltaSpec`。
pub(crate) fn parse_delta_spec(content: &str) -> DeltaPlan {
    let normalized = normalize(content);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mask = fence_mask(&lines);
    let sections = split_top_level_sections(&lines, &mask);
    let (added_title, added_bodies, added_present) =
        sections_named(&sections, "ADDED Requirements");
    let (modified_title, modified_bodies, modified_present) =
        sections_named(&sections, "MODIFIED Requirements");
    let (_, removed_bodies, removed_present) = sections_named(&sections, "REMOVED Requirements");
    let (_, renamed_bodies, renamed_present) = sections_named(&sections, "RENAMED Requirements");

    let mut skipped = Vec::new();
    let added = added_bodies
        .iter()
        .flat_map(|body| parse_requirement_blocks(body, Some((added_title.as_str(), &mut skipped))))
        .collect();
    let modified = modified_bodies
        .iter()
        .flat_map(|body| {
            parse_requirement_blocks(body, Some((modified_title.as_str(), &mut skipped)))
        })
        .collect();
    let removed = removed_bodies
        .iter()
        .flat_map(|body| parse_removed_names(body))
        .collect();
    let mut unpaired = Vec::new();
    let renamed = renamed_bodies
        .iter()
        .flat_map(|body| parse_renamed_pairs(body, &mut unpaired))
        .collect();
    unpaired.sort_by_key(|item| item.line);
    skipped.sort_by_key(|item| item.line);
    DeltaPlan {
        added,
        modified,
        removed,
        renamed,
        unpaired_renames: unpaired,
        orphaned_requirements: find_orphaned_requirements(&lines, &mask),
        skipped_headers: skipped,
        added_present,
        modified_present,
        removed_present,
        renamed_present,
    }
}

/// `extractRequirementsSection(content).bodyBlocks`：第一個 `## Requirements`
/// 底下的 `### Requirement:` block。
pub(crate) fn requirements_section_blocks(content: &str) -> Vec<Block> {
    let normalized = normalize(content);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mask = fence_mask(&lines);
    let Some(header) =
        (0..lines.len()).find(|&index| !mask[index] && REQUIREMENTS_HEADER.is_match(lines[index]))
    else {
        return Vec::new();
    };
    let end = (header + 1..lines.len())
        .find(|&index| !mask[index] && TOP_LEVEL_HEADER.is_match(lines[index]))
        .unwrap_or(lines.len());
    let body = SectionBody {
        lines: &lines[header + 1..end],
        mask: &mask[header + 1..end],
        body_start_line: header + 2,
    };
    // `extractRequirementsSection` 的 block 只在下一個 requirement header 或 `## `
    // 結束，與 delta 的 reader 相同，只是不記 stray header。
    parse_requirement_blocks(&body, None)
}

/// `extractRequirementBody`：header 之後、第一個標題之前的非空行（fence 內略過）；
/// `**key**:` metadata 行只在沒有其他內文時才算內文。
pub(crate) fn extract_requirement_body(body_lines: &[&str]) -> String {
    let mask = fence_mask(body_lines);
    let mut captured = Vec::new();
    let mut metadata = Vec::new();
    for (index, line) in body_lines.iter().enumerate() {
        if mask[index] {
            continue;
        }
        if HEADER_LINE.is_match(line) {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if METADATA_LINE.is_match(trimmed) {
            metadata.push(trimmed);
        } else {
            captured.push(trimmed);
        }
    }
    if captured.is_empty() {
        metadata.join("\n")
    } else {
        captured.join("\n")
    }
}

/// `Validator.extractRequirementText`：block 第一行是 header，不以 header 補位。
pub(crate) fn block_requirement_text(raw: &str) -> String {
    let lines: Vec<&str> = raw.split('\n').collect();
    extract_requirement_body(&lines[1..])
}

fn scenario_bodies(body_lines: &[&str]) -> Vec<String> {
    let mask = fence_mask(body_lines);
    let mut bodies = Vec::new();
    for index in 0..body_lines.len() {
        if mask[index] || !SCENARIO_HEADER.is_match(body_lines[index]) {
            continue;
        }
        let mut end = index + 1;
        while end < body_lines.len() && (mask[end] || !SCENARIO_BODY_END.is_match(body_lines[end]))
        {
            end += 1;
        }
        bodies.push(body_lines[index + 1..end].join("\n"));
    }
    bodies
}

/// `Validator.countScenarios`：有內文的 `#### ` 才算 scenario（C5）。
pub(crate) fn block_scenario_count(raw: &str) -> usize {
    let lines: Vec<&str> = raw.split('\n').collect();
    scenario_bodies(&lines[1..])
        .iter()
        .filter(|body| !body.trim().is_empty())
        .count()
}

pub(crate) fn block_empty_scenario_count(raw: &str) -> usize {
    let lines: Vec<&str> = raw.split('\n').collect();
    scenario_bodies(&lines[1..])
        .iter()
        .filter(|body| body.trim().is_empty())
        .count()
}

/// `parseScenarioBlocks` 的名稱：`#### ` 後的文字去掉結尾 `#` 串與 `Scenario:` 前綴。
fn scenario_names(raw: &str) -> Vec<String> {
    let normalized = normalize(raw);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mask = fence_mask(&lines);
    lines
        .iter()
        .enumerate()
        .filter(|(index, line)| !mask[*index] && SCENARIO_HEADER.is_match(line))
        .map(|(_, line)| {
            let text = SCENARIO_HEADER.replace(line, "");
            let text = CLOSING_HASHES.replace(&text, "");
            SCENARIO_PREFIX.replace(&text, "").trim().to_string()
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScenarioDiff {
    pub missing: Vec<String>,
    pub added: Vec<String>,
    pub current_count: usize,
    pub incoming_count: usize,
}

/// `diffScenarioNames`（計入重複次數）。
pub(crate) fn diff_scenario_names(current_raw: &str, incoming_raw: &str) -> ScenarioDiff {
    let current = scenario_names(current_raw);
    let incoming = scenario_names(incoming_raw);
    let unmatched = |names: &[String], against: &[String]| {
        let mut remaining: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
        for name in against {
            *remaining.entry(name).or_default() += 1;
        }
        let mut out = Vec::new();
        for name in names {
            match remaining.get_mut(name.as_str()) {
                Some(left) if *left > 0 => *left -= 1,
                _ => out.push(name.clone()),
            }
        }
        out
    };
    ScenarioDiff {
        missing: unmatched(&current, &incoming),
        added: unmatched(&incoming, &current),
        current_count: current.len(),
        incoming_count: incoming.len(),
    }
}

/// `describeScenarioBalance`。
pub(crate) fn describe_scenario_balance(diff: &ScenarioDiff) -> String {
    let count = |value: usize| {
        format!(
            "{value} {}",
            if value == 1 { "scenario" } else { "scenarios" }
        )
    };
    let scale = format!(
        "The modified block has {}; the current spec has {}.",
        count(diff.incoming_count),
        count(diff.current_count)
    );
    if diff.added.is_empty() {
        return format!("{scale} It adds none.");
    }
    let listed = diff
        .added
        .iter()
        .take(3)
        .map(|name| format!("\"{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let names = if diff.added.len() > 3 {
        format!("{listed} and {} more", diff.added.len() - 3)
    } else {
        listed
    };
    format!(
        "{scale} It adds {} not in the current spec: {names}.",
        count(diff.added.len())
    )
}

/// `MarkdownParser.parseSpec` 解出的 requirement：只留 validate 會用到的欄位。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpecRequirement {
    pub text: String,
    pub scenarios: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedSpec {
    pub overview: String,
    pub requirements: Vec<SpecRequirement>,
}

struct HeadingNode {
    level: usize,
    title: String,
    content: String,
    children: Vec<usize>,
}

/// `MarkdownParser.parseSpec`：標題樹；Purpose 與 Requirements 可在任何層級
/// （深度優先找第一個同名標題，S7），Requirements 的每個子標題都是
/// requirement（S1），requirement 的每個有內文的子標題都是 scenario（S6）。
/// 找不到 Purpose（或內容為空）／Requirements 時回傳 OpenSpec 丟出的錯誤訊息。
pub(crate) fn parse_spec(content: &str) -> Result<ParsedSpec, &'static str> {
    let normalized = normalize(content);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mask = fence_mask(&lines);
    let mut nodes: Vec<HeadingNode> = Vec::new();
    let mut roots = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if mask[index] {
            continue;
        }
        let Some(captures) = SPEC_HEADER.captures(line) else {
            continue;
        };
        let level = captures[1].len();
        let title = captures[2].trim().to_string();
        let mut content_lines = Vec::new();
        for next in index + 1..lines.len() {
            let is_boundary = !mask[next]
                && SPEC_HEADER_LEVEL
                    .captures(lines[next])
                    .is_some_and(|header| header[1].len() <= level);
            if is_boundary {
                break;
            }
            content_lines.push(lines[next]);
        }
        let node = nodes.len();
        nodes.push(HeadingNode {
            level,
            title,
            content: content_lines.join("\n").trim().to_string(),
            children: Vec::new(),
        });
        while stack.last().is_some_and(|&top| nodes[top].level >= level) {
            stack.pop();
        }
        match stack.last() {
            Some(&parent) => nodes[parent].children.push(node),
            None => roots.push(node),
        }
        stack.push(node);
    }

    fn find(nodes: &[HeadingNode], within: &[usize], title: &str) -> Option<usize> {
        for &node in within {
            if nodes[node].title.to_lowercase() == title.to_lowercase() {
                return Some(node);
            }
            if let Some(found) = find(nodes, &nodes[node].children, title) {
                return Some(found);
            }
        }
        None
    }

    let purpose = find(&nodes, &roots, "Purpose")
        .map(|node| nodes[node].content.clone())
        .unwrap_or_default();
    let requirements = find(&nodes, &roots, "Requirements");
    if purpose.is_empty() {
        return Err("Spec must have a Purpose section");
    }
    let Some(requirements) = requirements else {
        return Err("Spec must have a Requirements section");
    };
    let requirements = nodes[requirements]
        .children
        .iter()
        .map(|&child| {
            let node = &nodes[child];
            let body: Vec<&str> = node.content.split('\n').collect();
            let body_text = extract_requirement_body(&body);
            SpecRequirement {
                text: if body_text.is_empty() {
                    node.title.trim().to_string()
                } else {
                    body_text
                },
                scenarios: node
                    .children
                    .iter()
                    .filter(|&&scenario| !nodes[scenario].content.trim().is_empty())
                    .count(),
            }
        })
        .collect();
    Ok(ParsedSpec {
        overview: purpose.trim().to_string(),
        requirements,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StructureIssue {
    pub line: usize,
    pub message: String,
}

/// `findMainSpecStructureIssues`（S2 delta header、S3 Requirements 外的
/// requirement、S4 重複的 requirement）。
pub(crate) fn main_spec_structure_issues(content: &str) -> Vec<StructureIssue> {
    // 這裡只換行尾、不去 BOM，與 spec-structure.js 相同。
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let raw_lines: Vec<&str> = normalized.split('\n').collect();
    let mask = fence_mask(&raw_lines);
    let lines: Vec<&str> = raw_lines
        .iter()
        .enumerate()
        .map(|(index, line)| if mask[index] { "" } else { *line })
        .collect();
    let header = lines
        .iter()
        .position(|line| REQUIREMENTS_HEADER.is_match(line));
    let end = header
        .and_then(|header| {
            (header + 1..lines.len()).find(|&index| TOP_LEVEL_HEADER.is_match(lines[index]))
        })
        .unwrap_or(lines.len());
    let mut issues = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if STRUCTURE_DELTA_HEADER.is_match(line) {
            issues.push(StructureIssue {
                line: index + 1,
                message: format!(
                    "Main spec contains delta header \"{trimmed}\". Delta headers are only valid inside openspec/changes/<name>/specs/<capability-path>/spec.md and truncate the parsed ## Requirements section."
                ),
            });
            continue;
        }
        let Some(captures) = STRUCTURE_REQUIREMENT_HEADER.captures(line) else {
            continue;
        };
        let inside = header.is_some_and(|header| index > header && index < end);
        if !inside {
            issues.push(StructureIssue {
                line: index + 1,
                message: format!(
                    "Requirement header \"{trimmed}\" appears outside the main ## Requirements section. Main specs only parse requirements inside that section, so this requirement is currently invisible to validate, list, and archive."
                ),
            });
            continue;
        }
        let name = normalize_requirement_name(&captures[1]);
        match seen.get(&name) {
            Some(previous) => issues.push(StructureIssue {
                line: index + 1,
                message: format!(
                    "Requirement header \"{trimmed}\" duplicates the requirement declared on line {previous}. Requirement names must be unique so spec updates cannot discard one block while updating another."
                ),
            }),
            None => {
                seen.insert(name, index + 1);
            }
        }
    }
    issues
}

/// `WORD_END`：後面不是字母、數字、組合符號或 `_`。
fn word_end(rest: &str) -> bool {
    !WORD_CHAR.is_match(rest)
}

/// `MARKER_PUNCTUATION`：略過空白與 tab 後，是行尾或 `:-–—.,;()[]{}` 之一。
fn marker_punctuation(rest: &str) -> bool {
    let rest = rest.trim_start_matches([' ', '\t']);
    match rest.chars().next() {
        None => true,
        Some(ch) => matches!(
            ch,
            '\n' | ':'
                | '-'
                | '\u{2013}'
                | '\u{2014}'
                | '.'
                | ','
                | ';'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
        ),
    }
}

fn leading_marker(prose: &str) -> bool {
    ["TBD", "TODO"].iter().any(|marker| {
        let n = marker.len();
        if prose.len() < n || !prose.is_char_boundary(n) {
            return false;
        }
        let (head, rest) = prose.split_at(n);
        if head == *marker {
            // 全大寫：後面是什麼都算。
            return word_end(rest);
        }
        head.eq_ignore_ascii_case(marker) && word_end(rest) && marker_punctuation(rest)
    })
}

fn generated_placeholder_prefix_index(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(offset) = text[from..].find(PURPOSE_PLACEHOLDER_SUFFIX) {
        let suffix_at = from + offset;
        // JS `lastIndexOf(prefix, suffixAt)`：起點不超過 suffixAt 的最後一個 prefix。
        let limit = (suffix_at + PURPOSE_PLACEHOLDER_PREFIX.len()).min(text.len());
        let mut limit = limit;
        while !text.is_char_boundary(limit) {
            limit -= 1;
        }
        if let Some(prefix_at) = text[..limit].rfind(PURPOSE_PLACEHOLDER_PREFIX) {
            return Some(prefix_at);
        }
        from = suffix_at + 1;
        while from < text.len() && !text.is_char_boundary(from) {
            from += 1;
        }
    }
    None
}

/// `findPurposePlaceholderIssue`：`None` 表示不是 placeholder；`Some(line)` 是
/// placeholder，`line` 為能定位時的行號。
pub(crate) fn purpose_placeholder(overview: &str, content: &str) -> Option<Option<usize>> {
    let normalized = normalize_line_endings_only(overview);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mask = fence_mask(&lines);
    let prose = lines
        .iter()
        .enumerate()
        .filter(|(index, _)| !mask[*index])
        .map(|(_, line)| *line)
        .collect::<Vec<_>>()
        .join("\n");
    let prose = prose.trim();
    let leading = leading_marker(prose);
    if !leading && generated_placeholder_prefix_index(prose).is_none() {
        return None;
    }
    Some(placeholder_line(content, leading))
}

fn normalize_line_endings_only(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn placeholder_line(content: &str, leading: bool) -> Option<usize> {
    let normalized = normalize_line_endings_only(content);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mask = fence_mask(&lines);
    let header =
        (0..lines.len()).find(|&index| !mask[index] && PURPOSE_HEADER.is_match(lines[index]))?;
    let mut purpose_lines = Vec::new();
    for index in header + 1..lines.len() {
        if mask[index] {
            continue;
        }
        if PURPOSE_END_HEADER.is_match(lines[index]) {
            break;
        }
        if leading && !lines[index].trim().is_empty() {
            return Some(index + 1);
        }
        purpose_lines.push((index + 1, lines[index]));
    }
    if leading {
        return None;
    }
    let purpose = purpose_lines
        .iter()
        .map(|(_, text)| *text)
        .collect::<Vec<_>>()
        .join("\n");
    let prefix_at = generated_placeholder_prefix_index(&purpose)?;
    let offset = purpose[..prefix_at].matches('\n').count();
    purpose_lines.get(offset).map(|(line, _)| *line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stray_level_three_header_stays_inside_the_requirement_block() {
        // C4：OpenSpec 讓 block 延伸到下一個 `### Requirement:`，stray `###` 之後的
        // scenario 仍屬於這個 requirement，並把 stray header 記成 INFO 用的資料。
        let plan = parse_delta_spec(
            "## ADDED Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha.\n\n\
             ### Notes\n\n#### Scenario: a1\n\n- **WHEN** x\n- **THEN** y\n",
        );
        assert_eq!(plan.added.len(), 1);
        assert_eq!(block_scenario_count(&plan.added[0].raw), 1);
        assert_eq!(
            plan.skipped_headers,
            vec![SkippedHeader {
                header: "Notes".to_string(),
                section: "ADDED Requirements".to_string(),
                line: 7,
            }]
        );
    }

    #[test]
    fn repeated_delta_sections_are_merged() {
        // C7
        let plan = parse_delta_spec(
            "## ADDED Requirements\n\n### Requirement: A\n\nx SHALL.\n\n\
             ## ADDED Requirements\n\n### Requirement: B\n\ny SHALL.\n",
        );
        let names: Vec<_> = plan.added.iter().map(|block| block.name.as_str()).collect();
        assert_eq!(names, ["A", "B"]);
    }

    #[test]
    fn a_scenario_header_without_a_body_is_not_a_scenario() {
        // C5
        let raw = "### Requirement: A\nThe system SHALL a.\n#### Scenario: empty\n";
        assert_eq!(block_scenario_count(raw), 0);
        assert_eq!(block_empty_scenario_count(raw), 1);
    }

    #[test]
    fn shall_detection_uses_ascii_word_boundaries() {
        assert!(contains_shall_or_must("系統SHALL記錄"));
        assert!(!contains_shall_or_must("MARSHALL"));
    }

    #[test]
    fn main_spec_requirements_are_every_child_of_requirements_at_any_level() {
        // S1／S6／S7
        let spec = parse_spec(
            "# s\n\n### Purpose\n\nWhy.\n\n## Requirements\n\n### Admin Portal\n\n\
             The system SHALL admin.\n\n##### Scenarios:\n\n- x\n",
        )
        .unwrap();
        assert_eq!(spec.overview, "Why.");
        assert_eq!(
            spec.requirements,
            vec![SpecRequirement {
                text: "The system SHALL admin.".to_string(),
                scenarios: 1,
            }]
        );
    }

    #[test]
    fn placeholder_detection_follows_case_and_punctuation_rules() {
        let content = "## Purpose\n\nTODO write it\n";
        assert_eq!(purpose_placeholder("TODO write it", content), Some(Some(3)));
        assert_eq!(
            purpose_placeholder("todo: later", "## Purpose\n\ntodo: later\n"),
            Some(Some(3))
        );
        assert_eq!(
            purpose_placeholder("Todo el sistema", "## Purpose\n\nTodo el sistema\n"),
            None
        );
        assert_eq!(
            purpose_placeholder("TODOs remain", "## Purpose\n\nTODOs remain\n"),
            None
        );
    }
}
