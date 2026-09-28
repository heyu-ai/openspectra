//! 驗收 A2：oracle 3.0.0 的 skill 模板與內嵌 skill 裡每一個 `spectra ...` 呼叫，
//! 都必須能被本 CLI 的 clap 定義解析（docs/migration-plan.md「總目標與驗收條件」）。
//!
//! 只有「不認得」類的錯誤算失敗（未知參數、未知子指令、不合法的值……）。缺必要參數
//! 不算：模板常在內文提到指令名稱（例如 `spectra task done`），那不是完整呼叫。
//!
//! 尚未移植的介面列在 [`KNOWN_GAPS`]。補上之後該項會開始解析成功，測試會要求把它
//! 從清單移除，讓清單只能縮小、不能靜默過期。

use clap::error::ErrorKind;
use clap::Parser;

use super::Cli;

/// 已知缺口：正規化後的呼叫字串（placeholder 已代入）。每個執行佇列項目補完就移除對應條目。
const KNOWN_GAPS: &[&str] = &[];

/// 3.0.0 `--help` 列出的子指令，加上本 CLI 獨有的 `search`、`trace`。
/// 只把 `spectra` 後面接這些字的片段視為呼叫，避免把內文的 "spectra CLI" 當指令。
const SUBCOMMANDS: &[&str] = &[
    "init",
    "update",
    "list",
    "show",
    "validate",
    "scope",
    "analyze",
    "drift",
    "archive",
    "decisions",
    "status",
    "instructions",
    "new",
    "schemas",
    "templates",
    "feedback",
    "schema",
    "config",
    "completion",
    "park",
    "unpark",
    "task",
    "in-progress",
    "demo",
    "search",
    "trace",
];

/// 從 Markdown 取出 inline code span 與 fenced code block 的內容（只有這些地方寫的是指令）。
fn code_fragments(markdown: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            out.push(line.to_string());
            continue;
        }
        let mut parts = line.split('`');
        parts.next();
        while let Some(inside) = parts.next() {
            out.push(inside.to_string());
            parts.next();
        }
    }
    out
}

/// 在一段程式碼片段中找出所有 `spectra <子指令> ...`，回傳 token 串（尚未代入 placeholder）。
fn invocations_in(fragment: &str) -> Vec<Vec<String>> {
    let tokens: Vec<&str> = fragment.split_whitespace().collect();
    let mut found = Vec::new();
    for (i, tok) in tokens.iter().enumerate() {
        if *tok != "spectra" {
            continue;
        }
        let Some(sub) = tokens.get(i + 1) else {
            continue;
        };
        if !SUBCOMMANDS.contains(sub) {
            continue;
        }
        let mut call = Vec::new();
        for t in &tokens[i + 1..] {
            if t.starts_with("<<")
                || t.starts_with('|')
                || t.starts_with("&&")
                || t.starts_with(';')
                || t.starts_with('>')
                || t.starts_with("2>")
                || *t == "spectra"
            {
                break;
            }
            if let Some(pos) = t.find(')') {
                let head = &t[..pos];
                if !head.is_empty() {
                    call.push(head.to_string());
                }
                break;
            }
            call.push((*t).to_string());
        }
        found.push(call);
    }
    found
}

/// placeholder 的代入值；`{{TOOL}}` 由呼叫端逐一展開。
fn placeholder_value(name: &str) -> String {
    if let Some(first) = name.split('|').next().filter(|_| name.contains('|')) {
        return first.to_string();
    }
    match name {
        "task-id" => "1.1".to_string(),
        "path" => "src/lib.rs".to_string(),
        "artifact-id" => "proposal".to_string(),
        _ => "x".to_string(),
    }
}

/// 展開 `[...]` 可選群組（全部省略、全部保留兩種）並代入 placeholder。
fn concretize(call: &[String], tool: &str) -> Vec<Vec<String>> {
    let mut without = Vec::new();
    let mut with = Vec::new();
    let mut in_optional = false;
    let mut had_optional = false;
    for raw in call {
        let mut tok = raw.as_str();
        let opens = tok.starts_with('[');
        if opens {
            in_optional = true;
            had_optional = true;
            tok = &tok[1..];
        }
        let closes = tok.ends_with(']');
        if closes {
            tok = &tok[..tok.len() - 1];
        }
        let unquoted = tok.trim_matches('"').trim_matches('\'');
        let value = if unquoted == "{{TOOL}}" {
            tool.to_string()
        } else if unquoted.starts_with('<') && unquoted.ends_with('>') && unquoted.len() > 2 {
            placeholder_value(&unquoted[1..unquoted.len() - 1])
        } else {
            unquoted.to_string()
        };
        if !value.is_empty() {
            with.push(value.clone());
            if !in_optional {
                without.push(value);
            }
        }
        if closes {
            in_optional = false;
        }
    }
    if had_optional {
        vec![without, with]
    } else {
        vec![with]
    }
}

/// 只有「不認得」類的錯誤代表 CLI 缺介面。
fn is_surface_gap(kind: ErrorKind) -> bool {
    !matches!(
        kind,
        ErrorKind::MissingRequiredArgument
            | ErrorKind::MissingSubcommand
            | ErrorKind::DisplayHelp
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            | ErrorKind::DisplayVersion
    )
}

