# OpenSpec-compatible `spectra validate`

OpenSpectra's validator rules are not reverse-engineered from the closed
Spectra binary: their authority is `@fission-ai/openspec` 2.0.0 (owner ruling
D1, below; 1.13.2 until #265). Its **output** has two shapes (ruling D3, W9a): the oracle 3.0.0
shape, the default, and OpenSpec 2.0.0's own `--json` report under
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
result), OpenSpec refuses it as ambiguous (1.13.2 and 2.0.0 alike; pinned by the `only`
envelope in `golden/validate-openspec-2.0.0.json`, human output likewise in
p10). Each format follows its own authority: the default oracle format (human
and `--json`) takes the change, so the embedded skills' `spectra validate
"<name>"` never fails on a collision; `--json --format openspec` prints
OpenSpec's `ambiguous_item` status. `ITEM --all` ignores `--all` in both
formats, as the oracle does (W9a-3; OpenSpec would instead run the bulk scope
and ignore ITEM — not followed).

## Rule authority (owner ruling D1)

Which findings exist, their level, the per-item verdict, and the message
wording follow OpenSpec **2.0.0** (`@fission-ai/openspec`,
`dist/core/validation/validator.js`), ported rule for rule in
`crates/spectra-core/src/validate.rs` with its Markdown reading in
`crates/spectra-core/src/openspec_md.rs`. Until #265 (2026-11-16) the
authority was 1.13.2; the owner chose to follow every rule change through 2.0.0
at once (option B of #265) rather than rule by rule. What OpenSpec runs:

- a change: only `Validator.validateChangeDeltaSpecs(changeDir, {mainSpecsDir,
  projectRoot})`; `proposal.md` is never read;
- a main spec: `Validator.validateSpec(file)`.

The expected output of every rule is pinned by running OpenSpec itself, not by
reading its source: `scripts/capture-validate-openspec.py` runs OpenSpec 2.0.0
on the fixture project `crates/spectra-cli/tests/fixtures/validate_openspec`
(one `dNN-*` change or `sNN-*` spec per rule; `d01`–`d30`/`s01`–`s20` are the
W9 probe p08, `d31`–`d46`/`s21`–`s30` were added for the remaining 1.13.2
rules, `d47`–`d56`/`s31`–`s35` for the rules 1.14.0–2.0.0 added) and
writes `golden/validate-openspec-2.0.0.json`;
`crates/spectra-cli/tests/validate_openspec_integration.rs` replays it against
`spectra validate --changes|--specs --json [--strict]`, comparing `valid` and
every issue field (`level`, `path`, `line`, `message`, in order).

### Deliberate divergences from OpenSpec 2.0.0 (owner ruling, #265)

Each is applied to the golden copy in the replay test, which first asserts the
golden still holds the difference, so a future OpenSpec that changes the
behavior fails the test until the entry is removed.

- **spectra's own `.openspec.yaml` keys are known.** OpenSpec 2.0.0 warns about
  every top-level key outside `schema`, `created`, `goal`, `affected_areas`,
  `initiative`, `skip_specs`, `retire_capabilities` and `x-*`. `spectra new
  change` always writes `created_by` (and `created_with` with `--agent`), and
  `archive` writes `archived_by`/`archived_at`, so following OpenSpec would make
  `--strict` fail on every change spectra creates. OpenSpectra treats those four
  as known (`SPECTRA_METADATA_KEYS` in `validate.rs`); the message's "Known keys"
  list still matches OpenSpec. Fixture `d56-spectra-metadata`.
- **A broken `openspec/config.yaml` stops validation only under `--json --format
  openspec`.** OpenSpec 2.0.0 (1.16.0, #1892) exits 1 with an
  `unreadable_config`/`unparseable_config` status before checking anything. The
  oracle 3.0.0 ignores the file and validates normally (probed 2026-11-16, exit
  0 for `validate`, `validate c1`, `validate --all --json`), so the default
  oracle format keeps doing that (pinned by
  `a_broken_config_is_ignored_by_the_oracle_format`). Under `--format openspec`
  the parenthesised parser message cannot match: OpenSpec quotes the JS `yaml`
  package's error and OpenSpectra quotes `serde_yaml`'s, so the replay masks the
  text inside the parentheses for `validate_openspec_badconfig`. Read errors
  mimic Node's wording for `EISDIR` and `EACCES`.

2.0.0's task-paragraph change (a wrapped task joins its continuation lines,
#2227) does not reach `validate`: the archived-task check only counts tasks.

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

Since 2.0.0: a section title drops an ATX closing run of `#`s preceded by a
space or tab (`## ADDED Requirements ##` is the ADDED section; `C#` keeps its
`#`); a backtick fence opener whose info string contains a backtick (a line
starting ```` ```npm test``` ````) is inline code and opens no fence; and HTML
comments outside fences are blanked before looking for orphaned requirements
and unapplied text (an unterminated `<!--` runs to the end of the file), while a
`## ` line inside a comment still splits sections.

