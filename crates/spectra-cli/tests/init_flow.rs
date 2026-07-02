//! End-to-end flow test for `spectra init`: drives the *built binary* (via
//! `CARGO_BIN_EXE_spectra`) through the full Linux bootstrap path the roadmap
//! calls out — `init -> new change -> drift` — plus the two error edges
//! (running a command before init, and re-initializing an initialized tree).
//!
//! This exercises the real `clap` wiring and process exit codes, which the
//! in-crate unit tests (pure functions) deliberately don't.

use std::path::Path;
use std::process::{Command, Output};

/// RAII scratch directory, unique per test, removed on drop (even on panic).
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "spectra-init-flow-{}-{}-{seq}",
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

/// Run the built `spectra` binary with `args` in `cwd`.
fn spectra(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_spectra"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("failed to run spectra binary")
}

fn git(cwd: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .current_dir(cwd)
            .args(args)
            .output()
            .expect("failed to run git")
            .status
            .success(),
        "git {args:?} failed"
    );
}

#[test]
fn init_then_new_change_then_drift_succeeds_end_to_end() {
    let tmp = TempDir::new();
    // A real repo with one commit so `new change` can record a baseline SHA,
    // matching a realistic first-use setup.
    git(&tmp, &["init", "-q"]);
    git(&tmp, &["config", "user.email", "t@t.co"]);
    git(&tmp, &["config", "user.name", "t"]);
    std::fs::write(tmp.join("README.md"), "# demo\n").unwrap();
    git(&tmp, &["add", "README.md"]);
    git(&tmp, &["commit", "-q", "-m", "init"]);

    // init
    let out = spectra(&tmp, &["init"]);
    assert!(out.status.success(), "init should succeed: {out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Initialized spectra project"));
    assert!(tmp.join(".spectra.yaml").is_file());
    assert!(tmp.join("openspec").join("changes").is_dir());
    assert!(tmp.join("openspec").join("specs").is_dir());
    let gitignore = std::fs::read_to_string(tmp.join(".gitignore")).unwrap();
    assert!(gitignore.lines().any(|l| l.trim() == ".spectra/"));

    // new change
    let out = spectra(&tmp, &["new", "change", "add-thing"]);
    assert!(out.status.success(), "new change should succeed: {out:?}");
    assert!(tmp
        .join("openspec")
        .join("changes")
        .join("add-thing")
        .join("proposal.md")
        .is_file());

    // list --changes --json: the explicit active-change listing form
    // `capture-golden.sh` relies on. Must emit the `{"changes":[...]}` wrapper
    // with the change just created.
    let out = spectra(&tmp, &["list", "--changes", "--json"]);
    assert!(
        out.status.success(),
        "list --changes should succeed: {out:?}"
    );
    let listing: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("list --changes --json must emit valid JSON");
    let names: Vec<&str> = listing["changes"]
        .as_array()
        .expect("`changes` must be an array")
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["add-thing"]);

    // --changes is mutually exclusive with the other two selectors.
    let clash = spectra(&tmp, &["list", "--changes", "--specs"]);
    assert!(
        !clash.status.success(),
        "--changes with --specs must be rejected by clap"
    );

    // drift --json: a freshly scaffolded change has no broken anchors, so it
    // scores as light drift and exits 0 (the CI-gate success code).
    let out = spectra(&tmp, &["drift", "add-thing", "--json"]);
    assert!(
        out.status.success(),
        "drift on a fresh change should exit 0: {out:?}"
    );
    let report: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("drift --json must emit valid JSON");
    assert_eq!(report["change_id"], "add-thing");
    assert_eq!(report["severity"], "light");
}

#[test]
fn commands_before_init_fail_with_the_init_hint() {
    let tmp = TempDir::new();

    // `drift` before any init must fail (exit 3, the tool-error code) and tell
    // the user to run init — this is the message the whole `init` feature
    // exists to make actionable.
    let out = spectra(&tmp, &["drift"]);
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("spectra init"), "got: {stderr}");
}

#[test]
fn reinitializing_an_initialized_project_fails() {
    let tmp = TempDir::new();

    let first = spectra(&tmp, &["init"]);
    assert!(first.status.success());

    let second = spectra(&tmp, &["init"]);
    assert!(!second.status.success());
    assert_eq!(second.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(stderr.contains("already initialized"), "got: {stderr}");
}

#[test]
fn init_json_emits_a_machine_readable_object() {
    let tmp = TempDir::new();

    let out = spectra(&tmp, &["init", "--json"]);
    assert!(out.status.success(), "init --json should succeed: {out:?}");
    let value: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("init --json must emit valid JSON");
    assert_eq!(value["spec_dir"], "openspec");
    assert_eq!(value["gitignore_updated"], true);
    assert!(value["config"].as_str().unwrap().ends_with(".spectra.yaml"));
}
