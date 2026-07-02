//! Project bootstrap: `spectra init`.
//!
//! Creates the minimum on-disk state every other command assumes exists:
//! a `.spectra.yaml` config (which is also what [`Config::is_initialized`]
//! keys off), the `<spec_dir>/changes/` and `<spec_dir>/specs/` skeleton, and
//! a `.spectra/` entry in `.gitignore` (OpenSpectra's own state directory —
//! PR #19's self-recording bug traced back to a project that had never
//! git-ignored it).
//!
//! Oracle status: the reference `spectra` CLI's `init` was not observable on
//! Linux (macOS-only app bundle) at the time this was written, so the exact
//! file set and message wording here are *not* oracle-verified — see
//! `docs/reverse-engineering/init.md`. The design is driven by the error
//! messages every other subcommand emits ("Run 'spectra init' first.") and by
//! `Config`'s own layout expectations.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config::{Config, DEFAULT_SPEC_DIR};

/// The `.gitignore` pattern for OpenSpectra's state directory. Written with a
/// trailing slash so it only ever matches the directory, never a file a user
/// might legitimately name `.spectra`.
pub const GITIGNORE_PATTERN: &str = ".spectra/";

/// What [`init`] created, so the CLI can report it precisely (and tests can
/// assert on it) rather than re-deriving paths from the `Config`.
#[derive(Debug)]
pub struct InitOutcome {
    /// The `.spectra.yaml` config file that was written.
    pub config_path: PathBuf,
    /// Resolved spec directory name (always [`DEFAULT_SPEC_DIR`] for a plain
    /// `init`, but carried explicitly so a future `--spec-dir` doesn't have to
    /// change this contract).
    pub spec_dir: String,
    /// `<spec_dir>/changes` (created).
    pub changes_dir: PathBuf,
    /// `<spec_dir>/specs` (created).
    pub specs_dir: PathBuf,
    /// Whether `.gitignore` was created or appended to. `false` means the
    /// `.spectra/` pattern was already present (a re-init on a tree someone
    /// set up by hand), not that the step was skipped.
    pub gitignore_updated: bool,
}

/// Initialize a project rooted at `root`.
///
/// Errors if the project is already initialized (`.spectra.yaml` exists) —
/// `init` is a bootstrap, not a reconcile; adopting an existing `openspec/`
/// tree is Phase 2's `--adopt`, deliberately out of scope here. Every
/// individual filesystem step is otherwise idempotent (directories via
/// `create_dir_all`, the `.gitignore` pattern de-duplicated), so a project
/// left half-initialized by an interrupted run can still be completed by
/// removing `.spectra.yaml` and re-running.
pub fn init(root: &Path) -> Result<InitOutcome> {
    if Config::is_initialized(root) {
        anyhow::bail!(
            "already initialized: {} exists",
            root.join(".spectra.yaml").display()
        );
    }

    let spec_dir = DEFAULT_SPEC_DIR.to_string();

    // Build a Config by hand rather than Config::load: load() keys off the
    // very file we're about to write, and we already know the spec_dir.
    let cfg = Config {
        root: root.to_path_buf(),
        spec_dir: spec_dir.clone(),
        locale: None,
    };

    let changes_dir = cfg.changes_dir();
    let specs_dir = cfg.specs_dir();
    std::fs::create_dir_all(&changes_dir)
        .with_context(|| format!("creating {}", changes_dir.display()))?;
    std::fs::create_dir_all(&specs_dir)
        .with_context(|| format!("creating {}", specs_dir.display()))?;

    let gitignore_updated = ensure_gitignore(root)?;

    // Write the config *last*: it's the initialized sentinel, so committing it
    // only after the skeleton + gitignore succeed means an early failure
    // leaves the project un-initialized (and thus safely re-runnable) rather
    // than initialized-but-incomplete.
    let config_path = root.join(".spectra.yaml");
    let config_body = format!("spec_dir: {spec_dir}\n");
    std::fs::write(&config_path, config_body)
        .with_context(|| format!("writing {}", config_path.display()))?;

    Ok(InitOutcome {
        config_path,
        spec_dir,
        changes_dir,
        specs_dir,
        gitignore_updated,
    })
}

