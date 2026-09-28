# Reverse-engineering `spectra archive`

How the closed-source `spectra archive` command moves a completed change out
of the active set and merges its proposed spec changes, and how OpenSpectra
reproduces it.

> Source: `Spectra.app/Contents/MacOS/spectra` v2.3.1 (arm64 Mach-O, symbols
> retained). Confirmed by running the binary as a **golden oracle** in
> scratch git repos, following `task.md`'s method. The "Trace data" section
> additionally relies on probes of v3.0.0 (2026-09-26); each oracle observation
> there names the version it was observed on, and statements without a version
> describe OpenSpectra's own behavior.

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

`--mark-tasks-complete` rewrites every `[ ]` checkbox on a `-`/`*`/`+`
bullet (indented or not) to `[x]` — including blank-description ones such as
`- [ ]` and `- [ ] `, which do not count as tasks elsewhere — and leaves
`[x]`/`[X]`, other markers (`[~]`, `[-]`), and every non-bullet line
untouched. Probed on oracle v3.0.0 during the #177 review; before that
OpenSpectra skipped blank-description checkboxes.

~~**No `--json` flag exists on the reference `archive` command**~~ — true of
2.3.1 only. **Oracle 3.0.0 adds `--preview` and `--json`** (probed
2026-09-28, one jail per operation, both binaries on identical fixtures):

- `--preview` prints what archive would do and **modifies nothing** (verified
  by comparing the full file tree before and after on the oracle and on
  OpenSpectra). It skips the confirmation prompt. Human form:
  `Archive preview: <change>` / `Incomplete tasks: <n>` /
  `Spec updates: <number of capabilities>` (or `none`).
- `--preview --json` prints one line:
  `{"change_id", "spec_updates": [{"capability", "exists", "added",
  "modified", "removed", "renamed", "conflict_source"}], "incomplete_tasks",
  "warnings", "has_delta_specs"}`. The counts include only operations that
  would apply: a MODIFIED/REMOVED naming a requirement the canonical spec
  lacks, or an ADDED naming one it already has, is not counted and is not an
  error here (real archive then fails). `--skip-specs`,
  `--mark-tasks-complete` and `--no-validate` do not change the preview; only
  `.openspec.yaml` `skip_specs: true` empties `spec_updates` (and sets
  `has_delta_specs: false`). A delta file with no operation section fails even
  in preview: `Failed to parse delta spec: Invalid format: Delta spec must
  contain at least one operation (ADDED, MODIFIED, REMOVED, or RENAMED)`. A
  missing change reports `Change '<name>' does not exist`.
  `conflict_source` was `null` and `warnings` was `[]` in every probed case,
  including a second active change modifying the same capability; their
  triggers are unknown and OpenSpectra always emits those values.
- `--json` without `--preview` archives and prints one line:
  `{"archived_id", "archived_path", "applied_specs", "snapshot_created",
  "total_added", "total_modified", "total_removed", "total_renamed",
  "cleanup_warnings"}`. The confirmation prompt still applies on a terminal.
- The human result line is now `✓ Archived: <change> → <archived id>`,
  followed by the `Specs applied:` lines and `Snapshot created for unarchive
  support.`

