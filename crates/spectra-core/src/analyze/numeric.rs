//! `conNumericClaimMismatch`（oracle 3.0.0 新增）：proposal.md 與 design.md 對同一
//! 數值宣稱給出不同數字。規則由 W10 探測 p40–p59 與 golden `numeric-claims` 釘住，
//! 逐條見 `docs/reverse-engineering/analyze.md`。

use std::cmp::Reverse;
use std::collections::HashSet;

use super::consistency::{significant_tokens, strip_numbering_prefix};
use super::extract::is_fence_delimiter;
use super::{params, FindingText};

/// 可以緊接在數字後面的單位（不分大小寫）。其他 ASCII 字母緊接在數字後就不是宣稱。
const UNITS: [&str; 38] = [
    "ns", "us", "ms", "s", "h", "hr", "hrs", "sec", "secs", "min", "mins", "kb", "mb", "gb", "tb",
    "kib", "mib", "gib", "tib", "bit", "bits", "byte", "bytes", "bps", "kbps", "mbps", "gbps",
    "hz", "khz", "mhz", "ghz", "fps", "px", "pt", "em", "rem", "vw", "vh",
];

/// label 最後一個字是這些字（不分大小寫）時，數字是結構編號而不是宣稱。
const STRUCTURAL_WORDS: [&str; 6] = ["phase", "step", "layer", "stage", "version", "decision"];
const STRUCTURAL_SUFFIXES: [&str; 5] = ["決策", "步驟", "階段", "版本", "層"];

/// 帶小數點的數值在這些字（整字、不分大小寫）出現於 label 時視為版本號。
const VERSION_WORDS: [&str; 14] = [
    "framework",
    "upgrading",
    "downgrading",
    "migration",
    "dependencies",
    "version",
    "release",
    "runtime",
    "library",
    "upgrade",
    "dependency",
    "versions",
    "upgraded",
    "downgrade",
];
const VERSION_FRAGMENTS: [&str; 4] = ["版本", "升級", "降級", "遷移"];

/// 一個數值宣稱。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Claim {
    /// 數值之前的文字（處理後、trim）。
    label: String,
    /// 數值原文（含緊鄰的正負號，不含單位）。
    value: String,
    /// 該行第幾個數字（被拒絕的數字也算），配對時兩邊必須相同。
    index: usize,
}

/// 行內 code span 連同裡面的數字一起移除；落單的反引號之後整段丟掉。
fn remove_code_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else {
            return out;
        };
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

/// 數字之後緊接的 ASCII 英數字串必須是已知單位，且單位之後不能是 `/`、`:`、`_`。
fn unit_is_accepted(chars: &[char], start: usize) -> bool {
    let end = chars[start..]
        .iter()
        .position(|c| !c.is_ascii_alphanumeric())
        .map_or(chars.len(), |offset| start + offset);
    let word: String = chars[start..end]
        .iter()
        .collect::<String>()
        .to_ascii_lowercase();
    UNITS.contains(&word.as_str()) && !matches!(chars.get(end), Some('/' | ':' | '_'))
}

fn is_number_token(token: &str) -> bool {
    token.chars().any(|c| c.is_ascii_digit())
        && token
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, ',' | '.' | '+' | '-'))
}

/// label 的最後一個字是結構編號（`Phase 3`、`MAX_RETRIES 3`、`第二階段 3`）。
/// label 以 `→`／`->` 結尾（轉折的後一個值）時，改看轉折前那個數值之前的字：
/// `phase 3 → 5` 的 `5` 也不算宣稱（golden x1／x7），`step 3 ms → 5` 則算（x5）。
fn ends_with_structural_identifier(label: &str) -> bool {
    let mut base = label.trim_end();
    if let Some(before) = base.strip_suffix('→').or_else(|| base.strip_suffix("->")) {
        base = before.trim_end();
        if let Some(last) = base.split_whitespace().last() {
            if is_number_token(last) {
                base = base[..base.len() - last.len()].trim_end();
            }
        }
    }
    let Some(word) = base.split_whitespace().last() else {
        return false;
    };
    STRUCTURAL_WORDS.contains(&word.to_lowercase().as_str())
        || STRUCTURAL_SUFFIXES
            .iter()
            .any(|suffix| word.ends_with(suffix))
        || (word.contains('_')
            && word
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
}

/// 帶小數點的值在版本語境中（兩個以上的點，或 label 含版本相關字）視為版本號。
fn is_version_number(value: &str, label: &str) -> bool {
    if !value.contains('.') {
        return false;
    }
    value.matches('.').count() >= 2
        || label
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| VERSION_WORDS.contains(&word.to_ascii_lowercase().as_str()))
        || VERSION_FRAGMENTS
            .iter()
            .any(|fragment| label.contains(fragment))
}

