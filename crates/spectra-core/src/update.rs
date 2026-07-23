//! `spectra update` — 把偵測到的 AI 工具 instruction 檔更新到目前 schema
//! 版本。
//!
//! 行為逐項 probe 自 oracle 2.3.1（見 `docs/reverse-engineering/update.md`）：
//!
//! - 偵測：工具的偵測路徑（如 `.claude`、`.github/prompts`）**存在**即算
//!   （檔案也算，不限目錄——oracle 用 exists 而非 is_dir，偵測路徑是普通
//!   檔案時後續 create_dir_all 會炸原始 io error，這個 bug 也照搬）。
//! - 每次執行對每個偵測到的工具**無條件重寫**其整組檔案（內容 idempotent
//!   但一定寫檔）；缺檔補回；非管理檔（使用者自己的 skill 等）不動。
//! - `--force` 在 oracle 2.3.1 觀察不到任何行為差異（CLI 收下但語義同
//!   預設），這裡照樣只收不用。
//! - 模板中 `{{SPEC_DIR}}` 代換為 config 的 `spec_dir`；oracle 輸出本身
//!   含有漏代換的字面 `{{SPEC_DIR}}`（cursor spectra-ask 的 frontmatter），
//!   capture 時跳脫成 `{{RAW_SPEC_DIR}}`，render 時還原字面值。
//! - 根目錄的 marker 檔（`CLAUDE.md`、`.cursorrules` 等）以
//!   `<!-- SPECTRA:START … -->` / `<!-- SPECTRA:END -->` 區塊管理：
//!   區塊完整 → 原地替換（前後內容保留）；只有 START 沒 END → 整塊附加到
//!   檔尾；沒有 START（不管有沒有孤兒 END）→ 前置到檔首。
//! - `.claude/settings.json` 是 JSON 合併：管理鍵強制為管理值、使用者鍵
//!   保留、鍵按字母排序（oracle 即 serde_json 預設 BTreeMap 行為）、
//!   2 空格縮排、無結尾換行；無法解析成 JSON object 時整檔換成預設模板。

use anyhow::Result;
use std::path::Path;

use crate::config::Config;
use crate::update_manifest;

/// 模板中的 spec_dir 代換點。
const SPEC_DIR_PLACEHOLDER: &str = "{{SPEC_DIR}}";
/// 「oracle 輸出裡就是字面 `{{SPEC_DIR}}`」的跳脫形式（見 module doc）。
const RAW_SPEC_DIR_PLACEHOLDER: &str = "{{RAW_SPEC_DIR}}";

/// 管理區塊起點（版本尾碼會變，所以用前綴比對）。
const MARKER_START: &str = "<!-- SPECTRA:START";
/// 管理區塊終點。
const MARKER_END: &str = "<!-- SPECTRA:END -->";

/// 一個工具檔案的寫入策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// 整檔覆寫（skills、commands、prompts……）。
    Plain,
    /// 根目錄 marker 檔：只管理 START/END 區塊，區塊外內容保留。
    Managed,
    /// `.claude/settings.json`：JSON 物件合併。
    ClaudeSettings,
}

/// 工具檔案：寫到 `relpath`，內容由 `template` 經 spec_dir 代換而來。
pub struct FileSpec {
    pub relpath: &'static str,
    pub kind: FileKind,
    pub template: &'static str,
}

/// 一個 AI 工具：`detect_dir` 存在（exists，不限目錄）即偵測成立。
pub struct ToolDef {
    pub id: &'static str,
    pub detect_dir: &'static str,
    pub files: &'static [FileSpec],
}

/// 依 oracle registry 順序回傳偵測到的工具（訊息的排序就是這個順序）。
pub fn detect_tools(root: &Path) -> Vec<&'static ToolDef> {
    update_manifest::TOOLS
        .iter()
        .filter(|t| root.join(t.detect_dir).exists())
        .collect()
}

