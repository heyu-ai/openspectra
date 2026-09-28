//! W14：專案根的標記（oracle 3.0.0，探測 p02）——`.spectra.yaml`，或任何名為 `openspec` 的
//! 項目；純 OpenSpec 專案不需要 `.spectra.yaml`。以及未初始化時的錯誤訊息。

mod common;

use std::path::Path;

use common::{git, spectra, TempDir};

fn change_names(dir: &Path) -> Vec<String> {
    let out = spectra()
        .args(["list", "--json"])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect()
}

fn repo(label: &str) -> TempDir {
    let dir = TempDir::new(label);
    git(&dir, &["init", "-q"]);
    dir
}

#[test]
fn an_openspec_directory_alone_marks_the_project() {
    let dir = repo("root-openspec-only");
    std::fs::create_dir_all(dir.join("openspec")).unwrap();
    assert!(change_names(&dir).is_empty());

    std::fs::create_dir_all(dir.join("openspec/changes/x")).unwrap();
    std::fs::create_dir_all(dir.join("a/b")).unwrap();
    assert_eq!(change_names(&dir.join("a/b")), ["x"], "found walking up");
}

#[test]
fn an_openspec_file_also_counts_as_the_marker() {
    let dir = repo("root-openspec-file");
    std::fs::write(dir.join("openspec"), "").unwrap();
    assert!(change_names(&dir).is_empty());
}

#[test]
fn the_nearest_marker_wins_and_spectra_yaml_decides_at_the_same_level() {
    let dir = repo("root-nearest");
    std::fs::create_dir_all(dir.join("docs/s/changes/outer")).unwrap();
    std::fs::write(dir.join(".spectra.yaml"), "spec_dir: docs/s\n").unwrap();
    std::fs::create_dir_all(dir.join("inner/openspec/changes/inner-c")).unwrap();
    assert_eq!(change_names(&dir.join("inner")), ["inner-c"]);

    std::fs::create_dir_all(dir.join("openspec/changes/ignored")).unwrap();
    assert_eq!(change_names(&dir), ["outer"], "spec_dir from .spectra.yaml");
}

#[test]
fn uninitialized_projects_get_the_oracle_message() {
    let dir = repo("root-none");
    let out = spectra()
        .args(["list"])
        .current_dir(&*dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "Error: Not initialized. Run 'spectra init' to initialize.\n"
    );
}

/// `init` 在只有 `openspec/` 的專案以非破壞方式完成（oracle 會拒絕，見 init.md 的 W14 分歧），
/// 既有內容不動，並補上兩個 `.gitkeep`。
#[test]
fn init_in_an_openspec_only_project_keeps_existing_content() {
    let dir = repo("root-init-openspec");
    std::fs::create_dir_all(dir.join("openspec/specs/cap")).unwrap();
    std::fs::write(dir.join("openspec/specs/cap/spec.md"), "keep\n").unwrap();
    let out = spectra().arg("init").current_dir(&*dir).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        std::fs::read_to_string(dir.join("openspec/specs/cap/spec.md")).unwrap(),
        "keep\n"
    );
    assert!(dir.join("openspec/specs/.gitkeep").is_file());
    assert!(dir.join("openspec/changes/archive/.gitkeep").is_file());
}
