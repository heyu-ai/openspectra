//! Integration contract for `spectra config` (path/list/get/set/unset/reset/
//! edit), pinned against the closed-source 2.3.1 oracle's probed output —
//! see `docs/reverse-engineering/config.md`. Every invocation points `HOME`
//! (and clears `XDG_CONFIG_HOME`) at a per-test temp dir, so the suite never
//! touches the operator's real global config.

mod common;

use common::TempDir;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A `spectra` command whose global-config env is jailed to `home`.
fn spectra_cfg(home: &Path) -> Command {
    let mut cmd = common::spectra();
    cmd.env("HOME", home).env_remove("XDG_CONFIG_HOME");
    cmd
}

/// Where the jailed config file lands on this platform (the probed macOS
/// layout; the XDG fallback elsewhere).
fn config_file(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/openspec/config.yaml")
    } else {
        home.join(".config/openspec/config.yaml")
    }
}

fn run(home: &Path, args: &[&str]) -> std::process::Output {
    spectra_cfg(home).args(args).output().unwrap()
}

fn run_ok(home: &Path, args: &[&str]) -> String {
    let out = run(home, args);
    assert!(out.status.success(), "spectra {args:?} failed: {out:?}");
    assert!(
        out.stderr.is_empty(),
        "unexpected stderr for {args:?}: {out:?}"
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn path_prints_the_config_file_location_without_creating_anything() {
    let home = TempDir::new("config-path");
    let stdout = run_ok(&home, &["config", "path"]);
    assert_eq!(stdout, format!("{}\n", config_file(&home).display()));
    // Probed: `path` is a pure print — no directories are created.
    assert!(!config_file(&home).parent().unwrap().exists());
}

#[test]
fn config_works_without_an_initialized_project() {
    // All tests here run from an empty temp HOME with no `.spectra.yaml`
    // anywhere above (probed: the oracle needs no project). Make the working
    // directory explicit for this one to pin that contract.
    let home = TempDir::new("config-noproj");
    let out = spectra_cfg(&home)
        .args(["config", "list"])
        .current_dir(&*home)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout, b"No configuration set.\n");
}

#[test]
fn list_empty_prints_the_oracle_sentinel_in_both_modes() {
    let home = TempDir::new("config-list-empty");
    assert_eq!(
        run_ok(&home, &["config", "list"]),
        "No configuration set.\n"
    );
    assert_eq!(run_ok(&home, &["config", "list", "--json"]), "{}\n");
}

#[test]
fn set_get_round_trips_typed_values() {
    let home = TempDir::new("config-roundtrip");
    // Probed echo: the raw CLI argument, not the parsed value.
    assert_eq!(
        run_ok(&home, &["config", "set", "parallel_tasks", "TRUE"]),
        "\u{2713} parallel_tasks = TRUE\n"
    );
    assert_eq!(
        run_ok(&home, &["config", "get", "parallel_tasks"]),
        "true\n"
    );
    run_ok(&home, &["config", "set", "answer", "42"]);
    assert_eq!(run_ok(&home, &["config", "get", "answer"]), "42\n");
    run_ok(&home, &["config", "set", "greet", "hello world"]);
    assert_eq!(run_ok(&home, &["config", "get", "greet"]), "hello world\n");
}

#[test]
fn get_of_a_null_value_prints_the_probed_double_newline() {
    let home = TempDir::new("config-null");
    // Probed: an empty value stores YAML null, and `get` renders it through
    // the YAML serializer — `null\n` plus the println newline.
    assert_eq!(
        run_ok(&home, &["config", "set", "empty", ""]),
        "\u{2713} empty = \n"
    );
    assert_eq!(run_ok(&home, &["config", "get", "empty"]), "null\n\n");
}

#[test]
fn get_missing_key_matches_the_oracle_error_contract() {
    let home = TempDir::new("config-get-missing");
    let out = run(&home, &["config", "get", "nonexistent_key"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty(), "no partial stdout: {out:?}");
    assert_eq!(out.stderr, b"Error: Key 'nonexistent_key' not found.\n");
}

#[test]
fn set_with_string_flag_stores_a_quoted_string() {
    let home = TempDir::new("config-string-flag");
    run_ok(&home, &["config", "set", "tdd", "true", "--string"]);
    assert_eq!(
        std::fs::read_to_string(config_file(&home)).unwrap(),
        "tdd: 'true'\n"
    );
    // Rendered indistinguishably from a real bool (probed).
    assert_eq!(run_ok(&home, &["config", "get", "tdd"]), "true\n");
}

#[test]
fn set_accepts_unknown_keys_with_and_without_allow_unknown() {
    let home = TempDir::new("config-unknown");
    // Probed: 2.3.1 does not validate key names; --allow-unknown is inert.
    assert_eq!(
        run_ok(&home, &["config", "set", "unknown_key", "hello"]),
        "\u{2713} unknown_key = hello\n"
    );
    assert_eq!(
        run_ok(
            &home,
            &["config", "set", "unknown_key", "hello", "--allow-unknown"]
        ),
        "\u{2713} unknown_key = hello\n"
    );
}

#[test]
fn dotted_keys_are_flat_not_nested() {
    let home = TempDir::new("config-dotted");
    run_ok(&home, &["config", "set", "claude_effort.apply", "high"]);
    assert_eq!(
        std::fs::read_to_string(config_file(&home)).unwrap(),
        "claude_effort.apply: high\n"
    );
    assert_eq!(
        run_ok(&home, &["config", "get", "claude_effort.apply"]),
        "high\n"
    );
}

#[test]
fn list_sorts_keys_and_renders_nulls_with_a_blank_line() {
    let home = TempDir::new("config-list-bytes");
    run_ok(&home, &["config", "set", "pi", "2.5"]);
    run_ok(&home, &["config", "set", "answer", "42"]);
    run_ok(&home, &["config", "set", "empty", ""]);
    // Byte-golden against the probed oracle listing (sorted; the null's
    // serializer newline yields the blank separator line).
    assert_eq!(
        run_ok(&home, &["config", "list"]),
        "answer = 42\nempty = null\n\npi = 2.5\n"
    );
}

#[test]
fn list_json_preserves_types() {
    let home = TempDir::new("config-list-json");
    run_ok(&home, &["config", "set", "tdd", "true"]);
    run_ok(&home, &["config", "set", "answer", "42"]);
    run_ok(&home, &["config", "set", "tag", "v1", "--string"]);
    run_ok(&home, &["config", "set", "arr", "[1, 2, 3]"]);
    run_ok(&home, &["config", "set", "obj", "{a: 1}"]);
    let parsed: serde_json::Value =
        serde_json::from_str(&run_ok(&home, &["config", "list", "--json"])).unwrap();
    assert_eq!(
        parsed,
        serde_json::json!({
            "tdd": true,
            "answer": 42,
            "tag": "v1",
            "arr": [1, 2, 3],
            "obj": {"a": 1},
        })
    );
}

#[test]
fn flow_style_values_render_as_block_yaml_on_get() {
    let home = TempDir::new("config-flow");
    run_ok(&home, &["config", "set", "arr", "[1, 2, 3]"]);
    // Probed bytes: block-style YAML plus the println newline.
    assert_eq!(
        run_ok(&home, &["config", "get", "arr"]),
        "- 1\n- 2\n- 3\n\n"
    );
}

#[test]
fn unset_is_idempotent_and_creates_the_file_when_missing() {
    let home = TempDir::new("config-unset");
    // Probed: unset of a missing key on a missing file still reports success
    // — and leaves an empty `{}` config behind.
    assert_eq!(
        run_ok(&home, &["config", "unset", "ghost"]),
        "\u{2713} Removed key: ghost\n"
    );
    assert_eq!(std::fs::read_to_string(config_file(&home)).unwrap(), "{}\n");
    run_ok(&home, &["config", "set", "only", "one"]);
    assert_eq!(
        run_ok(&home, &["config", "unset", "only"]),
        "\u{2713} Removed key: only\n"
    );
    assert_eq!(
        run_ok(&home, &["config", "list"]),
        "No configuration set.\n"
    );
}

#[test]
fn reset_deletes_the_file_and_accepts_the_inert_flags() {
    let home = TempDir::new("config-reset");
    run_ok(&home, &["config", "set", "k", "v"]);
    assert!(config_file(&home).exists());
    assert_eq!(
        run_ok(&home, &["config", "reset"]),
        "\u{2713} Config reset.\n"
    );
    assert!(!config_file(&home).exists());
    // Probed: a second reset (and --all/-y) still succeed with no prompt.
    assert_eq!(
        run_ok(&home, &["config", "reset", "--all", "-y"]),
        "\u{2713} Config reset.\n"
    );
}

#[test]
fn corrupt_config_file_reads_as_empty_and_is_overwritten_by_set() {
    let home = TempDir::new("config-corrupt");
    let file = config_file(&home);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "not: [valid: yaml\n").unwrap();
    // Probed: no error, no backup — the oracle just sees an empty config.
    assert_eq!(
        run_ok(&home, &["config", "list"]),
        "No configuration set.\n"
    );
    let out = run(&home, &["config", "get", "anything"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stderr, b"Error: Key 'anything' not found.\n");
    run_ok(&home, &["config", "set", "k", "v"]);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "k: v\n");
}

#[test]
fn edit_propagates_the_editor_exit_status() {
    let home = TempDir::new("config-edit");
    // `true`/`false` exist on every CI platform and take the path argument
    // silently, standing in for an editor that saves/fails.
    let ok = spectra_cfg(&home)
        .args(["config", "edit"])
        .env("EDITOR", "true")
        .output()
        .unwrap();
    assert!(ok.status.success(), "{ok:?}");
    assert!(ok.stdout.is_empty());

    let fail = spectra_cfg(&home)
        .args(["config", "edit"])
        .env("EDITOR", "false")
        .output()
        .unwrap();
    assert_eq!(fail.status.code(), Some(1));
    // Probed oracle error when the editor exits non-zero.
    assert_eq!(fail.stderr, b"Error: Editor exited with error.\n");
}

#[cfg(target_os = "linux")]
#[test]
fn xdg_config_home_wins_over_home_on_linux() {
    let home = TempDir::new("config-xdg-home");
    let xdg = TempDir::new("config-xdg-dir");
    let out = common::spectra()
        .args(["config", "path"])
        .env("HOME", &*home)
        .env("XDG_CONFIG_HOME", &*xdg)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{}\n", xdg.join("openspec/config.yaml").display())
    );
}
