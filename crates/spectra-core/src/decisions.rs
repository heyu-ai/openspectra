//! `spectra decisions`：跨 change 列出 `design.md` 的 `## Decisions` 決策（oracle 3.0.0，
//! 見 `docs/reverse-engineering/decisions.md`）。

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::config::Config;

/// 一筆決策；JSON key 依宣告順序輸出（oracle 以 struct 序列化，不是字母序）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub heading: String,
    pub change: String,
    pub date: String,
    pub rationale: String,
    pub supersedes: Option<SupersedesRef>,
    pub superseded: bool,
    pub unresolvable_supersession: Option<String>,
}

/// 已解析的 `**Supersedes**: <change> / <heading>`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SupersedesRef {
    pub change: String,
    pub heading: String,
}

const SUPERSEDES_FIELD: &str = "**Supersedes**:";

/// fence 開關：`trim_start()` 後以 ```` ``` ```` 或 `~~~` 開頭的行（共用一個旗標，
/// 不比對 fence 長度與種類，oracle 3.0.0 p09）。
fn toggles_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// 解析一份 design.md，依文件順序回傳 `(heading, rationale)`。
///
/// 只看 `trim_end()` 等於 `## Decisions` 的區段（可有多個），區段到下一個以 `## `
/// 開頭的行為止；區段內以 `### ` 開頭的行是一筆決策，heading 為其餘部分 `trim()`。
/// rationale 是 heading 之後到下一個 heading／區段結束的所有行，以 `\n` 串接後整段
/// `trim()`。fence 內的行不會被當成區段或 heading，但仍屬 rationale 內容。
pub(crate) fn parse_design(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut in_section = false;
    let mut current: Option<(String, Vec<&str>)> = None;
    let flush = |current: &mut Option<(String, Vec<&str>)>, out: &mut Vec<(String, String)>| {
        if let Some((heading, lines)) = current.take() {
            out.push((heading, lines.join("\n").trim().to_string()));
        }
    };
    for line in text.lines() {
        if toggles_fence(line) {
            in_fence = !in_fence;
        } else if !in_fence {
            if line.trim_end() == "## Decisions" {
                flush(&mut current, &mut out);
                in_section = true;
                continue;
            }
            if line.starts_with("## ") {
                flush(&mut current, &mut out);
                in_section = false;
                continue;
            }
            if in_section {
                if let Some(rest) = line.strip_prefix("### ") {
                    flush(&mut current, &mut out);
                    current = Some((rest.trim().to_string(), Vec::new()));
                    continue;
                }
            }
        }
        if let Some((_, lines)) = current.as_mut() {
            lines.push(line);
        }
    }
    flush(&mut current, &mut out);
    out
}

/// rationale 中第一個 `trim_start()` 後以 `**Supersedes**:` 開頭的行（不分 fence）。
/// 值以第一個 `/` 切開、兩邊 `trim()`；沒有 `/` 或任一邊為空就當作沒有這個欄位，
/// 且不再往後找其他行（oracle 3.0.0 p10/p11）。
fn supersedes_field(rationale: &str) -> Option<(String, String)> {
    let line = rationale
        .lines()
        .map(str::trim_start)
        .find(|line| line.starts_with(SUPERSEDES_FIELD))?;
    let (change, heading) = line[SUPERSEDES_FIELD.len()..].split_once('/')?;
    let (change, heading) = (change.trim(), heading.trim());
    if change.is_empty() || heading.is_empty() {
        return None;
    }
    Some((change.to_string(), heading.to_string()))
}

