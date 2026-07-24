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
//!   含有漏代換的字面 `{{SPEC_DIR}}`（每個工具的 spectra-ask 命令檔，
//!   445 個 tool-file 中有 19 個、去重後 9 個 blob），capture 時跳脫成
//!   `{{RAW_SPEC_DIR}}`，render 時還原字面值。
//! - [`FileKind`] 是**對 oracle 實測**分類的（sentinel 存活法），不是從模板
//!   文字推論：kilocode 的 10 個 `.kilocode/workflows/*.md` 模板本身是完整
//!   marker 區塊，oracle 卻整檔覆寫。
//! - marker 檔（`CLAUDE.md`、`.cursorrules`、`AGENTS.md` 等 11 個路徑）以
//!   `<!-- SPECTRA:START … -->` / `<!-- SPECTRA:END -->` 區塊管理：
//!   區塊完整 → 原地替換（marker 之外的內容保留，**含同行前後綴**）；
//!   只有 START 沒 END → 整塊附加到檔尾；沒有 START（不管有沒有孤兒 END）
//!   → 前置到檔首。詳細替換區間見 [`merge_managed_block`]。
//! - 既有檔讀不成 UTF-8 → 當作不存在（整份丟棄重寫），與 oracle 一致。
//! - `.claude/settings.json` 是 JSON 合併：管理鍵強制為管理值、使用者鍵
//!   保留、鍵按字母排序（oracle 即 serde_json 預設 BTreeMap 行為）、
//!   2 空格縮排、無結尾換行；無法解析成 JSON object 時整檔換成預設模板。

use anyhow::{Context, Result};
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
    /// 整檔覆寫（skills、commands、prompts，以及 kilocode 的 workflows——
    /// 後者的模板看起來是 marker 區塊，但 oracle 實測是整檔覆寫）。
    /// oracle 的做法是 unlink + 重建，故不跟隨 symlink、且能換掉唯讀檔。
    Plain,
    /// marker 檔：只管理 START/END 區塊，區塊外內容保留。
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

/// 寫出單一工具檔案。
///
/// **這條路徑上的 I/O 錯誤一律不加 `.with_context`**：AC-2 要求錯誤字串與
/// oracle 逐字一致，而 oracle 印的是裸的 `Error: Permission denied (os error
/// 13)`（唯讀父目錄實測）。crate 其他地方的慣例是附上路徑，那是給沒有
/// oracle 字串可對齊的指令用的；此處對齊 oracle 優先。
///
/// 這一點推翻了 PR #86 第一輪 review 接受的一個 finding（「`fs::write` 應比照
/// 5 個 sibling call site 附加路徑 context」）——第二輪 Codex 指出加了 context
/// 就不再等於 oracle 的輸出，實測確認：加 context 後我們印
/// `Error: removing <path>: Permission denied (os error 13)`，oracle 印
/// `Error: Permission denied (os error 13)`。
fn write_file(root: &Path, file: &FileSpec, spec_dir: &str) -> Result<()> {
    let path = root.join(file.relpath);
    if let Some(parent) = path.parent() {
        // oracle 在偵測路徑是普通檔案時就是裸的 "File exists (os error 17)"。
        std::fs::create_dir_all(parent)?;
    }
    let rendered = render(file.template, spec_dir);
    match file.kind {
        // Plain：oracle 實測是 unlink + 重建（連可寫檔的 inode 都會變），
        // 這帶來兩個可觀察行為，兩者都要照做：唯讀既有檔會被成功換掉、
        // 而 symlink **不會**被跟隨（link 本身被移除，指向的外部檔案毫髮無傷）。
        // 先前用 `fs::write` 同時錯失這兩點——唯讀檔讓整個 run exit 1，
        // symlink 則被寫穿而覆寫專案外檔案。
        FileKind::Plain => {
            remove_if_present(&path)?;
            std::fs::write(&path, rendered)?;
        }
        // Managed / ClaudeSettings：讀取既有內容再合併，屬 read-modify-write。
        // oracle 這兩類是就地寫入（會跟隨 symlink）；本 repo 對這個取捨已有
        // 明文裁定——`artifact.rs` 的
        // `force_write_through_a_symlinked_artifact_path_cannot_escape_the_change_dir`
        // 記載「oracle 跟隨 link 是共有的漏洞」，openspectra 一律改用
        // temp + atomic rename。故此處**刻意偏離 oracle**，換得兩件事：
        // 使用者的 CLAUDE.md 不會因中途中斷而被截斷成空檔，且 symlink 不會
        // 被寫穿。副作用是唯讀的 Managed 檔在我們這邊會成功、oracle 則
        // exit 1——差異已記錄於 update.md。
        FileKind::Managed => {
            let existing = read_existing(&path)?;
            let content = merge_managed_block(existing.as_deref(), &rendered);
            crate::fsutil::write_atomically(&path, &content)?;
        }
        FileKind::ClaudeSettings => {
            let existing = read_existing(&path)?;
            let content = merge_settings(existing.as_deref(), &rendered);
            crate::fsutil::write_atomically(&path, &content)?;
        }
    }
    Ok(())
}

