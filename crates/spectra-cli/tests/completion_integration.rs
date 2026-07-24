mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::{spectra, TempDir};

fn isolated_spectra(root: &Path) -> Command {
    let mut command = spectra();
    command
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("ZDOTDIR", root.join("zdotdir"));
    command
}

fn combined_output(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn generate_supports_all_five_shells() {
    let root = TempDir::new("completion-generate");

    for shell in ["bash", "zsh", "fish", "elvish", "powershell"] {
        let output = spectra()
            .args(["completion", "generate", shell])
            .current_dir(&*root)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "{shell} generate failed: {output:?}"
        );
        assert!(!output.stdout.is_empty(), "{shell} output was empty");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("spectra"),
            "{shell} output did not mention spectra: {output:?}"
        );
    }
}

/// A marker unique to each shell's generated script. `contains("spectra")`
/// is true of all five, so it cannot tell them apart -- installing bash's
/// script into the zsh and fish locations passed every content assertion
/// until these markers landed.
fn shell_marker(shell: &str) -> &'static str {
    match shell {
        "bash" => "complete -F _spectra",
        "zsh" => "#compdef spectra",
        "fish" => "complete -c spectra",
        other => panic!("no marker registered for {other}"),
    }
}

#[test]
fn install_bash_writes_completion_to_xdg_data_home() {
    let root = TempDir::new("completion-install-bash");
    let output = isolated_spectra(&root)
        .args(["completion", "install", "bash", "--verbose"])
        .output()
        .unwrap();

    assert!(output.status.success(), "bash install failed: {output:?}");
    let path = root.join("xdg-data/bash-completion/completions/spectra");
    assert!(
        path.is_file(),
        "completion file missing at {}",
        path.display()
    );
    let script = std::fs::read_to_string(&path).unwrap();
    assert!(
        script.contains(shell_marker("bash")),
        "installed script is not a bash script: {script:.200}"
    );

    // --verbose is advertised in README; without this it can be deleted and
    // become a silent no-op with the suite green. The byte count must match
    // the file actually on disk, not just be present.
    let bytes = std::fs::metadata(&path).unwrap().len();
    assert!(
        combined_output(&output).contains(&format!("Bytes written: {bytes}")),
        "--verbose must report the real byte count ({bytes}): {output:?}"
    );
}

#[test]
fn installing_bash_twice_overwrites_rather_than_appends() {
    let root = TempDir::new("completion-install-twice");
    let path = root.join("xdg-data/bash-completion/completions/spectra");

    let mut after_each = Vec::new();
    for attempt in 1..=2 {
        let output = isolated_spectra(&root)
            .args(["completion", "install", "bash"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "bash install attempt {attempt} failed: {output:?}"
        );
        after_each.push(std::fs::read(&path).unwrap());
    }

    // Exit 0 twice is also what an *appending* write looks like, and that
    // would leave a doubled `_spectra()` definition that bash re-sources.
    assert!(!after_each[0].is_empty(), "first install wrote nothing");
    assert_eq!(
        after_each[0],
        after_each[1],
        "reinstall must overwrite, not grow the file ({} -> {} bytes)",
        after_each[0].len(),
        after_each[1].len()
    );
}

#[test]
fn install_never_follows_a_symlink_into_an_rc_file() {
    // The strongest promise this command makes is that it never touches rc
    // files. `std::fs::write` follows a symlink at the target path and
    // truncates whatever it points at, so a completion path symlinked to
    // ~/.bashrc silently replaced the user's config with a completion
    // script -- and reported success.
    let root = TempDir::new("completion-symlink");
    let home = root.join("home");
    let completions = root.join("xdg-data/bash-completion/completions");
    std::fs::create_dir_all(&completions).unwrap();
    std::fs::create_dir_all(&home).unwrap();

    let rc = home.join(".bashrc");
    let rc_contents = "# precious user config\nexport FOO=bar\n";
    std::fs::write(&rc, rc_contents).unwrap();

    let link = completions.join("spectra");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&rc, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&rc, &link).unwrap();

    let output = isolated_spectra(&root)
        .args(["completion", "install", "bash"])
        .output()
        .unwrap();
    assert!(output.status.success(), "bash install failed: {output:?}");

    assert_eq!(
        std::fs::read_to_string(&rc).unwrap(),
        rc_contents,
        "install must not write through a symlink into an rc file"
    );
    assert!(
        std::fs::read_to_string(&link)
            .unwrap()
            .contains(shell_marker("bash")),
        "the completion path itself must now hold the completion script"
    );
    let entries: Vec<String> = std::fs::read_dir(&completions)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        entries,
        vec!["spectra".to_string()],
        "no temp file may be left behind: {entries:?}"
    );
}

