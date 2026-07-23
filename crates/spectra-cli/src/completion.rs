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

fn completion_path(shell: Shell) -> Result<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match shell {
        Shell::Bash => {
            let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
            bash_completion_path(xdg_data_home.as_deref(), home.as_deref())
        }
        Shell::Fish => {
            let xdg_config_home = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
            fish_completion_path(xdg_config_home.as_deref(), home.as_deref())
        }
        Shell::Zsh => {
            let zdotdir = std::env::var_os("ZDOTDIR").map(PathBuf::from);
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

pub(crate) fn install(shell: Shell, verbose: bool) -> Result<i32> {
    let path = completion_path(shell)?;
    let parent = path
        .parent()
        .context("completion path has no parent directory")?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("creating completion directory {}", parent.display()))?;

    let script = generate_script(shell);
    std::fs::write(&path, &script)
        .with_context(|| format!("writing completion script {}", path.display()))?;

    println!("Installed {shell} completion to {}.", path.display());
    if verbose {
        println!("Completion path: {}", path.display());
        println!("Bytes written: {}", script.len());
    }
    if shell == Shell::Zsh {
        println!("Hint: ensure .zshrc contains `fpath+=~/.zfunc` before `compinit`.");
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