/// 一行（已去掉 heading 記號、編號與 code span）裡的宣稱，由左到右。
fn line_claims(line: &str, claims: &mut Vec<Claim>) {
    let chars: Vec<char> = line.chars().collect();
    let byte_at = |index: usize| -> usize { chars[..index].iter().map(|c| c.len_utf8()).sum() };
    let mut number_index = 0;
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut end = i;
        while end < chars.len()
            && (chars[end].is_ascii_digit() || matches!(chars[end], ',' | '.' | '-'))
        {
            end += 1;
        }
        let mut value_end = end;
        while matches!(chars[value_end - 1], ',' | '.' | '-') {
            value_end -= 1;
        }
        let index = number_index;
        number_index += 1;
        let start = if i > 0 && matches!(chars[i - 1], '+' | '-') {
            i - 1
        } else {
            i
        };
        let rejected_before = start > 0
            && (chars[start - 1].is_ascii_alphanumeric()
                || matches!(chars[start - 1], '#' | '/' | ':' | '\\' | '_'));
        let rejected_after = match chars.get(value_end) {
            Some('/' | ':' | '_') => true,
            Some(c) if c.is_ascii_alphabetic() => !unit_is_accepted(&chars, value_end),
            _ => false,
        };
        i = end;
        if rejected_before || rejected_after {
            continue;
        }
        let label = line[..byte_at(start)].trim().to_string();
        let value: String = chars[start..value_end].iter().collect();
        if ends_with_structural_identifier(&label) || is_version_number(&value, &label) {
            continue;
        }
        claims.push(Claim {
            label,
            value,
            index,
        });
    }
}

/// 一份文件的宣稱：跳過 fence；每行 trim、去掉開頭的 `#`、去掉編號前綴、移除 code span。
/// HTML 註解不跳過。
fn claims(text: &str) -> Vec<Claim> {
    let mut claims = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if is_fence_delimiter(line) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let line = line.trim().trim_start_matches('#').trim_start();
        let line = remove_code_spans(strip_numbering_prefix(line));
        line_claims(&line, &mut claims);
    }
    claims
}

fn token_set(label: &str) -> HashSet<String> {
    significant_tokens(&label.to_lowercase())
        .into_iter()
        .collect()
}

fn normalized(value: &str) -> String {
    value.replace(',', "")
}

