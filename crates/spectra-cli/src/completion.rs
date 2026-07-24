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

/// Accept an environment path only when it is usable as a base directory:
/// present, non-empty, and **absolute**.
///
/// Anything else must be treated as unset. `var_os` returns `Some("")` for
/// `XDG_DATA_HOME=`, and both an empty and a relative value join into a
/// *relative* path -- so without this filter `install` writes
/// `bash-completion/completions/spectra` under whatever directory the user
/// happens to be standing in, prints that relative path as if it were
/// correct, and exits 0; `uninstall -y` then deletes relative to the cwd.
/// The XDG Base Directory spec says a value "must be an absolute path" and
/// that an invalid or unset value takes the `$HOME` fallback. `HOME` and
/// `ZDOTDIR` get the same treatment, so an unusable `HOME` fails loudly
/// ("HOME is not set") instead of silently resolving to `.`.
///
/// Split from [`env_path`] so it is testable without mutating the process
/// environment: `std::env::set_var` is `unsafe` because it races any
/// concurrent reader, and libtest runs tests on a thread pool.
fn usable_base_dir(value: Option<std::ffi::OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}

fn env_path(key: &str) -> Option<PathBuf> {
    usable_base_dir(std::env::var_os(key))
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
/// `std::fs::write` **follows** an existing symlink and truncates whatever it
/// points at: with `~/.local/share/bash-completion/completions/spectra`
/// symlinked to `~/.bashrc`, a plain write replaces the user's rc file with a
/// completion script -- breaking this command's strongest guarantee (it never
/// touches rc files) while reporting success. `rename` replaces the directory
/// entry itself, so a symlink at `path` is swapped out rather than traversed.
///
/// The temp file is created with `create_new` (`O_EXCL`), which both refuses
/// to follow a symlink and refuses a pre-existing entry -- otherwise the temp
/// path, whose name is predictable, would re-open the very hole `rename`
/// closes one path over. A collision is retried under a fresh sequence number
/// rather than failing the install. Mirrors `spectra-core`'s
/// `init::write_atomically`, plus that `O_EXCL` hardening.
fn write_replacing_any_symlink(path: &Path, contents: &[u8]) -> Result<()> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let file_name = path
        .file_name()
        .context("completion path has no file name")?
        .to_string_lossy()
        .into_owned();

    let mut last_collision = None;
    for _ in 0..MAX_TEMP_FILE_ATTEMPTS {
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp_path = path.with_file_name(format!("{file_name}.tmp-{}-{seq}", std::process::id()));

        let mut file = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)
        {
            Ok(file) => file,
            // Someone else holds this name (or planted a symlink there);
            // never write through it -- take the next sequence number.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                last_collision = Some(e);
                continue;
            }
            Err(e) => {
                return Err(e).with_context(|| format!("creating {}", tmp_path.display()));
            }
        };

        if let Err(e) = file.write_all(contents) {
            drop(file);
            cleanup_temp_file(&tmp_path);
            return Err(e).with_context(|| format!("writing {}", tmp_path.display()));
        }
        drop(file);

        if let Err(e) = std::fs::rename(&tmp_path, path) {
            cleanup_temp_file(&tmp_path);
            return Err(e)
                .with_context(|| format!("installing completion script to {}", path.display()));
        }
        return Ok(());
    }

    let cause = last_collision.expect("a collision is the only way to exhaust the attempts");
    Err(cause).with_context(|| {
        format!(
            "creating a temp file next to {} ({MAX_TEMP_FILE_ATTEMPTS} names were already taken)",
            path.display()
        )
    })
}

/// Bounded so a directory pre-seeded with temp names fails loudly instead of
/// spinning forever.
const MAX_TEMP_FILE_ATTEMPTS: u32 = 16;

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
        // Single-quoted: the path is pasted into zsh as an array element, and
        // an unquoted `/Users/John Smith/.zfunc` word-splits into two bogus
        // entries -- leaving completion broken in exactly the way this hint
        // exists to prevent. Embedded single quotes are escaped the way zsh
        // requires ('\'' closes, escapes, reopens).
        println!(
            "Hint: ensure your .zshrc contains `fpath+=('{}')` before `compinit`.",
            parent.display().to_string().replace('\'', r"'\''")
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

    /// The filter lives in `usable_base_dir`, so the path helpers below never
    /// see an unusable value -- this pins the filter itself, which is where
    /// the write-to-cwd regression came from. Deliberately tested through the
    /// pure function rather than `env_path`: `std::env::set_var` is `unsafe`
    /// precisely because it races concurrent readers, and libtest runs this
    /// binary's tests on a thread pool.
    /// Unit-level on purpose: the temp name embeds `std::process::id()`, and
    /// only an in-process test shares that pid with the code under test -- a
    /// spawned binary's pid is unknowable from the parent, so an integration
    /// test cannot plant anything at the name the helper will pick. The
    /// counter starts at 0 in this binary and only this test advances it, so
    /// planting a generous range guarantees the collision is real rather
    /// than a vacuously-passing assertion.
    #[cfg(unix)]
    #[test]
    fn write_replacing_any_symlink_never_writes_through_a_symlinked_temp_path() {
        let tmp = std::env::temp_dir().join(format!("spectra-tmp-symlink-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();

        let victim = tmp.join("victim.txt");
        let victim_contents = "# precious user config\n";
        std::fs::write(&victim, victim_contents).unwrap();

        let target = tmp.join("_spectra");
        for seq in 0..(MAX_TEMP_FILE_ATTEMPTS * 4) {
            let planted = tmp.join(format!("_spectra.tmp-{}-{seq}", std::process::id()));
            std::os::unix::fs::symlink(&victim, &planted).unwrap();
        }

        // Every candidate name is taken, so the helper must exhaust its
        // attempts and fail -- never follow one of those links.
        let result = write_replacing_any_symlink(&target, b"completion script\n");

        assert!(
            result.is_err(),
            "all temp names were taken; the write must fail rather than reuse one"
        );
        assert_eq!(
            std::fs::read_to_string(&victim).unwrap(),
            victim_contents,
            "the symlink target must never be written through"
        );
        assert!(
            !target.exists(),
            "nothing may be installed when no temp file could be created"
        );

        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn usable_base_dir_rejects_absent_empty_and_relative_values() {
        use std::ffi::OsString;

        assert_eq!(usable_base_dir(None), None, "absent");
        assert_eq!(usable_base_dir(Some(OsString::from(""))), None, "empty");
        assert_eq!(
            usable_base_dir(Some(OsString::from("relative/dir"))),
            None,
            "a relative value would resolve against the cwd"
        );
        assert_eq!(
            usable_base_dir(Some(OsString::from("./also-relative"))),
            None,
            "an explicitly cwd-relative value is equally unusable"
        );
        assert_eq!(
            usable_base_dir(Some(OsString::from("/absolute/dir"))),
            Some(PathBuf::from("/absolute/dir")),
            "an absolute value is the only usable form"
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