/// `YYYY-MM-DD` 的樣式（只看數字與 `-` 的位置，不檢查月日是否合法）。
fn is_date_like(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

/// 封存目錄名 → (change 名稱, 前綴日期)。oracle 3.0.0 p12/p13：長度大於 11 且第 5 個
/// 位元組是 `-` 就去掉前 11 個位元組（與日期是否合法無關，`2026-5-5-short` → `ort`）；
/// 長度大於 10 且前 10 個位元組符合 `YYYY-MM-DD` 樣式才取為日期。
fn archived_name_and_date(dir_name: &str) -> (String, Option<String>) {
    let bytes = dir_name.as_bytes();
    let name = if bytes.len() > 11 && bytes[4] == b'-' {
        dir_name.get(11..).unwrap_or(dir_name)
    } else {
        dir_name
    };
    let date = (bytes.len() > 10)
        .then(|| dir_name.get(..10))
        .flatten()
        .filter(|prefix| is_date_like(prefix))
        .map(str::to_string);
    (name.to_string(), date)
}

/// `.openspec.yaml` 的原始 `created`，規則與 `show` 相同（須同時有 `schema` 與 `created`）。
fn metadata_date(dir: &Path) -> String {
    crate::show::schema_and_created(dir).1.unwrap_or_default()
}

/// `dir` 底下的直接子目錄名稱（含隱藏目錄；`skip_archive` 時略過 `archive`）。
fn child_dirs(dir: &Path, skip_archive: bool) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| !(skip_archive && name == "archive"))
        .collect()
}

/// 一個 change 的決策；design.md 不存在、是目錄或不是合法 UTF-8 時靜默略過（oracle p14）。
fn push_change(out: &mut Vec<Decision>, dir: &Path, change: &str, date: &str) {
    let Ok(text) = std::fs::read_to_string(dir.join("design.md")) else {
        return;
    };
    for (heading, rationale) in parse_design(&text) {
        out.push(Decision {
            heading,
            change: change.to_string(),
            date: date.to_string(),
            rationale,
            supersedes: None,
            superseded: false,
            unresolvable_supersession: None,
        });
    }
}

/// 收集所有決策並解析 supersession（在 keyword 過濾之前）。
///
/// 順序：先是使用中的 change（`changes/` 下除 `archive` 外的每個目錄，含隱藏與日期開頭
/// 的名稱），依 `list` 預設的 modified 排序；再是 `changes/archive/` 下的每個目錄。
/// oracle 的同值順序與整個封存順序都是 readdir 順序（APFS 雜湊序，不可攜），
/// OpenSpectra 一律改以目錄名稱遞增（刻意分歧，見 decisions.md）。parked change 不列入。
pub fn collect(cfg: &Config) -> Vec<Decision> {
    let changes_dir = cfg.changes_dir();
    let mut active = child_dirs(&changes_dir, true);
    active.sort_by_cached_key(|name| {
        (
            std::cmp::Reverse(crate::change::latest_file_mtime(&changes_dir.join(name))),
            name.clone(),
        )
    });
    let archive_dir = changes_dir.join("archive");
    let mut archived = child_dirs(&archive_dir, false);
    archived.sort();

    let mut out = Vec::new();
    for name in &active {
        let dir = changes_dir.join(name);
        push_change(&mut out, &dir, name, &metadata_date(&dir));
    }
    for dir_name in &archived {
        let dir = archive_dir.join(dir_name);
        let (change, prefix_date) = archived_name_and_date(dir_name);
        let date = prefix_date.unwrap_or_else(|| metadata_date(&dir));
        push_change(&mut out, &dir, &change, &date);
    }
    resolve_supersession(&mut out);
    out
}

/// 以 (change, heading) 精確比對（大小寫敏感）解析每筆 `**Supersedes**`。同一目標有多筆
/// 決策時，只有清單順序中最後一筆被標為 superseded（oracle p11 dup-target）。
fn resolve_supersession(decisions: &mut [Decision]) {
    let mut index: HashMap<(String, String), usize> = HashMap::new();
    for (i, d) in decisions.iter().enumerate() {
        index.insert((d.change.clone(), d.heading.clone()), i);
    }
    let mut targets = Vec::new();
    for d in decisions.iter_mut() {
        let Some((change, heading)) = supersedes_field(&d.rationale) else {
            continue;
        };
        match index.get(&(change.clone(), heading.clone())) {
            Some(&target) => {
                targets.push(target);
                d.supersedes = Some(SupersedesRef { change, heading });
            }
            None => d.unresolvable_supersession = Some(format!("{change} / {heading}")),
        }
    }
    for target in targets {
        decisions[target].superseded = true;
    }
}

