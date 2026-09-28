# Reverse-engineering `list`, `show`, and `schema which` (oracle 3.0.0)

Probed on 2026-09-28 against `spectra 3.0.0 (Apple Silicon)` (W7a in
`docs/migration-plan.md`), in scratch projects with an explicit
`.spectra.yaml` (`spec_dir: openspec`). An acceptance matrix of 29
invocations (every command below, JSON and human, over changes with and
without metadata, summaries of each shape, upper-case and underscore names,
an empty `tasks.md`, nested delta specs, spec directories with several
Markdown files or none, and project/built-in/unknown schemas) matches the
oracle byte for byte.

## `list`

JSON entries: `{completedTasks, name, status, summary?, totalTasks}`, keys
alphabetical (a `serde_json` map). `list --parked --json` wraps the same
entries under `parked` with `status: "parked"`.

**Which directories count.** Every directory under `changes/` except
`archive`, including names with upper-case letters or underscores and
directories without `.openspec.yaml`. OpenSpectra also skips hidden
directories (its own archive staging directories) and `YYYY-MM-DD-` names.
Before this, OpenSpectra silently dropped non-kebab-case names.

**`summary`** comes from `proposal.md` only:

1. A raw line (not trimmed) starting with `## Why`, `## Problem`, or
   `## Summary` starts a section: a case-sensitive prefix match, so
   `## Problems` counts and `### Why` does not. Code fences are not
   recognized. The first matching heading wins, whatever its keyword.
2. The section ends at the next line starting with `## `. Its first line
   that is non-empty after `trim()` and does not start with `<!--` is the
   summary; Markdown is not stripped (`### Sub`, `- item` are taken as is).
3. A section with no such line moves on to the next matching heading; with
   none, `summary` is **absent** (not `null`).
4. Over 30 characters (Unicode scalar values) → the first 30 plus `…`.

**Sort** (`--sort`, default `modified`):

* `modified` — descending by the newest mtime of any regular file under the
  change directory (recursive; directory mtimes do not count), compared in
  whole seconds.
* `created` — descending by the raw `created` string of `.openspec.yaml`
  (a string compare, not a date parse). Metadata counts only when it has
  both `schema` and `created`, as in `show`; otherwise the change sorts
  last. Ties fall back to `modified`.
* `name` — ascending by bytes.

**Deliberate divergence:** the oracle breaks remaining ties by raw `readdir`
order (APFS order, not portable); OpenSpectra breaks them by name, as it does
for `instructions`' `contextFiles`.

**Human:** `Changes:` / `Parked:` then `  • <name>`, plus ` [<done>/<total>]`
whenever `tasks.md` exists (an empty file prints `[0/0]`), plus
` — <summary>`. Empty: `No active changes.` / `No parked changes.`

## `list --specs`

JSON `{"specs": [{"id", "path"}]}`: `id` is the capability name, `path` the
canonicalized absolute path of the spec **directory**. Only directories
containing `spec.md` are listed, by name; `--sort` is ignored. Human:
`Specs:` then `  • <id>`, or `No specs.`

OpenSpectra keeps listing nested capabilities (`identity/auth`) the way it
already did; on the corpus projects the oracle lists the same number of
entries.

## `show`

A change wins over a spec with the same name. Not found:
`Item '<name>' not found as a change or spec.` (exit 1).

**Change** — JSON with all 7 keys always present, alphabetical:
`created, deltaSpecs, design, name, proposal, schema, tasks`.

* `proposal`, `design`, `tasks`: file content, `null` when the file is
  missing, `""` when it is empty.
* `schema`, `created`: from `.openspec.yaml`, only when it parses **and** has
  both keys; otherwise both are `null`. Scalars pass through as strings
  (`20260210` → `"20260210"`). No warning is printed for an unparseable file.
* `deltaSpecs`: every `*.md` anywhere under `specs/`, relative to it with `/`
  separators, sorted by bytes.

Human: `Change: <name>`, then `Schema: <s>` / `Created: <c>` when the metadata
parsed, then `\n--- Proposal ---\n<content>\n` when `proposal.md` exists,
then `\n--- Delta Specs ---\n` with `  <path>` lines when there are any.
`design.md` and `tasks.md` are not printed.

**Spec** — any directory under `specs/` resolves, even without `spec.md`.
JSON `{"files": [{"content", "name"}], "name"}` with every `*.md` in the
directory, recursively, sorted. Human: `Spec: <name>`, then
`\n--- <file> ---\n<content>\n` per file.

OpenSpectra's `show --diff` is its own extension and unchanged. The oracle's
`--item-type`, `--deltas-only`, and `-r/--requirements` are not implemented
yet (W12).

## `schema which [NAME]`

JSON `{"name", "resolved", "sources": [{"path", "source"}]}`, exit 0 in every
case, and no initialized project is needed. `sources` lists every location
holding `<name>/schema.yaml` (validity is not checked), in precedence order:

1. project — `<root>/<spec_dir>/schemas/<name>/schema.yaml`
2. user — `$HOME/Library/Application Support/openspec/schemas/<name>/schema.yaml`
   on macOS (OpenSpectra uses `$XDG_DATA_HOME/openspec/schemas` when that is
   set, and `~/.local/share/openspec/schemas` on other platforms)
3. built-in — `{"path": "(embedded in binary)", "source": "built-in"}`, only
   for `spec-driven`; the oracle reports `no-spec` with `resolved: null` and
   no sources, and OpenSpectra reproduces that quirk.

`resolved` is the first source, or `null`. Without a name the command reports
`spec-driven` (not the configured schema); `--all` changed nothing in any
probe. Human: `Schema: <name>`, then `  → <path> (<source>)` for the first
source and `    <path> (<source>)` for the rest, or `Not found.`