/// 移除既有檔案；不存在就當作成功。symlink 也是移除 link 本身
/// （`remove_file` 不跟隨），這正是 Plain 路徑要的語意。
///
/// 失敗時回裸的 io error，不加路徑 context——理由見 [`write_file`] 上方
/// 關於錯誤字串的說明。
fn remove_if_present(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// 讀取既有內容供合併使用。**讀不成 UTF-8 就當作檔案不存在**——這是對
/// oracle 實測的結果：非 UTF-8 的 `CLAUDE.md`（即使內含合法 marker 配對）
/// 會被整份丟棄、直接寫入全新區塊，既不是 lossy 轉碼也不是保留原位元組。
/// 先前用 `read_to_string` 直接 `?`，讓一個 latin-1 的既有檔把整個
/// 445 檔的 run 打斷成 exit 1 + 部分寫入（17 檔只寫出 4 檔）。
///
/// 真正的 I/O 失敗（權限等）仍然往上拋，不與「讀不懂」混為一談。
fn read_existing(path: &Path) -> Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(String::from_utf8(bytes).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// 模板代換：先展開 `{{SPEC_DIR}}`，再把跳脫的 `{{RAW_SPEC_DIR}}` 還原成
/// 字面 `{{SPEC_DIR}}`（順序不可對調，否則還原出的字面值會被二次展開）。
fn render(template: &str, spec_dir: &str) -> String {
    template
        .replace(SPEC_DIR_PLACEHOLDER, spec_dir)
        .replace(RAW_SPEC_DIR_PLACEHOLDER, SPEC_DIR_PLACEHOLDER)
}

/// 管理區塊合併。
///
/// 被替換的區間是 **純子字串接合，沒有任何行錨定**——這是對 oracle 2.3.1
/// 用 sentinel 實測刻畫出來的（`docs/reverse-engineering/update.md` 的
/// 「Replacement region」一節列出全部觀察）：
///
/// ```text
/// replace [ 字面 MARKER_START 的 byte offset ,
///           字面 MARKER_END 結束的 byte offset（下一個 byte 是 '\n' 就再 +1） ]
/// ```
///
/// 三個後果都經實測確認，且都與「行錨定」直覺相反：
/// - marker 同行、位於 START **之前**的內容保留（縮排、前綴文字皆然）；
/// - END **之後**的同行尾隨內容保留（早期版本會把它連同整行刪掉）；
/// - 尾端的 `+1 if '\n'` 正是 CRLF 檔案殘留一個 `\r\n` 的成因：`END -->`
///   後面接的是 `\r` 而非 `\n`，所以不吃掉，該 `\r\n` 留在原地。
///
/// PR #86 review 之前這裡用的是行錨定 + 整行替換，造成四個 oracle 分歧，
/// 其中兩個會**刪掉使用者內容**（空 body 的區塊、END 同行尾隨內容）。
///
/// `block` 是完整的新區塊（START 起、END 止、含結尾換行）。
fn merge_managed_block(existing: Option<&str>, block: &str) -> String {
    let content = match existing {
        None => return block.to_string(),
        Some(c) => c,
    };
    match content.find(MARKER_START) {
        Some(start) => {
            match content[start..].find(MARKER_END) {
                Some(rel) => {
                    // 區塊完整：原地替換，marker 之外的內容（含同行前後綴）保留。
                    let mut end = start + rel + MARKER_END.len();
                    if content[end..].starts_with('\n') {
                        end += 1;
                    }
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
    fn managed_orphan_start_swallows_content_on_the_second_run_matching_the_oracle() {
        // 這不是 bug，是 **parity**：孤兒 START 在 run 1 之後，新附加區塊的 END
        // 會與那個孤兒 START 配成一對，於是 run 2 把兩者之間的使用者內容吃掉。
        // 對 oracle 2.3.1 實測兩次執行皆逐位元相同（BEFORE 留、INSIDE 消失）。
        //
        // 釘住它的理由：先前只測到 run 1，未來若有人「順手把 append 路徑改成
        // 收斂」，就會在全套測試綠燈的情況下偏離 oracle。這個測試讓那種改動
        // 必須是有意識的決定。
        let existing = "MY-BEFORE\n<!-- SPECTRA:START v0.0.1 -->\nMY-INSIDE\n";
        let run1 = merge_managed_block(Some(existing), &block());
        assert!(run1.contains("MY-BEFORE"));
        assert!(run1.contains("MY-INSIDE"), "run 1 preserves user content");

        let run2 = merge_managed_block(Some(&run1), &block());
        assert!(run2.contains("MY-BEFORE"));
        assert!(
            !run2.contains("MY-INSIDE"),
            "run 2 swallows content between the orphan START and the appended END \
             -- oracle-verified parity, do not 'fix' without re-probing"
        );
        // run 3 起才是固定點。
        assert_eq!(merge_managed_block(Some(&run2), &block()), run2);
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

    // 以下 6 個 case 全部 pin 自對 oracle 2.3.1 的差分實測。PR #86 review 之前
    // 這裡只有一個 `managed_indented_marker_is_not_recognized`，斷言的是與
    // oracle **相反**的行為（縮排 marker 不算），等於用一個綠燈測試把 parity
    // bug 鎖住。

    #[test]
    fn managed_indented_marker_is_recognized_and_its_indent_survives() {
        // oracle：marker 不需在行首；replacement 從 marker 的 byte offset 起算，
        // 所以同行且位於 marker 之前的內容（這裡的兩個空格）保留。
        let existing = "  <!-- SPECTRA:START v1.0.2 -->\nX\n<!-- SPECTRA:END -->\n";
        assert_eq!(
            merge_managed_block(Some(existing), &block()),
            format!("  {}", block())
        );
    }

    #[test]
    fn managed_empty_body_pair_is_replaced_in_place_not_appended() {
        // 迴歸：END 緊接在 START 下一行時，舊的行錨定搜尋找不到 END，於是把
        // 完整配對誤判成「有 START 沒 END」→ 附加第二個區塊，下一次執行再把
        // 兩個區塊之間的使用者內容一併吃掉。四個 reviewer 各自獨立指出。
        let existing =
            "USER BEFORE\n<!-- SPECTRA:START v1.0.2 -->\n<!-- SPECTRA:END -->\nUSER AFTER\n";
        let got = merge_managed_block(Some(existing), &block());
        assert_eq!(got, format!("USER BEFORE\n{}USER AFTER\n", block()));
        // 且必須是固定點：再跑一次不得改變任何位元組。
        assert_eq!(merge_managed_block(Some(&got), &block()), got);
    }

    #[test]
    fn managed_trailing_text_after_the_end_marker_survives() {
        // oracle 只吃到 END marker 文字結束（外加緊接的一個 '\n'），
        // 同行尾隨內容保留；舊實作連整行一起刪，屬使用者資料遺失。
        let existing = "<!-- SPECTRA:START v1.0.2 -->\nOLD\n<!-- SPECTRA:END --> trailing\ntail\n";
        assert_eq!(
            merge_managed_block(Some(existing), &block()),
            format!("{} trailing\ntail\n", block())
        );
    }

    #[test]
    fn managed_text_before_the_start_marker_on_the_same_line_survives() {
        let existing = "PREFIX <!-- SPECTRA:START v1.0.2 -->\nOLD\n<!-- SPECTRA:END -->\nZ\n";
        assert_eq!(
            merge_managed_block(Some(existing), &block()),
            format!("PREFIX {}Z\n", block())
        );
    }

    #[test]
    fn managed_crlf_file_keeps_the_carriage_return_the_oracle_leaves_behind() {
        // `+1 if next byte is '\n'` 的直接後果：CRLF 檔案 END 之後是 '\r'，
        // 不吃掉，所以殘留一個 "\r\n"。這是 oracle 的行為，逐位元照抄。
        let existing =
            "P\r\n<!-- SPECTRA:START v1.0.2 -->\r\nOLD\r\n<!-- SPECTRA:END -->\r\nafter\r\n";
        assert_eq!(
            merge_managed_block(Some(existing), &block()),
            format!("P\r\n{}\r\nafter\r\n", block())
        );
    }

    #[test]
    fn managed_replacement_is_not_line_anchored_at_all() {
        // 極端形狀：START 與 END 同在一行、兩側都有文字。
        let existing = "AAA <!-- SPECTRA:START v1 --> MID <!-- SPECTRA:END --> ZZZ\n";
        assert_eq!(
            merge_managed_block(Some(existing), &block()),
            format!("AAA {} ZZZ\n", block())
        );
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
    fn file_kinds_match_the_probed_oracle_classification() {
        // 這些數字是**對 oracle 實測**（sentinel 存活法）得到的，不是從模板
        // 文字推的。舊版把「模板以 START marker 開頭」直接當成 Managed，於是
        // 把 10 個 kilocode workflow 誤判成 Managed（oracle 其實整檔覆寫）。
        // 這個測試同時堵住 reviewer 指出的循環斷言問題：
        // `every_managed_template_is_a_complete_marker_block` 斷言的正是產生器
        // 用來分類的那個述詞，永遠不可能紅。
        let mut managed = Vec::new();
        let mut settings = 0;
        let mut plain_but_marker_shaped = Vec::new();
        for tool in update_manifest::TOOLS {
            for file in tool.files {
                match file.kind {
                    FileKind::Managed => managed.push(file.relpath),
                    FileKind::ClaudeSettings => settings += 1,
                    FileKind::Plain => {
                        if file.template.starts_with(MARKER_START) {
                            plain_but_marker_shaped.push(file.relpath);
                        }
                    }
                }
            }
        }
        managed.sort_unstable();
        managed.dedup();
        assert_eq!(
            managed,
            [
                ".cursorrules",
                ".windsurfrules",
                "AGENTS.md",
                "CLAUDE.md",
                "CLINE.md",
                "CODEBUDDY.md",
                "COSTRICT.md",
                "GEMINI.md",
                "IFLOW.md",
                "QODER.md",
                "QWEN.md"
            ],
            "Managed set drifted from the probed oracle classification"
        );
        assert_eq!(settings, 1);
        plain_but_marker_shaped.sort_unstable();
        plain_but_marker_shaped.dedup();
        assert_eq!(
            plain_but_marker_shaped.len(),
            10,
            "expected exactly kilocode's 10 workflow files to look managed but be \
             full-overwrite; got {plain_but_marker_shaped:?}"
        );
        assert!(
            plain_but_marker_shaped
                .iter()
                .all(|p| p.starts_with(".kilocode/workflows/")),
            "{plain_but_marker_shaped:?}"
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
    fn update_does_not_write_through_a_symlinked_plain_path() {
        // Plain 路徑：oracle 本身就是 unlink+recreate（實測），所以既符合
        // parity 也符合本 repo 的安全 baseline——link 被換成一般檔，指向的
        // 專案外檔案毫髮無傷。舊版的 `fs::write` 會寫穿。
        let tmp = TempDir::new("update-symlink-plain");
        std::fs::create_dir_all(tmp.join(".claude/skills/spectra-drift")).unwrap();
        let outside = tmp.join("outside-secret.txt");
        std::fs::write(&outside, "PRECIOUS").unwrap();
        let victim = tmp.join(".claude/skills/spectra-drift/SKILL.md");
        std::os::unix::fs::symlink(&outside, &victim).unwrap();

        update_instruction_files(&init_cfg(&tmp)).unwrap();

        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "PRECIOUS");
        assert!(!victim.symlink_metadata().unwrap().file_type().is_symlink());
        assert!(std::fs::read_to_string(&victim)
            .unwrap()
            .contains("spectra-drift"));
    }

    #[test]
    fn update_does_not_write_through_a_symlinked_managed_path() {
        // Managed 路徑：oracle **會**寫穿 symlink，這裡刻意偏離——理由與
        // `artifact.rs` 的同名 baseline 一致（見 fsutil::write_atomically 的
        // doc）。這個測試就是那個裁定在 `update` 上的釘子。
        let tmp = TempDir::new("update-symlink-managed");
        std::fs::create_dir_all(tmp.join(".claude")).unwrap();
        let outside = tmp.join("outside-notes.md");
        std::fs::write(&outside, "PRECIOUS").unwrap();
        std::os::unix::fs::symlink(&outside, tmp.join("CLAUDE.md")).unwrap();

        update_instruction_files(&init_cfg(&tmp)).unwrap();

        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "PRECIOUS");
        let claude_md = tmp.join("CLAUDE.md");
        assert!(!claude_md
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(std::fs::read_to_string(&claude_md)
            .unwrap()
            .starts_with(MARKER_START));
    }

    #[test]
    fn update_treats_a_non_utf8_existing_file_as_absent_instead_of_aborting() {
        // oracle 實測：非 UTF-8 的既有檔（即使含合法 marker 配對）整份丟棄、
        // 寫入全新區塊。舊版用 read_to_string + `?`，一個 latin-1 檔就讓整個
        // 445 檔的 run 變成 exit 1 + 部分寫入。
        let tmp = TempDir::new("update-non-utf8");
        std::fs::create_dir_all(tmp.join(".claude")).unwrap();
        std::fs::write(tmp.join("CLAUDE.md"), b"caf\xe9 notes\n").unwrap();

        update_instruction_files(&init_cfg(&tmp)).unwrap();

        let claude_md = std::fs::read_to_string(tmp.join("CLAUDE.md")).unwrap();
        assert!(claude_md.starts_with(MARKER_START));
        assert!(
            !claude_md.contains("notes"),
            "non-UTF-8 content must be discarded"
        );
        // 其餘檔案照樣寫完，不因這一個檔中止。
        assert!(tmp.join(".claude/skills/spectra-drift/SKILL.md").is_file());
    }

    #[test]
    fn update_replaces_a_read_only_plain_file_like_the_oracle_does() {
        // oracle 的 Plain 寫入是 unlink+recreate，所以 0400 的既有檔會被成功
        // 換掉（unlink 只需要目錄權限）。舊版的 fs::write 在這裡 exit 1。
        let tmp = TempDir::new("update-readonly-plain");
        std::fs::create_dir_all(tmp.join(".claude/skills/spectra-drift")).unwrap();
        let locked = tmp.join(".claude/skills/spectra-drift/SKILL.md");
        // sentinel 必須是模板裡不會出現的字串。第一版用 "locked" 而模板含
        // "blocked"，斷言因此為了錯的理由而紅——測試自身的 bug，不是產品碼的。
        std::fs::write(&locked, "ZZ_READONLY_SENTINEL_ZZ\n").unwrap();
        let mut perms = std::fs::metadata(&locked).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o400);
        std::fs::set_permissions(&locked, perms).unwrap();

        update_instruction_files(&init_cfg(&tmp)).unwrap();

        let text = std::fs::read_to_string(&locked).unwrap();
        assert!(
            !text.contains("ZZ_READONLY_SENTINEL_ZZ"),
            "read-only file must be replaced"
        );
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
