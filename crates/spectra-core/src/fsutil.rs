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
/// which writes in place, keeps the original mode.
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
    let mut last_err = None;
    for _ in 0..16 {
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp_path = path.with_file_name(format!("{file_name}.tmp-{}-{seq}", std::process::id()));
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
    // The target's mode is applied **at creation**, not chmod'd afterwards.
    // Fixing it up after the write left a window in which the fully merged
    // content -- including every user key the merge exists to preserve, and
    // `.claude/settings.json` is where people keep tokens -- sat on disk
    // group/world-readable at a guessable name (PR #86 round-2).
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    if let Some(mode) = existing_file_mode(path)? {
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, mode);
    }
    let mut file = opts.open(tmp_path)?;
    if let Err(e) = std::io::Write::write_all(&mut file, contents.as_bytes()) {
        drop(file);
        cleanup_temp_file(tmp_path);
        return Err(e);
    }
    drop(file);
    if let Err(e) = std::fs::rename(tmp_path, path) {
        cleanup_temp_file(tmp_path);
        return Err(e);
    }
    Ok(())
}

/// `target`'s permission bits when it is an existing regular file, so the
/// replacement can be created with them. `None` for a missing target (the
/// create case: keep the umask default) and for a symlink, which is deliberately
/// not followed since the rename is about to replace the link itself.
fn existing_file_mode(target: &Path) -> std::io::Result<Option<u32>> {
    match std::fs::symlink_metadata(target) {
        Ok(meta) if meta.file_type().is_file() => Ok(Some(
            std::os::unix::fs::PermissionsExt::mode(&meta.permissions()),
        )),
        Ok(_) => Ok(None),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
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
    fn write_atomically_retries_past_an_occupied_temp_name_and_still_succeeds() {
        // 對照：上一個測試證明「被佔用就拒絕」，這個證明外層迴圈會換名字重試，
        // 所以拒絕不會變成阻斷正常寫入。
        let tmp = TempDir::new();
        let target = tmp.join("out.txt");
        let outside = tmp.join("outside.txt");
        fs::write(&outside, "PRECIOUS").unwrap();
        // 佔用接下來 12 個候選名（上限 16，故仍有餘裕成功）。
        let pid = std::process::id();
        let probe = tmp.join("probe.txt");
        write_atomically(&probe, "x").unwrap(); // 讓 COUNTER 前進到已知起點附近
        let start = (0..u64::MAX)
            .find(|seq| !tmp.join(format!("out.txt.tmp-{pid}-{seq}")).exists())
            .unwrap();
        for seq in start..start + 12 {
            std::os::unix::fs::symlink(&outside, tmp.join(format!("out.txt.tmp-{pid}-{seq}")))
                .unwrap();
        }

        write_atomically(&target, "new content").unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new content");
        assert_eq!(fs::read_to_string(&outside).unwrap(), "PRECIOUS");
    }

    #[test]
    fn existing_file_mode_reports_a_regular_files_bits_and_nothing_else() {
        // 這是「暫存檔一開始就用最終權限建立」的機制本身。先前是寫完再 chmod，
        // 中間有一段合併後內容以 0644 落地的視窗（PR #86 round-2）。
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new();
        let regular = tmp.join("regular");
        fs::write(&regular, "x").unwrap();
        fs::set_permissions(&regular, fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            existing_file_mode(&regular).unwrap().map(|m| m & 0o777),
            Some(0o600)
        );

        assert_eq!(existing_file_mode(&tmp.join("absent")).unwrap(), None);

        let link = tmp.join("link");
        std::os::unix::fs::symlink(&regular, &link).unwrap();
        assert_eq!(
            existing_file_mode(&link).unwrap(),
            None,
            "a symlink must not be followed -- the rename replaces the link itself"
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
