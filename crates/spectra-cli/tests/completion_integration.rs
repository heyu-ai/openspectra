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

#[test]
fn install_bash_writes_completion_to_xdg_data_home() {
    let root = TempDir::new("completion-install-bash");
    let output = isolated_spectra(&root)
        .args(["completion", "install", "bash"])
        .output()
        .unwrap();

    assert!(output.status.success(), "bash install failed: {output:?}");
    let path = root.join("xdg-data/bash-completion/completions/spectra");
    assert!(
        path.is_file(),
        "completion file missing at {}",
        path.display()
    );
    let script = std::fs::read_to_string(path).unwrap();
    assert!(script.contains("spectra"));
}

#[test]
fn installing_bash_twice_is_idempotent() {
    let root = TempDir::new("completion-install-twice");

    for attempt in 1..=2 {
        let output = isolated_spectra(&root)
            .args(["completion", "install", "bash"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "bash install attempt {attempt} failed: {output:?}"
        );
    }
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
    assert!(root.join("zdotdir/.zfunc/_spectra").is_file());
    assert!(
        !root.join("zdotdir/.zshrc").exists(),
        "zsh install must not edit .zshrc"
    );

    let fish = isolated_spectra(&root)
        .args(["completion", "install", "fish"])
        .output()
        .unwrap();
    assert!(fish.status.success(), "fish install failed: {fish:?}");
    assert!(root
        .join("xdg-config/fish/completions/spectra.fish")
        .is_file());
}

#[test]
fn install_elvish_fails_with_generate_guidance() {
    let root = TempDir::new("completion-install-elvish");
    let output = isolated_spectra(&root)
        .args(["completion", "install", "elvish"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "elvish install unexpectedly succeeded: {output:?}"
    );
    let message = combined_output(&output);
    assert!(
        message.contains("spectra completion generate elvish"),
        "elvish guidance missing: {output:?}"
    );
}
