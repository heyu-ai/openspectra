//! Canonical spec 的追溯資料 sidecar：`specs/<cap>/spec.trace.yaml`
//! （OpenSpectra-only，heyu-ai/openspectra#98；方向由下游 ADR-0029 D3 裁決）。
//!
//! oracle 在每個 ADDED／MODIFIED requirement 底下各寫一份 inline
//! `<!-- @trace ... -->` footer，`code:` 清單對同一次 archive 的每個
//! requirement 都一樣，spec 因此以 O(requirements × archives) 成長。這裡改成
//! 每次 archive 在 sidecar 追加一筆紀錄，`spec.md` 只在標題下留一行指標
//! [`POINTER`]。
//!
//! oracle 3.0.0 仍會寫 inline footer（實測：ADDED 與 MODIFIED 都會），所以
//! 混用時 spec.md 裡會再出現 footer；[`extract_inline`] 在下一次 archive
//! 或 `spectra trace migrate` 時把它們吸收進 sidecar。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// sidecar 檔名，與 `spec.md` 放在同一個 capability 目錄。
pub const SIDECAR_FILE: &str = "spec.trace.yaml";

/// `spec.md` 標題下的一行指標。HTML 註解在渲染後看不到，oracle 3.0.0 的
/// validate／archive 實測都能接受。
pub const POINTER: &str = "<!-- @trace-sidecar: spec.trace.yaml -->";

const FORMAT_VERSION: u32 = 1;

const HEADER: &str = "# Traceability for spec.md, written by `spectra archive` and\n\
# `spectra trace migrate`. One entry per archived change.\n";

/// `spec.md` 對應的 sidecar 路徑。
pub fn sidecar_path(spec_path: &Path) -> PathBuf {
    spec_path.with_file_name(SIDECAR_FILE)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceFile {
    pub version: u32,
    #[serde(default)]
    pub traces: Vec<TraceEntry>,
}

impl Default for TraceFile {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            traces: Vec::new(),
        }
    }
}

/// 一次 archive（或一組被吸收的 inline footer）的追溯紀錄。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceEntry {
    pub source: String,
    pub updated: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renamed: Vec<RenamedRequirement>,
    /// 從 inline footer 吸收的 requirement。footer 本身不記錄當時是 ADDED
    /// 還是 MODIFIED，所以不猜，另列一欄。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imported: Vec<String>,
    #[serde(default)]
    pub code: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tests: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenamedRequirement {
    pub from: String,
    pub to: String,
}

/// 從 spec.md 剝出來的一個 inline footer。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineFooter {
    /// footer 所在的 requirement；footer 不在任何 requirement 內時為 `None`。
    pub requirement: Option<String>,
    pub source: String,
    pub updated: String,
    pub code: Vec<String>,
    pub tests: Vec<String>,
}

/// [`extract_inline`] 的結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    /// 剝掉可解析 footer 之後的內容。
    pub content: String,
    pub footers: Vec<InlineFooter>,
    /// 認得開頭、但內容無法解析（缺 `source`／`updated`、有不認得的欄位、
    /// 沒有結尾 `-->`）而原樣留在 spec.md 的 footer，記 1-based 行號。
    /// 不認得的內容不猜，保留原文讓人處理。
    pub unparsed_lines: Vec<usize>,
}

/// 剝除 `content` 裡所有 code fence 以外、可解析的 `<!-- @trace ... -->`
/// footer，連同它前面的空行一起移除。`content` 應已經過
/// `markdown::normalize_markdown`。
pub fn extract_inline(content: &str) -> Extracted {
    let requirements = crate::markdown::parse_main_requirements(content);
    let lines: Vec<&str> = content.split('\n').collect();
    let mask = crate::markdown::fenced_line_mask(&lines);
    let mut offsets = Vec::with_capacity(lines.len());
    let mut offset = 0;
    for line in &lines {
        offsets.push(offset);
        offset += line.len() + 1;
    }

    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    let mut footers = Vec::new();
    let mut unparsed_lines = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        if mask[index] || line.trim() != "<!-- @trace" {
            out.push(line);
            index += 1;
            continue;
        }
        let close = ((index + 1)..lines.len()).find(|j| lines[*j].trim() == "-->");
        let parsed = close.and_then(|close| parse_footer_body(&lines[index + 1..close]));
        let (Some(close), Some((source, updated, code, tests))) = (close, parsed) else {
            unparsed_lines.push(index + 1);
            out.push(line);
            index += 1;
            continue;
        };
        let at = offsets[index];
        let requirement = requirements
            .iter()
            .rev()
            .find(|requirement| requirement.start <= at)
            .map(|requirement| requirement.name.clone());
        footers.push(InlineFooter {
            requirement,
            source,
            updated,
            code,
            tests,
        });
        while out.last().is_some_and(|last| last.trim().is_empty()) {
            out.pop();
        }
        index = close + 1;
    }
    Extracted {
        content: out.join("\n"),
        footers,
        unparsed_lines,
    }
}

