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
//! 另外逐字比對 `--format openspec --json` 的整份 envelope（W9a，決策 D3）：
//! golden 的 `envelopes` 與 `all_order`。
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
        .args([
            "validate",
            &format!("--{scope}"),
            "--json",
            "--format",
            "openspec",
        ])
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

/// golden `envelopes` 的筆數；少了就是 golden 被截短（`capture-validate-openspec.py`
/// 的 `ENVELOPES`）。
const ENVELOPE_COUNT: usize = 19;

fn copy_fixture(name: &str) -> TempDir {
    let dir = TempDir::new(&format!("validate-envelope-{name}"));
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
        &dir,
    );
    dir
}

/// 與 capture 腳本相同的正規化：`durationMs` 一律 0、專案路徑換成 `<ROOT>`。
/// 同 id 的 change／spec 平手排序不在這裡處理：OpenSpectra 本身就輸出 change 在前，
/// 由 golden 端把 OpenSpec 不固定的平手順序排成一樣。
fn normalize_envelope(stdout: &str, root: &Path) -> String {
    const KEY: &str = "\"durationMs\": ";
    let mut out = String::with_capacity(stdout.len());
    let mut rest = stdout;
    while let Some(at) = rest.find(KEY) {
        out.push_str(&rest[..at + KEY.len()]);
        rest = rest[at + KEY.len()..].trim_start_matches(|c: char| c.is_ascii_digit());
        out.push('0');
    }
    out.push_str(rest);
    let quoted = serde_json::to_string(root.to_str().unwrap()).unwrap();
    out.replace(&quoted, "\"<ROOT>\"")
}

/// `--format openspec --json` 的整份輸出（含 key 順序、summary、byType、root、
/// 錯誤 envelope、findings report）逐字等於 OpenSpec 1.13.2（決策 D3，W9a）。
#[test]
fn validate_format_openspec_matches_the_openspec_envelopes() {
    let golden = golden();
    let envelopes = golden["envelopes"]
        .as_array()
        .expect("golden 缺少 envelopes");
    assert_eq!(envelopes.len(), ENVELOPE_COUNT, "golden envelopes 的筆數");
    let mut copies: std::collections::HashMap<String, TempDir> = Default::default();
    let mut mismatches = Vec::new();
    for envelope in envelopes {
        let fixture = envelope["fixture"].as_str().unwrap().to_string();
        let args: Vec<&str> = envelope["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap())
            .collect();
        let root = copies
            .entry(fixture.clone())
            .or_insert_with(|| copy_fixture(&fixture));
        let output = spectra()
            .arg("validate")
            .args(&args)
            .args(["--json", "--format", "openspec"])
            .current_dir(&**root)
            .output()
            .unwrap();
        let got = (
            output.status.code().map(i64::from),
            normalize_envelope(&String::from_utf8_lossy(&output.stdout), root),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        );
        let want = (
            envelope["rc"].as_i64(),
            envelope["stdout"].as_str().unwrap().to_string(),
            envelope["stderr"].as_str().unwrap().to_string(),
        );
        if got != want {
            mismatches.push(format!(
                "{fixture} validate {args:?}:\n--- OpenSpec 1.13.2 (rc {:?}, stderr {:?})\n{}\n\
                 --- OpenSpectra (rc {:?}, stderr {:?})\n{}",
                want.0, want.2, want.1, got.0, got.2, got.1
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} 個 envelope 與 OpenSpec 1.13.2 不同：\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

/// 規則 fixture 上 `--all --format openspec` 的 item 順序（`localeCompare`）與 summary。
#[test]
fn validate_all_format_openspec_orders_items_like_openspec() {
    let golden = golden();
    let expected = &golden["all_order"];
    assert_eq!(expected["order"].as_array().unwrap().len(), 79);
    let root = fixture();
    let output = spectra()
        .args(["validate", "--all", "--json", "--format", "openspec"])
        .current_dir(&*root)
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let order: Vec<String> = report["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            format!(
                "{}/{}",
                item["type"].as_str().unwrap(),
                item["id"].as_str().unwrap()
            )
        })
        .collect();
    let want: Vec<String> = expected["order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry.as_str().unwrap().to_string())
        .collect();
    assert_eq!(order, want);
    assert_eq!(report["summary"], expected["summary"]);
}

#[test]
fn validate_changes_matches_the_openspec_golden() {
    assert_scope_matches("changes", CHANGE_COUNTS);
}

#[test]
fn validate_specs_matches_the_openspec_golden() {
    assert_scope_matches("specs", SPEC_COUNTS);
}
