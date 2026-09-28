//! `spectra validate` 預設的 oracle 格式（W9a，決策 D3、D12）。
//!
//! 形狀來自 oracle 3.0.0 的實測（W9 RE 規格 A1–A4，probe p06/p08/p09）：
//! human 是 `✓ <name> — valid`／`✗ <name> — invalid`，底下先列 `  error: `、再列
//! `  warn: `，有 invalid item 時 stderr 多一行 `Error: Validation failed.`、rc 1；
//! `--json` 是 `{change|spec, errors, valid, warnings}` 陣列。內容依 D12：
//! OpenSpec 的 ERROR／WARNING 照搬，INFO 只有「Archive would refuse this delta」進
//! warnings，delta 檔的訊息前綴 `specs/<cap>/spec.md: `。訊息本身來自 OpenSpec
//! 1.13.2（見 `validate-openspec-1.13.2.json`），這裡一律寫字面值。
//!
//! 內建 skill 以 `spectra validate "<name>"` 呼叫、只看 human 輸出與 exit code，
//! 所以單一 item 的 human 路徑逐字固定。

mod common;

use std::path::Path;

use common::{spectra, TempDir};

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// 規則 fixture（`tests/fixtures/validate_openspec`）的一份複本。
fn rules() -> TempDir {
    let dir = TempDir::new("validate-format-rules");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/validate_openspec"),
        &dir,
    );
    dir
}

