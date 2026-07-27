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

/// Write `contents` to `path` atomically: write to a temp file in the same
/// directory, then rename into place. A same-filesystem `rename` is atomic
/// with respect to concurrent readers and to the process being killed
/// (`SIGKILL`) or hitting a disk-full error after the syscall returns, so
/// `path` is never observed as a partial write that satisfies `path.exists()`
/// while failing to parse. This matters most for `.spectra.yaml`, since
/// [`crate::config::Config::is_initialized`] treats its mere existence as a
/// reliable "scaffolding is complete" signal.
///
/// This does *not* guarantee durability across true power loss (that needs
/// `fsync`-ing the temp file and the parent directory, which this skips as
/// disproportionate for a few bytes of config) -- but a `.spectra.yaml`
/// corrupted by that rarer case is still a clean two-step recovery: delete
/// it and re-run `spectra init` (see `Config::load`'s parse-error hint).
///
/// **If the final path component is a symlink, the rename replaces the link
/// itself with a regular file rather than writing through to its target.** For
/// `init` that was merely incidental; for `update` it is the whole point, and
/// the reason this helper lives here rather than staying private to `init`.
/// `update` writes 445 paths inside a user's project, any of whose *final
/// component* a dotfile manager may have symlinked outside it; a direct
/// `std::fs::write` would follow the link and clobber the target.
/// `artifact.rs` made the same ruling for change dirs (see its
/// `force_write_through_a_symlinked_artifact_path_cannot_escape_the_change_dir`
/// test) -- the reference binary follows the link, and OpenSpectra
/// deliberately does not.
///
/// Only the final component is defended: a symlinked **ancestor** (e.g.
/// `.claude -> ~/dotfiles/claude`) is followed, matching the reference binary,
/// and is recorded as a residual risk in
/// `docs/reverse-engineering/update.md` ("Residual risk: symlinked *ancestor*
/// directories").
///
/// On failure, the temp file is removed on a best-effort basis; if that
/// cleanup itself also fails, the original error is still what's returned,
/// with the cleanup failure logged to stderr rather than silently dropped.
/// The temp file is created with `create_new` (`O_EXCL`), so an attacker who
/// pre-creates the predictable temp path -- including as a **symlink** to a
/// file outside the project -- cannot get us to write through it: `O_EXCL`
/// refuses to open an existing path of any kind, and the write is retried
/// under a fresh name. Without it, `fs::write` would follow such a symlink and
/// truncate the target, defeating the whole point of the rename (PR #86
/// review, Codex).
///
/// When `path` already exists as a regular file, its permission bits are
/// copied onto the replacement before the rename. Otherwise a `0600`
/// `.claude/settings.json` would come back `0644` after an update, exposing
/// the user keys the merge deliberately preserves -- and the reference binary,
/// which writes in place, keeps the original mode. A missing target is created
/// at the reference binary's `0666 & ~umask` (issue #93). A symlinked or
/// otherwise non-regular target keeps the temp file's `0600`: the replacement's
/// content may have been read through the link from a secret-bearing file, and
/// the oracle -- which writes through the link and never creates an entry
/// there -- offers no parity mode for the file that replaces it (PR #100
/// review, consensus).
///
/// Returns [`std::io::Result`] rather than [`anyhow::Result`] on purpose: the
/// `update` command must surface the raw OS error text to stay byte-identical
/// with the reference binary, while `init` wraps it with path context. Adding
/// context here would take that choice away from both callers.
pub(crate) fn write_atomically(path: &Path, contents: &str) -> std::io::Result<()> {
    write_atomically_inner(path, contents, false)
}

