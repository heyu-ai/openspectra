# Reverse-engineering `spectra analyze`

How the closed-source `spectra analyze` command finds quality gaps across a
change's artifacts, and how OpenSpectra reproduces it.

> Source binary: `Spectra.app/Contents/MacOS/spectra` **v3.0.0** (W10). The
> v2.3.1 catalogue (issue #169, probe log in
> `docs/openspec/changes/archive/2026-08-11-fill-artifact-workflow-cli/design.md`)
> was re-probed and extended: 65 targeted probes (p01–p65, with disassembly of
> the `spectra_core::analyzer::*` functions for the numeric thresholds), a
> self-describing golden of 240 oracle runs
> (`docs/reverse-engineering/golden/analyze-3.0.0.json`, produced by
> `scripts/capture-analyze.py`), and a corpus comparison over all 31 active
> changes of three real projects.

## TL;DR

`analyze` is **pure markdown heuristics — no AI**, same as `drift`. Five
dimensions (Coverage, Consistency, Ambiguity, Gaps, Localization) emit
findings from a closed set of 14 rules. It always exits `0` on a successful
run (a report, not a gate). OpenSpectra implements it in
`crates/spectra-core/src/analyze/`, one module per oracle module:
`extract.rs` (markdown parsing), `coverage.rs`, `consistency.rs` (with
`numeric.rs`), `ambiguity.rs`, `gaps.rs`, `localization.rs`.

## Reproducing the oracle

