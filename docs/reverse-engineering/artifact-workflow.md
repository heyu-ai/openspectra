# Reverse-engineering the artifact workflow (`status` / `new artifact` / metadata-only `new change`)

How the closed-source spectra CLI models the spec-driven artifact workflow —
the artifact DAG, status derivation, artifact scaffolding, and change
metadata — and how OpenSpectra reproduces it. Covers WP1+WP2 of
`fill-artifact-workflow-cli` (issue #43); `instructions`/`analyze` (WP3/WP4)
will extend this file and `analyze.md`.

> Source: `Spectra.app/Contents/MacOS/spectra` v2.3.1 (arm64 Mach-O). All
> contracts below were confirmed by running the binary as a **golden oracle**
> in scratch git repos (probe sessions 2026-07-18); instruction/template text
> is pinned byte-for-byte by golden fixtures under
> `golden/instructions-{proposal,design,specs,tasks}-2.3.1.json` and the
> `embedded_instruction_text_matches_oracle_goldens_byte_for_byte` test.

## `new change` — metadata only

`spectra new change <name>` creates ONLY the change directory and
`.openspec.yaml`; no artifact files are scaffolded (they come later via
`new artifact`). The metadata file has exactly three keys:

```yaml
schema: spec-driven
created: YYYY-MM-DD
created_by: <identity>
```

`created_by` comes from `git config user.name` / `user.email` with a 4-way
fallback matrix (unlike archive metadata, partial configuration is preserved
and the key always has a value):

| user.name | user.email | created_by |
| --- | --- | --- |
| set | set | `Name <email>` |
| set | unset/empty | `Name` |
| unset/empty | set | `<email>` |
| unset/empty | unset/empty | `unknown` |

Outside a git repo, `git config <key>` still reads user-level configuration,
so identity is picked up from the operator's global config
(`change_creator_identity_falls_back_to_user_config_outside_a_repo` pins
this). "Key absent" (git exits non-zero) and "key set to empty" both land on
the same fallback row.

**OpenSpectra implementation note (not an observed oracle divergence):**
OpenSpectra serializes the metadata through `serde_yaml` rather than raw
string formatting, so a git identity containing YAML-special content
(`:`, ` #`, leading indicator chars) is quoted and round-trips. Plain values
serialize byte-identical to the oracle's output. The oracle's behavior for
YAML-special identities is **unprobed**; if a probe later shows it writes
them raw (producing an unparseable file), that would be a deliberate,
recorded divergence — we keep the readable-back behavior.

## Workflow schema (artifact DAG)

Built-in `spec-driven` schema, four artifacts (declaration order is the
canonical display order):

| id | outputPath | deps |
| --- | --- | --- |
| `proposal` | `proposal.md` | — |
| `design` | `design.md` | `proposal` |
| `specs` | `specs/**/*.md` | `proposal` |
| `tasks` | `tasks.md` | `specs` |

`design` is NOT a dependency of `tasks` (probed; the proposal arrow chain is
`proposal → design` and `proposal → specs → tasks`). `applyRequires` is
`["tasks"]`.

### done / ready / blocked derivation

Pure file existence, re-derived per invocation, no cascade or persistence:

- `done` — the artifact's outputPath exists. For the globbed `specs`
  artifact: at least one `.md` file anywhere under `specs/` (any name — a
  stray `README.md` counts; the glob is the contract, not the `spec.md`
  naming convention).
- `ready` — not done, all deps done.
- `blocked` — not done, some dep not done; `missingDeps` lists the missing
  ids in schema declaration order.

Deleting a dep's files after a downstream artifact exists does not un-done
the downstream artifact (no cascade).

**Symlinks (probed 2026-07-18):** the oracle's `specs/**/*.md` glob FOLLOWS
directory symlinks — a change whose `specs/<cap>` is a symlink to a
directory containing `spec.md` reports `specs: done`. OpenSpectra matches
this (resolving through `fs::metadata`), adds cycle protection via a
canonicalized-visited set (cycle behavior itself is unprobed on the oracle),
and — divergence-safe hardening — propagates non-NotFound I/O errors
(e.g. permission denied) instead of silently misreporting them as
"not done".

## `spectra status [--change] [--schema] [--json]`

- Unknown schema errors BEFORE change resolution:
  `Schema not found: Schema '<name>' not found in project, user, or built-in locations`
- Missing change: `Change '<name>' not found.` (WITH trailing period — unlike
  `new artifact`'s change-not-found, which has none).
- JSON: camelCase, struct declaration order
  (`changeName, schemaName, isComplete, applyRequires, artifacts[]`), each
  artifact `{id, outputPath, status, missingDeps?}` where `missingDeps`
  appears only when blocked.
- Human output: always all four artifacts, `✓`/`○`/`✗` markers with
  `blocked by:` detail lines.

## `spectra new artifact <TYPE> [CAPABILITY] [--change] [--stdin] [--force] [--json]`

CLI TYPE is singular `spec`; the DAG id is plural `specs` — both
independently observed, neither renames to match the other.

### Check order (probed; each error short-circuits)

1. change auto-resolution (`--change` omitted with 0/2+ active changes)
2. unknown TYPE: `Unknown artifact type '<t>'. Valid types: proposal, design, tasks, spec`
3. change existence: `Change '<name>' not found` (no period)
4. capability required for spec: `Capability name is required for spec type. Usage: ...`
5. capability kebab-case: `Invalid capability name '<cap>'. Must be kebab-case (e.g., user-auth, data-export)`
6. already exists (skipped by `--force`): `Artifact already exists: <path>. Use --force to overwrite`
7. empty stdin: `No content received from stdin`
8. per-type content validation (stdin only; templates are written unvalidated)

`create_runs_probed_checks_in_order_and_does_not_write_on_validation_failure`
locks the order pairwise.

### Per-type stdin validation

- proposal: requires a `## Why`, `## Problem`, or `## Summary` heading
  (case-insensitive substring)
- design: requires `## Context`
- tasks: requires at least one checkbox (`- [ ]`, `* [ ]`, or `+ [ ]`)
- spec: requires one of the four delta-operation `##` headings; error text is
  the oracle's `Delta spec parse error: Invalid format: ...` string

### Semantics

- `--force` does NOT skip content validation: invalid stdin + `--force`
  exits 1 and the existing file is untouched
  (`force_with_invalid_content_exits_1_and_preserves_the_original_file`).
- **Extra CAPABILITY positional for non-spec types is silently ignored**
  (probed 2026-07-18: `new artifact proposal extra-arg --change X --stdin`
  exits 0 and creates the file; same for tasks without `--stdin`). Pinned by
  `extra_capability_positional_is_ignored_for_non_spec_types`; rejecting it
  would be a divergence.
- Without `--stdin`, the artifact's byte-pinned template is written verbatim
  and unvalidated; `validated` is `false` in JSON.
- JSON success output is compact single-line:
  `{"artifact":...,"change":...,"path":...,"status":"created","validated":...,"warnings":[]}`
- Errors are identical with and without `--json`: plain `Error: <msg>` on
  stderr, exit 1, nothing on stdout.
- **OpenSpectra hardening (behavior-compatible):** the final write uses
  `create_new` (no `--force`) / temp-file + atomic rename (`--force`) so a
  concurrent creator surfaces the oracle-aligned already-exists error instead
  of silently clobbering. The oracle's own race behavior is unprobed.

## Instructions goldens provenance

Captured 2026-07-18 against Spectra.app 2.3.1 with all four artifacts
present, so `dependencies[].done` are all `true` and `unlocks` all `[]` —
capture-state-dependent fields, NOT part of the pinned contract. Pinned
fields: `description`, `instruction`, `template`, `outputPath`,
`dependencies[].id`. `changeDir` values are normalized to
`/tmp/oracle-probe/...` (neutralized, not the raw capture value).

## Known divergences (tracked, not silently shipped)

1. **Multi-change resolve error** — oracle:
   `Multiple changes found. Use --change to specify one: <names>` sorted by
   mtime newest-first; OpenSpectra (pre-existing `change::resolve`):
   `Use a change name to specify one:` sorted alphabetically. Consciously
   deferred (see design.md); now load-bearing for `status`/`new artifact`.
2. **Fresh design scaffold vs `drift`** — probed 2026-07-18: after
   `new artifact design` (template only), the oracle's drift reports
   `Structure 0/20 anchors broken`; OpenSpectra's anchor Resolver marks the
   template's ~20 prose Symbol anchors broken (the `KNOWN DIVERGENCE`
   over-count in anchors.rs). Fix belongs in the Symbol filter/stoplist
   calibration, not the byte-pinned template.