/// Ensure `.gitignore` ignores OpenSpectra's `.spectra/` state directory.
/// Creates the file if absent, appends the pattern if missing, and does
/// nothing if it's already ignored — returning whether a write happened.
///
/// "Already present" matches either `.spectra/` or a bare `.spectra` (a user
/// may have added it without the trailing slash); a match on any non-comment
/// line, trimmed, counts, so we don't append a redundant second entry.
fn ensure_gitignore(root: &Path) -> Result<bool> {
    let path = root.join(".gitignore");
    let existing = match std::fs::read_to_string(&path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };

    if let Some(contents) = &existing {
        if gitignore_already_covers(contents) {
            return Ok(false);
        }
    }

    // Append (or create). When appending to a file whose last line lacks a
    // trailing newline, insert one first so we don't glue our pattern onto an
    // existing entry (e.g. `target\n.spectra/` would be fine, but `target`
    // with no newline must not become `target.spectra/`).
    let mut out = existing.unwrap_or_default();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(GITIGNORE_PATTERN);
    out.push('\n');
    std::fs::write(&path, out).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

/// Whether `contents` already ignores `.spectra/` (matching a bare `.spectra`
/// too, and ignoring comment lines). Pulled out so the exact match rule is
/// unit-testable without touching the filesystem.
fn gitignore_already_covers(contents: &str) -> bool {
    contents.lines().any(|line| {
        let trimmed = line.trim();
        trimmed == GITIGNORE_PATTERN || trimmed == ".spectra"
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RAII scratch directory (mirrors the helper used across the other
    /// modules' test suites: unique per test, removed on drop even on panic).
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
    fn init_creates_config_skeleton_and_gitignore() {
        let tmp = TempDir::new();

        let outcome = init(&tmp).unwrap();

        assert!(tmp.join(".spectra.yaml").is_file());
        assert!(tmp.join("openspec").join("changes").is_dir());
        assert!(tmp.join("openspec").join("specs").is_dir());
        assert_eq!(outcome.spec_dir, "openspec");
        assert!(outcome.gitignore_updated);

        // The written config must actually load back as an initialized project
        // with the same spec_dir (guards against writing a config `Config::load`
        // can't parse).
        assert!(Config::is_initialized(&tmp));
        let cfg = Config::load(&tmp).unwrap();
        assert_eq!(cfg.spec_dir, "openspec");

        let gitignore = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        assert!(gitignore.lines().any(|l| l.trim() == ".spectra/"));
    }

    #[test]
    fn init_errors_when_already_initialized() {
        let tmp = TempDir::new();
        init(&tmp).unwrap();

        let err = init(&tmp).unwrap_err();
        assert!(
            err.to_string().contains("already initialized"),
            "got: {err}"
        );
    }

    #[test]
    fn init_appends_to_an_existing_gitignore_without_clobbering_it() {
        let tmp = TempDir::new();
        std::fs::write(tmp.join(".gitignore"), "/target\nnode_modules/\n").unwrap();

        let outcome = init(&tmp).unwrap();
        assert!(outcome.gitignore_updated);

        let gitignore = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        // Pre-existing entries preserved...
        assert!(gitignore.contains("/target"));
        assert!(gitignore.contains("node_modules/"));
        // ...and our pattern appended.
        assert!(gitignore.lines().any(|l| l.trim() == ".spectra/"));
    }

    #[test]
    fn init_adds_a_newline_before_appending_to_a_gitignore_missing_its_trailing_one() {
        let tmp = TempDir::new();
        // No trailing newline: a naive append would produce `target.spectra/`.
        std::fs::write(tmp.join(".gitignore"), "/target").unwrap();

        init(&tmp).unwrap();

        let gitignore = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        assert!(gitignore.lines().any(|l| l.trim() == "/target"));
        assert!(gitignore.lines().any(|l| l.trim() == ".spectra/"));
        assert!(!gitignore.contains("target.spectra"));
    }

    #[test]
    fn ensure_gitignore_is_idempotent_when_pattern_already_present() {
        let tmp = TempDir::new();
        std::fs::write(tmp.join(".gitignore"), "/target\n.spectra/\n").unwrap();

        // Called directly (init would refuse a second run) to assert the
        // no-duplicate behavior in isolation.
        let updated = ensure_gitignore(&tmp).unwrap();
        assert!(!updated, "must not append when already ignored");

        let gitignore = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        assert_eq!(gitignore.matches(".spectra/").count(), 1);
    }

    #[test]
    fn gitignore_already_covers_matches_bare_and_slashed_and_ignores_comments() {
        assert!(gitignore_already_covers(".spectra/\n"));
        assert!(gitignore_already_covers("foo\n.spectra\nbar\n"));
        assert!(gitignore_already_covers("  .spectra/  \n"));
        assert!(!gitignore_already_covers("# .spectra/\n"));
        assert!(!gitignore_already_covers("target\nnode_modules/\n"));
        // A substring match must not count (`.spectra-backup/` is unrelated).
        assert!(!gitignore_already_covers(".spectra-backup/\n"));
    }

    #[test]
    fn init_creates_gitignore_when_none_exists() {
        let tmp = TempDir::new();
        assert!(!tmp.join(".gitignore").exists());

        let outcome = init(&tmp).unwrap();
        assert!(outcome.gitignore_updated);
        assert_eq!(
            std::fs::read_to_string(tmp.join(".gitignore")).unwrap(),
            ".spectra/\n"
        );
    }
}
