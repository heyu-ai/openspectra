# W9 `validate` spec — oracle 3.0.0 output format × OpenSpec 1.13.2 rules × OpenSpectra 0.13.0

- Oracle: `/Applications/Spectra.app/Contents/MacOS/spectra` (`spectra 3.0.0 (Apple Silicon)`), the
  authority for **output format**.
- OpenSpec: `@fission-ai/openspec` **1.13.2**, `/Users/howie/.npm/_npx/0aaef5be8686a8bb/node_modules/@fission-ai/openspec`
  (run with `node <dir>/bin/openspec.js`), the authority for **validation rules** (D1) and for the
  `--format openspec` JSON (D3). Source line numbers below are into its `dist/`.
- OpenSpectra: `.claude/worktrees/w8-locale/target/release/spectra` (`spectra 0.13.0`).
  Code refs are into that worktree (`crates/spectra-core/src/…`, `crates/spectra-cli/src/main.rs`).
- Probe date 2026-09-28. Probes: `pNN_*.sh`/`*.py` in this directory, raw output `pNN.out` (plus
  per-tool JSON in `p02/`, `p08/`). Jails: `jails/`; corpus copies: `corpus/` (see B0). Helpers in
  `lib.sh` (`GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1`, `OPENSPEC_TELEMETRY=0`, `CI=1`).
  The lead's `p01-*` captures (corpus under `/var/folders/…`) are reused as p01.

Marking: **[V pNN]** verified by that probe; **[S file:line]** read from source, not probed;
**[I]** inferred.

Ruled decisions assumed, not re-argued: **D1** rules follow OpenSpec 1.13.2; **D3** `validate
--json` gets `--format oracle|openspec`, default `oracle`.

---

## Part A — oracle 3.0.0 output format

### A1. CLI surface and scope semantics [V p06, p09, p11]

`spectra validate [OPTIONS] [ITEM]`, options `--all --no-color --changes --specs --json`. **No**
`--strict`, `--type`, `--archived`, `--report` (OpenSpectra-only).

| invocation | items the oracle validates |
|---|---|
| `validate` (no item, no flag) | every active change (same as `--changes`) |
| `validate --changes` | every active change |
| `validate --specs` | every main spec |
| `validate --all` | **every active change only — no specs** (p06: specs present and one invalid, `--all` printed only the 3 changes; p09: specs-only project → `--all --json` = `[]`) |
| `validate --changes --specs` | **specs only** |
| `validate --specs --all` | specs only |
| `validate --changes --all` | changes only |
| `validate ITEM` | ITEM as a **change only**. A spec id (flat or nested) → `Error: Change 'cap-one' not found.` |
| `validate ITEM --all` | ITEM only (the flag is ignored) |
| change and spec share a name | the change (no ambiguity error) |
| no `.spectra.yaml`/`openspec/` | human: nothing, rc 0; `--json`: `[]`, rc 0 |

Item discovery:
- Changes: the same set and order as `list` — **`list --json` order, i.e. descending latest-file
  mtime, ties in readdir order** (see w7re SPEC §1). Checked identical on p09 (`["zeta",
  "Upper_Case","only","alpha"]` for both) and p06/p08. Names with uppercase/underscore are
  validated (`✓ Upper_Case — valid`).
- Specs: every `specs/**/spec.md`, id = directory path relative to `specs/`; **byte-ascending**
  over the full id: `["B-upper","a-b","a.b","a/b","aa","only","parentless/child"]`. A directory
  with no `spec.md` is not an item; a nested spec whose parent has no `spec.md` is.

### A2. Human output [V p06, p08, p09]

Per item, no blank lines, no header, no summary line:
```
✓ <name> — valid
✗ <name> — invalid
  error: <message>
  warn: <message>
```
- `✓` U+2713, `✗` U+2717, `—` U+2014, single spaces; `valid`/`invalid` lowercase.
- All `error:` lines first, then all `warn:` lines (p08 `d23-mixed`: the error comes from `zz/`,
  the warning from `base/`, and the error still prints first — so it is not file order).
- A valid item can carry `warn:` lines (`✓ d19-skipspecs — valid` / `  warn: No delta specs found`).
- Everything above goes to **stdout**. When at least one item is invalid, **stderr** gets
  `Error: Validation failed.\n` after stdout, rc 1. Same in `--json` mode.
- Unknown item: stdout empty, stderr `Error: Change '<item>' not found.\n`, rc 1 (also with `--json`).
- Empty scope: prints nothing, rc 0.

Colours (on a TTY, `script -q /dev/null`, p09):
```
\e[32m✓\e[0m <name> — valid
\e[31m✗\e[0m <name> — invalid
  \e[31merror:\e[0m <message>
  \e[33mwarn:\e[0m <message>
```
Only the glyph and the `error:`/`warn:` label are coloured; name, `—`, `valid`/`invalid`,
message and the stderr `Error: Validation failed.` are plain. `--no-color`, `NO_COLOR=1` and a
non-TTY stdout all disable colour (the non-TTY case was checked with stdout piped). Whether the
TTY test uses stdout or stderr is **[I]** (stdout, like drift). JSON has no colour on a TTY.

### A3. JSON output [V p01, p06, p09]

