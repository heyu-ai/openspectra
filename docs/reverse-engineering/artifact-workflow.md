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
> `docs/openspec/changes/fill-artifact-workflow-cli/design.md`. Additional
> dispute-resolution probes from the PR #48 mob review (capability handling,
> symlink glob semantics, stdin ordering, fresh-scaffold drift) are dated
> 2026-07-18 below.

## TL;DR

The workflow surface is a pure filesystem DAG — **no persistent state**. A
built-in `spec-driven` schema defines four artifacts with dependencies
`proposal → {design, specs} → tasks`; every status is derived on the fly from
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
against the golden JSON captures, which also covers `outputPath` and
`dependencies[].id`). `applyRequires` is `["tasks"]`.

Golden-capture provenance: the `instructions-*-2.3.1.json` fixtures were
captured 2026-07-18 with all four artifacts present, so their
`dependencies[].done` are all `true` and `unlocks` all `[]` —
capture-state-dependent fields, not part of the pinned contract. `changeDir`
values are normalized to `/tmp/oracle-probe/...` (neutralized, not the raw
capture value).

Status derivation (pure function of the change dir):

- `done` — outputPath exists (for `specs`: any `.md` recursively under
  `specs/`).
- `ready` — not done, all deps done.
- `blocked` — not done, some dep not done (`missingDeps` lists them).

Statuses are independent: deleting `specs/` flips `specs` back to `ready`
but leaves `tasks` at `done` (file still exists) — no cascade.

**Symlinks (probed 2026-07-18):** the oracle's `specs/**/*.md` glob FOLLOWS
directory symlinks — a change whose specs live behind a symlink still counts
as done. OpenSpectra matches (resolving through `fs::metadata`, unlike
`fsutil::collect_delta_specs`, whose archive/validate domain deliberately
skips symlinks), adds cycle protection via a canonicalized-visited set
(cycle behaviour itself is unprobed on the oracle), and — divergence-safe
hardening — its error semantics are match-wins and order-independent: a
found `.md` anywhere is deterministically "done" regardless of unreadable
siblings, while a no-match scan with a traversal error (e.g. permission
denied) propagates instead of being silently misreported as "not done".

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

**OpenSpectra hardening (not an observed oracle divergence):** the metadata
is serialized through `serde_yaml` rather than raw string formatting, so a
git identity containing YAML-special content (`:`, ` #`, leading indicator
chars) is quoted and round-trips. Plain values serialize byte-identical to
the oracle's output. The oracle's behaviour for YAML-special identities is
**unprobed**; if a probe later shows it writes them raw (producing an
unparseable file), that would be a deliberate, recorded divergence — we keep
the readable-back behaviour.

## `spectra status [--change] [--schema] [--json]`

JSON (pretty, camelCase): `{changeName, schemaName, isComplete,
applyRequires, artifacts[{id, outputPath, status, missingDeps?}]}` —
`missingDeps` present only when `blocked`. Human output marks
done/ready/blocked as `✓`/`○`/`✗` with `    blocked by: <deps>` under
blocked items; when everything is done a final `  ✓ All artifacts complete`
line is appended. Errors (exit 1): `Change '<name>' not found.` and
`Schema not found: Schema '<s>' not found in project, user, or built-in
locations`.

## No-active-change command matrix

A fresh initialized project with zero active changes was probed against
Spectra 2.3.1 on 2026-08-06. The read/report commands treat this as a normal
empty state even in JSON mode; their sentinel remains plain text.

| command | exit | channel | output |
|---|---:|---|---|
| `status` / `status --json` | 0 | stdout | `No active changes. Create one with: spectra new change <name>` |
| `instructions tasks` / `instructions tasks --json` | 0 | stdout | same plain-text sentinel |
| `drift` / `drift --json` | 0 | stdout | same plain-text sentinel |
| `analyze` / `analyze --json` | 0 | stdout | same plain-text sentinel |
| `validate` | 0 | stdout | empty |
| `validate --json` | 0 | stdout | `[]` |
| `list` | 0 | stdout | `No active changes.` |
| `list --json` | 0 | stdout | pretty `{"changes": []}` |
| `task done 1` | 1 | stderr | `Error: No active changes. Create one with: spectra new change <name>` |
| `show` | 1 | stderr | `Error: Please specify an item name.` |
| `park` / `unpark` | 2 | stderr | clap's required-`<NAME>` usage error |

