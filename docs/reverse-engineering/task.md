# Reverse-engineering `spectra task done`

How the closed-source `spectra task done` command marks a task complete and
tracks which files it touched, and how OpenSpectra reproduces it.

> Source: `Spectra.app/Contents/MacOS/spectra` v2.3.1 (arm64 Mach-O, symbols
> retained). String mining located the CLI help text and error-message
> fragments; the exact behavior (checkbox indexing, touched-file selection,
> JSON shapes) was confirmed by running the binary as a **golden oracle** in
> scratch git repos, since none of it was covered by `drift.md`'s earlier RE
> pass.
>
> **Oracle v3.0.0 redesigned `task done`.** OpenSpectra still implements the
> v2.3.1 behavior described in "CLI shape" and "Behavior" below; how v3.0.0
> differs, and which parts still match byte-for-byte, is pinned by the golden
> described in [Oracle 3.0.0 golden and known
> divergences](#oracle-300-golden-and-known-divergences) (#110).

## CLI shape

`task` is a nested subcommand family (`spectra task <COMMAND>`), of which
`done` is currently the only member:

```
spectra task done <TASK_ID> [--change <NAME>] [--json]
```

* `TASK_ID` — 1-based index across **every** checkbox in `tasks.md`, counted
  top-to-bottom in file order, ignoring any `## N.` group headers. A
  checkbox is any line matching `^\s*[-*+]\s*\[(.)\]\s*(.+)$` with a
  non-blank description — the same rule `instructions apply` uses (the code
  shares one regex, `tasks::CHECKBOX_RE`). Oracle v3.0.0 probe (#172):
  `* [ ]`, `+ [ ]`, `- [~]`, `- [-]` are tasks (pending); ordered-list
  `1. [ ]` / `1) [ ]`, `- [ x]`, and `- []` are not. Before #172 OpenSpectra
  only recognized `- [ ]`/`- [x]`/`- [X]`, so a `*`/`+` task was invisible
  to `list`, `task done`, `drift`, `archive --mark-tasks-complete`, and
  `validate`'s archived-task check while `instructions apply` still counted
  it — the two numberings diverged. (`archive --mark-tasks-complete` is the
  one consumer that does not require a non-blank description: oracle v3.0.0
  flips `- [ ]` and `- [ ] ` too, see `archive.md`/`tasks::mark_all_done`.) A
  `tasks.md` with two `##` groups of two tasks each numbers its checkboxes
  1–4 regardless of the `1.1`/`1.2`/`2.1`/`2.2` labels written in the task
  text itself — those labels are just prose, not the identifier `task done`
  operates on.
* `--change` — same auto-detect semantics as `drift`/`show`/`park`: omit it
  when exactly one active change exists, otherwise it's required.

## Behavior

OpenSpectra's actual evaluation order. The v3.0.0 golden shows the oracle
uses a **different** order — it resolves the change and loads `tasks.md`
before looking at `TASK_ID` at all (`task done abc --change nope` reports
`tasks.md not found for change 'nope'`) — and reports every bad ID with one
message; see the `evaluation-order` and `error-message` divergence classes.
The steps below are OpenSpectra's own:

0. Parse `TASK_ID` as an unsigned integer *before* looking at the change at
   all → `Invalid task ID '<input>': must be a number` on failure.
1. Resolve the change (auto-detect or `--change`; `--change`'s own
   auto-detect errors, e.g. "No active changes...", are reachable here same
   as `drift`/`show`/`park`). If the change doesn't exist, or exists but has
   no `tasks.md`, both report the **same** error:
   `tasks.md not found for change '<name>'` — the oracle doesn't distinguish
   the two cases.
2. Validate the remaining `TASK_ID` cases (inside `tasks::mark_done`, after
   the change/`tasks.md` load above):
   * `0` → `Task ID must be >= 1`
   * greater than the total checkbox count → `Task <id> not found (total: <n>)`
   * already `[x]` or `[X]` → `Task <id> is already done`
   * any other non-blank marker (`[~]`, `[-]`) → **success** with the
     content of `tasks.md` unchanged: oracle v3.0.0 exits 0 with
     `status: "done"` and leaves the content as-is (probed in #172; the probe
     compared content, not mtime). OpenSpectra reproduces this; steps 3–4
     still run (step 3 writes the unchanged content back). Step 4 attributing
     files to a task that stays pending is oracle behavior too: the #177
     review probed `- [~] 1.2 b` + `task done 2 --file src/x.rs` on v3.0.0 →
     `.spectra/touched/<name>.json` records `src/x.rs` under task 2
     (`provenance: explicit_files`).
3. Flip that checkbox from `[ ]` to `[x]`, rewriting `tasks.md` with every
   other line's content preserved verbatim. (LF line endings; a CRLF
   `tasks.md` is normalized to LF as a side effect of the line-based
   rewrite — not literally byte-for-byte for that input.)
4. Best-effort record newly-dirty files to `.spectra/touched/<name>.json`:
   * "touched files" = `git status --porcelain` output (modified, staged,
     and untracked paths; a rename reports the new path)…
   * … **minus** anything under the change's own artifact directory
     (`<spec_dir>/changes/<name>/` — `tasks.md` itself is always dirty right
     after step 3, and must never show up as a "touched" file) …
   * … **minus** anything under OpenSpectra's own `.spectra/` state directory
     (see the dedicated section below) …
   * … **minus** (OpenSpectra-only, #98) anything whose content fingerprint
     is unchanged since the previous checkpoint —
     `.spectra/changes/<name>.touched-baseline.json`, written by
     `new change` and rewritten at the end of every `task done` whose
     recording succeeds (a failed recording keeps the old checkpoint so a
     later task still picks those files up). A file that was already dirty
     before the change started and that no task edited is therefore not
     attributed to the change; one a task reverted to its committed content
     *is*, since it is checkpointed-but-now-clean with a changed fingerprint.
     Paths whose state cannot be determined are checkpointed with a `null`
     fingerprint and always count as changed (so a pre-dirty submodule is
     attributed to the first task; see `archive.md` for this known
     limitation). With no
     baseline (a change created before this divergence), or an
     unreadable/corrupt one (which warns), this filter is skipped and the
     v2.3.1 oracle's session-wide behavior applies. See `archive.md` ›
     "Deliberate divergences (#98)" …
   * … **minus** anything already recorded against an *earlier* task in this
     change's tracking file (confirmed empirically on v2.3.1: a file that's
     still dirty after being recorded under task 1 is *not* re-attributed to
     task 2 — attribution is first-task-wins, scoped per change). v3.0.0
     dropped this: with per-task baselines, a file edited again during task 2
     is recorded under both tasks (golden scenario `task-start-baseline`).
   * If the resulting file list is empty, no tracking file is written at
     all (confirmed: marking a task done with zero unrelated dirty files
     never creates `.spectra/`).
5. Print `✓ Task <id> marked as done: <task_desc>` (human, oracle-verified)
   or `{"change","status","task_desc","task_id"}` (`--json`, alphabetical
   key order, **`task_id` rendered as a string**, matching the oracle
   exactly — not a JSON number). **OpenSpectra deliberately omits the `✓`**
   in its own human-readable output — none of `park`/`unpark`/`new change`
   use a checkmark either, and this is a conscious choice to stay
   consistent with those already-shipped commands rather than an oversight.
   The alphabetical `--json` key order is a byproduct of `serde_json`'s
   default `Map` (a `BTreeMap`, since the `preserve_order` feature isn't
   enabled) rather than something OpenSpectra sorts explicitly — pinned by
   a dedicated test so enabling that feature later doesn't silently change
   the shape without a failing test.

## No active changes

Probed on 2026-08-06: `task done 1` with zero active changes remains an
operational error, exiting 1 with the shared no-active-change message prefixed
by `Error: ` on stderr. This deliberately differs from the four read/report
commands' successful empty state. See the complete matrix in
[`artifact-workflow.md`](artifact-workflow.md#no-active-change-command-matrix).

## JSON schemas

These are the v2.3.1 shapes OpenSpectra emits; v3.0.0's additions
(`provenance`, `touched_files`, `warnings`, `review_base`) are described in
[How v3.0.0 records touched files](#how-v300-records-touched-files).

`spectra task done <id> --json`:

```json
{"change": "try-feature", "status": "done", "task_desc": "1.2 ...", "task_id": "2"}
```

`.spectra/touched/<name>.json` (recovered from the binary's bundled
`/spectra:commit` skill doc, which reads this file to group a commit's dirty
files by task):

```json
{
  "change": "<change-name>",
  "touched": [
    { "task_id": "1", "task_desc": "Task description", "files": ["src/file1.ts", "src/file2.ts"] }
  ]
}
```

Struct shapes match the binary's serde-derive error strings verbatim:
`struct TouchedTracking with 2 elements` (`change`, `touched`) and
`struct TouchedEntry with 3 elements` (`task_id`, `task_desc`, `files`).

## `.spectra/` and spec-directory exclusion

OpenSpectra excludes any dirty path under `.spectra/` from the touched-files
candidate list, in addition to the change's own artifact directory,
unconditionally — independent of the repository's current ignore rules.
`init` (both the oracle's and OpenSpectra's) adds `.spectra/` to
`.gitignore`, so the filter only matters when that entry is removed or
overridden; without it, `.spectra/touched/<name>.json` would record tool state
as an implementation file.

**Oracle-confirmed on v3.0.0** (golden scenario `baseline-exclusions`, with
`.spectra/` deliberately *not* ignored): after `task start 1`, writing
`.spectra/other-state.txt`, the change's own `design.md`, an archived change's
`proposal.md`, and a canonical `specs/cap/spec.md` alongside `src/a.rs`
attributes only `src/a.rs`. So the oracle also excludes `.spectra/` regardless
of ignore rules (the tracking file's own self-recording is not separately
probed: it is already dirty before `task start 2`, so the baseline would
mask it anyway) — and it excludes the **entire configured spec directory**, not
just the change's own directory (`--file` pointing inside it is an error:
`Explicit path '<path>' is inside the configured spec directory`). OpenSpectra
excludes only the change's own directory, so a canonical-spec or other-change
edit is a divergence; it is not pinned by the golden replay because v3.0.0 only
exhibits it through `task start`, which OpenSpectra doesn't implement.

## Oracle 3.0.0 golden and known divergences

`scripts/capture-task-done.py` (macOS + the reference binary, version-pinned
to 3.0.0) runs 14 scenarios / 60 `spectra` steps in scratch git repos and
writes `golden/task-done-3.0.0.json`. For every step it records exit code,
stdout, stderr, the resulting `tasks.md` bytes, the resulting
`.spectra/touched/<change>.json` bytes, and the list of files under
`.spectra/` (informational; not compared by the replay). The golden is
self-describing — base tree, every setup step, and the isolated git
environment (`GIT_CONFIG_GLOBAL=/dev/null`, fixed identity and dates, which
also makes `review_base.head_revision` deterministic) — and the script's
default mode fails on any drift from the committed file.

`crates/spectra-cli/tests/task_done_golden_integration.rs` replays it against
OpenSpectra. Divergences are handled in three layers, and each layer fails
loudly when it goes stale:

1. **Transforms** derive OpenSpectra's expected value from the oracle's for
   three systematic differences: `--json` keeps only v2.3.1's four keys
   (`change`, `status`, `task_desc`, `task_id`) pretty-printed, where v3.0.0
   prints one compact line that adds `provenance`, `touched_files`, and
   `warnings`; human output has no `✓ ` prefix (deliberate, see step 5
   above); and OpenSpectra prints no
   `! touched_tracking_skipped_no_baseline_or_explicit_files` stderr line.
   A transform that is never applied fails the test.
2. **The ledger** `golden/task-done-3.0.0.divergences.json` pins
   OpenSpectra's value for each remaining divergent field, grouped into
   classes: `error-message` (v3.0.0 reports every bad ID as
   `Task <id> not found for change '<name>'`), `evaluation-order`,
   `leading-zero-id` (v3.0.0 rejects `01`; OpenSpectra marks task 1),
   `change-flag-hint` (`Use --change` vs OpenSpectra's shared
   `Use a change name`), and `session-wide-touched` (below). An unlisted
   divergence, an entry whose value now matches the oracle, and an entry
   matching no step all fail the test.
3. **v3-only scenarios** — every scenario using `task start` or `--file` —
   are not replayed; a separate test asserts OpenSpectra still rejects both
   (clap exit 2) without touching `tasks.md`, so implementing either forces
   those scenarios into the replay.

What still matches byte-for-byte: every `tasks.md` rewrite (grouped
numbering that ignores `##` headers and `1.1`-style labels; `*`/`+`/nested
bullets as tasks; ordered lists and blank descriptions skipped; `[x]`/`[X]`
already done; `[~]`/`[-]` succeed with the line unchanged; trailing spaces
preserved; CRLF normalized to LF; a missing final newline preserved), the
`already done` and `tasks.md not found` messages, the multiple-change exit
code, and the non-git success path.

### How v3.0.0 records touched files

v3.0.0 no longer records session-wide dirty files. Without a baseline or
`--file`, `task done` records nothing, warns
`touched_tracking_skipped_no_baseline_or_explicit_files` (in `--json`'s
`warnings`, or as a `! ` stderr line), and never creates the tracking file;
outside git the warning is `git_tracking_unavailable`. OpenSpectra instead
records every dirty file (the v2.3.1 behavior, with its own #98 baseline
filter when `new change` wrote one) — the `session-wide-touched` class.

* `task start <id> [--change] [--json]` captures a per-task baseline in
  `.spectra/task-baselines/<change>/<id>.json` without touching `tasks.md`
  (`✓ Task <id> baseline captured`; `--json`:
  `{"baseline_created","change","git_tracking_available","status":"started","task_id","warnings"}`,
  `baseline_created: false` when one already exists). It also creates the
  tracking file with an empty `touched` list and a `review_base`
  (`head_revision` plus a `dirty_fingerprints` entry for each path already
  dirty, with a git status bitmask and index/worktree identities).
* `task done` after `task start` records only paths changed since that
  baseline (`provenance: "task_baseline"`) and deletes the baseline. A path
  dirty before `task start` and untouched afterwards is not attributed. A
  `.spectra/touched/<change>.lock` sits beside the tracking file once either
  has written it.
* `task done --file <PATH>` (repeatable) records the named paths,
  deduplicated, whether or not they exist (`provenance: "explicit_files"`).
  Path normalization (e.g. a `./` prefix) is not probed: the golden step
  carrying `./src/c.rs` fails earlier on its spec-directory path.
* Each tracking entry gains a fourth field, `provenance`.

Porting this surface is tracked separately; replacing or keeping
OpenSpectra's #98 per-change baseline is an open architecture decision.

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
