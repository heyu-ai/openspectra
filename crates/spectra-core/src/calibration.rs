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
//! In the oracle calibration fixtures only FilePath and CliFlag can remain
//! broken on a committed change (Function/Symbol self-match the tracked
//! design.md), so "non-CliFlag broken" == FilePath there.
//!
//! CliFlags and unresolvable Functions were briefly reported as unresolved and
//! withheld from this formula (#83); #119 restored them as broken, which makes
//! the golden triples above *reachable* again — under #83 no resolver run could
//! yield a non-zero `broken_cliflags`. That is the whole of what it
//! establishes. The golden test asserts this pure function on hand-copied
//! literals, and the four fixtures record the oracle's **output only** — their
//! input repos and `design.md` files were never captured, so extraction and
//! resolution have never run on them and cannot without new snapshots (#132).
//! One resolver divergence remains: a missing FilePath that did not exist at
//! the change baseline is unresolved (`forward reference`) rather than broken.
//!
//! Time (score is a function of days since `created`) — boundaries now pinned
//! exactly (previously interpolated from sparse field samples):
//! ```text
//!   days     status        score
//!   0..=6    fresh          0
//!   7..=21   aging          1
//!  22..=60   stale          2
//!  61..      abandoned      4   (score ladder skips 3)
//!   (none)   no created     0
//! ```
//!
//! Every boundary and the abandoned score above were pinned exactly by sweeping
//! synthetic changes with controlled `created` dates through the oracle
//! (`scripts/calibrate-time.py --mode boundaries`): transitions at 7, 22, and
//! 61 days. Two off-by-one guesses and a wrong `abandoned` score were corrected
//! this way (the old table read `<21`/`<60` and scored abandoned `3`). See the
//! Time section of `docs/reverse-engineering/drift.md`.

/// Anchor count up to which every extracted design anchor is checked. Sets
/// exclude nothing at `50`; at `51` the oracle switches to the sampled regime
/// below. Boundary pinned by probe (pure-FilePath designs: `50` → `50/50`,
/// `51` → `12/12`).
pub const ANCHOR_CAP: usize = 50;

/// Anchors kept *per category* once the extracted set exceeds [`ANCHOR_CAP`].
/// The oracle does not truncate the over-cap set — it keeps an evenly spaced
/// sample of each category at indices `i * n / 12`. See `anchors::extract`.
///
/// The reported denominator above the cap is therefore `sum(min(n_category,
/// 12))`, **not** a multiple of this value: a category with 12 or fewer
/// candidates survives whole. Probed — 45 Symbol + 10 CliFlag reports `22/22`,
/// and 60 FilePath + 5 Function reports `17`. Pinned end-to-end against the
/// oracle by `scripts/calibrate-anchor-budget.py`, which asserts the exact
/// anchor *identities* (not just counts) over twelve cases spanning all four
/// categories at 40–137 candidates, and exits non-zero on divergence.
pub const ANCHOR_SAMPLE_PER_CATEGORY: usize = 12;

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
    // Boundaries pinned exactly against the v2.3.1 oracle via
    // scripts/calibrate-time.py: transitions at 7 (fresh|aging), 22
    // (aging|stale), and 61 (stale|abandoned). The score ladder skips 3 —
    // abandoned scores 4, not 3.
    if days < 7 {
        ("fresh", 0)
    } else if days < 22 {
        ("aging", 1)
    } else if days < 61 {
        ("stale", 2)
    } else {
        ("abandoned", 4)
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
    fn structure_score_reproduces_the_oracle_goldens() {
        const ORACLE_ADD_TOKEN_ECONOMY: i64 = 0;
        const ORACLE_ENHANCE_D5: i64 = 3;
        const ORACLE_MYCELIUM: i64 = 7;
        const ORACLE_PR_CONTROL_LOG: i64 = 7;

        // Every broken anchor in these four golden fixtures is a CliFlag, so
        // #119 (CliFlags broken again) makes these triples reachable; under #83
        // no resolver run could produce them. This asserts the formula on
        // hand-copied literals and nothing more — the fixtures are output-only
        // and are never replayed through `drift::analyze` (#132).
        assert_eq!(structure_score(0, 0, 16), ORACLE_ADD_TOKEN_ECONOMY);
        assert_eq!(structure_score(3, 3, 40), ORACLE_ENHANCE_D5);
        assert_eq!(structure_score(9, 9, 29), ORACLE_MYCELIUM);
        assert_eq!(structure_score(12, 12, 25), ORACLE_PR_CONTROL_LOG);
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
    fn time_bucket_boundaries_pinned_against_oracle() {
        // Every edge below is the exact transition day recovered by
        // scripts/calibrate-time.py --mode boundaries against the v2.3.1 oracle.
        // Defensive floor: negative days never reach here in production
        // (time_dimension clamps at 0, matching the oracle's "fresh (0d)" for
        // future created dates), but the bucket itself is total over i64.
        assert_eq!(time_bucket(-1), ("fresh", 0));
        // fresh: 0..=6
        assert_eq!(time_bucket(0), ("fresh", 0));
        assert_eq!(time_bucket(5), ("fresh", 0)); // field sample
        assert_eq!(time_bucket(6), ("fresh", 0));
        // fresh|aging edge at 7
        assert_eq!(time_bucket(7), ("aging", 1));
        // aging: 7..=21 (old code wrongly flipped 21 to stale)
        assert_eq!(time_bucket(19), ("aging", 1)); // field sample
        assert_eq!(time_bucket(21), ("aging", 1));
        // aging|stale edge at 22
        assert_eq!(time_bucket(22), ("stale", 2));
        // stale: 22..=60 (old code wrongly flipped 60 to abandoned)
        assert_eq!(time_bucket(25), ("stale", 2)); // field sample
        assert_eq!(time_bucket(36), ("stale", 2)); // field sample
        assert_eq!(time_bucket(60), ("stale", 2));
        // stale|abandoned edge at 61; abandoned scores 4, not 3 (ladder skips 3)
        assert_eq!(time_bucket(61), ("abandoned", 4));
        // no further transition: probed out to 3650d, all abandoned score 4
        assert_eq!(time_bucket(365), ("abandoned", 4));
        assert_eq!(time_bucket(3650), ("abandoned", 4));
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
