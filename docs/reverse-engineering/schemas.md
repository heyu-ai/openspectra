# Reverse-engineering `spectra schemas`

How OpenSpectra's `schemas` command mirrors the closed-source reference, and
how every command resolves *which* schema to run. Unlike `init.md`, this is
oracle-verified: every detail below was probed against `spectra 2.3.1 (Apple
Silicon)`. The **listing output** sections are additionally pinned to golden
fixtures (`golden/schemas-2.3.1.{json,txt}`); the selector, check-order,
`new artifact`, project-listing and `context`/`rules` sections below are
probe-backed only — `golden/schemas-2.3.1.json` still holds just the built-in
entry.

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

The oracle ships one *built-in* schema, `spec-driven`, reported with
`source: "package"` (embedded in the binary, as opposed to a project- or
user-level schema file).

**It does not stop there: `schemas` also lists project schemas.** Probed with a
single `openspec/schemas/withdesc/schema.yaml` in place, whose contents are
`name: totally-different`, `description: My very own workflow`, and one
`proposal` artifact:

```console
$ spectra schemas
Available schemas:
  spec-driven (package) — Default OpenSpec workflow - proposal → specs → design → tasks
  withdesc (project)

$ spectra schemas --json
[
  {
    "artifacts": ["proposal", "specs", "design", "tasks"],
    "description": "Default OpenSpec workflow - proposal → specs → design → tasks",
    "name": "spec-driven",
    "source": "package"
  },
  {
    "artifacts": ["proposal"],
    "description": null,
    "name": "withdesc",
    "source": "project"
  }
]
```

Three rules #126 will need, all visible in that one probe:

- **`description` is forced to null** for a project entry — nothing follows the
  `(project)` tag in the human form, and `--json` says `null`. Not because the
  file lacks one: this file has `description: My very own workflow`.
- **the listed `name` is the directory name**, not the file's `name:` field
  (`withdesc`, not `totally-different`) — the opposite of what `status` prints
  for the same schema (see below).
- **`artifacts` *is* read from the file** (`["proposal"]` here), so only `name`
  and `description` are overridden.

> An earlier revision of this write-up explained the missing description tail as
> "the forked `schema.yaml` has no `description`". Both halves were wrong:
> `schema fork` copies the `description:` line, and the tail is absent even when
> one is present.

