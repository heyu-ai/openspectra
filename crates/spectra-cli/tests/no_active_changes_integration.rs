mod common;

use std::path::Path;

use common::{git, spectra, TempDir};

const NO_ACTIVE_CHANGES_OUTPUT: &[u8] =
    b"No active changes. Create one with: spectra new change <name>\n";

fn init_empty_project(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Howie"]);
    git(root, &["config", "user.email", "howie@example.com"]);
    git(root, &["commit", "--allow-empty", "-q", "-m", "init"]);

    let output = spectra().arg("init").current_dir(root).output().unwrap();
    assert!(output.status.success(), "專案初始化失敗：{output:?}");
}

#[test]
fn read_commands_treat_no_active_changes_as_a_normal_empty_state() {
    let root = TempDir::new("no-active-read-commands");
    init_empty_project(&root);

    let cases: &[(&str, &[&str])] = &[
        ("status", &["status"]),
        ("status --json", &["status", "--json"]),
        ("instructions tasks", &["instructions", "tasks"]),
        (
            "instructions tasks --json",
            &["instructions", "tasks", "--json"],
        ),
        ("drift", &["drift"]),
        ("drift --json", &["drift", "--json"]),
        ("analyze", &["analyze"]),
        ("analyze --json", &["analyze", "--json"]),
        // The empty state outranks the schema gate: probed on 2026-08-06,
        // the oracle prints the sentinel and exits 0 even with a bogus
        // --schema on an empty project (pinned in schemas.md "Check order").
        // These lock the CLI's resolve-first ordering so a refactor that
        // gates on the schema flag before resolving fails loudly.
        ("status --schema bogus", &["status", "--schema", "bogus"]),
        (
            "status --schema bogus --json",
            &["status", "--schema", "bogus", "--json"],
        ),
        (
            "instructions tasks --schema bogus",
            &["instructions", "tasks", "--schema", "bogus"],
        ),
        (
            "instructions tasks --schema bogus --json",
            &["instructions", "tasks", "--schema", "bogus", "--json"],
        ),
    ];

    for &(label, args) in cases {
        let output = spectra().args(args).current_dir(&*root).output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{label} 應結束於狀態碼 0");
        assert_eq!(
            output.stdout, NO_ACTIVE_CHANGES_OUTPUT,
            "{label} 的 stdout 不符預期"
        );
        assert!(output.stderr.is_empty(), "{label} 的 stderr 應為空");
    }
}

#[test]
fn task_done_keeps_no_active_changes_on_the_error_path() {
    let root = TempDir::new("no-active-task-done");
    init_empty_project(&root);

    let output = spectra()
        .args(["task", "done", "1"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"Error: No active changes. Create one with: spectra new change <name>\n"
    );
}

#[test]
fn list_and_validate_keep_their_probed_empty_outputs() {
    let root = TempDir::new("no-active-list-validate");
    init_empty_project(&root);

    let cases: &[(&str, &[&str], &[u8])] = &[
        ("list", &["list"], b"No active changes.\n"),
        (
            "list --json",
            &["list", "--json"],
            b"{\n  \"changes\": []\n}\n",
        ),
        // oracle 3.0.0：沒有 change 時什麼都不印（W9 RE 規格 A2，p09 實測）。
        ("validate", &["validate"], b""),
    ];

    for &(label, args, expected_stdout) in cases {
        let output = spectra().args(args).current_dir(&*root).output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{label} 應結束於狀態碼 0");
        assert_eq!(output.stdout, expected_stdout, "{label} 的 stdout 不符預期");
        assert!(output.stderr.is_empty(), "{label} 的 stderr 應為空");
    }

    let json_out = spectra()
        .args(["validate", "--json"])
        .current_dir(&*root)
        .output()
        .unwrap();
    assert_eq!(
        json_out.status.code(),
        Some(0),
        "validate --json 應結束於狀態碼 0"
    );
    // oracle 格式（預設）：空陣列。
    assert_eq!(json_out.stdout, b"[]\n");
    assert!(json_out.stderr.is_empty());
}