#[test]
fn empty_xdg_data_home_falls_back_to_home_not_the_current_directory() {
    // An exported-but-empty variable means "unset" per the XDG spec. Treating
    // it as a path makes it relative, so install wrote into whatever
    // directory the user was standing in and printed that as if correct.
    let root = TempDir::new("completion-empty-xdg");
    let home = root.join("home");
    let cwd = root.join("elsewhere");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();

    let output = spectra()
        .args(["completion", "install", "bash"])
        .current_dir(&cwd)
        .env("HOME", &home)
        .env("XDG_DATA_HOME", "")
        .output()
        .unwrap();
    assert!(output.status.success(), "bash install failed: {output:?}");

    assert!(
        !cwd.join("bash-completion").exists(),
        "empty XDG_DATA_HOME must not resolve relative to the cwd"
    );
    assert!(
        home.join(".local/share/bash-completion/completions/spectra")
            .is_file(),
        "empty XDG_DATA_HOME must take the $HOME fallback: {output:?}"
    );
}

#[test]
fn empty_home_is_an_error_not_a_relative_path() {
    let root = TempDir::new("completion-empty-home");
    let cwd = root.join("elsewhere");
    std::fs::create_dir_all(&cwd).unwrap();

    let output = spectra()
        .args(["completion", "install", "zsh"])
        .current_dir(&cwd)
        .env("HOME", "")
        .env("ZDOTDIR", "")
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "empty HOME must fail loudly, not write to the cwd: {output:?}"
    );
    assert!(
        !cwd.join(".zfunc").exists(),
        "nothing may be written when HOME is unusable"
    );
}

#[test]
fn uninstall_bash_with_yes_removes_completion() {
    let root = TempDir::new("completion-uninstall-bash");
    let install = isolated_spectra(&root)
        .args(["completion", "install", "bash"])
        .output()
        .unwrap();
    assert!(install.status.success(), "bash install failed: {install:?}");

    let path = root.join("xdg-data/bash-completion/completions/spectra");
    assert!(path.is_file());

    let uninstall = isolated_spectra(&root)
        .args(["completion", "uninstall", "bash", "-y"])
        .output()
        .unwrap();
    assert!(
        uninstall.status.success(),
        "bash uninstall failed: {uninstall:?}"
    );
    assert!(!path.exists(), "completion file was not removed");
}

#[test]
fn uninstalling_missing_completion_succeeds() {
    let root = TempDir::new("completion-uninstall-missing");
    let output = isolated_spectra(&root)
        .args(["completion", "uninstall", "bash", "-y"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "missing uninstall failed: {output:?}"
    );
    assert!(
        combined_output(&output)
            .to_ascii_lowercase()
            .contains("not installed"),
        "missing uninstall output was not actionable: {output:?}"
    );
}

#[test]
fn generate_rejects_unknown_shell_with_clap_exit_code() {
    let root = TempDir::new("completion-invalid-shell");
    let output = spectra()
        .args(["completion", "generate", "bogus-shell"])
        .current_dir(&*root)
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(2),
        "unexpected output: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid value"),
        "expected clap invalid-value error: {output:?}"
    );
}

