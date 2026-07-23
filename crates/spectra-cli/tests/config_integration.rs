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

/// Parsing before comparing (as the test above does) is blind to whitespace,
/// so the oracle's 2-space pretty shape needs its own byte-level pin -- a
/// silent drop to compact JSON would otherwise ship green.
#[test]
fn list_json_is_two_space_pretty_printed() {
    let home = TempDir::new("config-json-bytes");
    run_ok(&home, &["config", "set", "answer", "42"]);
    run_ok(&home, &["config", "set", "tdd", "true"]);
    assert_eq!(
        run_ok(&home, &["config", "list", "--json"]),
        "{\n  \"answer\": 42,\n  \"tdd\": true\n}\n"
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
fn plain_reset_truncates_to_an_empty_mapping_rather_than_deleting() {
    let home = TempDir::new("config-reset-plain");
    run_ok(&home, &["config", "set", "k", "v"]);
    assert_eq!(
        run_ok(&home, &["config", "reset"]),
        "\u{2713} Config reset.\n"
    );
    // Probed: the oracle leaves `{}\n` behind -- it does NOT delete the file.
    assert_eq!(std::fs::read_to_string(config_file(&home)).unwrap(), "{}\n");
    assert_eq!(
        run_ok(&home, &["config", "list"]),
        "No configuration set.\n"
    );
    // Idempotent, and `-y` alone does not change the mode.
    run_ok(&home, &["config", "reset", "-y"]);
    assert_eq!(std::fs::read_to_string(config_file(&home)).unwrap(), "{}\n");
}

#[test]
fn plain_reset_seeds_a_config_that_never_existed() {
    let home = TempDir::new("config-reset-virgin");
    assert_eq!(
        run_ok(&home, &["config", "reset"]),
        "\u{2713} Config reset.\n"
    );
    // Probed: on a virgin HOME the oracle creates the dir and an empty mapping.
    assert_eq!(std::fs::read_to_string(config_file(&home)).unwrap(), "{}\n");
}

#[test]
fn reset_all_deletes_the_file_and_is_idempotent() {
    let home = TempDir::new("config-reset-all");
    run_ok(&home, &["config", "set", "k", "v"]);
    assert!(config_file(&home).exists());
    // Probed: `--all` is NOT an inert flag -- it switches truncate to delete.
    assert_eq!(
        run_ok(&home, &["config", "reset", "--all"]),
        "\u{2713} Config reset.\n"
    );
    assert!(!config_file(&home).exists());
    // A second `--all` reset still succeeds; `-y` never prompts.
    assert_eq!(
        run_ok(&home, &["config", "reset", "--all", "-y"]),
        "\u{2713} Config reset.\n"
    );
    assert!(!config_file(&home).exists());
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

/// Write an executable stub "editor" that records its argv into `receipt`, so
/// tests can assert *which* program was launched and *which* path it was
/// handed -- `true`/`false` ignore argv entirely and cannot show either.
#[cfg(unix)]
fn stub_editor(dir: &Path, name: &str, receipt: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let script = dir.join(name);
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '{name} %s' \"$1\" > '{}'\nexit 0\n",
            receipt.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

/// Pins the whole `edit` launch contract in one place: the probed
/// `EDITOR` -> `VISUAL` -> `vi` precedence, that an *empty* `EDITOR` reaches
/// spawn instead of being treated as unset, and that the config path (not some
/// other path) is what gets handed to the editor.
#[cfg(unix)]
#[test]
fn edit_resolves_editor_visual_then_vi_and_passes_the_config_path() {
    let home = TempDir::new("config-edit-precedence");
    let bin = TempDir::new("config-edit-bin");
    let receipt = bin.join("receipt.txt");
    for name in ["vi", "myeditor", "myvisual"] {
        stub_editor(&bin, name, &receipt);
    }
    let expected_path = config_file(&home);

    let run_edit = |envs: &[(&str, &str)], remove: &[&str]| -> String {
        let _ = std::fs::remove_file(&receipt);
        let mut cmd = spectra_cfg(&home);
        cmd.args(["config", "edit"]).env("PATH", &*bin);
        for (k, v) in envs {
            cmd.env(k, v);
        }
        for k in remove {
            cmd.env_remove(k);
        }
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "{out:?}");
        std::fs::read_to_string(&receipt).unwrap()
    };

    // EDITOR wins over VISUAL, and receives the jailed config path.
    assert_eq!(
        run_edit(&[("EDITOR", "myeditor"), ("VISUAL", "myvisual")], &[]),
        format!("myeditor {}", expected_path.display())
    );
    // VISUAL is consulted when EDITOR is absent (probed against the oracle).
    assert_eq!(
        run_edit(&[("VISUAL", "myvisual")], &["EDITOR"]),
        format!("myvisual {}", expected_path.display())
    );
    // Neither set -> `vi` (probed: the oracle looks up `vi`, not `vim`).
    assert_eq!(
        run_edit(&[], &["EDITOR", "VISUAL"]),
        format!("vi {}", expected_path.display())
    );
}

/// Probed: an empty `EDITOR` is *not* treated as unset -- the oracle attempts
/// to spawn `""` and fails, rather than falling back to VISUAL or `vi`.
#[test]
fn edit_with_an_empty_editor_fails_instead_of_falling_back() {
    let home = TempDir::new("config-edit-empty");
    let out = spectra_cfg(&home)
        .args(["config", "edit"])
        .env("EDITOR", "")
        .env("VISUAL", "true")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.starts_with("Error: Failed to open editor '':"),
        "unexpected stderr: {stderr}"
    );
}

/// Probed: the oracle's stderr for an unspawnable editor.
#[test]
fn edit_spawn_failure_matches_the_oracle_wording() {
    let home = TempDir::new("config-edit-nospawn");
    let out = spectra_cfg(&home)
        .args(["config", "edit"])
        .env("EDITOR", "definitely_not_a_real_editor")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.starts_with("Error: Failed to open editor 'definitely_not_a_real_editor':"),
        "unexpected stderr: {stderr}"
    );
}

/// Probed: `edit` creates the config dir and seeds a missing file with the
/// oracle's exact header before spawning, so the editor can actually save --
/// and it leaves an existing config untouched.
#[test]
fn edit_seeds_a_missing_config_and_preserves_an_existing_one() {
    let home = TempDir::new("config-edit-seed");
    let out = spectra_cfg(&home)
        .args(["config", "edit"])
        .env("EDITOR", "true")
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        std::fs::read_to_string(config_file(&home)).unwrap(),
        "# OpenSpec global config\n"
    );

    run_ok(&home, &["config", "set", "keep", "me"]);
    let again = spectra_cfg(&home)
        .args(["config", "edit"])
        .env("EDITOR", "true")
        .output()
        .unwrap();
    assert!(again.status.success());
    assert_eq!(
        std::fs::read_to_string(config_file(&home)).unwrap(),
        "keep: me\n"
    );
}