fn run(root: &Path, args: &[&str]) -> (Option<i32>, String, String) {
    let output = spectra()
        .arg("validate")
        .args(args)
        .current_dir(root)
        .env_remove("NO_COLOR")
        .output()
        .unwrap();
    (
        output.status.code(),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

const FAILED: &str = "Error: Validation failed.\n";

#[test]
fn human_lists_errors_before_the_archive_refusal_warning() {
    let root = rules();
    // OpenSpec 的順序是 INFO（archive 拒絕）在前、ERROR 在後；oracle 格式先印 error。
    assert_eq!(
        run(&root, &["d23-mixed"]),
        (
            Some(1),
            "✗ d23-mixed — invalid\n\
             \x20 error: specs/zz/spec.md: No delta sections found. Add headers such as \
             \"## ADDED Requirements\" or move non-delta notes outside specs/.\n\
             \x20 warn: specs/base/spec.md: Archive would refuse this delta: base MODIFIED \
             failed for header \"### Requirement: Nope\" - not found\n"
                .to_string(),
            FAILED.to_string()
        )
    );
}

#[test]
fn human_valid_item_carries_the_archive_refusal_as_a_warning() {
    let root = rules();
    assert_eq!(
        run(&root, &["d10-modmissing"]),
        (
            Some(0),
            "✓ d10-modmissing — valid\n\
             \x20 warn: specs/base/spec.md: Archive would refuse this delta: base MODIFIED \
             failed for header \"### Requirement: Nope\" - not found\n"
                .to_string(),
            String::new()
        )
    );
}

/// D12-1：其餘 INFO（skip_specs 接受、stray header）不顯示。
#[test]
fn human_drops_every_other_info_finding() {
    let root = rules();
    for name in ["d19-skipspecs", "d13-strayh3"] {
        assert_eq!(
            run(&root, &[name]),
            (Some(0), format!("✓ {name} — valid\n"), String::new()),
            "{name}"
        );
    }
}

/// D12-2：只有 delta 檔的訊息加 `specs/<path>: `；change 層級（`file`）與 task 檔的
/// 訊息不加。specs/ 根目錄的 spec.md 與非 spec.md 的 delta 檔也是 delta 檔。
#[test]
fn human_prefixes_only_delta_file_messages() {
    let root = rules();
    assert_eq!(
        run(&root, &["d04-rootspec"]).1,
        "✗ d04-rootspec — invalid\n\
         \x20 error: specs/spec.md: Delta spec found at specs/spec.md. Delta specs must live \
         under a capability path (e.g. specs/<capability-path>/spec.md) — a file at the specs/ \
         root is ignored when the change is applied or archived.\n"
    );
    assert_eq!(
        run(&root, &["d18-capmd"]).1,
        "✗ d18-capmd — invalid\n\
         \x20 error: specs/x.md: Delta spec found at specs/x.md. Delta specs must be a spec.md \
         inside a capability folder — this file is ignored when the change is applied or \
         archived. Move its requirements into specs/x/spec.md.\n"
    );
    assert_eq!(
        run(&root, &["d09-noshall"]),
        (
            Some(0),
            "✓ d09-noshall — valid\n\
             \x20 warn: specs/x/spec.md: ADDED \"Alpha\" should contain SHALL or MUST (RFC 2119 \
             best practice for English specs)\n"
                .to_string(),
            String::new()
        )
    );
    assert_eq!(
        run(&root, &["d02-nospecs"]).1,
        "✗ d02-nospecs — invalid\n\
         \x20 error: Change must have at least one delta. No deltas found. Ensure your change \
         has a specs/ directory with capability folders (e.g. specs/http-server/spec.md) \
         containing .md files that use delta headers (## ADDED/MODIFIED/REMOVED/RENAMED \
         Requirements) and that each requirement includes at least one \"#### Scenario:\" \
         block. If this change intentionally modifies no specs (pure refactor, tooling, docs), \
         set \"skip_specs: true\" in the change's .openspec.yaml instead. Tip: run \"openspec \
         change show <change-id> --json --deltas-only\" to inspect parsed deltas.\n"
    );
    assert_eq!(
        run(&root, &["d30-tasks"]),
        (
            Some(0),
            "✓ d30-tasks — valid\n\
             \x20 warn: Task ID \"1.1\" is duplicated; it was first declared on line 3.\n\
             \x20 warn: Task \"2.1\" is under group 1, but its leading number points to group \
             2. Move it to group 2 or renumber it.\n"
                .to_string(),
            String::new()
        )
    );
}

/// D12-4：oracle 格式也接受 spec 名稱；main spec 的訊息沒有前綴。
#[test]
fn human_accepts_a_spec_name() {
    let root = rules();
    assert_eq!(
        run(&root, &["s08-noscenario"]),
        (
            Some(1),
            "✗ s08-noscenario — invalid\n\
             \x20 error: Requirement must have at least one scenario\n\
             \x20 warn: Requirement must have at least one scenario. Scenarios must use level-4 \
             headers. Convert bullet lists into:\n#### Scenario: Short name\n- **WHEN** ...\n\
             - **THEN** ...\n- **AND** ...\n"
                .to_string(),
            FAILED.to_string()
        )
    );
    assert_eq!(
        run(&root, &["s01-good"]),
        (Some(0), "✓ s01-good — valid\n".to_string(), String::new())
    );
}

#[test]
fn json_is_the_oracle_array_with_alphabetical_keys() {
    let root = rules();
    assert_eq!(
        run(&root, &["d23-mixed", "--json"]),
        (
            Some(1),
            "[\n  {\n    \"change\": \"d23-mixed\",\n    \"errors\": [\n      \
             \"specs/zz/spec.md: No delta sections found. Add headers such as \\\"## ADDED \
             Requirements\\\" or move non-delta notes outside specs/.\"\n    ],\n    \
             \"valid\": false,\n    \"warnings\": [\n      \"specs/base/spec.md: Archive would \
             refuse this delta: base MODIFIED failed for header \\\"### Requirement: Nope\\\" - \
             not found\"\n    ]\n  }\n]\n"
                .to_string(),
            FAILED.to_string()
        )
    );
    assert_eq!(
        run(&root, &["s01-good", "--json"]),
        (
            Some(0),
            "[\n  {\n    \"errors\": [],\n    \"spec\": \"s01-good\",\n    \"valid\": true,\n    \
             \"warnings\": []\n  }\n]\n"
                .to_string(),
            String::new()
        )
    );
    // `--format oracle` 就是預設值。
    assert_eq!(
        run(&root, &["s01-good", "--json", "--format", "oracle"]),
        run(&root, &["s01-good", "--json"])
    );
}

#[test]
fn unknown_item_is_the_oracle_error_in_both_modes() {
    let root = rules();
    for args in [&["nope"][..], &["nope", "--json"][..]] {
        assert_eq!(
            run(&root, args),
            (
                Some(1),
                String::new(),
                "Error: Change 'nope' not found.\n".to_string()
            ),
            "{args:?}"
        );
    }
}

fn json_names(stdout: &str) -> Vec<String> {
    let items: serde_json::Value = serde_json::from_str(stdout).unwrap();
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|item| match (item.get("change"), item.get("spec")) {
            (Some(name), None) => format!("change/{}", name.as_str().unwrap()),
            (None, Some(name)) => format!("spec/{}", name.as_str().unwrap()),
            _ => panic!("item 必須恰有 change 或 spec：{item}"),
        })
        .collect()
}

/// D12-3：`--all` 與 `--changes --specs` 都驗 changes 與 specs（OpenSpec 語意；oracle
/// 的 `--all` 只驗 changes、`--changes --specs` 只驗 specs，是刻意分歧）。D12-5：
/// 不帶參數時跟 oracle，驗全部 changes。
#[test]
fn scopes_follow_openspec_and_a_bare_validate_takes_every_change() {
    let root = rules();
    let all = run(&root, &["--all", "--json"]);
    let names = json_names(&all.1);
    assert_eq!(names.len(), 46 + 33);
    let changes = names.iter().filter(|n| n.starts_with("change/")).count();
    assert_eq!(changes, 46);
    // changes 全部在前，specs 在後（oracle 格式不混排）。
    assert!(names[..46].iter().all(|n| n.starts_with("change/")));
    assert_eq!(names[46], "spec/base");
    assert_eq!(names[78], "spec/s30-generated-placeholder");
    assert_eq!(run(&root, &["--changes", "--specs", "--json"]), all);
    assert_eq!(run(&root, &["--all", "--changes", "--json"]), all);

    let changes_only = run(&root, &["--changes", "--json"]);
    assert_eq!(json_names(&changes_only.1).len(), 46);
    assert_eq!(run(&root, &["--json"]), changes_only);
    assert_eq!(run(&root, &[]), run(&root, &["--changes"]));
    let specs_only = run(&root, &["--specs", "--json"]);
    assert_eq!(json_names(&specs_only.1), names[46..].to_vec());
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn set_mtime(dir: &Path, secs: u64) {
    let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(time)
                .unwrap();
        }
    }
}