fn parse_gap(args: &[String]) -> Option<String> {
    let argv = std::iter::once("spectra".to_string()).chain(args.iter().cloned());
    match Cli::try_parse_from(argv) {
        Ok(_) => None,
        Err(e) if is_surface_gap(e.kind()) => Some(format!("{:?}", e.kind())),
        Err(_) => None,
    }
}

/// 收集所有來源的呼叫：(來源描述, 正規化後的 argv)。
fn collect_invocations() -> Vec<(String, Vec<String>)> {
    let tool_ids: Vec<&str> = spectra_core::update::registry()
        .iter()
        .map(|t| t.id)
        .collect();
    let mut sources: Vec<(String, &str)> = Vec::new();
    for tool in spectra_core::update::registry() {
        for file in tool.files {
            sources.push((format!("update:{}", file.relpath), file.template));
        }
    }
    for (name, body) in spectra_core::skills::all_skills() {
        sources.push((format!("skill:{name}"), body));
    }

    let mut out = Vec::new();
    for (origin, text) in sources {
        for fragment in code_fragments(text) {
            for call in invocations_in(&fragment) {
                let tools: Vec<&str> = if call.iter().any(|t| t.contains("{{TOOL}}")) {
                    tool_ids.clone()
                } else {
                    vec![""]
                };
                for tool in tools {
                    for argv in concretize(&call, tool) {
                        out.push((origin.clone(), argv));
                    }
                }
            }
        }
    }
    out
}

#[test]
fn every_template_invocation_parses() {
    let invocations = collect_invocations();

    // 抽取失效會讓測試變成空轉的綠燈：兩個來源都必須有貢獻，總數也要有下限。
    assert!(
        invocations.iter().any(|(o, _)| o.starts_with("update:")),
        "no invocation extracted from update templates"
    );
    assert!(
        invocations.iter().any(|(o, _)| o.starts_with("skill:")),
        "no invocation extracted from embedded skills"
    );
    assert!(
        invocations.len() >= 100,
        "only {} invocations extracted; extraction is probably broken",
        invocations.len()
    );

    let mut unexpected = Vec::new();
    let mut still_failing = std::collections::BTreeSet::new();
    for (origin, argv) in &invocations {
        let joined = argv.join(" ");
        if let Some(kind) = parse_gap(argv) {
            if KNOWN_GAPS.contains(&joined.as_str()) {
                still_failing.insert(joined);
            } else {
                unexpected.push(format!("{kind}: spectra {joined}  ({origin})"));
            }
        }
    }
    unexpected.sort();
    unexpected.dedup();
    assert!(
        unexpected.is_empty(),
        "template invocations the CLI cannot parse:\n{}",
        unexpected.join("\n")
    );

    let stale: Vec<&&str> = KNOWN_GAPS
        .iter()
        .filter(|g| !still_failing.contains(**g))
        .collect();
    assert!(
        stale.is_empty(),
        "KNOWN_GAPS entries that now parse (or no longer appear); remove them: {stale:?}"
    );
}

#[test]
fn classifier_flags_an_unknown_flag_and_accepts_a_prose_reference() {
    // 正向對照：分類器對已知壞輸入必須判失敗，否則上面的綠燈沒有資訊量。
    let bad: Vec<String> = ["list", "--definitely-not-a-flag"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(parse_gap(&bad).as_deref(), Some("UnknownArgument"));
    let unknown_sub: Vec<String> = vec!["definitely-not-a-subcommand".to_string()];
    assert!(parse_gap(&unknown_sub).is_some());
    // 內文提到指令名稱、缺必要參數，不算缺介面。
    let prose: Vec<String> = ["task", "done"].iter().map(|s| s.to_string()).collect();
    assert_eq!(parse_gap(&prose), None);
}

#[test]
fn extraction_handles_template_shapes() {
    let calls = invocations_in(
        r#"spectra new artifact proposal --change "<name>" --stdin <<'ARTIFACT_EOF'"#,
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(
        concretize(&calls[0], ""),
        vec![vec![
            "new", "artifact", "proposal", "--change", "x", "--stdin"
        ]]
    );

    let calls = invocations_in(
        r#"spectra instructions proposal --change "<name>" --json [--type <bug-fix|refactor>]"#,
    );
    assert_eq!(
        concretize(&calls[0], ""),
        vec![
            vec!["instructions", "proposal", "--change", "x", "--json"],
            vec![
                "instructions",
                "proposal",
                "--change",
                "x",
                "--json",
                "--type",
                "bug-fix"
            ],
        ]
    );

    let calls = invocations_in("spectra status --json) for completion checking");
    assert_eq!(concretize(&calls[0], ""), vec![vec!["status", "--json"]]);

    assert!(invocations_in("the spectra CLI is required").is_empty());
    assert_eq!(
        code_fragments("run `spectra list --json` now\n```\nspectra drift x\n```\nspectra park y"),
        vec!["spectra list --json", "spectra drift x"]
    );
}
