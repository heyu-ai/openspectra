//! `spectra validate` 對 OpenSpec 1.13.2 golden 的重播比對（決策 D1、W9b）。
//!
//! `docs/reverse-engineering/golden/validate-openspec-1.13.2.json` 由
//! `scripts/capture-validate-openspec.py` 在 `tests/fixtures/validate_openspec`
//! 上實際執行 OpenSpec 1.13.2 產生：每個 `dNN-*` change／`sNN-*` spec 對應一條
//! 規則（清單見 `docs/reverse-engineering/validate.md`）。期望值全部來自
//! OpenSpec 的真實輸出，不是讀原始碼推得的。
//!
//! 比對每個 item 的 `valid` 與完整的 issue 清單（level、path、line、message，
//! 含順序），四種組合：`--changes`／`--specs` × 一般／`--strict`。
//!
//! 截短保護：item 數與比對的 issue 總數必須等於下面的常數，golden 少了內容
//! 就會失敗，而不是比對較少的東西後照樣通過。

mod common;

use std::path::Path;

use serde_json::Value;

use common::{spectra, TempDir};

/// (item 數, 兩種模式合計的 issue 數)
const CHANGE_COUNTS: (usize, usize) = (46, 116);
const SPEC_COUNTS: (usize, usize) = (33, 50);

fn golden() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/reverse-engineering/golden/validate-openspec-1.13.2.json");
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("讀取 {path:?}：{e}"));
    serde_json::from_str(&raw).unwrap()
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn fixture() -> TempDir {
    let dir = TempDir::new("validate-openspec");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/validate_openspec"),
        &dir,
    );
    dir
}

/// 把 OpenSpectra 的 issue 轉成與 OpenSpec 相同的 JSON 物件（`line` 缺席時不出現）。
fn items_of(root: &Path, scope: &str, strict: bool) -> Vec<Value> {
    let mut command = spectra();
    command
        .args(["validate", &format!("--{scope}"), "--json"])
        .current_dir(root);
    if strict {
        command.arg("--strict");
    }
    let output = command.output().unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "validate --{scope} 的 stdout 不是 JSON（{e}）：\nstdout={}\nstderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let mut items: Vec<Value> = report["items"]
        .as_array()
        .expect("items 是陣列")
        .iter()
        .map(|item| {
            serde_json::json!({
                "id": item["id"],
                "valid": item["valid"],
                "issues": item["issues"],
            })
        })
        .collect();
    items.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    items
}

fn assert_scope_matches(scope: &str, (expected_items, expected_issues): (usize, usize)) {
    let golden = golden();
    assert_eq!(golden["openspec"], "1.13.2");
    let root = fixture();
    let mut mismatches = Vec::new();
    let mut compared_issues = 0usize;
    for mode in ["normal", "strict"] {
        let expected = golden["scopes"][scope][mode]
            .as_array()
            .unwrap_or_else(|| panic!("golden 缺少 {scope}/{mode}"));
        assert_eq!(
            expected.len(),
            expected_items,
            "golden {scope}/{mode} 的 item 數"
        );
        let actual = items_of(&root, scope, mode == "strict");
        let ids = |items: &[Value]| {
            items
                .iter()
                .map(|item| item["id"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&actual), ids(expected), "{scope}/{mode} 的 item 清單");
        for (want, got) in expected.iter().zip(&actual) {
            compared_issues += want["issues"].as_array().unwrap().len();
            if want != got {
                mismatches.push(format!(
                    "{scope}/{mode} {}:\n  OpenSpec 1.13.2: {want}\n  OpenSpectra:     {got}",
                    want["id"]
                ));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} 個 item 與 OpenSpec 1.13.2 不同：\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
    assert_eq!(
        compared_issues, expected_issues,
        "golden {scope} 比對的 issue 數"
    );
}

#[test]
fn validate_changes_matches_the_openspec_golden() {
    assert_scope_matches("changes", CHANGE_COUNTS);
}

#[test]
fn validate_specs_matches_the_openspec_golden() {
    assert_scope_matches("specs", SPEC_COUNTS);
}
