//! `tasks.md` parsing and task/commit collision detection.
//!
//! Tasks are checkboxes on a `-`, `*`, or `+` bullet; only `[x]`/`[X]` is
//! done, any other single-character marker (`[ ]`, `[~]`, …) is pending.
//! Drift flags pending tasks that collide with commits since the change's
//! `created` date (oracle 3.0.0; `docs/reverse-engineering/drift.md`, "3. Tasks"):
//!   * `tasks_blocked_external`: a commit touched a path the task names (drift's
//!     FilePath anchor regex).
//!   * `tasks_maybe_resolved`: a commit subject contains the task's leading verb
//!     and one of its keywords.
//!
//! Neither the `.started` baseline, the change directory, nor commit subjects
//! naming the change matter; the commit list is `git log --since=<created>`.

use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;

/// The oracle's (v3.0.0) task-line regex. Shared with
/// `instructions.rs::parse_apply_tasks` so the apply-mode task list and
/// `task done <id>` number the same lines. A match is a task only if the
/// description is also non-blank — see `is_task_line`.
pub(crate) static CHECKBOX_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*[-*+]\s*\[(.)\]\s*(.+)$").unwrap());
/// Any blank checkbox on a bullet, description or not: what oracle v3.0.0
/// `archive --mark-tasks-complete` flips (probed in the #177 review).
static BLANK_CHECKBOX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*[-*+]\s*\[( )\]").unwrap());
static BACKTICK_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"`([^`]+)`").unwrap());

pub(crate) fn is_done_marker(marker: &str) -> bool {
    matches!(marker, "x" | "X")
}

/// Whether a line is a real task: a checkbox whose description is non-empty
/// after trimming. `- [ ]` and `- [ ] ` (only trailing whitespace) are NOT
/// tasks — the oracle drops them from numbering, progress, and `task done`
/// targeting alike (`CHECKBOX_RE`'s `\s*(.+)` otherwise backtracks and
/// captures a lone trailing space). `parse` and `mark_done` route through
/// here, and `parse_apply_tasks` applies the same regex and
/// empty-description rule, so a `task done <id>` taken from the apply-mode
/// list targets the same line. `mark_all_done` deliberately does not: it
/// numbers nothing, and the oracle flips blank-description checkboxes too.
fn is_task_line(line: &str) -> bool {
    CHECKBOX_RE
        .captures(line)
        .is_some_and(|c| !c[2].trim().is_empty())
}
/// A backtick span looks like a file path if it contains a `/` and a file
/// extension; this filters out commands and prose captured in backticks.
static PATHLIKE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[\w./-]+/[\w./-]+\.\w+$").unwrap());

#[derive(Debug, Clone)]
pub struct Task {
    pub done: bool,
    pub description: String,
    pub files: Vec<String>,
}

/// 一筆碰撞（oracle 3.0.0：兩個清單都一律這四個 key、這個順序）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskCollision {
    pub task_description: String,
    /// 完整 SHA 的前 7 碼（不是 git 的 `%h`）。
    pub commit_sha: String,
    pub commit_subject: String,
    /// author 時間的 UTC 日期 `YYYY-MM-DD`。
    pub commit_date: String,
}

/// oracle 3.0.0 對 checkbox 後文字（已 trim）的解析：legacy `[P] ` 前綴、`N.M` 編號、
/// 緊接編號的 `[after: …]` 前置宣告，以及去掉這兩者後的 description。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskText {
    pub number: Option<String>,
    pub prerequisites: Vec<String>,
    pub legacy_parallel: bool,
    pub description: String,
}