type FooterBody = (String, String, Vec<String>, Vec<String>);

/// 解析 footer 內文（oracle 的格式：`source:`、`updated:`、`code:`／`tests:`
/// 清單）。任何不認得的行都讓整個 footer 視為無法解析。
fn parse_footer_body(lines: &[&str]) -> Option<FooterBody> {
    let mut source = None;
    let mut updated = None;
    let mut code = Vec::new();
    let mut tests = Vec::new();
    let mut list: Option<&mut Vec<String>> = None;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix("- ") {
            list.as_mut()?.push(unquote(item.trim()).to_string());
            continue;
        }
        let (key, value) = trimmed.split_once(':')?;
        let value = value.trim();
        match key.trim() {
            "source" if !value.is_empty() => {
                source = Some(unquote(value).to_string());
                list = None;
            }
            "updated" if !value.is_empty() => {
                updated = Some(unquote(value).to_string());
                list = None;
            }
            "code" | "tests" if value.is_empty() || value == "[]" => {
                let target = if key.trim() == "code" {
                    &mut code
                } else {
                    &mut tests
                };
                list = (value.is_empty()).then_some(target);
            }
            _ => return None,
        }
    }
    Some((source?, updated?, code, tests))
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return inner;
        }
    }
    value
}

impl TraceFile {
    /// 讀取 sidecar；不存在時回傳 `None`。無法解析或版本不認得時回錯，
    /// 絕不當成空檔覆寫——那會永久丟掉既有的追溯資料。
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let Some(text) = crate::fsutil::read_optional(path)? else {
            return Ok(None);
        };
        let file: TraceFile = serde_yaml::from_str(&text)
            .with_context(|| format!("{} is not a valid trace sidecar", path.display()))?;
        if file.version != FORMAT_VERSION {
            anyhow::bail!(
                "{} has unsupported trace sidecar version {} (expected {FORMAT_VERSION})",
                path.display(),
                file.version
            );
        }
        Ok(Some(file))
    }

    pub fn to_yaml(&self) -> Result<String> {
        Ok(format!("{HEADER}{}", serde_yaml::to_string(self)?))
    }

    /// 把 inline footer 併進 sidecar。`(source, updated, code, tests)` 相同的
    /// footer 合成一筆，requirement 名稱記在 `imported`；已經記錄過的名稱不
    /// 重複加入，所以對同一份內容重跑是冪等的（遷移中途失敗後可以重跑）。
    pub fn absorb(&mut self, footers: &[InlineFooter]) {
        for footer in footers {
            let existing = self.traces.iter_mut().find(|entry| {
                entry.source == footer.source
                    && entry.updated == footer.updated
                    && entry.code == footer.code
                    && entry.tests == footer.tests
            });
            let entry = match existing {
                Some(entry) => entry,
                None => {
                    self.traces.push(TraceEntry {
                        source: footer.source.clone(),
                        updated: footer.updated.clone(),
                        code: footer.code.clone(),
                        tests: footer.tests.clone(),
                        ..Default::default()
                    });
                    self.traces.last_mut().expect("just pushed")
                }
            };
            let Some(name) = &footer.requirement else {
                continue;
            };
            let known = entry
                .added
                .iter()
                .chain(&entry.modified)
                .chain(&entry.imported)
                .any(|existing| same_name(existing, name));
            if !known {
                entry.imported.push(name.clone());
            }
        }
    }

    /// 把既有紀錄裡的 requirement 名稱跟著 RENAMED 改名，讓舊紀錄仍對得到
    /// 現在的 requirement。`renamed` 欄位本身記的是歷史，不改寫。
    pub fn apply_renames(&mut self, renames: &[RenamedRequirement]) {
        for rename in renames {
            for entry in &mut self.traces {
                for list in [
                    &mut entry.added,
                    &mut entry.modified,
                    &mut entry.removed,
                    &mut entry.imported,
                ] {
                    for name in list.iter_mut() {
                        if same_name(name, &rename.from) {
                            *name = rename.to.clone();
                        }
                    }
                }
            }
        }
    }
}