`serde_json` pretty (2 spaces), trailing newline, top level is an **array**, keys alphabetical:
```json
[
  {
    "change": "a-badop",
    "errors": [
      "/abs/…/changes/a-badop/specs/cap-x/spec.md: Parse error: Invalid format: Delta spec must contain at least one operation (ADDED, MODIFIED, REMOVED, or RENAMED)"
    ],
    "valid": false,
    "warnings": []
  }
]
```
- Change item: `{change, errors, valid, warnings}`; spec item: `{errors, spec, valid, warnings}`.
  `errors`/`warnings` are arrays of plain strings; there are no levels, paths, lines or totals.
- The item list and its order are the same as in human mode (A1). Since `--all` holds only changes,
  an array never mixes change and spec items [V p06].
- Single `ITEM` → a one-element array. Empty → `[]`. Invalid → stdout JSON **plus** stderr
  `Error: Validation failed.`, rc 1.

### A4. Exit codes [V p06, p08, p09]

| situation | rc |
|---|---|
| all items valid (warnings allowed) | 0 |
| at least one item with a non-empty `errors` | 1 (+ stderr `Error: Validation failed.`) |
| unknown item | 1 (stderr `Error: Change '<x>' not found.`) |
| empty scope / uninitialised dir | 0 |
| bad flag | 2 (clap) |

`valid == errors.is_empty()`. A warning never fails (there is no strict mode).

### A5. Oracle message catalogue and triggers [V p08 unless marked; wording from p07b strings]