/// Probed: an unreadable config must survive a refused write rather than being
/// replaced by a file holding only the new key. This is the data-loss guard.
#[cfg(unix)]
#[test]
fn writes_refuse_an_unreadable_config_instead_of_clobbering_it() {
    use std::os::unix::fs::PermissionsExt;

    let home = TempDir::new("config-unreadable");
    run_ok(&home, &["config", "set", "alpha", "1"]);
    run_ok(&home, &["config", "set", "beta", "2"]);
    let file = config_file(&home);
    let original = std::fs::read_to_string(&file).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_to_string(&file).is_ok() {
        // Announce the skip (and restore the mode) rather than returning
        // silently: this is the only CLI-level coverage of the data-loss
        // guard, so a quiet skip on a root runner reads as "verified".
        eprintln!(
            "skipping writes_refuse_an_unreadable_config_instead_of_clobbering_it: \
             running as root (chmod 0o000 not enforced)"
        );
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        return;
    }

    for args in [
        vec!["config", "set", "delta", "4"],
        vec!["config", "unset", "alpha"],
        vec!["config", "reset"],
    ] {
        let out = run(&home, &args);
        assert_eq!(out.status.code(), Some(1), "{args:?} should fail: {out:?}");
        assert!(out.stdout.is_empty(), "{args:?} must not print success");
        // Pin the exact wording, not just "it failed": the oracle's message is
        // part of AC-1, and asserting only the exit code lets a reworded (or
        // wrongly-classified) error ship green -- which is how the non-UTF-8
        // regression stayed invisible to this suite.
        assert_eq!(
            String::from_utf8(out.stderr).unwrap(),
            "Error: Permission denied (os error 13)\n",
            "{args:?} stderr must match the oracle"
        );
    }

    // Display paths stay lenient, matching the oracle.
    assert_eq!(
        run_ok(&home, &["config", "list"]),
        "No configuration set.\n"
    );

    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
}

