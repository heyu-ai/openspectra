//! `spectra init`: scaffold a fresh project so every other command (which all
//! require `.spectra.yaml` — see [`Config::is_initialized`]) has somewhere to
//! read and write.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config::{Config, DEFAULT_SPEC_DIR};

/// Line ensured present in `.gitignore`. `.spectra/` holds per-change sidecar
/// state (baseline SHAs, parked markers, touched-file tracking) that must
/// never be committed — the root cause of the PR #19 self-recording bug was a
/// project that had never run `init` and so had no such ignore entry.
const GITIGNORE_ENTRY: &str = ".spectra/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitOutcome {
    pub root: PathBuf,
    pub spec_dir: String,
    /// Whether `.gitignore` was created or appended to. `false` when the
    /// entry was already present (e.g. a hand-written `.gitignore`).
    pub gitignore_updated: bool,
}

/// Scaffold `.spectra.yaml`, `<spec_dir>/{changes,specs}/`, and a
/// `.gitignore` entry for `.spectra/` under `root`. Errors if `root` is
/// already initialized; run at most once per project.
pub fn init(root: &Path) -> Result<InitOutcome> {
    if Config::is_initialized(root) {
        anyhow::bail!(
            "already initialized ({} exists)",
            root.join(".spectra.yaml").display()
        );
    }

    let spec_dir = DEFAULT_SPEC_DIR;
    std::fs::create_dir_all(root.join(spec_dir).join("changes"))
        .with_context(|| format!("creating {spec_dir}/changes"))?;
    std::fs::create_dir_all(root.join(spec_dir).join("specs"))
        .with_context(|| format!("creating {spec_dir}/specs"))?;

    std::fs::write(
        root.join(".spectra.yaml"),
        format!("spec_dir: {spec_dir}\n"),
    )
    .context("writing .spectra.yaml")?;

    let gitignore_updated = ensure_gitignore_entry(root)?;

    Ok(InitOutcome {
        root: root.to_path_buf(),
        spec_dir: spec_dir.to_string(),
        gitignore_updated,
    })
}

/// Append [`GITIGNORE_ENTRY`] to `.gitignore` as its own line, unless a line
/// already matches it (ignoring surrounding whitespace). Creates the file if
/// it doesn't exist. Returns whether a write happened.
fn ensure_gitignore_entry(root: &Path) -> Result<bool> {
    let path = root.join(".gitignore");
    let existing = read_gitignore(&path)?;
    if existing.lines().any(|l| l.trim() == GITIGNORE_ENTRY) {
        return Ok(false);
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(GITIGNORE_ENTRY);
    updated.push('\n');
    std::fs::write(&path, updated).context("writing .gitignore")?;
    Ok(true)
}

fn read_gitignore(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "spectra-init-test-{}-{}-{seq}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl std::ops::Deref for TempDir {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn init_creates_config_and_scaffold_dirs() {
        let tmp = TempDir::new();
        let outcome = init(&tmp).unwrap();

        assert_eq!(outcome.spec_dir, "openspec");
        assert!(tmp.join(".spectra.yaml").is_file());
        assert!(tmp.join("openspec/changes").is_dir());
        assert!(tmp.join("openspec/specs").is_dir());

        let cfg = Config::load(&tmp).unwrap();
        assert_eq!(cfg.spec_dir, "openspec");
    }

    #[test]
    fn init_errors_when_already_initialized() {
        let tmp = TempDir::new();
        init(&tmp).unwrap();

        let err = init(&tmp).unwrap_err();
        assert!(err.to_string().contains("already initialized"));
    }

    #[test]
    fn init_creates_gitignore_with_spectra_entry_when_missing() {
        let tmp = TempDir::new();
        let outcome = init(&tmp).unwrap();

        assert!(outcome.gitignore_updated);
        let contents = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        assert!(contents.lines().any(|l| l == ".spectra/"));
    }

    #[test]
    fn init_appends_to_an_existing_gitignore_without_a_trailing_newline() {
        let tmp = TempDir::new();
        std::fs::write(tmp.join(".gitignore"), "target/").unwrap();

        let outcome = init(&tmp).unwrap();

        assert!(outcome.gitignore_updated);
        let contents = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        assert_eq!(contents, "target/\n.spectra/\n");
    }

    #[test]
    fn init_does_not_duplicate_an_existing_spectra_gitignore_entry() {
        let tmp = TempDir::new();
        std::fs::write(tmp.join(".gitignore"), "target/\n.spectra/\n").unwrap();

        let outcome = init(&tmp).unwrap();

        assert!(!outcome.gitignore_updated);
        let contents = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        assert_eq!(contents, "target/\n.spectra/\n");
    }
}