/// oracle 的 change 順序與 `list --json` 相同：最新檔案 mtime 由新到舊（A1）；
/// specs 依完整 id 的 byte 順序。
#[test]
fn oracle_order_is_list_order_for_changes_and_byte_order_for_specs() {
    let root = TempDir::new("validate-format-order");
    write(&root.join(".spectra.yaml"), "spec_dir: openspec\n");
    write(&root.join("openspec/config.yaml"), "schema: spec-driven\n");
    for (name, secs) in [
        ("alpha", 1_000_000),
        ("zeta", 3_000_000),
        ("mid", 2_000_000),
    ] {
        let dir = root.join("openspec/changes").join(name);
        write(
            &dir.join(".openspec.yaml"),
            "schema: spec-driven\ncreated: 2026-09-01\n",
        );
        write(&dir.join("proposal.md"), "## Why\n\nBecause.\n");
        set_mtime(&dir, secs);
    }
    for cap in ["b", "B-upper", "a/b", "a.b"] {
        write(
            &root.join("openspec/specs").join(cap).join("spec.md"),
            "# x\n\n## Purpose\n\nThis capability exists to describe enough behavior here.\n\n\
             ## Requirements\n\n### Requirement: R\n\nThe system SHALL work.\n\n\
             #### Scenario: s\n\n- **WHEN** x\n- **THEN** y\n",
        );
    }
    let list = spectra()
        .args(["list", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    let listed: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    let listed: Vec<String> = listed["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| format!("change/{}", c["name"].as_str().unwrap()))
        .collect();
    assert_eq!(listed, ["change/zeta", "change/mid", "change/alpha"]);
    let (_, stdout, _) = run(&root, &["--all", "--json"]);
    assert_eq!(
        json_names(&stdout),
        [
            "change/zeta",
            "change/mid",
            "change/alpha",
            "spec/B-upper",
            "spec/a.b",
            "spec/a/b",
            "spec/b",
        ]
    );
}

/// 空的 scope：什麼都不印、rc 0；`--json` 是 `[]`（A2、A3）。
#[test]
fn empty_scope_prints_nothing() {
    let root = TempDir::new("validate-format-empty");
    write(&root.join(".spectra.yaml"), "spec_dir: openspec\n");
    write(&root.join("openspec/config.yaml"), "schema: spec-driven\n");
    for args in [&[][..], &["--all"][..], &["--specs"][..]] {
        assert_eq!(run(&root, args), (Some(0), String::new(), String::new()));
    }
    assert_eq!(
        run(&root, &["--all", "--json"]),
        (Some(0), "[]\n".to_string(), String::new())
    );
}

/// `--report findings` 在 oracle 格式只留下有 error／warn 行的 item；rc 仍依整次結果。
#[test]
fn findings_report_keeps_only_items_with_oracle_messages() {
    let root = rules();
    let (rc, stdout, stderr) = run(&root, &["--changes", "--report", "findings", "--json"]);
    assert_eq!((rc, stderr.as_str()), (Some(1), FAILED));
    let names = json_names(&stdout);
    assert!(names.contains(&"change/d10-modmissing".to_string()));
    assert!(names.contains(&"change/d02-nospecs".to_string()));
    // d01-good 沒有 finding；d19、d13 只有被丟掉的 INFO。
    for hidden in ["d01-good", "d19-skipspecs", "d13-strayh3"] {
        assert!(!names.contains(&format!("change/{hidden}")), "{hidden}");
    }
    let (_, human, _) = run(&root, &["--changes", "--report", "findings"]);
    assert!(!human.contains("d19-skipspecs"), "{human}");
    assert!(human.contains("✓ d10-modmissing — valid\n"), "{human}");
}

/// D12-6：OpenSpectra 獨有的 archive 拒絕理由（這裡是被擋下的 capability
/// retirement）仍是 error，路徑是 change 層級，不加前綴。
#[test]
fn openspectra_only_archive_refusals_stay_errors() {
    let root = TempDir::new("validate-format-retire");
    write(&root.join(".spectra.yaml"), "spec_dir: openspec\n");
    write(&root.join("openspec/config.yaml"), "schema: spec-driven\n");
    write(
        &root.join("openspec/changes/feat/.openspec.yaml"),
        "schema: spec-driven\nretire_capabilities: true\n",
    );
    write(
        &root.join("openspec/specs/auth/spec.md"),
        "# auth Specification\n\n## Purpose\n\nAuthentication of family accounts across every \
         client.\n\n## Requirements\n\n### Requirement: Login\nThe system SHALL authenticate.\n\n\
         #### Scenario: Login\n- **WHEN** requested\n- **THEN** access is granted\n\n\
         ## Operational Notes\n\nThis content prevents whole-capability retirement.\n",
    );
    write(
        &root.join("openspec/changes/feat/specs/auth/spec.md"),
        "## REMOVED Requirements\n\n### Requirement: Login\n",
    );
    assert_eq!(
        run(&root, &["feat"]),
        (
            Some(1),
            "✗ feat — invalid\n\
             \x20 error: capability 'auth' cannot be retired because its spec contains content \
             outside Purpose and Requirements\n"
                .to_string(),
            FAILED.to_string()
        )
    );
}

#[test]
fn format_requires_json() {
    let root = rules();
    let (rc, stdout, stderr) = run(&root, &["d01-good", "--format", "openspec"]);
    assert_eq!(rc, Some(2), "{stderr}");
    assert!(stdout.is_empty());
    assert!(stderr.contains("--json"), "{stderr}");
}

/// 在 PTY 上跑 `spectra validate <args>`，回傳合併的輸出。stdin 撐到子程序結束才關
/// （`script` 在 stdin EOF 時會對 PTY 送 ^D，見 CLAUDE.md 的 PTY 測試規則）。
#[cfg(unix)]
fn on_terminal(root: &Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("script");
    command
        .env("OPENSPECTRA_IMPL", "oss")
        .env_remove("NO_COLOR");
    #[cfg(target_os = "macos")]
    {
        command.args(["-q", "/dev/null", env!("CARGO_BIN_EXE_spectra"), "validate"]);
        command.args(args);
    }
    #[cfg(not(target_os = "macos"))]
    command.args([
        "-q",
        "-c",
        &format!(
            "{} validate {}",
            env!("CARGO_BIN_EXE_spectra"),
            args.join(" ")
        ),
        "/dev/null",
    ]);
    let mut child = command
        .current_dir(root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let output = child.wait_with_output().unwrap();
    drop(stdin);
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A2 的顏色：只有符號與 `error:`／`warn:` 標籤上色（oracle 3.0.0，p09 TTY 實測）；
/// 名稱、`—`、`valid`／`invalid`、訊息與 stderr 那行都不上色。
#[cfg(unix)]
#[test]
fn human_colors_only_the_glyph_and_the_label_on_a_terminal() {
    let root = rules();
    let text = on_terminal(&root, &["d23-mixed"]);
    assert!(
        text.contains("\x1b[31m✗\x1b[0m d23-mixed — invalid\r\n"),
        "{text:?}"
    );
    assert!(
        text.contains("  \x1b[31merror:\x1b[0m specs/zz/spec.md: No delta sections found."),
        "{text:?}"
    );
    assert!(
        text.contains("  \x1b[33mwarn:\x1b[0m specs/base/spec.md: Archive would refuse"),
        "{text:?}"
    );
    assert!(text.contains("\nError: Validation failed.\r\n"), "{text:?}");

    let valid = on_terminal(&root, &["d01-good"]);
    assert!(
        valid.contains("\x1b[32m✓\x1b[0m d01-good — valid\r\n"),
        "{valid:?}"
    );
    let plain = on_terminal(&root, &["d01-good", "--no-color"]);
    assert!(plain.contains("✓ d01-good — valid\r\n"), "{plain:?}");
    assert!(!plain.contains('\x1b'), "{plain:?}");
}
