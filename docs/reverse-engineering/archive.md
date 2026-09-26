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
   markers, the OpenSpectra-only `.spectra/changes/<name>.touched-baseline.json`
   checkpoint (see "Deliberate divergences (#98)" below), and
   `.spectra/touched/<name>.json` best-effort.

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
verbatim, each followed by its own trace footer"** (the oracle's footer;
OpenSpectra writes trace data to a sidecar instead, see "Trace data") — not a
smart merge. Header
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

### Trace data

**Oracle behavior.** The oracle appends an inline footer under every ADDED
requirement block, and (probed on v3.0.0, 2026-09-26) under every MODIFIED one
as well:

```
<!-- @trace
source: <change-name>
updated: <YYYY-MM-DD>
code:
  - <file 1>
  - <file 2>
-->
```

The `code:` list is identical for every requirement of one archive, so a spec
grows O(requirements × archives): downstream (heyu-ai/openspectra#98) measured
trace footers at 67% of the bytes in yibi-mvp specs over 20 KB, 87% in the
largest. The list itself is the change's session-wide dirty-file set:
the v2.3.1 corpus contains fonts, screenshots, PID files and spreadsheets
(kaochenlong/spectra-app#47, #95, #102). On v3.0.0 the same probe showed
`code:` still including a file that was dirty before `new change`, and the
footer was written even though `task done` had recorded nothing (it printed
`touched_tracking_skipped_no_baseline_or_explicit_files`), so v3.0.0's list does
not come from `.spectra/touched/<name>.json`. The v3.0.0 formatting puts two
blank lines before the first footer and none after the last.

**OpenSpectra: `spec.trace.yaml` sidecar (deliberate divergence, #98).** The
downstream ADR-0029 D3 (heyu-ai/yibi-mvp) ruled for moving trace data out of
`spec.md`. OpenSpectra's archive writes no inline footer. Instead each archive
appends one entry to `specs/<cap>/spec.trace.yaml`, and `spec.md` carries a
single pointer line under its title:

```
# <cap> Specification

<!-- @trace-sidecar: spec.trace.yaml -->

## Purpose
```

```yaml
# Traceability for spec.md, written by `spectra archive` and
# `spectra trace migrate`. One entry per archived change.
version: 1
traces:
- source: add-login
  updated: 2026-09-26
  added:
  - Login Button
  code:
  - src/auth.rs
- source: tweak-login
  updated: 2026-10-02
  modified:
  - Login Button
  renamed:
  - from: Sign In
    to: Log In
  code: []
```

- An entry lists the requirements that archive `added`, `modified`, `removed`
  and `renamed`. Empty lists are omitted, except `code`, which is always
  written. `code` is this change's touched files, sorted (see the divergences
  below).
- Before applying the delta, archive strips every parseable inline footer
  outside code fences from the canonical spec and absorbs it into the sidecar.
  This covers both footers the oracle wrote in a mixed setup and ones from
  before this divergence. Stripping first means a MODIFIED or REMOVED block
  does not take its footer with it. Footers with identical `source`, `updated`,
  `code` and `tests` merge into one entry whose requirement names go under
  `imported`: a footer does not record whether it came from ADDED or MODIFIED,
  so none is guessed. Absorption is idempotent. A footer attributed to no
  requirement (outside every requirement block) is still absorbed, with no
  name. After the delta is applied, archive strips once more, so footers that
  arrive inside the delta's own ADDED or MODIFIED blocks are absorbed too. This
  happens because the convention is to paste a whole requirement into MODIFIED,
  and a block copied from an oracle-written spec carries its footer.
- A footer that opens with `<!-- @trace` but has an unknown key, a missing or
  empty `source`/`updated`, a list item outside a `code:`/`tests:` list, or no
  closing `-->` is not guessed at. It stays in `spec.md` and a warning names
  its line in the written file. If a MODIFIED or REMOVED delta targets a
  requirement holding such a footer, the archive fails (already at validation)
  instead of discarding it.
- RENAMED rewrites the old names recorded in earlier entries, so they keep
  matching the current requirement. It rewrites only entries after the last
  removal of that name, because an earlier requirement with the same name was a
  different one. `removed` and `renamed` are history and are never rewritten.
  A rename done by the oracle does not update the sidecar, so names in older
  entries can go stale in a mixed setup. `spectra trace migrate` reports such
  stale names: names whose last recorded event is not a removal but that match
  no current requirement. Events are taken in entry order, and within an entry
  `removed` comes before `modified`/`added`/`imported`. It does not fix them
  automatically.
- The sidecar goes through the same prepare/commit/rollback path as `spec.md`
  (see "Architecture decision: atomicity versus recovery"). Retiring a
  capability removes its sidecar too, so the directory does not linger with
  only a YAML file. A sidecar that does not parse, has an unknown field, or
  whose `version` is not `1` fails the archive before any canonical file is
  written; it is never overwritten. Unknown fields are rejected, not ignored,
  because a mistyped key in a hand-edited sidecar would otherwise be silently
  dropped on the next rewrite.
- `spectra trace migrate [--dry-run] [--check] [--json]` applies the same
  absorption to every canonical spec without an archive, so existing bloated
  specs can be migrated at once. For each spec it writes the sidecar before
  `spec.md`, and a rerun after an interruption does not duplicate entries. A
  corrupt sidecar fails only that spec and makes the command exit 1. `--check`
  writes nothing and exits 1 if any spec still has an inline footer (parseable
  or not), a stale trace name, or an unreadable sidecar. It is meant for CI or
  pre-commit in a repo where the oracle may still archive.

**Probed interoperability (v3.0.0, 2026-09-26).** With either a footer-free
`spec.md` plus sidecar, or a spec that keeps only `source`/`updated` in each
footer, the oracle validated and archived an ADDED + MODIFIED + REMOVED delta
without complaint and left `spec.trace.yaml` byte-identical. It did write full
inline footers again for the ADDED and MODIFIED requirements. A second probe
alternated the two tools on one repo: oracle ADDED, then `trace migrate`, then
OpenSpectra MODIFIED, oracle ADDED, and OpenSpectra ADDED. The oracle kept the
`<!-- @trace-sidecar: spec.trace.yaml -->` pointer line, left the sidecar
untouched, and footed only the requirement it added. The final OpenSpectra
archive absorbed that footer, and both tools' `validate` passed afterwards.
Hence the absorption above. Mixing the tools re-grows a capability's footers
only until the next OpenSpectra archive that touches that capability, or the
next `trace migrate`. An archive absorbs footers only in the capabilities its
delta changes. `trace migrate --check` catches them in between. Footers absorbed this way keep the oracle's `code:` list
as-is, so the collection divergences below apply only to entries OpenSpectra
writes itself.

**Deliberate divergences in `code` collection (#98), both OpenSpectra-only
(relative to v2.3.1):**

- **Task-scoped collection.** `spectra new change` and every `task done` that
  records successfully write a checkpoint,
  `.spectra/changes/<name>.touched-baseline.json`, holding a content
  fingerprint (length + FNV-1a 64; symlink target; `missing` for deleted) of
  every dirty file at that moment. A path whose state cannot be determined (an
  unreadable file or symlink, a directory or submodule, a stat error other than
  not-found) is still checkpointed, with a `null` fingerprint, and always counts
  as changed. Known limitation: a submodule (or an untracked nested git repo,
  which `git status` also reports as a directory) that was already dirty before
  the change started is therefore attributed to the first `task done` even if no
  task touched it; fingerprinting it precisely would need an extra git call per
  submodule. The
  next `task done` records the files whose fingerprint differs from the
  checkpoint, drawn from the current dirty set plus checkpointed paths that are
  now clean (a pre-existing edit a task reverted to its committed content). A
  file that was already dirty before the change started, and is never edited by
  a task, is no longer attributed to the change. When recording into
  `.spectra/touched/<name>.json` fails, the checkpoint is left as it was, so a
  later `task done` still records those files. A change with no baseline
  (created before this divergence) falls back to the old session-wide
  behavior; an unreadable or corrupt baseline does the same, with a warning.
  The baseline lives beside `.started` rather than under `.spectra/touched/`,
  which keeps that directory to oracle-format tracking files. It is cleared
  with the other sidecars on `new change` and `archive`.
- **Stale-path pruning.** At archive time, paths that no longer exist on disk
  (`symlink_metadata` reports not-found or not-a-directory; a dangling symlink
  still counts as present) are left out of `code:`. Any other stat error keeps
  the path and prints a warning. `.spectra/touched/<name>.json` itself is not
  pruned, since commit tooling still needs to know which task deleted a file.

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
   insertion-point behavior described above. Trace data for all four kinds
   goes to the sidecar (see "Trace data").

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
Validation computes the same merged blocks but builds no trace entry and does
not read this change's `.spectra/touched/` sidecar, because trace provenance
is irrelevant to compatibility. It does parse an existing `spec.trace.yaml`, so
`spectra validate` already reports a corrupt sidecar. In `archive` the
failure comes before any canonical file is written, and the frozen change is
restored. The trace entry and the sidecar pointer are produced only while
preparing the exact bytes that will be committed.

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

- **`code` trace provenance.** OpenSpectra fills `code` from
  `.spectra/touched/<name>.json`; v3.0.0 demonstrably does not (see "Trace
  data"), and how v3.0.0 derives its list is not reverse-engineered yet. Nor
  are v3.0.0's `task start` and `task done --file`, which feed its per-task
  baseline; OpenSpectra implements neither.
- **MODIFIED drops the block's `---` separator.** A requirement block's range
  includes the `---` line before the next requirement, and MODIFIED keeps only
  the replaced block's trailing whitespace, so the separator after a modified
  requirement disappears (the headers stay on their own lines). The v3.0.0
  oracle keeps it. This predates the sidecar.

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
