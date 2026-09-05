# OpenSpec-compatible `spectra validate`

OpenSpectra's validator is not reverse-engineered from the closed Spectra
binary. Its format authority is `@fission-ai/openspec` 1.12.0, while preserving
the additive JSON fields consumed by the original OpenSpectra 1.5-compatible
CI gate.

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

## Shared Markdown contract

Validation and archive consume the same parsed delta plan. Input is normalized
from UTF-8 BOM and LF/CRLF/CR line endings before parsing. Markdown structure
inside fenced code blocks is ignored; closing fences must use the same marker
and at least the opening marker's length.

Delta files live at `changes/<name>/specs/<capability-path>/spec.md` and support:

- `## ADDED Requirements`
- `## MODIFIED Requirements`
- `## REMOVED Requirements`
- `## RENAMED Requirements`
- an optional leading `## Purpose` for a new capability

Capability paths may be nested. A `spec.md` directly under a change's `specs/`
has no capability ID and is rejected. Validation and archive use the same
recursive collector, do not descend symlinked directories, and fail on
unreadable entries.

## Findings

Findings have `ERROR`, `WARNING`, or `INFO` severity plus a path, message, and
an optional grounded line number.

- `ERROR` always fails validation.
- `WARNING` is guidance in normal mode and fails under `--strict`.
- `INFO` never fails validation.

Rules include:

1. A change needs at least one parsed delta unless `.openspec.yaml` declares
   `skip_specs: true`.
2. `skip_specs` conflicts with any file under the change's `specs/` tree.
3. ADDED and MODIFIED requirements need descriptive text and at least one real
   level-4 scenario child. `#### Scenario:` is canonical; other level-4 child
   headings count for loss prevention.
4. A non-empty requirement without whole-word `SHALL` or `MUST` produces a
   warning.
5. Duplicate sections, duplicate requirement names, malformed rename pairs,
   and conflicting operations across sections are errors.
6. A MODIFIED block is compared with the current main requirement after rename
   mapping. Omitting any existing scenario is an error because archive replaces
   the whole block.
7. Canonical specs need non-empty Purpose and Requirements sections, descriptive
   requirement text, and scenarios.
8. Generated or leading TBD/TODO Purpose text is a warning and therefore fails
   strict validation.
9. Duplicate task IDs and task IDs under the wrong numbered group are warnings
   with line numbers.
10. `--archived` fails when an archived change still has pending tasks.

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
          "path": "specs/auth/spec.md",
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
