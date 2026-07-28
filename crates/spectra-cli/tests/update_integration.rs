//! `spectra update` 整合測試 — 訊息、exit code、寫檔集合與檔案位元組
//! 全部對照 oracle 2.3.1 的 probe / golden
//! （docs/reverse-engineering/update.md、golden/update-trees-2.3.1.tsv）。

mod common;

use std::path::Path;
use std::process::Output;

use common::{spectra, TempDir};
use sha2::{Digest, Sha256};

/// 建一個最小已初始化專案（`.spectra.yaml` 是唯一的判定檔）。
fn init_root(root: &Path, spec_dir: &str) {
    std::fs::write(
        root.join(".spectra.yaml"),
        format!("spec_dir: {spec_dir}\n"),
    )
    .unwrap();
}

fn enable_claude_slash_commands(root: &Path) {
    let config = root.join(".spectra.yaml");
    let mut text = std::fs::read_to_string(&config).unwrap();
    text.push_str("claude_slash_commands: true\n");
    std::fs::write(config, text).unwrap();
}

fn run_update(root: &Path, extra: &[&str]) -> Output {
    spectra()
        .arg("update")
        .arg(root)
        .args(extra)
        .arg("--no-color")
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

// ---- 訊息與 exit code（逐字 pin 自 oracle）----

#[test]
fn fresh_project_without_tools_reports_and_exits_zero() {
    let root = TempDir::new("update-notools");
    init_root(&root, "openspec");

    let out = run_update(&root, &[]);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out),
        "! No AI tool configurations found. Use 'spectra init --tools' to set up.\n"
    );
    assert!(out.stderr.is_empty());
    // 沒偵測到工具就什麼都不寫。
    assert_eq!(std::fs::read_dir(&*root).unwrap().count(), 1);
}

#[test]
fn uninitialized_path_errors_like_every_other_command() {
    let root = TempDir::new("update-uninit");

    let out = run_update(&root, &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stdout(&out).is_empty());
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: Not initialized. Run 'spectra init' first.\n"
    );
}

#[test]
fn explicit_path_does_not_walk_up_but_cwd_does() {
    // oracle probe：無 [PATH] 從 cwd 往上找專案根；明確 [PATH] 必須正好是
    // 根，子目錄直接 Not initialized。
    let root = TempDir::new("update-walkup");
    init_root(&root, "openspec");
    std::fs::create_dir_all(root.join("sub/inner")).unwrap();
    std::fs::create_dir_all(root.join(".claude")).unwrap();

    let from_sub = spectra()
        .args(["update", "--no-color"])
        .current_dir(root.join("sub/inner"))
        .output()
        .unwrap();
    assert!(
        from_sub.status.success(),
        "cwd walk-up failed: {from_sub:?}"
    );
    assert_eq!(
        String::from_utf8(from_sub.stdout).unwrap(),
        "✓ Updated instruction files for: claude\n"
    );

    let explicit_sub = run_update(&root.join("sub"), &[]);
    assert_eq!(explicit_sub.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(explicit_sub.stderr).unwrap(),
        "Error: Not initialized. Run 'spectra init' first.\n"
    );
}

#[test]
fn multi_tool_message_lists_ids_in_registry_order() {
    let root = TempDir::new("update-multi");
    init_root(&root, "openspec");
    // 建立順序故意反著來：訊息仍要照 registry 順序。
    for d in [".agents", ".cursor", ".claude", ".gemini"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }

    let out = run_update(&root, &[]);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out),
        "✓ Updated instruction files for: claude, cursor, gemini, codex\n"
    );
    assert!(out.stderr.is_empty(), "unexpected stderr: {out:?}");
}

#[test]
fn an_unwritable_target_reports_the_oracle_error_verbatim() {
    // AC-2 要求錯誤字串逐字對齊：oracle 對唯讀父目錄印裸的
    // `Error: Permission denied (os error 13)`，不含路徑。第一輪 review 曾
    // 要求比照 crate 慣例加上路徑 context，第二輪實測發現那樣就不再等於
    // oracle 的輸出——這個測試釘住實測的那一版。
    let root = TempDir::new("update-unwritable");
    init_root(&root, "openspec");
    let locked_dir = root.join(".claude/skills/spectra-drift");
    std::fs::create_dir_all(&locked_dir).unwrap();
    std::fs::write(locked_dir.join("SKILL.md"), "x\n").unwrap();
    let mut perms = std::fs::metadata(&locked_dir).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o500);
    std::fs::set_permissions(&locked_dir, perms.clone()).unwrap();

    let out = run_update(&root, &[]);

    // 還原權限，否則 TempDir 的 drop 清不掉。
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(&locked_dir, perms).unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: Permission denied (os error 13)\n"
    );
}