/// [`write_atomically`], but falling back to an in-place write when the temp
/// file cannot be created because the **directory** is unwritable.
///
/// Only `update` uses this, and only for oracle parity: creating a temp file
/// needs write permission on the directory, while writing an existing file in
/// place needs it only on the file, so a `0500` project root breaks temp+rename
/// while the reference binary — which writes in place — updates the file and
/// exits 0 (measured, with controls, PR #86 round-2).
///
/// `init` deliberately does **not** get this: it has no oracle string to match
/// here, and `init_gitignore_update_routes_through_write_atomically_not_a_plain_write`
/// exists precisely to stop `.gitignore` regressing to a plain write.
///
/// The fallback still refuses to follow a symlink and still refuses to create a
/// missing file, so neither the security stance nor the "both fail when the
/// entry must be created" parity shape is affected.
pub(crate) fn write_atomically_or_in_place(path: &Path, contents: &str) -> std::io::Result<()> {
    write_atomically_inner(path, contents, true)
}

fn write_atomically_inner(
    path: &Path,
    contents: &str,
    fall_back_in_place: bool,
) -> std::io::Result<()> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let file_name = path.file_name().ok_or_else(|| {
        std::io::Error::new(
            ErrorKind::InvalidInput,
            format!("path {} has no file name", path.display()),
        )
    })?;
    let file_name = file_name.to_string_lossy();

    // O_EXCL means a pre-existing temp path is a hard error rather than a
    // write-through; bounded retries keep an adversarially-recreated path from
    // turning into an infinite loop.
    write_with_retry(path, contents, fall_back_in_place, &mut || {
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        path.with_file_name(format!("{file_name}.tmp-{}-{seq}", std::process::id()))
    })
}

/// The bounded retry loop, with the temp-path source injected.
///
/// `next_tmp` is a parameter rather than an inlined counter read so the retry
/// branch is **deterministically testable**: a test can supply one occupied
/// path followed by a free one. The previous test planted symlinks at
/// `…-{0..11}` and hoped the shared `COUNTER` was still in that window; under
/// `cargo test --all` it never is, so the retry branch went unexercised while
/// the test stayed green — the same vacuity, in the same file, that round 2
/// had already caught once (both external reviewers flagged it again).
fn write_with_retry(
    path: &Path,
    contents: &str,
    fall_back_in_place: bool,
    next_tmp: &mut dyn FnMut() -> std::path::PathBuf,
) -> std::io::Result<()> {
    let mut last_err = None;
    for _ in 0..16 {
        let tmp_path = next_tmp();
        match write_via_temp(path, contents, &tmp_path) {
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                last_err = Some(e);
                continue;
            }
            Err(e) if fall_back_in_place && e.kind() == ErrorKind::PermissionDenied => {
                return write_in_place_fallback(path, contents, e);
            }
            other => return other,
        }
    }
    Err(last_err.unwrap_or_else(|| {
        std::io::Error::new(ErrorKind::AlreadyExists, "temp file name kept colliding")
    }))
}

/// The in-place fallback behind [`write_atomically_or_in_place`]: used **only
/// when the target is an existing regular file**. A symlink still refuses to be
/// followed (that is the whole point of the helper) and an absent target still
/// errors — which is also what the reference binary does when it has to create
/// an entry in an unwritable directory, so parity holds on that shape too.
///
/// Atomicity is lost here. That is not a regression against the reference
/// binary, which never had it; the alternative is failing a run the oracle
/// completes, which breaks the drop-in claim outright.
fn write_in_place_fallback(
    path: &Path,
    contents: &str,
    original: std::io::Error,
) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => std::fs::write(path, contents),
        _ => Err(original),
    }
}

