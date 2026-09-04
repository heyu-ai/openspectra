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
implements all of these options. On an interactive terminal, archive prompts
with `Archive '<name>'? (y/N) ` unless `-y`/`--yes` is present. Piped input
skips the prompt. `--no-validate` skips the pre-move validation pass but still
applies spec deltas after moving the change; `--skip-specs` skips both steps.
This is independent of the implemented top-level `spectra validate` command.

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
2. Resolve every delta through the shared fence-aware Markdown parser and
   prepare all resulting canonical spec contents in memory. Identical
   ADDED/MODIFIED operations and already-applied REMOVED/RENAMED operations are
   no-ops; case/whitespace variants and differing content remain conflicts.
3. Create an exclusive claim in `changes/archive/` and re-check that
   `<YYYY-MM-DD>-<name>` does not exist. This serializes archive writers before
   the first canonical spec mutation.
4. Snapshot every affected canonical spec and verify it still matches the
   prepared baseline immediately before writing. Writes are atomic. A
   cross-device change move falls back to an exclusive, recursively verified
   copy before source removal.
5. Unless `--skip-specs` or change metadata declares `skip_specs: true`, apply
   every prepared spec mutation. New capability deltas may seed the main
   `## Purpose`; an existing capability keeps its current Purpose. A change
   declaring `retire_capabilities: true` may delete a spec whose final
   requirement was removed, but only after validation confirms no unaccounted
   content would be lost.
6. Move `<spec_dir>/changes/<name>/` to
   `<spec_dir>/changes/archive/<YYYY-MM-DD>-<name>/`, then optionally mark tasks
   complete and stamp `archived_at`/`archived_by`.
7. If any write, retirement, move, task update, or metadata update fails,
   restore the active change and every spec that still matches the transaction's
   expected output. A concurrent edit is never overwritten during rollback.
8. On success, clear the change's `.spectra/changes/<name>.{started,in-progress}`
   markers and `.spectra/touched/<name>.json` best-effort.

The spec tree is recursive, so `specs/<Epic>/<Feature>/spec.md` maps to the
same nested canonical capability. The collector rejects a root-level
`specs/spec.md`, does not descend symlinked directories, and fails on unreadable
entries. The oracle's durable \"Snapshot created for unarchive support\" feature
remains separate and unimplemented; see \"Known limitations\" below.

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

## Architecture decision: atomicity versus recovery

OpenSpectra deliberately separates two guarantees that the oracle's snapshot
message otherwise makes easy to conflate:

1. **Single-run atomicity** — an `archive` invocation must either commit the
   canonical spec updates and archive move together, or restore the active
   change and every affected spec. This is a safety divergence from the probed
   oracle order above: OpenSpectra will stage and validate all mutations, claim
   the destination, snapshot affected paths, apply the mutations, move the
   change, and roll back on failure.
2. **Later unarchive support** — retaining a durable snapshot after a successful
   archive so a future command can reverse it. This remains the separate,
   unresolved parity question tracked in issue #111.

The first guarantee prevents a failed command from leaving a half-archived
change; it does not imply or implement the second. The maintainer selected this
safe default on 2026-09-05. There is no legacy unsafe mode: preserving an
observed partial-write failure is not worth a second public archive contract.