// ---- 寫檔集合與位元組（對 golden TSV）----

#[derive(Debug)]
struct GoldenRow {
    tool: String,
    relpath: String,
    sha256: String,
    gate: String,
}

/// golden TSV：tool → relpath / sha256 / gate，capture 自 oracle 實際輸出。
fn golden_rows() -> Vec<GoldenRow> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/reverse-engineering/golden/update-trees-2.3.1.tsv");
    std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| {
            let mut it = l.split('\t');
            let row = GoldenRow {
                tool: it.next().unwrap().to_string(),
                relpath: it.next().unwrap().to_string(),
                sha256: it.next().unwrap().to_string(),
                // 第四欄為必填：TSV 已於 capture 重跑時全面帶上 gate 欄。
                // 不留 "Always" fallback —— 缺欄的資料列必須大聲失敗，否則
                // 一列漏了 gate 會被靜默當成 Always 而通過（PR #102 review，
                // Codex）。
                gate: it
                    .next()
                    .unwrap_or_else(|| panic!("golden row missing gate column: {l}"))
                    .to_string(),
            };
            assert!(
                matches!(row.gate.as_str(), "Always" | "ClaudeSlashCommands"),
                "unknown gate in golden row: {row:?}"
            );
            assert!(it.next().is_none(), "extra columns in golden row: {row:?}");
            row
        })
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// 每個工具單獨跑一次 update，寫出的「每個檔案」都必須 hash-match oracle
/// 的 golden，且不能多寫或少寫任何檔。這是 CI 端不需要 oracle binary 的
/// 逐位元 parity 驗證。
#[test]
fn every_tool_tree_matches_the_oracle_golden_byte_for_byte() {
    let rows = golden_rows();
    assert_eq!(
        rows.len(),
        455,
        "golden TSV still needs the reviewer-only capture rerun (445 existing + \
         10 gated Claude command rows)"
    );
    let mut tools: Vec<String> = Vec::new();
    for row in &rows {
        if !tools.contains(&row.tool) {
            tools.push(row.tool.clone());
        }
    }
    assert_eq!(tools.len(), 23, "golden TSV tool count drifted");

    for tool in &tools {
        let detect_dir = match tool.as_str() {
            "claude" => ".claude",
            "cursor" => ".cursor",
            "windsurf" => ".windsurf",
            "cline" => ".clinerules",
            "gemini" => ".gemini",
            "github-copilot" => ".github/prompts",
            "kiro" => ".kiro",
            "roocode" => ".roo",
            "continue" => ".continue",
            "opencode" => ".opencode",
            "codebuddy" => ".codebuddy",
            "costrict" => ".cospec",
            "antigravity" => ".agent",
            "auggie" => ".augment",
            "amazon-q" => ".amazonq",
            "kilocode" => ".kilocode",
            "factory" => ".factory",
            "iflow" => ".iflow",
            "qoder" => ".qoder",
            "qwen" => ".qwen",
            "codex" => ".agents",
            "crush" => ".crush",
            "trae" => ".trae",
            other => panic!("unknown tool in golden TSV: {other}"),
        };
        for slash_commands in [false, true] {
            let switch = if slash_commands { "on" } else { "off" };
            let root = TempDir::new(&format!("update-golden-{tool}-{switch}"));
            init_root(&root, "openspec");
            if slash_commands {
                enable_claude_slash_commands(&root);
            }
            std::fs::create_dir_all(root.join(detect_dir)).unwrap();

            let out = run_update(&root, &[]);
            assert!(
                out.status.success(),
                "{tool}/{switch}: update failed: {out:?}"
            );
            assert_eq!(
                stdout(&out),
                format!("✓ Updated instruction files for: {tool}\n")
            );
            // stderr parity：oracle 成功時 stderr 全空。少了這個斷言，一行
            // `eprintln!` 可以在全套測試綠燈下混進去（PR #86 round-2, Codex 以
            // mutation 示範）。
            assert!(
                out.stderr.is_empty(),
                "{tool}/{switch}: unexpected stderr: {out:?}"
            );

            let expected: Vec<(&str, &str)> = rows
                .iter()
                .filter(|row| row.tool == *tool && (slash_commands || row.gate == "Always"))
                .map(|row| (row.relpath.as_str(), row.sha256.as_str()))
                .collect();
            for (rel, sha) in &expected {
                let bytes = std::fs::read(root.join(rel))
                    .unwrap_or_else(|e| panic!("{tool}/{switch}: missing {rel}: {e}"));
                assert_eq!(
                    &sha256_hex(&bytes),
                    sha,
                    "{tool}/{switch}: {rel} bytes drifted"
                );
            }

            // 反向：不能多寫 golden 以外的檔案。
            let mut written = Vec::new();
            collect_files(&root, &root, &mut written);
            written.retain(|p| p != ".spectra.yaml");
            assert_eq!(
                written.len(),
                expected.len(),
                "{tool}/{switch}: extra files written: {written:?}"
            );
        }
    }
}

