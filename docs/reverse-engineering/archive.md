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
skips the prompt. `--no-validate` skips the side-effect-free compatibility
preflight but the frozen deltas are still prepared and applied before the
final archive move; `--skip-specs` skips both steps. This is independent of
the implemented top-level `spectra validate` command.

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
2. Create an exclusive claim in `changes/archive/`, re-check that
   `<YYYY-MM-DD>-<name>` does not exist, and freeze the active change with a
   same-filesystem rename to a hidden sibling staging path. All subsequent
   parsing reads that frozen directory; the active name is no longer available
   for another writer to mutate or archive.
3. Fingerprint the frozen tree, resolve every delta through the shared
   fence-aware Markdown parser, and prepare all canonical spec contents in
   memory. ADDED/MODIFIED operations are no-ops only when the canonical block
   already has identical content. RENAMED is a no-op only when the exact TO
   target proves the final state. A missing REMOVED target in an existing
   canonical spec is an error; an already-absent whole capability is a no-op
   only when `retire_capabilities: true` explicitly authorizes retirement.
4. Snapshot every affected canonical spec plus the frozen metadata/tasks
   bytes. Compute the exact metadata/tasks output bytes, then verify both the
   canonical baselines and frozen-tree fingerprint before the first spec
   commit. Canonical writes are atomic.
5. Unless `--skip-specs` or change metadata declares `skip_specs: true`, apply
   every prepared spec mutation. New capability deltas may seed the main
   `## Purpose`; an existing capability keeps its current Purpose. A change
   declaring `retire_capabilities: true` may delete a spec whose final
   requirement was removed, but only after the side-effect-free compatibility
   preflight confirms no unaccounted content would be lost.
6. Move the hidden frozen directory to
   `<spec_dir>/changes/archive/<YYYY-MM-DD>-<name>/`. An EXDEV move copies from
   the frozen source to an exclusive destination and recursively verifies it.
   Copy or verification failure (including a verification I/O error) cleans
   the destination. Once verification succeeds the destination is
   authoritative: source-cleanup failure is a warning, and the hidden staged
   source is retained at the reported path for manual recovery.
7. Optionally write the precomputed completed-task bytes, then stamp the
   precomputed `archived_at`/`archived_by` metadata bytes.
8. If preparation, fingerprint verification, a spec write, the move, task
   update, or metadata update fails, restore the frozen/archive directory to
   the active name only when that name is unoccupied. Metadata, tasks, and
   canonical specs are restored only when their current bytes equal either the
   original or this transaction's exact expected output; concurrent edits are
   never overwritten.
9. On success, clear the change's `.spectra/changes/<name>.{started,in-progress}`
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

**Confirmed via golden run: `## ADDED Requirements` means "insert these
requirement blocks into the canonical spec's `## Requirements` section,
verbatim, each followed by its own trace footer"** — not a smart merge. Header
recognition is ASCII-case-insensitive, so accepted hand-written forms such as
`## added requirements`, `### requirement: Name`, `## requirements`, and
`## purpose` retain their parsed raw block while participating in the same
merge and placement rules.

New blocks are inserted right after the canonical Requirements header, before
whatever `##` section (if any) follows it, rather than blindly appended to the
end of the file. This matters once a canonical spec has grown a trailing
section of its own (e.g. a human-added `## Notes`/`## Appendix`): appending at
the file's end would incorrectly nest the new requirement under that unrelated
section instead of inside Requirements (OpenSpectra-only fix, not independently
oracle-confirmed for this specific edge case — golden samples observed only a
bare Requirements section). If the canonical spec has no Requirements header,
insertion falls back to right after Purpose using the same
before-the-next-section logic; only a spec with neither header falls back to
the literal end.
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

A requirement block runs from a parsed level-three Requirement header up to,
but not including, the next level-three header, the next level-two section, or
EOF. The parser recognizes the `Requirement:` prefix and the Purpose,
Requirements, and delta section names ASCII-case-insensitively. Parsed
`markdown::Requirement` names and raw blocks are authoritative throughout the
merge; archive does not reparse an exact-case header. Requirement *names*
remain case-sensitive after trimming leading/trailing whitespace and
collapsing internal whitespace runs to one space. A case-only name variant is
a spelling conflict rather than an idempotent match.

Compatibility validation runs after the active directory has been frozen but
before canonical specs are mutated or the final archive destination is
created. It computes the full merged result in memory using the same RENAMED →
REMOVED → MODIFIED → ADDED order as application, without reading or repairing
touched sidecars. A missing MODIFIED or REMOVED target, a missing
RENAMED-FROM whose exact RENAMED-TO target does not prove the final state, a
RENAMED-TO name that already exists, or a conflicting ADDED requirement is an
error. ADDED-only deltas may create a missing canonical spec.
MODIFIED/RENAMED against a missing canonical spec are conflicts; REMOVED is
also a conflict unless explicit capability-retirement metadata makes the
already-absent whole capability an authorized no-op.

Several **malformed-delta** shapes are also rejected loudly at validation
(rather than silently dropping the author's intent, which would be worse than
the pre-Phase-2 unsupported-header reject this replaced): a recognized
`## MODIFIED/REMOVED/RENAMED Requirements` header that parses to zero entries;
a duplicate section header of the same kind (only the first is parsed, so the
second would be dropped); the same requirement ADDED twice within one delta
(the canonical-spec exists check can't see an intra-delta duplicate); and a
malformed `## RENAMED Requirements` FROM/TO pair (a FROM without a TO, a
missing/unbalanced backtick, or backtick content that isn't a
`### Requirement:` header). RENAMED accepts either `-` or `*` list bullets.
Validation builds side-effect-free ADDED blocks without trace footers, because
trace provenance is irrelevant to compatibility and reading this change's
`.spectra/touched/` sidecar could rename a corrupt file aside. Trace footers
are added only while preparing the exact bytes that will be committed.

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

## Architecture decision: atomicity versus recovery

OpenSpectra deliberately separates two guarantees that the oracle's snapshot
message otherwise makes easy to conflate:

1. **Single-run atomicity** — an `archive` invocation freezes the source before
   preparation, verifies its fingerprint before committing specs, and must
   either finish the archive or restore the unoccupied active path and every
   transaction-owned output. Rollback uses original and exact expected bytes,
   so it never restores untouched paths or overwrites concurrent edits. A
   verified EXDEV destination is the one exception to source removal:
   cleanup failure retains the hidden staged copy and reports its recovery
   path, but does not roll back the authoritative archive or canonical specs.
2. **Later unarchive support** — retaining a durable snapshot after a successful
   archive so a future command can reverse it. This remains the separate,
   unresolved parity question tracked in issue #111.

The first guarantee prevents a failed command from leaving a half-archived
change; it does not imply or implement the second. The maintainer selected this
safe default on 2026-09-05. There is no legacy unsafe mode: preserving an
observed partial-write failure is not worth a second public archive contract.