/// The documented exception to the rule above: `reset --all` never reads, so
/// it deletes an unreadable config and exits 0 (probed).
#[cfg(unix)]
#[test]
fn reset_all_deletes_even_an_unreadable_config() {
    use std::os::unix::fs::PermissionsExt;

    let home = TempDir::new("config-unreadable-all");
    run_ok(&home, &["config", "set", "alpha", "1"]);
    let file = config_file(&home);
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_to_string(&file).is_ok() {
        eprintln!(
            "skipping reset_all_deletes_even_an_unreadable_config: \
             running as root (chmod 0o000 not enforced)"
        );
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        return;
    }

    assert_eq!(
        run_ok(&home, &["config", "reset", "--all"]),
        "\u{2713} Config reset.\n"
    );
    assert!(!file.exists());
}

/// Probed: when a path component is a regular file the removal fails ENOTDIR,
/// and the oracle -- like us -- treats the config as simply absent and exits 0.
#[test]
fn reset_all_exits_zero_when_the_config_parent_is_a_regular_file() {
    let home = TempDir::new("config-reset-enotdir");
    let parent = config_file(&home);
    let parent = parent.parent().unwrap();
    std::fs::create_dir_all(parent.parent().unwrap()).unwrap();
    std::fs::write(parent, "i am a file\n").unwrap();
    assert_eq!(
        run_ok(&home, &["config", "reset", "--all"]),
        "\u{2713} Config reset.\n"
    );
}

/// Probed: the oracle emits the bare OS error when it cannot create the config
/// directory; ours must not prefix it with `creating <path>:`.
#[cfg(unix)]
#[test]
fn a_failed_directory_creation_reports_the_bare_os_error() {
    use std::os::unix::fs::PermissionsExt;

    let home = TempDir::new("config-readonly-home");
    std::fs::set_permissions(&*home, std::fs::Permissions::from_mode(0o555)).unwrap();
    let probe = home.join("probe-write-check");
    if std::fs::write(&probe, b"x").is_ok() {
        eprintln!(
            "skipping a_failed_directory_creation_reports_the_bare_os_error: \
             running as root (mode 0o555 not enforced)"
        );
        let _ = std::fs::remove_file(&probe);
        std::fs::set_permissions(&*home, std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }

    // Both write entry points must be covered: `set` reaches `create_dir_all`
    // through `save`, `edit` through `ensure_editable`. Testing only one lets
    // the other's context wrapper come back unnoticed.
    let set_out = run(&home, &["config", "set", "k", "v"]);
    let edit_out = spectra_cfg(&home)
        .args(["config", "edit"])
        .env("EDITOR", "true")
        .output()
        .unwrap();
    std::fs::set_permissions(&*home, std::fs::Permissions::from_mode(0o755)).unwrap();

    for (label, out) in [("set", set_out), ("edit", edit_out)] {
        assert_eq!(out.status.code(), Some(1), "{label} should fail: {out:?}");
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(
            !stderr.contains("creating "),
            "{label} stderr must not carry a path-prefixed context: {stderr}"
        );
        assert_eq!(
            stderr, "Error: Permission denied (os error 13)\n",
            "{label}"
        );
    }
}

/// Probed: when the config path is a directory, `reset --all`'s `remove_file`
/// fails and the oracle prints the bare OS error -- no path prefix. Pins the
/// absence of a `with_context` wrapper on that arm.
#[test]
fn reset_all_on_a_directory_config_path_reports_the_bare_os_error() {
    let home = TempDir::new("config-reset-dir");
    std::fs::create_dir_all(config_file(&home)).unwrap();
    let out = run(&home, &["config", "reset", "--all"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        !stderr.contains("removing "),
        "stderr must not carry a path-prefixed context: {stderr}"
    );
    assert!(
        stderr.starts_with("Error: ") && stderr.contains("(os error "),
        "unexpected stderr: {stderr}"
    );
}

/// Non-UTF-8 bytes are a *content* problem, not an I/O fault: probed, the
/// oracle overwrites such a file exactly like corrupt YAML. Reading the config
/// via `read_to_string` would misclassify it as `InvalidData` and refuse the
/// write -- a regression this pins.
#[test]
fn non_utf8_config_reads_as_empty_and_is_overwritten_by_set() {
    let home = TempDir::new("config-non-utf8");
    let file = config_file(&home);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, b"alpha: 1\nbeta: \"\xff\xfe\"\n").unwrap();

    assert_eq!(
        run_ok(&home, &["config", "list"]),
        "No configuration set.\n"
    );
    assert_eq!(
        run_ok(&home, &["config", "set", "delta", "4"]),
        "\u{2713} delta = 4\n"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "delta: 4\n");
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
