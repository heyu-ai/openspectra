use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::Shell;

use crate::Cli;

fn bash_completion_path(xdg_data_home: Option<&Path>, home: Option<&Path>) -> Result<PathBuf> {
    let base = if let Some(xdg_data_home) = xdg_data_home {
        xdg_data_home.to_path_buf()
    } else {
        home.context("HOME is not set")?.join(".local/share")
    };
    Ok(base.join("bash-completion/completions/spectra"))
}

fn fish_completion_path(xdg_config_home: Option<&Path>, home: Option<&Path>) -> Result<PathBuf> {
    let base = if let Some(xdg_config_home) = xdg_config_home {
        xdg_config_home.to_path_buf()
    } else {
        home.context("HOME is not set")?.join(".config")
    };
    Ok(base.join("fish/completions/spectra.fish"))
}

fn zsh_completion_path(zdotdir: Option<&Path>, home: Option<&Path>) -> Result<PathBuf> {
    let base = zdotdir.or(home).context("HOME is not set")?;
    Ok(base.join(".zfunc/_spectra"))
}

fn unsupported_install(shell: Shell, operation: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{operation} {shell} completion is not supported; use 'spectra completion generate {shell}' and source it manually"
    )
}

/// An exported-but-empty variable means "unset", not "the empty path".
///
/// `var_os` returns `Some("")` for `XDG_DATA_HOME=`, and `PathBuf::from("")`
/// joins into a *relative* path -- so without this filter `install` writes
/// `bash-completion/completions/spectra` under whatever directory the user
/// happens to be standing in, prints that relative path as if it were
/// correct, and exits 0. `uninstall -y` then deletes relative to the cwd.
/// The XDG Base Directory spec is explicit that an unset *or empty* value
/// takes the `$HOME` fallback; empty `HOME`/`ZDOTDIR` get the same treatment
/// so they fail loudly ("HOME is not set") instead of resolving to `.`.
fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn completion_path(shell: Shell) -> Result<PathBuf> {
    let home = env_path("HOME");
    match shell {
        Shell::Bash => {
            let xdg_data_home = env_path("XDG_DATA_HOME");
            bash_completion_path(xdg_data_home.as_deref(), home.as_deref())
        }
        Shell::Fish => {
            let xdg_config_home = env_path("XDG_CONFIG_HOME");
            fish_completion_path(xdg_config_home.as_deref(), home.as_deref())
        }
        Shell::Zsh => {
            let zdotdir = env_path("ZDOTDIR");
            zsh_completion_path(zdotdir.as_deref(), home.as_deref())
        }
        Shell::Elvish | Shell::PowerShell => Err(unsupported_install(shell, "installing")),
        _ => Err(unsupported_install(shell, "installing")),
    }
}

fn generate_script(shell: Shell) -> Vec<u8> {
    let mut script = Vec::new();
    clap_complete::generate(shell, &mut Cli::command(), "spectra", &mut script);
    script
}

/// Write `contents` to `path` via a sibling temp file plus `rename`.
///
/// `std::fs::write` **follows** an existing symlink at `path` and truncates
/// whatever it points at: with `~/.local/share/bash-completion/completions/spectra`
/// symlinked to `~/.bashrc`, a plain write replaces the user's rc file with a
/// completion script -- breaking this command's strongest guarantee (it never
/// touches rc files) while reporting success. `rename` replaces the directory
/// entry itself, so the symlink is swapped out rather than traversed, and it
/// closes the check-then-write race a `symlink_metadata` guard would leave
/// open. Mirrors `spectra-core`'s `init::write_atomically`.
fn write_replacing_any_symlink(path: &Path, contents: &[u8]) -> Result<()> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let file_name = path
        .file_name()
        .context("completion path has no file name")?
        .to_string_lossy()
        .into_owned();
    let tmp_path = path.with_file_name(format!("{file_name}.tmp-{}-{seq}", std::process::id()));

    if let Err(e) = std::fs::write(&tmp_path, contents) {
        cleanup_temp_file(&tmp_path);
        return Err(e).with_context(|| format!("writing {}", tmp_path.display()));
    }
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        cleanup_temp_file(&tmp_path);
        return Err(e)
            .with_context(|| format!("installing completion script to {}", path.display()));
    }
    Ok(())
}

/// Best-effort cleanup after a failed write or rename. The caller always
/// returns the primary error; a secondary failure is logged rather than
/// silently dropped. `NotFound` is skipped: it means the write never created
/// the temp file, so there is nothing to clean up and no second fault.
fn cleanup_temp_file(tmp_path: &Path) {
    if let Err(e) = std::fs::remove_file(tmp_path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            eprintln!(
                "warning: failed to remove temp file {}: {e}",
                tmp_path.display()
            );
        }
    }
}

