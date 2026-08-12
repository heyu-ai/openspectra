# Reverse-engineering `spectra templates`

This document records the observable `templates` contract from the
closed-source Spectra CLI v2.3.1 (Apple Silicon) and the corresponding
OpenSpectra implementation.

## CLI surface

```text
Show template paths

Usage: spectra templates [OPTIONS]

Options:
      --no-color         Disable colored output
      --schema <SCHEMA>  Schema name
      --json             Output as JSON
```

The command does not require an initialized project. The built-in schema and
its templates are embedded in the binary.

## Reproducing the oracle (2026-08-11)

The probes used `/Users/doxa/.local/bin/spectra`, resolving to Spectra.app
v2.3.1. Each behavior probe ran in a separate empty temporary directory.

### Text

`spectra templates --no-color` exits 0, writes nothing to stderr, and emits
exactly 131 bytes including its final LF:

```text
Templates (spec-driven)
  ✓ proposal → proposal.md
  ✓ specs → spec.md
  ✓ design → design.md
  ✓ tasks → tasks.md
```

### JSON

`spectra templates --json` exits 0, writes nothing to stderr, and emits a
2-space pretty-printed array of exactly 374 bytes including its final LF:

```json
[
  {
    "artifactId": "proposal",
    "hasContent": true,
    "templateName": "proposal.md"
  },
  {
    "artifactId": "specs",
    "hasContent": true,
    "templateName": "spec.md"
  },
  {
    "artifactId": "design",
    "hasContent": true,
    "templateName": "design.md"
  },
  {
    "artifactId": "tasks",
    "hasContent": true,
    "templateName": "tasks.md"
  }
]
```

Object field order is `artifactId`, `hasContent`, `templateName`. Artifact
order follows the schema authoring order, which differs from the internal
definition order documented in `schemas.md`.

### Unknown schema

`spectra templates --schema bogus --json` exits 1 with empty stdout and this
newline-terminated stderr:

```text
Error: Schema not found: Schema 'bogus' not found in project, user, or built-in locations
```

The built-in schema has content for all four templates. A reproducible
`hasContent:false` case was not established for v2.3.1; custom-schema template
loading belongs to issue #126 and must be probed with that runtime rather than
inferred here.

### Color

When color is enabled (a TTY, no `--no-color`, no `NO_COLOR`), the oracle
bolds only the word `Templates` in the header:

```
\x1b[1mTemplates\x1b[0m (spec-driven)
```

The `(spec-driven)` suffix and every listing line (marker, artifact id, arrow,
template name) are left uncolored — unlike `schemas`, whose whole header is
bolded and whose `(source)` tag is dimmed (see `schemas.md`'s Color section).
Probed by forcing a TTY (`script -q /dev/null spectra templates`) and
confirming the exact byte sequence with `xxd`. OpenSpectra reproduces this
with the shared `colorize(text, sgr_code, use_color)` helper wrapping only
`"Templates"` with SGR code `1` (bold).

## OpenSpectra implementation

`spectra_core::templates` derives the four entries from the embedded schema
registry and renders them through the thin CLI. It is available outside a
project, preserves the oracle's field/artifact order and whitespace, and uses
the existing fail-loud schema selector. Until custom schemas are implemented,
a present custom schema remains an explicit unsupported-schema error rather
than silently falling back to built-in template metadata.