#[test]
fn zsh_and_fish_install_to_their_native_autoload_directories() {
    let root = TempDir::new("completion-install-zsh-fish");

    let zsh = isolated_spectra(&root)
        .args(["completion", "install", "zsh"])
        .output()
        .unwrap();
    assert!(zsh.status.success(), "zsh install failed: {zsh:?}");
    let zsh_script = root.join("zdotdir/.zfunc/_spectra");
    assert!(zsh_script.is_file());
    assert!(
        std::fs::read_to_string(&zsh_script)
            .unwrap()
            .contains(shell_marker("zsh")),
        "zsh location must hold the zsh script, not another shell's"
    );
    // The path honours ZDOTDIR, so the hint must name that directory -- a
    // hardcoded ~/.zfunc sends ZDOTDIR users to a directory the script is
    // not in, leaving completion silently non-functional. Assert the exact
    // `fpath+=(...)` the user is told to paste: matching the bare directory
    // anywhere in the output is satisfied by the "Installed ... to <path>"
    // line printed just above, so a hardcoded hint would slip through.
    assert!(
        combined_output(&zsh).contains(&format!(
            "fpath+=('{}')",
            root.join("zdotdir/.zfunc").display()
        )),
        "zsh hint must tell the user to add the directory actually written to, \
         single-quoted so a path with spaces does not word-split: {zsh:?}"
    );

    let fish = isolated_spectra(&root)
        .args(["completion", "install", "fish"])
        .output()
        .unwrap();
    assert!(fish.status.success(), "fish install failed: {fish:?}");
    let fish_script = root.join("xdg-config/fish/completions/spectra.fish");
    assert!(fish_script.is_file());
    assert!(
        std::fs::read_to_string(&fish_script)
            .unwrap()
            .contains(shell_marker("fish")),
        "fish location must hold the fish script, not another shell's"
    );
}

#[test]
fn install_and_uninstall_never_modify_an_existing_rc_file() {
    // Asserting an rc file does not *exist* in a fresh temp dir proves
    // nothing -- it is satisfied by the fixture, not by the code. The real
    // failure mode is appending to an rc file that is already there, so the
    // rc files must exist first and be compared byte-for-byte afterwards.
    let root = TempDir::new("completion-rc-immutable");
    let rc_files = [
        ("home/.bashrc", "# bashrc sentinel\n"),
        ("home/.bash_profile", "# bash_profile sentinel\n"),
        ("zdotdir/.zshrc", "# zshrc sentinel\n"),
        ("xdg-config/fish/config.fish", "# fish config sentinel\n"),
    ];
    for (rel, contents) in rc_files {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, contents).unwrap();
    }

    for shell in ["bash", "zsh", "fish"] {
        for op in ["install", "uninstall"] {
            let mut command = isolated_spectra(&root);
            command.args(["completion", op, shell]);
            if op == "uninstall" {
                command.arg("-y");
            }
            let output = command.output().unwrap();
            assert!(output.status.success(), "{op} {shell} failed: {output:?}");
        }
    }

    for (rel, contents) in rc_files {
        assert_eq!(
            std::fs::read_to_string(root.join(rel)).unwrap(),
            contents,
            "{rel} was modified; this command must never touch rc files"
        );
    }
}

#[test]
fn generate_detects_the_shell_from_the_environment() {
    let root = TempDir::new("completion-detect-shell");

    // More than one shell on purpose: with a single sample, a detection that
    // ignores $SHELL and always returns that one shell passes -- and a bash
    // user would silently get the zsh script.
    for (shell_env, expected) in [
        ("/bin/bash", "bash"),
        ("/bin/zsh", "zsh"),
        ("/usr/local/bin/fish", "fish"),
    ] {
        let output = isolated_spectra(&root)
            .args(["completion", "generate"])
            .env("SHELL", shell_env)
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "auto-detection failed for {shell_env}: {output:?}"
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(shell_marker(expected)),
            "$SHELL={shell_env} must produce a {expected} script: {output:?}"
        );
    }
}

