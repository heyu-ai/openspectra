//! Consistency dimension（oracle `analyzer::consistency::analyze_consistency`）：
//! 依序輸出全部 `conDesignNotInTasks`、`conNumericClaimMismatch`、
//! `conGoalsNonGoalsOverlap`（編號連續，oracle 3.0.0 p59c）。

use std::collections::HashSet;

use super::extract::is_fence_delimiter;
use super::{numeric, params, Artifacts, FindingText};

pub(super) fn findings(artifacts: &Artifacts) -> Vec<FindingText> {
    let mut findings = Vec::new();
    if let (Some(design), Some(tasks)) = (artifacts.design, artifacts.tasks) {
        findings.extend(design_not_in_tasks(design, tasks));
    }
    if let (Some(proposal), Some(design)) = (artifacts.proposal, artifacts.design) {
        findings.extend(numeric::findings(proposal, design));
    }
    if let Some(design) = artifacts.design {
        findings.extend(goals_non_goals_overlap(design));
    }
    findings
}

fn is_han(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}')
}

/// oracle `significant_tokens`：ASCII 英數字的最長連續段（轉小寫）長度至少 4 才算；
/// 漢字連續段輸出重疊的雙字（`解析器` → `解析`、`析器`，單一漢字不產生 token）；
/// 其他字元（含非 ASCII 字母、假名、全形拉丁）都是分隔符。保留重複。
pub(super) fn significant_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut ascii = String::new();
    let mut han: Vec<char> = Vec::new();
    let flush_ascii = |ascii: &mut String, tokens: &mut Vec<String>| {
        if ascii.len() >= 4 {
            tokens.push(std::mem::take(ascii));
        }
        ascii.clear();
    };
    let flush_han = |han: &mut Vec<char>, tokens: &mut Vec<String>| {
        for pair in han.windows(2) {
            tokens.push(pair.iter().collect());
        }
        han.clear();
    };
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            flush_han(&mut han, &mut tokens);
            ascii.push(c.to_ascii_lowercase());
        } else if is_han(c) {
            flush_ascii(&mut ascii, &mut tokens);
            han.push(c);
        } else {
            flush_ascii(&mut ascii, &mut tokens);
            flush_han(&mut han, &mut tokens);
        }
    }
    flush_ascii(&mut ascii, &mut tokens);
    flush_han(&mut han, &mut tokens);
    tokens
}

/// oracle `strip_numbering_prefix`：開頭的 `[0-9.]+` 後面至少一個空白時，去掉這段
/// 與其後的空白（只做一次）。`1)`、`1:`、`1.pqrs`、`(1)`、全形數字都不動。
pub(super) fn strip_numbering_prefix(text: &str) -> &str {
    let digits_end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    if digits_end == 0 {
        return text;
    }
    let rest = &text[digits_end..];
    if rest.starts_with(char::is_whitespace) {
        rest.trim_start()
    } else {
        text
    }
}

