//! MODIFIED requirement 的 scenario 身分比對（決策 D13，刻意與 OpenSpec 1.13.2 分歧）。
//!
//! OpenSpec 以 scenario 標頭整行（`#### Scenario:` 之後的文字）當身分，所以
//! `slug -- 舊標題` 改成 `slug -- 新標題` 會被當成「刪掉一個、新增一個」，而 MODIFIED
//! 區塊也沒有辦法刻意移除 scenario。validate 的兩處檢查與 archive 的拒絕條件共用本模組，
//! 三者對同一份 delta 的判定才會一致。
//!
//! 比對分三輪，前一輪配對到的不再參與後面的輪次：
//! 1. 整行完全相同（OpenSpec 原本的規則，計入重複次數）。
//! 2. slug 相同：只在該 slug 於兩邊各自只出現一次時才成立。同一個 requirement 內 slug
//!    重複時退回整行比對，否則 `login -- error path` 換成 `login -- new path` 會被當成
//!    同一個 scenario 而藏住真正的刪除。
//! 3. 補 slug：current 沒有 slug、incoming 的 ` -- ` 後標題與 current 整行相同。
//!
//! 仍沒配對到的 current scenario，若 MODIFIED 區塊有
//! `<!-- removed-scenario: <slug 或整行> -->` 標記點名它，就視為刻意移除；標記只豁免它點名的
//! 那一個。archive 寫回 main spec 前會拿掉標記行。

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

static REMOVED_MARKER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*<!--\s*removed-scenario:\s*(.*?)\s*-->\s*$").unwrap());
static SLUG: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[a-z0-9]+(?:-[a-z0-9]+)*$").unwrap());

/// 比對結果；`missing` 與 `added` 保留原始出現順序。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ScenarioMatch {
    /// current 有、incoming 沒有，且沒有標記豁免的 scenario。
    pub missing: Vec<String>,
    /// incoming 有、current 沒有配對到的 scenario。
    pub added: Vec<String>,
    /// 標記豁免、刻意移除的 current scenario。
    pub removed: Vec<String>,
    /// 沒有對應到任何被移除 scenario 的標記值。
    pub unused_markers: Vec<String>,
}

/// scenario 名稱的 slug：` -- ` 前的 kebab-case 字串，或整個名稱本身就是 slug。
fn slug_of(name: &str) -> Option<&str> {
    let head = match name.split_once(" -- ") {
        Some((head, _)) => head.trim(),
        None => name.trim(),
    };
    SLUG.is_match(head).then_some(head)
}

/// ` -- ` 之後的標題；沒有 slug 時就是整個名稱。
fn title_of(name: &str) -> &str {
    match name.split_once(" -- ") {
        Some((head, title)) if SLUG.is_match(head.trim()) => title.trim(),
        _ => name.trim(),
    }
}

fn slug_counts(names: &[String]) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for name in names {
        if let Some(slug) = slug_of(name) {
            *counts.entry(slug).or_insert(0) += 1;
        }
    }
    counts
}

