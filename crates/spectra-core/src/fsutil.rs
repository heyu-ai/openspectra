//! Small filesystem helpers shared across core modules.
//!
//! Extracted so a single hardening (e.g. the NotFound-only collapse in
//! [`read_optional`]) can't drift between two byte-identical copies — the
//! situation the `archive`/`validate` mob review flagged.

use std::io::ErrorKind;
use std::path::Path;

use anyhow::{anyhow, Context, Result};

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

/// Recursively collect every `spec.md` beneath `specs_root`, paired with its
/// capability id: the `/`-joined path from `specs_root` down to the file's
/// parent directory (e.g. `auth`, or `Billing/Invoices` for a nested
/// `<Epic>/<Feature>` layout; the empty string for a stray `specs/spec.md`).
/// Returned sorted by capability id for deterministic ordering.
///
/// A missing `specs_root` yields an empty vec — the caller decides what "no
/// deltas" means (a validation failure, or nothing to archive). Descent uses
/// [`is_real_dir`] (via `symlink_metadata`) so a checked-in directory-symlink
/// cycle (`specs/loop -> specs`) can't recurse without bound and crash the
/// caller. A capability directory name that isn't valid UTF-8 is a hard error
/// rather than a lossy conversion: `archive` turns this id into a *write*
/// target (`specs/<cap>/spec.md`), so a silent `U+FFFD` substitution would
/// merge into the wrong path.
///
/// Shared by the `archive` merge walk and the `validate` structural walk so
/// their traversal (and its symlink-cycle safety) can't drift apart — the
/// asymmetry that let a nested delta validate cleanly yet archive silently
/// (issue #39).
pub(crate) fn collect_delta_specs(specs_root: &Path) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    collect_delta_specs_into(specs_root, specs_root, &mut out)?;
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn collect_delta_specs_into(
    specs_root: &Path,
    dir: &Path,
    out: &mut Vec<(String, String)>,
) -> Result<()> {
    let Some(entries) = read_dir_optional(dir)? else {
        return Ok(());
    };

    // A `spec.md` directly at this level is one capability's delta. Record it
    // whether or not the dir also has subdirectories, so mixed flat/nested
    // layouts (a capability spec alongside sub-capability dirs) are all seen.
    if let Some(content) = read_optional(&dir.join("spec.md"))? {
        out.push((capability_id(specs_root, dir)?, content));
    }

    for entry in entries {
        let entry = entry.with_context(|| format!("reading {}", dir.display()))?;
        let path = entry.path();
        if is_real_dir(&path)? {
            collect_delta_specs_into(specs_root, &path, out)?;
        }
    }
    Ok(())
}

/// The `/`-joined capability id for `dir` relative to `specs_root`, erroring
/// on any path component that isn't valid UTF-8 (see [`collect_delta_specs`]).
fn capability_id(specs_root: &Path, dir: &Path) -> Result<String> {
    let rel = dir.strip_prefix(specs_root).unwrap_or(dir);
    let mut parts = Vec::new();
    for component in rel.components() {
        let raw = component.as_os_str();
        let part = raw
            .to_str()
            .ok_or_else(|| anyhow!("capability directory name {raw:?} is not valid UTF-8"))?;
        parts.push(part);
    }
    Ok(parts.join("/"))
}

/// Whether `path` is a real directory to descend into, via `symlink_metadata`
/// (which does **not** follow symlinks) rather than `fs::metadata` (which
/// does). This is load-bearing, not cosmetic: the walk in
/// [`collect_delta_specs`] recurses, so following a directory symlink lets a
/// checked-in cycle (`specs/loop -> .`, or two dirs pointing at each other)
/// recurse without bound -> stack overflow, crashing the caller instead of
/// erroring cleanly. Not following symlinked subdirectories bounds traversal
/// to the real tree. `symlink_metadata` (like `fs::metadata`) still surfaces a
/// permission error instead of silently returning `false` the way
/// `Path::is_dir` would. A `spec.md` that is itself a symlink is still read —
/// only directory *descent* stops following links.
fn is_real_dir(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(m) => Ok(m.is_dir()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}
