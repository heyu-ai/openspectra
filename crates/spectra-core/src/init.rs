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

/// Scaffold `<spec_dir>/{changes,specs}/`, a `.gitignore` entry for
/// `.spectra/`, and `.spectra.yaml` under `root`. Errors if `root` is already
/// initialized; run at most once per project.
///
/// `.spectra.yaml` — the file [`Config::is_initialized`] checks — is written
/// *last*, after every other step has succeeded, so its mere existence is a
/// reliable signal that scaffolding is complete. Writing it any earlier would
/// let a failure in a later step (e.g. an unwritable `.gitignore`) leave the
/// project marked initialized but missing the `.spectra/` ignore entry, with
/// no way to retry — every subsequent `init` would immediately bail with
/// "already initialized" instead of finishing the interrupted work.
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

    let gitignore_updated = ensure_gitignore_entry(root)?;

    write_atomically(
        &root.join(".spectra.yaml"),
        &format!("spec_dir: {spec_dir}\n"),
    )
    .context("writing .spectra.yaml")?;

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

    // Match the existing file's line-ending style so appending doesn't leave
    // a CRLF file with one stray LF-terminated line.
    let newline = if existing.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push_str(newline);
    }
    updated.push_str(GITIGNORE_ENTRY);
    updated.push_str(newline);
    write_atomically(&path, &updated).context("writing .gitignore")?;
    Ok(true)
}

fn read_gitignore(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Write `contents` to `path` atomically: write to a temp file in the same
/// directory, then rename into place. A same-filesystem `rename` is atomic
/// on POSIX, so `path` is always either fully absent or fully valid — never
/// left as a partial write (from a disk-full error, `SIGKILL`, or power
/// loss mid-write) that satisfies `path.exists()` while failing to parse.
/// This matters most for `.spectra.yaml`, since [`Config::is_initialized`]
/// treats its mere existence as a reliable "scaffolding is complete" signal.
fn write_atomically(path: &Path, contents: &str) -> Result<()> {
    let file_name = path
        .file_name()
        .expect("write_atomically requires a path with a file name")
        .to_string_lossy();
    let tmp_path = path.with_file_name(format!("{file_name}.tmp-{}", std::process::id()));
    std::fs::write(&tmp_path, contents)
        .with_context(|| format!("writing {}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, path)
        .with_context(|| format!("renaming {} into place", tmp_path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomically_writes_full_content_and_leaves_no_temp_file_behind() {
        let tmp = TempDir::new();
        let target = tmp.join("out.txt");

        write_atomically(&target, "hello\n").unwrap();

        assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello\n");
        let entries: Vec<_> = std::fs::read_dir(&*tmp)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            entries,
            vec!["out.txt".to_string()],
            "no stray <file>.tmp-<pid> should remain after a successful write"
        );
    }

    #[test]
    fn write_atomically_leaves_the_target_untouched_when_the_write_fails() {
        let tmp = TempDir::new();
        // A nonexistent parent directory makes the temp-file write fail
        // before any rename is attempted, so the final path is never
        // created or truncated.
        let target = tmp.join("nonexistent-dir").join("out.txt");

        write_atomically(&target, "hello").unwrap_err();

        assert!(!target.exists());
    }

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
    fn init_preserves_crlf_line_endings_when_appending_to_gitignore() {
        let tmp = TempDir::new();
        std::fs::write(tmp.join(".gitignore"), "target/\r\n").unwrap();

        let outcome = init(&tmp).unwrap();

        assert!(outcome.gitignore_updated);
        let contents = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        assert_eq!(contents, "target/\r\n.spectra/\r\n");
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

    #[test]
    fn init_does_not_duplicate_an_entry_with_trailing_whitespace() {
        let tmp = TempDir::new();
        std::fs::write(tmp.join(".gitignore"), "target/\n.spectra/ \n").unwrap();

        let outcome = init(&tmp).unwrap();

        assert!(!outcome.gitignore_updated);
        let contents = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        assert_eq!(contents, "target/\n.spectra/ \n");
    }

    #[test]
    fn init_does_not_leave_spectra_yaml_behind_when_gitignore_handling_fails() {
        let tmp = TempDir::new();
        // A directory named `.gitignore` makes `ensure_gitignore_entry`'s read
        // fail with a real I/O error (not NotFound), simulating a .gitignore
        // write failure without needing chmod/root shenanigans.
        std::fs::create_dir_all(tmp.join(".gitignore")).unwrap();

        let err = init(&tmp).unwrap_err();

        assert!(!err.to_string().contains("already initialized"));
        assert!(
            !tmp.join(".spectra.yaml").exists(),
            "must not mark the project initialized when gitignore handling fails"
        );
    }

    #[test]
    fn init_can_be_retried_after_a_transient_gitignore_failure_is_fixed() {
        let tmp = TempDir::new();
        std::fs::create_dir_all(tmp.join(".gitignore")).unwrap();
        init(&tmp).unwrap_err();

        // Fix the obstruction and retry: since .spectra.yaml is written last,
        // the failed attempt above must not have marked the project
        // initialized, so this retry is free to complete normally.
        std::fs::remove_dir(tmp.join(".gitignore")).unwrap();
        let outcome = init(&tmp).unwrap();
        assert!(outcome.gitignore_updated);
    }
}
