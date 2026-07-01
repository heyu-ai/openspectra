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
2. If `--mark-tasks-complete`: flip every pending checkbox in `tasks.md` to
   done (`tasks::mark_all_done`), regardless of group headers.
3. Move `<spec_dir>/changes/<name>/` to
   `<spec_dir>/changes/archive/<YYYY-MM-DD>-<name>/` (confirmed via golden
   run — **not** a top-level `archived/` directory, and **not** a flat
   `changes/<date>-<name>/` either; it's nested one level deeper, under a
   literal `archive` subdirectory of `changes/`). This matches
   `change.rs::walk_change_names`'s pre-existing `name == "archive"`
   exclusion (added before this PR, inferred from this very layout) and its
   `ARCHIVED_PREFIX_RE` filter.
4. Stamp `.openspec.yaml` (at the new location) with two appended fields:
   `archived_by: "<git config user.name> <git config user.email>"` and
   `archived_at: <YYYY-MM-DD>` (both confirmed via golden run; matches the
   already-existing `created_by`/`created`-style fields on `ChangeMetadata`).
5. Unless `--skip-specs`: for each `specs/<capability>/spec.md` under the
   (now-moved) change, merge its delta into
   `<spec_dir>/specs/<capability>/spec.md`, then print
   `Specs applied: <capability> (added: N, modified: 0, removed: 0, renamed: 0)`
   per capability. The oracle also prints "Snapshot created for unarchive
   support." — **not implemented**; see "Known limitations" below.

## Spec delta format (and OpenSpectra's narrowed scope)

A change's own `specs/<cap>/spec.md` is a *delta* against the canonical
`<spec_dir>/specs/<cap>/spec.md`, using section headers to mean:

```
## ADDED Requirements
## MODIFIED Requirements
## REMOVED Requirements
## RENAMED Requirements
```

each followed by one or more `### Requirement: <name>` blocks.

**Confirmed via golden run: `## ADDED Requirements` just means "append these
requirement blocks to the canonical spec's `## Requirements` section,
verbatim, each followed by its own trace footer"** — not a smart merge.
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

**MODIFIED/REMOVED/RENAMED are not implemented.** Applying those correctly
means locating a specific existing requirement in the canonical spec by name
and replacing/deleting/renaming it — a materially different, more complex
operation than ADDED's pure append, and genuinely under-observed (only one
golden sample per header would be needed to nail down the exact behavior,
but doing so for all three, plus their interaction with the same-named
duplicate-append quirk ADDED itself has, was out of scope for this pass).
`spectra archive` errors when the delta contains any of these three headers,
naming the offending capability and instructing the user to re-run with
`--skip-specs` and apply that spec change by hand:

```
capability '<cap>': <MODIFIED|REMOVED|RENAMED> requirement deltas aren't supported yet -- re-run with --skip-specs and apply this spec change by hand
```

This validation runs **before** the change directory is moved (a
self-caught bug during manual E2E testing: the original implementation
validated *after* the move, so a MODIFIED-delta error left the change
stuck half-archived — directory already gone from the active list, but
never actually fully archived, with no easy way back short of manually
moving the directory back). Validating first means the change is left
exactly as it was on any spec-format error.

## Known limitations (deferred, matching the project's existing
"conservative implementation, document the gap" pattern used elsewhere —
e.g. `drift`'s uncalibrated Tasks-collision detection)

- **No snapshot/unarchive support.** The oracle prints "Snapshot created for
  unarchive support." and (presumably) a `spectra unarchive` command exists
  to reverse an archive using that snapshot; neither the snapshot mechanism
  nor an `unarchive` command is implemented. Reversing an OpenSpectra
  archive today means manually moving the directory back and reverting
  `.openspec.yaml`/the canonical spec by hand (or via `git revert`, since
  archiving isn't its own commit).
- **MODIFIED/REMOVED/RENAMED spec deltas** — see above.
- **`code:` trace provenance** — see above.