/// 依 oracle 3.0.0 解析 task 文字（規則與實證見 `docs/reverse-engineering/artifact-workflow.md`
/// 的 apply tasks 一節）：
/// 1. 恰好以 `"[P] "`（大小寫敏感、一個空白）開頭才是 legacy parallel，只去掉這 4 bytes；
/// 2. 編號是開頭的 `\d+\.\d+`，後面不能再接 `.` 或數字；
/// 3. `[after: …]` 只在編號後面隔至少一個空白（空白或 tab）時辨識，只吃第一個區塊，
///    內容以 `,` 切開、各自 trim、丟掉空項；
/// 4. 辨識到區塊時 description 改寫成 `"{編號} {其餘.trim_start()}"`（其餘為空時只有編號），
///    否則維持步驟 1 之後的原文。
pub fn parse_task_text(raw: &str) -> TaskText {
    let (legacy_parallel, rest) = match raw.strip_prefix("[P] ") {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let number = task_number(rest);
    let mut text = TaskText {
        number: number.map(str::to_string),
        prerequisites: Vec::new(),
        legacy_parallel,
        description: rest.to_string(),
    };
    let Some(number) = number else {
        return text;
    };
    let after_number = &rest[number.len()..];
    let block = after_number.trim_start_matches([' ', '\t']);
    if block.len() == after_number.len() {
        return text; // 編號後面沒有空白
    }
    let Some(inner) = block.strip_prefix("[after:") else {
        return text;
    };
    let Some(close) = inner.find(']') else {
        return text;
    };
    text.prerequisites = inner[..close]
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect();
    let remaining = inner[close + 1..].trim_start();
    text.description = if remaining.is_empty() {
        number.to_string()
    } else {
        format!("{number} {remaining}")
    };
    text
}

/// 開頭的 `\d+\.\d+`，後面不能再接 `.` 或數字（`1.2.3`、`1.` 都不是編號）。
fn task_number(rest: &str) -> Option<&str> {
    let bytes = rest.as_bytes();
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let major = digits(0);
    if major == 0 || bytes.get(major) != Some(&b'.') {
        return None;
    }
    let minor = digits(major + 1);
    if minor == 0 {
        return None;
    }
    let end = major + 1 + minor;
    match bytes.get(end) {
        Some(b'.') => None,
        _ => Some(&rest[..end]),
    }
}

/// Parse all checkbox tasks from `tasks.md` text.
pub fn parse(md: &str) -> Vec<Task> {
    md.lines()
        .filter(|line| is_task_line(line))
        .map(|line| {
            let c = CHECKBOX_RE
                .captures(line)
                .expect("is_task_line matched CHECKBOX_RE");
            let done = is_done_marker(&c[1]);
            let description = c[2].trim().to_string();
            let files = BACKTICK_RE
                .captures_iter(&description)
                .map(|m| m[1].to_string())
                .filter(|s| PATHLIKE_RE.is_match(s))
                .collect();
            Task {
                done,
                description,
                files,
            }
        })
        .collect()
}

/// oracle 3.0.0 的 task ID 比對：參數必須**逐字**等於某個 1-based 序號的十進位寫法，
/// 所以 `0`、`01`、`+1`、`1.1`、`abc`、前後有空白、超過總數都找不到。
pub fn resolve_task_id(md: &str, arg: &str) -> Option<usize> {
    let total = md.lines().filter(|line| is_task_line(line)).count();
    let n: usize = arg.parse().ok()?;
    (n >= 1 && n <= total && n.to_string() == arg).then_some(n)
}

/// Toggle the 1-based `task_id`-th checkbox (counted across ALL checkboxes
/// in file order, ignoring any `## N.` group headers) from pending to done.
/// Returns the rewritten markdown and the task's raw description text
/// (everything after the checkbox marker, matching the reference CLI's
/// `spectra task done` output). Error wording matches the reference CLI
/// exactly as reverse-engineered against `/Applications/Spectra.app` v2.3.1
/// (the "already done" case was re-probed on v3.0.0 in #172; v3.0.0 reworded
/// the not-found error to `Task {id} not found for change '<name>'`, not yet
/// ported):
/// - `task_id == 0` → "Task ID must be >= 1"
/// - `task_id` exceeds the total checkbox count → "Task {id} not found (total: {n})"
/// - the task is already `[x]`/`[X]` → "Task {id} is already done"
///
/// Any other non-blank marker (`[~]`, `[-]`) returns `Ok` with `md`
/// unchanged: oracle v3.0.0 reports such a task as done yet leaves the
/// content of tasks.md unchanged (probed in #172).
pub fn mark_done(md: &str, task_id: usize) -> Result<(String, String)> {
    if task_id == 0 {
        return Err(anyhow!("Task ID must be >= 1"));
    }
    let lines: Vec<&str> = md.lines().collect();
    let checkbox_line_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| is_task_line(line))
        .map(|(i, _)| i)
        .collect();
    let total = checkbox_line_indices.len();
    if task_id > total {
        return Err(anyhow!("Task {task_id} not found (total: {total})"));
    }
    let line_idx = checkbox_line_indices[task_id - 1];
    let line = lines[line_idx];
    let caps = CHECKBOX_RE
        .captures(line)
        .expect("line matched CHECKBOX_RE above");
    let state = caps
        .get(1)
        .expect("group 1 always captures on a CHECKBOX_RE match");
    // oracle 3.0.0 回報（JSON、human 訊息、touched 紀錄）的是去掉 `[P] ` 與 `[after: …]`
    // 後的 description（探測 p26），tasks.md 本身不改。
    let description = parse_task_text(caps[2].trim()).description;
    if is_done_marker(state.as_str()) {
        return Err(anyhow!("Task {task_id} is already done"));
    }
    if state.as_str() != " " {
        return Ok((md.to_string(), description));
    }

    let mut new_line = line.to_string();
    new_line.replace_range(state.range(), "x");
    let mut owned_lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    owned_lines[line_idx] = new_line;
    let mut new_md = owned_lines.join("\n");
    if md.ends_with('\n') {
        new_md.push('\n');
    }
    Ok((new_md, description))
}

