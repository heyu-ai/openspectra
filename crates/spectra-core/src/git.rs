//! Thin wrappers over the `git` CLI via `std::process::Command`.
//!
//! Everything degrades gracefully when git is unavailable or the directory is
//! not a repository: callers receive empty results / `None` rather than errors,
//! mirroring the reference binary's "git unavailable" status.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run `git -C <root> <args...>` and return stdout on success (exit 0).
fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        None
    }
}

/// True when `root` is inside a git work tree and git is callable.
pub fn is_repo(root: &Path) -> bool {
    git(root, &["rev-parse", "--is-inside-work-tree"])
        .map(|s| s.trim() == "true")
        .unwrap_or(false)
}

/// The repository's *common* git directory — the shared `.git` even when
/// `root` is a linked worktree, where `--git-dir` would point at
/// `.git/worktrees/<name>` instead.
///
/// The oracle stores parked changes under this directory, and it resolves to
/// the same place from every worktree: parking from a linked worktree and
/// listing from the main checkout show the same change (probed on v2.3.1).
pub fn common_dir(root: &Path) -> Option<PathBuf> {
    let out = git(root, &["rev-parse", "--git-common-dir"])?;
    let path = PathBuf::from(out.trim());
    if path.as_os_str().is_empty() {
        return None;
    }
    // git prints a path relative to the work tree for the common case (".git").
    Some(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

/// All tracked file paths (repo-relative, forward slashes), via `git ls-files`.
pub fn ls_files(root: &Path) -> HashSet<String> {
    git(root, &["ls-files"])
        .map(|s| s.lines().map(|l| l.trim().to_string()).collect())
        .unwrap_or_default()
}

/// The current `HEAD` commit SHA, via `git rev-parse HEAD`. `None` when
/// `root` isn't a git repository or has no commits yet.
pub fn head_sha(root: &Path) -> Option<String> {
    git(root, &["rev-parse", "HEAD"])
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Whether `path`, relative to `root`, existed at the baseline `revision`.
///
/// Returns `None` when the revision is not a full SHA, the repository cannot
/// be inspected, or the revision is unavailable. A definite missing path is
/// `Some(false)`, allowing callers to distinguish a forward reference from a
/// deleted file without turning an unusable baseline into a false negative.
pub fn path_exists_at(root: &Path, revision: &str, path: &str) -> Option<bool> {
    let valid_sha =
        matches!(revision.len(), 40 | 64) && revision.bytes().all(|byte| byte.is_ascii_hexdigit());
    if !valid_sha {
        return None;
    }

    // `ls-tree --full-tree` expects a repository-root-relative path. `root`
    // may itself be a project nested below the repository root, so preserve
    // git's own prefix rather than assuming the two roots coincide.
    let prefix = git(root, &["rev-parse", "--show-prefix"])?;
    let repo_path = format!("{}{}", prefix.trim_end_matches('\n'), path);
    let out = git(
        root,
        &["ls-tree", "-z", "--full-tree", revision, "--", &repo_path],
    )?;
    Some(!out.is_empty())
}

/// Last commit date for `path` in `YYYY-MM-DD` form. Returns `None` when the
/// repository has no commit for the file or git is unavailable.
pub fn last_commit_date(root: &Path, path: &str) -> Option<String> {
    git(root, &["log", "-1", "--format=%cs", "--", path])
        .map(|date| date.trim().to_string())
        .filter(|date| !date.is_empty())
}

/// The configured committer identity in git's own "Name <email>" format, via
/// `git config user.name`/`user.email`. `None` if either is unset (or not a
/// git repo) — best-effort, matching every other identity/state lookup in
/// this module.
pub fn user_identity(root: &Path) -> Option<String> {
    let name = git(root, &["config", "user.name"])?.trim().to_string();
    let email = git(root, &["config", "user.email"])?.trim().to_string();
    if name.is_empty() || email.is_empty() {
        return None;
    }
    Some(format!("{name} <{email}>"))
}

/// The identity format used by `spectra new change`: unlike archive metadata,
/// partial git configuration is preserved and the key always has a value.
pub fn change_creator_identity(root: &Path) -> String {
    // `git config` falls back to user-level config outside a repo, matching the oracle.
    let configured = |key| {
        git(root, &["config", key])
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    match (configured("user.name"), configured("user.email")) {
        (Some(name), Some(email)) => format!("{name} <{email}>"),
        (Some(name), None) => name,
        (None, Some(email)) => format!("<{email}>"),
        (None, None) => "unknown".to_string(),
    }
}

/// Count of commits authored on or after `since` (a `YYYY-MM-DD` date),
/// via `git rev-list --count --since=<date> HEAD`.
pub fn commits_since(root: &Path, since: &str) -> u64 {
    let arg = format!("--since={since}");
    git(root, &["rev-list", "--count", &arg, "HEAD"])
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Paths of all dirty files (modified, staged, or untracked), relative to
/// `root`. A rename/copy reports the new path. `None` when `root` isn't a
/// git repository or the underlying git commands fail — distinct from
/// `Some(vec![])`, which means git ran successfully and found nothing dirty,
/// so callers can tell "couldn't check" from "genuinely clean".
///
/// Uses `-z` (NUL-delimited, `porcelain=v1`) rather than the human-readable
/// porcelain format: with `-z`, git never quotes/escapes a path (not even
/// ones with spaces, non-ASCII bytes, or a literal " -> "), and a rename's
/// old path is a genuinely separate NUL-terminated field rather than text
/// joined with " -> " — both of which make ad hoc trimming/splitting of the
/// human-readable format lossy for legal-but-unusual filenames.
///
/// `git status` itself always reports paths relative to the **repository
/// root**, not `-C <root>` (unlike `ls_files`), so a project nested in a git
/// repo subdirectory needs its output re-anchored to `root` via
/// `rev-parse --show-toplevel`; paths outside `root` (e.g. a monorepo
/// sibling) are dropped as out of scope. `root` is canonicalized before that
/// comparison, since git's own output is always canonical (e.g. macOS
/// resolves `/var/...` to `/private/var/...`) and a symlinked `root` would
/// otherwise silently drop every path as "outside root".
pub fn dirty_files(root: &Path) -> Option<Vec<String>> {
    let repo_root = git(root, &["rev-parse", "--show-toplevel"])?;
    // trim_end (not trim): a repo path could legally have leading spaces;
    // only the trailing newline from command output needs stripping.
    let repo_root = Path::new(repo_root.trim_end_matches('\n')).to_path_buf();
    // Canonicalize-failure is treated the same as any other git-command
    // failure (`None`) rather than silently falling back to the raw `root`:
    // a raw fallback that happens to be relative (or otherwise not
    // byte-identical to git's always-canonical, always-absolute output)
    // would make every `strip_prefix` below fail, silently producing
    // `Some(vec![])` -- indistinguishable from "genuinely clean" -- instead
    // of the "couldn't determine" signal this function exists to give.
    let canonical_root = std::fs::canonicalize(root).ok()?;
    // --untracked-files=all: without it, a new untracked *directory* collapses
    // to a single "newdir/" porcelain entry instead of listing the files
    // inside it, which would make touched-file tracking record a directory
    // path rather than the actual files a task touched.
    let out = git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    let mut files = Vec::new();
    while let Some(entry) = fields.next() {
        if entry.len() <= 3 {
            continue;
        }
        // A rename/copy status ("R "/"C " in either status column) emits the
        // new path as this field, followed by a *separate* NUL-terminated
        // field for the old path -- consume and discard that second field so
        // it isn't misread as its own status entry.
        let is_rename_or_copy =
            entry.as_bytes()[..2].contains(&b'R') || entry.as_bytes()[..2].contains(&b'C');
        let repo_rel = &entry[3..];
        if is_rename_or_copy {
            fields.next();
        }
        let abs = repo_root.join(repo_rel);
        if let Ok(rel) = abs.strip_prefix(&canonical_root) {
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Some(files)
}

/// For each needle, whether it appears in any tracked file, computed with a
/// single `git grep` invocation. Returns the set of needles that resolve.
///
/// This intentionally uses `git grep -F -I -h` and then checks whether each
/// needle is a substring of any matching output line. `-l` is wrong for batched
/// checks because it reports only matching files under OR semantics, losing
/// which needle matched. `-o` is also wrong because `git grep` prints only the
/// matched substrings and scans each line left-to-right without re-scanning
/// consumed characters: with needles `abc` and `cde` against a line `abcde`,
/// `-o` prints only `abc`, so a substring check would miss `cde` even though it
/// is present. With `-h`, the whole matching line is printed, so a needle
/// appears in tracked content iff it is a substring of at least one printed
/// line, preserving the old per-needle `git grep -F` behavior even for
/// overlapping text.
///
/// Exit codes preserve the previous `grep_exists` contract: `0` means parse
/// stdout; `1` is a clean no-match for all needles; every other status,
/// including signal termination (`code() == None`) or spawn failure, is a real
/// error where we cannot tell, so all input needles are treated as resolved to
/// avoid over-reporting drift.
pub fn grep_existing(root: &Path, needles: &[&str]) -> HashSet<String> {
    if needles.is_empty() {
        return HashSet::new();
    }

    let mut all: HashSet<String> = HashSet::with_capacity(needles.len());
    all.extend(needles.iter().map(|needle| (*needle).to_string()));

    let mut args = Vec::with_capacity(5 + needles.len() * 2);
    args.push("grep".to_string());
    args.push("-F".to_string());
    args.push("-I".to_string());
    args.push("-h".to_string());
    // --no-color, not `-c color.ui=never`: a user gitconfig with
    // `color.grep=always` forces ANSI escapes into the piped output even for a
    // non-tty, and `color.grep` takes precedence over the generic `color.ui`, so
    // `-c color.ui=never` does NOT suppress it (verified empirically). The
    // `--no-color` grep flag overrides any color config unconditionally. This
    // matters because git colorizes each -e match separately, so the escapes
    // can land between adjacent matched substrings and break the
    // `line.contains(needle)` check below, producing a false "not found".
    args.push("--no-color".to_string());
    // Iterate the deduped `all` set (not `needles`) so a caller passing repeated
    // needles does not append redundant `-e <needle>` pairs; -e order does not
    // affect results.
    for needle in &all {
        args.push("-e".to_string());
        args.push(needle.clone());
    }

    // -c grep.lineNumber=false / grep.column=false: both are real git config
    // keys that, when set true in a user's gitconfig, prefix -h output lines
    // with `N:` / `col:`; neutralize them so the parsed lines are pure content.
    let out = Command::new("git")
        .args(["-c", "grep.lineNumber=false", "-c", "grep.column=false"])
        .arg("-C")
        .arg(root)
        .args(args)
        .output();

    match out {
        Ok(o) => match o.status.code() {
            Some(0) => {
                let mut pending = all.clone();
                let mut resolved = HashSet::new();
                let stdout = String::from_utf8_lossy(&o.stdout);
                for line in stdout.lines() {
                    pending.retain(|needle| {
                        if line.contains(needle.as_str()) {
                            resolved.insert(needle.clone());
                            false
                        } else {
                            true
                        }
                    });
                    if pending.is_empty() {
                        break;
                    }
                }
                resolved
            }
            Some(1) => HashSet::new(),
            other => {
                // exit >1 (not a repo, corrupt index, bad pathspec) or signal
                // termination (code() == None). We cannot tell which anchors
                // resolve, so we treat them all as resolved to avoid
                // over-reporting drift -- but surface a diagnostic so a broken
                // git environment is not silently indistinguishable from a
                // healthy clean check.
                eprintln!(
                    "spectra: `git grep` exited with status {other:?}; \
                     treating all {} anchor(s) as resolved to avoid over-reporting drift",
                    all.len()
                );
                all
            }
        },
        Err(e) => {
            eprintln!(
                "spectra: could not spawn `git grep` ({e}); \
                 treating all {} anchor(s) as resolved to avoid over-reporting drift",
                all.len()
            );
            all
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "spectra-git-test-{label}-{}-{seq}-{}",
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

    fn init_repo(dir: &Path) {
        let run = |args: &[&str]| {
            assert!(Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "t@t.co"]);
        run(&["config", "user.name", "t"]);
    }

    fn init_repo_with_commit(dir: &Path) -> String {
        init_repo(dir);
        let run = |args: &[&str]| {
            assert!(Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        };
        std::fs::write(dir.join("f.txt"), "hi\n").unwrap();
        run(&["add", "f.txt"]);
        run(&["commit", "-q", "-m", "init"]);
        git(dir, &["rev-parse", "HEAD"]).unwrap().trim().to_string()
    }

    fn commit_file(dir: &Path, name: &str, contents: &str) {
        let run = |args: &[&str]| {
            assert!(Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .unwrap()
                .status
                .success());
        };
        std::fs::write(dir.join(name), contents).unwrap();
        run(&["add", name]);
        run(&["commit", "-q", "-m", name]);
    }

    #[test]
    fn grep_existing_empty_needles_returns_empty_set() {
        let dir = TempDir::new("grep-empty");

        assert_eq!(grep_existing(&dir, &[]), HashSet::new());
    }

    #[test]
    fn grep_existing_returns_present_needles() {
        let dir = TempDir::new("grep-present");
        init_repo(&dir);
        commit_file(
            &dir,
            "code.rs",
            "fn alpha_fn_0() {}\nstruct PresentSymbol;\n",
        );

        let resolved = grep_existing(
            &dir,
            &[
                "alpha_fn_0",
                "missing_fn_1",
                "PresentSymbol",
                "MissingSymbol",
            ],
        );

        assert_eq!(
            resolved,
            ["alpha_fn_0", "PresentSymbol"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
    }

    #[test]
    fn grep_existing_resolves_partially_overlapping_needles() {
        // Regression guard for the `-h`-over-`-o` decision. The needles `abc`
        // and `cde` partially overlap in the line `abcde`. `-h` prints the whole
        // line, so both resolve. The rejected `-o` would print only the
        // leftmost match `abc` (scanning continues past the consumed `abc`, so
        // `cde` is never emitted), and a substring check over that output would
        // miss `cde` -- so this test FAILS under `-o` and passes only under the
        // shipped `-h`. A nested example (e.g. `abc`/`abcde`) would not
        // discriminate, since `abc` is a substring of `abcde`.
        let dir = TempDir::new("grep-overlap");
        init_repo(&dir);
        commit_file(&dir, "code.txt", "abcde\n");

        let resolved = grep_existing(&dir, &["abc", "cde"]);

        assert_eq!(
            resolved,
            ["abc", "cde"].into_iter().map(str::to_string).collect()
        );
    }

    #[test]
    fn grep_existing_ignores_user_color_grep_always() {
        // Regression: `color.grep=always` forces ANSI escapes into piped output
        // and takes precedence over `color.ui` (so `-c color.ui=never` alone does
        // NOT suppress it). git highlights matches left-to-right, so for
        // `abc`/`cde` on `abcde` it colors only `abc` and inserts a reset escape
        // after it (`\e[..abc\e[..de`); then `contains("cde")` is false because
        // the escape splits `c` from `de`, producing a false "not found in repo".
        // grep_existing passes `--no-color`, which overrides any color config
        // unconditionally. Without `--no-color` this test fails (cde is lost).
        let dir = TempDir::new("grep-color");
        init_repo(&dir);
        assert!(Command::new("git")
            .arg("-C")
            .arg(&*dir)
            .args(["config", "color.grep", "always"])
            .output()
            .unwrap()
            .status
            .success());
        commit_file(&dir, "code.txt", "abcde\n");

        let resolved = grep_existing(&dir, &["abc", "cde"]);

        assert_eq!(
            resolved,
            ["abc", "cde"].into_iter().map(str::to_string).collect()
        );
    }

    #[test]
    fn grep_existing_treats_non_repo_errors_as_resolved() {
        // A guaranteed-nonexistent path makes `git -C <path>` fail to chdir and
        // exit 128 (>1), deterministically exercising the "cannot tell -> treat
        // all as resolved" arm with no filesystem dependency. (A real temp dir
        // could sit inside a git checkout in some CI/sandbox setups, where git
        // grep would instead exit 1 and this test would exercise the wrong
        // branch.)
        let root = Path::new("/nonexistent/spectra-grep-existing-not-a-repo-xyz");

        let resolved = grep_existing(root, &["alpha_fn_0", "PresentSymbol"]);

        assert_eq!(
            resolved,
            ["alpha_fn_0", "PresentSymbol"]
                .into_iter()
                .map(str::to_string)
                .collect()
        );
    }

    #[test]
    fn head_sha_returns_the_current_commit() {
        let dir = TempDir::new("head");
        let expected = init_repo_with_commit(&dir);

        assert_eq!(head_sha(&dir), Some(expected));
    }

    #[test]
    fn head_sha_is_none_outside_a_git_repo() {
        let dir = TempDir::new("norepo");

        assert_eq!(head_sha(&dir), None);
    }

    #[test]
    fn head_sha_is_none_in_a_repo_with_no_commits() {
        let dir = TempDir::new("nocommit");
        init_repo(&dir);

        assert_eq!(head_sha(&dir), None);
    }

    #[test]
    fn path_exists_at_distinguishes_present_and_missing_paths() {
        let dir = TempDir::new("path-at-revision");
        let baseline = init_repo_with_commit(&dir);

        assert_eq!(path_exists_at(&dir, &baseline, "f.txt"), Some(true));
        assert_eq!(
            path_exists_at(&dir, &baseline, "never-existed.txt"),
            Some(false)
        );
    }

    #[test]
    fn path_exists_at_returns_none_for_an_unavailable_baseline() {
        let dir = TempDir::new("path-at-invalid-revision");
        init_repo_with_commit(&dir);

        assert_eq!(path_exists_at(&dir, "not-a-sha", "f.txt"), None);
        assert_eq!(path_exists_at(&dir, &"0".repeat(40), "f.txt"), None);
    }

    #[test]
    fn dirty_files_is_none_outside_a_git_repo() {
        let dir = TempDir::new("dirty-norepo");
        assert_eq!(dirty_files(&dir), None);
    }

    #[test]
    fn dirty_files_reports_modified_staged_and_untracked_paths() {
        let dir = TempDir::new("dirty-mixed");
        init_repo_with_commit(&dir);
        std::fs::write(dir.join("f.txt"), "changed\n").unwrap();
        std::fs::write(dir.join("staged.txt"), "new\n").unwrap();
        assert!(Command::new("git")
            .arg("-C")
            .arg(&*dir)
            .args(["add", "staged.txt"])
            .output()
            .unwrap()
            .status
            .success());
        std::fs::write(dir.join("untracked.txt"), "new\n").unwrap();

        let mut files = dirty_files(&dir).unwrap();
        files.sort();
        assert_eq!(
            files,
            vec![
                "f.txt".to_string(),
                "staged.txt".to_string(),
                "untracked.txt".to_string()
            ]
        );
    }

    #[test]
    fn dirty_files_expands_an_untracked_directory_into_its_files() {
        // Without --untracked-files=all, git collapses a wholly-untracked
        // directory to a single "newdir/" porcelain entry instead of listing
        // the files inside it.
        let dir = TempDir::new("dirty-untracked-dir");
        init_repo_with_commit(&dir);
        std::fs::create_dir_all(dir.join("newdir")).unwrap();
        std::fs::write(dir.join("newdir").join("a.rs"), "a\n").unwrap();
        std::fs::write(dir.join("newdir").join("b.rs"), "b\n").unwrap();

        let mut files = dirty_files(&dir).unwrap();
        files.sort();
        assert_eq!(
            files,
            vec!["newdir/a.rs".to_string(), "newdir/b.rs".to_string()]
        );
    }

    #[test]
    fn dirty_files_reports_the_new_path_for_a_rename() {
        let dir = TempDir::new("dirty-rename");
        init_repo_with_commit(&dir);
        assert!(Command::new("git")
            .arg("-C")
            .arg(&*dir)
            .args(["mv", "f.txt", "renamed.txt"])
            .output()
            .unwrap()
            .status
            .success());

        let files = dirty_files(&dir).unwrap();
        assert_eq!(files, vec!["renamed.txt".to_string()]);
    }

    #[test]
    fn dirty_files_does_not_mangle_an_untracked_file_literally_named_with_arrow() {
        // -z never quotes/splits on text, so a literal " -> " substring in an
        // ordinary (non-rename) filename passes through untouched.
        let dir = TempDir::new("dirty-arrow-name");
        init_repo_with_commit(&dir);
        std::fs::write(dir.join("a -> b.txt"), "new\n").unwrap();

        let files = dirty_files(&dir).unwrap();
        assert_eq!(files, vec!["a -> b.txt".to_string()]);
    }

    #[test]
    fn dirty_files_reports_a_rename_target_that_itself_contains_an_arrow() {
        // The old human-readable "old -> new" porcelain format is genuinely
        // ambiguous here (rsplit_once(" -> ") would misparse the target);
        // -z's separate NUL-terminated fields resolve it unambiguously.
        let dir = TempDir::new("dirty-rename-arrow-target");
        init_repo_with_commit(&dir);
        assert!(Command::new("git")
            .arg("-C")
            .arg(&*dir)
            .args(["mv", "f.txt", "a -> b.txt"])
            .output()
            .unwrap()
            .status
            .success());

        let files = dirty_files(&dir).unwrap();
        assert_eq!(files, vec!["a -> b.txt".to_string()]);
    }

    #[test]
    fn dirty_files_preserves_spaces_and_non_ascii_bytes_without_quoting() {
        let dir = TempDir::new("dirty-unicode-space");
        init_repo_with_commit(&dir);
        std::fs::write(dir.join("café with space.txt"), "new\n").unwrap();

        let files = dirty_files(&dir).unwrap();
        assert_eq!(files, vec!["café with space.txt".to_string()]);
    }

    #[test]
    fn user_identity_formats_local_repo_config_as_name_and_email() {
        let dir = TempDir::new("identity-set");
        init_repo(&dir); // sets local user.name/user.email

        assert_eq!(user_identity(&dir), Some("t <t@t.co>".to_string()));
    }

    // The "no identity configured anywhere" / "non-repo global fallback"
    // variants live in tests/git_identity_isolation.rs: they must mutate
    // process-wide env vars (`GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`), which
    // is not safe in this parallel lib-test binary while sibling tests
    // concurrently spawn processes (observed flaky `git init` failures).

    #[test]
    fn change_creator_identity_preserves_partial_git_configuration() {
        let dir = TempDir::new("change-creator-matrix");
        init_repo_no_identity(&dir);
        let set = |key: &str, value: &str| {
            assert!(Command::new("git")
                .arg("-C")
                .arg(&*dir)
                .args(["config", key, value])
                .output()
                .unwrap()
                .status
                .success());
        };

        set("user.name", "Howie");
        set("user.email", "howie@example.com");
        assert_eq!(change_creator_identity(&dir), "Howie <howie@example.com>");

        set("user.email", "");
        assert_eq!(change_creator_identity(&dir), "Howie");

        set("user.name", "");
        set("user.email", "howie@example.com");
        assert_eq!(change_creator_identity(&dir), "<howie@example.com>");

        set("user.email", "");
        assert_eq!(change_creator_identity(&dir), "unknown");
    }

    fn init_repo_no_identity(dir: &Path) {
        assert!(Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["init", "-q"])
            .output()
            .unwrap()
            .status
            .success());
    }
}
