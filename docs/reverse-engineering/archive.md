# Reverse-engineering `spectra archive`

How the closed-source `spectra archive` command moves a completed change out
of the active set and merges its proposed spec changes, and how OpenSpectra
reproduces it.

> Source: `Spectra.app/Contents/MacOS/spectra` v2.3.1 (arm64 Mach-O, symbols
> retained). Confirmed by running the binary as a **golden oracle** in
> scratch git repos, following `task.md`'s method.

## CLI shape

```
spectra archive [OPTIONS] [CHANGE]
```

Reference CLI options: `--no-color`, `-y`/`--yes` (skip confirmation),
`--skip-specs`, `--no-validate`, `--mark-tasks-complete`. OpenSpectra
implements `[CHANGE]`, `--skip-specs`, `--mark-tasks-complete`, and the
global `--no-color` — **`-y`/`--yes` and `--no-validate` are deliberately
not implemented**: OpenSpectra has no interactive confirmation prompt (no
other mutating command has one either) and no `validate` command for
`--no-validate` to skip, so both flags would be inert if added. Per the
"don't ship documented but inert CLI flags" project convention, they're
omitted rather than accepted-and-ignored.

**No `--json` flag exists on the reference `archive` command** — confirmed
via `--help`, unlike every other mutating command (`park`, `unpark`,
`new change`, `task done`), which all have `--json`. OpenSpectra matches
this asymmetry rather than inventing a flag the oracle doesn't have.

`[CHANGE]` is positional and optional, auto-detecting the same way as
`drift`/`show`/`park`/`task done` (reused via `change::resolve`).

## Behavior

1. Resolve the change. Errors with **`Change '<name>' not found.`** — this
   exact message covers both "never existed" and "already archived", since
   archived changes live under `changes/archive/`, outside where active-change
   resolution looks. The "no active changes"/"multiple changes" auto-detect
   errors are OpenSpectra's existing `change::resolve` wording (confirmed
   identical to the oracle for the zero-changes case; the oracle's
   multiple-changes wording — `"Specify which: a, b"` — differs slightly from
   OpenSpectra's already-shipped `"Use a change name to specify one: a, b"`;
   left as-is, matching the project's established practice of not
   re-litigating pre-existing, already-shipped command wording for a
   different command's sake).
2. Move `<spec_dir>/changes/<name>/` to
   `<spec_dir>/changes/archive/<YYYY-MM-DD>-<name>/` (confirmed via golden
   run — **not** a top-level `archived/` directory, and **not** a flat
   `changes/<date>-<name>/` either; it's nested one level deeper, under a
   literal `archive` subdirectory of `changes/`). This matches
   `change.rs::walk_change_names`'s pre-existing `name == "archive"`
   exclusion (added before this PR, inferred from this very layout) and its
   `ARCHIVED_PREFIX_RE` filter.
3. If `--mark-tasks-complete`: flip every pending checkbox in the
   *now-archived* `tasks.md` to done (`tasks::mark_all_done`), regardless of
   group headers. This runs **after** the move (not before, as an earlier
   draft had it) so that a rename failure — e.g. a same-day archive-name
   collision from re-archiving a same-named change twice — never leaves the
   still-active change with prematurely-flipped checkboxes.
4. Stamp `.openspec.yaml` (at the new location) with `archived_at: <YYYY-MM-DD>`
   and, when `git config user.name`/`user.email` are both set,
   `archived_by: <name> <email>` (unquoted, e.g. `archived_by: Ada Lovelace
   <ada@example.com>` — both fields confirmed via golden run; matches the
   already-existing `created_by`/`created`-style fields on
   `ChangeMetadata`). If git identity isn't configured, only `archived_at`
   is written — OpenSpectra warns on stderr when this happens rather than
   silently omitting the field with no signal.
