# Reverse-engineering `spectra schema validate` / `spectra schema fork`

This document records the observable contract of `schema validate` and
`schema fork` in the closed-source Spectra CLI v3.0.0 (Apple Silicon), the
OpenSpectra implementation, and the deliberate divergences the owner ruled on
(D11, howie, 2026-09-28). The full probe write-up (probes `p01`–`p15`, every
case with its raw output) is the W7g spec in
`docs/reverse-engineering/specs/w7g-schema/` (PR #218); this file keeps the
contract and the reasons.

Marking follows the spec: **[V]** verified by a probe, **[I]** inferred.

## Reproducing the oracle

`scripts/capture-schema-validate.py` is the verification contract. It needs
macOS and the reference binary (`--spectra-bin` / `SPECTRA_BIN`, version
pinned to 3.0.0). For every directory under
`crates/spectra-cli/tests/fixtures/schema_validate/<case>/` — the W7g probe
inputs, copied from the probe jails: a valid control, 51 single-fault schemas
(`p04`), 23 multi-fault / ordering cases (`p05`), and `one` / `crlf` / `bom`
(`p14`) — it builds a scratch project with the case at `openspec/schemas/m/`
and runs, one scratch project per command:

```text
schema validate m | schema validate m --json | schema validate m --verbose
schema fork m m2
```

It also forks both built-ins into an empty project and records every file
written. The result is
`docs/reverse-engineering/golden/schema-validate-3.0.0.json` (scratch path
replaced by `<ROOT>`). Drift exits non-zero and keeps the scratch dir;
`--write` captures twice and writes only when both agree. The golden is
generated — never hand-edit it; add a fixture case and recapture.

`crates/spectra-cli/tests/schema_validate_golden_integration.rs` replays all
77 × 4 runs against OpenSpectra and compares exit code, stdout and stderr byte
for byte. Deliberate divergences are listed in its `divergence` table with
OpenSpectra's literal output; the table is a ratchet (a listed case that
matches the oracle fails too).

One case is not in the fixture tree: an absolute `template:` path is specific
to the probe jail, so it is covered by the unit test
`resolved_load_rejects_absolute_template_path`.

## CLI surface [V p01]

```text
spectra schema validate [OPTIONS] [NAME]      --json, --verbose, --no-color
spectra schema fork [OPTIONS] <SOURCE> [NAME] --json, --force, --no-color
```

Help strings match the oracle: "Validate a schema", `[NAME]  Schema name`,
`--json  Output as JSON`, `--verbose  Verbose output`; "Fork (copy) a
schema", `--force  Overwrite if exists`.

## `schema validate`

### Which schema [V p02, p03]

