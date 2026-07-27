//! AC-1 end-to-end across the contract's four umasks (PR #100 re-review,
//! Codex): the in-process unit test in `fsutil.rs` can only observe the
//! ambient umask -- `process_umask`'s `OnceLock` reads once per process --
//! so each umask value gets a **fresh subprocess**: the parent test re-runs
//! this same test binary filtered down to the child test, with the desired
//! umask passed through an env var. This also kills the one mutant the
//! in-process test documents as its accepted blind spot (a cache layer
//! hardcoded to `0o022` passes on an umask-022 host; it fails here at
//! `000`/`002`/`077`).

use std::path::PathBuf;

const CHILD_ENV: &str = "SPECTRA_UMASK_E2E_CHILD";

/// RAII scratch directory (same shape as `init_integration.rs`).
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "spectra-umask-it-{label}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl std::ops::Deref for TempDir {
    type Target = std::path::Path;
    fn deref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Child half: a no-op unless spawned by the parent below with `CHILD_ENV`
/// set. Sets the requested umask **before** any file creation, so the fresh
/// process's `OnceLock` reads exactly this value, then asserts the mode of a
/// file `init` created through the atomic write path.
#[test]
fn child_asserts_init_created_file_mode_under_env_umask() {
    let Ok(raw) = std::env::var(CHILD_ENV) else {
        return;
    };
    let umask = u32::from_str_radix(&raw, 8).unwrap();
    // SAFETY: `umask` accepts every `mode_t` value and has no pointer or
    // lifetime preconditions. This is a single-purpose child process and the
    // mask is set before any file is created; no restore is needed.
    unsafe { libc::umask(umask as libc::mode_t) };

    let dir = TempDir::new("child");
    spectra_core::init::init(&dir).unwrap();

    use std::os::unix::fs::PermissionsExt;
    for file in [".spectra.yaml", ".gitignore"] {
        let mode = std::fs::metadata(dir.join(file))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode,
            0o666 & !umask,
            "{file} created under umask {umask:03o}"
        );
    }
}

/// Parent half: one subprocess per contract umask (AC-1 enumerates
/// 000/002/022/077), each with a fresh `OnceLock`.
#[test]
fn init_creates_files_at_0666_filtered_by_each_contract_umask() {
    for umask in [0o000u32, 0o002, 0o022, 0o077] {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "child_asserts_init_created_file_mode_under_env_umask",
                "--nocapture",
            ])
            .env(CHILD_ENV, format!("{umask:o}"))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "child under umask {umask:03o} failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