**Deliberate divergences.** (1) The oracle takes a snapshot for unarchive
(`.spectra/snapshots/`); OpenSpectra has no snapshot mechanism (#111), so it
prints `snapshot_created: false` and omits the snapshot line rather than
claim one exists. (2) The oracle lists capabilities in raw `readdir` order
(probed: identical to `ls -f`, independent of creation order, and different on
other filesystems); OpenSpectra sorts them by name, as the archive merge
already does. (3) Error text for a delta that cannot apply still uses
OpenSpectra's wording; the oracle says `Delta for <cap> declares N MODIFIED
operation(s) but 0 applied; …` (tracked with the validate alignment, W9 in
`docs/migration-plan.md`).

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
requirement block (v2.3.1 golden runs), and (probed on v3.0.0, 2026-09-26)
under every MODIFIED one as well:

```
<!-- @trace
source: <change-name>
updated: <YYYY-MM-DD>
code:
  - <file 1>
  - <file 2>
-->
```

On v2.3.1 the `code:` list is identical for every requirement of one archive
(downstream corpus of 96 oracle-archived specs, #98), so a spec grows
O(requirements × archives): downstream (heyu-ai/openspectra#98) measured
trace footers at 67% of the bytes in yibi-mvp specs over 20 KB, 87% in the
largest. The list itself is the change's session-wide dirty-file set:
the v2.3.1 corpus contains fonts, screenshots, PID files and spreadsheets
(kaochenlong/spectra-app#47, #95, #102). On v3.0.0 the same probe showed
`code:` still including a file that was dirty before `new change`, and the
footer was written even though `task done` had recorded nothing (it printed
`touched_tracking_skipped_no_baseline_or_explicit_files`), so v3.0.0's list does
not come from `.spectra/touched/<name>.json`. The v3.0.0 formatting puts two
blank lines before the first footer and none after the last. When the working
tree has no uncommitted file at archive time, v3.0.0 writes no footer at all:
the ADDED and MODIFIED requirements are applied and nothing records them
(`scripts/capture-trace-interop.py`, probes `mixed-archive-clean` and
`mixed-archive-dirty`; one uncommitted file is enough for both footers to
appear, each listing it).

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
# `spectra trace migrate`. Entries are appended in order: one per archive,
# plus entries absorbed from inline footers.
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
  outside code fences and indented code blocks from the canonical spec and
  absorbs it into the sidecar. A footer opens with a line indented by at most
  three spaces that is `<!--`, any whitespace, then `@trace`, where `@trace` is
  followed by the end of the line or by whitespace. Only trailing whitespace
  may follow it on a multi-line footer's opener, so `<!--@trace`,
  `<!--  @trace` and `<!-- @trace  ` all count. An opener indented by four
  spaces or a tab (after up to three spaces) is an indented code block, not a
  comment (#179). When `@trace` is glued to any other character
  (`<!-- @trace-sidecar: ... -->`, `<!-- @trace:`, `<!--@trace-->`), the line
  is not a footer at all: it is neither stripped nor warned about, `trace
  migrate --check` does not count it, and a REMOVED delta deletes it with its
  requirement.
  This covers both footers the oracle wrote in a mixed setup and ones from
  before this divergence. Stripping first means a MODIFIED or REMOVED block
  does not take its footer with it. Footers with identical `source`, `updated`,
  `code` and `tests` merge into one entry whose requirement names go under
  `imported`: a footer does not record whether it came from ADDED or MODIFIED,
  so none is guessed. Absorption is idempotent. A footer belongs to the
  requirement block it sits in, in any `##` section, not only the first
  `## Requirements`: specs with a second `## Requirements` section or a
  leftover `## ADDED Requirements` section exist downstream, and attributing
  only within the first section silently dropped those names (#179). Delta
  application itself still sees only the first section (not probed against
  the oracle). A footer attributed to no requirement (outside every
  requirement block) is still absorbed, with no name. After the delta is
  applied, archive strips once more, so footers that arrive inside the
  delta's own ADDED or MODIFIED blocks are absorbed too. This happens because
  the convention is to paste a whole requirement into MODIFIED, and a block
  copied from an oracle-written spec carries its footer. The
  "identical content" check that makes an ADDED or MODIFIED block a no-op
  ignores multi-line footers recognized by the same opener rule, so an
  unchanged block pasted with a `<!--@trace` footer is still a no-op, and a
  block whose only change is inside a code example is still applied.
- A footer whose opener line carries whitespace and then more content after
  `@trace` (including a single-line `<!-- @trace ... -->`), or whose body has
  an unknown key, a line
  without a colon, a flow-style list (`code: [a]`), a repeated or missing/empty
  `source`/`updated`, a list item outside a `code:`/`tests:` list, or no
  closing `-->` line, is not guessed at. It stays in `spec.md` and a warning names
  its line in the written file. A REMOVED delta targeting a requirement that
  holds such a footer fails the archive (already at validation) instead of
  discarding it. A MODIFIED delta fails the same way unless its pasted block
  carries that footer unchanged, in which case nothing is lost.
- RENAMED rewrites the old names recorded in earlier entries, so they keep
  matching the current requirement. It rewrites only events after the last
  removal of that name, because an earlier requirement with the same name was a
  different one. That includes the `added`/`modified`/`imported` names of the
  removal entry itself, since `removed` comes first within an entry. `removed`
  and `renamed` are history and are never rewritten.
  The oracle never writes the sidecar: `scripts/capture-trace-interop.py`
  records `spec.trace.yaml` byte-identical after an oracle v3.0.0 archive of
  ADDED + MODIFIED (clean and dirty tree) and of RENAMED, and the pointer line
  kept. Its golden output is
  `docs/reverse-engineering/golden/trace-interop-3.0.0.tsv`. A rename done by
  the oracle therefore leaves names in older entries stale in a mixed setup
  (the same script records the resulting `--check` warning). The oracle only
  accepts a RENAMED spelling that OpenSpectra rejects, and the reverse (see
  "Spec delta format" and #192). `spectra trace migrate` reports such
  stale names: names whose last recorded event is not a removal but that match
  no current requirement. Events are taken in entry order, and within an entry
  `removed` comes before `modified`/`added`/`imported`. It does not fix them
  automatically.
- The sidecar goes through the same prepare/commit/rollback path as `spec.md`
  (see "Architecture decision: atomicity versus recovery"). Retiring a
  capability removes its sidecar too, so the directory does not linger with
  only a YAML file. A sidecar that is not valid UTF-8, does not parse, has an
  unknown field, or whose `version` is not `1` fails the archive before any
  canonical file is written; it is never overwritten. The version is checked
  first, so a future-version sidecar reports the version rather than its new
  fields. Unknown fields are rejected, not ignored,
  because a mistyped key in a hand-edited sidecar would otherwise be silently
  dropped on the next rewrite. A `spec.md` that carries the pointer while its
  sidecar is missing (typically a new sidecar that was never `git add`ed, or
  one lost in a merge) also fails the archive, including during validation,
  rather than rebuilding the sidecar from empty and losing its history (#179).
  The pointer counts only outside code fences and when indented by at most
  three spaces with no tab, the same rule as a footer opener, so a pointer
  shown as an indented code example is ignored. If the sidecar cannot be
  recovered, deleting the pointer line lets the next archive start a new one.
- `spectra trace migrate [--dry-run] [--check] [--json]` applies the same
  absorption to every canonical spec without an archive, so existing bloated
  specs can be migrated at once. For each spec it writes the sidecar before
  `spec.md`, and a rerun after an interruption does not duplicate entries. A
  corrupt sidecar, or a pointer whose sidecar is missing, fails only that spec
  and makes the command exit 1. `--check` writes nothing and exits 1 if any
  spec still has an inline footer (parseable or not), a stale trace name, an
  unreadable sidecar, or a pointer without a sidecar. A requirement counts as
  current for the stale-name check when it exists in any `##` section, the
  same rule that attributes footers. It is meant for CI or
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
not against a golden oracle sample. Apart from the RENAMED spelling probe
below, no golden samples were captured for these delta kinds, so the
closed-source reference could still diverge in edge
cases such as conflict wording. The oracle's trace-footer treatment of
MODIFIED has since been probed on v3.0.0 (see "Trace data"). That probe also
applied a REMOVED delta, but what happened to the removed block's footer was
not recorded. RENAMED was probed on v3.0.0 (2026-09-27,
`scripts/capture-trace-interop.py`, probes `renamed-validate-*` and
`renamed-archive`): the oracle rejects the OpenSpec bullet spelling below
(`Delta spec must contain at least one operation`, with or without a blank
line after the heading) and accepts only `FROM: ### Requirement: <name>` /
`TO: ### Requirement: <name>` lines without a bullet or backticks. With that
spelling it applies the rename (`renamed: 1`, heading rewritten). OpenSpectra
rejects the oracle's spelling (`section contains no recognizable entries`).
Both sides fail loud, so a RENAMED delta written for one tool is refused by
the other rather than skipped. Whether OpenSpectra should also accept the
oracle's spelling is open (#192). For Phase 2, the OpenSpec convention is the
compatibility target.

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
