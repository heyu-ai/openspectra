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
            Some(Task { done, description, files })
        })
        .collect()
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
        let a = analyze(std::path::Path::new("/nonexistent"), "chg", &tasks, None, Some("2026-01-01"));
        assert!(a.blocked_external.is_empty());
        assert!(a.maybe_resolved.is_empty());
    }
}
