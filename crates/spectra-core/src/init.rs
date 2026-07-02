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
    pub adopted: bool,
    /// Whether `.gitignore` was created or appended to. `false` when the
    /// entry was already present (e.g. a hand-written `.gitignore`).
    pub gitignore_updated: bool,
}

/// Scaffold `<spec_dir>/{changes,specs}/`, a `.gitignore` entry for
/// `.spectra/`, and `.spectra.yaml` under `root`. Errors if `root` is already
/// initialized. Intended to run once per project: `Config::is_initialized`'s
/// check-then-act isn't lock-protected, so two concurrent invocations on the
/// same never-initialized `root` could both pass it and both scaffold --
/// harmless (the content each writes is deterministic) but redundant, not
/// actually serialized against each other.
///
/// `.spectra.yaml` — the file [`Config::is_initialized`] checks — is written
/// *last*, after every other step has succeeded, so its mere existence is a
/// reliable signal that scaffolding is complete. Writing it any earlier would
/// let a failure in a later step (e.g. an unwritable `.gitignore`) leave the
/// project marked initialized but missing the `.spectra/` ignore entry, with
/// no way to retry — every subsequent `init` would immediately bail with
/// "already initialized" instead of finishing the interrupted work.
pub fn init(root: &Path) -> Result<InitOutcome> {
    init_with_options(root, false)
}

/// Variant of [`init`] that can adopt an existing OpenSpec-style project.
///
/// Plain `init` keeps its original "fresh project" contract. `adopt` is the
/// explicit compatibility path for a root that already has OpenSpec content:
/// it only creates the two required directories if missing, ensures
/// `.spectra/` is ignored, and writes `.spectra.yaml` last. It deliberately
/// does not touch `project.md`, `AGENTS.md`, `config.yaml`, or any existing
/// change/spec content under the spec directory.
pub fn init_with_options(root: &Path, adopt: bool) -> Result<InitOutcome> {
    if Config::is_initialized(root) {
        anyhow::bail!(
            "already initialized ({} exists)",
            root.join(".spectra.yaml").display()
        );
    }

    let spec_dir = if adopt {
        detect_adopt_spec_dir(root)?
    } else {
        DEFAULT_SPEC_DIR.to_string()
    };
    std::fs::create_dir_all(root.join(&spec_dir).join("changes"))
        .with_context(|| format!("creating {spec_dir}/changes"))?;
    std::fs::create_dir_all(root.join(&spec_dir).join("specs"))
        .with_context(|| format!("creating {spec_dir}/specs"))?;

    let gitignore_updated = ensure_gitignore_entry(root)?;

    write_atomically(
        &root.join(".spectra.yaml"),
        &format!("spec_dir: {spec_dir}\n"),
    )
    .context("writing .spectra.yaml")?;

    Ok(InitOutcome {
        root: root.to_path_buf(),
        spec_dir,
        adopted: adopt,
        gitignore_updated,
    })
}

/// Resolve the spec directory for an `--adopt`. Only the default `openspec`
/// name is used today (configurable spec-dir discovery is future work), so this
/// always resolves to [`DEFAULT_SPEC_DIR`] -- it does **not** inspect the
/// directory's contents to pick a name. The probe still earns its keep: it
/// surfaces a real I/O error (e.g. permission denied) before scaffolding
/// proceeds, and it fails with a clear message when `openspec` already exists as
/// a non-directory (a file or a symlink to one), rather than letting the later
/// `create_dir_all` fail with a generic, harder-to-read error.
fn detect_adopt_spec_dir(root: &Path) -> Result<String> {
    let candidate = root.join(DEFAULT_SPEC_DIR);
    match std::fs::metadata(&candidate) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => anyhow::bail!(
            "cannot adopt: {} exists but is not a directory",
            candidate.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("reading {}", candidate.display())),
    }
    Ok(DEFAULT_SPEC_DIR.to_string())
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
/// with respect to concurrent readers and to the process being killed
/// (`SIGKILL`) or hitting a disk-full error after the syscall returns, so
/// `path` is never observed as a partial write that satisfies `path.exists()`
/// while failing to parse. This matters most for `.spectra.yaml`, since
/// [`Config::is_initialized`] treats its mere existence as a reliable
/// "scaffolding is complete" signal.
///
/// This does *not* guarantee durability across true power loss (that needs
/// `fsync`-ing the temp file and the parent directory, which this skips as
/// disproportionate for a few bytes of config) -- but a `.spectra.yaml`
/// corrupted by that rarer case is still a clean two-step recovery: delete
/// it and re-run `spectra init` (see [`Config::load`]'s parse-error hint).
///
/// If `path` is a symlink, the rename replaces the link itself with a
/// regular file rather than writing through to its target -- not handled
/// specially, since this CLI has no evidence of needing to support a
/// symlinked `.gitignore`/`.spectra.yaml` (e.g. a dotfile manager) and doing
/// so isn't a data-loss risk (the correct content still lands, just not at
/// the symlink's target).
///
/// On failure, the temp file is removed on a best-effort basis; if that
/// cleanup itself also fails, the original error is still what's returned,
/// with the cleanup failure logged to stderr rather than silently dropped.
fn write_atomically(path: &Path, contents: &str) -> Result<()> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("path {} has no file name", path.display()))?
        .to_string_lossy();
    let tmp_path = path.with_file_name(format!("{file_name}.tmp-{}-{seq}", std::process::id()));

    if let Err(e) = std::fs::write(&tmp_path, contents) {
        cleanup_temp_file(&tmp_path);
        return Err(e).with_context(|| format!("writing {}", tmp_path.display()));
    }
    if let Err(e) = std::fs::rename(&tmp_path, path) {
        cleanup_temp_file(&tmp_path);
        return Err(e).with_context(|| format!("renaming {} into place", tmp_path.display()));
    }
    Ok(())
}