/// proposal 與 design 的宣稱配對：同一行內位置（number index）相同，label 的
/// significant token 交集至少 4 個且至少是較大集合的 80%。候選依（分數、label 完全相同、
/// 交集大小、值相同、proposal 順序、design 順序）排序後一對一貪婪配對；配到且值不同
/// （去掉 `,` 比對）就回報，依排序輸出。
pub(super) fn findings(proposal: &str, design: &str) -> Vec<FindingText> {
    let left = claims(proposal);
    let right = claims(design);
    let left_tokens: Vec<_> = left.iter().map(|claim| token_set(&claim.label)).collect();
    let right_tokens: Vec<_> = right.iter().map(|claim| token_set(&claim.label)).collect();

    let mut candidates = Vec::new();
    for (li, lclaim) in left.iter().enumerate() {
        for (ri, rclaim) in right.iter().enumerate() {
            if lclaim.index != rclaim.index {
                continue;
            }
            let shared = left_tokens[li].intersection(&right_tokens[ri]).count();
            let larger = left_tokens[li].len().max(right_tokens[ri].len());
            if shared < 4 || shared * 100 < 80 * larger {
                continue;
            }
            let exact = lclaim.label.to_lowercase() == rclaim.label.to_lowercase();
            let equal = normalized(&lclaim.value) == normalized(&rclaim.value);
            candidates.push((
                (
                    Reverse(shared * 100 / larger),
                    !exact,
                    Reverse(shared),
                    !equal,
                    li,
                    ri,
                ),
                equal,
            ));
        }
    }
    candidates.sort();

    let mut used_left = vec![false; left.len()];
    let mut used_right = vec![false; right.len()];
    let mut findings = Vec::new();
    for ((.., li, ri), equal) in candidates {
        if used_left[li] || used_right[ri] {
            continue;
        }
        used_left[li] = true;
        used_right[ri] = true;
        if equal {
            continue;
        }
        let (label, left_value, right_value) = (&left[li].label, &left[li].value, &right[ri].value);
        findings.push(FindingText {
            severity: "Warning",
            location: "proposal.md ↔ design.md".to_string(),
            summary: format!(
                "Numeric claim '{label}' differs: proposal.md={left_value}, design.md={right_value}"
            ),
            recommendation: "Align the numeric claim across artifacts or clarify the labels"
                .to_string(),
            key: "conNumericClaimMismatch",
            summary_params: params(&[
                ("label", label),
                ("leftFile", "proposal.md"),
                ("leftValue", left_value),
                ("rightFile", "design.md"),
                ("rightValue", right_value),
            ]),
            recommendation_params: params(&[]),
        });
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(line: &str) -> Vec<(String, String, usize)> {
        let mut out = Vec::new();
        line_claims(line, &mut out);
        out.into_iter()
            .map(|c| (c.label, c.value, c.index))
            .collect()
    }

    #[test]
    fn extraction_rejects_glued_and_structural_numbers_but_counts_them() {
        assert_eq!(
            values("zzz1 zzz2 12"),
            [("zzz1 zzz2".to_string(), "12".to_string(), 2)]
        );
        assert_eq!(
            values("a 3-5 items"),
            [("a".to_string(), "3-5".to_string(), 0)]
        );
        assert_eq!(values("a -5"), [("a".to_string(), "-5".to_string(), 0)]);
        assert!(values("a a-3").is_empty());
        assert_eq!(values("a 3."), [("a".to_string(), "3".to_string(), 0)]);
        assert_eq!(values("a 30ms."), [("a".to_string(), "30".to_string(), 0)]);
        for rejected in [
            "a #3", "a /3", "a 3/ x", "a 3: x", "a 3_x", "a \\3", "a 3x", "a 3ns2", "a 3gb/s",
            "a 10k",
        ] {
            assert!(values(rejected).is_empty(), "{rejected}");
        }
        assert_eq!(values("a 3個"), [("a".to_string(), "3".to_string(), 0)]);
        assert!(values("run Phase 3").is_empty());
        assert!(values("run MAX_RETRIES 3").is_empty());
        assert!(values("run 第二階段 3").is_empty());
        assert_eq!(values("run phase: 3").len(), 1);
    }

    #[test]
    fn transitions_yield_both_values_unless_the_source_is_structural() {
        assert_eq!(
            values("a 3 → 5"),
            [
                ("a".to_string(), "3".to_string(), 0),
                ("a 3 →".to_string(), "5".to_string(), 1)
            ]
        );
        assert!(values("a phase 3→5").is_empty());
        assert!(values("a phase 3 -> 5").is_empty());
        assert_eq!(values("a step 3 ms → 5").len(), 1);
    }

    #[test]
    fn dotted_values_in_version_context_are_dropped() {
        assert!(is_version_number("3.2", "the upgrade"));
        assert!(is_version_number("3.2", "the-upgrade"));
        assert!(is_version_number("3.2.1", "plain"));
        assert!(is_version_number("3.2", "版本號"));
        assert!(!is_version_number("3.2", "upgrades"));
        assert!(!is_version_number("3", "upgrade"));
    }

    #[test]
    fn code_spans_and_unclosed_backticks_are_removed() {
        assert_eq!(remove_code_spans("a `x 3` b 4"), "a  b 4");
        assert_eq!(remove_code_spans("a `unclosed 3"), "a ");
    }

    #[test]
    fn greedy_pairing_prefers_exact_labels_then_equal_values() {
        let mismatches = |proposal: &str, design: &str| -> Vec<(String, String)> {
            findings(proposal, design)
                .into_iter()
                .map(|f| {
                    (
                        f.summary_params["leftValue"].clone(),
                        f.summary_params["rightValue"].clone(),
                    )
                })
                .collect()
        };
        let l = "The maximum request timeout budget is";
        // 值相同的配對先配，於是 30 沒有對象。
        assert!(mismatches(&format!("{l} 30\n{l} 50\n"), &format!("{l} 50\n")).is_empty());
        assert_eq!(
            mismatches(&format!("{l} 30\n"), &format!("{l} 50\n{l} 70\n")),
            [("30".to_string(), "50".to_string())]
        );
        // label 完全相同優先於只有 token 相同。
        assert_eq!(
            mismatches(
                "alpha bravo charlie delta is 30\n",
                "alpha bravo charlie delta 50\nalpha bravo charlie delta is 70\n"
            ),
            [("30".to_string(), "70".to_string())]
        );
        // 4/5 = 80% 仍是候選；4/6 不是。
        assert_eq!(
            mismatches(
                "alpha bravo charlie delta 30\n",
                "alpha bravo charlie delta extra 50\n"
            )
            .len(),
            1
        );
        assert!(mismatches(
            "alpha bravo charlie delta 30\n",
            "alpha bravo charlie delta extra more 50\n"
        )
        .is_empty());
        assert!(mismatches(
            "alpha bravo charlie delta 1,000\n",
            "alpha bravo charlie delta 1000\n"
        )
        .is_empty());
    }
}