/// Flip every blank (`[ ]`) checkbox on a `-`/`*`/`+` bullet in `md` to done
/// (`[x]`) — including ones with a blank description, which are not tasks
/// for numbering — leaving already-done checkboxes, other markers (`[~]`,
/// `[-]`), and every other line untouched. Matches oracle v3.0.0
/// `spectra archive --mark-tasks-complete`.
pub fn mark_all_done(md: &str) -> String {
    let mut new_md: String = md
        .lines()
        .map(|line| match BLANK_CHECKBOX_RE.captures(line) {
            Some(caps) => {
                let state = caps
                    .get(1)
                    .expect("group 1 always captures on a BLANK_CHECKBOX_RE match");
                let mut new_line = line.to_string();
                new_line.replace_range(state.range(), "x");
                new_line
            }
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if md.ends_with('\n') {
        new_md.push('\n');
    }
    new_md
}

#[derive(Debug, Default)]
pub struct TaskAnalysis {
    pub blocked_external: Vec<TaskCollision>,
    pub maybe_resolved: Vec<TaskCollision>,
}

/// 路徑偵測沿用 drift 的 FilePath anchor regex（未錨定、不需要反引號）。
static TASK_PATH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:src-tauri|src|crates|docs)/[\w./-]+\.(?:rs|ts|svelte|md|toml)").unwrap()
});
/// maybe-resolved 認得的動詞（小寫、完全相等）。
const TASK_VERBS: &[&str] = &[
    "add",
    "implement",
    "fix",
    "update",
    "refactor",
    "remove",
    "delete",
    "create",
    "rename",
    "modify",
];
/// 關鍵字的停用字（探測約 120 個 3 字母以上的常見字）。
const TASK_STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "into", "this", "that", "are", "was", "were",
];

/// `git log --since` 的一個 commit。
struct Commit {
    sha7: String,
    subject: String,
    date: String,
    files: HashSet<String>,
}

