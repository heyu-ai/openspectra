# Reverse-engineering `spectra validate`

How OpenSpectra's `validate` command was designed, and why it is calibrated
against a *different* reference than the rest of the CLI.

> **Not oracle-verified — and not calibrated against the Spectra oracle at
> all.** Unlike `drift.md`/`archive.md`/`task.md`, this command's behavior was
> **not** and cannot easily be confirmed against
> `Spectra.app/Contents/MacOS/spectra`: the reference binary is macOS-only, so
> a downstream Linux CI (the use case that motivated this command — see
> [issue #37](https://github.com/howie/openspectra/issues/37)) has no way to
> probe its `validate` surface. Instead, `validate` matches the documented
> **`@fission-ai/openspec` (OSS) 1.5.0 `validate` contract**, so it can act as
> a drop-in replacement for that OSS gate. Treat every detail below as
> OpenSpectra's own design choice against the OSS contract, not a confirmed
> match to the closed binary.

## Why this command exists

The OSS `@fission-ai/openspec` CLI ships a `validate` gate but has a real
limitation: it **cannot traverse nested-capability layouts**
(`specs/<Epic>/<Feature>/spec.md`), so it reports such changes as "no deltas
found" and downstream gates must SKIP them. OpenSpectra's `validate` covers
the same rule set *without* that blind spot, so a single Rust/musl binary can
back both `drift` and `validate` in ubuntu CI and let a downstream repo retire
the Node install.

## CLI shape

```
spectra validate [CHANGE] [--changes] [--strict] [--json]
```

- `CHANGE` — a single change to validate; auto-detects when exactly one active
  change exists (same `change::resolve` behavior as `drift`). Conflicts with
  `--changes`.
- `--changes` — validate every active (non-parked, non-archived) change.
- `--strict` — escalate content-quality findings to hard errors (see below).
- `--json` — emit the machine-readable report instead of the human summary.

## Rules

A change's deltas live in `changes/<name>/specs/**/spec.md`. `validate`
recursively collects **every** `spec.md` beneath that `specs/` root — this is
the nested-layout fix; the capability id reported in issues is the `/`-joined
path from `specs/` to the file's parent (e.g. `Billing/Invoices`). The descent
does **not** follow directory symlinks (it decides recursion with
`symlink_metadata`, not `metadata`): a checked-in cyclic directory symlink
would otherwise recurse without bound and stack-overflow the gate. A `spec.md`
that is itself a symlink is still read — only directory *traversal* stops at
links.

1. **Structural (always an `ERROR`).** A change must contain at least one
   requirement delta: an `### Requirement:` header under an `## ADDED`,
   `## MODIFIED`, or `## REMOVED` section, or a `- TO:` entry under
   `## RENAMED`. A change with no deltas fails with a message containing
   `at least one delta`.
2. **Content quality (an `ERROR` only under `--strict`).** Each ADDED or
   MODIFIED requirement must:
   - state a normative `SHALL` or `MUST` (matched word-boundaried and
     case-sensitively — `MARSHALL` and a lowercase `shall` do not count), and
   - carry at least one `#### Scenario:` block.

   Without `--strict` these findings are not reported, so a non-strict run
   gates purely on structure. This mirrors OSS, where `--strict` is what turns
   content-quality findings into failures; the downstream gate always passes
   `--strict`, so it gets the full rule set.

REMOVED and RENAMED requirements carry no body, so the content-quality rules
do not apply to them — only the structural "counts as a delta" rule does.

## Exit code

`validate` is a **pass/fail gate**, so — unlike `drift`, whose exit code never
encodes analysis severity (see `drift.md` and issue #37) — it exits:

- `0` when every validated change is valid,
- `1` when any change is invalid, and
- `1` on an operational error (e.g. not initialized), via the CLI's normal
  error path.

The JSON is still authoritative; the recommended CI gate keys on
`summary.totals.failed`.

## `--json` shape

Matches the shape the downstream gate already parses
(`scripts/openspec_validate_report.py` in the motivating repo):

```json
{
  "items": [
    {
      "id": "<change-name>",
      "valid": true,
      "issues": [
        { "level": "ERROR", "path": "specs/<cap>/spec.md", "message": "..." }
      ]
    }
  ],
  "summary": { "totals": { "passed": 1, "failed": 0, "total": 1 } }
}
```

`passed`/`total` are OpenSpectra additions alongside the `failed` total the
gate reads; a consumer that only inspects `summary.totals.failed` is
unaffected. Struct field order in the serialized JSON (`level`/`path`/`message`
per issue) is pinned by a unit test so it can't drift silently.

## Known divergences from the OSS validator

- The OSS validator has additional checks (e.g. cross-referencing modified
  requirements against the canonical spec) that are **not** reproduced here;
  `validate` implements the subset the issue #37 gate depends on plus the
  nested-layout traversal. It is intended to *replace* the OSS structural gate,
  not to be byte-identical to every OSS diagnostic.
- Because there is no oracle, the exact `message` wording is OpenSpectra's own
  and is not guaranteed to match OSS strings — gate on `level`/`valid`/
  `summary.totals.failed`, not on message text.