fn collect_files(base: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_files(base, &path, out);
        } else {
            out.push(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

// ---- Issue #55 驗收情境：無 --force / --force / 連跑兩次 ----

#[test]
fn existing_files_are_overwritten_even_without_force() {
    // oracle probe：update 對管理檔一律重寫，--force 與否無差別。
    let root = TempDir::new("update-noforce");
    init_root(&root, "openspec");
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    assert!(run_update(&root, &[]).status.success());

    let skill = root.join(".claude/skills/spectra-drift/SKILL.md");
    let original = std::fs::read_to_string(&skill).unwrap();
    std::fs::write(&skill, "tampered").unwrap();

    assert!(run_update(&root, &[]).status.success());
    assert_eq!(std::fs::read_to_string(&skill).unwrap(), original);
}

#[test]
fn force_flag_is_accepted_and_behaves_identically() {
    let root = TempDir::new("update-force");
    init_root(&root, "openspec");
    std::fs::create_dir_all(root.join(".claude")).unwrap();

    let out = run_update(&root, &["--force"]);
    assert!(out.status.success());
    assert_eq!(stdout(&out), "✓ Updated instruction files for: claude\n");

    // 與無 --force 的輸出樹完全一致（逐檔比對）。
    let plain = TempDir::new("update-force-ctl");
    init_root(&plain, "openspec");
    std::fs::create_dir_all(plain.join(".claude")).unwrap();
    assert!(run_update(&plain, &[]).status.success());
    let mut files = Vec::new();
    collect_files(&root, &root, &mut files);
    for rel in files.iter().filter(|p| *p != ".spectra.yaml") {
        assert_eq!(
            std::fs::read(root.join(rel)).unwrap(),
            std::fs::read(plain.join(rel)).unwrap(),
            "{rel} differs between --force and default"
        );
    }
}

#[test]
fn running_twice_is_byte_for_byte_idempotent() {
    let root = TempDir::new("update-twice");
    init_root(&root, "openspec");
    std::fs::create_dir_all(root.join(".claude")).unwrap();

    let first = run_update(&root, &[]);
    assert!(first.status.success());
    let mut files = Vec::new();
    collect_files(&root, &root, &mut files);
    let snapshot: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|rel| (rel.clone(), std::fs::read(root.join(rel)).unwrap()))
        .collect();

    let second = run_update(&root, &[]);
    assert!(second.status.success());
    assert_eq!(stdout(&first), stdout(&second));
    for (rel, bytes) in &snapshot {
        assert_eq!(
            &std::fs::read(root.join(rel)).unwrap(),
            bytes,
            "{rel} changed on the second run"
        );
    }
}

// ---- 使用者內容保留與 codex-gemini 怪癖（end-to-end）----

#[test]
fn user_content_outside_managed_regions_survives_an_update() {
    let root = TempDir::new("update-user-content");
    init_root(&root, "openspec");
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(root.join("CLAUDE.md"), "My own notes\n").unwrap();
    std::fs::write(
        root.join(".claude/settings.json"),
        r#"{"userKey": true, "includeGitInstructions": true}"#,
    )
    .unwrap();

    assert!(run_update(&root, &[]).status.success());

    let claude_md = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert!(claude_md.starts_with("<!-- SPECTRA:START"));
    assert!(claude_md.ends_with("<!-- SPECTRA:END -->\n\nMy own notes\n"));
    // 管理鍵被強制回 false、使用者鍵保留、字母排序、無結尾換行。
    assert_eq!(
        std::fs::read_to_string(root.join(".claude/settings.json")).unwrap(),
        "{\n  \"includeGitInstructions\": false,\n  \"userKey\": true\n}"
    );
}

#[test]
fn codex_writes_only_agents_md_when_gemini_is_present() {
    let root = TempDir::new("update-codex-quirk");
    init_root(&root, "openspec");
    std::fs::create_dir_all(root.join(".agents")).unwrap();
    std::fs::create_dir_all(root.join(".gemini")).unwrap();

    let out = run_update(&root, &[]);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out),
        "✓ Updated instruction files for: gemini, codex\n"
    );
    assert!(root.join("AGENTS.md").is_file());
    assert!(!root.join(".agents/skills").exists());
    assert!(root.join(".gemini/skills/spectra-apply/SKILL.md").is_file());
}

