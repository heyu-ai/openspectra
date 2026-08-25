//! `spectra init`: scaffold a fresh project so every other command (which all
//! require `.spectra.yaml` — see [`Config::is_initialized`]) has somewhere to
//! read and write.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config::{Config, DEFAULT_SPEC_DIR};
use crate::fsutil::write_atomically;

/// Line ensured present in `.gitignore`. `.spectra/` holds per-change sidecar
/// state (baseline SHAs, parked markers, in-progress markers, touched-file
/// tracking) that must
/// never be committed — the root cause of the PR #19 self-recording bug was a
/// project that had never run `init` and so had no such ignore entry.
const GITIGNORE_ENTRY: &str = ".spectra/";
const GITIGNORE_COMMENT: &str = "# Spectra app data";

const SPEC_CONFIG_TEMPLATE: &str = concat!(
    "schema: spec-driven\n",
    "\n",
    "# Project context (optional)\n",
    "# This is shown to AI when creating artifacts.\n",
    "# Add your tech stack, conventions, style guides, domain knowledge, etc.\n",
    "# Example:\n",
    "#   context: |\n",
    "#     Tech stack: TypeScript, React, Node.js\n",
    "#     We use conventional commits\n",
    "#     Domain: e-commerce platform\n",
    "\n",
    "# Per-artifact rules (optional)\n",
    "# Add custom rules for specific artifacts.\n",
    "# Example:\n",
    "#   rules:\n",
    "#     proposal:\n",
    "#       - Keep proposals under 500 words\n",
    "#       - Always include a \"Non-goals\" section\n",
    "#     tasks:\n",
    "#       - Break tasks into chunks of max 2 hours\n",
);