`NAME` omitted means `spec-driven`, unconditionally (D11-6): neither
`config.yaml`'s `schema:` nor a change's `.openspec.yaml` is read, exactly
like `schema which`. Resolution is the normal one — project
`<spec_dir>/schemas/<NAME>/` shadows the user-level directory, which shadows
the built-ins — so a project `spec-driven` that is invalid fails even though
the built-in is fine. A directory counts as a schema when it contains an entry
named `schema.yaml`, even a directory (reading it then fails). Project-root
discovery accepts a bare `openspec/` (PR #211).

### Output [V p02, p03, p07, p14]

`<N>` is the NAME argument (directory name), never the YAML `name:`.

| outcome | stdout | stderr | rc |
|---|---|---|---|
| valid | `✓ Schema '<N>' is valid (<n> artifacts)` (never singular) | empty | 0 |
| valid, `--json` | `{"artifactCount": n, "name": "<N>", "valid": true}` (pretty, sorted keys) | empty | 0 |
| invalid / not found | empty | `Schema '<N>' is invalid: <ERR>` then `Error: Schema validation failed: <ERR>` | 1 |
| invalid, `--json` | `{"error": "<ERR>", "name": "<N>", "valid": false}` | `Error: Schema validation failed: <ERR>` | 1 |

`--verbose` is inert. On a terminal only the `✓` is green; failure lines have
no colour.

### Error catalogue and order [V p04, p05]

Only the first problem is reported, in this order:

1. Read / deserialise: `Schema parse error: <serde_yaml message>` (the
   library's own text, e.g. `artifacts[0]: missing field `id` at line 5
   column 5`), or `Schema parse error: Failed to read <path>: <io error>`.
   Required: `name` (string), `version` (u32, any value), `artifacts`; per
   artifact `id`, `generates`, `description`, `template`; `apply.requires`
   when `apply` is present. Optional: `description`, artifact `instruction`,
   `requires` (default `[]`), the whole `apply`, `apply.tracks`,
   `apply.instruction`. Unknown keys are ignored.
2. `Invalid schema: Duplicate artifact IDs`
3. `Invalid schema: Artifact '<id>' requires unknown artifact '<dep>'`
   (artifacts in file order, then each `requires` in order)
4. `Invalid schema: Apply phase requires unknown artifact '<id>'`
5. `Invalid schema: Schema contains circular dependencies`
6. OpenSpectra only (D11-1, D11-7):
   `Invalid schema: Artifact '<id>' template|generates '<path>' must be a relative path`
   / `… must not contain '..'`

The same `<ERR>` is what `status`, `instructions` and `schema fork` print as
`Error: <ERR>`, because every command loads schemas through
`ResolvedSchema::load`.

### Downstream behaviour of the relaxed fields [V p06, q01, q03]

- A schema without `apply`: the apply phase requires every artifact
  (`instructions apply` lists all of them as missing), with no tracks and no
  instruction.
- An artifact without `instruction`: `instructions --json` omits the
  `instruction` key; the human output omits the `Instruction:` section.
- A missing or empty template: `instructions --json` has `"template": ""`;
  the human output omits the `Template:` section and its preceding blank line.

## `schema fork`

### Output [V p07, p08]

`✓ Forked '<SOURCE>' → '<TARGET>'` on stdout, rc 0 (the `✓` green on a
terminal); `--json` changes nothing. `<TARGET>` defaults to
`<SOURCE>-custom`. Errors are `Error: <msg>` on stderr, rc 1: the unknown or
invalid source's `<ERR>` (checked before anything is written), or
`Schema '<TARGET>' already exists. Use --force to overwrite.`

### Built-in sources [V p08]

The fork is a serialisation of the built-in, byte-identical to the oracle's
except the first line (see divergences). Artifacts are written in
`artifact_order` — spec-driven: proposal, specs, design, tasks (OpenSpectra
used to write the dependency-definition order proposal, design, specs,
tasks). Templates are copied to `templates/<template>`.

## Deliberate divergences (owner ruling D11, 2026-09-28)

| # | area | oracle 3.0.0 | OpenSpectra | why |
|---|---|---|---|---|
| D11-1 | `template:` absolute or containing `..` | valid; `instructions` reads the outside file, `fork` writes outside the target's `templates/` [V p06, p13] | rejected by every command (`Invalid schema: Artifact 'b' template '../x.md' must not contain '..'`) | security: a schema must not read or write outside its own directory |
| D11-7 | `generates:` absolute or containing `..` | valid; passed through as `outputPath` [V p06] | rejected the same way (`… generates '/abs/b.md' must be a relative path`) | same reason as D11-1 (artifacts would land outside the change directory) |
| D11-2 | missing or empty template file | silently valid; fork skips missing, copies empty [V p04, p13] | still valid and forked the same way, but each one prints `Warning: Template '<file>' for artifact '<id>' is missing` / `… is empty` on stderr (validate: only on success, before the result line; fork: after a successful fork). stdout and rc are unchanged | keeps the oracle's acceptance while preserving a useful signal; stderr keeps `--json` stdout machine-readable |
| D11-3 | fork target name | no validation: `../esc` writes outside `schemas/`, `a/b` nests, `""` / `.` point at `schemas/` itself [V p10 E, p15, q02] | rejected: `schema fork target '' must name a directory inside schemas/` (also `.`), `… must not contain '..'`, `… must not contain path separators`, reserved transaction names; nothing is written | a fork must create exactly one directory under `schemas/` |
| D11-4 | `--force` write strategy | writes over the target in place, keeps stale files, leaves a partial tree on failure; a regular-file target fails with `File exists (os error 17)` [V p10 A, q02] | stage into `.<target>.stage-*`, validate, then atomically replace the whole target (directory or file); the target afterwards contains exactly the new fork | atomicity |
| D11-5 | project / user sources | re-serialises the model: comments and unknown keys dropped, `null`s written, only referenced templates copied, a template in a subdirectory fails with `os error 2` [V p09, p13] | the source tree is copied verbatim (comments, extra files, subdirectories kept) and only the top-level `name:` line is rewritten; required fields follow the oracle (no `instruction` / `apply` needed) | a fork is a copy the user owns; re-serialising destroys their comments |
| — | `name:` of the fork | keeps the source name (`name: spec-driven` inside `f1`) [V p08] | rewritten to the target name | pre-existing lead decision: the fork's identity must match its directory |

Rulings D11-6 (validate without NAME → `spec-driven`) and the relaxed required
fields of D11-5 follow the oracle and are not divergences.

## OpenSpectra defects fixed along the way [V p10]

- `schema fork no-spec ""` failed with `Invalid argument (os error 22)` and
  left `schemas/..stage-<pid>-0/` behind, which `schemas --json` then listed.
  Empty and `.` targets are now rejected before anything is created.
- `schema fork no-spec tf --force` onto a regular file leaked
  `schemas/.tf.backup-<pid>-0` (the backup was a file and `remove_dir_all`
  failed silently). The backup is now removed by type and a failure is
  reported.

## Known remaining differences (outside W7g)

Found by probe q03 against the same fixtures, not changed here:

- `status --json` for a project schema adds `isPlanningComplete` and a
  `requires` array per artifact that the oracle does not print (every custom
  schema, including the valid control).
- `instructions apply --json` prints `"instruction": null` when the schema has
  no apply instruction (or no `apply`); the oracle omits the key. The existing
  test `apply_instruction_is_optional` indexes `value["instruction"]`, which
  cannot tell the two apart.
- ~~`schemas` still skips a project schema that fails to load~~ — fixed in
  #226: `schemas` now lists it whenever `schema.yaml` exists, with leniently
  read `artifacts`, as the oracle does. See `schemas.md` "Schemas that fail to
  load" for the probe and the divergences that remain.
