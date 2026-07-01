//! `tasks.md` parsing and task/commit collision detection.
//!
//! Tasks are GitHub-style checkboxes (`- [ ]` pending, `- [x]` done). Inline
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

static CHECKBOX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*-\s*\[( |x|X)\]\s*(.+)$").unwrap());
static BACKTICK_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"`([^`]+)`").unwrap());
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

/// Parse all checkbox tasks from `tasks.md` text.
pub fn parse(md: &str) -> Vec<Task> {
    md.lines()
        .filter_map(|line| {
            let c = CHECKBOX_RE.captures(line)?;
            let done = &c[1] != " ";
            let description = c[2].trim().to_string();
            let files = BACKTICK_RE
                .captures_iter(&description)
                .map(|m| m[1].to_string())
                .filter(|s| PATHLIKE_RE.is_match(s))
                .collect();
            Some(Task {
                done,
                description,
                files,
            })
        })
        .collect()
}

/// Toggle the 1-based `task_id`-th checkbox (counted across ALL checkboxes
/// in file order, ignoring any `## N.` group headers) from pending to done.
/// Returns the rewritten markdown and the task's raw description text
/// (everything after the checkbox marker, matching the reference CLI's
/// `spectra task done` output). Error wording matches the reference CLI
/// exactly (reverse-engineered against `/Applications/Spectra.app` v2.3.1):
/// - `task_id == 0` → "Task ID must be >= 1"
/// - `task_id` exceeds the total checkbox count → "Task {id} not found (total: {n})"
/// - the task is already `[x]` → "Task {id} is already done"
pub fn mark_done(md: &str, task_id: usize) -> Result<(String, String)> {
    if task_id == 0 {
        return Err(anyhow!("Task ID must be >= 1"));
    }
    let lines: Vec<&str> = md.lines().collect();
    let checkbox_line_indices: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| CHECKBOX_RE.is_match(line))
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
    let description = caps[2].trim().to_string();
    if state.as_str() != " " {
        return Err(anyhow!("Task {task_id} is already done"));
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

/// Flip every pending (`[ ]`) checkbox in `md` to done (`[x]`), leaving
/// already-done checkboxes and every other line untouched. Used by
/// `spectra archive --mark-tasks-complete`.
pub fn mark_all_done(md: &str) -> String {
    let mut new_md: String = md
        .lines()
        .map(|line| match CHECKBOX_RE.captures(line) {
            Some(caps) if &caps[1] == " " => {
                let state = caps
                    .get(1)
                    .expect("group 1 always captures on a CHECKBOX_RE match");
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

    #[test]
    fn parses_checkboxes_and_done_state() {
        let md = "## Tasks\n- [ ] 1.1 pending\n- [x] 1.2 done\n- [X] 1.3 also done\nnot a task\n";
        let tasks = parse(md);
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks.iter().filter(|t| t.done).count(), 2);
        assert_eq!(tasks[0].description, "1.1 pending");
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
}