/// design.md 每個 `### ` topic：關鍵字的 significant token 在整份 tasks.md（小寫）
/// 以子字串出現的比例低於 60% 就回報；沒有 token 的 topic 永不回報。
fn design_not_in_tasks(design: &str, tasks: &str) -> Vec<FindingText> {
    let tasks_lowercase = tasks.to_lowercase();
    let mut findings = Vec::new();
    for line in design.lines() {
        let Some(topic) = line.trim_start().strip_prefix("### ") else {
            continue;
        };
        let keyword = strip_numbering_prefix(topic.trim()).to_lowercase();
        let tokens = significant_tokens(&keyword);
        if tokens.is_empty() {
            continue;
        }
        let matched = tokens
            .iter()
            .filter(|token| tasks_lowercase.contains(token.as_str()))
            .count();
        if matched * 100 >= tokens.len() * 60 {
            continue;
        }
        findings.push(FindingText {
            severity: "Warning",
            location: "design.md".to_string(),
            summary: format!("Design topic '{keyword}' not referenced in tasks"),
            recommendation: "Verify tasks cover this design decision".to_string(),
            key: "conDesignNotInTasks",
            summary_params: params(&[("keyword", &keyword)]),
            recommendation_params: params(&[]),
        });
    }
    findings
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GoalsList {
    Goals,
    NonGoals,
}

/// design.md 的 `**Goals:**`／`**Non-Goals:**` 清單（跳過 fence）。標記行 `trim` 後
/// 必須完全相符；`#` 開頭的行結束目前項目並離開清單（已收集的保留）；`- `、`* `、
/// `+ ` 開始新項目，縮排的續行以一個空白接到目前項目。其他行（空行、未縮排的
/// 一般文字）結束目前項目是推論，oracle 沒有探測到這個邊界。
fn goals_lists(design: &str) -> (Vec<String>, Vec<String>) {
    let mut goals = Vec::new();
    let mut non_goals = Vec::new();
    let mut list: Option<GoalsList> = None;
    let mut item: Option<String> = None;
    let mut in_fence = false;
    let flush = |item: &mut Option<String>,
                 list: Option<GoalsList>,
                 goals: &mut Vec<String>,
                 non_goals: &mut Vec<String>| {
        if let Some(text) = item.take() {
            match list {
                Some(GoalsList::Goals) => goals.push(text),
                Some(GoalsList::NonGoals) => non_goals.push(text),
                None => {}
            }
        }
    };
    for line in design.lines() {
        if is_fence_delimiter(line) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let trimmed = line.trim();
        let marker = match trimmed {
            "**Goals:**" => Some(GoalsList::Goals),
            "**Non-Goals:**" => Some(GoalsList::NonGoals),
            _ => None,
        };
        if marker.is_some() || trimmed.starts_with('#') {
            flush(&mut item, list, &mut goals, &mut non_goals);
            list = marker;
            continue;
        }
        if list.is_none() {
            continue;
        }
        let bullet = ["- ", "* ", "+ "]
            .iter()
            .find_map(|prefix| line.trim_start().strip_prefix(prefix));
        if let Some(text) = bullet {
            flush(&mut item, list, &mut goals, &mut non_goals);
            item = Some(text.trim().to_string());
        } else if let Some(current) = item
            .as_mut()
            .filter(|_| line.starts_with(char::is_whitespace) && !trimmed.is_empty())
        {
            current.push(' ');
            current.push_str(trimmed);
        } else {
            flush(&mut item, list, &mut goals, &mut non_goals);
        }
    }
    flush(&mut item, list, &mut goals, &mut non_goals);
    (goals, non_goals)
}

/// 每一對 (goal, non-goal) 的 significant token 集合交集至少 8 個，且至少是較小集合的
/// 40% 就回報（goal 為主序）。以 `非 ` 開頭的 non-goal 跳過。只讀 design.md。
fn goals_non_goals_overlap(design: &str) -> Vec<FindingText> {
    let (goals, non_goals) = goals_lists(design);
    let token_set = |text: &str| -> HashSet<String> {
        significant_tokens(&text.to_lowercase())
            .into_iter()
            .collect()
    };
    let non_goals: Vec<(String, HashSet<String>)> = non_goals
        .into_iter()
        .filter(|text| !text.starts_with("非 "))
        .map(|text| {
            let tokens = token_set(&text);
            (text, tokens)
        })
        .collect();
    let mut findings = Vec::new();
    for goal in goals {
        let goal_tokens = token_set(&goal);
        for (non_goal, non_goal_tokens) in &non_goals {
            let shared = goal_tokens.intersection(non_goal_tokens).count();
            let smaller = goal_tokens.len().min(non_goal_tokens.len());
            if shared < 8 || shared * 100 < 40 * smaller {
                continue;
            }
            findings.push(FindingText {
                severity: "Warning",
                location: "design.md".to_string(),
                summary: format!("Goals item '{goal}' overlaps Non-Goals item '{non_goal}'"),
                recommendation: "Keep the work in either Goals or Non-Goals, not both".to_string(),
                key: "conGoalsNonGoalsOverlap",
                summary_params: params(&[("goal", &goal), ("nonGoal", non_goal)]),
                recommendation_params: params(&[]),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn significant_tokens_keep_long_ascii_runs_and_han_bigrams() {
        assert_eq!(significant_tokens("the parser"), ["parser"]);
        assert_eq!(significant_tokens("widget_gadget"), ["widget", "gadget"]);
        assert_eq!(significant_tokens("1234 abc"), ["1234"]);
        assert_eq!(
            significant_tokens("解析器設計"),
            ["解析", "析器", "器設", "設計"]
        );
        assert_eq!(
            significant_tokens("版本升 widget"),
            ["版本", "本升", "widget"]
        );
        assert_eq!(significant_tokens("école ωmega домик 漢"), ["cole", "mega"]);
        assert!(significant_tokens("x y z").is_empty());
    }

    #[test]
    fn numbering_prefix_needs_digits_or_dots_then_whitespace() {
        for (input, expected) in [
            ("1. pqrs", "pqrs"),
            ("1.2.3 pqrs", "pqrs"),
            ("7   pqrs", "pqrs"),
            ("1 - pqrs", "- pqrs"),
            ("1 2 3 pqrs", "2 3 pqrs"),
            ("1) pqrs", "1) pqrs"),
            ("1.pqrs", "1.pqrs"),
            ("12abc pqrs", "12abc pqrs"),
            ("(1) pqrs", "(1) pqrs"),
            ("５ pqrs", "５ pqrs"),
        ] {
            assert_eq!(strip_numbering_prefix(input), expected, "{input}");
        }
    }

    #[test]
    fn design_topics_are_flagged_below_sixty_percent_token_coverage() {
        let tasks = "- [ ] alpha1 alpha2 alpha3 alpha4 pqrs";
        let keywords = |design: &str| -> Vec<String> {
            design_not_in_tasks(design, tasks)
                .into_iter()
                .map(|f| f.summary_params["keyword"].clone())
                .collect()
        };
        // 3/5 = 60% 不回報；4/7 ≈ 57% 回報；1/2 回報；沒有 token 不回報。
        assert!(keywords("### alpha1 alpha2 alpha3 miss1x miss2x").is_empty());
        assert_eq!(
            keywords("### alpha1 alpha2 alpha3 alpha4 miss1x miss2x miss3x"),
            ["alpha1 alpha2 alpha3 alpha4 miss1x miss2x miss3x"]
        );
        assert_eq!(keywords("### 1. pqrs bravo"), ["pqrs bravo"]);
        assert!(keywords("### a\n### \n#### pqrs zzzz\n").is_empty());
    }

    #[test]
    fn goals_overlap_needs_eight_shared_tokens_and_forty_percent() {
        let eight = "alpha bravo charlie delta echoo foxtrot golf1 hotel";
        let design = |goal: &str, non_goal: &str| {
            format!("**Goals:**\n- {goal}\n\n**Non-Goals:**\n- {non_goal}\n")
        };
        assert_eq!(goals_non_goals_overlap(&design(eight, eight)).len(), 1);
        let seven = "alpha bravo charlie delta echoo foxtrot golf1";
        assert!(goals_non_goals_overlap(&design(seven, seven)).is_empty());
        let pad = |prefix: &str, n: usize| -> String {
            (1..=n).map(|i| format!(" {prefix}{i:03}")).collect()
        };
        // 8/20 = 40% 回報；8/21 不回報。
        let g20 = format!("{eight}{}", pad("aa", 12));
        let n20 = format!("{eight}{}", pad("bb", 12));
        assert_eq!(goals_non_goals_overlap(&design(&g20, &n20)).len(), 1);
        let g21 = format!("{eight}{}", pad("aa", 13));
        let n21 = format!("{eight}{}", pad("bb", 13));
        assert!(goals_non_goals_overlap(&design(&g21, &n21)).is_empty());
        assert!(goals_non_goals_overlap(&design(eight, &format!("非 {eight}"))).is_empty());
    }

    #[test]
    fn goals_lists_follow_markers_bullets_continuations_and_fences() {
        let design = "## Goals / Non-Goals\n\n**Goals:**\n- first part\n  continued\n* second\n+ third\n1. numbered\n\n\
                      ## Other\n\n  **Non-Goals:**\n- nope\n```\n- fenced\n```\n**Goals**:\n- ignored marker\n";
        let (goals, non_goals) = goals_lists(design);
        assert_eq!(goals, ["first part continued", "second", "third"]);
        assert_eq!(non_goals, ["nope", "ignored marker"]);
    }
}
