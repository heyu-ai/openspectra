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
/// `<Epic>/<Feature>` layout). Returned sorted by capability id for
/// deterministic ordering.
///
/// A missing `specs_root` yields an empty vec — the caller decides what "no
/// deltas" means (a validation failure, or nothing to archive). Descent uses
/// `DirEntry::file_type` (which does not follow symlinks) so a checked-in
/// directory-symlink cycle (`specs/loop -> specs`) can't recurse without bound
/// and crash the caller. A capability directory name that isn't valid UTF-8 is a hard error
/// rather than a lossy conversion: `archive` turns this id into a *write*
/// target (`specs/<cap>/spec.md`), so a silent `U+FFFD` substitution would
/// merge into the wrong path.
///
/// Two malformed layouts fail loud rather than mis-writing or vanishing:
/// - A `spec.md` sitting **directly** under `specs_root` (no capability
///   directory) is a hard error — its capability id would be empty, and
///   `archive` would otherwise write a nameless `specs/spec.md` with a
///   `#  Specification` header.
/// - A capability directory that is a **symlink** is not descended (the
///   cycle guard bounds the walk to the real tree), so its delta is skipped;
///   because old `archive` followed such symlinks, the skip is announced on
///   stderr rather than dropped silently. Following symlinked capability dirs
///   (with visited-set cycle tracking) is a deliberate non-goal here.
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
        // `DirEntry::file_type` does not follow symlinks (like `symlink_metadata`)
        // and is usually served from the `readdir` `d_type` without an extra
        // stat. Following a directory symlink is deliberately avoided: the walk
        // recurses, so a checked-in cycle (`specs/loop -> specs`, or two dirs
        // pointing at each other) would recurse without bound -> stack overflow,
        // crashing the caller. A symlinked capability dir is therefore skipped --
        // but announced on stderr (old `archive` followed it via `fs::metadata`),
        // never dropped silently.
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            // An entry that vanished between `read_dir` and here (a concurrent
            // remove) is treated as absent and skipped, matching the
            // NotFound-tolerance of `read_optional`/`read_dir_optional` and the
            // removed `is_real_dir` -- not a reason to abort the whole walk.
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        if file_type.is_dir() {
            collect_delta_specs_into(specs_root, &path, out)?;
        } else if file_type.is_symlink() && symlink_target_is_dir(&path)? {
            eprintln!(
                "warning: skipping symlinked capability directory {} -- its spec \
                 delta will not be collected (symlinked directories are not \
                 traversed, to bound the walk against cycles)",
                path.display()
            );
        }
    }
    Ok(())
}

/// The `/`-joined capability id for `dir` relative to `specs_root`, erroring
/// on any path component that isn't valid UTF-8, and on an **empty** id — a
/// `spec.md` directly under `specs_root`, which names no capability (see
/// [`collect_delta_specs`]).
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
    if parts.is_empty() {
        return Err(anyhow!(
            "found spec.md directly under {} -- a delta must live under a capability \
             directory (specs/<capability>/spec.md)",
            specs_root.display()
        ));
    }
    Ok(parts.join("/"))
}

/// Whether a symlink's target is a directory — the case
/// [`collect_delta_specs_into`] warns about before skipping. The caller has
/// already confirmed `path` is a symlink (via `DirEntry::file_type`), so this
/// only follows the link: a dangling target errors `NotFound` -> `false`, so a
/// broken link is skipped quietly (there's no capability delta behind it to
/// lose). A non-`NotFound` error (permission denied, or an `ELOOP` symlink
/// loop) is surfaced rather than swallowed, matching the walk's fail-loud
/// stance — a `spec.md` that is itself a symlink is still read; only directory
/// *descent* stops following links.
fn symlink_target_is_dir(path: &Path) -> Result<bool> {
    match std::fs::metadata(path) {
        Ok(m) => Ok(m.is_dir()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "spectra-fsutil-test-{}-{seq}-{}",
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

    fn write_spec(specs_root: &Path, cap_path: &str, content: &str) {
        let mut dir = specs_root.to_path_buf();
        for part in cap_path.split('/').filter(|p| !p.is_empty()) {
            dir = dir.join(part);
        }
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("spec.md"), content).unwrap();
    }

    #[test]
    fn missing_specs_root_yields_empty() {
        let tmp = TempDir::new();
        let out = collect_delta_specs(&tmp.join("does-not-exist")).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn collects_a_mixed_flat_and_nested_layout_sorted_by_capability_id() {
        // A parent capability with its own spec.md *and* a nested sub-capability:
        // both must be collected (the walk records a dir's spec.md and still
        // descends), and the result must be deterministically sorted so the
        // `apply_spec_deltas` sort could be dropped. Sort is byte-wise, so the
        // uppercase-`B` capabilities precede the lowercase `auth`.
        let tmp = TempDir::new();
        let specs_root = tmp.join("specs");
        write_spec(&specs_root, "auth", "auth delta");
        write_spec(&specs_root, "Billing", "billing delta");
        write_spec(&specs_root, "Billing/Invoices", "invoices delta");

        let out = collect_delta_specs(&specs_root).unwrap();

        let caps: Vec<&str> = out.iter().map(|(c, _)| c.as_str()).collect();
        assert_eq!(caps, ["Billing", "Billing/Invoices", "auth"]);
        // Content travels with its own capability, not another's.
        assert_eq!(out[1].1, "invoices delta");
    }

    #[test]
    fn errors_on_a_spec_md_directly_under_specs_root() {
        // A stray `specs/spec.md` names no capability; collecting it with an
        // empty id would make `archive` write a nameless `specs/spec.md`. It
        // must fail loud instead.
        let tmp = TempDir::new();
        let specs_root = tmp.join("specs");
        fs::create_dir_all(&specs_root).unwrap();
        fs::write(specs_root.join("spec.md"), "orphan delta").unwrap();

        let err = collect_delta_specs(&specs_root).unwrap_err();
        assert!(
            err.to_string().contains("directly under"),
            "expected an orphan-spec error, got: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn skips_a_symlinked_capability_directory() {
        // A capability dir that is a directory symlink is not descended (cycle
        // guard) -- so `alias`'s delta is skipped and only the real `auth`
        // capability is collected. (Old `archive` followed such symlinks; the
        // collector now warns on stderr, asserted here only by the skip.)
        let tmp = TempDir::new();
        let specs_root = tmp.join("specs");
        write_spec(&specs_root, "auth", "auth delta");
        let target = tmp.join("elsewhere");
        write_spec(&target, "", "aliased delta");
        std::os::unix::fs::symlink(&target, specs_root.join("alias")).unwrap();

        let out = collect_delta_specs(&specs_root).unwrap();

        let caps: Vec<&str> = out.iter().map(|(c, _)| c.as_str()).collect();
        assert_eq!(
            caps,
            ["auth"],
            "the symlinked capability dir must be skipped"
        );
    }
}
