//! `tasks.md` parsing and task/commit collision detection.
//!
//! Tasks are checkboxes on a `-`, `*`, or `+` bullet; only `[x]`/`[X]` is
//! done, any other single-character marker (`[ ]`, `[~]`, …) is pending. Inline
//! backtick spans hold the file paths a task touches. Drift flags pending tasks
//! that collide with work that happened outside the change:
//!   * `tasks_blocked_external`: a referenced file was modified by a commit
//!     after the change's `.started` baseline SHA.
//!   * `tasks_maybe_resolved`: the task appears to have been done elsewhere — a
//!     commit subject since `created` names this change or a file it touches.
//!
//! CALIBRATION NOTE: every captured oracle sample (including in-progress changes
//! with pending tasks and many intervening commits) reported `0 blocked,
//! 0 maybe-done`. With no positive oracle sample, the exact firing predicates
//! cannot be verified, so both detectors are deliberately STRICT here to match
//! the observed all-zero field behavior rather than emit false positives. See
//! `docs/reverse-engineering/drift.md` for the open question.

use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;
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

#[derive(Debug, Clone, Serialize)]
pub struct TaskCollision {
    pub task_description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_subject: Option<String>,
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

/// Analyze pending tasks for external collisions.
/// * `change_name` scopes "external" commits (those whose subject names the change are its own work).
/// * `started_sha` is the `.started` baseline (blocked detection is skipped when absent).
/// * `created` is the `YYYY-MM-DD` date used as the lower bound for commit subjects.
pub fn analyze(
    root: &Path,
    change_name: &str,
    tasks: &[Task],
    started_sha: Option<&str>,
    created: Option<&str>,
) -> TaskAnalysis {
    let analysis = TaskAnalysis::default();
    let pending: Vec<&Task> = tasks.iter().filter(|t| !t.done).collect();

    // Every captured oracle sample reported `0 blocked, 0 maybe-done`, including
    // in-progress changes with many pending tasks and 100+ intervening commits.
    // With no positive sample the real firing predicates cannot be verified, and
    // every heuristic tried (file-touched-since-baseline, file-missing, commit
    // subject naming the change) produced false positives the oracle never emits.
    // Detection therefore stays OFF until a positive oracle sample is captured to
    // calibrate against; flip `TASKS_DETECTION_CALIBRATED` once it is. The parser
    // and data model above are exercised regardless (used by `list` task counts).
    if !crate::calibration::TASKS_DETECTION_CALIBRATED || pending.is_empty() {
        return analysis;
    }

    // --- uncalibrated heuristics (disabled by the gate above) ---------------
    let _ = (root, change_name, started_sha, created);
    analysis
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn analyze_is_conservative_zero_until_calibrated() {
        let md = "- [ ] 1.1 do `missing/file.rs`";
        let tasks = parse(md);
        let a = analyze(
            std::path::Path::new("/nonexistent"),
            "chg",
            &tasks,
            None,
            Some("2026-01-01"),
        );
        assert!(a.blocked_external.is_empty());
        assert!(a.maybe_resolved.is_empty());
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