pub(crate) fn install(shell: Shell, verbose: bool) -> Result<i32> {
    let path = completion_path(shell)?;
    let parent = path
        .parent()
        .context("completion path has no parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating completion directory {}", parent.display()))?;

    let script = generate_script(shell);
    write_replacing_any_symlink(&path, &script)?;

    println!("Installed {shell} completion to {}.", path.display());
    if verbose {
        println!("Bytes written: {}", script.len());
    }
    if shell == Shell::Zsh {
        // Must name the directory actually written to: the path honours
        // ZDOTDIR, so a hardcoded `~/.zfunc` tells a ZDOTDIR user to add a
        // directory the script is not in -- following it verbatim leaves
        // completion silently non-functional, and contradicts the path this
        // command just printed one line above.
        println!(
            "Hint: ensure your .zshrc contains `fpath+=({})` before `compinit`.",
            parent.display()
        );
    }
    Ok(0)
}

pub(crate) fn uninstall(shell: Shell, yes: bool) -> Result<i32> {
    let path = completion_path(shell).map_err(|error| {
        if matches!(shell, Shell::Elvish | Shell::PowerShell) {
            unsupported_install(shell, "uninstalling")
        } else {
            error
        }
    })?;

    if !path
        .try_exists()
        .with_context(|| format!("checking completion script {}", path.display()))?
    {
        println!("Completion for {shell} is not installed.");
        return Ok(0);
    }

    let stdin = std::io::stdin();
    if !yes && stdin.is_terminal() {
        print!("Remove {shell} completion from {}? [y/N] ", path.display());
        std::io::stdout()
            .flush()
            .context("flushing confirmation prompt")?;
        let mut answer = String::new();
        stdin
            .read_line(&mut answer)
            .context("reading confirmation")?;
        if !answer.trim().eq_ignore_ascii_case("y") {
            println!("Uninstall cancelled.");
            return Ok(0);
        }
    }

    std::fs::remove_file(&path)
        .with_context(|| format!("removing completion script {}", path.display()))?;
    println!("Uninstalled {shell} completion from {}.", path.display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_path_uses_xdg_data_home_when_set() {
        let path =
            bash_completion_path(Some(Path::new("/xdg-data")), Some(Path::new("/home"))).unwrap();
        assert_eq!(
            path,
            PathBuf::from("/xdg-data/bash-completion/completions/spectra")
        );
    }

    /// The filter lives in `env_path`, so the pure helpers below never see an
    /// empty value -- this pins the filter itself, which is where the CWD
    /// regression came from.
    #[test]
    fn env_path_treats_an_exported_empty_value_as_unset() {
        // SAFETY: single-threaded test process section; the variable name is
        // unique to this test so no sibling test observes it.
        unsafe {
            std::env::set_var("SPECTRA_TEST_EMPTY_ENV_PATH", "");
            std::env::set_var("SPECTRA_TEST_SET_ENV_PATH", "/somewhere");
        }
        assert_eq!(env_path("SPECTRA_TEST_EMPTY_ENV_PATH"), None);
        assert_eq!(
            env_path("SPECTRA_TEST_SET_ENV_PATH"),
            Some(PathBuf::from("/somewhere"))
        );
        assert_eq!(env_path("SPECTRA_TEST_ABSENT_ENV_PATH"), None);
        unsafe {
            std::env::remove_var("SPECTRA_TEST_EMPTY_ENV_PATH");
            std::env::remove_var("SPECTRA_TEST_SET_ENV_PATH");
        }
    }

    #[test]
    fn bash_path_falls_back_to_home_when_xdg_data_home_is_unset() {
        let path = bash_completion_path(None, Some(Path::new("/home"))).unwrap();
        assert_eq!(
            path,
            PathBuf::from("/home/.local/share/bash-completion/completions/spectra")
        );
    }

    #[test]
    fn fish_path_uses_xdg_config_home_when_set() {
        let path =
            fish_completion_path(Some(Path::new("/xdg-config")), Some(Path::new("/home"))).unwrap();
        assert_eq!(
            path,
            PathBuf::from("/xdg-config/fish/completions/spectra.fish")
        );
    }

    #[test]
    fn fish_path_falls_back_to_home_when_xdg_config_home_is_unset() {
        let path = fish_completion_path(None, Some(Path::new("/home"))).unwrap();
        assert_eq!(
            path,
            PathBuf::from("/home/.config/fish/completions/spectra.fish")
        );
    }

    #[test]
    fn zsh_path_uses_zdotdir_when_set() {
        let path =
            zsh_completion_path(Some(Path::new("/zdotdir")), Some(Path::new("/home"))).unwrap();
        assert_eq!(path, PathBuf::from("/zdotdir/.zfunc/_spectra"));
    }

    #[test]
    fn zsh_path_falls_back_to_home_when_zdotdir_is_unset() {
        let path = zsh_completion_path(None, Some(Path::new("/home"))).unwrap();
        assert_eq!(path, PathBuf::from("/home/.zfunc/_spectra"));
    }
}
