//! `task done` 的 touched-file baseline（#98，OpenSpectra-only）在 CLI 層的契約：
//! baseline 壞掉時要在 stderr 警告、仍以狀態碼 0 完成，並退回記錄所有 dirty 檔案。

mod common;

use std::path::Path;

use common::{git, spectra, TempDir};

fn run_ok(root: &Path, args: &[&str]) -> std::process::Output {
    let output = spectra().args(args).current_dir(root).output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{args:?} 失敗：{output:?}");
    output
}

#[test]
fn task_done_warns_and_records_every_dirty_file_when_the_baseline_is_corrupt() {
    let root = TempDir::new("touched-baseline-corrupt");
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@t.co"]);
    git(&root, &["commit", "--allow-empty", "-q", "-m", "init"]);
    run_ok(&root, &["init"]);

    std::fs::write(root.join("unrelated.rs"), "// pre-existing edit\n").unwrap();
    run_ok(&root, &["new", "change", "demo"]);
    let baseline = root.join(".spectra/changes/demo.touched-baseline.json");
    assert!(baseline.is_file(), "new change 應寫出 baseline");
    std::fs::write(&baseline, "not json").unwrap();
    std::fs::write(root.join("openspec/changes/demo/tasks.md"), "- [ ] one\n").unwrap();

    let output = run_ok(&root, &["task", "done", "1", "--change", "demo"]);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("is corrupt") && stderr.contains("recording every dirty file"),
        "stderr 應警告 baseline 壞掉，實際：{stderr}"
    );
    let touched = std::fs::read_to_string(root.join(".spectra/touched/demo.json")).unwrap();
    assert!(
        touched.contains("unrelated.rs"),
        "應退回記錄所有 dirty 檔案：{touched}"
    );
}
