//! OpenSpectra core: change discovery, capability spec discovery, and drift
//! detection, reverse-engineered from the closed-source `spectra` CLI (v2.3.1).

pub mod anchors;
pub mod archive;
pub mod calibration;
pub mod change;
pub mod config;
pub mod drift;
pub mod git;
pub mod init;
mod names;
pub mod spec;
pub mod tasks;
pub mod touched;

pub use change::{Change, ChangeMetadata};
pub use config::Config;
pub use drift::{analyze, DriftReport};
pub use init::{init, InitOutcome};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Test-only helpers shared across module test suites.
#[cfg(all(test, unix))]
pub(crate) mod testutil {
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    /// Whether the current process + filesystem actually enforce a `0o000`
    /// file mode against reads.
    ///
    /// Under `root` (euid 0) the DAC read check is bypassed, and some
    /// container/overlay filesystems ignore mode bits entirely — in both
    /// cases a test that relies on a permission-denied read to force an I/O
    /// error would spuriously *pass its read* and fail its assertion. Callers
    /// skip such tests (printing why) when this returns `false`.
    ///
    /// Implemented as a direct probe rather than a `geteuid() == 0` check so
    /// it also covers the permission-ignoring-filesystem case, and so it needs
    /// no `libc` dependency.
    pub(crate) fn permissions_enforced() -> bool {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path: PathBuf =
            std::env::temp_dir().join(format!("spectra-perm-probe-{}-{seq}", std::process::id()));
        std::fs::write(&path, b"probe").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let denied = std::fs::read(&path)
            .err()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied);
        // Restore a readable mode before removal so cleanup can't itself be
        // blocked by the 0o000 we just set.
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644));
        let _ = std::fs::remove_file(&path);
        denied
    }
}