/// 更新所有偵測到的工具的 instruction 檔，回傳其 id（registry 順序）。
/// 空 Vec 表示沒偵測到任何工具（呼叫端印 "No AI tool configurations
/// found" 訊息）。
pub fn update_instruction_files(cfg: &Config) -> Result<Vec<&'static str>> {
    let tools = detect_tools(&cfg.root);
    // RE'd 怪癖（對 2.3.1 成對 probe，見 update.md）：gemini 同時被偵測到
    // 時，codex 只寫 AGENTS.md、整組 .agents/skills/* 被抑制。其他工具
    // 兩兩組合都互不影響（含同樣會寫 GEMINI.md 的 antigravity）。
    let gemini_present = tools.iter().any(|t| t.id == "gemini");
    for tool in &tools {
        for file in tool.files {
            if tool.id == "codex" && gemini_present && file.relpath != "AGENTS.md" {
                continue;
            }
            write_file(&cfg.root, file, &cfg.spec_dir)?;
        }
    }
    Ok(tools.iter().map(|t| t.id).collect())
}

fn write_file(root: &Path, file: &FileSpec, spec_dir: &str) -> Result<()> {
    let path = root.join(file.relpath);
    if let Some(parent) = path.parent() {
        // 不加 context：oracle 在偵測路徑是普通檔案時就是裸的
        // "File exists (os error 17)"（機率極低的 parity bug 保留）。
        std::fs::create_dir_all(parent)?;
    }
    let rendered = render(file.template, spec_dir);
    let content = match file.kind {
        FileKind::Plain => rendered,
        FileKind::Managed => {
            let existing = read_if_exists(&path)?;
            merge_managed_block(existing.as_deref(), &rendered)
        }
        FileKind::ClaudeSettings => {
            let existing = read_if_exists(&path)?;
            merge_settings(existing.as_deref(), &rendered)
        }
    };
    std::fs::write(&path, content)?;
    Ok(())
}