const SPECTRA_CONFIG_TEMPLATE: &str = concat!(
    "# Spectra application config\n",
    "# See: https://github.com/spectra-app/spectra\n",
    "\n",
    "# OpenSpec directory path (relative to project root)\n",
    "# Changing this requires rebuilding the vector search index.\n",
    "# spec_dir: docs/specs\n",
    "\n",
    "# Language for AI-generated artifacts\n",
    "# locale: tw\n",
    "\n",
    "# Workflow toggles\n",
    "# tdd: true\n",
    "# audit: true\n",
    "# parallel_tasks: true\n",
    "\n",
    "# Claude slash commands (set true to also generate /spectra:X commands)\n",
    "# claude_slash_commands: true\n",
    "\n",
    "# Enable git worktree support for isolated change branches\n",
    "# worktree: true\n",
    "\n",
    "# Custom git worktrees directory\n",
    "# worktrees_dir: .spectra/worktrees\n",
    "\n",
    "# Claude Code skill effort levels (low/medium/high/xhigh/max)\n",
    "# claude_effort:\n",
    "#   apply: high\n",
    "\n",
    "# AI tools to generate instruction files for\n",
    "# tools:\n",
    "#   - claude\n",
    "#   - cursor\n",
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitOutcome {
    pub root: PathBuf,
    pub spec_dir: String,
    pub adopted: bool,
    /// Whether `.gitignore` was created or appended to. `false` when the
    /// entry was already present (e.g. a hand-written `.gitignore`).
    pub gitignore_updated: bool,
}

/// Scaffold `<spec_dir>/{changes/archive,specs}/`, `<spec_dir>/config.yaml`, a
/// `.gitignore` entry for `.spectra/`, and `.spectra.yaml` under `root`. Errors
/// if `root` is already initialized. Intended to run once per project:
/// `Config::is_initialized`'s check-then-act isn't lock-protected, so two
/// concurrent invocations on the same never-initialized `root` could both pass
/// it and both scaffold -- harmless (the content each writes is deterministic)
/// but redundant, not actually serialized against each other.
///
/// `.spectra.yaml` — the file [`Config::is_initialized`] checks — is written
/// *last*, after every other step has succeeded, so its mere existence is a
/// reliable signal that scaffolding is complete. Writing it any earlier would
/// let a failure in a later step (e.g. an unwritable `.gitignore`) leave the
/// project marked initialized but missing the `.spectra/` ignore entry, with
/// no way to retry — every subsequent `init` would immediately bail with
/// "already initialized" instead of finishing the interrupted work.
pub fn init(root: &Path) -> Result<InitOutcome> {
    init_with_options(root, false, false, None)
}

/// Scaffold a project and generate instruction files for explicitly requested
/// AI tools. Tool selection and all file-write semantics are shared with
/// `spectra update`; unknown ids are accepted and generate nothing.
pub fn init_with_tools(
    root: &Path,
    adopt: bool,
    tools: &[String],
    force: bool,
    spec_dir: Option<&str>,
) -> Result<InitOutcome> {
    let outcome = init_with_options(root, adopt, force, spec_dir)?;
    if !tools.is_empty() {
        let cfg = Config::load(root)?;
        crate::update::generate_instruction_files(&cfg, tools)?;
    }
    Ok(outcome)
}

/// Variant of [`init`] that can adopt an existing OpenSpec-style project.
///
/// Plain `init` keeps its original "fresh project" contract. `adopt` is the
/// explicit compatibility path for a root that already has OpenSpec content:
/// it creates the required directories and `config.yaml` if missing, ensures
/// `.spectra/` is ignored, and writes `.spectra.yaml` last. It deliberately
/// does not overwrite `project.md`, `AGENTS.md`, `config.yaml`, or any existing
/// change/spec content under the spec directory.
/// `force` 只略過已初始化檢查，其餘檔案仍沿用相同的非破壞性處理。
pub fn init_with_options(
    root: &Path,
    adopt: bool,
    force: bool,
    spec_dir: Option<&str>,
) -> Result<InitOutcome> {
    if !force && Config::is_initialized(root) {
        anyhow::bail!("Already initialized. Use --force to reinitialize.");
    }

    let spec_dir = if let Some(spec_dir) = spec_dir {
        spec_dir.to_string()
    } else if adopt {
        detect_adopt_spec_dir(root)?
    } else {
        DEFAULT_SPEC_DIR.to_string()
    };
    init_resolved_spec_dir(root, spec_dir, adopt)
}

fn init_resolved_spec_dir(root: &Path, spec_dir: String, adopted: bool) -> Result<InitOutcome> {
    // One call: `create_dir_all` creates `changes/` as a parent. A failure at
    // either level is reported with the archive path -- slightly less precise
    // than two calls, but both levels fail for the same reasons (unwritable
    // spec_dir, or `changes/` occupied by a file).
    std::fs::create_dir_all(root.join(&spec_dir).join("changes/archive"))
        .with_context(|| format!("creating {spec_dir}/changes/archive"))?;
    std::fs::create_dir_all(root.join(&spec_dir).join("specs"))
        .with_context(|| format!("creating {spec_dir}/specs"))?;

    let spec_config_path = root.join(&spec_dir).join("config.yaml");
    if !spec_config_path
        .try_exists()
        .with_context(|| format!("checking {}", spec_config_path.display()))?
    {
        write_atomically(&spec_config_path, SPEC_CONFIG_TEMPLATE)
            .map_err(anyhow::Error::from)
            .with_context(|| format!("writing {}", spec_config_path.display()))?;
    }

    let gitignore_updated = ensure_gitignore_entry(root)?;

    write_atomically(&root.join(".spectra.yaml"), &spectra_config(&spec_dir))
        .map_err(anyhow::Error::from)
        .context("writing .spectra.yaml")?;

    Ok(InitOutcome {
        root: root.to_path_buf(),
        spec_dir,
        adopted,
        gitignore_updated,
    })
}

fn spectra_config(spec_dir: &str) -> String {
    if spec_dir == DEFAULT_SPEC_DIR {
        SPECTRA_CONFIG_TEMPLATE.to_string()
    } else {
        SPECTRA_CONFIG_TEMPLATE.replacen(
            "# spec_dir: docs/specs",
            &format!("spec_dir: {spec_dir}"),
            1,
        )
    }
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

/// Append the Spectra comment and [`GITIGNORE_ENTRY`] block to `.gitignore`,
/// unless a line already matches the entry (ignoring surrounding whitespace).
/// Creates the file if it doesn't exist. Returns whether a write happened.
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
    if !updated.is_empty() {
        if !updated.ends_with('\n') {
            updated.push_str(newline);
        }
        updated.push_str(newline);
    }
    updated.push_str(GITIGNORE_COMMENT);
    updated.push_str(newline);
    updated.push_str(GITIGNORE_ENTRY);
    updated.push_str(newline);
    write_atomically(&path, &updated)
        .map_err(anyhow::Error::from)
        .context("writing .gitignore")?;
    Ok(true)
}

fn read_gitignore(path: &Path) -> Result<String> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

// `write_atomically` now lives in `crate::fsutil` so `update` and
// `global_config` can share it (PR #86: `update` performs read-modify-write
// over user-owned files and must not follow symlinks; a second copy here would
// be the kind of drift `fsutil` exists to prevent — and would silently ship
// `global_config` the pre-hardening version without O_EXCL / fchmod / the
// unwritable-directory fallback).

#[cfg(test)]
mod tests {
    use super::*;

    // `write_atomically`'s own unit tests live beside the function in
    // `fsutil.rs` (this repo's convention: unit tests in the same file as the
    // code). Only `init`'s *call site* is tested here -- see
    // `init_gitignore_update_routes_through_write_atomically_not_a_plain_write`
    // below.

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
        std::fs::create_dir_all(tmp.join("openspec/changes/archive")).unwrap();
        std::fs::create_dir_all(tmp.join("openspec/specs")).unwrap();
        std::fs::write(tmp.join("openspec/config.yaml"), SPEC_CONFIG_TEMPLATE).unwrap();
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

    fn entry_names(path: &Path) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn init_creates_config_and_scaffold_dirs() {
        let tmp = TempDir::new();
        let outcome = init(&tmp).unwrap();

        assert_eq!(outcome.spec_dir, "openspec");
        assert!(!outcome.adopted);
        assert!(tmp.join(".spectra.yaml").is_file());
        assert!(tmp.join("openspec/changes").is_dir());
        assert!(tmp.join("openspec/changes/archive").is_dir());
        assert!(tmp.join("openspec/specs").is_dir());
        assert!(tmp.join("openspec/config.yaml").is_file());
        assert_eq!(
            entry_names(&tmp),
            [".gitignore", ".spectra.yaml", "openspec"]
        );
        assert_eq!(
            entry_names(&tmp.join("openspec")),
            ["changes", "config.yaml", "specs"]
        );
        assert_eq!(entry_names(&tmp.join("openspec/changes")), ["archive"]);
        assert!(entry_names(&tmp.join("openspec/changes/archive")).is_empty());
        assert!(entry_names(&tmp.join("openspec/specs")).is_empty());

        let cfg = Config::load(&tmp).unwrap();
        assert_eq!(cfg.spec_dir, "openspec");
    }

    #[test]
    fn init_creates_an_empty_changes_archive_directory() {
        let tmp = TempDir::new();

        init(&tmp).unwrap();

        let archive = tmp.join("openspec/changes/archive");
        assert!(archive.is_dir());
        assert_eq!(std::fs::read_dir(archive).unwrap().count(), 0);
    }

    #[test]
    fn init_with_tools_generates_requested_files_without_detection_setup() {
        let tmp = TempDir::new();

        init_with_tools(&tmp, false, &["claude".to_string()], false, None).unwrap();

        assert!(tmp.join("CLAUDE.md").is_file());
        assert!(tmp.join(".claude/settings.json").is_file());
        assert!(tmp.join(".claude/skills/spectra-drift/SKILL.md").is_file());
    }

    #[test]
    fn init_creates_the_oracle_spec_config_template() {
        let tmp = TempDir::new();

        init(&tmp).unwrap();

        assert_eq!(
            std::fs::read_to_string(tmp.join("openspec/config.yaml")).unwrap(),
            concat!(
                "schema: spec-driven\n",
                "\n",
                "# Project context (optional)\n",
                "# This is shown to AI when creating artifacts.\n",
                "# Add your tech stack, conventions, style guides, domain knowledge, etc.\n",
                "# Example:\n",
                "#   context: |\n",
                "#     Tech stack: TypeScript, React, Node.js\n",
                "#     We use conventional commits\n",
                "#     Domain: e-commerce platform\n",
                "\n",
                "# Per-artifact rules (optional)\n",
                "# Add custom rules for specific artifacts.\n",
                "# Example:\n",
                "#   rules:\n",
                "#     proposal:\n",
                "#       - Keep proposals under 500 words\n",
                "#       - Always include a \"Non-goals\" section\n",
                "#     tasks:\n",
                "#       - Break tasks into chunks of max 2 hours\n",
            )
        );
    }

    #[test]
    fn spectra_config_replaces_only_line_six_for_a_non_default_spec_dir() {
        let default = spectra_config(DEFAULT_SPEC_DIR);
        let custom = spectra_config("docs/myspecs");

        let default_lines: Vec<_> = default.lines().collect();
        let custom_lines: Vec<_> = custom.lines().collect();
        assert_eq!(default_lines[5], "# spec_dir: docs/specs");
        assert_eq!(custom_lines[5], "spec_dir: docs/myspecs");
        assert_eq!(default_lines.len(), custom_lines.len());
        assert_eq!(&default_lines[..5], &custom_lines[..5]);
        assert_eq!(&default_lines[6..], &custom_lines[6..]);
    }

    #[test]
    fn resolved_non_default_spec_dir_places_artifacts_and_replaces_line_six() {
        let tmp = TempDir::new();

        let outcome = init_resolved_spec_dir(&tmp, "docs/myspecs".to_string(), true).unwrap();

        assert_eq!(outcome.spec_dir, "docs/myspecs");
        assert!(outcome.adopted);
        assert!(tmp.join("docs/myspecs/changes/archive").is_dir());
        assert!(tmp.join("docs/myspecs/specs").is_dir());
        assert!(tmp.join("docs/myspecs/config.yaml").is_file());
        let config = std::fs::read_to_string(tmp.join(".spectra.yaml")).unwrap();
        assert_eq!(config.lines().nth(5), Some("spec_dir: docs/myspecs"));
        // 字面 32（不是 SPECTRA_CONFIG_TEMPLATE.lines().count()）：拿產出跟
        // 同一個常數比是自我參照，模板長度回歸兩邊一起變、斷言恆真。oracle
        // 實測為 32 個 \n 結尾行（PR #101 review；761 bytes 是 default render
        // 的數字——本例的 spec_dir 替換行剛好等長，位元組數非本測試所釘）。
        assert_eq!(config.lines().count(), 32);
    }

    #[test]
    fn init_creates_the_oracle_spectra_config_template() {
        let tmp = TempDir::new();

        init(&tmp).unwrap();

        assert_eq!(
            std::fs::read_to_string(tmp.join(".spectra.yaml")).unwrap(),
            concat!(
                "# Spectra application config\n",
                "# See: https://github.com/spectra-app/spectra\n",
                "\n",
                "# OpenSpec directory path (relative to project root)\n",
                "# Changing this requires rebuilding the vector search index.\n",
                "# spec_dir: docs/specs\n",
                "\n",
                "# Language for AI-generated artifacts\n",
                "# locale: tw\n",
                "\n",
                "# Workflow toggles\n",
                "# tdd: true\n",
                "# audit: true\n",
                "# parallel_tasks: true\n",
                "\n",
                "# Claude slash commands (set true to also generate /spectra:X commands)\n",
                "# claude_slash_commands: true\n",
                "\n",
                "# Enable git worktree support for isolated change branches\n",
                "# worktree: true\n",
                "\n",
                "# Custom git worktrees directory\n",
                "# worktrees_dir: .spectra/worktrees\n",
                "\n",
                "# Claude Code skill effort levels (low/medium/high/xhigh/max)\n",
                "# claude_effort:\n",
                "#   apply: high\n",
                "\n",
                "# AI tools to generate instruction files for\n",
                "# tools:\n",
                "#   - claude\n",
                "#   - cursor\n",
            )
        );
    }

    #[test]
    fn init_errors_when_already_initialized() {
        let tmp = TempDir::new();
        init(&tmp).unwrap();

        let err = init(&tmp).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Already initialized. Use --force to reinitialize."
        );
    }

    #[test]
    fn init_adopt_preserves_existing_openspec_content() {
        let tmp = TempDir::new();
        let project = tmp.join("openspec/project.md");
        let spec_config = tmp.join("openspec/config.yaml");
        let spec = tmp.join("openspec/specs/search/spec.md");
        std::fs::create_dir_all(spec.parent().unwrap()).unwrap();
        std::fs::write(&project, "# Existing project\n\nDo not touch.\n").unwrap();
        std::fs::write(&spec_config, "schema: custom\n").unwrap();
        std::fs::write(
            &spec,
            "# Search Specification\n\n## Requirements\n\n### Requirement: Search\n\nExisting.\n",
        )
        .unwrap();

        let outcome = init_with_options(&tmp, true, false, None).unwrap();

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
            std::fs::read_to_string(&spec_config).unwrap(),
            "schema: custom\n"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.join(".spectra.yaml")).unwrap(),
            SPECTRA_CONFIG_TEMPLATE
        );
    }

    #[test]
    fn init_adopt_errors_when_already_initialized() {
        let tmp = TempDir::new();
        init(&tmp).unwrap();

        let err = init_with_options(&tmp, true, false, None).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Already initialized. Use --force to reinitialize."
        );
    }

    #[test]
    fn init_adopt_on_empty_dir_creates_the_default_skeleton() {
        let tmp = TempDir::new();

        let outcome = init_with_options(&tmp, true, false, None).unwrap();

        assert!(outcome.adopted);
        assert_eq!(outcome.spec_dir, "openspec");
        assert!(tmp.join("openspec/changes").is_dir());
        assert!(tmp.join("openspec/changes/archive").is_dir());
        assert!(tmp.join("openspec/specs").is_dir());
        assert!(tmp.join("openspec/config.yaml").is_file());
        assert!(tmp.join(".spectra.yaml").is_file());
    }

    #[test]
    fn init_adopt_errors_when_openspec_exists_as_a_file() {
        // A clear "not a directory" error beats letting the later
        // create_dir_all surface a generic I/O error.
        let tmp = TempDir::new();
        std::fs::write(tmp.join("openspec"), "not a directory\n").unwrap();

        let err = init_with_options(&tmp, true, false, None).unwrap_err();

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
        assert_eq!(contents, "# Spectra app data\n.spectra/\n");
    }

    #[test]
    fn init_treats_an_empty_existing_gitignore_like_a_missing_one() {
        // 分支邊界（PR #101 review NIT）：既有但零位元組的 .gitignore 走
        // 「空內容不加分隔空行」路徑，產出與全新檔案相同。此案例 oracle
        // 未 probe（AC-2 未主張），僅釘住 OpenSpectra 自身行為。
        let tmp = TempDir::new();
        std::fs::write(tmp.join(".gitignore"), "").unwrap();

        let outcome = init(&tmp).unwrap();

        assert!(outcome.gitignore_updated);
        assert_eq!(
            std::fs::read_to_string(tmp.join(".gitignore")).unwrap(),
            "# Spectra app data\n.spectra/\n"
        );
    }

    #[test]
    fn init_appends_the_oracle_block_to_a_non_empty_gitignore() {
        let tmp = TempDir::new();
        std::fs::write(tmp.join(".gitignore"), "node_modules/\n*.log\n").unwrap();

        let outcome = init(&tmp).unwrap();

        assert!(outcome.gitignore_updated);
        assert_eq!(
            std::fs::read_to_string(tmp.join(".gitignore")).unwrap(),
            "node_modules/\n*.log\n\n# Spectra app data\n.spectra/\n"
        );
    }

    #[test]
    fn init_appends_to_an_existing_gitignore_without_a_trailing_newline() {
        let tmp = TempDir::new();
        std::fs::write(tmp.join(".gitignore"), "target/").unwrap();

        let outcome = init(&tmp).unwrap();

        assert!(outcome.gitignore_updated);
        let contents = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        assert_eq!(contents, "target/\n\n# Spectra app data\n.spectra/\n");
    }

    #[test]
    fn init_preserves_crlf_line_endings_when_appending_to_gitignore() {
        let tmp = TempDir::new();
        std::fs::write(tmp.join(".gitignore"), "target/\r\n").unwrap();

        let outcome = init(&tmp).unwrap();

        assert!(outcome.gitignore_updated);
        let contents = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
        assert_eq!(
            contents,
            "target/\r\n\r\n# Spectra app data\r\n.spectra/\r\n"
        );
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
