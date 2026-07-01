//! Thin wrappers over the `git` CLI via `std::process::Command`.
//!
//! Everything degrades gracefully when git is unavailable or the directory is
//! not a repository: callers receive empty results / `None` rather than errors,
//! mirroring the reference binary's "git unavailable" status.

use std::collections::HashSet;
use std::path::Path;
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

/// Count of commits authored on or after `since` (a `YYYY-MM-DD` date),
/// via `git rev-list --count --since=<date> HEAD`.
pub fn commits_since(root: &Path, since: &str) -> u64 {
    let arg = format!("--since={since}");
    git(root, &["rev-list", "--count", &arg, "HEAD"])
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Paths of all dirty files (modified, staged, or untracked), relative to
/// `root`. A rename ("old -> new") reports the new path. `None` when `root`
/// isn't a git repository or the underlying git commands fail — distinct
/// from `Some(vec![])`, which means git ran successfully and found nothing
/// dirty, so callers can tell "couldn't check" from "genuinely clean".
///
/// `git status --porcelain` itself always reports paths relative to the
/// **repository root**, not `-C <root>` (unlike `ls_files`), so a project
/// nested in a git repo subdirectory needs its output re-anchored to
/// `root` via `rev-parse --show-toplevel`; paths outside `root` (e.g. a
/// monorepo sibling) are dropped as out of scope. `root` is canonicalized
/// before that comparison, since git's own output is always canonical
/// (e.g. macOS resolves `/var/...` to `/private/var/...`) and a symlinked
/// `root` would otherwise silently drop every path as "outside root".
pub fn dirty_files(root: &Path) -> Option<Vec<String>> {
    let repo_root = git(root, &["rev-parse", "--show-toplevel"])?;
    let repo_root = Path::new(repo_root.trim()).to_path_buf();
    let canonical_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let out = git(root, &["status", "--porcelain"])?;
    Some(
        out.lines()
            .filter(|line| line.len() > 3)
            .filter_map(|line| {
                let rest = &line[3..];
                let repo_rel = rest.rsplit_once(" -> ").map_or(rest, |(_, new)| new).trim();
                let repo_rel = unquote_git_path(repo_rel);
                let abs = repo_root.join(repo_rel);
                let rel = abs.strip_prefix(&canonical_root).ok()?;
                Some(rel.to_string_lossy().replace('\\', "/"))
            })
            .collect(),
    )
}

/// Undo git's porcelain path quoting (`core.quotePath`, on by default):
/// a path containing non-ASCII bytes or control characters is wrapped in
/// double quotes with C-style backslash/octal escapes. Paths without
/// quoting needed are returned unchanged.
fn unquote_git_path(s: &str) -> String {
    let Some(inner) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return s.to_string();
    };
    let bytes = inner.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'\\' {
            out.push(bytes[i]);
            i += 1;
            continue;
        }
        if i + 4 <= bytes.len() && bytes[i + 1].is_ascii_digit() {
            if let Ok(v) =
                u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 4]).unwrap_or(""), 8)
            {
                out.push(v);
                i += 4;
                continue;
            }
        }
        match bytes.get(i + 1) {
            Some(b'n') => {
                out.push(b'\n');
                i += 2;
            }
            Some(b't') => {
                out.push(b'\t');
                i += 2;
            }
            Some(b'\\') => {
                out.push(b'\\');
                i += 2;
            }
            Some(b'"') => {
                out.push(b'"');
                i += 2;
            }
            _ => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// True if `needle` appears in any tracked file (`git grep -F -l`),
/// i.e. the symbol/function is referenced or defined somewhere in the repo.
pub fn grep_exists(root: &Path, needle: &str) -> bool {
    // -F fixed string, -l names only, -I skip binary. Word boundaries are not
    // used so a definition like `fn foo(` or a struct `Foo` both match.
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["grep", "-F", "-I", "-l", "-e", needle])
        .output();
    match out {
        // `git grep` exits 0 on a match, 1 on a clean "no match", and >1 on a
        // real error (not a repo, bad pathspec, ...). Only a clean no-match
        // means the anchor is broken; on any error we cannot tell, so we do
        // NOT over-report drift — treat it as resolved.
        Ok(o) => o.status.code() != Some(1),
        Err(_) => true,
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
    fn unquote_git_path_leaves_unquoted_paths_unchanged() {
        assert_eq!(unquote_git_path("src/foo.rs"), "src/foo.rs");
    }

    #[test]
    fn unquote_git_path_decodes_octal_escapes() {
        // git quotes a path containing the UTF-8 bytes for "é" (0xc3 0xa9)
        // as octal escapes when core.quotePath is on (the default).
        assert_eq!(unquote_git_path("\"caf\\303\\251.txt\""), "café.txt");
    }

    #[test]
    fn unquote_git_path_decodes_backslash_escapes() {
        assert_eq!(unquote_git_path("\"a\\\\b\""), "a\\b");
        assert_eq!(unquote_git_path("\"a\\\"b\""), "a\"b");
    }
}
