# Reverse-engineering `spectra analyze`

How the closed-source `spectra analyze` command finds consistency gaps
across a change's artifacts, and how OpenSpectra reproduces it.

> Source binary: `Spectra.app/Contents/MacOS/spectra` v2.3.1. The finding
> catalogue was **closed by string mining** (the i18n message keys are in the
> binary), then every finding was pinned with positive *and* negative
> synthetic fixtures (31 fixture changes) run through the binary as a golden
> oracle. Full probe log:
> `docs/openspec/changes/fill-artifact-workflow-cli/design.md`.

## TL;DR

`analyze` is **pure markdown heuristics — no AI**, same as `drift`. Four
dimensions (Coverage, Consistency, Ambiguity, Gaps) emit findings from a
closed set of ten rules. It always exits `0` (like `drift`, it is a report,
not a gate). OpenSpectra implements it in `crates/spectra-core/src/analyze.rs`.

## Output contract

`--json` is pretty **snake_case** (`change_id`, `finding_count`,
`artifacts_missing`) — unlike `status`/`instructions` (camelCase) and
`new artifact` (compact); the inconsistency is the oracle's own. Shape:
`{change_id, dimensions[{dimension,status,finding_count}], findings[],
artifacts_analyzed, artifacts_missing}`. Each finding:
`{id, dimension, severity, location, summary, recommendation,
summary_msg{key,params}, recommendation_msg{key,params}}`; ids are
`COV-1`, `AMB-2`, … numbered per dimension.

Dimension `status` strings: `Clean` / `N issue(s) found` /
`Skipped (insufficient artifacts)`.

## Dimension gating

| dimension | runs when |
|---|---|
| Coverage | ≥ 2 of {proposal, specs, tasks} present |
| Consistency | design AND tasks present |
| Ambiguity | specs present |
| Gaps | any artifact present |

With no artifacts at all, every dimension is skipped and `findings` is empty.

## The ten findings (closed set)

| key | severity | trigger | location |
|---|---|---|---|
| `covMissingSpec` | Critical | backticked capability in proposal's `## Capabilities` section has no `specs/<cap>/spec.md` in the change | `proposal.md → Capabilities` |
| `covMissingTask` | Warning | requirement name not found in tasks.md (case-insensitive substring) | delta spec path |
| `covDeltaValidation` | Critical | duplicate requirement in one section, or same-named requirement across two operation sections (nothing else triggers it) | delta spec path |
| `conDesignNotInTasks` | Warning | design `###` topic (lowercased; `##` doesn't count) absent from tasks.md | `design.md` |
| `ambNoScenario` | Warning | requirement with no scenario before the next requirement/operation heading | delta spec path |
| `ambAbstractScenario` | Suggestion | scenario with no `##### Example:` before the next heading | delta spec path |
| `ambWeakLanguage` | Suggestion | line matches the weak-word rule (below) | `<delta spec>:<line>` |
| `gapNoProposal` | Critical | specs exist but `proposal.md` doesn't | literal `change directory` |
| `gapNoMainSpec` | Warning | delta has a MODIFIED requirement but the capability's main spec doesn't exist | delta spec path |
| `gapModifiedNotFound` | Warning | MODIFIED requirement name (trimmed, **exact** equality — not substring) absent from the main spec | delta spec path |

Notes pinned by fixtures:

- Weak-word rule: literal `spec.md` delta files only (sidecar Markdown such
  as `notes.md` is ignored); the list `should`, `may`, `might`, `consider`,
  `possibly`, `TBD`, `TODO`, `???`, `TKTK` is checked in that priority order per line,
  case-insensitive **plain substring** (`mayhem` hits `may`), max one
  finding per line, reporting the list's canonical spelling.
- Capabilities extraction takes only the first backtick token per bullet
  inside `## Capabilities`; non-backticked bullets are ignored and the
  `<name>` placeholder is *not* filtered. Only change-local delta paths are
  checked (never the main `openspec/specs/`).
- `params` asymmetries are real: `covDeltaValidation` and
  `conDesignNotInTasks` have recommendation params `{}`;
  `gapModifiedNotFound` has summary `{name}` vs recommendation
  `{name, spec}`.

## Human output

`Change: <name>`, then one line per dimension
(`  <✓|●> <dimension padded to 15><status> (<N> findings)`), then
`Analyzed:`/`Missing:` lines, then `Findings (N):` with three lines per
finding (`[CRITICAL|WARNING|SUGGEST] …`, `at: …`, `→ …`) or
`✓ No issues found`. Plain text; `--no-color` changes nothing.

## Deliberate determinism choices

The oracle iterates spec files via `readdir` and serializes
`gapModifiedNotFound` recommendation params from a hash map — both orders
flip between *runs of the oracle itself*. OpenSpectra sorts spec files by
change-relative path and fixes params order `name` → `spec`. Same class of
choice as `instructions`' `contextFiles` ordering (see
`artifact-workflow.md`).

## Exit codes

Always `0` on a successful run regardless of findings (gate on the JSON
severity fields instead); `1` on operational errors
(`Error: Change '<name>' not found.`).