`scripts/capture-analyze.py` (macOS, reference binary pinned to 3.0.0,
`--spectra-bin`/`SPECTRA_BIN` override) builds every scenario as a scratch
project, runs the oracle, and compares against the committed golden; drift
exits non-zero and keeps the scratch projects. `--write` captures twice and
only rewrites the golden when both captures agree byte for byte, which is how
run-to-run randomness in the oracle is detected (see "Deliberate
divergences"). `crates/spectra-cli/tests/analyze_golden_integration.rs`
replays every run against OpenSpectra and compares exit code, stdout and
stderr byte for byte; `tty` runs go through `script(1)` so the colour
contract is checked on a real pseudo-terminal. The conNumericClaimMismatch
fixtures live in `scripts/capture-analyze-numeric-cases.json` (the W10
p40–p59 probe cases plus eight added while implementing).

## Output contract

`--json` is pretty **snake_case** (the inconsistency with `status`'s camelCase
is the oracle's own):

```
{change_id, dimensions[{dimension, status, finding_count}], findings[],
 artifacts_analyzed, artifacts_missing}
```

Each finding is `{id, dimension, severity, location, summary, recommendation,
summary_msg{key, params}, recommendation_msg{key, params}}`. `params` is
always present (`{}` when empty).

- `dimensions[]` always has **5** entries, in the order Coverage, Consistency,
  Ambiguity, Gaps, Localization.
- `findings[]` are ordered **LOC, COV, CON, AMB, GAP**: Localization findings
  come first even though Localization is the last dimension row. Ids are
  `<PREFIX>-<n>`, numbered per dimension from 1.
- `status` is `Clean`, `<n> issue(s) found`, or
  `Skipped (insufficient artifacts)`.
- `artifacts_analyzed` / `artifacts_missing` keep the fixed order
  `proposal, specs, design, tasks`.

### Artifact presence

| artifact | present iff |
|---|---|
| proposal / design / tasks | `proposal.md` / `design.md` / `tasks.md` is a file (empty counts) |
| specs | some `specs/<dir>/spec.md` is a regular file **exactly one level deep** (empty counts; a symlinked `<dir>` counts) |

`specs/<a>/<b>/spec.md`, `specs/x.md`, `specs/x/other.md` and a directory named
`spec.md` do not make specs present, and nested files are ignored by every
rule. The spec files analyzed are exactly those `specs/<dir>/spec.md`; the
capability name is `<dir>`. Before W10 OpenSpectra used the schema glob
(`specs/**/*.md`) and analyzed nested layouts the oracle ignores.

### Dimension gating

| dimension | runs iff |
|---|---|
| Coverage | at least 2 of the **four** artifacts are present |
| Consistency | design is present, or proposal **and** tasks are |
| Ambiguity | specs are present |
| Gaps | any artifact is present |
| Localization | `.spectra.yaml` `locale` is exactly `tw`, `cn` or `ja`, and proposal, design or tasks is present |

A dimension that runs with nothing applicable reports `Clean` (for example
Consistency with proposal and tasks only). Per-rule preconditions inside a
running dimension: `covMissingSpec` needs proposal, `covMissingTask` specs and
tasks, `covDeltaValidation` specs, `conDesignNotInTasks` design and tasks,
`conNumericClaimMismatch` proposal and design, `conGoalsNonGoalsOverlap`
design.

## The 14 findings

| key | severity | location |
|---|---|---|
| `locWrongLanguage` | Warning | `proposal.md` / `design.md` / `tasks.md` |
| `covMissingSpec` | Critical | `proposal.md → Capabilities` |
| `covMissingTask` | Warning | delta spec path |
| `covDeltaValidation` | Critical | delta spec path |
| `conDesignNotInTasks` | Warning | `design.md` |
| `conNumericClaimMismatch` | Warning | `proposal.md ↔ design.md` |
| `conGoalsNonGoalsOverlap` | Warning | `design.md` |
| `ambNoScenario` | Warning | delta spec path |
| `ambAbstractScenario` | Suggestion | delta spec path |
| `ambWeakLanguage` | Suggestion | `<delta spec>:<line>` |
| `gapNoProposal` | Critical | literal `change directory` |
| `gapNewCapabilityNoPurpose` | Warning | delta spec path |
| `gapNoMainSpec` | Warning | delta spec path |
| `gapModifiedNotFound` | Warning | delta spec path |

`params` asymmetries are real: `covDeltaValidation`, `conDesignNotInTasks`,
`conNumericClaimMismatch` and `conGoalsNonGoalsOverlap` have recommendation
params `{}`; `gapModifiedNotFound` has summary `{name}` vs recommendation
`{name, spec}`.

## Delta-spec parsing shared by the rules

Several rules read the same structure, and they do not all read it the same
way (each difference below is pinned by the golden):

- **Fences.** A fence delimiter is a line whose `trim_start` begins with
  ```` ``` ```` or `~~~` (an info string is allowed); any delimiter toggles,
  without matching the opening character.
- **Section headings** are `## ` lines starting in column 0, compared after
  `trim_end` with `## ADDED Requirements` etc.: trailing spaces and CRLF are
  fine, `  ## MODIFIED Requirements` and `## added requirements` are not
  sections. Headings inside fences are ignored.
- **Requirement headers** are lines whose `trim` starts with
  `### Requirement:`; the name is trimmed and an empty name is not a
  requirement. Headers inside fences are ignored.
- **The validation parser** keeps one list per operation section: a repeated
  `## ADDED Requirements` heading **replaces** the earlier section (so
  `ADDED [X] … ADDED [X]` is not a duplicate). `## Purpose` (column 0,
  `trim_end`, exact: not `### Purpose`, `## Purpose:` or `## purpose`) holds
  everything up to the next column-0 `## ` heading, including `### ` and `# `
  lines; a repeated Purpose replaces the earlier one.

## Coverage

Findings come grouped: all `covMissingSpec` in proposal order, then all
`covMissingTask` across spec files, then all `covDeltaValidation` across spec
files.

- **`covMissingSpec`.** A line whose `trim` starts with `## Capabilities`
  (suffix allowed), `### New Capabilities` or `### Modified Capabilities`
  opens the section; the next line whose `trim` starts with `## ` closes it
  (`###`/`####` do not). The scan is **not fence-aware**. From every
  line in the section (bullets, `*` bullets, continuation lines, table rows)
  the **first** backtick token is taken; empty tokens and tokens containing an
  ASCII space are rejected, a tab is kept, and duplicates are reported twice.
  A capability is missing when it is not exactly (case-sensitively) the name
  of a spec directory: `nested/cap` and `x` (with only `specs/X/`) are missing.
- **`covMissingTask`.** Checked names, in document order: every requirement
  header not under `## REMOVED Requirements` (headers under unknown `## `
  headings or before any heading count, and so do `### Requirement:` headers
  inside the RENAMED section), plus the **FROM** name of every RENAMED entry.
  A FROM line is `- FROM:` (case-sensitive, `-` bullet, indentation allowed)
  followed by `### Requirement: <name>`, optionally in backticks;
  `- FROM: Bare`, `* FROM:`, `- from:` and `` `Requirement: X` `` are not
  names. A name is covered when it is a case-insensitive substring of some
  **task line**: a line whose `trim_start` starts with `- [`, `* [` or `+ [`
  followed by one character and `]` (`- [ ]1.8` counts; `- []`,
  `- [link](url)`, `1. [ ]`, `-\t[ ]`, headings, continuation and plain lines
  do not).
- **`covDeltaValidation`** errors per file, in this order: duplicates within
  ADDED, MODIFIED, REMOVED (one error per extra occurrence, document order);
  cross-section conflicts for the pairs (ADDED, MODIFIED), (ADDED, REMOVED),
  (MODIFIED, REMOVED), once per name; then at most one Purpose error:
  `Invalid format: Purpose section is empty; …` when the Purpose content is
  whitespace-only, else `Invalid format: Purpose section contains placeholder
  text (TBD/TODO); …` when it contains `TBD` or `TODO` (**case-sensitive**
  substring, so `TBDX` fires and `todo` does not). RENAMED takes part in
  neither duplicates nor conflicts. Not reported by `analyze`: a file with no
  operation section, `### Requirement:` with no name, `### Req: X`,
  `#### Requirement:`, an empty file or section, a bare `- FROM:`.

## Consistency

Findings come grouped: all `conDesignNotInTasks`, all
`conNumericClaimMismatch`, all `conGoalsNonGoalsOverlap`, numbered
continuously.

- **`significant_tokens`**, shared by the three rules, over lowercased text:
  maximal ASCII `[0-9a-z]` runs of length ≥ 4, plus overlapping bigrams of
  each run of Han characters (U+3400–4DBF, U+4E00–9FFF, U+F900–FAFF; a single
  Han character yields nothing); everything else separates.
- **`conDesignNotInTasks`.** Every design line whose `trim_start` starts with
  `### ` is a topic (not fence-aware). Keyword = the rest, trimmed, with
  `strip_numbering_prefix` (a leading `[0-9.]+` followed by whitespace, once:
  `1.`, `1.2.3`, `7`, but not `1)`, `1.pqrs`, `(1)` or a fullwidth digit),
  lowercased. A keyword without tokens is never flagged; otherwise it is
  flagged when fewer than 60% of its tokens (with repetition) are substrings
  of the whole lowercased tasks.md (`matched*100 < len*60`).
- **`conGoalsNonGoalsOverlap`.** design.md only, fences skipped. A line whose
  `trim` is exactly `**Goals:**` / `**Non-Goals:**` switches lists (`## Goals`
  and `**Goals**:` do not); a line starting with `#` ends the current item and
  leaves list mode but keeps what was collected. `- `, `* `, `+ ` bullets start
  items and indented lines continue the current item (joined with one space).
  A Non-Goals item starting with `非 ` is skipped. A pair overlaps when the
  token sets share at least 8 tokens and at least 40% of the smaller set;
  emission is goal-major in document order.
- **`conNumericClaimMismatch`** (proposal.md vs design.md only). Per line:
  fences skipped (HTML comments are not), `trim`, leading `#`s and
  `strip_numbering_prefix` removed, inline code spans removed (an unclosed
  backtick drops the rest of the line). Every candidate — an ASCII digit
  extended over `[0-9,.-]`, trailing `,-.` trimmed, a directly preceding `+`/`-`
  included — takes the next per-line number index, even when rejected. A
  candidate is rejected when the character before it (before the sign) is an
  ASCII letter or digit or one of `# / : \ _`, when the character after it is
  `/`, `:` or `_`, or when an ASCII letter follows that does not start a
  recognized unit (`ns us ms s h hr hrs sec secs min mins kb mb gb tb kib mib
  gib tib bit bits byte bytes bps kbps mbps gbps hz khz mhz ghz fps px pt em
  rem vw vh`, case-insensitive; the alphanumeric run must be exactly the unit
  and must not be followed by `/`, `:` or `_`). A non-ASCII letter after the
  number is fine (`3個`). The label is the processed text before the value.
  A claim is dropped when the label's last word is a structural identifier
  (`phase step layer stage version decision`, a word ending in
  `決策 步驟 階段 版本 層`, or an all-`[A-Z0-9_]` word containing `_`); for a
  label ending in `→`/`->` (the second value of a transition) the word before
  the previous number is used instead, so `phase 3 → 5` yields no claims while
  `step 3 ms → 5` keeps the `5`. A value containing `.` is dropped in version
  context (two or more dots, a whole word `framework upgrading downgrading
  migration dependencies version release runtime library upgrade dependency
  versions upgraded downgrade` in the label, or `版本 升級 降級 遷移` anywhere
  in it). Pairs with the same number index whose label token sets share at
  least 4 tokens and at least 80% of the larger set are candidates, sorted by
  integer score `shared*100/larger` (descending), exact lowercase label match
  first, shared count (descending), equal values first, then proposal and
  design order; they are assigned greedily one-to-one and every assigned pair
  whose values differ (compared with `,` removed; `3` vs `3.0` and `+3` vs `3`
  differ) is reported in that order, with the proposal-side label.

## Ambiguity

For each spec file: all `ambNoScenario`, then all `ambAbstractScenario`, then
all `ambWeakLanguage`.

- **`ambNoScenario`.** Requirement headers outside the REMOVED **and** RENAMED
  sections. A requirement's block runs to the next requirement header (other
  `### `, `## ` and `# ` headings do not end it — yibi-mvp 0098 puts
  `### Interface` and `### Scenarios` between the requirement and its
  scenarios); it has a scenario when any line of the block has a `trim`
  starting with `#### Scenario:`, **even inside a fence**.
- **`ambAbstractScenario`.** Scenario headers (`#### Scenario:` outside
  fences) anywhere, including REMOVED sections. The block ends at the next
  `## `, `### ` or `#### ` heading outside a fence (`#####`, `# ` and
  `####Note` do not end it). It is concrete when a non-fenced, trimmed line
  starts with `##### Example:` or `- **GIVEN**`, starts with `|` and has at
  least three `|`, or starts with `- **WHEN**`, `- **THEN**` or `- **AND**` and
  contains `"`, a backtick or an ASCII digit (all case-sensitive).
- **`ambWeakLanguage`.** `should, may, might, consider, possibly, TBD, TODO,
  ???, TKTK`, first hit per line in that priority, case-insensitive plain
  substring (`mayhem` hits `may`), canonical spelling reported. Since 3.0.0
  lines whose `trim_start` begins with `#` are skipped; URLs, inline code and
  fenced lines are still scanned.

## Gaps

`gapNoProposal` (specs present without proposal), then one pass over the spec
files for `gapNewCapabilityNoPurpose`, then a second pass for `gapNoMainSpec` /
`gapModifiedNotFound` (exact trimmed-name equality against the main spec).
`gapNewCapabilityNoPurpose` fires when `<spec_dir>/specs/<cap>/spec.md` is not
a file, the delta has at least one requirement in ADDED, MODIFIED or REMOVED
(RENAMED-only, empty and no-operation files do not fire), and there is no
non-empty `## Purpose` (the heading rules above; an empty Purpose fires this
and the validation error).

## Localization

`is_wrong_language` per file (proposal.md, design.md, tasks.md, in that
order, at most one finding each; specs are never checked). Lines are split on
`\n`; fence blocks (any delimiter toggles, an unclosed fence runs to the end)
are dropped; then `http://`, `https://` and `www.` (case-sensitive, anywhere,
also mid-word) are removed up to whitespace (including NBSP and U+3000) or one
of `) > | " ' ]`; then paired backtick spans are removed (an unclosed backtick
keeps the rest). `alpha` counts ASCII letters plus non-ASCII
`char::is_alphabetic`; `cjk` counts those in U+4E00–9FFF, U+3400–4DBF and
U+3040–30FF (Hangul, U+F900 compatibility ideographs, Ext-B and fullwidth
Latin count as alpha only). The file is flagged when `alpha >= 80` and
`cjk/alpha < 0.1`. Only "written in English" exists.

## Human output

```
Change: <id>

  <✓|●> <Dimension padded to 15><status> (<n> findings)      × 5
                                                  blank line only if Analyzed is non-empty
  Analyzed: …                                     only if non-empty
  Missing: …                                      only if non-empty

  ✓ No issues found     or     Findings (<N>):  blank, then per finding
                                 [CRITICAL|WARNING|SUGGEST] <summary>
                                   at: <location>
                                   → <recommendation>
```

The glyph is `✓` when `finding_count == 0` (Skipped included), `●` otherwise.
On a terminal the oracle colours the report: bold `Change`, dimension names
bold and padded to 14 plus a space, statuses dim, `✓` green, `●` always yellow
(even for a Critical finding), `Analyzed:` dim, `Missing:` yellow, `Findings`
bold, `CRITICAL` bold red, `WARNING` yellow, `SUGGEST` dim, and the `at:` /
`→` lines dim. OpenSpectra emits the same bytes; colour follows the CLI-wide
rule (a terminal, and neither `--no-color` nor `NO_COLOR`). `--json` is never
coloured.

## Exit codes and errors

Always `0` on a successful run regardless of findings (gate on the JSON
severity fields instead). `1` with `Error: Change '<name>' not found.` for an
unknown or archived name. With several active changes and no name, `analyze`
says `Error: Multiple changes found. Specify one: <names>` — the wording is
per command in the oracle (`status`/`instructions` say
`Use --change to specify one:`). With zero active changes both human and
`--json` modes print the plain-text sentinel and exit `0`
(see [`artifact-workflow.md`](artifact-workflow.md#no-active-change-command-matrix)).

## Deliberate divergences

- **`params` key order.** The oracle serializes `params` from a hash map, so
  multi-key objects (`locWrongLanguage`, `conNumericClaimMismatch`,
  `conGoalsNonGoalsOverlap`, the `gapModifiedNotFound` recommendation) change
  key order from run to run — even between `summary_msg` and
  `recommendation_msg` of one finding. OpenSpectra emits keys in alphabetical
  order; the capture script normalizes the oracle's JSON the same way after
  checking that its re-serialization reproduces the oracle's bytes.
- **Directory visit order.** The oracle visits spec directories and lists
  changes in `readdir` order (APFS hash order). OpenSpectra uses byte-sorted
  name order (as W7a `list` does), so finding order and ids differ only where
  readdir order is not name order; the multi-change error lists names sorted.
  The golden fixtures use names whose readdir order equals name order, and the
  capture script fails otherwise. In the corpus this is the only remaining
  difference (see below).
- **Cross-section conflicts within one pair.** The oracle reports the names
  of one pair (for example ADDED/MODIFIED) in HashSet order, which also changes
  between runs (two captures of the same fixture disagreed). OpenSpectra uses
  the earlier section's document order. The pair order itself is fixed and
  pinned.
- **`CLICOLOR_FORCE`.** The oracle colours `analyze` (and `drift`, `schemas`)
  when `CLICOLOR_FORCE` is set to anything but `0` on a pipe, and then ignores
  `NO_COLOR`. OpenSpectra's CLI-wide colour rule does not read
  `CLICOLOR_FORCE` at all, and `analyze` follows that rule; changing it is a
  CLI-wide decision, not an analyze one.

Oracle quirks reproduced on purpose (faithful port): the RENAMED **FROM** name
is what `covMissingTask` checks; the Purpose placeholder check is
case-sensitive; the capability scan and weak-language scan are not
fence/URL/code-aware; `ambNoScenario` counts a fenced `#### Scenario:`.

## Inferred, not probed

- Where a Goals/Non-Goals item ends on a blank or unindented plain line
  (OpenSpectra ends the item), and the order of several overlapping pairs
  beyond the goal-major case that was probed.
- A requirement header with an empty name is not a requirement (so it is
  never a `covMissingTask`/`ambNoScenario` subject).
- Whether a `## ` section heading inside a requirement's block ends it for
  `ambNoScenario` when the next requirement is further away (probed only with
  a non-operation heading, which does not end it).

## Corpus

On 2026-09-28, `analyze <change> --json` and `analyze <change>` were compared
for every active change of yibi-mvp, nextrek-cli and yibi-stack (31 changes):
29 are byte-identical in both forms; the other 2 (yibi-mvp
`0098-e07-prompt-lab-extended-sections-and-safety` and yibi-stack
`add-retro-evidence-gate`) have the same dimensions, artifacts and finding
multiset and differ only in finding order and ids, because their spec
directories' readdir order is not name order. `scripts/parity-probe.py` on the
same corpus leaves only the analyze rows that this ordering produces.
