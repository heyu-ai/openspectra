//! CLI-level integration tests that spawn the built `spectra` binary, for
//! contracts that depend on full dispatch through `run()` (not just clap
//! parsing) -- unit tests in `main.rs` cover the parser; these cover the
//! actual runtime behavior.

use std::path::{Path, PathBuf};
use std::process::Command;

fn spectra() -> Command {
    Command::new(env!("CARGO_BIN_EXE_spectra"))
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs")
        .status
        .success();
    assert!(ok, "git {args:?} failed");
}

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "spectra-cli-it-{label}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
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
fn list_changes_flag_output_is_byte_identical_to_the_default() {
    let tmp = TempDir::new("list-changes");
    git(&tmp, &["init", "-q"]);
    git(&tmp, &["config", "user.email", "t@t.co"]);
    git(&tmp, &["config", "user.name", "t"]);

    let init = spectra().arg("init").current_dir(&*tmp).output().unwrap();
    assert!(init.status.success(), "init failed: {init:?}");
    let new_change = spectra()
        .args(["new", "change", "add-search-filter"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(
        new_change.status.success(),
        "new change failed: {new_change:?}"
    );

    let default_human = spectra().arg("list").current_dir(&*tmp).output().unwrap();
    let changes_human = spectra()
        .args(["list", "--changes"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(
        default_human.status.success(),
        "list failed: {default_human:?}"
    );
    assert!(
        changes_human.status.success(),
        "list --changes failed: {changes_human:?}"
    );
    assert_eq!(default_human.stdout, changes_human.stdout);

    let default_json = spectra()
        .args(["list", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    let changes_json = spectra()
        .args(["list", "--changes", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(
        default_json.status.success(),
        "list --json failed: {default_json:?}"
    );
    assert!(
        changes_json.status.success(),
        "list --changes --json failed: {changes_json:?}"
    );
    assert_eq!(default_json.stdout, changes_json.stdout);
    assert!(!default_json.stdout.is_empty());
}

#[test]
fn init_text_output_reports_root_spec_dir_and_gitignore_update() {
    let tmp = TempDir::new("init-text");
    git(&tmp, &["init", "-q"]);

    let out = spectra().arg("init").current_dir(&*tmp).output().unwrap();
    assert!(out.status.success(), "init failed: {out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();

    // Canonicalize before comparing: on macOS `std::env::temp_dir()` returns
    // a `/var/...` path that's actually a symlink to `/private/var/...`, and
    // the CLI reports whatever `std::env::current_dir()` resolves to after
    // `cd`-ing in, which follows the symlink -- a raw `contains` would only
    // pass by coincidence (see the sibling JSON test for the same issue).
    let canonical_root = tmp.canonicalize().unwrap();
    assert!(stdout.contains(&canonical_root.display().to_string()));
    assert!(stdout.contains("spec_dir: openspec"));
    assert!(stdout.contains("Added '.spectra/' to .gitignore."));
}

#[test]
fn init_json_output_matches_the_documented_shape() {
    let tmp = TempDir::new("init-json");
    git(&tmp, &["init", "-q"]);

    let out = spectra()
        .args(["init", "--json"])
        .current_dir(&*tmp)
        .output()
        .unwrap();
    assert!(out.status.success(), "init --json failed: {out:?}");
    let stdout = String::from_utf8(out.stdout).unwrap();

    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["spec_dir"], "openspec");
    assert_eq!(value["gitignore_updated"], true);
    // Compare canonicalized paths: on macOS `std::env::temp_dir()` returns a
    // `/var/...` path that's actually a symlink to `/private/var/...`, and
    // the CLI reports whatever `std::env::current_dir()` resolves to after
    // `cd`-ing in, which follows the symlink.
    let reported_root = PathBuf::from(value["root"].as_str().unwrap());
    assert_eq!(
        reported_root.canonicalize().unwrap(),
        tmp.canonicalize().unwrap()
    );
}

#[test]
fn list_help_does_not_mention_changes_as_unimplemented() {
    let out = spectra().args(["list", "--help"]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.contains("not yet implemented"));
}