/// Best-effort removal of a `write_atomically` temp file after its write or
/// rename step failed. The primary error is always what the caller returns;
/// this only logs (rather than silently dropping) a secondary failure here,
/// so a double-fault doesn't vanish without a trace. `NotFound` isn't logged:
/// it means the initial `std::fs::write` failed before ever creating the
/// temp file (the common case -- an unwritable/missing target directory),
/// so there's nothing to clean up and no secondary fault to report.
fn cleanup_temp_file(tmp_path: &Path) {
    if let Err(e) = std::fs::remove_file(tmp_path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            eprintln!(
                "warning: failed to clean up temp file {}: {e}",
                tmp_path.display()
            );
        }
    }
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
        // A nonexistent parent directory makes the temp-file write itself
        // fail before any rename is attempted -- this covers "write fails
        // before touching the target", not a torn/interrupted write (which
        // needs fault injection this test doesn't attempt).
        let target = tmp.join("nonexistent-dir").join("out.txt");

        write_atomically(&target, "hello").unwrap_err();

        assert!(!target.exists());
    }

    #[test]
    fn write_atomically_cleans_up_the_temp_file_when_rename_fails() {
        let tmp = TempDir::new();
        let target = tmp.join("out.txt");
        // A pre-existing directory at the target path lets the temp-file
        // write succeed (it's a different filename) but makes the rename
        // fail (renaming a file onto an existing directory is rejected),
        // exercising the cleanup-on-rename-failure branch specifically.
        std::fs::create_dir_all(&target).unwrap();

        write_atomically(&target, "hello").unwrap_err();

        let entries: Vec<_> = std::fs::read_dir(&*tmp)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            entries,
            vec!["out.txt".to_string()],
            "no stray out.txt.tmp-<pid>-<seq> should remain after a failed rename, got: {entries:?}"
        );
    }

    /// After chmod(0o555), root (or a container with CAP_DAC_OVERRIDE) can
    /// still create files inside `path`, so the permission-denied scenario
    /// below is unconstructible; skip rather than fail in that case. Unlike
    /// `touched.rs`'s/`archive.rs`'s identically-named helper (which probes
    /// a *file*'s own `0o000` read permission), this probes whether *this
    /// directory* still accepts new entries under `0o555` -- a different
    /// check for a different scenario, not a duplicate.
    #[cfg(unix)]
    fn permission_denied_is_constructible(path: &Path) -> bool {
        let probe = path.join(".spectra-permission-probe");
        let blocked = std::fs::write(&probe, "x").is_err();
        let _ = std::fs::remove_file(&probe);
        blocked
    }

    #[cfg(unix)]
    #[test]
    fn init_gitignore_update_routes_through_write_atomically_not_a_plain_write() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new();
        // Pre-create everything init() would otherwise scaffold, and an
        // existing `.gitignore` that still needs the `.spectra/` entry
        // appended. The discriminator: a plain `std::fs::write` on an
        // *existing* `.gitignore` only needs the file's own (default
        // 0o644) write permission and would succeed even with `root`
        // read-only, silently updating its content despite `init()`
        // failing later at `.spectra.yaml` (which always needs `root`'s
        // write permission to create, atomic or not, so the overall Err
        // alone can't tell the two implementations apart). But
        // `write_atomically`'s temp-file-then-rename needs write
        // permission on `root` itself just to create the temp file, so it
        // fails *before* ever touching `.gitignore`'s real content --
        // leaving it byte-for-byte unchanged. Verified by reverting both
        // call sites to plain `std::fs::write` and confirming the content
        // assertion below fails (while a naive "did init() return Err"
        // assertion would not have).
        let original_gitignore = "target/\n";
        std::fs::create_dir_all(tmp.join("openspec/changes")).unwrap();
        std::fs::create_dir_all(tmp.join("openspec/specs")).unwrap();
        std::fs::write(tmp.join(".gitignore"), original_gitignore).unwrap();

        std::fs::set_permissions(&*tmp, std::fs::Permissions::from_mode(0o555)).unwrap();

        if !permission_denied_is_constructible(&tmp) {
            eprintln!(
                "skipping init_gitignore_update_routes_through_write_atomically_not_a_plain_write: \
                 running as root (chmod 0o555 not enforced)"
            );
            std::fs::set_permissions(&*tmp, std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }

        let result = init(&tmp);
        std::fs::set_permissions(&*tmp, std::fs::Permissions::from_mode(0o755)).unwrap();

        result.unwrap_err();
        assert!(
            !tmp.join(".spectra.yaml").exists(),
            "must not mark the project initialized when the gitignore update fails"
        );
        let gitignore_after = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        assert_eq!(
            gitignore_after, original_gitignore,
            ".gitignore must stay byte-for-byte unchanged if write_atomically's temp-file \
             creation failed before any rename -- if this fails, the write no longer routes \
             through write_atomically"
        );
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
        assert!(!outcome.adopted);
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
    fn init_adopt_preserves_existing_openspec_content() {
        let tmp = TempDir::new();
        let project = tmp.join("openspec/project.md");
        let spec = tmp.join("openspec/specs/search/spec.md");
        std::fs::create_dir_all(spec.parent().unwrap()).unwrap();
        std::fs::write(&project, "# Existing project\n\nDo not touch.\n").unwrap();
        std::fs::write(
            &spec,
            "# Search Specification\n\n## Requirements\n\n### Requirement: Search\n\nExisting.\n",
        )
        .unwrap();

        let outcome = init_with_options(&tmp, true).unwrap();

        assert!(outcome.adopted);
        assert_eq!(outcome.spec_dir, "openspec");
        assert_eq!(
            std::fs::read_to_string(&project).unwrap(),
            "# Existing project\n\nDo not touch.\n"
        );
        assert_eq!(
            std::fs::read_to_string(&spec).unwrap(),
            "# Search Specification\n\n## Requirements\n\n### Requirement: Search\n\nExisting.\n"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.join(".spectra.yaml")).unwrap(),
            "spec_dir: openspec\n"
        );
    }

    #[test]
    fn init_adopt_errors_when_already_initialized() {
        let tmp = TempDir::new();
        init(&tmp).unwrap();

        let err = init_with_options(&tmp, true).unwrap_err();

        assert!(err.to_string().contains("already initialized"));
    }

    #[test]
    fn init_adopt_on_empty_dir_creates_the_default_skeleton() {
        let tmp = TempDir::new();

        let outcome = init_with_options(&tmp, true).unwrap();

        assert!(outcome.adopted);
        assert_eq!(outcome.spec_dir, "openspec");
        assert!(tmp.join("openspec/changes").is_dir());
        assert!(tmp.join("openspec/specs").is_dir());
        assert!(tmp.join(".spectra.yaml").is_file());
    }

    #[test]
    fn init_adopt_errors_when_openspec_exists_as_a_file() {
        // A clear "not a directory" error beats letting the later
        // create_dir_all surface a generic I/O error.
        let tmp = TempDir::new();
        std::fs::write(tmp.join("openspec"), "not a directory\n").unwrap();

        let err = init_with_options(&tmp, true).unwrap_err();

        assert!(err.to_string().contains("is not a directory"), "got: {err}");
        // Nothing was marked initialized.
        assert!(!tmp.join(".spectra.yaml").exists());
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