fn same_name(left: &str, right: &str) -> bool {
    crate::markdown::normalize_name(left) == crate::markdown::normalize_name(right)
}

/// 確保 `content` 在標題（第一個 code fence 外的 `# ` 行）下有 [`POINTER`]；
/// 沒有標題時放在最前面。已經有就原樣回傳。
pub fn ensure_pointer(content: &str) -> String {
    let lines: Vec<&str> = content.split('\n').collect();
    let mask = crate::markdown::fenced_line_mask(&lines);
    if lines
        .iter()
        .enumerate()
        .any(|(index, line)| !mask[index] && line.trim() == POINTER)
    {
        return content.to_string();
    }
    let title = lines
        .iter()
        .enumerate()
        .find(|(index, line)| !mask[*index] && line.starts_with("# "))
        .map(|(index, _)| index);
    let Some(title) = title else {
        return format!("{POINTER}\n\n{content}");
    };
    let mut out: Vec<&str> = Vec::with_capacity(lines.len() + 2);
    out.extend_from_slice(&lines[..=title]);
    out.push("");
    out.push(POINTER);
    let rest = &lines[title + 1..];
    if !rest.first().is_some_and(|line| line.trim().is_empty()) {
        out.push("");
    }
    out.extend_from_slice(rest);
    out.join("\n")
}

