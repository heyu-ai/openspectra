# Reverse-engineering the artifact workflow commands

How the closed-source `spectra` binary implements `status`, `new artifact`,
and `instructions` (the artifact-DAG workflow surface), and how OpenSpectra
reproduces them.

> Source binary: `Spectra.app/Contents/MacOS/spectra` v2.3.1 (arm64 Mach-O,
> symbols retained). Behaviour was pinned by running the binary as a **golden
> oracle** over purpose-built synthetic projects (status/new-artifact/
> instructions state matrices), by string mining, and — for the `instructions`
> preflight regexes — by disassembly. Golden captures live in
> `docs/reverse-engineering/golden/instructions-*-2.3.1.json`; the full probe
> log (with dates) is in
> `docs/openspec/changes/fill-artifact-workflow-cli/design.md`.

## TL;DR

The workflow surface is a pure filesystem DAG — **no persistent state**. A
built-in `spec-driven` schema defines four artifacts with dependencies
`proposal → design; proposal → specs → tasks`; every status is derived on the fly from
file existence. OpenSpectra implements it in `crates/spectra-core/src/
{schema,artifact,instructions}.rs`.

## The `spec-driven` schema (`schema.rs`)

| artifact | outputPath | deps |
|---|---|---|
| `proposal` | `proposal.md` | — |
| `design` | `design.md` | `proposal` |
| `specs` | `specs/**/*.md` | `proposal` |
| `tasks` | `tasks.md` | `specs` (NOT `design`) |

Per-artifact `description`, `instruction`, and `template` texts are embedded
verbatim from the oracle (pinned by a test that byte-compares the constants
against the golden JSON captures). `applyRequires` is `["tasks"]`.

Status derivation (pure function of the change dir):

- `done` — outputPath exists (for `specs`: any `.md` recursively under
  `specs/`).
- `ready` — not done, all deps done.
- `blocked` — not done, some dep not done (`missingDeps` lists them).

Statuses are independent: deleting `specs/` flips `specs` back to `ready`
but leaves `tasks` at `done` (file still exists) — no cascade.

## `spectra new change` (v2.3.1 alignment, BREAKING in OpenSpectra)

The oracle creates only the change dir plus `.openspec.yaml`
(`schema`/`created`/`created_by`), **no artifact files** — otherwise `status`
would start all-done and `new artifact` would always hit "already exists".
OpenSpectra ≤0.2.1 scaffolded `proposal.md`/`design.md`/`tasks.md`; this was
removed. `created_by` comes from `git config` (falls back to user-level
config outside a repo): `name <email>` / `name` / `<email>` / `unknown` —
the key is never omitted. OpenSpectra keeps writing its own
`.spectra/changes/<name>.started` baseline (drift needs it; the oracle has
no equivalent).

## `spectra status [--change] [--schema] [--json]`

JSON (pretty, camelCase): `{changeName, schemaName, isComplete,
applyRequires, artifacts[{id, outputPath, status, missingDeps?}]}` —
`missingDeps` present only when `blocked`. Human output marks
done/ready/blocked as `✓`/`○`/`✗` with `    blocked by: <deps>` under
blocked items; when everything is done a final `  ✓ All artifacts complete`
line is appended. Errors (exit 1): `Change '<name>' not found.` and
`Schema not found: Schema '<s>' not found in project, user, or built-in
locations`.

## `spectra new artifact <TYPE> [CAPABILITY] [--change] [--stdin] [--force] [--json]`

`--json` is a **single compact line** (unlike the other three commands):
`{"artifact","change","path","status":"created","validated","warnings":[]}`.
`warnings` was `[]` in every probed scenario. Template mode (no `--stdin`)
writes the schema template and reports `validated: false` (templates are not
validated); stdin content is validated per type and written byte-for-byte
(no trailing-newline fixup):

