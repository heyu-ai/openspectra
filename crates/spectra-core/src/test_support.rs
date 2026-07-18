//! Shared test-only helpers for this crate's unit tests.
//!
//! Factored out of per-module copies (this crate had accumulated one
//! hand-copied `TempDir` per `#[cfg(test)]` module); new test modules should
//! use this instead of pasting another copy. Integration tests (`tests/`)
//! cannot see `#[cfg(test)]` lib modules and keep their own copy under
//! `tests/common/`.

use std::path::Path;

/// A process-unique temp directory that removes itself on drop.
pub(crate) struct TempDir(std::path::PathBuf);

impl TempDir {
    pub(crate) fn new(label: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("spectra-test-{label}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
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