/// One attempt of [`write_atomically`] at a caller-chosen temp path.
///
/// Split out so the `O_EXCL` guarantee is **directly testable**. It previously
/// lived inline, and the regression test had to guess which sequence number the
/// shared `COUNTER` would hand out; under `cargo test --all` the counter is
/// already far past any guessable window, so the collision branch was never
/// reached and the test passed without exercising anything (PR #86 round-2,
/// found by mutation with a control — the lead's own mutation run missed it
/// because it filtered to the single test, where the counter does start at 0).
///
/// Returns [`ErrorKind::AlreadyExists`] iff `tmp_path` was occupied — by a
/// regular file, a directory, or a symlink planted by an attacker.
fn write_via_temp(path: &Path, contents: &str, tmp_path: &Path) -> std::io::Result<()> {
    // Two constraints pull against each other here, and both must hold:
    //
    // 1. The temp file must never be wider than the mode the renamed file
    //    will end up with, and never wider than `0600` before that final mode
    //    is chosen -- `.claude/settings.json` is where people keep tokens. So
    //    it is created `0600`, before any content exists, and only widened
    //    (if at all) to the exact final mode right before the rename. (An
    //    earlier wording claimed it never exceeds `0600` *while holding the
    //    merged content*; that was false -- the widening happens exactly
    //    then -- and PR #100's review flagged the stated-but-false invariant.)
    // 2. The final mode must be exact. For an existing regular target that
    //    means preserving its bits; `OpenOptions::mode` cannot do that because
    //    the process umask silently narrows them (`0644` under `umask 077`
    //    became `0600`, measured in PR #86 round-2). For a new target the
    //    oracle uses the normal file-creation mode, `0666 & ~umask` (issue
    //    #93), not this temp file's deliberately narrow `0600`. In both cases
    //    `File::set_permissions` is `fchmod`, which ignores the umask, so the
    //    selected bits are applied only after the write and before the rename.
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut file = opts.open(tmp_path)?;
    if let Err(e) = std::io::Write::write_all(&mut file, contents.as_bytes()) {
        drop(file);
        cleanup_temp_file(tmp_path);
        return Err(e);
    }
    let mode = match target_mode(path) {
        Ok(TargetMode::Regular(bits)) => Some(bits),
        Ok(TargetMode::Absent) => Some(create_mode(process_umask())),
        // The entry being replaced is a symlink (or other non-regular file).
        // Its own bits describe the link, not the content's confidentiality,
        // and the merged content may have been read *through* the link from a
        // secret-bearing target -- keep the temp file's `0600` instead of
        // treating this as a fresh create (PR #100 mob review, consensus
        // Critical: `create_mode` here turned a symlinked 0600 settings.json
        // into a 0644 regular file holding the same keys).
        Ok(TargetMode::NonRegular) => None,
        // Sibling failure branches all remove the temp file; this one must
        // too, or a temp file holding the fully merged content is silently
        // stranded, violating the cleanup invariant documented above
        // (PR #100 mob review).
        Err(e) => {
            drop(file);
            cleanup_temp_file(tmp_path);
            return Err(e);
        }
    };
    if let Some(mode) = mode {
        let exact = <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(mode);
        if let Err(e) = file.set_permissions(exact) {
            drop(file);
            cleanup_temp_file(tmp_path);
            return Err(e);
        }
    }
    drop(file);
    if let Err(e) = std::fs::rename(tmp_path, path) {
        cleanup_temp_file(tmp_path);
        return Err(e);
    }
    Ok(())
}

/// What currently occupies `target`, distinguished because each case takes a
/// different replacement mode in [`write_via_temp`]: an existing regular
/// file's bits are preserved exactly, an absent target gets the reference
/// binary's `0666 & ~umask` create mode, and a non-regular target (symlink,
/// fifo, socket -- deliberately not followed, since the rename is about to
/// replace the entry itself) keeps the temp file's `0600`. `Absent` and
/// `NonRegular` were previously folded into one `None`, which sent the
/// symlink case down the create path and widened secret-bearing content
/// (PR #100 mob review, consensus Critical).
#[derive(Debug, PartialEq)]
enum TargetMode {
    Regular(u32),
    NonRegular,
    Absent,
}

fn target_mode(target: &Path) -> std::io::Result<TargetMode> {
    match std::fs::symlink_metadata(target) {
        Ok(meta) if meta.file_type().is_file() => Ok(TargetMode::Regular(
            std::os::unix::fs::PermissionsExt::mode(&meta.permissions()),
        )),
        Ok(_) => Ok(TargetMode::NonRegular),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(TargetMode::Absent),
        Err(e) => Err(e),
    }
}