/// oracle 的 commit 清單：`git log --since=<created 原字串> --name-only`（git 的 approxidate
/// 會補上現在的時刻，所以 created 當天的 commit 算不算取決於執行時間，與 oracle 相同）。
/// 走訪在第一個早於 cutoff 的 commit 停止；merge commit 沒有檔名、不會 block，但 subject
/// 仍參與 maybe。
fn commits_since(root: &Path, created: &str) -> Vec<Commit> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "log",
            &format!("--since={created}"),
            "--pretty=format:COMMIT|%H|%at|%s",
            "--name-only",
        ])
        .output();
    let Some(output) = output.ok().filter(|o| o.status.success()) else {
        return Vec::new();
    };
    let mut commits: Vec<Commit> = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(rest) = line.strip_prefix("COMMIT|") {
            let mut parts = rest.splitn(3, '|');
            let (Some(sha), Some(at), Some(subject)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let date = at
                .parse::<i64>()
                .ok()
                .and_then(|ts| chrono::DateTime::from_timestamp(ts, 0))
                .map(|t| t.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            commits.push(Commit {
                sha7: sha.chars().take(7).collect(),
                subject: subject.to_string(),
                date,
                files: HashSet::new(),
            });
        } else if !line.is_empty() {
            if let Some(commit) = commits.last_mut() {
                commit.files.insert(line.to_string());
            }
        }
    }
    commits
}

/// 未完成的 task 的描述（去掉 `[P] ` 與 `[after: …]`），依檔案順序；重複的行各算一次。
fn pending_descriptions(md: &str) -> Vec<String> {
    md.lines()
        .filter_map(|line| {
            let c = CHECKBOX_RE.captures(line)?;
            let raw = c[2].trim();
            (!raw.is_empty() && !is_done_marker(&c[1])).then(|| parse_task_text(raw).description)
        })
        .collect()
}

/// 去掉頭尾非 ASCII 英數字元。
fn trim_ascii_alnum(word: &str) -> &str {
    word.trim_matches(|c: char| !c.is_ascii_alphanumeric())
}

/// maybe-resolved 的動詞與關鍵字：跳過開頭全是數字與 `.` 的 token，下一個 token（小寫、
/// 不去標點）必須是動詞；之後的 token 去頭尾非英數、小寫，長度 ≥ 3、全為 ASCII 字母且
/// 不是停用字者為關鍵字。
fn verb_keywords(description: &str) -> Option<(String, Vec<String>)> {
    let mut tokens = description
        .split_whitespace()
        .skip_while(|t| t.chars().all(|c| c.is_ascii_digit() || c == '.'));
    let verb = tokens.next()?.to_lowercase();
    if !TASK_VERBS.contains(&verb.as_str()) {
        return None;
    }
    let keywords: Vec<String> = tokens
        .map(|t| trim_ascii_alnum(t).to_lowercase())
        .filter(|w| {
            w.len() >= 3
                && w.chars().all(|c| c.is_ascii_alphabetic())
                && !TASK_STOPWORDS.contains(&w.as_str())
        })
        .collect();
    (!keywords.is_empty()).then_some((verb, keywords))
}

fn collision(description: &str, commit: &Commit) -> TaskCollision {
    TaskCollision {
        task_description: description.to_string(),
        commit_sha: commit.sha7.clone(),
        commit_subject: commit.subject.clone(),
        commit_date: commit.date.clone(),
    }
}