> An earlier revision also claimed the opposite of the headline above ("lists
> only this one even when a project schema exists"). That came from a probe
> whose output was piped through `head -2`, which cut the project line. Read the
> whole output before recording a negative.

OpenSpectra has no project/user schema discovery, so its registry —
`spectra_core::schema::schemas()` — returns only the built-in entry, built from
constants in `schema.rs`. Listing project schemas is part of #126:

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

## Which schema a command actually runs

`schemas` only lists. The selector is resolved per command, and it has **three
layers, not one** — the change's own metadata dominates the project config.
Probed on v2.3.1 by varying the two files independently:

| `<change>/.openspec.yaml` `schema:` | `<spec_dir>/config.yaml` `schema:` | `spectra status --change c1` |
|---|---|---|
| `spec-driven` | `no-such-schema` | **exit 0**, `Schema: spec-driven` — the project config is ignored outright |
| `no-such-schema` | `spec-driven` | exit 1, `Schema not found: …` |
| *key absent* | `no-such-schema` | exit 1 — this is the only case the project config decides |
| `no-such-schema` | `no-such-schema`, plus `--schema spec-driven` | **exit 0** — the flag beats both |

So the order is **`--schema` → the change's `.openspec.yaml` → `<spec_dir>/config.yaml` → built-in**.

This matters more than it looks: `spectra new change` stamps
`schema: <whatever config.yaml said at creation time>` into every change, so in
any real project the change-level key is set and the project config is dead
weight for existing changes. Reading only `config.yaml` therefore looks correct
on a hand-made change directory and is wrong on every generated one.

Two further probed details:

- **Blank values are asymmetric.** A bare `schema:` in the *change's* metadata
  is an explicit empty name — the oracle reports `Schema '' not found`. A bare
  `schema:` in the *project* `config.yaml` is treated as unset and exits 0.
- **The reported name comes from inside the file.** `status` prints the `name:`
  field of the resolved `schema.yaml`, not the directory name. That is a trap:
  `spectra schema fork spec-driven mycustom` copies the definition **without
  rewriting `name:`**, so a freshly forked schema loads from
  `schemas/mycustom/` yet still reports itself as `spec-driven`.

### Check order: the change comes first

The schema error is **not** the first thing a command reports. Probed:

```console
$ spectra status --change ghost        # config.yaml names an unknown schema
Error: Change 'ghost' not found.       # exit 1 — schema never mentioned

$ spectra status --schema bogus        # project with no changes at all
No active changes. Create one with: spectra new change <name>   # exit 0
```

So a command resolves and loads the change, *then* gates on the schema. Any
implementation that gates first reports the wrong error for a bad change name.

### `new artifact` never errors on the selector — but it does resolve it

Probed: with the change's `.openspec.yaml` recording `schema: no-such-schema`,
`spectra new artifact design --change c1` **exits 0**. The command has no
`--schema` flag and no failure path for the selector. An early revision of #117
added a gate here; it was removed because it both diverged from the oracle and
inserted a check ahead of this command's probed sequence in
[`artifact-workflow.md`](artifact-workflow.md) — a reorder PR #48's review had
already ruled out without a supporting probe.

"Never errors" is not "ignores": the oracle resolves the selector for **template
lookup**, and the two ends of that are a live divergence:

| change's `schema:` | oracle writes | OpenSpectra writes |
|---|---|---|
| resolvable custom schema | that schema's template (probed: a `# MARKER-CUSTOM-TEMPLATE` file in `schemas/mycustom/templates/proposal.md` comes out verbatim) | the built-in template — wrong workflow, until #126 |
| unresolvable | a **0-byte** file | the built-in template (1213 bytes for `design`) |

The empty-file case is a deliberate divergence: a usable built-in artifact beats
an empty one, and nothing is destroyed either way. The custom-template case is
#126. `schema_selector_integration.rs` asserts the OpenSpectra side so neither
can drift silently.

### OpenSpectra: fail loud rather than fall back (#117)

OpenSpectra cannot load a custom schema yet (tracked by #126). Until it can,
`status` and `instructions` resolve the selector in the order above and
**refuse to run** on anything but `spec-driven`:

- name resolves nowhere → the oracle's message, byte for byte.
- `schemas/<name>/schema.yaml` exists → a distinct message naming the file and
  #126. Reusing the oracle's "not found in project ... locations" wording here
  would be a false statement, since the schema *is* in the project.

One edge case is deliberately not reproduced: a change whose `schema:` key is
present but null. `ChangeMetadata`'s `Option<String>` cannot distinguish that
from an absent key, so OpenSpectra falls through to the project config where the
oracle reports `Schema '' not found`. Matching it needs `Option<Option<String>>`
on a struct `archive` round-trips, for an input no tool writes.

`crates/spectra-cli/tests/schema_selector_integration.rs` pins every row of the
table above through the real CLI. The unit tests in `schema.rs` cover the same
layers with a synthetic `Change`, but they call `require_supported` directly and
never through a real `.openspec.yaml` on disk — before #117's review the
pre-existing unit tests passed no change at all, which is exactly how the
missing change-level layer went unnoticed.

Before #117 the selector had no reader at all: a project naming a custom schema
silently ran the built-in one, exit 0, and `instructions` emitted the generic
instructions with the project's own conventions missing — no warning anywhere.

`schema:` is not the only live key in that file. The oracle also reads
`context:` and `rules:` and surfaces them in artifact `instructions --json` as
two extra top-level fields. OpenSpectra implements that contract in #127; the
full probe record, including key order and absence semantics, is in
`artifact-workflow.md`:

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

This behavior matters because **`spectra init` writes the promise itself**.
`init.rs`'s `SPEC_CONFIG_TEMPLATE` is byte-identical
to the oracle's `config.yaml` (verified by diffing both binaries' output in an
empty jail), and it contains:

```yaml
# Project context (optional)
# This is shown to AI when creating artifacts.
```

The template must **not** be edited — byte fidelity with the oracle's `init`
output is a pinned contract (#94) — so #127 made the existing tutorial comment
true without changing the generated file.

## Consumer

The `sdd` plugin shells out to `spectra schemas` (and treats
`spectra schemas failed or unavailable` as a drop-in gate). This command exists
to satisfy that probe; the JSON shape above is the contract that plugin reads.
