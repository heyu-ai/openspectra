//! Small filesystem helpers shared across core modules.
//!
//! Extracted so a single hardening (e.g. the NotFound-only collapse in
//! [`read_optional`]) can't drift between two byte-identical copies — the
//! situation the `archive`/`validate` mob review flagged.

use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result};

/// Read `path` as UTF-8 text: `Ok(None)` when it's genuinely absent, `Err`
/// for any other I/O failure (permission denied, invalid UTF-8, etc.).
/// Callers must not fold a real read failure into "doesn't exist yet" —
/// doing so before a subsequent write would silently clobber unreadable
/// content.
pub(crate) fn read_optional(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// `Ok(None)` when `path` is genuinely absent; `Err` for any other failure
/// (mirrors [`read_optional`]'s NotFound-only collapse). Shared by the
/// `archive` merge walk and the `validate` recursive spec walk.
pub(crate) fn read_dir_optional(path: &Path) -> Result<Option<std::fs::ReadDir>> {
    match std::fs::read_dir(path) {
        Ok(entries) => Ok(Some(entries)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}