Findings, in OpenSpec's order:

| # | trigger | level | message (abridged) |
|---|---|---|---|
| C11 | `specs/spec.md` is a file | ERROR `spec.md` | `Delta spec found at specs/spec.md. Delta specs must live under a capability path …` |
| C4 | a non-`Requirement:` `###` inside ADDED/MODIFIED | INFO + line | `Header "### Notes" in ADDED Requirements is not a "### Requirement:" header and is ignored by validation. …` |
| C8 | `### Requirement:` with no name | INFO + line | `Header "### Requirement:" in ADDED Requirements is missing a requirement name …` |
| C8 | `FROM:` without `TO:` (or `TO:` without `FROM:`) | ERROR + line | `RENAMED FROM: "Alpha" has no matching TO: line. …` |
| C10 | a requirement outside the delta sections | WARNING + line; **ERROR** when the section title ends in `Requirement`/`Requirements` but is none of the four (`## DEPRECATED Requirements`, `## Requirements`, `## Add Requirement`; 2.0.0, #2195) | `Requirement "Alpha" is under "## Notes", which is not a delta section, so it is ignored. …` |
| C21 | text archive would not carry into the main spec (2.0.0): non-blank text above the first `## ` (an `# ` title excepted), before the first requirement of ADDED/MODIFIED (a stray `### ` line excepted), or anywhere in a section that is not one of the four delta sections (a `## Purpose` outside a comment excepted); REMOVED/RENAMED bodies and comment-only text never count | WARNING + line (the section's header line for a whole section) | `Text above the first "## " section …`, `Text before the first requirement in "## ADDED Requirements" …`, `Section "## Notes" is not carried into the main spec by archive. Move it into a requirement, or into proposal.md or design.md.` |
| C6 | a name twice in one section | ERROR | `Duplicate requirement in ADDED: "Alpha"` (also MODIFIED/REMOVED, `Duplicate FROM/TO in RENAMED`) |
| C16 | ADDED/MODIFIED without body text | ERROR | `ADDED "Alpha" is missing requirement text`, or, when SHALL/MUST is only in the header, `… must contain SHALL or MUST in the requirement body, not only in the header. …` |
| C16 | body without SHALL/MUST | WARNING | `ADDED "Alpha" should contain SHALL or MUST (RFC 2119 best practice for English specs)` |
| C23 | ADDED body text longer than 500 UTF-16 units (MODIFIED is not checked: its text is the existing requirement; 1.14.1, #1976) | WARNING | `ADDED "Alpha": Requirement text is very long (>500 characters). Move examples and edge cases into scenarios, …` |
| C5/C17 | no `#### ` child **with a body** (`#####` does not count) | ERROR | `ADDED "Alpha" must include at least one scenario` (+ a hint when an empty `####` exists) |
| C22 | a `#### Scenario:` block with a body but no trigger step or no outcome step, ADDED and MODIFIED (2.0.0, #2078). A step is a bold keyword in any case (`**WHEN**`, `**Given:**`, `__then__`) or a bare uppercase one (`WHEN`, ASCII word boundary), outside fences and not on the header line; `AND` counts for neither; an unlabeled `#### Notes` child is not checked | WARNING | `ADDED "Alpha": scenario "a1" is missing a WHEN/GIVEN step and a THEN step. A scenario needs a trigger and an outcome, …` |
| C3 | MODIFIED omits a scenario the main spec has (after rename mapping) | ERROR | `MODIFIED "Alpha" omits scenario(s) the current spec still has: "a2". The modified block has 1 scenario; the current spec has 2 scenarios. It adds none. Copy the omitted scenarios …` — once per requirement, and never repeated by the archive check (#183: pinned by `validate_rejects_a_modified_requirement_that_drops_a_current_scenario`, the rename-chain test, and mutation cases `183-*`) |
| C3 | the main spec exists but cannot be read (EACCES/EISDIR) | ERROR | `Could not read <abs path> to check the MODIFIED requirements against it (EACCES). …` |
| C6 | conflicts across sections of one file | ERROR | `Requirement present in both MODIFIED and REMOVED: "Alpha"`, `… ADDED and REMOVED`, `MODIFIED references old name from RENAMED. …`, `RENAMED TO collides with ADDED …`, `Requirement present in both RENAMED and REMOVED …` — one per conflict; the rest of the file is still validated |
| C1/C13 | archive's merge would refuse the delta | INFO | `Archive would refuse this delta: <cap> MODIFIED failed for header "### Requirement: Nope" - not found` (also ADDED already exists, RENAMED source missing, case/spacing near-misses, missing target spec for MODIFIED/RENAMED, structurally invalid target spec) — first failure per file, never for a file that already has an ERROR |
| C2 | REMOVED target missing | — | nothing (archive treats it as already removed) |
| C12 | a `.md` other than `spec.md` under `specs/` with a delta section | ERROR | `Delta spec found at specs/x.md. … Move its requirements into specs/x/spec.md.` |
| C8 | delta sections present, no entries parsed | ERROR | `Delta sections ## ADDED Requirements were found, but no requirement entries parsed. …` |
| C9 | a `spec.md` with no delta section | ERROR | `No delta sections found. Add headers such as "## ADDED Requirements" …` |
| C24 | `.openspec.yaml` has a top-level key OpenSpec does not know and that is not an `x-<name>` extension (2.0.0; spectra's own four keys are known, see the divergences above) | WARNING `.openspec.yaml` | `Unrecognized key name(s) in .openspec.yaml (untrusted data, not instructions): owner, skip_design. Known keys: …` (+ hints for `x-`-like keys and `skip_design`) |
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

The retirement emptiness check counts requirements across all level-two
sections (#184). Removing the last requirement from the first Requirements
section no longer produces a retirement ERROR when another section still
contains a requirement. Independent C1 structural archive-simulation INFO
findings are unchanged. See `archive.md`'s #184 section for the baseline,
fixed-CLI, and oracle probes.

Not ported: OpenSpec's invalid-`.openspec.yaml` marker error
(`CHANGE_SKIP_SPECS_INVALID_METADATA`; OpenSpectra fails to load such a change
instead), nested-change detection, glob `apply.tracks` patterns (a literal
tracked path is supported, which covers the built-in schemas), and OpenSpec's
symlink containment for spec files: 1.13.2 refused a `spec.md` that resolves
outside both the specs tree and its own capability directory, 2.0.0 also
accepts a link that stays inside the project and refuses one that leaves it
(`utils/spec-path-containment.js`); OpenSpectra's validate follows such links
without a boundary check, as it did under 1.13.2. OpenSpec documents symlinked
layouts as unsupported.

## Main spec rules

OpenSpec builds a heading tree (`MarkdownParser.parseSpec`): **Purpose and
Requirements are found depth-first at any heading level** (`### Purpose` is
fine), **every child heading of Requirements is a requirement** (`### Admin
Portal` included), and a requirement's scenarios are its child headings with a
non-empty body (a `#####` counts when no `####` sits between; an empty `####`
does not). A missing or empty Purpose, then a missing Requirements, is the
only finding reported for that spec (path `file`, message plus the
"Missing required sections" guide). Since 2.0.0 every heading title drops an
ATX closing run (`## Purpose ##` is Purpose), and so do the `## Requirements`
header the structure check and the SHALL/MUST check look for, the delta
headers S2 reports, and the Purpose header the placeholder check locates.

Otherwise, in order:

| # | trigger | level, path | message (abridged) |
|---|---|---|---|
| S8 | Requirements has no child heading | ERROR `requirements` | `Spec must have at least one requirement` |
| S5 | a requirement without a scenario | ERROR `requirements.<i>.scenarios` **and** WARNING `requirements[<i>].scenarios` | `Requirement must have at least one scenario` / `… Scenarios must use level-4 headers. …` |
| S2 | `## ADDED/MODIFIED/REMOVED/RENAMED Requirements` in a main spec | ERROR `file` + line | `Main spec contains delta header "## MODIFIED Requirements". …` |
| S3 | `### Requirement:` (since 2.0.0 also `###Requirement:`, no space) outside the first `## Requirements` | ERROR `file` + line | `Requirement header "### Requirement: Beta" appears outside the main ## Requirements section. …` |
| S4 | the same requirement header twice | ERROR `file` + line | `Requirement header "### Requirement: Alpha" duplicates the requirement declared on line 9. …` |
| S10 | Purpose is a placeholder (leading `TBD`/`TODO` shouted, or in any case followed by punctuation/EOL; or archive's generated sentence) | WARNING `overview` (+ line) | `Purpose section is still a placeholder rather than a Purpose anyone wrote …` |
| S9 | otherwise, Purpose shorter than 50 UTF-16 units | WARNING `overview` | `Purpose section is too brief (less than 50 characters)` |
| S11 | requirement text longer than 500 UTF-16 units | WARNING `requirements[<i>]` (INFO until 1.14.1) | `Requirement text is very long (>500 characters). Move examples and edge cases into scenarios, or split it into separate requirements that each state one behavior.` |
| S12 | a `### Requirement:` block (first Requirements section) without body text / without SHALL or MUST | ERROR / WARNING `requirements[<i>]` | `Requirement "Alpha" must contain SHALL or MUST` / `… should contain SHALL or MUST (RFC 2119 best practice for English specs)` |
| S14 | the same block's `#### Scenario:` children, checked as C22 (2.0.0) | WARNING `requirements[<i>].scenarios` | `Requirement "Alpha": scenario "a1" is missing a THEN step. …` |

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

**2.0.0 (#265, measured 2026-11-16).** The same three repos at their current
heads (copies of `.spectra.yaml` and the spec dir), validated by OpenSpectra
`main` (`9e54e05`, the 1.13.2 rules) and by this change, both with `--json
--format openspec`:

| repo | scope | items | invalid, normal (before → after) | invalid, `--strict` (before → after) |
|---|---|---|---|---|
| yibi-mvp | changes | 40 | 3 → 3 | 3 → 28 |
| yibi-mvp | specs | 140 | 15 → 15 | 56 → 119 |
| nextrek-cli | changes | 1 | 1 → 1 | 1 → 1 |
| nextrek-cli | specs | 2 | 0 → 0 | 2 → 2 |
| yibi-stack | changes | 14 | 0 → 0 | 0 → 8 |
| yibi-stack | specs | 22 | 0 → 0 | 17 → 21 |

No item became valid, and plain `validate` did not change anywhere: the only
new ERROR (C10 under an unrecognized `## ... Requirements` section) has no
corpus hit. All 100 new `--strict` failures involve the 500-character limit
(C23 on ADDED, S11 promoted from INFO), and 99 of them have no other new
finding; one change also hits C21. Counting every finding, not only the new
failures: 602 for the 500-character limit, 7 for the scenario step check
(C22/S14, all in yibi-mvp main specs that `--strict` already failed), 2 for
C21 (yibi-stack), none for the C10 ERROR. Against OpenSpec 2.0.0 itself, every remaining difference on these
repos is one of the two ruled divergences: D13's scenario identity (13 issues)
and spectra's own `.openspec.yaml` keys (29 issues; without that ruling 8 more
changes, 5 in yibi-mvp and 3 in yibi-stack, would fail `--strict` on keys
`spectra new`/`archive` wrote).

## Archived changes

`--archived` fails when an archived change's top-level `tasks.md` still has
pending tasks. The validator uses OpenSpec's `parseTaskLines` grammar (1.13.2;
2.0.0 only adds continuation lines to a task's text, which counting ignores),
not the shared oracle task parser: `-`/`*`/`+` and ordered `1.`/`1)` list
markers (one to nine digits), a padded checkbox holding at most one
non-whitespace UTF-16 code unit, and even a blank description. Only `x`/`X`
means done. Whitespace follows ECMAScript: BOM is included, NEL is not;
an astral marker such as `[🚧]` is too wide and is not a task. One-character
Markdown links do not count, except whitespace-only boxes retain OpenSpec's
permissive treatment. Lines inside fences still count.
The same validation-only parser supplies active-change checkbox/numbering
checks; `task done`, `list`, `instructions` and `archive` retain their own
oracle task semantics.

An incomplete item reports ERROR at `tasks.md`:
`1 incomplete task (0/1 completed)` or `N incomplete tasks (c/t completed)`.
No task file or an empty file contributes no finding. Hidden archive
directories are ignored. OpenSpec's archived JSON uses UTF-16 code-unit
id order, **not** locale collation; the findings projection preserves it.
The `validate_openspec_archived` fixture pins counts, hidden directories,
case ordering, supplementary-plane versus BMP ordering, BOM/NEL whitespace
and the UTF-16 marker-width boundary.

The oracle rejects `--archived` (clap error, rc 2); this surface follows
OpenSpec, not the oracle. #227 changes the counter/report for the existing
top-level `tasks.md` reader; it does not add OpenSpec's schema-selected task
glob discovery or change the existing handling of task-file I/O failures.

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

Field for field OpenSpec 2.0.0 (`commands/validate.js`), pinned by the
`envelopes` and `all_order` sections of `golden/validate-openspec-2.0.0.json`
(fixtures `validate_openspec_envelope`, `validate_openspec_empty`,
`validate_openspec_archived`, `validate_openspec_unicode`,
`validate_openspec_badconfig`, `validate_openspec_unreadableconfig`, and the
rule fixture) and replayed by `validate_openspec_integration.rs`. The
envelope shape itself did not change between 1.13.2 and 2.0.0; only the
findings inside it did:

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
- Active items use ICU4X 2.1.1 with compiled CLDR 48 data and fixed en-US
  tertiary collation (owner ruling #227, 2026-10-11). This replaces the
  ASCII table/non-ASCII code-point approximation; accents, non-Latin scripts
  and canonical equivalence are handled by the collator. A change and spec
  sharing the same id retain change-first input order; OpenSpec's async tie
  is normalized change-first in the capture. Other collation-equal strings
  retain input order, without a byte-order tie-break.
- **Deliberate locale boundary:** OpenSpec's bare `localeCompare` follows
  the host locale. The measured Node 22.22.3 / ICU 78.2 / CLDR 48.0 baseline
  was en-US; sv-SE moves `ä`/`å` after `z`, and zh-TW moves CJK before Latin.
  Both capture and OpenSpectra pin en-US rather than varying across hosts.
  The replay runs with `LC_ALL=sv_SE.UTF-8`; the release CLI was also checked
  under zh-TW. The pinned dependency and authority golden are the guard
  against future collation-data changes, not a claim about arbitrary ICU
  versions. Archived items instead use UTF-16 order as described above.
- `root.source` is always `nearest` (OpenSpectra has no store roots).
- `--report findings` gives OpenSpec's `{report: {kind, version, scope,
  returnedItems, totalItems}, itemFindings, summary, root}`, items with any
  finding (INFO included); `--changes --specs` has scope `all`.
- An **untyped** unknown item prints `{"status": [{"severity": "error", "code":
  "unknown_item", "message": "Unknown item 'x'. Did you mean: …?"}]}` to
  stdout (exit 1); the suggestions are the five nearest ids by Levenshtein
  distance over UTF-16 units, active changes (by name) then specs, ties in
  that order. An ambiguous item prints the `ambiguous_item` status with
  `"fix": "Pass --type change|spec."`.
- An explicit `--type` validates its target even when it is absent or only
  exists as the other kind. Missing changes report the no-deltas finding;
  missing specs report an ERROR/file ENOENT finding containing the spec
  path. Both use the regular `items` envelope, empty stderr and exit 1.
  Capture/replay normalize the root inside diagnostic strings as well as
  the `root.path` field. Malformed typed names return `invalid_item` before
  lookup; OpenSpectra retains its existing refusal of colons/drive prefixes.
  Default oracle-format missing-item diagnostics remain unchanged.
- An unreadable or unparseable `openspec/config.yaml` (or `config.yml`) prints
  `{"status": [{"severity": "error", "code": "unreadable_config" |
  "unparseable_config", "message": "Could not read|parse <path> (<detail>).",
  "fix": …}]}` to stdout and exits 1 before any item is validated (2.0.0;
  see the divergences under "Rule authority" for the default format and the
  `<detail>` text).
- Deliberately not followed (rulings): a bare `validate` validates every
  change instead of printing OpenSpec's hint (D12-5); `ITEM --all` validates
  ITEM instead of the bulk scope (W9a-3). `durationMs` is OpenSpectra's own
  timing.
- **#227 verification boundary:** the three listed W9a-4 gaps are covered by
  31 captured envelopes (34 since #265 added the config cases), including missing/wrong-kind typed items, unsafe
  typed names, archived findings and Unicode full/findings reports. The
  capture is run twice before accepting regenerated output; no golden data
  is edited by hand. Capture: `python3 scripts/capture-validate-openspec.py
  --write` with OpenSpec 2.0.0 (Node.js 22.12 or newer, the floor since
  OpenSpec 1.16.0). Replay:
  `cargo test -p spectra-cli --test validate_openspec_integration`.
  This is evidence for those fixtures, not a new claim of exhaustive parity
  for every schema, I/O condition, locale or Unicode-data version.

The previous OpenSpectra v2 JSON (`version: "2.0"`, `totals.total`,
`root.spec_dir`) is gone; `summary.totals.failed` and the item fields a gate
reads are in the OpenSpec shape too.

## Exit status

- 0: every selected item is valid under the selected strictness.
- 1: at least one item is invalid, or an operational error occurred.

Unlike `drift`, validation severity intentionally controls the exit status.
Golden comparisons include diagnostic text; automation should still key on
level, valid, path, line and summary fields rather than matching prose.