#[test]
fn generate_without_a_detectable_shell_exits_one() {
    let root = TempDir::new("completion-detect-fail");
    let output = isolated_spectra(&root)
        .args(["completion", "generate"])
        .env_remove("SHELL")
        .output()
        .unwrap();

    // Exit 1, not a silent default: falling back to bash would write the
    // wrong shell's completions into an unsuspecting user's home.
    assert_eq!(
        output.status.code(),
        Some(1),
        "undetectable shell must exit 1: {output:?}"
    );
    assert!(
        combined_output(&output).contains("could not detect shell"),
        "error must say detection failed: {output:?}"
    );
}

#[test]
fn install_and_uninstall_reject_unsupported_shells_with_generate_guidance() {
    let root = TempDir::new("completion-unsupported");

    for shell in ["elvish", "powershell"] {
        for op in ["install", "uninstall"] {
            let output = isolated_spectra(&root)
                .args(["completion", op, shell])
                .output()
                .unwrap();

            assert_eq!(
                output.status.code(),
                Some(1),
                "{op} {shell} must exit 1: {output:?}"
            );
            let message = combined_output(&output);
            assert!(
                message.contains(&format!("spectra completion generate {shell}")),
                "{op} {shell} guidance missing: {output:?}"
            );
            // The uninstall path rewrites the verb; without this a deleted
            // rewrite tells users that *installing* is unsupported.
            //
            // Assert the exact phrase, not `contains(verb)`: "uninstalling"
            // contains "installing", so a `contains` check on the install
            // branch is satisfied by the uninstall message and has zero
            // discriminating power.
            let expected_verb = if op == "install" {
                "installing"
            } else {
                "uninstalling"
            };
            assert!(
                message.contains(&format!(
                    "{expected_verb} {shell} completion is not supported"
                )),
                "{op} {shell} message must open with '{expected_verb}': {output:?}"
            );
            if op == "install" {
                assert!(
                    !message.contains("uninstalling"),
                    "install must not claim uninstalling is unsupported: {output:?}"
                );
            }
        }
    }
}

#[test]
fn relative_xdg_data_home_falls_back_to_home_not_the_current_directory() {
    // The empty-value case is not the whole story: a non-empty *relative*
    // value joins the same way, so install wrote under the cwd and printed
    // that relative path as if correct. The XDG spec requires an absolute
    // path and says an invalid value takes the $HOME fallback.
    let root = TempDir::new("completion-relative-xdg");
    let home = root.join("home");
    let cwd = root.join("elsewhere");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();

    let output = spectra()
        .args(["completion", "install", "bash"])
        .current_dir(&cwd)
        .env("HOME", &home)
        .env("XDG_DATA_HOME", "relative-xdg")
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("ZDOTDIR", root.join("zdotdir"))
        .output()
        .unwrap();
    assert!(output.status.success(), "bash install failed: {output:?}");

    assert!(
        !cwd.join("relative-xdg").exists(),
        "a relative XDG_DATA_HOME must not resolve against the cwd: {output:?}"
    );
    assert!(
        home.join(".local/share/bash-completion/completions/spectra")
            .is_file(),
        "a relative XDG_DATA_HOME must take the $HOME fallback: {output:?}"
    );
}

#[test]
fn install_leaves_no_temp_file_when_the_rename_fails() {
    // A pre-existing *directory* at the completion path lets the temp write
    // succeed but makes the rename fail, exercising the cleanup-on-rename-
    // failure branch. Without the cleanup call, the leftover `_spectra.tmp-*`
    // would sit in the user's fpath directory still carrying `#compdef
    // spectra` on its first line. Mirrors spectra-core's
    // `write_atomically_cleans_up_the_temp_file_when_rename_fails`.
    let root = TempDir::new("completion-rename-fails");
    let completions = root.join("xdg-data/bash-completion/completions");
    std::fs::create_dir_all(completions.join("spectra")).unwrap();

    let output = isolated_spectra(&root)
        .args(["completion", "install", "bash"])
        .output()
        .unwrap();

    assert_eq!(
        output.status.code(),
        Some(1),
        "install onto a directory must fail loudly: {output:?}"
    );
    let entries: Vec<String> = std::fs::read_dir(&completions)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        entries,
        vec!["spectra".to_string()],
        "a failed rename must leave no temp file behind: {entries:?}"
    );
}
