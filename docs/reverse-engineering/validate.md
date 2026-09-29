# OpenSpec-compatible `spectra validate`

OpenSpectra's validator rules are not reverse-engineered from the closed
Spectra binary: their authority is `@fission-ai/openspec` 1.13.2 (owner ruling
D1, below). Its **output** has two shapes (ruling D3, W9a): the oracle 3.0.0
shape, the default, and OpenSpec 1.13.2's own `--json` report under
`--format openspec`. How OpenSpec's findings are shown in the oracle shape is
owner ruling D12 (see "Output formats").

## CLI

```text
spectra validate [ITEM] [--type change|spec] [--strict] [--json [--format oracle|openspec]]
spectra validate [--changes] [--specs] [--all] [--strict] [--report full|findings] [--json [--format …]]
spectra validate --archived [--report full|findings] [--json [--format …]]
```

`ITEM` auto-detects a change or canonical spec. `--type` resolves ambiguous
names. `--archived` checks task completion only; archived deltas have already
been applied. `--format` requires `--json`.

Scopes (rulings D12-3, D12-5, W9a-1, W9a-3):

| invocation | validates | oracle 3.0.0 |
|---|---|---|
| `validate` | every active change | same |
| `validate --changes` | every active change | same |
| `validate --specs` | every main spec | same |
| `validate --all` | every change **and** every spec | changes only (deliberate divergence) |
| `validate --changes --specs` | every change and every spec | specs only (deliberate divergence) |
| `validate ITEM` | ITEM as a change or a spec | change only (`Error: Change 'x' not found.` for a spec) |
| `validate ITEM` naming both a change and a spec | the change (`--type spec` for the spec); with `--json --format openspec`, OpenSpec's `ambiguous_item` status (exit 1) | validates the change |
| `validate ITEM --all` | ITEM only (`--all` ignored) | same |

The oracle's `--all`/`--changes --specs` behavior reads as a bug and no
consumer relies on it (the embedded skills only call `spectra validate
"<name>"`), so OpenSpec's meaning was ruled in (D12-3). A bare `validate`
follows the oracle rather than OpenSpec (which prints a hint and exits 1 when
not interactive; D12-5).

A name shared by a change and a spec (owner ruling W9a-1, 2026-09-29): the
oracle 3.0.0 validates the change (p09: `validate only` printed the change's
result), OpenSpec 1.13.2 refuses it as ambiguous (pinned by the `only`
envelope in `golden/validate-openspec-1.13.2.json`, human output likewise in
p10). Each format follows its own authority: the default oracle format (human
and `--json`) takes the change, so the embedded skills' `spectra validate
"<name>"` never fails on a collision; `--json --format openspec` prints
OpenSpec's `ambiguous_item` status. `ITEM --all` ignores `--all` in both
formats, as the oracle does (W9a-3; OpenSpec would instead run the bulk scope
and ignore ITEM — not followed).

## Rule authority (owner ruling D1)

Which findings exist, their level, the per-item verdict, and the message
wording follow OpenSpec **1.13.2** (`@fission-ai/openspec`,
`dist/core/validation/validator.js`), ported rule for rule in
`crates/spectra-core/src/validate.rs` with its Markdown reading in
`crates/spectra-core/src/openspec_md.rs`. What OpenSpec runs:

- a change: only `Validator.validateChangeDeltaSpecs(changeDir, {mainSpecsDir,
  projectRoot})`; `proposal.md` is never read;
- a main spec: `Validator.validateSpec(file)`.

The expected output of every rule is pinned by running OpenSpec itself, not by
reading its source: `scripts/capture-validate-openspec.py` runs OpenSpec 1.13.2
on the fixture project `crates/spectra-cli/tests/fixtures/validate_openspec`
(one `dNN-*` change or `sNN-*` spec per rule; `d01`–`d30`/`s01`–`s20` are the
W9 probe p08, `d31`–`d45`/`s21`–`s30` were added for the remaining rules) and
writes `golden/validate-openspec-1.13.2.json`;
`crates/spectra-cli/tests/validate_openspec_integration.rs` replays it against
`spectra validate --changes|--specs --json [--strict]`, comparing `valid` and
every issue field (`level`, `path`, `line`, `message`, in order).

The Markdown reader is deliberately **not** shared with `archive`
(`crate::markdown`, oracle behavior): OpenSpec reads stray `###` headers,
repeated sections, and `#####` scenarios differently, and archive's refusal
rules must not move when validate's rules do. Archive still refuses everything
it refused before.

