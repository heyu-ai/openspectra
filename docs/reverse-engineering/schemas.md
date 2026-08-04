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
project/user-level schema file). **`schemas` lists only this one even when a
project schema exists** — probed: with `openspec/schemas/mycustom/schema.yaml`
in place, `schemas` still prints the single `spec-driven (package)` line while
`schema which mycustom` resolves it and labels it `(project)`. So this listing
is not the place to look for custom-schema support.

OpenSpectra has no project/user schema discovery, so its registry —
`spectra_core::schema::schemas()` — always returns this single entry, built
from constants in `schema.rs`:

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

## Which schema a command actually runs (`<spec_dir>/config.yaml`)

`schemas` only lists. The selector every other command obeys is the `schema:`
key of `<spec_dir>/config.yaml`, which `spectra init` writes as
`schema: spec-driven`. Probed on v2.3.1:

| setup | oracle |
|---|---|
| `schema: mycustom` + `<spec_dir>/schemas/mycustom/schema.yaml` | loads it; `status` and `instructions` use its instructions |
| the name `status` prints | the **`name:` field inside schema.yaml**, not the directory name |
| `schema: no-such-schema` | exit 1, `Error: Schema not found: Schema 'no-such-schema' not found in project, user, or built-in locations` |
| `--schema` given as well | the flag wins |

The `name:` row is a trap worth repeating: `spectra schema fork spec-driven
mycustom` copies the definition **without rewriting `name:`**, so a freshly
forked schema loads from `schemas/mycustom/` but still reports itself as
`spec-driven`.

### OpenSpectra: fail loud rather than fall back (#117)

OpenSpectra cannot load a custom schema yet (tracked by #126). Until it can,
`status`, `instructions`, and `new artifact` resolve the same selector and
**refuse to run** on anything but `spec-driven`:

- name resolves nowhere → the oracle's message, byte for byte.
- `schemas/<name>/schema.yaml` exists → a distinct message naming the file and
  #126. Reusing the oracle's "not found in project ... locations" wording here
  would be a false statement, since the schema *is* in the project.

Before #117 the selector had no reader at all: a project naming a custom schema
silently ran the built-in one, exit 0, and `instructions` emitted the generic
instructions with the project's own conventions missing — no warning anywhere.

`schema:` is not the only live key in that file. The oracle also reads
`context:` and `rules:` and surfaces them in `instructions --json` as two extra
top-level fields, which OpenSpectra does not yet emit (#127):

```yaml
context: |
  Tech stack: TypeScript, React
rules:
  tasks:
    - Break tasks into chunks of max 2 hours
```

```json
{"context": "Tech stack: TypeScript, React",
 "rules": ["Break tasks into chunks of max 2 hours"]}
```

`context` is the trimmed scalar; `rules` is flattened to **just the requested
artifact's** list (asking for `proposal` returns the `proposal` rules, not the
whole map). Both keys are absent from the JSON when unset.

That divergence is sharper than a missing JSON field, because **`spectra init`
writes the promise itself**. `init.rs`'s `SPEC_CONFIG_TEMPLATE` is byte-identical
to the oracle's `config.yaml` (verified by diffing both binaries' output in an
empty jail), and it contains:

```yaml
# Project context (optional)
# This is shown to AI when creating artifacts.
```

True of the oracle, false of OpenSpectra. The template must **not** be edited —
byte fidelity with the oracle's `init` output is a pinned contract (#94) — so the
only way to make that comment honest is to implement #127. Until then, a project
that follows the instructions OpenSpectra itself wrote gets no effect and no
warning, which is the same failure shape as the `schema:` fallback #117 fixed.

## Consumer

The `sdd` plugin shells out to `spectra schemas` (and treats
`spectra schemas failed or unavailable` as a drop-in gate). This command exists
to satisfy that probe; the JSON shape above is the contract that plugin reads.