| type | rule (case-insensitive substring unless noted) |
|---|---|
| proposal | contains `## why`, `## problem`, or `## summary` |
| design | contains `## context` |
| tasks | contains literal `- [ ]`, `* [ ]`, or `+ [ ]` |
| spec | some line trims to exactly `## ADDED/MODIFIED/REMOVED/RENAMED Requirements` (case-sensitive; validation stops at the operation-heading level) |

Check order: change resolution → unknown type → change existence →
capability required/kebab-case (`[a-z0-9-]`, no leading/trailing `-`) →
already-exists (`--force` bypasses this check but **not** validation) →
empty stdin → content validation → write. Validation failure writes nothing.
Error strings are reproduced verbatim — including the oracle's own
inconsistency: here `Change '<name>' not found` has **no trailing period**,
while `status`'s variant has one.

## `spectra instructions [ARTIFACT] [--change] [--schema] [--json]`

Two modes. **Artifact mode** (explicit artifact, or — with no argument —
the first not-done artifact in schema order): pretty camelCase JSON
`{changeName, artifactId, schemaName, changeDir, outputPath, description,
instruction, locale, template, dependencies[{id,done,path,description}],
unlocks}`. `unlocks` lists only direct dependents that are currently blocked
on this artifact (both sides not-done). **Apply mode** (no argument with all
four artifacts done, or the literal `apply`): `{changeName, changeDir,
schemaName, contextFiles, progress{total,complete,remaining},
tasks[{id,description,done,parallel}], state, missingArtifacts?, locale,
instruction, preflight?}`.

- Apply uses its own loose checkbox parser `^\s*[-*+]\s*\[(.)\]\s*(.+)$`:
  any single-char state counts as a task, only `x`/`X` is done, and an
  uppercase `[P]` immediately after the checkbox is stripped into
  `parallel: true`. A match whose description is empty after trimming is
  discarded and does not affect task ids or progress.
- `state`: `blocked` (zero parsed tasks), `all_done`, else `ready`.
  `missingArtifacts` (only when non-empty) lists not-done `applyRequires`;
  `preflight` appears only in `ready`.
- `contextFiles` lists only done artifacts (absolute paths / glob). The
  oracle's key order is hash-map nondeterministic; OpenSpectra fixes schema
  order (documented determinism choice).

### Preflight (recovered by disassembly + behaviour matrix)

- `missingFiles` (`status: critical`): file refs from the proposal's
  "Affected code:" section only (marker also matches the Chinese variants
  `主要檔案`/`影響檔案`/`變更檔案`/`受影響檔案`; scan stops at the next
  heading). Backticked refs match
  `` `([^`]*?/[^`]*?\.(?:rs|ts|tsx|jsx|svelte|md|json|yaml|toml|css|html|js))` ``;
  bare lines must fully match the prefix-whitelisted
  `^(?:specs|src|src-tauri|crates|lib|tests|app|public)/[\w\-/]+\.(?:…same extensions…)$`.
- `driftedFiles` (`status: warnings`): backtick refs across
  proposal+design+tasks that exist on disk and whose
  `git log -1 --format=%cs -- <path>` date is strictly later than the
  change's `created` date.
- `staleness`: `daysOld` = today − `created` (negative values are not
  clamped); `isStale` = `daysOld > 7`; the whole key vanishes when
  `created` is missing/unparseable.

### Deliberate divergences (documented, not bugs)

- `--skill <name>`: the oracle prints large **proprietary embedded skill
  bodies**; OpenSpectra does not ship that text and answers
  `Unknown skill: <name>` for every value.
- `contextFiles` key order fixed to schema order (oracle nondeterministic).
- `new artifact --force` refuses a final artifact path that is a symlink,
  instead of following it and overwriting the link target as the oracle does.
  This is an intentional security divergence following the PR #41 symlink
  hardening precedent; parent-directory symlinks remain out of scope.
- The oracle's multiple-active-changes error (`Use --change to specify
  one:`, mtime-ordered) differs from OpenSpectra's pre-existing
  `change::resolve` wording (alphabetical); unification is tracked in
  issue #50.