Only the four auto-resolving read commands use the shared plain-text success
sentinel. Commands that require a concrete change, such as `task done`, keep
the resolver's operational-error path.

The empty state outranks the schema gate: `status --schema bogus` and
`instructions tasks --schema bogus` on an empty project still print the
sentinel and exit 0 (probed 2026-08-06) — the same "change comes first" check
order pinned in [`schemas.md`](schemas.md)'s "Check order" section.

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

Additional probed semantics (2026-07-18):

- **An extra CAPABILITY positional for non-spec types is silently ignored**
  (`new artifact proposal extra-arg --change X --stdin` exits 0 and creates
  the file; same for tasks without `--stdin`). Pinned by
  `extra_capability_positional_is_ignored_for_non_spec_types`; rejecting it
  would be a divergence.
- **`--stdin` is drained before precondition checks**: with an open stdin
  pipe and an invalid invocation (unknown TYPE), the oracle waits for EOF
  and only then prints the unknown-type error — preconditions do NOT
  short-circuit the stdin read. OpenSpectra matches; "fixing" the ordering
  would be a divergence.
- Errors are identical with and without `--json`: plain `Error: <msg>` on
  stderr, exit 1, nothing on stdout.

**OpenSpectra hardening (behaviour-compatible):** the target path only ever
appears as a fully-written file — content is written to a uniquely-named
sibling temp file (`create_new`; unique per process AND per call), then
installed atomically: `rename` (replace) for `--force`, `hard_link`
(no-clobber) otherwise. A concurrent creator surfaces the oracle-aligned
already-exists error instead of silently clobbering, and a failed write
leaves no partial artifact for `status` to misread as done. The oracle's own
race behaviour is unprobed. Residual risk (accepted): the no-clobber install
requires `link(2)`; a filesystem without hard links (FAT/exFAT) would fail
loudly on non-force creates — not a real scenario for a git-hosted
`openspec/` tree.

## `spectra instructions [ARTIFACT] [--change] [--schema] [--json]`

Two modes. **Artifact mode** (explicit artifact, or — with no argument —
the first not-done artifact in schema order): pretty camelCase JSON
`{changeName, artifactId, schemaName, changeDir, outputPath, description,
instruction, context?, rules?, locale, template,
dependencies[{id,done,path,description}],
unlocks}`. `unlocks` lists only direct dependents that are currently blocked
on this artifact (both sides not-done). **Apply mode** (no argument with all
four artifacts done, or the literal `apply`): `{changeName, changeDir,
schemaName, contextFiles, progress{total,complete,remaining},
tasks[{id,description,done,parallel}], state, missingArtifacts?, locale,
instruction, preflight?}`.

- Apply uses its own loose checkbox parser `^\s*[-*+]\s*\[(.)\]\s*(.+)$`:
  any single-char state counts as a task, only `x`/`X` is done, and an
  uppercase `[P]` immediately after the checkbox is stripped into
  `parallel: true`.
- `state`: `blocked` (zero parsed tasks), `all_done`, else `ready`.
  `missingArtifacts` (only when non-empty) lists not-done `applyRequires`;
  `preflight` appears only in `ready`.
- `contextFiles` lists only done artifacts (absolute paths / glob). The
  oracle's key order is hash-map nondeterministic; OpenSpectra fixes schema
  order (documented determinism choice).
- Artifact JSON reads optional values from `<spec_dir>/config.yaml`. `context`
  is a plain string with leading and trailing whitespace trimmed; internal
  newlines are presumed preserved by that trim behavior. `rules` is only the
  requested artifact's list, flattened from the `rules.<artifact>` map entry
  to a plain string array.
- An unset `context`, or a `rules` map with no entry for the requested
  artifact, omits the corresponding key entirely rather than emitting `null`
  or `[]`. With both fields present, the probed top-level order is
  `changeName, artifactId, schemaName, changeDir, outputPath, description,
  instruction, context, rules, locale, template, dependencies, unlocks`.
- Apply JSON and all human-readable output omit both project fields. Read or
  YAML parse failures are lenient and behave as though the keys were absent.
