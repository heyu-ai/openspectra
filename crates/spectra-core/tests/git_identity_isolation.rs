//! Identity lookups that require isolating the host's global/system git
//! config, which means mutating process-wide env vars
//! (`GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`).
//!
//! These live in their own integration-test binary on purpose: `cargo test`
//! runs test binaries sequentially, so the env mutation here cannot race
//! another binary's tests that concurrently spawn processes (observed as a
//! flaky `git init` failure when these tests lived in the lib test binary —
//! `setenv` is not thread-safe against a concurrent fork/exec's env copy).
//! Within this binary, `IsolatedGitConfig` serializes itself via a Mutex.

use std::path::Path;
use std::process::Command;

use spectra_core::git::{change_creator_identity, user_identity};

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "spectra-git-identity-it-{label}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(std::fs::canonicalize(dir).unwrap())
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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

/// Serializes every test that mutates `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`
/// within this binary: the parallel test harness would otherwise let two
/// guards interleave their set/restore pairs.
static GIT_CONFIG_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Temporarily points `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` at `/dev/null`
/// (or, via `with_global`, a caller-supplied config file), isolating a git
/// command from whatever global/system identity happens to be configured on
/// the machine running this test (every developer/CI machine plausibly has
/// one) — otherwise the "identity unset" branches below would be
/// unreachable. Holds `GIT_CONFIG_ENV_LOCK` for its whole lifetime.
struct IsolatedGitConfig {
    _serialized: std::sync::MutexGuard<'static, ()>,
    prev_global: Option<String>,
    prev_system: Option<String>,
}

impl IsolatedGitConfig {
    fn new() -> Self {
        Self::with_global(Path::new("/dev/null"))
    }

    fn with_global(global_config: &Path) -> Self {
        let serialized = GIT_CONFIG_ENV_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let prev_global = std::env::var("GIT_CONFIG_GLOBAL").ok();
        let prev_system = std::env::var("GIT_CONFIG_SYSTEM").ok();
        std::env::set_var("GIT_CONFIG_GLOBAL", global_config);
        std::env::set_var("GIT_CONFIG_SYSTEM", "/dev/null");
        Self {
            _serialized: serialized,
            prev_global,
            prev_system,
        }
    }
}

impl Drop for IsolatedGitConfig {
    fn drop(&mut self) {
        match &self.prev_global {
            Some(v) => std::env::set_var("GIT_CONFIG_GLOBAL", v),
            None => std::env::remove_var("GIT_CONFIG_GLOBAL"),
        }
        match &self.prev_system {
            Some(v) => std::env::set_var("GIT_CONFIG_SYSTEM", v),
            None => std::env::remove_var("GIT_CONFIG_SYSTEM"),
        }
    }
}

#[test]
fn user_identity_is_none_when_no_identity_is_configured_anywhere() {
    let dir = TempDir::new("identity-unset");
    init_repo_no_identity(&dir);
    let _isolated = IsolatedGitConfig::new();

    assert_eq!(user_identity(&dir), None);
}

#[test]
fn change_creator_identity_is_unknown_when_keys_are_truly_absent() {
    // Distinct from the empty-string matrix in the lib tests: with no user.*
    // keys at any level, `git config <key>` exits non-zero (absent-key
    // path), not exit-0-with-empty-value. Both paths must land on "unknown".
    let dir = TempDir::new("identity-absent");
    init_repo_no_identity(&dir);
    let _isolated = IsolatedGitConfig::new();

    assert_eq!(change_creator_identity(&dir), "unknown");
}

#[test]
fn change_creator_identity_falls_back_to_user_config_outside_a_repo() {
    // Pins the doc claim on `change_creator_identity`: outside a git repo,
    // `git config <key>` still reads user-level configuration, so
    // `new change` in a bare directory picks up the operator's identity.
    let dir = TempDir::new("identity-non-repo");
    let global = TempDir::new("identity-global-config");
    let config_path = global.join("gitconfig");
    std::fs::write(
        &config_path,
        "[user]\n\tname = Global User\n\temail = global@example.com\n",
    )
    .unwrap();
    let _isolated = IsolatedGitConfig::with_global(&config_path);

    assert_eq!(
        change_creator_identity(&dir),
        "Global User <global@example.com>"
    );
}
