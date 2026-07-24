# OpenSpec ecosystem compatibility

Can OpenSpectra run **directly** on a real [Fission-AI
OpenSpec](https://github.com/Fission-AI/OpenSpec) project? This is the
"format difference investigation" deliverable of Phase 2 (issue #26): a
difference matrix between OpenSpec's on-disk conventions and OpenSpectra's
model, plus the concrete compatibility work that matrix implies.

> **Sources.** OpenSpec conventions are quoted from the OpenSpec repo's own
> canonical spec (`openspec/specs/openspec-conventions/spec.md`) and
> `docs/concepts.md` at `Fission-AI/OpenSpec@main` (fetched 2026-07). Where a
> detail could not be confirmed from those, it's marked *unconfirmed*.
> OpenSpectra behavior is from this repo's source and the
> `docs/reverse-engineering/` write-ups.

## OpenSpec's on-disk layout (canonical)

From `openspec-conventions/spec.md`, "Requirement: Project Structure":

```
openspec/
├── project.md              # Project-specific context
├── AGENTS.md               # AI assistant instructions
├── config.yaml             # OpenSpec CLI config (newer versions)
├── specs/                  # Current deployed capabilities
│   └── [capability]/
│       ├── spec.md         # WHAT and WHY
│       └── design.md       # HOW (optional)
└── changes/                # Proposed changes
    ├── [change-name]/
    │   ├── proposal.md
    │   ├── tasks.md
    │   ├── design.md       # optional
    │   └── specs/
    │       └── [capability]/
    │           └── spec.md # delta against the canonical spec
    └── archive/
        └── YYYY-MM-DD-[change-name]/
```

Change directories may also carry a `.openspec.yaml` metadata file — it
appears in real OpenSpec change dirs but is **optional** (not every change
has one).

## Difference matrix

Legend: **✅ compatible** (works as-is), **🔧 adapt** (OpenSpectra needs a
small change), **➕ spectra-only** (OpenSpectra adds this; OpenSpec projects
won't have it).

| Concern | OpenSpec | OpenSpectra (before #26) | Verdict |
|---|---|---|---|
| Spec dir name | `openspec/` | `spec_dir`, defaults to `openspec` | ✅ compatible |
| Root config | `openspec/config.yaml`, `project.md`, `AGENTS.md` — **no `.spectra.yaml`** | requires `.spectra.yaml` at root; every command bails `Not initialized` without it | 🔧 adapt — `init --adopt` |
| Change layout | `changes/<name>/{proposal,tasks,design?,specs/<cap>/spec.md}` | identical | ✅ compatible |
| Change metadata | optional `.openspec.yaml` | read via `ChangeMetadata` (`serde(flatten)` keeps unknown keys; absence → default) | ✅ compatible |
| Canonical spec | `specs/<cap>/spec.md` with `## Purpose` / `## Requirements` / `### Requirement:` / `#### Scenario:` | same headers parsed by `archive`/`spec` | ✅ compatible |
| Scenario bullets | `- **WHEN**` / `- **THEN**` (conventions spec) — also plain `- GIVEN/WHEN/THEN` in some docs | not parsed by `archive` (ADDED copies blocks verbatim); `drift` anchor extraction reads prose | ✅ compatible for archive |
| Delta: `## ADDED Requirements` | append new requirement blocks | implemented (append into `## Requirements`, trace footer) | ✅ compatible |
| Delta: `## MODIFIED Requirements` | replace existing requirement by header match | **rejected with an error**, asks for `--skip-specs` | 🔧 adapt — implement |
| Delta: `## REMOVED Requirements` | delete existing requirement by header match | **rejected with an error** | 🔧 adapt — implement |
| Delta: `## RENAMED Requirements` | `- FROM:`/`- TO:` header rename | **rejected with an error** | 🔧 adapt — implement |
| Archive destination | `changes/archive/YYYY-MM-DD-<name>/` | identical | ✅ compatible |
| Per-change sidecar state | none | `.spectra/changes/<name>.{started,parked,in-progress}`, `.spectra/touched/<name>.json` | ➕ spectra-only |
| `.spectra/` in `.gitignore` | n/a | `init` ensures the entry | ➕ spectra-only |

### Key finding

The only *blocking* incompatibilities are two, both in this matrix as 🔧:

1. **No root `.spectra.yaml`.** An OpenSpec project has `openspec/` but no
   `.spectra.yaml`, so `find_root`/`Config::is_initialized` never recognize
   it and every command bails with `Not initialized`. Fixed by
   **`spectra init --adopt`** (below).
2. **Only `## ADDED Requirements` deltas archive.** OpenSpec's normal
   workflow produces MODIFIED/REMOVED/RENAMED deltas, which `archive`
   currently rejects outright. This is a compatibility *necessity*, not a
   nice-to-have — an OpenSpec user archiving a real change will hit it
   immediately. Fixed by **delta application** (below).

Everything else is either already compatible or additive state OpenSpectra
writes under `.spectra/` (git-ignored, invisible to OpenSpec tooling).

## Compatibility work implied

### 1. `spectra init --adopt`

`init --adopt` makes OpenSpectra usable on an existing `openspec/` project
**without touching any OpenSpec content**:

- Spec dir is `openspec` (the default). `--adopt` does **not** inspect the
  directory's contents to pick a different name — configurable spec-dir
  discovery is future work. The one content check it does make is a guard: if
  `openspec` already exists as a *file* (not a directory), adoption fails with a
  clear message instead of a generic downstream `create_dir_all` error.
- Write `.spectra.yaml` (`spec_dir: openspec`) — the one file OpenSpectra needs
  and an OpenSpec project doesn't have.
- Ensure `.spectra/` is in `.gitignore` (same as plain `init`).
- **Non-destructive:** create `openspec/{changes,specs}/` only if missing
  (idempotent `create_dir_all`), and never create or overwrite `project.md`,
  `AGENTS.md`, `config.yaml`, or any existing `changes/`/`specs/` content.

Mechanically, plain `init` and `--adopt` do the same filesystem work — both
create the two directories idempotently and both refuse only when
`.spectra.yaml` already exists. `--adopt` differs in **intent and messaging**:
it signals "wire OpenSpectra into a project that already has OpenSpec content"
and reports `Adopted …` rather than `Initialized …`. (Plain `init` on a dir
that already has `openspec/` content but no `.spectra.yaml` therefore also
succeeds; `--adopt` is the explicit, self-documenting way to do it.)

### 2. Archive: MODIFIED / REMOVED / RENAMED delta application

The authoritative algorithm, verbatim from `openspec-conventions/spec.md`
("Requirement: Archive Process Enhancement"):

> the archive command SHALL:
> 1. Parse RENAMED sections first and apply renames
> 2. Parse REMOVED sections and remove by normalized header match
> 3. Parse MODIFIED sections and replace by normalized header match (using new names if renamed)
> 4. Parse ADDED sections and append new requirements
> - AND validate that all MODIFIED/REMOVED headers exist in current spec
> - AND validate that ADDED headers don't already exist

Delta section formats (from `docs/concepts.md` and the conventions spec):

```markdown
## MODIFIED Requirements

### Requirement: Session Expiration
The system MUST expire sessions after 15 minutes of inactivity.
(Previously: 30 minutes)

#### Scenario: Idle timeout
- **WHEN** 15 minutes pass without activity
- **THEN** the session is invalidated

## REMOVED Requirements

### Requirement: Remember Me
(Deprecated in favor of 2FA.)

## RENAMED Requirements
- FROM: `### Requirement: Old Name`
- TO: `### Requirement: New Name`
```

Semantics OpenSpectra adopts:

- **Requirement block** = from a `### Requirement: <name>` line up to (but
  not including) the next `### Requirement:` or the next `## ` section header
  (or end of file) — the same block-boundary rule ADDED already uses.
- **Normalized header match:** compare requirement names trimmed of
  surrounding whitespace and collapsed internal whitespace, so
  `### Requirement:  Session   Expiration ` matches
  `### Requirement: Session Expiration`.
- **RENAMED** rewrites the matched block's header from the FROM name to the
  TO name (content untouched); subsequent MODIFIED referencing the TO name
  then applies.
- **MODIFIED** replaces the entire matched block with the delta's block
  (verbatim, including any `(Previously: …)` line — OpenSpec keeps it as
  human-facing provenance).
- **REMOVED** deletes the matched block.
- **Validation before any move** (mirrors the existing ADDED pre-move
  validation): a MODIFIED/REMOVED/RENAMED-FROM header that doesn't exist in
  the canonical spec, or an ADDED header that already does, is a conflict —
  error out naming the capability and requirement, leaving the change
  active and untouched. This matches OpenSpec's "report specific conflicts,
  require manual resolution" behavior.

`--skip-specs` remains the escape hatch for anything the user wants to apply
by hand.

#### Divergence from the reverse-engineered oracle

`docs/reverse-engineering/archive.md` records that the **closed-source
`Spectra.app` oracle's** exact MODIFIED/REMOVED/RENAMED behavior was never
captured (no golden samples), which is why OpenSpectra originally *rejected*
those deltas rather than guess. This change implements them against the
**OpenSpec published convention** instead — the right source of truth for the
Phase 2 goal ("run on a real OpenSpec project"). The two may differ in
edge-case wording or the `code:`/trace-footer treatment of
modified/renamed blocks; where they do, OpenSpec compatibility wins and the
divergence is noted in `archive.md`. If an oracle sample ever surfaces that
contradicts this, `archive.md` is the place to reconcile it.

## Verification

- `crates/spectra-cli/tests/openspec_compat.rs` runs the full
  `init --adopt → list → drift → archive` flow against a **vendored fixture**
  that mirrors a real OpenSpec project (root `openspec/` with `project.md`,
  no `.spectra.yaml`, canonical specs, and a change carrying ADDED +
  MODIFIED + REMOVED + RENAMED deltas), asserting no panic and that the
  canonical specs end up correctly merged.
- Optional future CI job: install the OpenSpec CLI (npm), scaffold a real
  project with it, and run every OpenSpectra command against it. Deferred —
  the vendored fixture covers the format contract without a Node toolchain in
  CI.
```
