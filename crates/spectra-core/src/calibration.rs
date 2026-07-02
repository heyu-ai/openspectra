//! Empirically reverse-engineered scoring constants for `spectra drift`.
//!
//! The closed-source `spectra` binary (v2.3.1) computes a drift score from
//! three contributing dimensions (Time, Structure, Tasks; Environment is
//! display-only). The exact magic constants below were recovered by running
//! the reference binary as a golden oracle over real changes and fitting the
//! observed `score`/`status` outputs. See `docs/reverse-engineering/drift.md`
//! for the full method.
//!
//! ## Structure score — exact formula (recovered via the calibration harness)
//!
//! Structure score is NOT a pure function of decay: it is category-weighted.
//! A broken **CliFlag** raises the score; a broken **FilePath** only raises the
//! decay (it does not add on its own). Recovered by sweeping the oracle with
//! synthetic changes (`scripts/calibrate-structure.py`) over the (broken
//! FilePath count × broken CliFlag count × total) space, then verified against
//! every real golden:
//! ```text
//!   decay = broken / total
//!   D = 0 if decay < 10% | 1 if 10% <= decay < 30% | 2 if decay >= 30%
//!   score = min(2*D + 3,  2*D + broken_cliflag_count)
//! ```
//! Golden checks (all exact): 0/16 cf0 -> 0; 3/40 cf3 -> 3; 9/29 cf9 -> 7;
//! 12/25 cf12 -> 7. Category isolation (harness): 1/7 FilePath -> 2 but
//! 1/7 CliFlag -> 3; 3/40 FilePath -> 0 but 3/40 CliFlag -> 3. The earlier
//! decay-only table happened to fit the goldens only because every golden's
//! broken anchors were CliFlags (so `min` saturated at the `2D+3` cap).
//! In practice only FilePath and CliFlag are ever broken on a committed change
//! (Function/Symbol self-match the tracked design.md), so "non-CliFlag broken"
//! == FilePath here.
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

/// Structure dimension score.
///
/// `broken` is the total broken-anchor count, `broken_cliflags` how many of
/// those are CliFlags, `total` the extracted anchor count. The score is
/// `min(2D + 3, 2D + broken_cliflags)` where `D` is the decay band (see the
/// module-level table). FilePath (and other non-CliFlag) broken anchors only
/// contribute through `D`; CliFlags additionally raise the additive term.
pub fn structure_score(broken: usize, broken_cliflags: usize, total: usize) -> i64 {
    if total == 0 || broken == 0 {
        return 0;
    }
    let decay = broken as f64 / total as f64;
    let d: i64 = if decay < STRUCTURE_DECAY_LOW {
        0
    } else if decay < STRUCTURE_DECAY_HIGH {
        1
    } else {
        2
    };
    (2 * d + 3).min(2 * d + broken_cliflags as i64)
}

/// Decay-band boundaries for the Structure score (recovered exactly: `10.0%` is
/// D1, `9.1%` is D0; `30.0%` is D2). `STRUCTURE_DECAY_HIGH` empirically equals
/// [`HEAVY_DECAY_THRESHOLD`], but they are kept as separate constants on purpose:
/// one is the score-band edge, the other the severity short-circuit, and the
/// oracle treats exactly 30% differently for each (D2 for the score, but *not*
/// forced `heavy` — verified by probe). Tuning one must not silently move the other.
pub const STRUCTURE_DECAY_LOW: f64 = 0.10;
pub const STRUCTURE_DECAY_HIGH: f64 = 0.30;

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
    fn structure_score_reproduces_real_goldens() {
        // (broken, broken_cliflags, total) -> score, from the 4 real golden JSONs.
        assert_eq!(structure_score(0, 0, 16), 0); // add-token-economy
        assert_eq!(structure_score(3, 3, 40), 3); // enhance-d5 (D0, cap 3)
        assert_eq!(structure_score(9, 9, 29), 7); // mycelium  (D2, cap 7)
        assert_eq!(structure_score(12, 12, 25), 7); // pr-control-log (D2, cap 7)
    }

    #[test]
    fn structure_score_is_category_weighted() {
        // Same (broken, total), different category composition -> different score
        // (harness-verified: a broken CliFlag weighs more than a broken FilePath).
        assert_eq!(structure_score(1, 0, 7), 2); // 1 FilePath broken, 14% -> D1 -> 2D=2
        assert_eq!(structure_score(1, 1, 7), 3); // 1 CliFlag broken  -> 2D+1=3
        assert_eq!(structure_score(3, 0, 40), 0); // 3 FilePath, 7.5% -> D0 -> 0
        assert_eq!(structure_score(3, 3, 40), 3); // 3 CliFlag        -> min(3,3)=3
    }

    #[test]
    fn structure_score_decay_bands_and_cap() {
        // D0 (<10%): 1/11 = 9.09% -> D0, cf1 -> min(0+3, 0+1) = 1.
        assert_eq!(structure_score(1, 1, 11), 1);
        // D1 boundary at exactly 10%: 1/10 -> D1, cf1 -> min(5, 2+1)=3.
        assert_eq!(structure_score(1, 1, 10), 3);
        // D2 boundary at 30%: 3/10 -> D2, cf3 -> min(7, 4+3)=7.
        assert_eq!(structure_score(3, 3, 10), 7);
        // CliFlag additive is capped by 2D+3: many flags at D1 saturate at 5.
        assert_eq!(structure_score(6, 6, 24), 5); // 25% -> D1, min(5, 2+6)=5
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
        assert_eq!(
            primary_recommendation("heavy", "c"),
            "spectra archive c --skip-specs"
        );
    }
}