5. Unless `--skip-specs`: for each `specs/**/spec.md` under the (now-moved)
   change, merge its delta into the matching canonical
   `<spec_dir>/specs/<capability>/spec.md`, then print
   `Specs applied: <capability> (added: N, modified: N, removed: N, renamed: N)`
   per capability. The `specs/` tree is walked **recursively**, so a
   nested-capability layout `specs/<Epic>/<Feature>/spec.md` merges into
   `<spec_dir>/specs/<Epic>/<Feature>/spec.md` (capability id `<Epic>/<Feature>`)
   rather than being silently skipped — `archive` and `validate` share one
   recursive collector (`fsutil::collect_delta_specs`) so their traversal, and
   its symlink-cycle safety, can't drift apart (issue #39). Two malformed
   layouts fail loud instead of mis-writing or vanishing: a `spec.md` placed
   directly under `specs/` (no capability directory) is a hard error, and a
   capability directory that is itself a **symlink** is not descended (a change
   from the pre-#39 walk, which used `fs::metadata` and followed such symlinks)
   — its delta is skipped with a stderr warning, not dropped silently. The
   oracle also prints "Snapshot created for unarchive support." — **not
   implemented**; see "Known limitations" below.
6. Clear the change's `.spectra/changes/<name>.{started,in-progress}`
   sidecar markers **and** its `.spectra/touched/<name>.json` tracking file, if any
   (best-effort; a failure here only warns, since archiving itself already
   succeeded by this point). Not oracle-confirmed, but a correctness fix:
   without it, a re-created change of the same name could silently inherit a
   stale baseline SHA or stale touched-file history from before it was
   archived — the same class of bug already fixed for `change::create` (see
   `change.rs`'s `clear_stale_sidecar_state`). Clearing `touched.json` matters
   specifically because `apply_spec_deltas`'s `code:` trace list (below) is
   sourced from it — without clearing it, a recreated change would attribute
   its trace footer to files touched by the *previous*, already-archived
   change of the same name.

## Spec delta format

A change's own `specs/<cap>/spec.md` is a *delta* against the canonical
`<spec_dir>/specs/<cap>/spec.md`, using section headers to mean:

```
## ADDED Requirements
## MODIFIED Requirements
## REMOVED Requirements
## RENAMED Requirements
```

each followed by one or more `### Requirement: <name>` blocks.

**Confirmed via golden run: `## ADDED Requirements` just means "insert these
requirement blocks into the canonical spec's `## Requirements` section,
verbatim, each followed by its own trace footer"** — not a smart merge. New
blocks are inserted right after the `## Requirements` header, before whatever
`##` section (if any) follows it, rather than blindly appended to the end of
the file. This matters once a canonical spec has grown a trailing section of
its own (e.g. a human-added `## Notes`/`## Appendix`): appending at the
file's end would incorrectly nest the new requirement under that unrelated
section instead of inside `## Requirements` (OpenSpectra-only fix, not
independently oracle-confirmed for this specific edge case — the golden
samples observed only had a bare `## Requirements` with nothing after it).
If the canonical spec has no `## Requirements` header at all (predating the
convention, or hand-edited to drop it), insertion falls back to right after
`## Purpose` instead, using the same before-the-next-section logic; only a
spec with neither header falls all the way back to the literal end of the
file, since at that point there's no recognizable section structure left to
nest under incorrectly.
The very first requirement appended to a fresh spec has no separator; every
subsequent one (including the first appended to an *already-populated*
spec) is preceded by a `\n---\n` line. If the canonical spec doesn't exist
yet, it's created first:

```
# <capability> Specification

## Purpose

TBD - created by archiving change '<source>'. Update Purpose after archive.

## Requirements
```

Each appended requirement block gets a trace footer:

```
<!-- @trace
source: <change-name>
updated: <YYYY-MM-DD>
code:
  - <touched file 1>
  - <touched file 2>
-->
```

**The `code:` list's exact source is unconfirmed** — every golden sample
observed happened to show the same single file (`.spectra.yaml`, dirty for
unrelated reasons in the scratch repo the whole time), which isn't enough
to distinguish "files this change's tasks touched" from some other
git-diff-based heuristic. OpenSpectra populates `code:` from this change's
`.spectra/touched/<name>.json` (the same tracking file `task done` writes),
sorted — a reasonable, self-consistent choice given the infrastructure
already exists for exactly this purpose, but **not verified against the
oracle**. `code: []` when no touched-file data exists for the change.

OpenSpectra Phase 2 implements `MODIFIED`, `REMOVED`, and `RENAMED` against
the **OpenSpec published convention** recorded in `docs/openspec-compat.md`,
not against a golden oracle sample. No oracle samples were captured for
these delta kinds, so the closed-source reference could still diverge in edge
cases such as trace-footer handling or conflict wording. For Phase 2, the
OpenSpec convention is the compatibility target.

Application order is:

1. `RENAMED Requirements`: parse `- FROM: ` / `- TO: ` bullet pairs whose
   values are backticked full `### Requirement: <name>` headers. The matched
   canonical requirement block's header line is rewritten to the TO name;
   the block body is otherwise untouched. A FROM without a following TO, or
   a TO without a preceding FROM, is an error naming the capability.
2. `REMOVED Requirements`: delete each matching canonical requirement block.
3. `MODIFIED Requirements`: replace each matching canonical requirement block
   with the delta's block verbatim, including any `(Previously: ...)` line.
4. `ADDED Requirements`: append new requirement blocks using the existing
   insertion-point and trace-footer behavior described above.

A requirement block runs from a `### Requirement:` header up to, but not
including, the next `### Requirement:` header, the next `## ` section header,
or EOF. Requirement names are matched case-sensitively after trimming
leading/trailing whitespace and collapsing internal whitespace runs to a
single space. This keeps OpenSpectra's existing case-sensitive section and
requirement-header behavior while tolerating hand-written spacing differences.

Spec validation still runs **before** the change directory is moved. During
validation OpenSpectra computes the full merged result in memory using the
same RENAMED → REMOVED → MODIFIED → ADDED order that archive later writes.
A missing MODIFIED/REMOVED/RENAMED-FROM target, a RENAMED-TO name that already
exists in the canonical spec, or an ADDED requirement that already exists after
the earlier operations, is a conflict and leaves the change active and unmoved.
ADDED-only deltas may create a previously missing canonical
`specs/<cap>/spec.md`; MODIFIED/REMOVED/RENAMED against a missing canonical spec
are conflicts because there is nothing to match.

Several **malformed-delta** shapes are also rejected loudly at validation
(rather than silently dropping the author's intent, which would be worse than
the pre-Phase-2 unsupported-header reject this replaced): a recognized
`## MODIFIED/REMOVED/RENAMED Requirements` header that parses to zero entries;
a duplicate section header of the same kind (only the first is parsed, so the
second would be dropped); the same requirement ADDED twice within one delta
(the canonical-spec exists check can't see an intra-delta duplicate); and a
malformed `## RENAMED Requirements` FROM/TO pair (a FROM without a TO, a
missing/unbalanced backtick, or backtick content that isn't a
`### Requirement:` header). RENAMED accepts either `-` or `*` list bullets. To
keep validation genuinely side-effect-free, the in-memory validation pass skips
the ADDED trace-footer step (it reads this change's `.spectra/touched/` sidecar
and would rename a corrupt one aside) -- that only happens during the real
write, so a validation that fails on a later capability leaves the sidecar
untouched.

## Known limitations

Deferred, matching the project's existing "conservative implementation,
document the gap" pattern used elsewhere — e.g. `drift`'s uncalibrated
Tasks-collision detection.

- **No snapshot/unarchive support.** The oracle prints "Snapshot created for
  unarchive support." and (presumably) a `spectra unarchive` command exists
  to reverse an archive using that snapshot; neither the snapshot mechanism
  nor an `unarchive` command is implemented. Reversing an OpenSpectra
  archive today means manually moving the directory back and reverting
  `.openspec.yaml`/the canonical spec by hand (or via `git revert`, since
  archiving isn't its own commit).
- **`code:` trace provenance** — see above.
- **Section-header matching is case-sensitive.** `## ADDED Requirements`,
  `## MODIFIED Requirements`, `## Requirements`, and `## Purpose` are matched
  with their exact reference casing. A hand-written delta using different
  casing (e.g. `## modified requirements`) matches none of these patterns
  and is treated as a no-op section — no oracle sample exists to confirm the
  reference CLI's actual casing sensitivity, so this wasn't changed
  speculatively.
