//! Empirically reverse-engineered scoring constants for `spectra drift`.
//!
//! The closed-source `spectra` binary (v2.3.1) computes a drift score from
//! three contributing dimensions (Time, Structure, Tasks; Environment is
//! display-only). The exact magic constants below were recovered by running
//! the reference binary as a golden oracle over real changes and fitting the
//! observed `score`/`status` outputs. See `docs/reverse-engineering/drift.md`
//! for the full method.
//!
//! ## Field-sample calibration table (oracle = spectra 2.3.1)
//!
//! Structure (score is a function of anchor *decay* = broken / total):
//! ```text
//!   broken/total  decay%   score
//!   0/*            0.0       0
//!   1/17           5.9       0
//!   3/46           6.5       1
//!   3/40           7.5       3
//!   4/16          25.0       5
//!   5/19          26.3       5
//!   9/29          31.0       7
//!  12/25          48.0       7
//! ```
//!
//! Time (score is a function of days since `created`):
//! ```text
//!   days   status        score
//!   0..=5  fresh          0
//!   7..=19 aging          1
//!  25..=36 stale          2
//!   (none) no created     0
//! ```
//!
//! NOTE: Several boundaries are interpolated between sparse samples and are
//! marked `CALIBRATE`. A controlled calibration harness (synthetic changes fed
//! to the oracle) is the way to pin them exactly; the architecture does not
//! depend on the precise values.

/// Maximum number of design anchors checked, per `ANCHOR_CAP` in
/// `spectra_core::drift` (recovered verbatim from the binary).
pub const ANCHOR_CAP: usize = 50;

/// Whether the Tasks-dimension collision detectors (blocked / maybe-resolved)
/// are calibrated. Held `false`: every captured oracle sample was `0 blocked,
/// 0 maybe-done`, giving no positive case to fit the firing predicates against.
/// Keeping detection off matches 100% of observed oracle behavior and avoids
/// shipping false positives. See `tasks::analyze` and the RE doc.
pub const TASKS_DETECTION_CALIBRATED: bool = false;

/// Structure dimension score from broken-anchor decay (broken / total).
/// Reproduces every observed field sample; inner boundaries are CALIBRATE.
pub fn structure_score(broken: usize, total: usize) -> i64 {
    if total == 0 || broken == 0 {
        return 0;
    }
    let decay = broken as f64 / total as f64;
    if decay < 0.06 {
        0
    } else if decay < 0.07 {
        1
    } else if decay < 0.25 {
        3
    } else if decay < 0.30 {
        5
    } else {
        7
    }
}

/// The "heavy" severity short-circuit: anchor decay over this fraction forces
/// `heavy` regardless of total score (recovered: "anchor decay >30%").
pub const HEAVY_DECAY_THRESHOLD: f64 = 0.30;

/// Time dimension: classify days-since-`created` into the oracle's status word
/// and contributing score. `None` days (no/invalid created date) handled by caller.
pub fn time_bucket(days: i64) -> (&'static str, i64) {
    // CALIBRATE: fresh/aging boundary observed between 5 and 7 days;
    // aging/stale between 19 and 25; stale/abandoned unobserved (guess 60).
    if days < 7 {
        ("fresh", 0)
    } else if days < 21 {
        ("aging", 1)
    } else if days < 60 {
        ("stale", 2)
    } else {
        ("abandoned", 3)
    }
}

/// Tasks dimension score from the count of (blocked + maybe-resolved) tasks.
/// CALIBRATE: every field sample was 0/0 -> 0, so only the zero case is
/// verified against the oracle. The non-zero mapping mirrors `structure_score`'s
/// odd-number ladder as a placeholder until a positive sample is captured.
pub fn tasks_score(blocked: usize, maybe_resolved: usize) -> i64 {
    match blocked + maybe_resolved {
        0 => 0,
        1 => 1,
        2 => 3,
        3 => 5,
        _ => 7,
    }
}

/// Severity band. `heavy` when total > 8 OR anchor decay exceeds the threshold.
pub fn severity(total_score: i64, structure_decay: f64) -> &'static str {
    if total_score > 8 || structure_decay > HEAVY_DECAY_THRESHOLD {
        "heavy"
    } else if total_score >= 4 {
        "medium"
    } else {
        "light"
    }
}

/// Single copy-pasteable next command, chosen by severity (recovered mapping).
pub fn primary_recommendation(severity: &str, change: &str) -> String {
    match severity {
        "light" => format!("/spectra-apply {change}"),
        "medium" => format!("/spectra-ingest {change}"),
        _ => format!("spectra archive {change} --skip-specs"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structure_score_reproduces_field_samples() {
        // (broken, total) -> expected score, from the oracle calibration table.
        assert_eq!(structure_score(0, 16), 0);
        assert_eq!(structure_score(1, 17), 0); // 5.9%
        assert_eq!(structure_score(3, 46), 1); // 6.5%
        assert_eq!(structure_score(3, 40), 3); // 7.5%
        assert_eq!(structure_score(4, 16), 5); // 25%
        assert_eq!(structure_score(5, 19), 5); // 26.3%
        assert_eq!(structure_score(9, 29), 7); // 31%
        assert_eq!(structure_score(12, 25), 7); // 48%
    }

    #[test]
    fn time_bucket_reproduces_field_samples() {
        assert_eq!(time_bucket(0), ("fresh", 0));
        assert_eq!(time_bucket(5), ("fresh", 0));
        assert_eq!(time_bucket(7), ("aging", 1));
        assert_eq!(time_bucket(19), ("aging", 1));
        assert_eq!(time_bucket(25), ("stale", 2));
        assert_eq!(time_bucket(36), ("stale", 2));
    }

    #[test]
    fn severity_bands_and_decay_shortcut() {
        assert_eq!(severity(0, 0.0), "light");
        assert_eq!(severity(3, 0.0), "light");
        assert_eq!(severity(5, 0.0), "medium"); // enhance-d5: total 5
        assert_eq!(severity(8, 0.0), "medium");
        assert_eq!(severity(9, 0.18), "heavy"); // mycelium: total 9
        // decay over 30% forces heavy even with a low total score.
        assert_eq!(severity(2, 0.48), "heavy");
    }

    #[test]
    fn recommendation_by_severity() {
        assert_eq!(primary_recommendation("light", "c"), "/spectra-apply c");
        assert_eq!(primary_recommendation("medium", "c"), "/spectra-ingest c");
        assert_eq!(primary_recommendation("heavy", "c"), "spectra archive c --skip-specs");
    }
}
