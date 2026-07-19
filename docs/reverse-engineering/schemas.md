# Reverse-engineering `spectra schemas`

How OpenSpectra's `schemas` command mirrors the closed-source reference. Unlike
`init.md`, this command **is** oracle-verified: every detail below was probed
against `/Applications/Spectra.app/Contents/MacOS/spectra` (version
`spectra 2.3.1 (Apple Silicon)`) and pinned to golden fixtures.

## CLI shape

```
List available workflow schemas

Usage: spectra schemas [OPTIONS]

Options:
      --json      Output as JSON
      --no-color  Disable colored output
  -h, --help      Print help
```

`--no-color` is a global flag in OpenSpectra (shared with every other command),
so it is accepted here without a per-command declaration. `schemas` does **not**
require an initialized project: the oracle lists its built-in registry from a
bare directory with no `.spectra.yaml`, and OpenSpectra matches that (it skips
`require_initialized`, like `init`).

## Built-in registry

The oracle ships exactly one schema, `spec-driven`, reported with
`source: "package"` (i.e. embedded in the binary, as opposed to a
project/user-level schema file). OpenSpectra has no project/user schema
discovery, so its registry — `spectra_core::schema::schemas()` — always returns
this single entry, built from constants in `schema.rs`:

| Field | Value |
|---|---|
| `name` | `spec-driven` |
| `source` | `package` |
| `description` | `Default OpenSpec workflow - proposal → specs → design → tasks` |
| `artifacts` | `["proposal", "specs", "design", "tasks"]` |

### Artifact order caveat

The `artifacts` list — and the `→`-separated tail of `description` — is
**proposal → specs → design → tasks**, the recommended *authoring* order. This
is deliberately **not** the same as the artifact-*definition* order in
`schema::ARTIFACTS` (proposal, design, specs, tasks), which is what `status` and
`instructions` iterate. `specs` and `design` are swapped: both depend only on
`proposal`, so dependency order alone doesn't fix their relative position, and
the oracle lists `specs` first here (tasks depends on specs, so the authoring
flow reaches specs before design).

Because the two orders genuinely diverge, `SCHEMA_ARTIFACT_ORDER` is maintained
as its own constant rather than derived from `ARTIFACTS`. A unit test asserts it
stays a permutation of the `ARTIFACTS` id set, so adding a fifth artifact can't
silently desync the schema listing.

## Output formats

### `--json` (golden: `golden/schemas-2.3.1.json`)

A pretty-printed array (2-space indent). Object keys are alphabetical —
`artifacts`, `description`, `name`, `source` — which OpenSpectra reproduces by
declaring `SchemaListing`'s fields in that order (serde serializes struct fields
in declaration order, so no `serde_json::Value` BTreeMap sorting is needed). The
CLI's `println!` adds the trailing newline the golden captures.

```json
[
  {
    "artifacts": [
      "proposal",
      "specs",
      "design",
      "tasks"
    ],
    "description": "Default OpenSpec workflow - proposal → specs → design → tasks",
    "name": "spec-driven",
    "source": "package"
  }
]
```

### Text (golden: `golden/schemas-2.3.1.txt`)

Two lines, each newline-terminated:

```
Available schemas:
  spec-driven (package) — Default OpenSpec workflow - proposal → specs → design → tasks
```

- Header: `Available schemas:`
- One indented line per schema (2 spaces): `<name> (<source>) — <description>`,
  where the separator is an em dash (`—`, U+2014) and the workflow arrows in the
  description are `→` (U+2192).

### Color

When color is enabled (a TTY, no `--no-color`, no `NO_COLOR`), the oracle wraps:

- the header in bold — `\x1b[1mAvailable schemas:\x1b[0m`
- each `(source)` tag in faint/dim — `\x1b[2m(package)\x1b[0m`

The em dash and description are left uncolored. OpenSpectra reuses the shared
`colorize(text, sgr_code, use_color)` helper with SGR codes `1` (bold) and `2`
(dim) to reproduce this.

## Consumer

The `sdd` plugin shells out to `spectra schemas` (and treats
`spectra schemas failed or unavailable` as a drop-in gate). This command exists
to satisfy that probe; the JSON shape above is the contract that plugin reads.