/// MODIFIED 區塊內的 `removed-scenario` 標記值（略過 code fence 內的行）。
pub(crate) fn removed_markers(raw: &str) -> Vec<String> {
    let lines: Vec<&str> = raw.split('\n').collect();
    let mask = crate::openspec_md::fence_mask(&lines);
    lines
        .iter()
        .enumerate()
        .filter(|(index, _)| !mask[*index])
        .filter_map(|(_, line)| REMOVED_MARKER.captures(line.trim_end_matches('\r')))
        .map(|caps| caps[1].to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

/// 拿掉 code fence 外的 `removed-scenario` 標記行，給 archive 寫回 main spec 用。
pub(crate) fn strip_removed_markers(raw: &str) -> String {
    let lines: Vec<&str> = raw.split('\n').collect();
    let mask = crate::openspec_md::fence_mask(&lines);
    lines
        .iter()
        .enumerate()
        .filter(|(index, line)| {
            mask[*index] || !REMOVED_MARKER.is_match(line.trim_end_matches('\r'))
        })
        .map(|(_, line)| *line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// 依模組說明的三輪規則比對 current 與 incoming 的 scenario 名稱。
pub(crate) fn match_scenarios(
    current: &[String],
    incoming: &[String],
    markers: &[String],
) -> ScenarioMatch {
    let mut current_left: Vec<Option<&String>> = current.iter().map(Some).collect();
    let mut incoming_left: Vec<Option<&String>> = incoming.iter().map(Some).collect();

    // 第 1 輪：整行完全相同。
    for slot in current_left.iter_mut() {
        let Some(name) = *slot else { continue };
        if let Some(found) = incoming_left
            .iter_mut()
            .find(|other| other.is_some_and(|o| o == name))
        {
            *found = None;
            *slot = None;
        }
    }

    // 第 2 輪：兩邊各自唯一的 slug。
    let current_slugs = slug_counts(current);
    let incoming_slugs = slug_counts(incoming);
    let unique =
        |slug: &str| current_slugs.get(slug) == Some(&1) && incoming_slugs.get(slug) == Some(&1);
    for slot in current_left.iter_mut() {
        let Some(name) = *slot else { continue };
        let Some(slug) = slug_of(name).filter(|slug| unique(slug)) else {
            continue;
        };
        if let Some(found) = incoming_left
            .iter_mut()
            .find(|other| other.is_some_and(|o| slug_of(o) == Some(slug)))
        {
            *found = None;
            *slot = None;
        }
    }

    // 第 3 輪：current 沒有 slug，incoming 補上 slug 且標題相同。
    for slot in current_left.iter_mut() {
        let Some(name) = *slot else { continue };
        if slug_of(name).is_some() {
            continue;
        }
        if let Some(found) = incoming_left.iter_mut().find(|other| {
            other.is_some_and(|o| {
                slug_of(o).is_some() && o.contains(" -- ") && title_of(o) == name.trim()
            })
        }) {
            *found = None;
            *slot = None;
        }
    }

    let mut result = ScenarioMatch::default();
    let mut used = vec![false; markers.len()];
    for name in current_left.into_iter().flatten() {
        let hit = markers.iter().enumerate().find(|(index, marker)| {
            !used[*index]
                && (marker.as_str() == name.trim() || Some(marker.as_str()) == slug_of(name))
        });
        match hit {
            Some((index, _)) => {
                used[index] = true;
                result.removed.push(name.clone());
            }
            None => result.missing.push(name.clone()),
        }
    }
    result.added = incoming_left.into_iter().flatten().cloned().collect();
    result.unused_markers = markers
        .iter()
        .zip(used)
        .filter(|(_, used)| !used)
        .map(|(marker, _)| marker.clone())
        .collect();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn retitled_scenario_with_the_same_unique_slug_is_not_missing() {
        let got = match_scenarios(
            &names(&["celebrate -- 註冊完成觸發慶祝"]),
            &names(&["celebrate -- 綁定完成觸發慶祝"]),
            &[],
        );
        assert!(got.missing.is_empty(), "{got:?}");
        assert!(got.added.is_empty(), "{got:?}");
    }

    #[test]
    fn duplicate_slug_falls_back_to_full_name_matching() {
        let got = match_scenarios(
            &names(&["login -- happy path", "login -- error path"]),
            &names(&["login -- happy path", "login -- new path"]),
            &[],
        );
        assert_eq!(got.missing, names(&["login -- error path"]));
        assert_eq!(got.added, names(&["login -- new path"]));
    }

    #[test]
    fn duplicate_slug_on_the_incoming_side_also_falls_back() {
        let got = match_scenarios(
            &names(&["login -- error path"]),
            &names(&["login -- new path", "login -- other path"]),
            &[],
        );
        assert_eq!(got.missing, names(&["login -- error path"]));
    }

    #[test]
    fn adding_a_slug_to_an_unslugged_scenario_keeps_its_identity() {
        let got = match_scenarios(
            &names(&["Partial unique index enforces one active template"]),
            &names(&["unique-index -- Partial unique index enforces one active template"]),
            &[],
        );
        assert!(got.missing.is_empty(), "{got:?}");
        assert!(got.added.is_empty(), "{got:?}");
    }

    #[test]
    fn a_different_slug_is_a_different_scenario() {
        let got = match_scenarios(
            &names(&["old-slug -- same title"]),
            &names(&["new-slug -- same title"]),
            &[],
        );
        assert_eq!(got.missing, names(&["old-slug -- same title"]));
    }

    #[test]
    fn marker_excuses_only_the_scenario_it_names() {
        let got = match_scenarios(
            &names(&["keep -- a", "drop-one -- b", "drop-two -- c"]),
            &names(&["keep -- a"]),
            // 點名第二個被刪的 scenario：「任何標記都豁免第一個遺失者」的錯誤寫法才會被抓到。
            &names(&["drop-two"]),
        );
        assert_eq!(got.removed, names(&["drop-two -- c"]));
        assert_eq!(got.missing, names(&["drop-one -- b"]));
        assert!(got.unused_markers.is_empty());
    }

    #[test]
    fn marker_can_name_an_unslugged_scenario_by_its_full_text() {
        let got = match_scenarios(
            &names(&["Admin restores defaults"]),
            &[],
            &names(&["Admin restores defaults"]),
        );
        assert_eq!(got.removed, names(&["Admin restores defaults"]));
        assert!(got.missing.is_empty());
    }

    #[test]
    fn marker_for_a_scenario_still_present_is_unused() {
        let got = match_scenarios(
            &names(&["keep -- a"]),
            &names(&["keep -- a"]),
            &names(&["keep"]),
        );
        assert_eq!(got.unused_markers, names(&["keep"]));
        assert!(got.removed.is_empty());
    }

    #[test]
    fn markers_are_read_outside_fences_and_stripped() {
        let raw = "### Requirement: X\n\nThe system SHALL x.\n\n<!-- removed-scenario: gone -->\n\n```\n<!-- removed-scenario: fenced -->\n```\n";
        assert_eq!(removed_markers(raw), names(&["gone"]));
        let stripped = strip_removed_markers(raw);
        assert!(!stripped.contains("removed-scenario: gone"), "{stripped}");
        assert!(stripped.contains("removed-scenario: fenced"), "{stripped}");
    }
}
