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
//! 混用時 spec.md 裡會再出現 footer；[`extract_inline`] 在下一次改到該
//! capability 的 archive，或 `spectra trace migrate` 時把它們吸收進 sidecar
//! （`trace migrate --check` 在那之前就能偵測到）。

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

/// 三個 sidecar 結構都 `deny_unknown_fields`：手改 sidecar 打錯的欄位若被
/// 默默接受，下一次重寫就會把它丟掉，所以寧可讓它走「壞掉的 sidecar」路徑。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
/// footer，連同它前面的空行一起移除；footer 後面若緊接著非空行，保留一個
/// 空行，免得 `---` 貼上前一段而被渲染成 setext 標題。`content` 應已經過
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
            .find(|requirement| requirement.start <= at && at < requirement.end)
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
        let next_is_content = lines
            .get(close + 1)
            .is_some_and(|next| !next.trim().is_empty());
        if next_is_content && out.last().is_some_and(|last| !last.trim().is_empty()) {
            out.push("");
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
        Self::parse(&text, path).map(Some)
    }

    /// 解析已經讀進來的 sidecar 內容；`path` 只用在錯誤訊息。讓呼叫端能用同
    /// 一份 bytes 同時做「寫入前沒被改過」的比對與解析，不必讀兩次。
    pub fn parse(text: &str, path: &Path) -> Result<Self> {
        let file: TraceFile = serde_yaml::from_str(text)
            .with_context(|| format!("{} is not a valid trace sidecar", path.display()))?;
        if file.version != FORMAT_VERSION {
            anyhow::bail!(
                "{} has unsupported trace sidecar version {} (expected {FORMAT_VERSION})",
                path.display(),
                file.version
            );
        }
        Ok(file)
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
    /// 現在的 requirement。只改最後一次移除該名稱之後的紀錄：被 RENAMED 的
    /// 必定是現存的 requirement，更早被移除的同名 requirement 是另一個。
    /// `removed`、`renamed` 記的是歷史，一律不改寫。
    pub fn apply_renames(&mut self, renames: &[RenamedRequirement]) {
        for rename in renames {
            let first = self
                .traces
                .iter()
                .rposition(|entry| {
                    entry
                        .removed
                        .iter()
                        .any(|name| same_name(name, &rename.from))
                })
                .map_or(0, |index| index + 1);
            for entry in &mut self.traces[first..] {
                for list in [&mut entry.added, &mut entry.modified, &mut entry.imported] {
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

/// 把 1-based 行號列成警告用的文字，例如 `line 3` 或 `lines 3, 9`。
pub fn describe_lines(lines: &[usize]) -> String {
    let joined = lines
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    if lines.len() == 1 {
        format!("line {joined}")
    } else {
        format!("lines {joined}")
    }
}

/// 是否為 [`POINTER`] 那一行（給需要忽略它的結構檢查用）。
pub(crate) fn is_pointer_line(line: &str) -> bool {
    line.trim() == POINTER
}

/// sidecar 裡記錄的 requirement 名稱中，最後一個事件不是「移除」、卻對不到
/// `content` 裡任何現有 requirement 的那些（依首次出現順序、去重）。
///
/// 典型成因是 oracle 做了 RENAMED：它只改 spec.md 的標題、不認得 sidecar，
/// 舊紀錄的名稱就此過時（openspectra 自己的 RENAMED 會同步改寫，見
/// [`TraceFile::apply_renames`]）。事件依紀錄順序判斷，同一筆紀錄內依
/// archive 的套用順序（先 `removed`、後 `modified`／`added`）；`imported`
/// 也算「存在」事件。所以名稱被移除後又重新加入時，舊的移除紀錄不會豁免
/// 它。`renamed` 記的是歷史，不列入判斷。
pub fn stale_names(trace: &TraceFile, content: &str) -> Vec<String> {
    let normalize = crate::markdown::normalize_name;
    let current: std::collections::HashSet<String> =
        crate::markdown::parse_main_requirements(content)
            .iter()
            .map(|requirement| normalize(&requirement.name))
            .collect();
    // 每個名稱的最後一個事件是否為「移除」，以及它第一次出現時的寫法與順序。
    let mut last_removed: std::collections::HashMap<String, bool> = Default::default();
    let mut order: Vec<(String, String)> = Vec::new();
    for entry in &trace.traces {
        let events = entry.removed.iter().map(|name| (name, true)).chain(
            entry
                .modified
                .iter()
                .chain(&entry.added)
                .chain(&entry.imported)
                .map(|name| (name, false)),
        );
        for (name, removed) in events {
            let key = normalize(name);
            if !last_removed.contains_key(&key) {
                order.push((key.clone(), name.clone()));
            }
            last_removed.insert(key, removed);
        }
    }
    order
        .into_iter()
        .filter(|(key, _)| !last_removed[key] && !current.contains(key))
        .map(|(_, name)| name)
        .collect()
}

/// `spectra trace migrate` 對一份 canonical spec 的結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigratedSpec {
    pub capability: String,
    /// 找到的可解析 inline footer 數；實際寫入時就是搬進 sidecar 的數量。
    pub footers: usize,
    /// 認得開頭但無法解析、原樣留在 spec.md 的 footer 行號，以處理後的
    /// spec.md 為準（沒有寫入時就是目前檔案的行號）。
    pub unparsed_lines: Vec<usize>,
    /// sidecar 裡對不到現有 requirement 的名稱（見 [`stale_names`]）；只回報、
    /// 不自動修正。
    pub stale_names: Vec<String>,
    /// 這份 spec 處理失敗的原因（例如 sidecar 壞掉）。有值時 spec.md 沒被
    /// 改寫；若失敗發生在寫 spec.md 那一步，sidecar 可能已經吸收了 footer，
    /// 重跑是冪等的（見 [`migrate`]）。
    pub error: Option<String>,
}

impl MigratedSpec {
    /// spec.md 裡還有 inline footer（可解析或不可解析）、sidecar 有過時名稱，
    /// 或處理失敗——`spectra trace migrate --check` 據此以非零結束。
    pub fn needs_attention(&self) -> bool {
        self.footers > 0
            || !self.unparsed_lines.is_empty()
            || !self.stale_names.is_empty()
            || self.error.is_some()
    }
}

/// 把所有 canonical spec 裡的 inline `@trace` footer 搬進各自的 sidecar，並
/// 補上 [`POINTER`]；同時回報 sidecar 裡的過時名稱。只回報
/// [`MigratedSpec::needs_attention`] 的 spec。
///
/// 每份 spec 先寫 sidecar、再寫 spec.md，各自原子寫入。中途失敗時 spec.md
/// 仍保有 footer，重跑會再吸收一次；[`TraceFile::absorb`] 對相同內容是冪等
/// 的，所以不會重複記錄。`dry_run` 只計算、不寫檔。
pub fn migrate(cfg: &crate::Config, dry_run: bool) -> Result<Vec<MigratedSpec>> {
    let specs_root = cfg.specs_dir();
    let mut report = Vec::new();
    for (capability, raw) in crate::fsutil::collect_delta_specs(&specs_root)? {
        let normalized = crate::markdown::normalize_markdown(&raw);
        let extracted = extract_inline(&normalized);
        let spec_path = specs_root.join(&capability).join("spec.md");
        let sidecar = sidecar_path(&spec_path);
        let mut entry = MigratedSpec {
            capability: capability.clone(),
            footers: extracted.footers.len(),
            unparsed_lines: extracted.unparsed_lines.clone(),
            stale_names: Vec::new(),
            error: None,
        };
        let outcome = (|| -> Result<()> {
            let existing = TraceFile::load(&sidecar)?;
            if existing.is_none() && extracted.footers.is_empty() {
                return Ok(());
            }
            let mut trace = existing.unwrap_or_default();
            trace.absorb(&extracted.footers);
            entry.stale_names = stale_names(&trace, &extracted.content);
            if extracted.footers.is_empty() || dry_run {
                return Ok(());
            }
            let yaml = trace.to_yaml()?;
            let mut content = ensure_pointer(&extracted.content);
            content.truncate(content.trim_end_matches('\n').len());
            content.push('\n');
            crate::fsutil::write_atomically(&sidecar, &yaml)
                .with_context(|| format!("writing {}", sidecar.display()))?;
            crate::fsutil::write_atomically(&spec_path, &content)
                .with_context(|| format!("writing {}", spec_path.display()))?;
            // 剝掉 footer、插入指標後行號會位移：改報寫出後檔案的行號。
            entry.unparsed_lines = extract_inline(&content).unparsed_lines;
            Ok(())
        })();
        if let Err(error) = outcome {
            entry.error = Some(format!("{error:#}"));
        }
        if entry.needs_attention() {
            report.push(entry);
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// oracle 3.0.0 的 footer 形狀（第一份 footer 前兩個空行）。oracle 實際輸出
    /// 在最後一個 `-->` 之後沒有換行（見 archive.rs 的 `ORACLE_ARCHIVED_SPEC`）；
    /// 這裡刻意多一個換行，涵蓋被編輯器補上檔尾換行的變體。
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

    fn migrate_cfg(tmp: &crate::test_support::TempDir) -> crate::Config {
        crate::Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        }
    }

    fn write_spec(cfg: &crate::Config, capability: &str, content: &str) -> PathBuf {
        let path = cfg.specs_dir().join(capability).join("spec.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn migrate_moves_footers_into_the_sidecar_and_is_idempotent() {
        let tmp = crate::test_support::TempDir::new("trace-migrate");
        let cfg = migrate_cfg(&tmp);
        let spec = write_spec(&cfg, "cap", ORACLE_SPEC);
        let clean = "# clean Specification\n\n## Purpose\n\nP.\n\n## Requirements\n\n### Requirement: X\n\ntext\n";
        let clean_path = write_spec(&cfg, "clean", clean);

        let report = migrate(&cfg, false).unwrap();

        assert_eq!(
            report,
            vec![MigratedSpec {
                capability: "cap".into(),
                footers: 2,
                unparsed_lines: vec![],
                stale_names: vec![],
                error: None,
            }]
        );
        let migrated = std::fs::read_to_string(&spec).unwrap();
        assert!(!migrated.contains("<!-- @trace\n"), "{migrated}");
        assert!(migrated.starts_with(&format!("# cap Specification\n\n{POINTER}\n\n")));
        let trace = TraceFile::load(&sidecar_path(&spec)).unwrap().unwrap();
        assert_eq!(trace.traces.len(), 1);
        assert_eq!(trace.traces[0].imported, vec!["Alpha", "Beta"]);
        // 沒有 footer 的 spec 完全不動，也不會多出 sidecar。
        assert_eq!(std::fs::read_to_string(&clean_path).unwrap(), clean);
        assert!(!sidecar_path(&clean_path).exists());

        assert!(
            migrate(&cfg, false).unwrap().is_empty(),
            "second run finds nothing"
        );
    }

    #[test]
    fn migrate_rerun_after_a_partial_failure_does_not_duplicate_entries() {
        // 模擬「sidecar 寫好、spec.md 還沒寫」就中斷：spec.md 仍有 footer。
        let tmp = crate::test_support::TempDir::new("trace-migrate-rerun");
        let cfg = migrate_cfg(&tmp);
        let spec = write_spec(&cfg, "cap", ORACLE_SPEC);
        migrate(&cfg, false).unwrap();
        let sidecar_before = std::fs::read_to_string(sidecar_path(&spec)).unwrap();
        std::fs::write(&spec, ORACLE_SPEC).unwrap();

        migrate(&cfg, false).unwrap();

        assert_eq!(
            std::fs::read_to_string(sidecar_path(&spec)).unwrap(),
            sidecar_before
        );
    }

    #[test]
    fn stale_names_flags_names_missing_from_the_spec_but_not_removed_ones() {
        let mut file = TraceFile::default();
        file.traces.push(TraceEntry {
            source: "a".into(),
            updated: "d".into(),
            added: vec!["Login Button".into(), "Gone".into(), "Kept".into()],
            imported: vec!["Login Button".into()],
            ..Default::default()
        });
        file.traces.push(TraceEntry {
            source: "b".into(),
            updated: "d".into(),
            removed: vec!["Gone".into()],
            ..Default::default()
        });
        // oracle 把 Login Button 改名成 Sign In Button，sidecar 沒跟著改。
        let content = "## Requirements\n\n### Requirement: Sign In Button\n\nx\n\n### Requirement: Kept\n\ny\n";

        assert_eq!(stale_names(&file, content), vec!["Login Button"]);
    }

    fn entry(source: &str) -> TraceEntry {
        TraceEntry {
            source: source.into(),
            updated: "d".into(),
            ..Default::default()
        }
    }

    #[test]
    fn stale_names_respects_event_order_for_a_removed_then_readded_name() {
        // #175 review：Alpha 被刪掉又重新加入，之後被 oracle 改名成 Beta——
        // 最早那筆 removed 不能豁免後來重新加入的 Alpha。
        let mut file = TraceFile::default();
        file.traces.push(TraceEntry {
            added: vec!["Alpha".into()],
            ..entry("one")
        });
        file.traces.push(TraceEntry {
            removed: vec!["Alpha".into()],
            ..entry("two")
        });
        file.traces.push(TraceEntry {
            added: vec!["Alpha".into()],
            ..entry("three")
        });
        let content = "## Requirements\n\n### Requirement: Beta\n\nx\n";
        assert_eq!(stale_names(&file, content), vec!["Alpha"]);

        // 對照：最後一個事件是 removed 時，對不到才是正常的。
        file.traces.push(TraceEntry {
            removed: vec!["Alpha".into()],
            ..entry("four")
        });
        assert!(stale_names(&file, content).is_empty());
    }

    #[test]
    fn stale_names_checks_the_modified_list_too() {
        let mut file = TraceFile::default();
        file.traces.push(TraceEntry {
            modified: vec!["Old Mod".into()],
            ..entry("a")
        });
        assert_eq!(
            stale_names(&file, "## Requirements\n\n### Requirement: New Mod\n\nx\n"),
            vec!["Old Mod"]
        );
    }

    #[test]
    fn apply_renames_leaves_removal_history_alone() {
        // 被 RENAMED 的必定是現存的 requirement，同名的 removed 紀錄描述的是更早
        // 被刪掉的另一個 requirement，改寫它會捏造「New 被刪過」的歷史。
        let mut file = TraceFile::default();
        file.traces.push(TraceEntry {
            added: vec!["Old".into()],
            ..entry("first-life")
        });
        file.traces.push(TraceEntry {
            removed: vec!["Old".into()],
            ..entry("a")
        });
        file.traces.push(TraceEntry {
            added: vec!["Old".into()],
            ..entry("b")
        });
        file.apply_renames(&[RenamedRequirement {
            from: "Old".into(),
            to: "New".into(),
        }]);
        // 被移除之前的那個 Old 是另一個 requirement，不跟著改名。
        assert_eq!(file.traces[0].added, vec!["Old"]);
        assert_eq!(file.traces[1].removed, vec!["Old"]);
        assert_eq!(file.traces[2].added, vec!["New"]);
    }

    #[test]
    fn extract_inline_does_not_attribute_a_footer_outside_every_requirement() {
        let content = "## Requirements\n\n### Requirement: A\n\nx\n\n## Notes\n\n\
<!-- @trace\nsource: s\nupdated: d\ncode: []\n-->\n";
        let extracted = extract_inline(content);
        assert_eq!(extracted.footers.len(), 1);
        assert_eq!(extracted.footers[0].requirement, None);
    }

    #[test]
    fn extract_inline_keeps_a_blank_line_before_a_following_separator() {
        // 剝掉 footer 後若 `---` 直接貼在段落下面，CommonMark 會把段落渲染成 H2。
        let content = "### Requirement: A\n\ntext\n\n<!-- @trace\nsource: s\nupdated: d\ncode: []\n-->\n---\n### Requirement: B\n";
        assert_eq!(
            extract_inline(content).content,
            "### Requirement: A\n\ntext\n\n---\n### Requirement: B\n"
        );
    }

    #[test]
    fn extract_inline_rejects_malformed_footer_bodies() {
        // 格式錯誤的 footer 不猜：不剝、不吸收，列出行號。
        for body in [
            "source: x\n- stray\nupdated: y",
            "source: x",
            "source:\nupdated: y",
            "source: x\nupdated: y\ncode: []\n  - a.rs",
        ] {
            let content = format!("### Requirement: A\n\ntext\n\n<!-- @trace\n{body}\n-->\n");
            let extracted = extract_inline(&content);
            assert!(extracted.footers.is_empty(), "{body:?} → {extracted:?}");
            assert_eq!(extracted.content, content, "{body:?}");
            assert_eq!(extracted.unparsed_lines, vec![5], "{body:?}");
        }
    }

    #[test]
    fn absorb_records_a_footer_that_belongs_to_no_requirement() {
        let mut file = TraceFile::default();
        let mut orphan = footer("unused", "orphan", &["o.rs"]);
        orphan.requirement = None;
        file.absorb(&[orphan]);
        assert_eq!(file.traces.len(), 1);
        assert_eq!(file.traces[0].source, "orphan");
        assert_eq!(file.traces[0].code, vec!["o.rs"]);
        assert!(file.traces[0].imported.is_empty());
    }

    #[test]
    fn ensure_pointer_ignores_a_heading_inside_a_code_fence() {
        assert_eq!(
            ensure_pointer("```\n# c\n```\n# T\n"),
            format!("```\n# c\n```\n# T\n\n{POINTER}\n")
        );
    }

    #[test]
    fn load_rejects_a_sidecar_with_unknown_fields() {
        // #175 review：手改 sidecar 打錯欄位（`addded`）時，重寫會把它靜默丟掉。
        let dir = crate::test_support::TempDir::new("trace-unknown-field");
        let path = dir.join(SIDECAR_FILE);
        std::fs::write(
            &path,
            "version: 1\ntraces:\n- source: a\n  updated: d\n  addded:\n  - Login Button\n  code: []\n",
        )
        .unwrap();
        let error = TraceFile::load(&path).unwrap_err();
        assert!(format!("{error:#}").contains("addded"), "{error:#}");
    }

    #[test]
    fn migrate_keeps_going_after_one_corrupt_sidecar() {
        // AC-5：某份 sidecar 壞掉只讓那一份失敗，其他照常處理。
        let tmp = crate::test_support::TempDir::new("trace-migrate-mixed");
        let cfg = migrate_cfg(&tmp);
        let broken = write_spec(&cfg, "a-broken", ORACLE_SPEC);
        std::fs::write(sidecar_path(&broken), "traces: [").unwrap();
        let fine = write_spec(&cfg, "b-fine", ORACLE_SPEC);
        let clean_broken = write_spec(
            &cfg,
            "c-clean",
            "# c Specification\n\n## Requirements\n\n### Requirement: X\n\nx\n",
        );
        std::fs::write(sidecar_path(&clean_broken), "traces: [").unwrap();

        let report = migrate(&cfg, false).unwrap();

        let by_cap = |cap: &str| report.iter().find(|spec| spec.capability == cap).unwrap();
        assert!(by_cap("a-broken").error.is_some());
        assert!(by_cap("b-fine").error.is_none());
        assert!(!std::fs::read_to_string(&fine)
            .unwrap()
            .contains("<!-- @trace\n"));
        // 沒有 footer、但 sidecar 壞掉的 spec 也要回報。
        assert!(by_cap("c-clean").error.is_some());
        assert!(by_cap("c-clean").needs_attention());
    }

    #[test]
    fn migrate_reports_unparsed_lines_of_the_rewritten_file() {
        // 剝掉前面的 footer、插入指標後行號會位移，回報的要是寫出後的行號。
        let tmp = crate::test_support::TempDir::new("trace-migrate-lines");
        let cfg = migrate_cfg(&tmp);
        let spec = write_spec(
            &cfg,
            "cap",
            "# cap Specification\n\n## Requirements\n\n### Requirement: A\n\na\n\n\
             <!-- @trace\nsource: s\nupdated: d\ncode: []\n-->\n\n---\n### Requirement: B\n\nb\n\n\
             <!-- @trace\nsource: x\nupdated: y\nowner: someone\n-->\n",
        );

        let report = migrate(&cfg, false).unwrap();

        let written = std::fs::read_to_string(&spec).unwrap();
        let expected = written
            .split('\n')
            .position(|line| line == "<!-- @trace")
            .unwrap()
            + 1;
        assert_eq!(report[0].unparsed_lines, vec![expected]);
    }

    #[test]
    fn migrate_reports_a_spec_whose_only_footer_is_unrecognized() {
        let tmp = crate::test_support::TempDir::new("trace-migrate-unparsed");
        let cfg = migrate_cfg(&tmp);
        let content = "# cap Specification\n\n## Requirements\n\n### Requirement: A\n\ntext\n\n<!-- @trace\nsource: x\nupdated: y\nowner: someone\n-->\n";
        write_spec(&cfg, "cap", content);

        let report = migrate(&cfg, false).unwrap();

        assert_eq!(report.len(), 1);
        assert_eq!(report[0].footers, 0);
        assert_eq!(report[0].unparsed_lines, vec![9]);
        assert!(report[0].needs_attention());
    }

    #[test]
    fn migrate_reports_stale_names_in_a_spec_without_footers_and_writes_nothing() {
        let tmp = crate::test_support::TempDir::new("trace-migrate-stale");
        let cfg = migrate_cfg(&tmp);
        let content = format!(
            "# cap Specification\n\n{POINTER}\n\n## Purpose\n\nP.\n\n## Requirements\n\n\
             ### Requirement: Sign In Button\n\ntext\n"
        );
        let spec = write_spec(&cfg, "cap", &content);
        let sidecar = "version: 1\ntraces:\n- source: a\n  updated: d\n  added:\n  - Login Button\n  code: []\n";
        std::fs::write(sidecar_path(&spec), sidecar).unwrap();

        let report = migrate(&cfg, false).unwrap();

        assert_eq!(report.len(), 1);
        assert_eq!(report[0].footers, 0);
        assert_eq!(report[0].stale_names, vec!["Login Button"]);
        assert!(report[0].needs_attention());
        assert_eq!(std::fs::read_to_string(&spec).unwrap(), content);
        assert_eq!(
            std::fs::read_to_string(sidecar_path(&spec)).unwrap(),
            sidecar
        );
    }

    #[test]
    fn migrate_dry_run_writes_nothing() {
        let tmp = crate::test_support::TempDir::new("trace-migrate-dry");
        let cfg = migrate_cfg(&tmp);
        let spec = write_spec(&cfg, "cap", ORACLE_SPEC);

        let report = migrate(&cfg, true).unwrap();

        assert_eq!(report[0].footers, 2);
        assert_eq!(std::fs::read_to_string(&spec).unwrap(), ORACLE_SPEC);
        assert!(!sidecar_path(&spec).exists());
    }

    #[test]
    fn migrate_reports_a_corrupt_sidecar_and_leaves_the_spec_alone() {
        let tmp = crate::test_support::TempDir::new("trace-migrate-corrupt");
        let cfg = migrate_cfg(&tmp);
        let spec = write_spec(&cfg, "cap", ORACLE_SPEC);
        std::fs::write(sidecar_path(&spec), "traces: [").unwrap();

        let report = migrate(&cfg, false).unwrap();

        let error = report[0].error.as_deref().unwrap();
        assert!(error.contains("is not a valid trace sidecar"), "{error}");
        assert_eq!(std::fs::read_to_string(&spec).unwrap(), ORACLE_SPEC);
        assert_eq!(
            std::fs::read_to_string(sidecar_path(&spec)).unwrap(),
            "traces: ["
        );
    }

    #[test]
    fn load_rejects_a_corrupt_or_unknown_version_sidecar() {
        let dir = crate::test_support::TempDir::new("trace-load");
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
    }
}