/// The reference binary's mode for a newly created regular file: the standard
/// `open(O_CREAT, 0666)` base filtered by the process umask.
fn create_mode(umask: u32) -> u32 {
    0o666 & !umask
}

/// Read the process umask once, then reuse it for every atomic create.
///
/// POSIX exposes no read-only umask operation: `umask(2)` returns the old value
/// only while replacing it. Restoring it immediately still leaves a
/// process-global set/restore window in which a sibling thread could create a
/// file under the temporary mask. `OnceLock` confines that unavoidable window
/// to the first atomic **create** (the only path that consults the umask)
/// rather than repeating it for every generated file.
fn process_umask() -> u32 {
    static UMASK: std::sync::OnceLock<u32> = std::sync::OnceLock::new();

    *UMASK.get_or_init(read_umask)
}

/// The set-then-restore dance itself, split out from [`process_umask`]'s
/// `OnceLock` so it is **testable**. Caching is what makes the window rare;
/// it is also what makes the read unobservable — once the lock is initialized
/// no test can make it read again, so a test written against `process_umask`
/// can only ever compare the cached value against the ambient mask. On the
/// common `umask 022` box that comparison also passes for a hardcoded `0o022`,
/// i.e. it is vacuous exactly where it matters (verified by mutation: the
/// hardcoded variant kept such a test green).
fn read_umask() -> u32 {
    // SAFETY: `umask` accepts every `mode_t` value and has no pointer or
    // lifetime preconditions. The first call returns the prior mask; the
    // second restores that exact value.
    //
    // `0o777` (not `0o022`) as the transient value: a file another thread
    // creates inside the set/restore window comes out *narrower*, never
    // wider, than intended -- the fail-closed direction (PR #100 mob review,
    // Codex R2 upgrade).
    unsafe {
        let old = libc::umask(0o777);
        libc::umask(old);
        mode_bits(old)
    }
}

/// Widen a [`libc::mode_t`] to the `u32` the rest of this module works in.
/// `mode_t` is `u16` on macOS but already `u32` on Linux, so the conversion is
/// real on one platform and an identity on the other — Linux clippy flags the
/// identity as `useless_conversion`, a platform-dependent false positive
/// confined to this one helper (macOS clippy needs the conversion to compile).
#[allow(clippy::useless_conversion)]
fn mode_bits(mode: libc::mode_t) -> u32 {
    u32::from(mode)
}

