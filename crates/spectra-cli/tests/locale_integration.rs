//! W8：`.spectra.yaml` 的 `locale` 對 `instructions` 輸出的影響（oracle 3.0.0）。
//! 規則與實證見 `docs/reverse-engineering/artifact-workflow.md` 的 locale 一節。

mod common;

use common::{init_project_with_change, spectra, TempDir};

/// oracle 3.0.0（W8 探測 p01）：`.spectra.yaml` 的 `locale: tw` 讓 artifact 與 apply 的
/// `locale` 都變成 `Traditional Chinese (繁體中文)`；instruction 文字不變。
#[test]
fn locale_from_spectra_yaml_reaches_artifact_and_apply_json() {
    let tmp = TempDir::new("instructions-locale");
    init_project_with_change(&tmp, "add-login");
    let config = tmp.join(".spectra.yaml");
    let original = std::fs::read_to_string(&config).unwrap();
    std::fs::write(&config, format!("{original}locale: tw\n")).unwrap();
    let change = tmp.join("openspec/changes/add-login");
    std::fs::write(change.join("proposal.md"), "## Why\n\nx.\n").unwrap();
    std::fs::create_dir_all(change.join("specs/cap")).unwrap();
    std::fs::write(change.join("specs/cap/spec.md"), "## ADDED Requirements\n").unwrap();
    std::fs::write(change.join("tasks.md"), "- [ ] 1.1 a\n").unwrap();
    for args in [
        &["instructions", "design", "--change", "add-login", "--json"][..],
        &["instructions", "apply", "--change", "add-login", "--json"][..],
    ] {
        let out = spectra().args(args).current_dir(&*tmp).output().unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["locale"], "Traditional Chinese (繁體中文)", "{args:?}");
    }
}