/// keyword 過濾：heading 或 rationale 在 `to_lowercase()` 後包含 keyword（keyword 同樣
/// 轉小寫、不 trim）；change 名稱與日期不搜尋。
pub fn filter(decisions: Vec<Decision>, keyword: Option<&str>) -> Vec<Decision> {
    let Some(keyword) = keyword else {
        return decisions;
    };
    let needle = keyword.to_lowercase();
    decisions
        .into_iter()
        .filter(|d| {
            d.heading.to_lowercase().contains(&needle)
                || d.rationale.to_lowercase().contains(&needle)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn pair(h: &str, r: &str) -> (String, String) {
        (h.to_string(), r.to_string())
    }

    // 以下預期值皆來自 oracle 3.0.0 對同一份 design.md 的輸出（W12 probe p08／p09）。

    #[test]
    fn only_the_exact_decisions_section_counts() {
        assert_eq!(
            parse_design("## Decisions  \n\n### A\n\nr\n"),
            vec![pair("A", "r")]
        );
        assert_eq!(
            parse_design("## Decisions\t\n\n### A\n\nr\n"),
            vec![pair("A", "r")]
        );
        for text in [
            "## Key Decisions\n\n### A\n\nr\n",
            "## decisions\n\n### A\n\nr\n",
            "# Decisions\n\n### A\n\nr\n",
            "### Decisions\n\n### A\n\nr\n",
            "## Decisions:\n\n### A\n\nr\n",
            " ## Decisions\n\n### A\n\nr\n",
            "\u{feff}## Decisions\n\n### A\n\nr\n",
            "### A\n\nr\n",
        ] {
            assert_eq!(parse_design(text), vec![], "{text:?}");
        }
        assert_eq!(
            parse_design(
                "## Decisions\n\n### A\n\nr\n\n## Other\n\n### X\n\n## Decisions\n\n### B\n\nr2\n"
            ),
            vec![pair("A", "r"), pair("B", "r2")]
        );
    }

    #[test]
    fn only_a_level_two_heading_with_a_space_ends_the_section() {
        assert_eq!(
            parse_design("## Decisions\n\n### A\n\nr\n\n# Top\n\nafter top\n"),
            vec![pair("A", "r\n\n# Top\n\nafter top")]
        );
        assert_eq!(
            parse_design("## Decisions\n\n### A\n\nr\n##\nafter\n"),
            vec![pair("A", "r\n##\nafter")]
        );
        assert_eq!(
            parse_design("## Decisions\n\n### A\n\nr\n##Nospace\nafter\n"),
            vec![pair("A", "r\n##Nospace\nafter")]
        );
        assert_eq!(
            parse_design("## Decisions\n\n### A\n\nr\n## \nafter\n"),
            vec![pair("A", "r")]
        );
        assert_eq!(
            parse_design("## Decisions\n\n### A\n\nr\n#### deep\nd\n"),
            vec![pair("A", "r\n#### deep\nd")]
        );
    }

    #[test]
    fn heading_shapes_follow_the_oracle() {
        assert_eq!(
            parse_design("## Decisions\n\n###A\n\nr\n\n### B\n\nr2\n"),
            vec![pair("B", "r2")]
        );
        assert_eq!(
            parse_design("## Decisions\n\n### \n\nr\n\n###\n\nr3\n\n### B\n\nr2\n"),
            vec![pair("", "r\n\n###\n\nr3"), pair("B", "r2")]
        );
        assert_eq!(
            parse_design("## Decisions\n\n###   Spaced   \n\nr\n"),
            vec![pair("Spaced", "r")]
        );
        assert_eq!(
            parse_design("## Decisions\n\n### Foo ###\n\nr\n"),
            vec![pair("Foo ###", "r")]
        );
        assert_eq!(parse_design("## Decisions\n\n###\tTab\n\nr\n"), vec![]);
        assert_eq!(
            parse_design("## Decisions\n\n  ### Indented\n\nr\n\n### B\n\nr2\n"),
            vec![pair("B", "r2")]
        );
        assert_eq!(
            parse_design("## Decisions\n\n### Same\n\none\n\n### Same\n\ntwo\n"),
            vec![pair("Same", "one"), pair("Same", "two")]
        );
    }

    #[test]
    fn fences_hide_structure_but_stay_in_the_rationale() {
        assert_eq!(
            parse_design(
                "## Decisions\n\n### A\n\n```\n### NotHeading\n## NotEnd\n```\n\nafter fence\n"
            ),
            vec![pair(
                "A",
                "```\n### NotHeading\n## NotEnd\n```\n\nafter fence"
            )]
        );
        assert_eq!(
            parse_design("## Decisions\n\n### A\n\n````\n```\n### InFour\n````\n\nx\n"),
            vec![pair("A", "````\n```"), pair("InFour", "````\n\nx")]
        );
        assert_eq!(
            parse_design("## Decisions\n\n### A\n\n  ```\n### InIndented\n  ```\n\nx\n"),
            vec![pair("A", "```\n### InIndented\n  ```\n\nx")]
        );
        assert_eq!(
            parse_design("## Decisions\n\n### A\n\n~~~\n### InTilde\n~~~\n\nx\n"),
            vec![pair("A", "~~~\n### InTilde\n~~~\n\nx")]
        );
        assert_eq!(
            parse_design("## Decisions\n\n### A\n\n```\n### InUnclosed\n\n## After\n"),
            vec![pair("A", "```\n### InUnclosed\n\n## After")]
        );
        assert_eq!(
            parse_design("```\n## Decisions\n```\n\n### A\n\nr\n"),
            vec![]
        );
    }

    #[test]
    fn rationale_is_trimmed_as_a_whole_and_crlf_is_normalized() {
        assert_eq!(
            parse_design("## Decisions\n\n### A\n\n   \n  indented first  \n\n  trailing  \n\n\n"),
            vec![pair("A", "indented first  \n\n  trailing")]
        );
        assert_eq!(
            parse_design(
                "## Decisions\r\n\r\n### A\r\n\r\nline1\r\nline2\r\n\r\n### B\r\n\r\nr2\r\n"
            ),
            vec![pair("A", "line1\nline2"), pair("B", "r2")]
        );
    }

    #[test]
    fn supersedes_field_takes_the_first_matching_line_only() {
        // 預期值來自 oracle 3.0.0 probe p10／p11。
        let ok = Some(("old".to_string(), "Use Redis".to_string()));
        assert_eq!(supersedes_field("**Supersedes**: old / Use Redis\n\nr"), ok);
        assert_eq!(
            supersedes_field("first para\n\n**Supersedes**: old / Use Redis"),
            ok
        );
        assert_eq!(supersedes_field("  **Supersedes**: old / Use Redis"), ok);
        assert_eq!(supersedes_field("**Supersedes**:old/Use Redis"), ok);
        assert_eq!(supersedes_field("**Supersedes**:\told\t/\tUse Redis"), ok);
        assert_eq!(
            supersedes_field("```\n**Supersedes**: old / Use Redis\n```"),
            ok
        );
        assert_eq!(
            supersedes_field("**Supersedes**: c / A / B"),
            Some(("c".to_string(), "A / B".to_string()))
        );
        for r in [
            "**Supersedes:** old / Use Redis",
            "Supersedes: old / Use Redis",
            "**supersedes**: old / Use Redis",
            "- **Supersedes**: old / Use Redis",
            "**Supersedes**: old Use Redis",
            "**Supersedes**:",
            "**Supersedes**:  / Use Redis",
            "**Supersedes**: old / ",
            "**Supersedes**: /",
            "**Supersedes**: nothing here\n**Supersedes**: old / Use Redis",
        ] {
            assert_eq!(supersedes_field(r), None, "{r:?}");
        }
    }

    #[test]
    fn archived_directory_names_follow_the_oracle_prefix_rules() {
        // 預期值逐一來自 oracle 3.0.0 probe p12／p13 的輸出。
        let cases = [
            ("2026-05-05-x", "x", Some("2026-05-05")),
            ("2026-5-5-short", "ort", None),
            ("2026-abcdefgh", "gh", None),
            ("abcd-efghijkl", "kl", None),
            ("2026-05-05x", "2026-05-05x", Some("2026-05-05")),
            ("2026-05-05-", "2026-05-05-", Some("2026-05-05")),
            ("2026-05-05", "2026-05-05", None),
            ("2026-05-05_under", "under", Some("2026-05-05")),
            ("2026-13-45-badate", "badate", Some("2026-13-45")),
            ("2026-0a-05-xyz", "xyz", None),
            ("2026--5-05-zz", "zz", None),
            ("12345-abcdefg", "12345-abcdefg", None),
            ("20260505-abcdef", "20260505-abcdef", None),
            ("x026-05-05-x", "x", None),
            ("nodate-arch", "nodate-arch", None),
        ];
        for (dir, name, date) in cases {
            assert_eq!(
                archived_name_and_date(dir),
                (name.to_string(), date.map(str::to_string)),
                "{dir}"
            );
        }
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn cfg(root: &Path) -> Config {
        Config {
            root: root.to_path_buf(),
            spec_dir: "openspec".to_string(),
            locale: None,
            claude_slash_commands: false,
        }
    }

    fn set_mtime(path: &Path, secs: u64) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
            .unwrap();
    }

    #[test]
    fn collect_orders_active_by_mtime_then_archived_and_resolves_supersession() {
        let dir = TempDir::new("decisions-collect");
        let c = dir.join("openspec/changes");
        write(
            &c.join("alpha/design.md"),
            "## Decisions\n\n### Replace Cache\n\n**Supersedes**: old / Use Redis\n\nnew\n\n### Bad Ref\n\n**Supersedes**: ghost/Nothing\n",
        );
        write(
            &c.join("alpha/.openspec.yaml"),
            "schema: spec-driven\ncreated: 2026-09-01\n",
        );
        write(&c.join("beta/design.md"), "## Decisions\n\n### Beta\n\nb\n");
        write(&c.join("beta/.openspec.yaml"), "created: 2026-02-10\n");
        write(
            &c.join("archive/2026-01-15-old/design.md"),
            "## Decisions\n\n### Use Redis\n\nfast\n",
        );
        write(
            &c.join("archive/nodate/design.md"),
            "## Decisions\n\n### N\n\nn\n",
        );
        write(
            &c.join("archive/nodate/.openspec.yaml"),
            "schema: spec-driven\ncreated: 2024-04-04\n",
        );
        write(
            &c.join("archive/design.md"),
            "## Decisions\n\n### Ignored\n\nx\n",
        );
        for (file, secs) in [
            ("alpha/design.md", 1_000),
            ("alpha/.openspec.yaml", 1_000),
            ("beta/design.md", 2_000),
            ("beta/.openspec.yaml", 2_000),
        ] {
            set_mtime(&c.join(file), secs);
        }

        let all = collect(&cfg(&dir));
        let summary: Vec<_> = all
            .iter()
            .map(|d| (d.change.as_str(), d.heading.as_str(), d.date.as_str()))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("beta", "Beta", ""),
                ("alpha", "Replace Cache", "2026-09-01"),
                ("alpha", "Bad Ref", "2026-09-01"),
                ("old", "Use Redis", "2026-01-15"),
                ("nodate", "N", "2024-04-04"),
            ]
        );
        assert_eq!(
            all[1].supersedes,
            Some(SupersedesRef {
                change: "old".to_string(),
                heading: "Use Redis".to_string()
            })
        );
        assert_eq!(
            all[2].unresolvable_supersession.as_deref(),
            Some("ghost / Nothing")
        );
        assert_eq!(
            all.iter().map(|d| d.superseded).collect::<Vec<_>>(),
            vec![false, false, false, true, false]
        );

        // 過濾在 supersession 之後：只剩被取代的那筆時仍標 superseded（oracle p12）。
        let fast = filter(all.clone(), Some("FAST"));
        assert_eq!(fast.len(), 1);
        assert!(fast[0].superseded);
        // change 名稱不在搜尋範圍內。
        assert!(filter(all.clone(), Some("alpha")).is_empty());
        assert_eq!(filter(all, Some("")).len(), 5);
    }

    #[test]
    fn only_the_last_duplicate_target_is_marked_superseded() {
        let dir = TempDir::new("decisions-dup");
        let c = dir.join("openspec/changes");
        write(
            &c.join("archive/2026-01-15-old/design.md"),
            "## Decisions\n\n### Use Redis\n\nold r\n\n### Use Redis\n\ndup\n",
        );
        write(
            &c.join("c/design.md"),
            "## Decisions\n\n### N\n\n**Supersedes**: old / Use Redis\n",
        );
        let all = collect(&cfg(&dir));
        assert_eq!(
            all.iter().map(|d| d.superseded).collect::<Vec<_>>(),
            vec![false, false, true]
        );
    }

    #[test]
    fn unreadable_design_files_are_skipped_silently() {
        let dir = TempDir::new("decisions-skip");
        let c = dir.join("openspec/changes");
        std::fs::create_dir_all(c.join("dir-design/design.md")).unwrap();
        std::fs::create_dir_all(c.join("latin1")).unwrap();
        std::fs::write(
            c.join("latin1/design.md"),
            b"## Decisions\n\n### Caf\xe9\n\nr\n",
        )
        .unwrap();
        write(&c.join(".hidden/design.md"), "## Decisions\n\n### H\n\nh\n");
        let all = collect(&cfg(&dir));
        assert_eq!(
            all.iter().map(|d| d.change.as_str()).collect::<Vec<_>>(),
            vec![".hidden"]
        );
    }

    #[test]
    fn keyword_uses_unicode_lowercase_not_case_folding() {
        // oracle 3.0.0 probe p13。
        let d = Decision {
            heading: "ÄBC Straße".to_string(),
            change: "u".to_string(),
            date: String::new(),
            rationale: "İstanbul ǅ".to_string(),
            supersedes: None,
            superseded: false,
            unresolvable_supersession: None,
        };
        let hits = |k: &str| filter(vec![d.clone()], Some(k)).len();
        assert_eq!(hits("äbc"), 1);
        assert_eq!(hits("straße"), 1);
        assert_eq!(hits("strasse"), 0);
        assert_eq!(hits("i\u{307}stanbul"), 1);
        assert_eq!(hits("istanbul"), 0);
        assert_eq!(hits("Ǆ"), 1);
        assert_eq!(hits("  äbc  "), 0);
    }

    #[test]
    fn json_keys_keep_declaration_order() {
        let d = Decision {
            heading: "h".to_string(),
            change: "c".to_string(),
            date: "2026-01-01".to_string(),
            rationale: "r".to_string(),
            supersedes: Some(SupersedesRef {
                change: "o".to_string(),
                heading: "x".to_string(),
            }),
            superseded: false,
            unresolvable_supersession: None,
        };
        assert_eq!(
            serde_json::to_string(&d).unwrap(),
            r#"{"heading":"h","change":"c","date":"2026-01-01","rationale":"r","supersedes":{"change":"o","heading":"x"},"superseded":false,"unresolvableSupersession":null}"#
        );
    }
}
