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

/// Count of commits authored on or after `since` (a `YYYY-MM-DD` date),
/// via `git rev-list --count --since=<date> HEAD`.
pub fn commits_since(root: &Path, since: &str) -> u64 {
    let arg = format!("--since={since}");
    git(root, &["rev-list", "--count", &arg, "HEAD"])
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
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
