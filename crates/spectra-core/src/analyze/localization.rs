//! Localization dimension（oracle `analyzer::localization::analyze_localization`
//! 與 `locale_check::is_wrong_language`，3.0.0 新增；探測 p60–p65）。

use super::extract::is_fence_delimiter;
use super::{params, Artifacts, FindingText};

/// 只有這三個 locale（大小寫敏感、完全相符）啟用這個 dimension。
const CHECKED_LOCALES: [&str; 3] = ["tw", "cn", "ja"];

/// 這個 dimension 是否執行：locale 是 tw／cn／ja，且 proposal、design、tasks 至少一個存在。
pub(super) fn runs(locale: Option<&str>, artifacts: &Artifacts) -> bool {
    locale.is_some_and(|locale| CHECKED_LOCALES.contains(&locale))
        && (artifacts.proposal.is_some() || artifacts.design.is_some() || artifacts.tasks.is_some())
}

/// 依 proposal.md、design.md、tasks.md 的固定順序，每個檔案至多一筆。specs 不檢查。
pub(super) fn findings(locale: &str, artifacts: &Artifacts) -> Vec<FindingText> {
    [
        ("proposal.md", artifacts.proposal),
        ("design.md", artifacts.design),
        ("tasks.md", artifacts.tasks),
    ]
    .into_iter()
    .filter_map(|(file, content)| Some((file, content?)))
    .filter(|(_, content)| is_wrong_language(content))
    .map(|(file, _)| FindingText {
        severity: "Warning",
        location: file.to_string(),
        summary: format!(
            "{file} appears to be written in English, but the project locale is '{locale}'"
        ),
        recommendation: format!("Rewrite {file} in the configured language ({locale})"),
        key: "locWrongLanguage",
        summary_params: params(&[("file", file), ("locale", locale)]),
        recommendation_params: params(&[("file", file), ("locale", locale)]),
    })
    .collect()
}

/// URL 在這些字元（或任何空白，含 NBSP 與 U+3000）之前結束。
fn ends_url(c: char) -> bool {
    c.is_whitespace() || matches!(c, ')' | '>' | '|' | '"' | '\'' | ']')
}

/// 移除 `http://`、`https://`、`www.`（大小寫敏感，可出現在字中）起算到結束字元前的內容。
fn strip_urls(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    loop {
        let start = ["http://", "https://", "www."]
            .iter()
            .filter_map(|prefix| rest.find(*prefix))
            .min();
        let Some(start) = start else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail.find(ends_url).unwrap_or(tail.len());
        rest = &tail[end..];
    }
}

/// 移除成對反引號之間（含反引號）的內容；落單的反引號保留其後文字。
fn strip_inline_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else {
            break;
        };
        out.push_str(&rest[..open]);
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{3040}'..='\u{30FF}')
}

/// 去掉 fence 區塊（任一分隔行切換、未關閉則吃到檔尾）、URL 與 inline code 後，
/// 字母（ASCII 字母加非 ASCII 的 `is_alphabetic`）至少 80 個且 CJK 比例小於 0.1
/// 即判定為英文。Hangul、U+F900 相容表意字、Ext-B 與全形拉丁字母算字母但不算 CJK。
fn is_wrong_language(text: &str) -> bool {
    let mut alpha = 0usize;
    let mut cjk = 0usize;
    let mut in_fence = false;
    for line in text.split('\n') {
        if is_fence_delimiter(line) {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        for c in strip_inline_code(&strip_urls(line)).chars() {
            if c.is_ascii_alphabetic() || (!c.is_ascii() && c.is_alphabetic()) {
                alpha += 1;
                if is_cjk(c) {
                    cjk += 1;
                }
            }
        }
    }
    alpha >= 80 && (cjk as f64) / (alpha as f64) < 0.1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letters(n: usize) -> String {
        "x".repeat(n)
    }

    #[test]
    fn thresholds_match_the_oracle_boundaries() {
        // oracle 3.0.0 p61：79 字母 Clean、80 字母報、80+8 漢字報、80+9 Clean、72+8 恰 0.1 Clean。
        assert!(!is_wrong_language(&letters(79)));
        assert!(is_wrong_language(&letters(80)));
        assert!(is_wrong_language(&format!(
            "{} {}",
            letters(80),
            "漢".repeat(8)
        )));
        assert!(!is_wrong_language(&format!(
            "{} {}",
            letters(80),
            "漢".repeat(9)
        )));
        assert!(!is_wrong_language(&format!(
            "{} {}",
            letters(72),
            "漢".repeat(8)
        )));
        assert!(!is_wrong_language(&format!(
            "{} {}",
            letters(80),
            "カ".repeat(9)
        )));
        assert!(is_wrong_language(&format!(
            "{} {}",
            letters(80),
            "한".repeat(9)
        )));
        assert!(is_wrong_language(&format!(
            "{} {}",
            letters(80),
            "\u{F900}".repeat(9)
        )));
    }

    #[test]
    fn fences_urls_and_inline_code_are_removed_before_counting() {
        let base = letters(79);
        let hidden = "y".repeat(100);
        assert!(!is_wrong_language(&format!(
            "{base}\n```rust\n{hidden}\n```\n"
        )));
        assert!(is_wrong_language(&format!(
            "{base}\n```\n{hidden}\n~~~\n{hidden}\n"
        )));
        assert!(is_wrong_language(&format!("{base}\ntext ```\n{hidden}\n")));
        assert!(!is_wrong_language(&format!("{base} `{hidden}`")));
        assert!(is_wrong_language(&format!("{base} ``{hidden}``")));
        assert!(is_wrong_language(&format!("{base} `{hidden}")));
        assert!(!is_wrong_language(&format!("{base} https://{hidden}")));
        assert!(is_wrong_language(&format!("{base} a www.{hidden}")));
        assert!(is_wrong_language(&format!("{base} https://x.com){hidden}")));
        assert!(!is_wrong_language(&format!(
            "{base} https://x.com,{hidden}"
        )));
        assert!(is_wrong_language(&format!("{base} HTTPS://{hidden}")));
    }

    #[test]
    fn only_the_three_locales_enable_the_dimension() {
        let artifacts = Artifacts {
            proposal: Some("x"),
            design: None,
            tasks: None,
            spec_files: &[],
        };
        for locale in ["tw", "cn", "ja"] {
            assert!(runs(Some(locale), &artifacts), "{locale}");
        }
        for locale in ["zh", "zh-TW", "TW", "en", ""] {
            assert!(!runs(Some(locale), &artifacts), "{locale}");
        }
        assert!(!runs(None, &artifacts));
        let specs_only = Artifacts {
            proposal: None,
            ..artifacts
        };
        assert!(!runs(Some("tw"), &specs_only));
    }
}
