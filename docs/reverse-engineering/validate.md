# OpenSpec-compatible `spectra validate`

OpenSpectra's validator is not reverse-engineered from the closed Spectra
binary. Its rule authority is `@fission-ai/openspec` 1.13.2 (owner ruling D1,
below); the JSON report keeps the additive fields consumed by the original
OpenSpectra 1.5-compatible CI gate (the `--format oracle|openspec` output
decision, D3, is separate work).

## CLI

```text
spectra validate [ITEM] [--type change|spec] [--strict] [--json]
spectra validate --changes [--strict] [--report full|findings] [--json]
spectra validate --specs [--strict] [--report full|findings] [--json]
spectra validate --all [--strict] [--report full|findings] [--json]
spectra validate --archived [--report full|findings] [--json]
```

`ITEM` auto-detects a change or canonical spec. `--type` resolves ambiguous
names. Bulk scopes are mutually exclusive. `--archived` checks task completion
only; archived deltas have already been applied.

With no item or scope and no active changes, the probed Spectra 2.3.1 empty
state remains: human mode emits nothing, `--json` emits `[]`, and the command
exits 0.

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
| C3 | MODIFIED omits a scenario the main spec has (after rename mapping) | ERROR | `MODIFIED "Alpha" omits scenario(s) the current spec still has: "a2". The modified block has 1 scenario; the current spec has 2 scenarios. It adds none. Copy the omitted scenarios …` — once per requirement |
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

### OpenSpectra-only finding (C20, pending owner decision)

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

## JSON contract

The v2 report is additive-compatible with the original gate:

```json
{
  "items": [
    {
      "id": "add-auth",
      "type": "change",
      "valid": true,
      "issues": [
        {
          "level": "WARNING",
          "path": "auth/spec.md",
          "message": "...",
          "line": 3
        }
      ],
      "durationMs": 0
    }
  ],
  "summary": {
    "totals": { "passed": 1, "failed": 0, "total": 1, "items": 1 },
    "byType": {
      "change": { "passed": 1, "failed": 0, "total": 1, "items": 1 }
    }
  },
  "version": "2.0",
  "root": { "path": "/project", "spec_dir": "openspec" }
}
```

Existing consumers may continue gating on `summary.totals.failed`.
`--report findings` returns only items carrying ERROR/WARNING/INFO findings but
preserves full-run totals and exit status under `itemFindings`, with explicit
report kind, version, and scope metadata.

## Exit status

- 0: every selected item is valid under the selected strictness.
- 1: at least one item is invalid, or an operational error occurred.

Unlike `drift`, validation severity intentionally controls the exit status.
Exact diagnostic wording is not a compatibility contract; consumers should use
level, valid, path, line, and summary fields.