- Edge shapes (all probed against v2.3.1, 2026-08-06): a blank `context`
  (`""` or whitespace-only) omits the key rather than emitting an empty
  string; a non-string scalar `context` (`123`, `true`, `1.5`) also omits
  the key — no scalar-to-string coercion; `rules` present with `context`
  absent emits only `rules` (the two fields are independent); a malformed
  `rules` value (a non-map, or a non-list artifact entry) drops only the
  `rules` key while `context` is still emitted — a malformed field never
  poisons its sibling or the `schema:` selector.

### Embedded skills

`--skill <name>` selects one of exactly 15 embedded bodies, in canonical
registry order: `tdd`, `audit`, `apply`, `archive`, `ask`, `commit`, `debug`,
`discuss`, `drift`, `ingest`, `propose`, `analyze`, `verify`, `sync`, and
`clarify`. The first 11 shipped before the complete enumeration finding. A
roughly 100-candidate wordlist probe and a binary-registry string cross-check
then independently yielded the same complete set of 15.

A known name writes the static body bytes directly to stdout and exits 0,
including outside an initialized project. Skill selection takes precedence
over the artifact argument, `--change`, and `--schema`; `--json` is inert in
this mode and the output remains raw Markdown. An unknown name writes
`Error: Unknown skill: <name>` to stderr, writes nothing to stdout, and exits
1.

The bodies under `crates/spectra-core/assets/skills/` are byte-exact captures
from the Spectra 2.3.1 oracle. The generated
`docs/reverse-engineering/golden/skills-2.3.1.tsv` manifest pins their oracle
provenance by byte length and SHA-256 digest. Both the assets and manifest are
generated artifacts and must never be hand-edited. By default,
`scripts/capture-skills.py` re-captures the bodies and verifies both; its
explicit `--write` mode regenerates both and then re-verifies them.

### Preflight (recovered by disassembly + behaviour matrix)

- `missingFiles` (`status: critical`): file refs from the proposal's
  "Affected code:" section only (marker also matches the Chinese variants
  `主要檔案`/`影響檔案`/`變更檔案`/`受影響檔案`; scan stops at the next
  heading). Backticked refs match
  `` `([^`]*?/[^`]*?\.(?:rs|ts|tsx|jsx|svelte|md|json|yaml|toml|css|html|js))` ``;
  bare lines must fully match the prefix-whitelisted
  `\b((?:specs|src|src-tauri|crates|lib|tests|app|public)/[\w\-/]+\.(?:…same extensions…))\b`.
- `driftedFiles` (`status: warnings`): backtick refs across
  proposal+design+tasks that exist on disk and whose
  `git log -1 --format=%cs -- <path>` date is strictly later than the
  change's `created` date.
- `staleness`: `daysOld` = today − `created` (negative values are not
  clamped); `isStale` = `daysOld > 7`; the whole key vanishes when
  `created` is missing/unparseable.

### Deliberate divergences (documented, not bugs)

- `contextFiles` key order fixed to schema order (oracle nondeterministic).
- The oracle's multiple-active-changes error (`Use --change to specify
  one:`, mtime-ordered) differs from OpenSpectra's pre-existing
  `change::resolve` wording (alphabetical); unification is tracked in
  issue #50 (operator ruling pending).
- **Fresh design scaffold vs `drift`** — RESOLVED (#51). The 2026-07-18 note
  here recorded the oracle reporting `Structure 0/20 anchors broken` on a
  template-only `design.md` while OpenSpectra marked ~20 prose Symbol anchors
  broken. Re-probed head-to-head on 2026-08-03, both binaries in the *same*
  jail: they agree on the resolved/broken *pattern* in both states, differing
  by one anchor — `0/20` vs `0/21` with `design.md` tracked
  (`git grep` self-matches the template), and `20/20` vs `21/21` with it
  untracked. The original comparison had the two binaries on different sides of
  that tracked/untracked split, which is what produced the apparent 0-vs-20 gap.
  The single real divergence *on the scaffold's tokens* was the `JSON`
  stop-list entry, now added, taking the scaffold to an exact `0/20` / `20/20`
  match. Pinned by
  `anchors::tests::design_template_symbols_match_the_oracle_stoplist`; the
  byte-pinned template is untouched. (The stop-list had further gaps on tokens
  the template does not use — 20 more entries recovered by the 2026-08-06
  sweep; see `drift.md`'s "Symbol stop-list sweep (#133)".)