/// 未完成 task 與 `created` 之後的 commit 的碰撞（oracle 3.0.0）。沒有 `created` 時不跑
/// `git log`、兩個清單都是空的。每個 task 各自回報 log 順序中**第一個**符合的 commit；
/// 同一個 task 可以同時出現在兩個清單。
pub fn analyze(root: &Path, tasks_md: &str, created: Option<&str>) -> TaskAnalysis {
    let mut analysis = TaskAnalysis::default();
    let Some(created) = created else {
        return analysis;
    };
    let pending = pending_descriptions(tasks_md);
    if pending.is_empty() {
        return analysis;
    }
    let commits = commits_since(root, created);
    for description in pending {
        let paths: HashSet<&str> = TASK_PATH_RE
            .find_iter(&description)
            .map(|m| m.as_str())
            .collect();
        if !paths.is_empty() {
            if let Some(commit) = commits
                .iter()
                .find(|c| paths.iter().any(|p| c.files.contains(*p)))
            {
                analysis
                    .blocked_external
                    .push(collision(&description, commit));
            }
        }
        if let Some((verb, keywords)) = verb_keywords(&description) {
            if let Some(commit) = commits.iter().find(|c| {
                let subject = c.subject.to_lowercase();
                subject.contains(&verb) && keywords.iter().any(|k| subject.contains(k.as_str()))
            }) {
                analysis
                    .maybe_resolved
                    .push(collision(&description, commit));
            }
        }
    }
    analysis
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vk(desc: &str) -> Option<(String, Vec<String>)> {
        verb_keywords(desc)
    }

    /// oracle 3.0.0 maybe-resolved 的動詞與關鍵字（W7d SPEC §A6，探測 p24–p28）。
    #[test]
    fn verb_keywords_follow_the_oracle_rules() {
        let words = |list: &[&str]| list.iter().map(|w| w.to_string()).collect::<Vec<_>>();
        assert_eq!(
            vk("1.2 Add login flow for the user"),
            Some(("add".into(), words(&["login", "flow", "user"])))
        );
        // 開頭只由數字與 `.` 組成的 token 都略過；`1)`、`T1` 不算。
        assert_eq!(
            vk("1 2 . Fix parser"),
            Some(("fix".into(), words(&["parser"])))
        );
        assert_eq!(vk("1) Fix parser"), None);
        // 動詞不去標點、不認屈折形與同義字。
        assert_eq!(vk("1.1 Add: login"), None);
        assert_eq!(vk("1.1 Adds login"), None);
        assert_eq!(vk("1.1 Move login"), None);
        // 關鍵字：去頭尾非英數、≥3、全 ASCII 字母、非停用字；沒有關鍵字就不算。
        assert_eq!(
            vk("1.1 Update **login** (auth) login-endpoint 3rd café"),
            Some(("update".into(), words(&["login", "auth", "caf"])))
        );
        assert_eq!(vk("1.1 Add UI"), None);
        assert_eq!(vk("1.1 新增 登入"), None);
    }

    #[test]
    fn task_paths_use_the_drift_anchor_regex() {
        let found: Vec<&str> = TASK_PATH_RE
            .find_iter("see `docs/a.md`, ./src/b.rs, lib/c.ts, src/g.tsx, xdocs/d.md tests/e.rs")
            .map(|m| m.as_str())
            .collect();
        assert_eq!(found, ["docs/a.md", "src/b.rs", "src/g.ts", "docs/d.md"]);
    }

    /// (原文, number, prerequisites, legacy `[P]`, description)
    type TextCase<'a> = (&'a str, Option<&'a str>, &'a [&'a str], bool, &'a str);

    /// oracle 3.0.0 `instructions apply --json` 的 `number`／`prerequisites`／`description`
    /// （docs/reverse-engineering/artifact-workflow.md 的 apply tasks 一節，探測 t01–t11、
    /// h01–h09、u01–u08）。
    #[test]
    fn task_text_matches_oracle_numbers_after_blocks_and_legacy_p() {
        let cases: &[TextCase] = &[
            ("1.1 dotted", Some("1.1"), &[], false, "1.1 dotted"),
            ("1. trailing dot", None, &[], false, "1. trailing dot"),
            ("2 bare int", None, &[], false, "2 bare int"),
            ("1.2.3 triple", None, &[], false, "1.2.3 triple"),
            ("T1 letter", None, &[], false, "T1 letter"),
            ("**1.4** bold", None, &[], false, "**1.4** bold"),
            ("3.1: colon", Some("3.1"), &[], false, "3.1: colon"),
            ("3.2) paren", Some("3.2"), &[], false, "3.2) paren"),
            ("10.20 big", Some("10.20"), &[], false, "10.20 big"),
            (
                "1.2 [after: 1.1] second",
                Some("1.2"),
                &["1.1"],
                false,
                "1.2 second",
            ),
            (
                "1.2 second [after: 1.1]",
                Some("1.2"),
                &[],
                false,
                "1.2 second [after: 1.1]",
            ),
            (
                "[after: 1.2] 1.1 lead",
                None,
                &[],
                false,
                "[after: 1.2] 1.1 lead",
            ),
            (
                "1.3 [After: 1.1] x",
                Some("1.3"),
                &[],
                false,
                "1.3 [After: 1.1] x",
            ),
            ("1.4 [after:1.1] y", Some("1.4"), &["1.1"], false, "1.4 y"),
            (
                "1.5 [after: 1.1][after: 1.2] e",
                Some("1.5"),
                &["1.1"],
                false,
                "1.5 [after: 1.2] e",
            ),
            ("1.1\t[after: 1.2] a", Some("1.1"), &["1.2"], false, "1.1 a"),
            ("1.2  [after: 1.3] b", Some("1.2"), &["1.3"], false, "1.2 b"),
            ("1.2 [after: 1.1]", Some("1.2"), &["1.1"], false, "1.2"),
            ("1.2 [after: ] b", Some("1.2"), &[], false, "1.2 b"),
            (
                "1.2 [after:  1.1 ] b",
                Some("1.2"),
                &["1.1"],
                false,
                "1.2 b",
            ),
            (
                "1.2 [after: 1.1 1.2] b",
                Some("1.2"),
                &["1.1 1.2"],
                false,
                "1.2 b",
            ),
            (
                "1.2 [after: 1.1, 1.1] b",
                Some("1.2"),
                &["1.1", "1.1"],
                false,
                "1.2 b",
            ),
            (
                "1.2 [after: setup, , 1] b",
                Some("1.2"),
                &["setup", "1"],
                false,
                "1.2 b",
            ),
            (
                "1.2[after: 1.1] b",
                Some("1.2"),
                &[],
                false,
                "1.2[after: 1.1] b",
            ),
            ("[P] 1.2 b", Some("1.2"), &[], true, "1.2 b"),
            (
                "[P] 1.3 [after: 1.1] third",
                Some("1.3"),
                &["1.1"],
                true,
                "1.3 third",
            ),
            ("[P] nonum a", None, &[], true, "nonum a"),
            ("[P]1.1 a", None, &[], false, "[P]1.1 a"),
            ("[P]  1.2 b", None, &[], true, " 1.2 b"),
            ("1.1 a [P]", Some("1.1"), &[], false, "1.1 a [P]"),
            ("[p] lower", None, &[], false, "[p] lower"),
        ];
        for (raw, number, prereqs, legacy, description) in cases {
            let expected = TaskText {
                number: number.map(str::to_string),
                prerequisites: prereqs.iter().map(|p| p.to_string()).collect(),
                legacy_parallel: *legacy,
                description: description.to_string(),
            };
            assert_eq!(parse_task_text(raw), expected, "{raw:?}");
        }
    }

    /// oracle 3.0.0 `task done` 的 `task_desc`（JSON、human 訊息與 touched 紀錄）去掉 `[P] `
    /// 與緊接編號的 `[after: …]`（探測 p26）。
    #[test]
    fn mark_done_reports_the_normalized_description() {
        let md =
            "- [ ] 1.1 first\n- [ ] 1.2 [after: 1.1] second\n- [ ] [P] 1.3 [after: 1.1] third\n";
        assert_eq!(mark_done(md, 2).unwrap().1, "1.2 second");
        assert_eq!(mark_done(md, 3).unwrap().1, "1.3 third");
    }

    #[test]
    fn parses_checkboxes_and_done_state() {
        let md = "## Tasks\n- [ ] 1.1 pending\n- [x] 1.2 done\n- [X] 1.3 also done\nnot a task\n";
        let tasks = parse(md);
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks.iter().filter(|t| t.done).count(), 2);
        assert_eq!(tasks[0].description, "1.1 pending");
    }

    #[test]
    fn empty_description_checkboxes_are_not_tasks_across_parse_and_mark_done() {
        // A `- [ ] ` line (only trailing whitespace after the marker) is not a
        // task: the oracle drops it from numbering, so `parse` (`list`/`drift`
        // progress) and `mark_done` (`task done <id>`) must agree,
        // otherwise following an id from one into the other marks the wrong
        // line. Probed against Spectra.app v2.3.1: `task done 2` on this input
        // targets "second real task", not the blank line.
        let md = "- [x] first task\n- [ ] \n- [ ] second real task\n";
        let tasks = parse(md);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[1].description, "second real task");

        let (new_md, desc) = mark_done(md, 2).unwrap();
        assert_eq!(desc, "second real task");
        assert_eq!(new_md, "- [x] first task\n- [ ] \n- [x] second real task\n");
    }

    #[test]
    fn parse_counts_star_and_plus_bullets_and_only_x_marks_done() {
        // Probed against Spectra.app v3.0.0 (issue #172): `*`/`+` bullets are
        // tasks, and any marker other than x/X (`[~]`, `[-]`) counts as pending.
        let md = "- [x] a\n* [ ] b\n+ [ ] c\n* [x] d\n+ [X] e\n- [~] f\n- [-] g\n  + [ ] h\n";
        let tasks = parse(md);
        let states: Vec<(&str, bool)> = tasks
            .iter()
            .map(|t| (t.description.as_str(), t.done))
            .collect();
        assert_eq!(
            states,
            vec![
                ("a", true),
                ("b", false),
                ("c", false),
                ("d", true),
                ("e", true),
                ("f", false),
                ("g", false),
                ("h", false),
            ]
        );
    }

    #[test]
    fn parse_drops_ordered_list_padded_and_empty_markers_like_the_oracle() {
        // Oracle v3.0.0 drops these too; counting them would diverge (#172).
        let md = "1. [ ] a\n1) [ ] b\n- [ x] c\n- [] d\n";
        assert!(parse(md).is_empty());
    }

    #[test]
    fn mark_done_targets_star_and_plus_bullets_by_the_same_numbering_as_parse() {
        let md = "- [x] 1.1 a\n* [ ] 1.2 b\n+ [ ] 1.3 c\n";
        let (new_md, desc) = mark_done(md, 3).unwrap();
        assert_eq!(desc, "1.3 c");
        assert_eq!(new_md, "- [x] 1.1 a\n* [ ] 1.2 b\n+ [x] 1.3 c\n");
    }

    #[test]
    fn mark_done_rejects_an_already_done_plus_bullet() {
        let err = mark_done("+ [x] a\n", 1).unwrap_err();
        assert_eq!(err.to_string(), "Task 1 is already done");
    }

    #[test]
    fn mark_done_on_a_non_checkbox_marker_reports_done_without_rewriting() {
        // Oracle v3.0.0: `task done` on `- [~]` exits 0 with status "done" but
        // leaves tasks.md byte-identical.
        let md = "- [x] a\n- [~] b\n";
        let (new_md, desc) = mark_done(md, 2).unwrap();
        assert_eq!(desc, "b");
        assert_eq!(new_md, md);
    }

    #[test]
    fn mark_all_done_flips_blank_description_checkboxes_like_the_oracle() {
        // Oracle v3.0.0 `archive --mark-tasks-complete` flips these too, even
        // though they are not tasks for numbering (`is_task_line`).
        let md = "- [ ] a\n- [ ] \n* [ ]   \n- [ ]\n* [ ] b\n";
        assert_eq!(
            mark_all_done(md),
            "- [x] a\n- [x] \n* [x]   \n- [x]\n* [x] b\n"
        );
    }

    #[test]
    fn mark_all_done_leaves_non_bullet_checkboxes_like_the_oracle() {
        // Same input probed on oracle v3.0.0 `archive --mark-tasks-complete`.
        let md = "1. [ ] a\n1) [ ] b\n[ ] c\ntext [ ] d\n- [ x] e\n- [~] f\n    - [ ] g\n- [ ] h\n";
        assert_eq!(
            mark_all_done(md),
            "1. [ ] a\n1) [ ] b\n[ ] c\ntext [ ] d\n- [ x] e\n- [~] f\n    - [x] g\n- [x] h\n"
        );
    }

    #[test]
    fn mark_all_done_flips_every_bullet_style_but_leaves_other_markers() {
        // Oracle v3.0.0 `archive --mark-tasks-complete` on the same input.
        let md = "- [ ] a\n* [ ] b\n+ [ ] c\n- [~] d\n- [-] e\n  + [ ] f\n";
        assert_eq!(
            mark_all_done(md),
            "- [x] a\n* [x] b\n+ [x] c\n- [~] d\n- [-] e\n  + [x] f\n"
        );
    }

    #[test]
    fn extracts_only_pathlike_backtick_spans() {
        let md = "- [ ] edit `src/foo/bar.rs` and run `cargo build` touching `a/b/c.py`";
        let tasks = parse(md);
        assert_eq!(tasks[0].files, vec!["src/foo/bar.rs", "a/b/c.py"]);
    }

    /// 讀不到 git 歷史或沒有 `created` 時兩個清單都是空的（drift 另外把前者標成
    /// `git unavailable`）。
    #[test]
    fn analyze_is_empty_without_history_or_created() {
        let md = "- [ ] 1.1 Fix `docs/a.md` parser";
        let nowhere = std::path::Path::new("/nonexistent");
        let a = analyze(nowhere, md, Some("2026-01-01"));
        assert!(a.blocked_external.is_empty() && a.maybe_resolved.is_empty());
        let a = analyze(nowhere, md, None);
        assert!(a.blocked_external.is_empty() && a.maybe_resolved.is_empty());
    }

    #[test]
    fn mark_done_toggles_the_nth_checkbox_across_group_headers() {
        // Matches the reference CLI's real scaffold shape: task_id counts
        // checkboxes 1-based across ALL groups, ignoring the "## N." headers.
        let md =
            "## 1. Group\n\n- [ ] 1.1 first\n- [ ] 1.2 second\n\n## 2. Group\n\n- [ ] 2.1 third\n";
        let (new_md, desc) = mark_done(md, 3).unwrap();
        assert_eq!(desc, "2.1 third");
        assert!(new_md.contains("- [x] 2.1 third"));
        // Untouched lines (including the other group's checkboxes) are preserved verbatim.
        assert!(new_md.contains("- [ ] 1.1 first"));
        assert!(new_md.contains("- [ ] 1.2 second"));
    }

    #[test]
    fn mark_done_preserves_trailing_newline_and_indentation() {
        let md = "  - [ ] indented task\n";
        let (new_md, _) = mark_done(md, 1).unwrap();
        assert_eq!(new_md, "  - [x] indented task\n");
    }

    #[test]
    fn mark_done_rejects_zero() {
        let err = mark_done("- [ ] a\n", 0).unwrap_err();
        assert_eq!(err.to_string(), "Task ID must be >= 1");
    }

    #[test]
    fn mark_done_rejects_out_of_range() {
        let err = mark_done("- [ ] a\n- [ ] b\n", 5).unwrap_err();
        assert_eq!(err.to_string(), "Task 5 not found (total: 2)");
    }

    #[test]
    fn mark_done_rejects_already_done() {
        let err = mark_done("- [x] a\n", 1).unwrap_err();
        assert_eq!(err.to_string(), "Task 1 is already done");
    }

    #[test]
    fn mark_all_done_flips_every_pending_checkbox() {
        let md = "## 1. Group\n\n- [ ] a\n- [x] b\n- [ ] c\n";
        assert_eq!(
            mark_all_done(md),
            "## 1. Group\n\n- [x] a\n- [x] b\n- [x] c\n"
        );
    }

    #[test]
    fn mark_all_done_is_a_noop_when_nothing_is_pending() {
        let md = "- [x] a\n- [x] b\n";
        assert_eq!(mark_all_done(md), md);
    }

    #[test]
    fn mark_all_done_preserves_non_checkbox_lines() {
        let md = "# tasks\n\nsome prose\n\n- [ ] a\n";
        assert_eq!(mark_all_done(md), "# tasks\n\nsome prose\n\n- [x] a\n");
    }

    #[test]
    fn mark_all_done_does_not_add_a_trailing_newline_when_the_input_has_none() {
        let md = "- [ ] a";
        assert_eq!(mark_all_done(md), "- [x] a");
    }
}
