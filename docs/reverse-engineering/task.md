# Reverse-engineering `spectra task start` / `task done`

How the closed-source `spectra task` commands mark a task complete and track
which files it touched, and how OpenSpectra reproduces them.

> **Source.** Oracle `Spectra.app/Contents/MacOS/spectra` **v3.0.0** (it
> links `libgit2-sys 0.18.8+1.9.7`). The golden
> `golden/task-done-3.0.0.json` (#110) plus a targeted probe matrix on
> 2026-09-28 (#190, W4 in `docs/migration-plan.md`) pin everything below.
> The v2.3.1 behavior this file used to describe — session-wide dirty-file
> recording, first-task-wins attribution, a `✓`-less human line, and
> OpenSpectra's own per-change baseline (#98) — is gone: decision D7
> replaced it with the v3.0.0 per-task baseline model.

## CLI shape

```
spectra task start <TASK_ID> [--change <NAME>] [--json]
spectra task done  <TASK_ID> [--change <NAME>] [--json] [--file <PATH>]...
```

* `TASK_ID` is matched **as a string** against the decimal, 1-based index of
  every checkbox in `tasks.md` (file order, ignoring `##` headers and the
  `1.1`-style labels in the task text). `0`, `01`, `+1`, `1.1`, a number past
  the total, `abc`, and anything with surrounding spaces all report
  `Task <arg> not found for change '<change>'` (the argument echoed verbatim).
  `-1` is a clap error (exit 2). A checkbox is any line matching
  `^\s*[-*+]\s*\[(.)\]\s*(.+)$` with a non-blank description — the same rule
  `instructions apply` uses (`tasks::CHECKBOX_RE`, #172).
* `--change` auto-detects when exactly one change is active. With several,
  both commands report `Multiple changes found. Use --change to specify one:
  <a>, <b>` (other commands still use the shared wording; #50).
* A missing change and a change without `tasks.md` both report
  `tasks.md not found for change '<name>'`.

## `task done`

Evaluation order: resolve the change → create the lock → load `tasks.md`
and resolve `TASK_ID` → already-done check → validate every `--file` in
argument order → tracking → write `tasks.md`.

* Already `[x]`/`[X]` → `Task <id> is already done`. Any other non-blank
  marker (`[~]`, `[-]`) succeeds and leaves `tasks.md`'s content unchanged
  (#172); tracking still runs for it.
* Any error before the `tasks.md` write leaves `tasks.md` untouched.
* Tracking, first matching rule wins:
  1. **Not a git repository** (or git unusable): nothing recorded,
     `--file` ignored, warning `git_tracking_unavailable`, `provenance: null`.
     A leftover baseline for this task is deleted.
  2. **`--file` given**: record exactly those paths (normalized, deduped,
     byte-sorted) with `provenance: "explicit_files"`, whether or not they
     exist; this task's baseline, if any, is deleted.
  3. **The task has a baseline** (`task start`): record the paths changed
     since it (rule below) with `provenance: "task_baseline"`. An empty
     result appends no entry and leaves the tracking file untouched.
  4. **Otherwise**: nothing recorded, warning
     `touched_tracking_skipped_no_baseline_or_explicit_files`,
     `provenance: null`, and no tracking file is created.
* Recording appends one entry to `.spectra/touched/<change>.json` (created
  without `review_base` if absent); entries are never merged, even for the
  same task. The tracking file is written **before** `tasks.md`; if the
  `tasks.md` write fails (`Failed to write tasks.md: <io error>`), the
  tracking file is restored and the baseline kept.
* An existing tracking file must parse and belong to this change, otherwise
  exit 1: `Failed to parse touched tracking <abs path>: <serde error>` /
  `Touched tracking belongs to "<other>", expected "<change>"`. A corrupt
  baseline also fails.
* Output: human `✓ Task <id> marked as done: <desc>` on stdout and each
  warning as `! <warning>` on stderr; `--json` is one compact line with
  alphabetical keys
  `{"change","provenance","status":"done","task_desc","task_id","touched_files","warnings"}`.

## `task start`

* Resolves the change and `TASK_ID` exactly like `task done`; on these
  failures **no lock is created** (unlike `task done`).
* Validates an existing tracking file the same way, then creates the lock.
* Baseline already present (even a corrupt one) → nothing is rewritten,
  `baseline_created: false`, human
  `Task <id> already has a baseline; preserving the original`.
* Otherwise writes `.spectra/task-baselines/<change>/<id>.json` with the
  current fingerprints, and — **only if the tracking file does not exist
  yet** — creates it with an empty `touched` list and a `review_base`
  (`head_revision` = HEAD oid or `null` on an unborn branch, plus the same
  fingerprints). An existing tracking file is never given or refreshed a
  `review_base`.
* Outside git: exit 0, no baseline, no tracking file, stdout empty, stderr
  `! Task <id> started without a Git baseline (git_tracking_unavailable)`;
  `git_tracking_available: false` and the warning in `--json`.
* Starting a task that is already done succeeds.
* Output: human `✓ Task <id> baseline captured`; `--json` one compact line
  `{"baseline_created","change","git_tracking_available","status":"started","task_id","warnings"}`.

## Files

All JSON files are serde_json pretty-printed with **no trailing newline**.

* `.spectra/task-baselines/<change>/<id>.json` — keys `change`, `task_id`
  (string), `fingerprints`.
* `.spectra/touched/<change>.json` — keys `change`, `touched`, then
  `review_base` only when present. Entries: `task_id` (string), `task_desc`,
  `files`, `provenance`. OpenSpectra also reads files written by its older
  versions (numeric `task_id`, no `provenance`) and keeps those entries as
  they are.
* `.spectra/touched/<change>.lock` — always 0 bytes, never removed (not even
  by `archive`, which deletes the tracking file and
  `task-baselines/<change>/`).

## Fingerprints

One entry per dirty path, keys `path`, `status`, `index_identity`,
`worktree_identity` (absent identities are `null`, never omitted), sorted by
path bytes (case-sensitive even with `core.ignorecase`). Paths are relative to
the repository root regardless of the working directory. The oracle computes
them with libgit2's status list: untracked files included and untracked
directories expanded, **no rename detection**, submodules excluded, ignored
files excluded. OpenSpectra reproduces the list with `git status
--porcelain=v2 -z --no-renames --untracked-files=all --ignore-submodules=all`
(`fingerprint.rs`).

Excluded paths, compared by path component: the **whole configured spec
directory** (not just the change's own directory) and `.spectra/`
(`docs/spectrafoo/` and `.spectra.yaml` are included). A nested non-submodule
repository appears once, as `inner` (no trailing slash), status 128, both
identities `null`.

* `status` — libgit2 `git_status_t` bits: 1 INDEX_NEW, 2 INDEX_MODIFIED,
  4 INDEX_DELETED, 16 INDEX_TYPECHANGE, 128 WT_NEW, 256 WT_MODIFIED,
  512 WT_DELETED, 1024 WT_TYPECHANGE, 32768 CONFLICTED, OR-ed per path.
  Examples: ` M` 256, `MM` 258, `AD` 513, `git rm --cached` (`D ` plus
  `??`) 132, intent-to-add 257; a staged rename is two entries (4 and 1).
* `index_identity` — the stage-0 index entry as
  `<oid>:<decimal mode>:<on-disk flags>`, where flags =
  `min(path length, 0xFFF) | stage<<12 | 0x4000 (extended: intent-to-add,
  skip-worktree) | 0x8000 (assume-valid)`. It is **not** a size or line count:
  `src/a.rs` gives 8, intent-to-add `src/ita.rs` gives 16394. `null` without
  a stage-0 entry (untracked, staged delete, conflict).
* `worktree_identity` — the blob oid of the raw file bytes
  (`git hash-object --no-filters`, so no CRLF filtering); a symlink is
  `symlink:<oid of the target string>`; `null` for a missing file or a
  directory. The file mode is not part of it.

**"Changed since the baseline"** = every path present on either side whose
`(status, index_identity, worktree_identity)` tuple differs (absence counts
as different). Consequences, all probed: a file dirty before `task start`
and untouched afterwards is not attributed; editing it further, reverting it
to HEAD, committing it, deleting it, or only staging/unstaging it *is*; a
clean file edited and committed between start and done is **not** (HEAD
movement is not diffed); a chmod or touch of an already-dirty file is not;
a `git mv` after start attributes both paths.

## `--file` normalization

Resolved against the project root, not the working directory, lexically:
`./src/a.rs`, `src//a.rs`, `src/../src/a.rs`, and an absolute path inside
the root all become `src/a.rs`; `src/` becomes `src`. No existence check;
directories are accepted; a backslash is kept literally. Errors (exit 1,
`tasks.md` untouched), in the order OpenSpectra checks them:

* `Explicit path '<arg>' is outside the project workspace` (`../x`,
  `/tmp/x`, `.`)
* `Explicit path '<arg>' resolves outside the project workspace` (a symlink
  escaping the root)
* `Explicit path '<arg>' is inside the configured spec directory` (lexical
  only: a symlink into the spec directory is accepted)
* `Explicit path '<arg>' is inside Spectra tracking metadata` (`.spectra/`)

## Verification

`crates/spectra-cli/tests/task_done_golden_integration.rs` replays all 14
scenarios of `golden/task-done-3.0.0.json` (captured by
`scripts/capture-task-done.py`, macOS + oracle 3.0.0) and compares, for each
of the 60 `spectra` steps, exit code, stdout, stderr, `tasks.md` bytes,
`touched/<change>.json` bytes, and the list of files under `.spectra/` —
360 fields, all byte-identical, with no transforms or divergence ledger (the
ledger `task-done-3.0.0.divergences.json` was deleted when D7 removed every
divergence it recorded). A mutation that drops the lock creation turns 42 of
those fields red. Rules the golden does not reach (index flags, `--file`
errors, task-ID edge cases, the not-found rule) are unit-tested in
`fingerprint.rs` and `change.rs`.

**Not determined:** the index flags of paths longer than 0xFFF bytes
(OpenSpectra caps at 0xFFF, as the on-disk format does); the trigger of the
oracle string `Git tracking became unavailable after the task baseline was
captured`; whether the lock is used with `flock` (OpenSpectra only creates
it); Unicode normalization of paths on macOS.

## Resolved discrepancy: `new change`'s scaffold

Running the real binary's `new change <name>` revealed it does **not**
scaffold `proposal.md`/`design.md`/`tasks.md` — only `.openspec.yaml`. Those
three files are created individually and later, via a separate
`spectra new artifact <type> --change <name>` command, with a much richer
templated `tasks.md` (task-group headers, HTML-comment placeholders) than a
static string constant could produce. `.openspec.yaml` also carries a
`created_by` field derived from git identity.

OpenSpectra now matches that split: `change.rs::create` writes only
`.openspec.yaml` in the change directory (plus its OpenSpectra-only baseline
sidecar when a git commit is available), and `new artifact` creates each
artifact later. `task done` handles both flat and grouped/numbered `tasks.md`
files because its checkbox indexing ignores group headers.