fn read_if_exists(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// 模板代換：先展開 `{{SPEC_DIR}}`，再把跳脫的 `{{RAW_SPEC_DIR}}` 還原成
/// 字面 `{{SPEC_DIR}}`（順序不可對調，否則還原出的字面值會被二次展開）。
fn render(template: &str, spec_dir: &str) -> String {
    template
        .replace(SPEC_DIR_PLACEHOLDER, spec_dir)
        .replace(RAW_SPEC_DIR_PLACEHOLDER, SPEC_DIR_PLACEHOLDER)
}

/// 找到「行首是 `needle`」的行，回傳該行的起始 byte offset。
/// 從 `from` 開始找；marker 必須在行首（縮排的不算——oracle 只在行首寫
/// marker，這裡採同樣的嚴格比對）。
fn find_marker_line(content: &str, needle: &str, from: usize) -> Option<usize> {
    let hay = &content[from..];
    if hay.starts_with(needle) && from == 0 {
        return Some(0);
    }
    // 之後的候選都必須緊跟在換行後。
    let mut search = 0;
    while let Some(pos) = hay[search..].find(needle) {
        let abs = search + pos;
        if abs == 0 && from == 0 {
            return Some(from);
        }
        if abs > 0 && hay.as_bytes()[abs - 1] == b'\n' {
            return Some(from + abs);
        }
        search = abs + needle.len();
    }
    None
}

/// 行尾（含換行）的 offset；最後一行沒換行就到 EOF。
fn line_end(content: &str, line_start: usize) -> usize {
    match content[line_start..].find('\n') {
        Some(off) => line_start + off + 1,
        None => content.len(),
    }
}

/// 管理區塊合併（marker 檔案的三種情況，probe 詳見 module doc）。
/// `block` 是完整的新區塊（START 起、END 止、含結尾換行）。
fn merge_managed_block(existing: Option<&str>, block: &str) -> String {
    let content = match existing {
        None => return block.to_string(),
        Some(c) => c,
    };
    match find_marker_line(content, MARKER_START, 0) {
        Some(start) => {
            match find_marker_line(content, MARKER_END, line_end(content, start)) {
                Some(end_start) => {
                    // 區塊完整：原地替換，前後內容保留。
                    let end = line_end(content, end_start);
                    format!("{}{}{}", &content[..start], block, &content[end..])
                }
                None => {
                    // 有 START 沒 END：整塊附加到檔尾（前面補到換行）。
                    let mut out = content.to_string();
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push('\n');
                    out.push_str(block);
                    out
                }
            }
        }
        None => {
            // 沒有 START：前置到檔首；空檔就只有區塊本身。
            if content.is_empty() {
                block.to_string()
            } else {
                format!("{block}\n{content}")
            }
        }
    }
}

/// `.claude/settings.json` 合併：管理鍵強制、使用者鍵保留、字母排序、
/// 2 空格縮排、無結尾換行。現有內容不是 JSON object（含解析失敗）→
/// 整檔換成模板。
fn merge_settings(existing: Option<&str>, template: &str) -> String {
    let managed: serde_json::Value =
        serde_json::from_str(template).expect("update settings template is valid JSON");
    let mut merged = match existing.and_then(|s| serde_json::from_str(s).ok()) {
        Some(serde_json::Value::Object(user)) => user,
        _ => serde_json::Map::new(),
    };
    if let serde_json::Value::Object(managed) = managed {
        for (k, v) in managed {
            merged.insert(k, v);
        }
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(merged))
        .expect("merged settings serialize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn block() -> String {
        "<!-- SPECTRA:START v1.0.2 -->\nBODY\n<!-- SPECTRA:END -->\n".to_string()
    }

    // ---- render ----

    #[test]
    fn render_substitutes_spec_dir_and_restores_raw_placeholder() {
        let out = render(
            "a {{SPEC_DIR}}/specs b {{RAW_SPEC_DIR}}documents",
            "docs/sp",
        );
        assert_eq!(out, "a docs/sp/specs b {{SPEC_DIR}}documents");
    }

    #[test]
    fn render_does_not_double_expand_a_spec_dir_shaped_spec_dir() {
        // 還原出的字面 {{SPEC_DIR}} 不能被二次展開。
        let out = render("{{RAW_SPEC_DIR}} and {{SPEC_DIR}}", "X");
        assert_eq!(out, "{{SPEC_DIR}} and X");
    }

    // ---- merge_managed_block（三種情況 + 邊界，全部 pin 自 oracle probe）----

    #[test]
    fn managed_missing_file_is_just_the_block() {
        assert_eq!(merge_managed_block(None, &block()), block());
    }

    #[test]
    fn managed_empty_file_is_just_the_block_without_extra_blank() {
        // oracle probe：空檔 update 後只有區塊本身（28 行、無尾端空行）。
        assert_eq!(merge_managed_block(Some(""), &block()), block());
    }

    #[test]
    fn managed_no_marker_prepends_block_with_one_blank_separator() {
        // oracle probe：無 marker 檔案 → 區塊、空行、原內容。
        let got = merge_managed_block(Some("P-line\nX-line\n"), &block());
        assert_eq!(got, format!("{}\nP-line\nX-line\n", block()));
    }

    #[test]
    fn managed_valid_pair_is_replaced_in_place_preserving_both_sides() {
        let existing = "before\n<!-- SPECTRA:START v0.9.0 -->\nOLD\n<!-- SPECTRA:END -->\nafter\n";
        let got = merge_managed_block(Some(existing), &block());
        assert_eq!(got, format!("before\n{}after\n", block()));
    }

    #[test]
    fn managed_start_without_end_appends_block_at_eof() {
        // oracle probe：P-line/START/X-line（無 END）→ 原內容原封不動，
        // 空行後接完整新區塊。
        let existing = "P-line\n<!-- SPECTRA:START v1.0.2 -->\nX-line\n";
        let got = merge_managed_block(Some(existing), &block());
        assert_eq!(got, format!("{existing}\n{}", block()));
    }

    #[test]
    fn managed_orphan_end_without_start_still_prepends() {
        // oracle probe：只有 END 沒 START → 當成無 marker，前置；孤兒 END
        // 留在原內容裡。
        let existing = "P-line\n<!-- SPECTRA:END -->\nX-line\n";
        let got = merge_managed_block(Some(existing), &block());
        assert_eq!(
            got,
            format!("{}\nP-line\n<!-- SPECTRA:END -->\nX-line\n", block())
        );
    }

    #[test]
    fn managed_end_before_start_does_not_count_as_a_pair() {
        // END 出現在 START 之前 → START 視為無配對，走附加路徑。
        let existing = "<!-- SPECTRA:END -->\n<!-- SPECTRA:START v1.0.2 -->\nX\n";
        let got = merge_managed_block(Some(existing), &block());
        assert_eq!(got, format!("{existing}\n{}", block()));
    }

    #[test]
    fn managed_indented_marker_is_not_recognized() {
        // marker 不在行首（縮排）→ 不算，照無 marker 前置。
        let existing = "  <!-- SPECTRA:START v1.0.2 -->\nX\n<!-- SPECTRA:END -->\n";
        let got = merge_managed_block(Some(existing), &block());
        assert!(got.starts_with(&block()));
        assert!(got.ends_with(existing));
    }

    #[test]
    fn managed_start_version_suffix_is_prefix_matched() {
        // 舊版本號（v0.9.0）的 START 也要被找到並整塊換新。
        let existing = "<!-- SPECTRA:START v0.9.0 -->\nOLD\n<!-- SPECTRA:END -->\n";
        assert_eq!(merge_managed_block(Some(existing), &block()), block());
    }

    #[test]
    fn managed_content_without_trailing_newline_gets_one_before_append() {
        let existing = "P\n<!-- SPECTRA:START v1.0.2 -->\nX";
        let got = merge_managed_block(Some(existing), &block());
        assert_eq!(got, format!("{existing}\n\n{}", block()));
    }

    // ---- merge_settings（pin 自 oracle probe）----

    const SETTINGS_TEMPLATE: &str = "{\n  \"includeGitInstructions\": false\n}";

    #[test]
    fn settings_missing_file_writes_the_template_verbatim() {
        assert_eq!(merge_settings(None, SETTINGS_TEMPLATE), SETTINGS_TEMPLATE);
    }

    #[test]
    fn settings_merge_forces_managed_key_keeps_user_keys_sorted() {
        // oracle probe：{"zeta":1,"alpha":{...},"mid":"x"} → 字母排序、
        // 管理鍵強制 false、巢狀值保留、無結尾換行。
        let got = merge_settings(
            Some(r#"{"zeta": 1, "alpha": {"nested": [1,2]}, "mid": "x"}"#),
            SETTINGS_TEMPLATE,
        );
        let expected = "{\n  \"alpha\": {\n    \"nested\": [\n      1,\n      2\n    ]\n  },\n  \"includeGitInstructions\": false,\n  \"mid\": \"x\",\n  \"zeta\": 1\n}";
        assert_eq!(got, expected);
    }

    #[test]
    fn settings_user_true_value_is_overwritten_to_managed_false() {
        let got = merge_settings(
            Some(r#"{"includeGitInstructions": true}"#),
            SETTINGS_TEMPLATE,
        );
        assert_eq!(got, SETTINGS_TEMPLATE);
    }

    #[test]
    fn settings_invalid_json_is_replaced_with_the_template() {
        assert_eq!(
            merge_settings(Some("not json{"), SETTINGS_TEMPLATE),
            SETTINGS_TEMPLATE
        );
    }

    #[test]
    fn settings_non_object_json_is_replaced_with_the_template() {
        assert_eq!(
            merge_settings(Some("[1,2]"), SETTINGS_TEMPLATE),
            SETTINGS_TEMPLATE
        );
    }

    // ---- detect_tools ----

    #[test]
    fn detect_reports_tools_in_registry_order_not_creation_order() {
        let tmp = TempDir::new("update-detect");
        for d in [".agents", ".cursor", ".claude"] {
            std::fs::create_dir_all(tmp.join(d)).unwrap();
        }
        let ids: Vec<_> = detect_tools(&tmp).iter().map(|t| t.id).collect();
        assert_eq!(ids, ["claude", "cursor", "codex"]);
    }

    #[test]
    fn detect_matches_a_plain_file_not_just_a_directory() {
        // oracle 用 exists 而非 is_dir：偵測路徑是普通檔案也算偵測到。
        let tmp = TempDir::new("update-detect-file");
        std::fs::write(tmp.join(".claude"), "").unwrap();
        let ids: Vec<_> = detect_tools(&tmp).iter().map(|t| t.id).collect();
        assert_eq!(ids, ["claude"]);
    }

    #[test]
    fn detect_github_copilot_needs_the_nested_prompts_dir() {
        let tmp = TempDir::new("update-detect-gh");
        std::fs::create_dir_all(tmp.join(".github")).unwrap();
        assert!(detect_tools(&tmp).is_empty());
        std::fs::create_dir_all(tmp.join(".github/prompts")).unwrap();
        let ids: Vec<_> = detect_tools(&tmp).iter().map(|t| t.id).collect();
        assert_eq!(ids, ["github-copilot"]);
    }

    #[test]
    fn registry_covers_all_twenty_three_probed_tools_in_oracle_order() {
        let ids: Vec<_> = update_manifest::TOOLS.iter().map(|t| t.id).collect();
        assert_eq!(
            ids,
            [
                "claude",
                "cursor",
                "windsurf",
                "cline",
                "gemini",
                "github-copilot",
                "kiro",
                "roocode",
                "continue",
                "opencode",
                "codebuddy",
                "costrict",
                "antigravity",
                "auggie",
                "amazon-q",
                "kilocode",
                "factory",
                "iflow",
                "qoder",
                "qwen",
                "codex",
                "crush",
                "trae"
            ]
        );
    }

    #[test]
    fn every_managed_template_is_a_complete_marker_block() {
        // Managed 模板必須 START 起、END（含換行）止——merge 演算法的前提。
        for tool in update_manifest::TOOLS {
            for file in tool.files {
                if file.kind == FileKind::Managed {
                    assert!(
                        file.template.starts_with(MARKER_START),
                        "{}:{} does not start with the START marker",
                        tool.id,
                        file.relpath
                    );
                    assert!(
                        file.template.ends_with(&format!("{MARKER_END}\n")),
                        "{}:{} does not end with the END marker",
                        tool.id,
                        file.relpath
                    );
                }
            }
        }
    }

    #[test]
    fn claude_settings_template_is_valid_json_and_only_claude_has_one() {
        let mut seen = Vec::new();
        for tool in update_manifest::TOOLS {
            for file in tool.files {
                if file.kind == FileKind::ClaudeSettings {
                    seen.push((tool.id, file.relpath));
                    assert!(serde_json::from_str::<serde_json::Value>(file.template).is_ok());
                }
            }
        }
        assert_eq!(seen, [("claude", ".claude/settings.json")]);
    }

    // ---- update_instruction_files（檔案系統整合層）----

    fn init_cfg(tmp: &TempDir) -> Config {
        Config {
            root: tmp.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
        }
    }

    #[test]
    fn update_with_no_tools_writes_nothing_and_returns_empty() {
        let tmp = TempDir::new("update-none");
        let ids = update_instruction_files(&init_cfg(&tmp)).unwrap();
        assert!(ids.is_empty());
        assert_eq!(std::fs::read_dir(&*tmp).unwrap().count(), 0);
    }

    #[test]
    fn update_claude_writes_the_full_file_set_and_reports_the_id() {
        let tmp = TempDir::new("update-claude");
        std::fs::create_dir_all(tmp.join(".claude")).unwrap();
        let ids = update_instruction_files(&init_cfg(&tmp)).unwrap();
        assert_eq!(ids, ["claude"]);
        for file in update_manifest::TOOLS[0].files {
            assert!(tmp.join(file.relpath).is_file(), "missing {}", file.relpath);
        }
    }

    #[test]
    fn update_is_idempotent_byte_for_byte() {
        let tmp = TempDir::new("update-idem");
        std::fs::create_dir_all(tmp.join(".claude")).unwrap();
        let cfg = init_cfg(&tmp);
        update_instruction_files(&cfg).unwrap();
        let first = std::fs::read_to_string(tmp.join("CLAUDE.md")).unwrap();
        update_instruction_files(&cfg).unwrap();
        let second = std::fs::read_to_string(tmp.join("CLAUDE.md")).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn update_restores_a_user_modified_plain_file() {
        let tmp = TempDir::new("update-restore");
        std::fs::create_dir_all(tmp.join(".claude")).unwrap();
        let cfg = init_cfg(&tmp);
        update_instruction_files(&cfg).unwrap();
        let skill = tmp.join(".claude/skills/spectra-drift/SKILL.md");
        let original = std::fs::read_to_string(&skill).unwrap();
        std::fs::write(&skill, "tampered").unwrap();
        update_instruction_files(&cfg).unwrap();
        assert_eq!(std::fs::read_to_string(&skill).unwrap(), original);
    }

    #[test]
    fn update_preserves_unmanaged_files_next_to_managed_ones() {
        let tmp = TempDir::new("update-unmanaged");
        std::fs::create_dir_all(tmp.join(".claude/skills/my-own")).unwrap();
        std::fs::write(tmp.join(".claude/skills/my-own/SKILL.md"), "mine\n").unwrap();
        update_instruction_files(&init_cfg(&tmp)).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.join(".claude/skills/my-own/SKILL.md")).unwrap(),
            "mine\n"
        );
    }

    #[test]
    fn update_substitutes_a_custom_spec_dir_into_templates() {
        let tmp = TempDir::new("update-specdir");
        std::fs::create_dir_all(tmp.join(".claude")).unwrap();
        let cfg = Config {
            root: tmp.to_path_buf(),
            spec_dir: "docs/specs".to_string(),
            locale: None,
        };
        update_instruction_files(&cfg).unwrap();
        let claude_md = std::fs::read_to_string(tmp.join("CLAUDE.md")).unwrap();
        assert!(claude_md.contains("`docs/specs/specs/`"));
        assert!(!claude_md.contains("openspec/"));
    }

    #[test]
    fn codex_skills_are_suppressed_when_gemini_is_also_detected() {
        // RE'd 怪癖：gemini + codex → codex 只寫 AGENTS.md。
        let tmp = TempDir::new("update-codex-gemini");
        std::fs::create_dir_all(tmp.join(".agents")).unwrap();
        std::fs::create_dir_all(tmp.join(".gemini")).unwrap();
        let ids = update_instruction_files(&init_cfg(&tmp)).unwrap();
        assert_eq!(ids, ["gemini", "codex"]);
        assert!(tmp.join("AGENTS.md").is_file());
        assert!(!tmp.join(".agents/skills").exists());
        // gemini 自己的檔案不受影響。
        assert!(tmp.join(".gemini/skills/spectra-apply/SKILL.md").is_file());
    }

    #[test]
    fn codex_skills_are_written_when_gemini_is_absent() {
        // 對照組：其他工具（含同樣寫 GEMINI.md 的 antigravity）不觸發抑制。
        let tmp = TempDir::new("update-codex-antigrav");
        std::fs::create_dir_all(tmp.join(".agents")).unwrap();
        std::fs::create_dir_all(tmp.join(".agent")).unwrap();
        let ids = update_instruction_files(&init_cfg(&tmp)).unwrap();
        assert_eq!(ids, ["antigravity", "codex"]);
        assert!(tmp.join(".agents/skills/spectra-apply/SKILL.md").is_file());
    }

    #[test]
    fn update_errors_with_the_raw_io_error_when_detect_path_is_a_file() {
        // oracle parity：偵測路徑是普通檔案 → create_dir_all 的裸 io error
        // （"File exists (os error 17)"，anyhow {:#} 下不能帶 context 前綴）。
        let tmp = TempDir::new("update-detfile");
        std::fs::write(tmp.join(".claude"), "").unwrap();
        let err = update_instruction_files(&init_cfg(&tmp)).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("os error 17") || msg.contains("os error 20"),
            "unexpected error text: {msg}"
        );
        assert!(
            !msg.contains(':') || !msg.contains("creating"),
            "context leaked: {msg}"
        );
    }
}