Changes (these are the oracle's full set as far as the probes reached):

| trigger (p08 case) | bucket | exact text |
|---|---|---|
| no `specs/` dir (d02), or `skip_specs: true` with no specs (d19) | warn | `No delta specs found` |
| a `specs/**/spec.md` with no ADDED/MODIFIED/REMOVED/RENAMED entry: notes only (d03), empty `## ADDED` (d17), FROM without TO (d15), requirement under `## Notes` (d27) | error | `<abs canonical path>: Parse error: Invalid format: Delta spec must contain at least one operation (ADDED, MODIFIED, REMOVED, or RENAMED)` (path canonicalised: `/var/…` prints as `/private/var/…`, p01) |
| same requirement twice in ADDED (d05) | error | `specs/<cap>/spec.md: Duplicate requirement '<name>' in ADDED section` |
| same name in ADDED and REMOVED (d06) | error | `specs/<cap>/spec.md: Requirement '<name>' appears in both ADDED and REMOVED sections` |
| MODIFIED / ADDED / REMOVED that the main spec cannot take (d10 missing target, d20 ADDED already exists, d21 REMOVED missing) | warn | `specs/<cap>/spec.md: Archive would refuse the delta for '<cap>': Delta for <cap> declares <n> <OP> operation(s) but <m> applied; the main spec does not hold the targeted requirement, or the requirement name does not match` |

Not flagged at all by the oracle (item valid, no message): `specs/spec.md` at the specs root (d04),
ADDED without scenario (d07), ADDED without text (d08), no SHALL (d09), MODIFIED against a missing
main spec (d11), MODIFIED dropping a scenario (d12), stray `### Notes` (d13), nameless
`### Requirement:` in a delta (d14), two `## ADDED` sections (d16), `specs/x.md` (d18), MODIFIED
into a structurally broken main spec (d22), missing proposal.md (d24), empty scenario (d25),
`#####` scenario (d26), SHALL only in the header (d28), duplicate scenario names (d29), task
numbering (d30).

Specs:

| trigger | bucket | exact text |
|---|---|---|
| no `## Requirements` (s05) | error | `Invalid format: Missing ## Requirements section` |
| `## Requirements` with no `### Requirement:` (s06; also after s07) | error | `Invalid format: No requirements found in ## Requirements section` |
| a non-`Requirement:` `###` under Requirements (s07) | error | `Invalid format: Invalid requirement header '### Admin Portal'; expected '### Requirement: <name>'` |
| `## ADDED/MODIFIED/… Requirements` in a main spec (s11) | error | `Invalid format: '## MODIFIED Requirements' is delta-spec syntax and cannot appear in a main spec; the delta was pasted in instead of being applied by archive` |
| `### Requirement:` without a name (s14) | error | `Invalid format: Requirement header is missing a name` |
| requirement with no `####` child (s08, s17 `#####`) | warn | `Requirement '<name>' has no scenarios` |

Not flagged: missing/empty/`### ` Purpose (s02, s03, s20), placeholder Purpose (s04, s19),
requirement without text (s09), no SHALL (s10), duplicate requirement (s12), requirement outside
Requirements (s13), short Purpose (s15), long text (s16), empty scenario body (s18).
Other strings present in the binary but not reached by any probe: `Missing section: `,
`Delta spec must be inside a capability directory: <p>/spec.md`, `RENAMED (FROM)`/`(TO)` section
labels [S p07b].

### A6. What makes an item invalid for the oracle

Only the error rows of A5: delta parse failures (a delta file with zero operations), duplicate or
conflicting names inside one delta file, and main-spec structure errors (missing/empty
Requirements, non-`Requirement:` `###` headers, delta headers, nameless requirement headers).
Every "archive would refuse" case is a warning, and nothing semantic (scenarios, SHALL, text,
scenario loss) is checked for changes. On the corpus that gives yibi-mvp changes 0/21 invalid,
yibi-stack changes 3/9, nextrek-cli changes 1/1, yibi-mvp specs 19/123 [V p02].

---

## Part B — OpenSpec 1.13.2 rules vs OpenSpectra 0.13.0

### B0. Method [V p02]

OpenSpec always reads `openspec/{changes,specs}` under the project root
(`utils/item-discovery.js:29`, `commands/workflow/shared.js:77`) and ignores `.spectra.yaml`.
Each corpus project was copied (`cp -R`) from the parity sandbox into `corpus/<proj>`, and for
`spec_dir: docs/openspec` projects (yibi-mvp, nextrek-cli) a relative symlink
`openspec -> docs/openspec` was added at the root. OpenSpec follows it, so discovery and all
relative paths are unchanged; OpenSpectra ignores the link (it reads `spec_dir`). yibi-stack
already uses `openspec/`. Run: `node …/bin/openspec.js validate --changes|--specs --json
[--strict] --no-interactive` with `CI=1 OPENSPEC_TELEMETRY=0`.

What OpenSpec runs [S `commands/validate.js`]: for a change, only
`Validator.validateChangeDeltaSpecs(changeDir, {mainSpecsDir, projectRoot})` (:257, :383) —
proposal.md is never read; for a spec, `Validator.validateSpec(file)` (:269, :395). A change is
invalid iff it has an ERROR (strict: or a WARNING) — `validator.js:850-865`.

### B1. Corpus verdicts [V p02/p03, p05]

| project / scope | items | oracle invalid | OpenSpec invalid | OpenSpectra invalid | OpenSpec `--strict` | OpenSpectra `--strict` |
|---|---|---|---|---|---|---|
| yibi-mvp `--changes` | 21 | 0 | 8 | **9** (0046) | 8 | 9 |
| yibi-mvp `--specs` | 123 | 19 | 17 | **12** | 49 | **42** |
| nextrek-cli `--changes` | 1 | 1 | 1 | 1 | 1 | 1 |
| nextrek-cli `--specs` | 2 | 0 | 0 | 0 | 2 | 2 |
| yibi-stack `--changes` | 9 | 3 | 3 | 3 | 3 | 3 |
| yibi-stack `--specs` | 16 | 0 | 0 | 0 | 16 | 16 |

Item sets are identical across the three tools (including nested spec ids). Verdict differences
(non-strict): change 0046; specs `E07-content-cms-admin`, `E08-content-browsing-app/F030-content-library`,
`E14-ai-interactive/F019-little-know-it-all`, `E20-growth-insights/F022-growth-report`,
`process-development-dod` (all: OpenSpec invalid, OpenSpectra valid). Strict adds `E08-content-browsing-app`,
`E12-device-interactive`, `E19-sleep-routine`, `E22-subscription` (Purpose too brief, rule B2-S9).

### B2. Difference table

Class: **(a)** OpenSpectra bug to fix to match OpenSpec; **(b)** wording/path only (same rule, same
level, same verdict); **(c)** OpenSpectra-only deliberate check; **(d)** unclear.
"Corpus" names the affected corpus items; "p08" names the synthetic case.

#### Changes (delta specs)

| # | rule | OpenSpec 1.13.2 | OpenSpectra 0.13.0 | root cause in OpenSpectra | OpenSpec source | evidence | class |
|---|---|---|---|---|---|---|---|
| C1 | MODIFIED/ADDED/RENAMED target conflicts with the main spec ("archive would refuse") | **INFO** `Archive would refuse this delta: <cap> MODIFIED failed for header "### Requirement: <n>" - not found` (first failure per file only), verdict unchanged | **ERROR** `capability '<cap>': cannot MODIFY requirement '<n>' -- it does not exist in <abs path>` at `changes/<name>` | `validate.rs:validate_change` 301-310 turns every `archive::validate_archive_compatibility` error into ERROR | `validator.js:802-849` (INFO :841-845); messages `specs-apply.js:261, 380, 412` | 0046; p08 d10, d11 (no main spec), d20 (ADDED exists) | (a) — known deviation 1 |
| C2 | REMOVED target missing | nothing (archive treats it as already removed, `specs-apply.js:367` only `warn()`s) | ERROR `cannot REMOVE requirement … does not exist` | same as C1 (`archive.rs:1314-1317`) | `specs-apply.js:351-368` | p08 d21 | (a) |
| C3 | MODIFIED drops a scenario the main spec has | one ERROR per requirement, path `<cap>/spec.md`, message adds the balance sentence and remedy: `… still has: "a2". The modified block has 1 scenario; the current spec has 2 scenarios. It adds none. Copy the omitted scenarios into the MODIFIED block (a MODIFIED requirement replaces the whole block, so archive refuses to drop them).` | the same ERROR **twice**: `specs/<cap>/spec.md` (validate.rs:270-299) **and** `changes/<name>: capability '<cap>': MODIFIED requirement '<n>' omits scenario(s)…` (archive compat, first failure only) | duplicate: validate.rs 301-310 re-reports `archive.rs:1326-1336`; wording: validate.rs 282-294 lacks `describeScenarioBalance` | `validator.js:578-655`, `requirement-blocks.js:396-456`; OpenSpec suppresses the archive re-report via `alreadyReported` (`validator.js:366-377, 825-829`) | 0070, 0098, 0140, 0141, 0148; p08 d12, d29 | duplicate (a) = #183, wording (b) |
| C4 | stray `###` header inside ADDED/MODIFIED | the requirement block **continues** through it (ends only at the next `### Requirement:` or `## `); INFO per stray header: `Header "### Notes" in ADDED Requirements is not a "### Requirement:" header and is ignored by validation. Use "### Requirement: Notes" if it should be validated as a requirement.` (line) | the block **ends** at any `###`, so `#### Scenario:` after it are lost → false ERROR `must have at least one \`#### Scenario:\`…` | `markdown.rs:requirement_blocks` 165-171 collects every level-3 heading as a block boundary | `requirement-blocks.js:247-291` (absorb :284-288, INFO sink :253-264), `validator.js:167-177` | 0098 (`prompt-lab-version-history`: `### Interface / Data Shape`, `### Scenarios`); p08 d13 | (a) — known deviation 3, confirmed false positive |
| C5 | scenario = level-4 header **with a body** | a `####` with empty body does not count; ERROR adds ` (a scenario header with no body under it does not count; add its steps, e.g. "- **WHEN** ..." and "- **THEN** ...")` | any `####` counts, body ignored | `markdown.rs:194-196` | `requirement-text.js:27-39, 101-125`, `validator.js:255-258, 910-914` | p08 d25 (OpenSpec invalid, OpenSpectra valid) | (a) |
| C6 | duplicate/conflicting names in one delta (dup ADDED, ADDED+REMOVED, …) | exactly one ERROR per conflict (`Duplicate requirement in ADDED: "Alpha"`, `Requirement present in both ADDED and REMOVED: "Alpha"`); the rest of the file is still validated | parse aborts; **three** ERRORs: the parse error at `specs/<cap>/spec.md`, the same error again via archive compat (`parsing delta: …`), and a bogus "Change must contain at least one delta" | `validate.rs` 215-221 (`continue` skips the ops count), 301-310 (re-report), 314-321 (no-delta fires because the ops count stayed 0); `markdown.rs:validate_delta_conflicts` 300-391 bails on the first conflict | `validator.js:228-362` | p08 d05, d06 | (a) (count), (b) (wording: OpenSpectra ADDED dup reads `delta ADDs requirement 'Alpha' more than once`) |
| C7 | repeated section (`## ADDED Requirements` twice) | sections are **merged**, valid | ERROR `delta has more than one \`## ADDED Requirements\` section` ×3 (as C6) | `markdown.rs:parse_delta` 431-446 | `requirement-blocks.js:198-246` | p08 d16 (OpenSpec valid, OpenSpectra invalid) | (a) |
| C8 | delta sections present but no entries (empty ADDED, nameless `### Requirement:`, FROM without TO) | ERROR `Delta sections ## ADDED Requirements were found, but no requirement entries parsed. Ensure each section includes at least one "### Requirement:" block (REMOVED may use bullet list syntax).` + ERROR no-deltas; nameless header adds INFO `Header "### Requirement:" in ADDED Requirements is missing a requirement name…`; unpaired rename adds ERROR `RENAMED FROM: "Alpha" has no matching TO: line. Write each rename as a FROM: line followed immediately by its TO: line.` (line) | parse error (`delta \`## ADDED Requirements\` section contains no recognizable entries` / `RENAMED Requirements contains FROM without following TO`) ×2 + no-delta | `markdown.rs` 461-499, 241-273; triple-report as C6 | `validator.js:167-189, 206-222, 401-407`; `requirement-blocks.js:346-382` | p08 d14, d15, d17 | (a) (INFO/count), (b) (wording); verdict equal |
| C9 | file under `specs/` with no delta section at all | ERROR per file `No delta sections found. Add headers such as "## ADDED Requirements" or move non-delta notes outside specs/.` + ERROR no-deltas | only the no-deltas ERROR | `validate.rs` has no per-file "no sections" rule | `validator.js:217-222, 408-414` | yibi-stack ×3 (add-harness-eval-validation-protocol, add-task-demand-normalization, add-token-economy-harness); p08 d03, d23 | (a) (missing ERROR; verdict equal) |
| C10 | requirement outside any delta section | WARNING `Requirement "Alpha" is under "## Notes", which is not a delta section, so it is ignored. Move it under "## ADDED Requirements", "## MODIFIED Requirements", "## REMOVED Requirements", or "## RENAMED Requirements".` (line) | nothing | not implemented | `requirement-blocks.js:165-189`, `validator.js:195-205` | p08 d27 | (a) (strict verdict can differ) |
| C11 | `specs/spec.md` at the specs root | item ERROR `Delta spec found at specs/spec.md. Delta specs must live under a capability path (e.g. specs/<capability-path>/spec.md) — a file at the specs/ root is ignored when the change is applied or archived.`, path `spec.md` | **fatal**: `Error: found spec.md directly under <abs>/specs -- …`, rc 1, **the whole bulk run aborts** (no JSON for any item) | `fsutil.rs:capability_id` 137-143 returns Err, propagated by `validate.rs:190` `?` | `validator.js:142-150` | p08 d04 | (a) |
| C12 | delta in a non-`spec.md` file (`specs/x.md`) | ERROR `Delta spec found at specs/x.md. Delta specs must be a spec.md inside a capability folder — this file is ignored when the change is applied or archived. Move its requirements into specs/x/spec.md.` (and no no-deltas error) | only the no-deltas ERROR | `fsutil::collect_delta_specs` reads `spec.md` only; no unread-file scan | `validator.js:390-400, 443`; `utils/spec-discovery.js:findUnreadDeltaFiles` | p08 d18 | (a) (message; verdict equal) |
| C13 | MODIFIED into a structurally invalid main spec | INFO `Archive would refuse this delta: <cap>: target spec is structurally invalid and cannot be updated until fixed:\nline 18: Main spec contains delta header …` | nothing | archive compat does not run the main-spec structure check | `specs-apply.js:289`, `validator.js:841-845` | 0098 (`E07-content-cms-admin/F029-prompt-lab-preview-progress`); p08 d22 | (a) (INFO only) |
| C14 | no delta at all | ERROR path `file`: `Change must have at least one delta. No deltas found. Ensure your change has a specs/ directory …` (the full `GUIDE_NO_DELTAS`, `constants.js:50`) | ERROR path `changes/<name>`: `Change must contain at least one delta (an ADDED/MODIFIED/REMOVED/RENAMED requirement under specs/**/spec.md)` | wording | `validator.js:443-450` | 0084, 0126, 0144, nextrek-cli-accounting; p08 d02 | (b) |
| C15 | `skip_specs: true`, no specs | INFO path `file`: `skip_specs is set in .openspec.yaml: change declares no spec-level behavior changes, zero deltas accepted`; conflict ERROR path `file` `CHANGE_SKIP_SPECS_CONFLICT`; invalid metadata ERROR `CHANGE_SKIP_SPECS_INVALID_METADATA` | INFO path `.openspec.yaml` `Change declares skip_specs and contains no delta specs`; conflict ERROR own wording; no invalid-metadata rule | `validate.rs:201-211` | `validator.js:415-449`, `constants.js:34-36` | p08 d19 | (b); invalid-metadata marker (a) [S] |
| C16 | missing requirement text / SHALL | text missing: ERROR `ADDED "Alpha" is missing requirement text`, or, if SHALL/MUST is in the header, ERROR `ADDED "<h>" must contain SHALL or MUST in the requirement body, not only in the header. Move the SHALL/MUST statement to the line immediately after the "### Requirement: ..." header.`; no SHALL: WARNING `ADDED "Alpha" should contain SHALL or MUST (RFC 2119 best practice for English specs)` (`MODIFIED "…"` likewise) | ERROR `Requirement 'Alpha' is missing requirement text`; WARNING `Requirement 'Alpha' should state a normative SHALL or MUST` | wording | `validator.js:238-254, 270-286, 892-899` | p08 d08, d09, d28 | (b) |
| C17 | no scenario (no `####` child; `#####` only) | ERROR `ADDED "Alpha" must include at least one scenario` | ERROR `Requirement 'Alpha' must have at least one \`#### Scenario:\` or other level-4 scenario block` | wording | `validator.js:255-258, 287-290` | p08 d07, d26 | (b) |
| C18 | task numbering | WARNINGs `Task ID "1.1" is duplicated; it was first declared on line 3.` / `Task "2.1" is under group 1, but its leading number points to group 2. Move it to group 2 or renumber it.`; **only** when the change resolves to the built-in `spec-driven` schema, and only in files that have a `## N.` heading, only after the first group heading | same two rules, shorter wording, always on, duplicates also counted outside groups | `validate.rs:task_numbering_issues` 458-503 | `task-numbering.js`, `validator.js:470-498, 551-566` | p08 d30 | (b) wording; gating (a) [S] |
| C19 | tracked task files with no checkbox | WARNING `This change counts as 0 tasks: …` | nothing | not implemented | `task-checkboxes.js:21-40`, `validator.js:491-493` | — (not in corpus) | (a) [S] |
| C20 | unrecognized `<!-- @trace` footer on a MODIFY/REMOVE target | nothing | ERROR `capability '<cap>': cannot MODIFY requirement '<n>' -- it holds an unrecognized \`<!-- @trace\` footer that would be discarded; …` | `archive.rs:refuse_to_discard_unrecognized_footer` 1406-1420 via validate.rs 301-310 | — | 0141 (`E01-auth-family`) | (c) |
| C21 | issue paths / lines | delta paths relative to the change's `specs/` (`E18-custom-creator/spec.md`); change-level `file`; most delta ERRORs have no `line` | `specs/<cap>/spec.md`, `changes/<name>`, `.openspec.yaml`; `line` on most requirement findings | `validate.rs:spec_rel_path` 548-550, `Issue::at_line` | `validator.js:160` | all | (b) for oracle format; (a) for `--format openspec` |
| C22 | requirement-header grammar | `/^###\s*Requirement:\s*(.+)\s*$/i`: column 0 only, space after `###` optional, name taken raw then only a closing ` ###` run stripped; `## ` sections also column 0 (`/^(##)\s+(.+)$/`) | `heading_text`: ≤3 leading spaces allowed, space after hashes required, trailing `#`s trimmed | `markdown.rs:132-155` | `requirement-blocks.js:2-8, 21, 204` | — | (d) [S]; edge cases only, not probed |

#### Main specs

OpenSpec validates a main spec in two passes [S `validator.js:24-47, 672-743`]:
`MarkdownParser.parseSpec` builds a heading tree (`markdown-parser.js:59-115`); **Purpose and
Requirements are found depth-first at any heading level** (`findSection`), and **every child
heading of Requirements is a requirement** (`### Admin Portal` included), whose scenarios are its
direct child headings with a non-empty body (`#####` counts when there is no `####` between).
Missing Purpose/Requirements **throws**, giving one ERROR at path `file` and skipping every other
rule (Purpose is checked first). Zod then checks `requirements.min(1)`, each requirement's
`scenarios.min(1)`; `applySpecRules` adds the structure checks, Purpose placeholder/brevity,
long-text INFO, no-scenario WARNING, and SHALL/MUST checks over the `### Requirement:` blocks only.
OpenSpectra instead reads only `### Requirement:` blocks under the first `## Requirements`
(`markdown.rs:parse_main_requirements` 507-522) and `## Purpose` at level 2 (`parse_main_purpose`
546-560).

| # | rule | OpenSpec 1.13.2 | OpenSpectra 0.13.0 | root cause | OpenSpec source | evidence | class |
|---|---|---|---|---|---|---|---|
| S1 | non-`Requirement:` `###` under Requirements | counted as requirements; with a scenario and text → valid; without a scenario → Zod ERROR + WARNING (S5) | ignored; if no `### Requirement:` remains → ERROR `Spec must contain at least one requirement under ## Requirements` | `parse_main_requirements` only sees `### Requirement:` | `markdown-parser.js:116-129` | `E07-content-cms-admin` (OpenSpec invalid via S5 on 5 plain `###`, OpenSpectra valid); p08 s07 (OpenSpec valid, OpenSpectra invalid) | (a) |
| S2 | delta header inside a main spec | ERROR path `file`, line: `Main spec contains delta header "## MODIFIED Requirements". Delta headers are only valid inside openspec/changes/<name>/specs/<capability-path>/spec.md and truncate the parsed ## Requirements section.` | nothing | not implemented | `spec-structure.js:29-38` | F019, F022, process-development-dod, process-route-safety; p08 s11, broken-main | (a) — verdict differs |
| S3 | `### Requirement:` outside `## Requirements` | ERROR path `file`, line: `Requirement header "### Requirement: Beta" appears outside the main ## Requirements section. Main specs only parse requirements inside that section, so this requirement is currently invisible to validate, list, and archive.` | nothing | not implemented | `spec-structure.js:40-56` | F030, F019, F022, process-development-dod, process-route-safety; p08 s13 | (a) — verdict differs |
| S4 | duplicate requirement header | ERROR path `file`, line: `Requirement header "### Requirement: Alpha" duplicates the requirement declared on line 9. Requirement names must be unique so spec updates cannot discard one block while updating another.` | nothing | not implemented | `spec-structure.js:57-72` | p08 s12 | (a) |
| S5 | requirement without scenario | **two** issues: ERROR path `requirements.<i>.scenarios` `Requirement must have at least one scenario` (Zod) **and** WARNING path `requirements[<i>].scenarios` `Requirement must have at least one scenario. Scenarios must use level-4 headers. Convert bullet lists into:\n#### Scenario: Short name\n- **WHEN** ...\n- **THEN** ...\n- **AND** ...` | one ERROR `Requirement 'Alpha' must include a scenario` | wording + count | `base.schema.js:10-17`, `validator.js:711-717` | E07-content-cms-admin; p08 s08 | (b) wording, (a) count/second WARNING |
| S6 | scenario counting | any direct child heading with non-empty body; `#####` counts; empty `####` does not | only level-4, body ignored | `markdown.rs:194-196` | `markdown-parser.js:130-142` | process-route-safety (OpenSpectra: 4 false ERRORs for `##### Scenarios:`); p08 s17 (false invalid), s18 (false valid) | (a) |
| S7 | Purpose lookup | any heading level (`### Purpose` accepted); missing or empty → ERROR path `file` `Spec must have a Purpose section. Missing required sections. Expected headers: "## Purpose" and "## Requirements". Example:\n…` and **nothing else is reported** | level-2 only; ERROR `Spec must contain a non-empty ## Purpose section`, other rules still run | `parse_main_purpose` 546-560; validate.rs 339-352 | `markdown-parser.js:16-25, 104-115`, `validator.js:37-45, 766-778`, `constants.js:51` | p08 s02, s03 (b); s20 (a: OpenSpectra false invalid) | (a)/(b) |
| S8 | missing `## Requirements` / empty | missing → ERROR path `file` `Spec must have a Requirements section. <GUIDE_MISSING_SPEC_SECTIONS>`; present but no children → ERROR path `requirements` `Spec must have at least one requirement` | both → ERROR `Spec must contain at least one requirement under ## Requirements` | wording | `markdown-parser.js:23-25`, `spec.schema.js:7-8` | 5 yibi-mvp specs (F029, F017, E14, E20, E90); p08 s05, s06 | (b) |
| S9 | Purpose shorter than 50 chars (after trim, JS `.length`) | WARNING path `overview` `Purpose section is too brief (less than 50 characters)` (skipped when S10 fires) | nothing | not implemented | `validator.js:696-702`, `constants.js:6, 41` | 8 yibi-mvp specs (strict verdict: E08, E12, E19, E22); p08 s15 | (a) |
| S10 | placeholder Purpose | WARNING path `overview`, line of the marker: `Purpose section is still a placeholder rather than a Purpose anyone wrote (…)` (full `PURPOSE_IS_PLACEHOLDER`, `constants.js:42-45`) | WARNING path `spec.md`, no line, `Purpose is still a TBD/TODO placeholder` | wording; detection rules differ in detail (OpenSpec: leading `TBD`/`TODO` shouted, or case-insensitive followed by punctuation/EOL, fence-aware; or the generated prefix…suffix pair) | `purpose-placeholder.js` | 23 yibi-mvp + 16 yibi-stack + 2 nextrek-cli specs [V p13]; p08 s04, s19 | (b); detection edge cases (d) [S] |
| S11 | requirement text > 500 chars | INFO path `requirements[<i>]` `Requirement text is very long (>500 characters). Consider breaking it down.` (JS UTF-16 length of the body before the first heading) | nothing | not implemented | `validator.js:703-710`, `constants.js:9, 46` | 286 in yibi-mvp specs, 31 in yibi-stack; p08 s16 | (a) (INFO only) |
| S12 | body text missing / SHALL | missing body → ERROR path `requirements[<i>]` `Requirement "<n>" must contain SHALL or MUST` (header-keyword variant as C16); no SHALL → WARNING `Requirement "<n>" should contain SHALL or MUST (RFC 2119 best practice for English specs)`; `### Requirement:` blocks only | ERROR `Requirement '<n>' is missing requirement text` / WARNING `… should state a normative SHALL or MUST` (path `spec.md`, line) | wording | `validator.js:725-741` | F037 (15×), F019/F022 (6×); p08 s09, s10 | (b) |
| S13 | nameless `### Requirement:` | no finding (a child requirement titled `Requirement:`; the structure/SHALL regexes need a name) → valid | ERROR no requirements | as S1 | `spec-structure.js:6`, `requirement-blocks.js:21` | p08 s14 | (a) |

### B3. Known deviations from the plan — status

| plan item | finding |
|---|---|
| MODIFIED target missing should be INFO "Archive would refuse this delta", not ERROR | Confirmed (C1). Broader than MODIFIED: every archive-compat failure is ERROR in OpenSpectra, including ADDED-exists and MODIFIED-without-main-spec (INFO in OpenSpec) and REMOVED-missing (nothing in OpenSpec, C2). Only change 0046 is affected on the corpus. |
| duplicate scenario-missing reports (#183) | Confirmed (C3): the archive-compat pass re-reports the first failure at `changes/<name>`. The same pass also causes the triple-reports in C6/C8. Fix: do not re-report archive-compat errors for a delta file that already has an ERROR (OpenSpec's `alreadyReported`, `validator.js:366-377`), and downgrade the remainder to INFO. |
| suspected false "missing Scenario" on 0098 | Confirmed false positive (C4): `### Interface / Data Shape` and `### Scenarios` end the requirement block in `markdown.rs:requirement_blocks`, so the `#### Scenario:` under them are lost. OpenSpec absorbs the stray `###` into the block and emits two INFO notes instead. |
| OpenSpectra-only trace-footer check | Confirmed on 0141 only (C20). With it removed, 0141 is still invalid via scenario loss. |
| (new) main-spec parser differences | S1-S13: 5 non-strict verdict differences on yibi-mvp specs, all OpenSpec-invalid/OpenSpectra-valid. |

After fixing C1+C3+C4 (and ignoring C20), yibi-mvp `--changes` would match OpenSpec 8/21
item by item **[I]** (0046 is the only verdict difference today; the others differ only in counts
and wording).

### B4. Evidence for "follow OpenSpec, not the oracle" on scenario loss [V p12]

On a copy of yibi-mvp, `oracle archive 0070-e18-prd-v2-sync --yes` exited 0
(`Specs applied: E16-story-engine, E18-custom-creator (added: 7, modified: 1, removed: 5, …)`).
Before: `E18-custom-creator/spec.md:793` `#### Scenario: return-to-script-review -- Return to
script review`. After: no such header (the name survives only inside a prose blockquote the
MODIFIED block itself added, line 366). So the oracle validates this change as valid and its
archive silently drops the scenario OpenSpec (and OpenSpectra) flag. This answers the plan's M4
"未實測" item.

### B5. JSON formats

#### `--format openspec` vs OpenSpec 1.13.2 [V p10; S `commands/validate.js:274-277, 400-421, 461-475`, `core/root-selection.js:297-303`]

OpenSpec (bulk):
```json
{
  "items": [{ "id": "zeta", "type": "change", "valid": true, "issues": [], "durationMs": 12 }],
  "summary": {
    "totals": { "items": 1, "passed": 1, "failed": 0 },
    "byType": { "change": { "items": 1, "passed": 1, "failed": 0 } }
  },
  "version": "1.0",
  "root": { "path": "/abs/project", "source": "nearest" }
}
```
Issue objects: key order `level, path, message`, or `level, path, line, message` when the rule
supplies a line [V p13] (OpenSpectra: `level, path, message, line`, `validate.rs:34-41`).

| field | OpenSpec 1.13.2 | OpenSpectra 0.13.0 (current `--json`) |
|---|---|---|
| `version` | `"1.0"` | `"2.0"` |
| `summary.totals` / `byType.*` | `{items, passed, failed}` in that order | `{passed, failed, total, items}` |
| `root` | `{path, source}` (`source` e.g. `"nearest"`; `store_id` added for store roots) | `{path, spec_dir}` |
| empty bulk `byType` | one zero entry per requested type (`--all`: `change` and `spec`) | `{}` |
| item order | all items (changes and specs mixed) sorted by `id.localeCompare` (ICU root collation): `a-b, a.b, a/b, aa, alpha, B-upper, only(spec), only(change), parentless/child, Upper_Case, zeta`; ties between a change and a spec of the same id follow async completion order (nondeterministic) | changes (name order) then specs |
| issue `path`/`line`/`message` | see B2 (C21 etc.) | differs |
| `validate` with no item/flag, non-interactive | stderr hint `Nothing to validate. Try one of: …`, rc 1 | validates the single active change; with several: `Error: Multiple changes found. …`, rc 1; with none: empty report, rc 0 |
| unknown item, `--json` | stdout `{"status":[{"severity":"error","code":"unknown_item","message":"Unknown item 'nope'. Did you mean: alpha, only, zeta, a-b, a.b?"}]}`, rc 1 | stderr `Error: Change 'nope' not found.`, rc 1 |
| ambiguous item, `--json` | stdout `{"status":[{"severity":"error","code":"ambiguous_item","message":"Ambiguous item 'only' matches both a change and a spec.","fix":"Pass --type change|spec."}]}`, rc 1 | stderr `Error: Ambiguous item 'only' matches both a change and a spec; pass --type change|spec`, rc 1 |
| human bulk | `✓ change/<id>` (stdout) / `✗ change/<id>` + `  ✗ [ERROR] <path>: <msg>` (stderr), `Totals: N passed, M failed (K items)`, `Details: openspec validate <first-failed> --type <t>` | `<id padded to 45> OK` / `FAIL (n issues)`, `  LEVEL path: msg`, blank line, `N passed, M failed (K total).` |

So the plan's "only three differences" understates it: key order and `total`, empty `byType`,
item order, issue paths/lines/messages, and the error envelopes also differ.

#### `--format oracle` (default) — shape to emit

Exactly A3: array, `{change|spec, errors, valid, warnings}` with alphabetical keys, items in A1
order. Human: exactly A2 (glyph, `— valid|invalid`, `  error:` then `  warn:`, colours, stderr
`Error: Validation failed.`, rc 1). The findings come from B2's OpenSpec rules; how each OpenSpec
issue maps into `errors`/`warnings` strings is open (see "Needs a human decision" 1-3).

---

## Open questions

1. Oracle colour detection: is it stdout-is-TTY (as drift) or stderr? Only the both-TTY case was
   probed. [I stdout]
2. Oracle archive-refuse warning for ADDED/REMOVED counts (`declares <n> … but <m> applied`) when
   several operations partly apply — only 1-op and 4-op (0046: `4 MODIFIED … but 0`) seen.
3. `Missing section: ` and `Delta spec must be inside a capability directory` are in the oracle
   binary but no probe reached them (d04 `specs/spec.md` was silently valid).
4. OpenSpec `localeCompare` order for non-ASCII ids (CJK spec dirs exist in no corpus project) —
   needs ICU collation to reproduce; not probed.
5. C22 header-grammar edge cases (indented headings, `###Requirement:`) not probed on either side.
6. S10 placeholder-detection edge cases (`todo:` lowercase, fenced `TBD`) not probed.
7. OpenSpec's scenario-loss check silently skips when the main spec is unreadable for
   non-"unusable" errno (`validator.js:586-602`); OpenSpectra errors on any read failure
   (`validate.rs:263-269`). Not probed.

## Needs a human decision

1. **INFO findings in the oracle format.** The oracle has only `errors`/`warnings`. OpenSpec INFO
   covers "archive would refuse" (which the oracle itself prints as `warn:`), stray-header notes,
   skip_specs-accepted, and 317 long-text notes on the corpus specs. Options: (a) drop INFO;
   (b) map all INFO to `warnings` (would add 317 warn lines to `validate --specs`); (c) map only
   "Archive would refuse …" to `warnings`, drop the rest. Recommendation: (c) — it reproduces the
   oracle's own `warn:` for 0046 and keeps `--specs` quiet; the verdict is identical under all three.
2. **Message text in the oracle format.** OpenSpec wording differs from the oracle's even for the
   shared rules (A5 vs B2). Options: OpenSpec message verbatim; or `"<path>: <message>"` for
   delta-file issues (the oracle prefixes `specs/<cap>/spec.md: `). Recommendation: prefix
   `specs/<cap>/spec.md: ` for delta-file issues and use the OpenSpec message otherwise; do not
   try to reproduce oracle wording for rules the oracle does not have. This matters little for
   consumers: the only oracle-format consumer found is the embedded skills' `spectra validate
   "<name>"` (13 calls, human output + exit code; no `--all`/`--specs`/`--json`).
3. **`--all` semantics in the oracle format.** The oracle's `--all` validates changes only and
   `--changes --specs` validates specs only (A1) — both look like oracle bugs. Options: copy them
   under `--format oracle`, or keep OpenSpec's meaning (`--all` = changes + specs, both flags =
   both) and only borrow the oracle's shape (an array can then mix `change` and `spec` items).
   Recommendation: keep OpenSpec semantics (no consumer relies on the oracle's behavior) and list it
   as a known divergence in `parity-known.tsv`.
4. **`validate <spec-id>` in the oracle format.** The oracle rejects spec ids
   (`Error: Change 'x' not found.`). Accepting them (OpenSpec behavior, a superset) is recommended;
   ambiguity then needs OpenSpectra's `--type` error, which the oracle never prints.
5. **`validate` with no arguments.** Oracle: all changes. OpenSpec: hint + rc 1 (non-interactive).
   OpenSpectra today: single-change resolution. Under D3's default-oracle stance, the oracle
   behavior (all changes) is the natural choice; it also changes what `--format openspec` users get
   from a bare `validate`.
6. **Trace-footer check (C20)** under D1: keep as ERROR (OpenSpectra-only, class (c)), or demote /
   drop. It is the only remaining non-OpenSpec ERROR after the fixes (0141 stays invalid either
   way because of scenario loss).
7. **D1 pulls OpenSpectra away from the oracle more than the plan assumed.** Beyond the 8/21
   change verdicts, following OpenSpec's main-spec parser (S1-S13) makes 5 more yibi-mvp specs
   invalid (S2/S3/S1) and one change pattern valid that OpenSpectra rejects today (C7 repeated
   sections). Nothing here contradicts D1, but it is a larger behavior change than "M4: three
   deviations"; the corpus impact list is in B1.