// ---- 自訂 spec_dir 代換（end-to-end）----

#[test]
fn a_non_utf8_existing_file_does_not_abort_the_run() {
    // 迴歸（PR #86 review）：一個 latin-1 的既有檔曾讓整個 run 變成 exit 1 +
    // 部分寫入（17 檔只寫出 4 檔）。oracle 把讀不懂的既有檔當成不存在。
    let root = TempDir::new("update-non-utf8");
    init_root(&root, "openspec");
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(root.join(".claude/settings.json"), b"{\"k\":\"caf\xe9\"}").unwrap();
    std::fs::write(root.join("CLAUDE.md"), b"caf\xe9 notes\n").unwrap();

    let out = run_update(&root, &[]);
    assert!(out.status.success(), "run aborted: {out:?}");
    assert_eq!(stdout(&out), "✓ Updated instruction files for: claude\n");

    let mut files = Vec::new();
    collect_files(&root, &root, &mut files);
    assert!(
        files.len() >= 15,
        "expected the full tree to be written, got {files:?}"
    );
    assert_eq!(
        std::fs::read_to_string(root.join(".claude/settings.json")).unwrap(),
        "{\n  \"includeGitInstructions\": false\n}"
    );
}

#[test]
fn a_symlinked_managed_file_does_not_leak_writes_outside_the_project() {
    // 本 repo 的安全 baseline（artifact.rs:374）在 `update` 上的釘子：
    // oracle 會寫穿 symlink 覆寫專案外檔案，openspectra 一律不。
    let root = TempDir::new("update-symlink");
    init_root(&root, "openspec");
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    let outside = root.join("outside-secret.md");
    std::fs::write(&outside, "PRECIOUS").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("CLAUDE.md")).unwrap();

    let out = run_update(&root, &[]);
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "PRECIOUS");
}

#[test]
fn a_directory_at_a_plain_target_reports_the_oracle_errno() {
    // oracle 對「目標是目錄」只回報建立步驟的錯誤（`Is a directory`）；若我們
    // 無條件 unlink，同一輸入會變成 `Operation not permitted`。AC-2 要求逐字對齊。
    let root = TempDir::new("update-dir-target");
    init_root(&root, "openspec");
    std::fs::create_dir_all(root.join(".claude/skills/spectra-drift/SKILL.md")).unwrap();

    let out = run_update(&root, &[]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: Is a directory (os error 21)\n"
    );
}

#[test]
fn a_directory_at_a_managed_target_reports_the_oracle_errno() {
    // 迴歸（PR #86 round-2，Claude round2 lens）：寫入半邊改成裸錯誤時，
    // 讀取半邊（read_existing）漏改，於是 Managed / ClaudeSettings 的失敗
    // 訊息多出一段絕對路徑。Plain 形狀當時已經對齊，所以只有這半邊會露餡。
    let root = TempDir::new("update-managed-dir");
    init_root(&root, "openspec");
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::create_dir_all(root.join("CLAUDE.md")).unwrap();

    let out = run_update(&root, &[]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: Is a directory (os error 21)\n"
    );
}

#[test]
fn a_directory_at_the_settings_target_reports_the_oracle_errno() {
    let root = TempDir::new("update-settings-dir");
    init_root(&root, "openspec");
    std::fs::create_dir_all(root.join(".claude/settings.json")).unwrap();

    let out = run_update(&root, &[]);

    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: Is a directory (os error 21)\n"
    );
}

#[test]
fn an_unwritable_project_root_still_updates_an_existing_managed_file() {
    // oracle 就地寫入 Managed 檔，只需要檔案的寫入權；0500 的根目錄仍能成功。
    // 我們的 temp+rename 需要目錄寫入權，故在該情境退回就地寫入以維持 parity。
    let root = TempDir::new("update-ro-root");
    init_root(&root, "openspec");
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    assert!(run_update(&root, &[]).status.success());

    let mut perms = std::fs::metadata(&*root).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o500);
    std::fs::set_permissions(&*root, perms.clone()).unwrap();

    let out = run_update(&root, &[]);

    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(&*root, perms).unwrap();

    assert!(
        out.status.success(),
        "expected oracle parity (exit 0): {out:?}"
    );
    assert_eq!(stdout(&out), "✓ Updated instruction files for: claude\n");
}

#[test]
fn custom_spec_dir_is_substituted_into_written_files() {
    let root = TempDir::new("update-specdir");
    init_root(&root, "docs/specs");
    std::fs::create_dir_all(root.join(".claude")).unwrap();

    assert!(run_update(&root, &[]).status.success());

    let claude_md = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    assert!(claude_md.contains("`docs/specs/specs/`"));
    assert!(!claude_md.contains("openspec/"));
}