JavaScript semantics kept by the port: lengths are UTF-16 code units (JS
`.length`), so a CJK Purpose is measured as OpenSpec measures it; `\b(SHALL|MUST)\b`
uses ASCII word boundaries, so `系統SHALL記錄` contains SHALL (Rust's default
Unicode `\b` would not match there).

## Change rules

Delta files are every `spec.md` at any depth under `changes/<name>/specs/`
(not the `specs/` root), sorted by path; dot entries are skipped and symlinked
directories are not followed. Issue paths are relative to the change's
`specs/` (`auth/spec.md`, `Epic/Feature/spec.md`); change-level findings use
the path `file`; `tasks.md` findings use the task file's path relative to the
change.

Delta parsing (`parseDeltaSpec`): sections are `^## ` lines (column 0); the
four delta sections are matched case-insensitively and **repeated sections are
merged**. A requirement is `^###\s*Requirement:\s*(.+)$` (column 0) and its
block runs to the next `### Requirement:` or `## ` header, so a stray `###`
inside it stays part of the block. REMOVED also accepts a bullet carrying the
header (`-`, `*`, `+`). RENAMED pairs a `FROM:` with the next `TO:`.

Findings, in OpenSpec's order:

| # | trigger | level | message (abridged) |
|---|---|---|---|
| C11 | `specs/spec.md` is a file | ERROR `spec.md` | `Delta spec found at specs/spec.md. Delta specs must live under a capability path …` |
| C4 | a non-`Requirement:` `###` inside ADDED/MODIFIED | INFO + line | `Header "### Notes" in ADDED Requirements is not a "### Requirement:" header and is ignored by validation. …` |
| C8 | `### Requirement:` with no name | INFO + line | `Header "### Requirement:" in ADDED Requirements is missing a requirement name …` |
| C8 | `FROM:` without `TO:` (or `TO:` without `FROM:`) | ERROR + line | `RENAMED FROM: "Alpha" has no matching TO: line. …` |
| C10 | a requirement outside the delta sections | WARNING + line | `Requirement "Alpha" is under "## Notes", which is not a delta section, so it is ignored. …` |
| C6 | a name twice in one section | ERROR | `Duplicate requirement in ADDED: "Alpha"` (also MODIFIED/REMOVED, `Duplicate FROM/TO in RENAMED`) |
| C16 | ADDED/MODIFIED without body text | ERROR | `ADDED "Alpha" is missing requirement text`, or, when SHALL/MUST is only in the header, `… must contain SHALL or MUST in the requirement body, not only in the header. …` |
| C16 | body without SHALL/MUST | WARNING | `ADDED "Alpha" should contain SHALL or MUST (RFC 2119 best practice for English specs)` |
| C5/C17 | no `#### ` child **with a body** (`#####` does not count) | ERROR | `ADDED "Alpha" must include at least one scenario` (+ a hint when an empty `####` exists) |
| C3 | MODIFIED omits a scenario the main spec has (after rename mapping) | ERROR | `MODIFIED "Alpha" omits scenario(s) the current spec still has: "a2". The modified block has 1 scenario; the current spec has 2 scenarios. It adds none. Copy the omitted scenarios …` — once per requirement, and never repeated by the archive check (#183: pinned by `validate_rejects_a_modified_requirement_that_drops_a_current_scenario`, the rename-chain test, and mutation cases `183-*`) |
| C3 | the main spec exists but cannot be read (EACCES/EISDIR) | ERROR | `Could not read <abs path> to check the MODIFIED requirements against it (EACCES). …` |
| C6 | conflicts across sections of one file | ERROR | `Requirement present in both MODIFIED and REMOVED: "Alpha"`, `… ADDED and REMOVED`, `MODIFIED references old name from RENAMED. …`, `RENAMED TO collides with ADDED …`, `Requirement present in both RENAMED and REMOVED …` — one per conflict; the rest of the file is still validated |
| C1/C13 | archive's merge would refuse the delta | INFO | `Archive would refuse this delta: <cap> MODIFIED failed for header "### Requirement: Nope" - not found` (also ADDED already exists, RENAMED source missing, case/spacing near-misses, missing target spec for MODIFIED/RENAMED, structurally invalid target spec) — first failure per file, never for a file that already has an ERROR |
| C2 | REMOVED target missing | — | nothing (archive treats it as already removed) |
| C12 | a `.md` other than `spec.md` under `specs/` with a delta section | ERROR | `Delta spec found at specs/x.md. … Move its requirements into specs/x/spec.md.` |
| C8 | delta sections present, no entries parsed | ERROR | `Delta sections ## ADDED Requirements were found, but no requirement entries parsed. …` |
| C9 | a `spec.md` with no delta section | ERROR | `No delta sections found. Add headers such as "## ADDED Requirements" …` |
| C15 | `skip_specs: true` with any non-dot file under `specs/` | ERROR `file` | `skip_specs is set in .openspec.yaml but spec files exist under specs/. …` |
| C14 | no delta at all (and no C11/C12 finding) | ERROR `file` | `Change must have at least one delta. No deltas found. Ensure your change has a specs/ directory …` |
| C15 | `skip_specs: true` and no delta | INFO `file` | `skip_specs is set in .openspec.yaml: change declares no spec-level behavior changes, zero deltas accepted` |
| C19 | the schema's tracked task files hold no checkbox | WARNING + line | `This change counts as 0 tasks: …` |
| C18 | duplicate task ID / task under the wrong `## N.` group | WARNING + line | `Task ID "1.1" is duplicated; it was first declared on line 3.` / `Task "2.1" is under group 1, but its leading number points to group 2. …` — **only** when the change resolves to the built-in `spec-driven` schema (a project schema of the same name does not count) |

Every `WARNING` fails under `--strict`; `INFO` never fails.

### OpenSpectra-only finding (C20, kept as ERROR by owner ruling D12-6)

`validate` still runs archive's own dry run (`archive::validate_archive_compatibility`)
and reports, as an `ERROR` at `changes/<name>`, only the refusals OpenSpec has no
rule for: an unrecognized `<!-- @trace` footer that MODIFIED/REMOVED would
discard, a corrupt or missing trace sidecar, and capability retirement
(`retire_capabilities`). They are marked `archive::OpenSpectraOnlyRefusal`.
Every other archive failure is left to the OpenSpec INFO above. The dry run
stops at the first failure, so a merge conflict earlier in the same run can
hide one of these refusals (as before W9b).

Not ported: OpenSpec's invalid-`.openspec.yaml` marker error
(`CHANGE_SKIP_SPECS_INVALID_METADATA`; OpenSpectra fails to load such a change
instead), nested-change detection, and glob `apply.tracks` patterns (a literal
tracked path is supported, which covers the built-in schemas).

## Main spec rules

OpenSpec builds a heading tree (`MarkdownParser.parseSpec`): **Purpose and
Requirements are found depth-first at any heading level** (`### Purpose` is
fine), **every child heading of Requirements is a requirement** (`### Admin
Portal` included), and a requirement's scenarios are its child headings with a
non-empty body (a `#####` counts when no `####` sits between; an empty `####`
does not). A missing or empty Purpose, then a missing Requirements, is the
only finding reported for that spec (path `file`, message plus the
"Missing required sections" guide).

Otherwise, in order:

| # | trigger | level, path | message (abridged) |
|---|---|---|---|
| S8 | Requirements has no child heading | ERROR `requirements` | `Spec must have at least one requirement` |
| S5 | a requirement without a scenario | ERROR `requirements.<i>.scenarios` **and** WARNING `requirements[<i>].scenarios` | `Requirement must have at least one scenario` / `… Scenarios must use level-4 headers. …` |
| S2 | `## ADDED/MODIFIED/REMOVED/RENAMED Requirements` in a main spec | ERROR `file` + line | `Main spec contains delta header "## MODIFIED Requirements". …` |
| S3 | `### Requirement:` outside the first `## Requirements` | ERROR `file` + line | `Requirement header "### Requirement: Beta" appears outside the main ## Requirements section. …` |
| S4 | the same requirement header twice | ERROR `file` + line | `Requirement header "### Requirement: Alpha" duplicates the requirement declared on line 9. …` |
| S10 | Purpose is a placeholder (leading `TBD`/`TODO` shouted, or in any case followed by punctuation/EOL; or archive's generated sentence) | WARNING `overview` (+ line) | `Purpose section is still a placeholder rather than a Purpose anyone wrote …` |
| S9 | otherwise, Purpose shorter than 50 UTF-16 units | WARNING `overview` | `Purpose section is too brief (less than 50 characters)` |
| S11 | requirement text longer than 500 UTF-16 units | INFO `requirements[<i>]` | `Requirement text is very long (>500 characters). Consider breaking it down.` |
| S12 | a `### Requirement:` block (first Requirements section) without body text / without SHALL or MUST | ERROR / WARNING `requirements[<i>]` | `Requirement "Alpha" must contain SHALL or MUST` / `… should contain SHALL or MUST (RFC 2119 best practice for English specs)` |

S13: a nameless `### Requirement:` is simply a requirement titled
`Requirement:` and produces no finding of its own.

### Corpus impact (D1 scope note)

Measured on the W9 corpus (2026-09-28): with these rules OpenSpectra's
per-item verdict equals OpenSpec 1.13.2's for every item of yibi-mvp (21
changes, 123 specs), nextrek-cli (1, 2), and yibi-stack (9, 16), both normal
and `--strict`. Following OpenSpec makes **five yibi-mvp main specs invalid
that OpenSpectra 0.13 passed** — `E07-content-cms-admin` (S1/S5: plain `###`
requirements without scenarios), `E08-content-browsing-app/F030-content-library`
(S3), `E14-ai-interactive/F019-little-know-it-all` and
`E20-growth-insights/F022-growth-report` (S2/S3), `process-development-dod`
(S2/S3). Under `--strict` the yibi-mvp spec failures go from 42 to 49: the
same five minus the two already strict-invalid (F019, F022), plus
`E08-content-browsing-app`, `E12-device-interactive`, `E19-sleep-routine`, and
`E22-subscription` on Purpose brevity (S9). Change
`0046` becomes valid (its archive conflicts are INFO, C1), and change 0098 no
longer gets the false "missing scenario" (C4). This pulls OpenSpectra further
from the oracle 3.0.0, which flags none of these specs; D1 accepts that.

## Archived changes

`--archived` fails when an archived change still has pending tasks.
"Pending" uses the shared task rule (`tasks::parse`, see `task.md`): any
`-`/`*`/`+` checkbox with a non-blank description whose marker is not
`x`/`X`, so `[~]`/`[-]` count as pending. `archive --mark-tasks-complete`
leaves those markers in place, so such a change fails here after archive.
The oracle rejects `--archived` (clap error, rc 2), so this is not
oracle-verified. OpenSpec 1.13.1 (#1761) likewise counts unrecognized
markers as incomplete, but also counts ordered-list checkboxes
(`1. [ ]`), which the shared rule does not.

## Output formats (rulings D3, D12; W9a)

Both shapes are built from the same findings (the rules above); only the
presentation differs. The oracle shape is the default for human output and
for `--json`; `--json --format openspec` switches the JSON to OpenSpec's.
Human output is always the oracle shape.

### Oracle shape (default)

Measured on oracle 3.0.0 (W9 RE spec A2–A4, probes p06/p08/p09):

```text
✓ <name> — valid
✗ <name> — invalid
  error: <message>
  warn: <message>
```

- `✓` U+2713, `✗` U+2717, `—` U+2014; no header, no blank lines, no summary.
  Every `error:` line comes before every `warn:` line. A valid item can carry
  `warn:` lines.
- On a terminal only the glyph (green `32` / red `31`) and the label
  (`error:` red, `warn:` yellow `33`) are colored; `--no-color`, `NO_COLOR`,
  and a non-terminal stdout disable color.
- When any item is invalid, stderr gets `Error: Validation failed.` after
  stdout, and the exit code is 1. An empty scope prints nothing (exit 0).
- An unknown item: stdout empty, stderr `Error: Change '<item>' not found.`,
  exit 1 (also with `--json`).
- `--json`: a pretty-printed array, keys alphabetical, the same items as the
  human output: `{"change": <name>, "errors": [...], "valid": …, "warnings":
  [...]}` or `{"errors": [...], "spec": <name>, "valid": …, "warnings": [...]}`.
  Errors and warnings are plain strings. An empty scope is `[]`.
- Order: changes in `list --json` order (latest file mtime, newest first),
  then specs by id in byte order. Only `--all`/`--changes --specs` put both in
  one array (changes first).

How OpenSpec's findings fill it (ruling D12):

| OpenSpec finding | oracle shape |
|---|---|
| ERROR | `errors` |
| WARNING | `warnings` |
| INFO `Archive would refuse this delta: …` (C1/C13) | `warnings` (D12-1; the oracle also warns in this case) |
| any other INFO (stray `###` header, nameless requirement, `skip_specs` accepted, long requirement text) | not shown (D12-1) |
| OpenSpectra-only archive refusal (C20) | `errors` (D12-6) |

Messages are OpenSpec's wording (D12-2), not the oracle's (the oracle has its
own, e.g. `Parse error: Invalid format: …`; only its format is copied). A
finding in a delta file — any finding whose OpenSpec path is relative to the
change's `specs/`: `<cap>/spec.md`, the root `spec.md` (C11), a non-`spec.md`
delta file (C12) — is prefixed `specs/<path>: `, as the oracle prefixes its
delta findings. Change-level findings (path `file`), task-file findings, the
C20 errors and every main-spec finding are not prefixed. `line` is dropped.
The verdict is the item's own, so under `--strict` an item with only
warnings is `✗ … — invalid` (the oracle has no `--strict`).

`--report findings` in the oracle shape keeps the items that print at least
one `error:`/`warn:` line; the exit code still reflects the whole run. This
is an OpenSpectra design (owner ruling W9a-2, 2026-09-29): the oracle has no
`--report`, and OpenSpec's findings report (which also keeps INFO-only items)
exists only in the OpenSpec shape.

### OpenSpec shape (`--json --format openspec`)

Field for field OpenSpec 1.13.2 (`commands/validate.js`), pinned by the
`envelopes` and `all_order` sections of `golden/validate-openspec-1.13.2.json`
(fixtures `validate_openspec_envelope`, `validate_openspec_empty`, and the rule
fixture) and replayed by `validate_openspec_integration.rs`:

```json
{
  "items": [
    { "id": "zeta", "type": "change", "valid": true, "issues": [], "durationMs": 0 }
  ],
  "summary": {
    "totals": { "items": 1, "passed": 1, "failed": 0 },
    "byType": { "change": { "items": 1, "passed": 1, "failed": 0 } }
  },
  "version": "1.0",
  "root": { "path": "/project", "source": "nearest" }
}
```

- Issue keys `level, path, line, message` (`line` only when grounded).
- `byType` has one entry per requested type, even with no items (`--all` on
  an empty project lists `change` and `spec` at zero).
- Items are sorted like OpenSpec's `a.id.localeCompare(b.id)` under Node's ICU
  (CLDR root, punctuation non-ignorable): punctuation, digits, then letters
  case-insensitively, case breaking ties lowercase first — `a-b, a.b, a/b, aa,
  alpha, B-upper, only, parentless/child, Upper_Case, zeta`. Exact for
  printable ASCII; other characters sort after `z` by code point (an
  approximation: no corpus has non-ASCII ids). A change and a spec with the
  same id keep change-first; OpenSpec orders that tie by async completion,
  so the golden normalizes it the same way.
- `root.source` is always `nearest` (OpenSpectra has no store roots).
- `--report findings` gives OpenSpec's `{report: {kind, version, scope,
  returnedItems, totalItems}, itemFindings, summary, root}`, items with any
  finding (INFO included); `--changes --specs` has scope `all`.
- An unknown item prints `{"status": [{"severity": "error", "code":
  "unknown_item", "message": "Unknown item 'x'. Did you mean: …?"}]}` to
  stdout (exit 1); the suggestions are the five nearest ids by Levenshtein
  distance over UTF-16 units, active changes (by name) then specs, ties in
  that order. An ambiguous item prints the `ambiguous_item` status with
  `"fix": "Pass --type change|spec."`.
- Deliberately not followed (rulings): a bare `validate` validates every
  change instead of printing OpenSpec's hint (D12-5); `ITEM --all` validates
  ITEM instead of the bulk scope (W9a-3). `durationMs` is OpenSpectra's own
  timing.
- **Known gaps** (owner ruling W9a-4: not fixed in W9a, tracked separately):
  1. `--type change|spec` on a name that is not of that type is still
     `Error: Change 'x' not found.` (or `Spec`) on stderr; OpenSpec validates
     the missing path anyway and reports its findings.
  2. The `--archived` message wording is OpenSpectra's
     (`N incomplete archived task(s)`); OpenSpec says `N incomplete tasks
     (c/t completed)` and counts tasks differently (see "Archived changes").
  3. Item order is exact only for printable-ASCII ids; non-ASCII ids use the
     approximation above instead of ICU collation.

The previous OpenSpectra v2 JSON (`version: "2.0"`, `totals.total`,
`root.spec_dir`) is gone; `summary.totals.failed` and the item fields a gate
reads are in the OpenSpec shape too.

## Exit status

- 0: every selected item is valid under the selected strictness.
- 1: at least one item is invalid, or an operational error occurred.

Unlike `drift`, validation severity intentionally controls the exit status.
Exact diagnostic wording is not a compatibility contract; consumers should use
level, valid, path, line, and summary fields.