/// Best-effort removal of a [`write_via_temp`] temp file after its write or
/// rename step failed. The primary error is always what the caller returns;
/// this only logs (rather than silently dropping) a secondary failure here,
/// so a double-fault doesn't vanish without a trace.
///
/// `NotFound` isn't logged: every call site now runs *after* the `create_new`
/// succeeded, so the temp file did exist and its absence means something else
/// (a concurrent run, a cleaner) removed it underneath us — there is nothing
/// left to remove and the primary error already covers the failure. (The
/// earlier rationale, "the initial `std::fs::write` failed before creating
/// it", described a code path that no longer exists.)
fn cleanup_temp_file(tmp_path: &Path) {
    if let Err(e) = std::fs::remove_file(tmp_path) {
        if e.kind() != ErrorKind::NotFound {
            eprintln!(
                "warning: failed to clean up temp file {}: {e}",
                tmp_path.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// umask 是 process 全域而 `cargo test` 平行執行：所有「修改 umask」或
    /// 「斷言由預設路徑新建之檔案 mode」的測試都必須先取得這把鎖。它曾是
    /// probe 測試內的 function-local static —— 其他測試根本無法命名它，註解
    /// 「日後的測試也要拿這把鎖」的指示因此不可執行（PR #100 mob review，
    /// Gemini；Codex 同項）。
    ///
    /// 前一個測試 panic 導致鎖中毒時取回內部值繼續：這裡保護的是全域 umask，
    /// 不是資料不變量，中毒不代表狀態不可用。
    static UMASK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    // ---- write_atomically (moved here from init.rs: unit tests belong beside
    // the code they cover, and the security-critical one was hiding in a module
    // nobody opens when reviewing this file) ----

    #[test]
    fn newly_created_file_mode_uses_a_0666_base_filtered_by_umask() {
        for (umask, expected) in [
            (0o000, 0o666),
            (0o002, 0o664),
            (0o022, 0o644),
            (0o077, 0o600),
        ] {
            assert_eq!(create_mode(umask), expected, "umask {umask:03o}");
        }
    }

    #[test]
    fn read_umask_reports_the_mask_actually_in_effect_and_restores_it() {
        // `create_mode` 的算術有上面那個純函式測試涵蓋，但它證明不了讀取端：
        // 把 `read_umask` 換成硬編 `0o022`，那個測試照樣全綠，而在 umask
        // 002 / 000 的機器上每個新建檔案的權限都會錯。
        //
        // 這個測試**故意設一個非預設的 mask**（0o057）再讀。第一版沒有這樣做，
        // 只把 `process_umask()` 拿去跟環境當下的 mask 比對 —— 在 umask 022 的
        // 開發機上，硬編 `0o022` 的突變體讓它保持綠燈，也就是說它在唯一需要它
        // 的地方是空的。突變驗證過：現在這一版對同一個突變體會失敗。
        //
        // 為什麼測 `read_umask` 而不是 `process_umask`：後者的 `OnceLock` 只讀
        // 一次，測試無從讓它重讀，所以只能拿快取值跟環境比 —— 正是上面那個空的
        // 比對。
        //
        // umask 是 process 全域，`cargo test` 平行執行，所以拿 module 層級的
        // UMASK_LOCK 序列化（見其 doc）。沒拿鎖的並行測試若恰好在探測窗口內
        // 建檔，`read_umask` 的暫態值是 0o777，建出來的檔案只會更窄不會更寬
        // —— fail-closed（PR #100 mob review，Codex R2）。
        let _guard = UMASK_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        // 先把 `process_umask` 的 OnceLock 釘在環境值上，再安裝探測 mask：
        // 否則某個建檔測試若在 0o057 窗口內首次觸發快取，整個 test binary
        // 之後的 create-mode 斷言都吃到 0o057（PR #100 mob review）。
        let pinned = process_umask();
        assert_eq!(
            pinned,
            read_umask(),
            "the cache must hold the ambient mask before any probe runs"
        );

        const PROBE: u32 = 0o057;
        // SAFETY: 同 `read_umask`。
        let original = unsafe { libc::umask(PROBE as libc::mode_t) };

        let observed = read_umask();

        // 先還原再 assert：assert 失敗會 panic，若還原在後面就會把 0o057 留給
        // 整個 test binary 的其餘部分。
        let left_behind = unsafe { libc::umask(original) };

        assert_eq!(
            observed, PROBE,
            "read_umask must report the mask actually in effect, not a hardcoded value"
        );
        assert_eq!(
            mode_bits(left_behind),
            PROBE,
            "read_umask must leave the process mask exactly as it found it"
        );
    }

    #[test]
    fn write_atomically_writes_full_content_and_leaves_no_temp_file_behind() {
        let tmp = TempDir::new();
        let target = tmp.join("out.txt");

        write_atomically(&target, "hello\n").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "hello\n");
        let entries: Vec<_> = fs::read_dir(&*tmp)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            entries,
            vec!["out.txt".to_string()],
            "no stray <file>.tmp-<pid> should remain after a successful write"
        );
    }

    #[test]
    fn write_atomically_preserves_permission_bits_the_umask_would_have_masked() {
        // 迴歸（PR #86 round-2，Codex 與 agy 各自獨立指出）：`OpenOptions::mode`
        // 會被 process umask 遮罩，所以「用目標的 mode 建立暫存檔」在
        // umask 077 下把 0644 悄悄窄化成 0600（oracle 就地寫入、保留 0644）。
        // 前一版測試只用 0644 以外不會被一般 umask 動到的 0600，因此漏掉。
        //
        // 用 0666：任何常見 umask（022/077）都會遮掉部分位元，所以只有
        // fchmod（不受 umask 影響）才能通過。
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new();
        let target = tmp.join("wide.txt");
        fs::write(&target, "old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o666)).unwrap();

        write_atomically(&target, "new").unwrap();

        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o666,
            "the umask must not narrow bits the target already had"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn write_atomically_preserves_an_existing_files_permission_bits() {
        // 迴歸（PR #86 round-2, Codex）：改用 temp+rename 之後，替換檔是以
        // umask 預設權限新建的，於是一個 0600 的 .claude/settings.json 更新後
        // 變成 0644，把它保留下來的使用者鍵暴露給同機其他使用者。oracle 就地
        // 寫入、保留原 mode。
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new();
        let target = tmp.join("secret.json");
        fs::write(&target, "old").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();

        write_atomically(&target, "new").unwrap();

        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "existing mode must survive the rename");
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn write_via_temp_refuses_a_pre_created_temp_symlink_instead_of_writing_through_it() {
        // 迴歸（PR #86 round-2, Codex）：暫存檔名由 pid + 序號組成，可被推測。
        // 攻擊者若先在該路徑放一個指向外部檔案的 symlink，舊版的 fs::write 會
        // 跟隨它並截斷該外部檔案，rename 提供的保護形同虛設。
        //
        // 這個測試直接呼叫 `write_via_temp` 並指定暫存路徑。前一版改為呼叫
        // `write_atomically` 並「猜」序號 0..8——但 COUNTER 是整個 test binary
        // 共用的 static，在 `cargo test --all`（CI 的跑法）下輪到本測試時序號
        // 早已遠超該範圍，於是 O_EXCL 分支從未被觸發、測試恆綠卻什麼都沒驗到。
        // 由 round-2 reviewer 以「突變 + 對照組」抓出；lead 自己的突變驗證因為
        // 用測試名稱過濾執行（序號從 0 起算）而漏掉。
        let tmp = TempDir::new();
        let target = tmp.join("out.txt");
        let outside = tmp.join("outside-secret.txt");
        fs::write(&outside, "PRECIOUS").unwrap();
        let tmp_path = tmp.join("out.txt.tmp-attacker");
        std::os::unix::fs::symlink(&outside, &tmp_path).unwrap();

        let err = write_via_temp(&target, "new content", &tmp_path).unwrap_err();

        assert_eq!(
            err.kind(),
            ErrorKind::AlreadyExists,
            "an occupied temp path must be refused, not written through"
        );
        assert_eq!(
            fs::read_to_string(&outside).unwrap(),
            "PRECIOUS",
            "the symlink target outside the project must be untouched"
        );
        assert!(!target.exists(), "the target must not have been created");
    }

    #[test]
    fn write_with_retry_moves_past_an_occupied_candidate_to_a_free_one() {
        // 對照：`write_via_temp_refuses_…` 證明「被佔用就拒絕」，這個證明外層
        // 迴圈會換名字重試，所以拒絕不會變成阻斷正常寫入。
        //
        // 候選路徑用注入的，不猜共用 COUNTER——前一版就是靠猜，在
        // `cargo test --all` 下永遠猜不中，retry 分支從未被執行卻恆綠。
        let tmp = TempDir::new();
        let target = tmp.join("out.txt");
        let outside = tmp.join("outside.txt");
        fs::write(&outside, "PRECIOUS").unwrap();

        let occupied = tmp.join("out.txt.tmp-occupied");
        std::os::unix::fs::symlink(&outside, &occupied).unwrap();
        let free = tmp.join("out.txt.tmp-free");

        let mut candidates = vec![free.clone(), occupied.clone()];
        write_with_retry(&target, "new content", false, &mut || {
            candidates.pop().unwrap()
        })
        .unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new content");
        assert_eq!(
            fs::read_to_string(&outside).unwrap(),
            "PRECIOUS",
            "the occupied candidate must have been skipped, not written through"
        );
        assert!(occupied
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn write_with_retry_gives_up_after_the_bounded_number_of_attempts() {
        // 上限存在的理由：對手可以持續重建被佔用的路徑。這裡讓產生器永遠回同
        // 一個被佔用的路徑，證明它會有界地放棄而不是無限迴圈。
        let tmp = TempDir::new();
        let target = tmp.join("out.txt");
        let occupied = tmp.join("out.txt.tmp-always");
        fs::write(&occupied, "squatter").unwrap();

        let mut calls = 0usize;
        let err = write_with_retry(&target, "x", false, &mut || {
            calls += 1;
            occupied.clone()
        })
        .unwrap_err();

        assert_eq!(err.kind(), ErrorKind::AlreadyExists);
        assert_eq!(calls, 16, "the retry bound must be exactly what it claims");
        assert!(!target.exists());
    }

    #[test]
    fn target_mode_distinguishes_regular_absent_and_non_regular() {
        // Regular 的 bits 是「暫存檔以最終權限收尾」機制的輸入；Absent 與
        // NonRegular 必須可區分 —— 兩者曾折疊成同一個 None，讓 symlink 取代
        // 檔走 create 分支（PR #100 mob review，consensus Critical）。
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new();
        let regular = tmp.join("regular");
        fs::write(&regular, "x").unwrap();
        fs::set_permissions(&regular, fs::Permissions::from_mode(0o600)).unwrap();
        match target_mode(&regular).unwrap() {
            TargetMode::Regular(m) => assert_eq!(m & 0o777, 0o600),
            other => panic!("expected Regular for a regular file, got {other:?}"),
        }

        assert_eq!(
            target_mode(&tmp.join("absent")).unwrap(),
            TargetMode::Absent
        );

        let link = tmp.join("link");
        std::os::unix::fs::symlink(&regular, &link).unwrap();
        assert_eq!(
            target_mode(&link).unwrap(),
            TargetMode::NonRegular,
            "a symlink must not be followed -- the rename replaces the link itself"
        );
    }

    #[test]
    fn write_atomically_creates_a_missing_target_at_0666_filtered_by_the_umask() {
        // AC-1 的端到端串接：三聲部 mob review 一致指出 `create_mode` 的算術與
        // `read_umask` 各自有測試，但沒有任何測試觀察 `write_atomically` 實際
        // 建出的檔案 mode —— 把串接改回 `.unwrap_or(0o600)`（= 完整回退 #93 的
        // 修復）當時所有測試仍綠。這裡直接斷言建檔結果。
        //
        // 拿鎖：斷言依賴 `process_umask` 快取值與環境一致。已接受的殘餘盲點：
        // (a) 快取層被硬編 0o022 時，本測試在 umask 022 機器上無法區分 ——
        //     讀取端硬編由 probe 測試的 0o057 殺掉，快取層只剩一行
        //     `get_or_init(read_umask)`，突變面極小；
        // (b) 在 umask 077 的機器上 expected == 0o600，本斷言退化為恆真 ——
        //     CI runner 是 022（expected 0o644 ≠ 0o600，殺掉回退突變體）。
        use std::os::unix::fs::PermissionsExt;
        let _guard = UMASK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = TempDir::new();
        let target = tmp.join("fresh.txt");

        write_atomically(&target, "new").unwrap();

        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode,
            create_mode(process_umask()),
            "a freshly created file must land at 0666 & ~umask, not the temp file's 0600"
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn write_atomically_keeps_0600_when_replacing_a_symlinked_target() {
        // 迴歸（PR #100 mob review，consensus Critical）：Absent 與 NonRegular
        // 曾折疊成同一個 None，取代 symlink 的 regular 檔因此走 create 分支以
        // 0666 & ~umask 落地 —— 而其內容可能是透過 link 從 0600 的
        // secret-bearing 目標讀進來的。斷言取代檔保留暫存檔的 0600。
        // 不需要 UMASK_LOCK：NonRegular 分支不消費 umask；若突變讓它走 create
        // 分支，無論窗口內外 mode 都不會是 0600（0o777 暫態只會更窄到 0）。
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new();
        let outside = tmp.join("dotfiles-secret.json");
        fs::write(&outside, "{\"token\":\"PRECIOUS\"}").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
        let target = tmp.join("settings.json");
        std::os::unix::fs::symlink(&outside, &target).unwrap();

        write_atomically(&target, "{\"token\":\"PRECIOUS\",\"merged\":true}").unwrap();

        let meta = fs::symlink_metadata(&target).unwrap();
        assert!(
            meta.file_type().is_file(),
            "the rename must replace the link with a regular file"
        );
        assert_eq!(
            meta.permissions().mode() & 0o777,
            0o600,
            "the file replacing a symlink holds content read through the link -- it must stay 0600"
        );
        assert_eq!(
            fs::read_to_string(&outside).unwrap(),
            "{\"token\":\"PRECIOUS\"}",
            "the link target itself must be untouched"
        );
    }

    #[test]
    fn write_atomically_falls_back_to_an_in_place_write_when_the_directory_is_unwritable() {
        // 迴歸（PR #86 round-2）：建立暫存檔需要**目錄**寫入權，就地寫入只需要
        // **檔案**寫入權。0500 的專案根加上既存可寫的 CLAUDE.md，oracle 更新成功
        // 並 exit 0，而 temp+rename 會 exit 1——實測確認並附對照組。
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new();
        let dir = tmp.join("locked");
        fs::create_dir(&dir).unwrap();
        let target = dir.join("out.txt");
        fs::write(&target, "old").unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();

        let result = write_atomically_or_in_place(&target, "new");

        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        result.unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "new");
    }

    #[test]
    fn the_unwritable_directory_fallback_still_refuses_to_follow_a_symlink() {
        // fallback 不得成為繞過 symlink 保護的後門：目標是 symlink 時照樣失敗。
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new();
        let outside = tmp.join("outside.txt");
        fs::write(&outside, "PRECIOUS").unwrap();
        let dir = tmp.join("locked");
        fs::create_dir(&dir).unwrap();
        let target = dir.join("out.txt");
        std::os::unix::fs::symlink(&outside, &target).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();

        let result = write_atomically_or_in_place(&target, "new");

        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            result.is_err(),
            "a symlinked target must not take the fallback"
        );
        assert_eq!(fs::read_to_string(&outside).unwrap(), "PRECIOUS");
    }

    #[test]
    fn write_atomically_leaves_the_target_untouched_when_the_write_fails() {
        let tmp = TempDir::new();
        // A nonexistent parent directory makes the temp-file write itself
        // fail before any rename is attempted -- this covers "write fails
        // before touching the target", not a torn/interrupted write (which
        // needs fault injection this test doesn't attempt).
        let target = tmp.join("nonexistent-dir").join("out.txt");

        write_atomically(&target, "hello").unwrap_err();

        assert!(!target.exists());
    }

    #[test]
    fn write_atomically_cleans_up_the_temp_file_when_rename_fails() {
        let tmp = TempDir::new();
        let target = tmp.join("out.txt");
        // A pre-existing directory at the target path lets the temp-file
        // write succeed (it's a different filename) but makes the rename
        // fail (renaming a file onto an existing directory is rejected),
        // exercising the cleanup-on-rename-failure branch specifically.
        fs::create_dir_all(&target).unwrap();

        write_atomically(&target, "hello").unwrap_err();

        let entries: Vec<_> = fs::read_dir(&*tmp)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            entries,
            vec!["out.txt".to_string()],
            "no stray out.txt.tmp-<pid>-<seq> should remain after a failed rename, got: {entries:?}"
        );
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