/// 是否為 [`POINTER`] 那一行（給需要忽略它的結構檢查用）。
pub(crate) fn is_pointer_line(line: &str) -> bool {
    line.trim() == POINTER
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORACLE_SPEC: &str = "# cap Specification\n\n## Purpose\n\nP.\n\n## Requirements\n\n\
### Requirement: Alpha\n\nThe system SHALL alpha.\n\n#### Scenario: a\n\n- **WHEN** x\n- **THEN** y\n\n\n\
<!-- @trace\nsource: demo\nupdated: 2026-09-26\ncode:\n  - pre.txt\n  - a.rs\n-->\n\n---\n\
### Requirement: Beta\n\nThe system SHALL beta.\n\n#### Scenario: b\n\n- **WHEN** x\n- **THEN** y\n\n\
<!-- @trace\nsource: demo\nupdated: 2026-09-26\ncode:\n  - pre.txt\n  - a.rs\n-->\n";

    fn footer(requirement: &str, source: &str, code: &[&str]) -> InlineFooter {
        InlineFooter {
            requirement: Some(requirement.to_string()),
            source: source.to_string(),
            updated: "2026-09-26".to_string(),
            code: code.iter().map(|c| c.to_string()).collect(),
            tests: Vec::new(),
        }
    }

    #[test]
    fn extract_inline_strips_oracle_footers_and_attributes_them_to_requirements() {
        // 這份輸入是 oracle 3.0.0 實際 archive 出來的形狀（含第一個 footer 前的兩個空行）。
        let extracted = extract_inline(ORACLE_SPEC);

        assert_eq!(
            extracted.content,
            "# cap Specification\n\n## Purpose\n\nP.\n\n## Requirements\n\n\
### Requirement: Alpha\n\nThe system SHALL alpha.\n\n#### Scenario: a\n\n- **WHEN** x\n- **THEN** y\n\n---\n\
### Requirement: Beta\n\nThe system SHALL beta.\n\n#### Scenario: b\n\n- **WHEN** x\n- **THEN** y\n"
        );
        assert_eq!(
            extracted.footers,
            vec![
                footer("Alpha", "demo", &["pre.txt", "a.rs"]),
                footer("Beta", "demo", &["pre.txt", "a.rs"]),
            ]
        );
        assert!(extracted.unparsed_lines.is_empty());
    }

    #[test]
    fn extract_inline_parses_empty_lists_tests_and_quoted_values() {
        let content = "### Requirement: A\n\ntext\n\n<!-- @trace\nsource: \"x\"\nupdated: '2026-01-01'\ncode: []\ntests:\n  - t.rs\n-->\n";
        let extracted = extract_inline(content);
        assert_eq!(
            extracted.footers,
            vec![InlineFooter {
                requirement: None,
                source: "x".into(),
                updated: "2026-01-01".into(),
                code: vec![],
                tests: vec!["t.rs".into()],
            }]
        );
        assert_eq!(extracted.content, "### Requirement: A\n\ntext\n");
    }

    #[test]
    fn extract_inline_leaves_unrecognized_or_fenced_footers_in_place() {
        let content = "## Requirements\n\n### Requirement: A\n\ntext\n\n\
<!-- @trace\nsource: x\nupdated: y\nowner: someone\n-->\n\n\
```\n<!-- @trace\nsource: fenced\nupdated: y\ncode: []\n-->\n```\n\n\
<!-- @trace\nsource: unterminated\n";
        let extracted = extract_inline(content);
        assert_eq!(
            extracted.content, content,
            "nothing parseable should be removed"
        );
        assert!(extracted.footers.is_empty());
        assert_eq!(extracted.unparsed_lines, vec![7, 21]);
    }

    #[test]
    fn absorb_groups_identical_footers_and_is_idempotent() {
        let mut file = TraceFile::default();
        let footers = vec![
            footer("Alpha", "demo", &["a.rs"]),
            footer("Beta", "demo", &["a.rs"]),
            footer("Gamma", "other", &["b.rs"]),
        ];
        file.absorb(&footers);
        let once = file.clone();
        file.absorb(&footers);

        assert_eq!(
            file, once,
            "re-absorbing the same footers must not duplicate"
        );
        assert_eq!(file.traces.len(), 2);
        assert_eq!(file.traces[0].imported, vec!["Alpha", "Beta"]);
        assert_eq!(file.traces[0].code, vec!["a.rs"]);
        assert_eq!(file.traces[1].imported, vec!["Gamma"]);
    }

    #[test]
    fn absorb_does_not_reimport_a_requirement_the_entry_already_records() {
        let mut file = TraceFile::default();
        file.traces.push(TraceEntry {
            source: "demo".into(),
            updated: "2026-09-26".into(),
            added: vec!["Alpha".into()],
            code: vec!["a.rs".into()],
            ..Default::default()
        });
        file.absorb(&[footer("Alpha", "demo", &["a.rs"])]);
        assert_eq!(file.traces.len(), 1);
        assert!(file.traces[0].imported.is_empty());
    }

    #[test]
    fn apply_renames_updates_names_but_not_rename_history() {
        let mut file = TraceFile::default();
        let rename = RenamedRequirement {
            from: "Old".into(),
            to: "New".into(),
        };
        file.traces.push(TraceEntry {
            source: "a".into(),
            updated: "d".into(),
            added: vec!["Old".into()],
            imported: vec!["Old".into()],
            renamed: vec![rename.clone()],
            ..Default::default()
        });
        file.apply_renames(std::slice::from_ref(&rename));
        assert_eq!(file.traces[0].added, vec!["New"]);
        assert_eq!(file.traces[0].imported, vec!["New"]);
        assert_eq!(file.traces[0].renamed, vec![rename]);
    }

    #[test]
    fn ensure_pointer_inserts_once_under_the_title() {
        let content = "# cap Specification\n\n## Purpose\n\nP.\n";
        let with = ensure_pointer(content);
        assert_eq!(
            with,
            format!("# cap Specification\n\n{POINTER}\n\n## Purpose\n\nP.\n")
        );
        assert_eq!(ensure_pointer(&with), with);
        assert_eq!(
            ensure_pointer("## Purpose\n"),
            format!("{POINTER}\n\n## Purpose\n")
        );
        assert_eq!(
            ensure_pointer("# t\n## Purpose\n"),
            format!("# t\n\n{POINTER}\n\n## Purpose\n")
        );
    }

    #[test]
    fn yaml_round_trips_and_omits_empty_lists_except_code() {
        let mut file = TraceFile::default();
        file.traces.push(TraceEntry {
            source: "add-login".into(),
            updated: "2026-09-26".into(),
            added: vec!["Login Button".into()],
            ..Default::default()
        });
        let yaml = file.to_yaml().unwrap();
        assert!(yaml.starts_with("# Traceability"));
        assert!(yaml.contains("code: []"), "{yaml}");
        assert!(!yaml.contains("modified"), "{yaml}");
        let parsed: TraceFile = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, file);
    }

    #[test]
    fn load_rejects_a_corrupt_or_unknown_version_sidecar() {
        let dir = std::env::temp_dir().join(format!(
            "spectra-trace-load-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(SIDECAR_FILE);
        assert!(TraceFile::load(&path).unwrap().is_none());
        std::fs::write(&path, "traces: [").unwrap();
        assert!(TraceFile::load(&path).is_err());
        std::fs::write(&path, "version: 2\ntraces: []\n").unwrap();
        let error = TraceFile::load(&path).unwrap_err().to_string();
        assert!(
            error.contains("unsupported trace sidecar version 2"),
            "{error}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
