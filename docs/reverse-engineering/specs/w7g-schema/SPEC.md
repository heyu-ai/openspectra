# W7g oracle spec — `spectra schema validate` / `spectra schema fork` (spectra 3.0.0 vs OpenSpectra w7f build)

Oracle: `/Applications/Spectra.app/Contents/MacOS/spectra` (`spectra 3.0.0 (Apple Silicon)`).
OpenSpectra ("ours"): `.claude/worktrees/w7f-custom-schema-apply/target/release/spectra` (built 2026-09-28 16:05).
Probe date: 2026-09-28 ~16:10 +0800. All probes live in this directory: `lib.sh` (jail helpers,
`GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1`, identity T/t@x, `run3` prints stdout / stderr /
rc separately), `matrix.py` (one-jail-per-case driver for the invalid-schema matrix; case lists in
`p04_cases.py`, `p05_cases.py`; `summ.py` condenses `p04.out` into `p04.summary`), `pNN_*.sh` + `pNN.out`.
Unless stated otherwise a jail = `.spectra.yaml` (`spec_dir: openspec`), `openspec/config.yaml`
(`schema: spec-driven`), `openspec/{changes/archive,specs}`, a git repo with one commit; matrix
jails also hold a change `c1` whose `.openspec.yaml` says `schema: m` and a project schema dir
`openspec/schemas/m/`. The "BASE" test schema (valid) is in `p04_cases.py`: two artifacts `a`, `b`
(`b` requires `a`), `apply.requires: [b]`, templates `a.md`, `b.md`.

Marking: **[V pNN]** verified by that probe; **[I]** inferred (consistent with probes, not isolated).
Streams: "stdout"/"stderr" are exact; `rc` = exit code.

---

## 1. CLI surface [V p01]

```
spectra schema validate [OPTIONS] [NAME]     # --json, --no-color, --verbose
spectra schema fork [OPTIONS] <SOURCE> [NAME] # --json, --no-color, --force
```
Oracle help strings: validate = "Validate a schema", `[NAME]  Schema name`, `--json  Output as JSON`,
`--verbose  Verbose output`; fork = "Fork (copy) a schema", `<SOURCE>  Source schema`,
`[NAME]  New schema name`, `--json  Output as JSON`, `--force  Overwrite if exists`.
Ours: validate's about is "Validate one schema, or every project schema when omitted" and the
validate/fork flags have no help text (fork's arguments already match).
Extra positional → clap usage error, rc 2 [V p14].

---

## 2. `schema validate`

### 2.1 Which schema is validated [V p02, p03]

**`NAME` omitted ≡ `NAME` = `spec-driven`, unconditionally.** The oracle does not read
`<spec_dir>/config.yaml` `schema:` nor any change's `.openspec.yaml`:

| layout | oracle validates | ours validates |
|---|---|---|
| config `schema: spec-driven` (p02 A) | spec-driven | every project schema (none → prints nothing, `[]`, rc 0) |
| config `schema: mine`, `schemas/mine` valid (p02 B) | spec-driven | `mine` |
| config without `schema:` / config.yaml absent (p02 C, C2) | spec-driven | every project schema |
| change `.openspec.yaml` `schema: mine` (p02 D) | spec-driven | every project schema |
| config `schema: no-spec` / `schema: nosuch` (p02 E, F) | spec-driven (rc 0) | nothing, rc 0 |
| two project schemas s1, s2 (p02 G) | spec-driven | `✓ s1` `✓ s2` |
| project `schemas/spec-driven/` that is invalid (p03 C) | the **project** one → fails rc 1 | same schema, fails |

So `schema validate` behaves exactly like `schema which` (which also defaults to `spec-driven`,
see the W7 spec §8). "Validate the configured schema" was never the oracle's behaviour.

### 2.2 Resolution [V p03, p06, p12]

- Lookup is the normal schema resolution: project `<spec_dir>/schemas/<NAME>/schema.yaml` shadows a
  user-level schema [I, not probed — see Open questions] which shadows the built-ins. A project
  schema named `spec-driven` or `no-spec` fully replaces the built-in; there is no fallback when it
  is invalid (p03 C: rc 1 even though the built-in is fine; p03 D: project `no-spec` with 2
  artifacts reports `(2 artifacts)`).
- `spec_dir` from `.spectra.yaml` is honoured: with `spec_dir: docs/spec`, `docs/spec/schemas/mine`
  is found and `openspec/schemas/other` is **not** (p03 B).
- A directory is a schema only if it contains an entry named `schema.yaml`: a dir with only
  `templates/`, or with `schema.yml`, is "not found" (p04 no-schema-yaml, schema-yml). If
  `schema.yaml` exists but is a directory, it counts as found and fails to read (p05
  schema-dir-is-file, §2.4).
- No name sanitising: `../x`, `a/b`, `""`, `Spec-Driven` all end in "not found" (p03). (Case
  sensitive on this APFS volume [V `Spec-Driven`].)
- Project-root discovery does not need `.spectra.yaml` [V p02 H2, p12]: walking up from cwd, the
  first directory that contains `.spectra.yaml` (then `spec_dir` from it) or an `openspec/`
  directory is the project root; at equal depth `.spectra.yaml` wins (p12 E), a nearer bare
  `openspec/` beats a farther `.spectra.yaml` (p12 D), and a `.git` in between does not stop the
  walk (p12 C, p10 G). The per-level check order is [I] from p12 D/E. Ours requires
  `.spectra.yaml` (`Error: Not initialized. Run 'spectra init' first.`, rc 1) for both commands
  (p02 H, p10 G, p12 F). This is the general oracle project discovery, not specific to `schema`.

### 2.3 Output contract [V p02, p03, p07, p14]

Let `<N>` = the **NAME argument** (the directory name), never the YAML `name:` (p04 name-mismatch:
dir `m`, `name: other` → `Schema 'm' is valid`; `status` reports `schemaName: other`, p06).

**Valid** — rc 0, stderr empty:
- human stdout: `✓ Schema '<N>' is valid (<count> artifacts)\n` — `<count>` is `artifacts.len()`,
  never pluralised (`(1 artifacts)` p14, `(0 artifacts)` p04 empty-artifacts).
- `--json` stdout (pretty, 2-space, keys alphabetical, trailing `\n`):
  ```json
  {
    "artifactCount": 2,
    "name": "m",
    "valid": true
  }
  ```

**Invalid or not found** — rc 1:
- human: stdout **empty**; stderr is two lines:
  ```
  Schema '<N>' is invalid: <ERR>
  Error: Schema validation failed: <ERR>
  ```
- `--json`: stdout
  ```json
  {
    "error": "<ERR>",
    "name": "<N>",
    "valid": false
  }
  ```
  and stderr is **only** `Error: Schema validation failed: <ERR>\n` (the `Schema '<N>' is invalid`
  line is not printed). No `artifactCount` key on failure, no `error` key on success.
- `<ERR>` for a missing schema: `Schema not found: Schema '<N>' not found in project, user, or built-in locations`.

`--verbose` changes nothing in any probed case (valid, invalid, not found; human and JSON) [V p02,
p03, p04 valid/yaml-syntax/name-mismatch]. `--json --verbose` = `--json`.

Colour [V p07, via `script(1)` PTY]: on a terminal only the `✓` is wrapped (`ESC[32m✓ESC[0m`);
`--no-color` removes it. The failure lines carry no colour and no `✗` glyph. Piped output has no
escapes.

Ours (for contrast): prints `✓ <N>` / `✗ <N>` + indented issue lines on **stdout** (failures too),
`--verbose` appends ` (<path>)` (`()` for a missing schema), `--json` is an **array** of
`{name, valid, path, issues}` (declaration order), no stderr line, rc 1 on any failure.

### 2.4 Error catalogue (`<ERR>` values) [V p03, p04, p05, p14]

Two families, both are single strings; only the **first** problem is ever reported.

**`Schema parse error: …`** — reading/deserialising `schema.yaml` (serde_yaml messages, verbatim):

| input | `<ERR>` |
|---|---|
| YAML syntax (`artifacts: [` unterminated) | `Schema parse error: did not find expected node content at line 3 column 3, while parsing a flow node` |
| tab indentation | `Schema parse error: found a tab character that violates indentation at line 6 column 1, while scanning a plain scalar at line 5 column 9` |
| empty file | `Schema parse error: missing field `name`` |
| top-level key missing | `Schema parse error: missing field `name`` / `` `version` `` / `` `artifacts` `` (no position) |
| artifact field missing | `Schema parse error: artifacts[0]: missing field `id` at line 5 column 5` (position = first key of that sequence item; same for flow style `- {…}`, p05 art2-missing → `artifacts[1]`) — likewise `generates`, `description`, `template` |
| `apply:` present without `requires` | `Schema parse error: apply: missing field `requires` at line 19 column 3` |
| `version: x` | `Schema parse error: version: invalid type: string "x", expected u32 at line 2 column 10` |
| `name: [x]` | `Schema parse error: name: invalid type: sequence, expected a string at line 1 column 7` |
| `requires: a` (scalar) | `Schema parse error: artifacts[1].requires: invalid type: string "a", expected a sequence at line 16 column 15` |
| `artifacts:` mapping | `Schema parse error: artifacts: invalid type: map, expected a sequence at line 5 column 3` |
| duplicate top-level key | `Schema parse error: duplicate field `name`` |
| `schema.yaml` is a directory | `Schema parse error: Failed to read <absolute path>/schema.yaml: Is a directory (os error 21)` |
| UTF-8 BOM before `name:` | `Schema parse error: missing field `version` at line 1 column 2` (sic; see Open questions) |

In the JSON form the message is JSON-escaped (`\"x\"`). Ours prints only `parsing <path>` for all
of these (the anyhow context, without the cause).

**`Invalid schema: …`** — semantic checks after a successful parse:

| problem | `<ERR>` |
|---|---|
| two artifacts share an id | `Invalid schema: Duplicate artifact IDs` (no id named) |
| `requires` names an unknown id | `Invalid schema: Artifact 'b' requires unknown artifact 'zzz'` |
| `apply.requires` names an unknown id | `Invalid schema: Apply phase requires unknown artifact 'zzz'` |
| dependency cycle (2-cycle, 3-cycle, self-dependency, two disjoint cycles) | `Invalid schema: Schema contains circular dependencies` (no path) |

Ours words all four differently and prefixes the absolute schema.yaml path, e.g.
`Schema '/…/schema.yaml': artifact 'b' requires 'zzz' which is not defined in this schema`,
`…: duplicate artifact ID 'a'`, `…: artifact dependency cycle: a -> b -> a`,
`…: apply.requires references 'zzz' which is not defined in this schema`.

### 2.5 Check order [V p05]

1. read + deserialise; serde reports the first missing field in declaration order
   `name` → `version` → `artifacts` (→ `apply` is optional); inside an artifact `generates` before
   `template`, `description` before `template` (p05 missing-*). A parse error wins over any
   semantic error (p05 parse+semantic).
2. duplicate ids (wins over unknown-requires regardless of list position, over cycle, over apply).
3. unknown `requires`: artifacts in file order, then each artifact's `requires` in order
   (p05 two-requnk, two-requnk-rev, one-art-two-unk → first offender).
4. unknown `apply.requires`, first in list order (p05 two-applyunk).
5. cycle detection (loses to 3 and 4: p05 requnk+cycle, cycle+applyunk).

Ours checks cycle **before** apply.requires (p05 cycle+applyunk: ours reports the cycle, oracle
the apply error), and does not require `version`.

### 2.6 What the oracle does NOT check (all report valid, rc 0) [V p04, p05, p14]

- missing `description` / `description: ~`; missing `apply` entirely; missing artifact
  `instruction`; missing artifact `requires` (defaults to `[]`); missing `apply.tracks` /
  `apply.instruction`; `apply.requires: []`.
- unknown keys at top level, in an artifact, in `apply` (all ignored).
- `version` value: any u32 (`0`, `2`, `7`) is accepted; only the type is checked.
- `name:` differing from the directory name; `name: ''`.
- templates: `templates/<t>` missing, empty, whole `templates/` dir missing; `template: ../x.md`,
  `template: ../nope.md` (absent), absolute `template:`, `template: sub/b.md`.
- `generates:` absolute (`/abs/b.md`), with `..` (`../b.md`), glob (`specs/**/*.md`).
- `artifacts: []` → `(0 artifacts)`.
- empty id `''`, id `'B C/..'`; `requires: [a, a]`.
- CRLF line endings.

Ours diverges in both directions [V p04 OURS lines]: it **accepts** a missing `version` (oracle
rejects, §2.4), and it **rejects** (rc 1) these oracle-valid schemas: missing `apply`, missing
artifact `instruction` (both as `parsing <path>`); missing/empty template and missing `templates/` (ours: `Template 'b.md' is missing
or empty for artifact 'b'`); template/generates absolute or containing `..` (ours:
`schema template '../x.md' must not contain '..'`, `schema generates '/abs/b.md' must be a relative
path`). Ours accepts `template: sub/b.md` like the oracle.

### 2.7 Other commands on the same schemas [V p04, p06]

For every case in §2.4, `status --change c1 --json`, `instructions a --change c1 --json` and
`instructions apply --change c1 --json` fail with rc 1 and stderr `Error: <ERR>` (the same string,
without the `Schema validation failed: ` prefix); stdout empty. `schemas --json` still lists the
schema (rc 0) whenever `schema.yaml` exists, even unparseable (p04: `names=['spec-driven',
'no-spec', 'm']`); it drops it only when `schema.yaml` is absent. For every "valid" case in §2.6
these commands succeed, with these observable consequences (p06):

| case | observed |
|---|---|
| template missing / empty / no templates dir | `instructions b --json` → `"template": ""`; human output has no `Template:` section; `templates --schema m` shows `✗ b → b.md` |
| `template: ../x.md` (exists), absolute template | **the file outside the schema dir is read**: `"template": "# X\n"`, `"# ABS\n"`; `templates` shows `✓ b → ../x.md` / `✓ b → /abs/path` |
| `generates: /abs/b.md` / `../b.md` | passed through: `"outputPath": "/abs/b.md"`; `status` lists `✗ b (/abs/b.md)` |
| no `apply` | `instructions apply --json` rc 0, `state: "blocked"`, no `instruction` key, `progress` all 0 |
| no artifact `instruction` | `instructions a --json` → `"instruction": null` |
| no description / `~` | `schemas --json` → `"description": null` (project schemas always null there, known) |
| `name:` ≠ dir | `status` `schemaName` = YAML name (`other`, or `""`); `schemas`/`which`/`validate` use the dir name |
| `artifacts: []` | `status` rc 0; `instructions a` → `Error: Artifact 'a' not found in schema`, rc 1 |

### 2.8 Inferred oracle deserialisation model [I from §2.4–2.6]

```rust
struct Schema { name: String, version: u32, description: Option<String>,
                artifacts: Vec<Artifact>, apply: Option<Apply> }            // unknown keys ignored
struct Artifact { id: String, generates: String, description: String, template: String,
                  instruction: Option<String>, #[serde(default)] requires: Vec<String> }
struct Apply { requires: Vec<String>, tracks: Option<String>, instruction: Option<String> }
```
The same model is serialised by `fork` (§3.4), which confirms field order and the `Option` fields
(they serialise as `null`).

---

## 3. `schema fork <SOURCE> [NAME]`

### 3.1 Output [V p07, p08, p10]

- Success: stdout `✓ Forked '<SOURCE>' → '<TARGET>'\n` (U+2192), rc 0, stderr empty. On a TTY
  only the `✓` is green (`ESC[32m✓ESC[0m`) [V p07]. `--json` changes nothing (same human line)
  [V p08] — ours matches both.
- `<TARGET>` defaults to `<SOURCE>-custom` (`spec-driven-custom`, `no-spec-custom`, `m-custom`)
  [V p08, p09]; ours matches.
- Errors are a single stderr line `Error: <msg>`, rc 1, stdout empty:

| situation | oracle `<msg>` | ours |
|---|---|---|
| unknown source | `Schema not found: Schema 'nosuch' not found in project, user, or built-in locations` | same |
| invalid project source | the §2.4 `<ERR>` verbatim (e.g. `Invalid schema: Artifact 'b' requires unknown artifact 'zzz'`, `Schema parse error: name: invalid type: …`) | path-prefixed / `parsing <path>: …` wording |
| target dir exists, no `--force` (includes `fork m m`) | `Schema 'm' already exists. Use --force to overwrite.` | same |
| target exists as a regular file, no `--force` | `Schema 'tf' already exists. Use --force to overwrite.` | same |
| target is a regular file, `--force` | `File exists (os error 17)` (nothing changed) | replaces the file with the schema dir, **leaks `.tf.backup-<pid>-0`** |
| a template in a subdirectory (`sub/deep.md`) | `No such file or directory (os error 2)` — partial fork left behind (§3.5) | succeeds |

Validation of the source happens before anything is written (invalid source → no target dir,
p10 D). The source is resolved exactly like `validate` (project shadows built-in).

### 3.2 Target name handling [V p10 E, p15]

The oracle performs **no validation** of `<TARGET>`; it is joined onto `<spec_dir>/schemas/`:

| `<TARGET>` | oracle result | ours |
|---|---|---|
| `../esc` | rc 0, writes `openspec/esc/` (outside `schemas/`) | `Error: schema fork target '../esc' must not contain '..'` |
| `a/b` | rc 0, writes `schemas/a/b/` (not listed by `schemas`) | `…must not contain path separators` |
| `""` with `schemas/` existing | `Error: Schema '' already exists. Use --force to overwrite.` | `Error: Invalid argument (os error 22)` **and leaks `schemas/..stage-<pid>-0/`**, which then shows up in `schemas --json` |
| `""` with no `schemas/` yet | rc 0, `✓ Forked 'no-spec' → ''`, writes `schemas/schema.yaml` + `schemas/templates/` (p15) | — |
| `.hidden`, `UPPER`, `x y`, `-dash` (after `--`), `x.yaml` | rc 0, created | same |
| `spec-driven`, `no-spec` (shadow the built-ins) | rc 0, created | same |

Side effect worth knowing: `fork no-spec spec-driven` keeps `name: no-spec` inside (§3.4), and the
oracle's `schemas --json` then lists `no-spec` twice and no `spec-driven` [V p10 E].

### 3.3 Existing target and `--force` [V p09, p10 A/B]

- `--force` with no existing target = plain fork.
- `--force` onto an existing directory **writes over it in place and deletes nothing**: after
  `fork no-spec t --force` onto a `t` that had `EXTRA.txt`, `templates/{a,b,old}.md`, the result is
  the new `schema.yaml` + `templates/{design,proposal,tasks}.md` **plus** the stale
  `EXTRA.txt`, `templates/a.md`, `b.md`, `old.md`.
- `fork m m --force` (onto itself) succeeds: `schema.yaml` is re-serialised in place (comments and
  unknown keys disappear, `git status` shows ` M …/schema.yaml`), templates rewritten with the same
  content, other files kept.
- Not atomic: a failure mid-way leaves the partially written target (§3.5).

Ours: stages into `.<target>.stage-<pid>-<n>`, validates, then swaps the whole directory
(extras removed), i.e. atomic replace — a deliberate design, but observably different.

### 3.4 `schema.yaml` content: always a re-serialisation [V p08, p09, p10 C, p15]

The oracle never copies `schema.yaml`; it loads the source into the §2.8 model and writes it back
with serde_yaml (block style, `- id:` at column 0, `requires: []` or block list, `|` block
scalars for multi-line strings). Consequences:

- `name:` keeps the **source** name (`name: spec-driven` in a fork called `f1`; `name: m` in `m2`).
  Ours rewrites it to the target — **deliberate, keep** (lead's instruction).
- `version` value kept (`version: 7` → `version: 7`).
- comments, unknown top-level/artifact/apply keys, flow style → dropped / normalised.
- absent optionals are written explicitly: `description: null`, artifact `instruction: null`,
  missing `requires` → `requires: []`, `apply` absent → `apply: null`, `tracks: null`,
  apply `instruction: null`.
- artifact order = source order.
- files other than `schema.yaml` and referenced templates are **not** copied (`NOTES.txt`,
  `.hidden`, unreferenced `templates/sub/x.md` all absent in `m2`, p09).

Ours copies a project schema's tree verbatim (then rewrites `name:`), and — because its
`SchemaYaml` requires `instruction` and `apply` — **refuses to fork** a source the oracle accepts
(p09 ours: `Error: parsing …/schema.yaml: artifacts[0]: missing field `instruction``).

### 3.5 Templates [V p10 C, p13, p15]

For each artifact, in order, the oracle reads `<source>/templates/<template>` and writes
`<target>/templates/<template>` (the `template:` string joined without sanitising on both sides):

| source template | oracle | ours |
|---|---|---|
| present | copied byte-identical | same |
| empty file | written as 0 bytes, rc 0 | `Error: Template 'b.md' is missing or empty for artifact 'b'` |
| missing | **silently skipped**, rc 0 (target lacks it) | same error as above |
| all missing / no `templates/` | rc 0, no `templates/` dir created in the target (p15) | error |
| `sub/deep.md` (exists) | `Error: No such file or directory (os error 2)`, rc 1, target left with `schema.yaml` + earlier templates (parent dir not created) | copied |
| `../outside.md` | reads `schemas/m/outside.md`, writes `schemas/m2/outside.md` (outside `templates/`) | rejected (`must not contain '..'`) |
| `../../../../esc.md` | reads and rewrites `<jail>/esc.md` (same path, same bytes) | rejected |
| absolute path | reads and rewrites that same absolute file | rejected (`must be a relative path`) |
| two artifacts share `a.md` | one file, rc 0 | same |

(`p10 C` combined several of these; its `os error 2` is explained by the `sub/deep.md` artifact.)

### 3.6 Forks of the built-ins — golden bytes [V p08]

Oracle output is kept in `golden/oracle/{f1,f2,spec-driven-custom,no-spec-custom}/` (ours in
`golden/ours/`). Files written: `schema.yaml` + `templates/{proposal,spec,design,tasks}.md`
(spec-driven) or `templates/{proposal,design,tasks}.md` (no-spec).

| file | bytes | sha256 |
|---|---|---|
| f1/schema.yaml (`fork spec-driven f1`) | 13194 | `d7643b6e95ef4b38e198f1f6ad4e6759b911334e85ac2676888261fc70aa2f59` |
| f1/templates/proposal.md | 1028 | `91afffc609cc94edc4c595cd163d13520f94a5381a5ab8b12a5408cd06a2ad9f` |
| f1/templates/spec.md | 764 | `6271f0867a8d53bcb83b598ac7dbc90a25f154352f0abc8df1e2b8535feb6a07` |
| f1/templates/design.md | 2369 | `a1ab361459a5bd1a5d9b80f10a8dba5b22d02992f3e25dcf05c18ca7bc543d81` |
| f1/templates/tasks.md | 780 | `f1b813f25866113382bb80f7560e8b8115bf9efe82b58151add4a3a924a90aae` |
| f2/schema.yaml (`fork no-spec f2`) | 3110 | `72569f14e07066de36a287d9244fa7846498659edb1d2aa832a07291255af909` |
| f2/templates/proposal.md | 525 | `e604f60f744635bd7a699145da13fa0cdb33271fe2877a9ad44d6ff0d22c6087` |
| f2/templates/design.md | 726 | `d08c6521555983827c9191b1ededfe0ab76f784444e3cf06f2598eea3bd2bc06` |
| f2/templates/tasks.md | 609 | `0896a6f7dc974c6746a9c73c219e06af1759258f586c3d740def41dd65970757` |

`schema.yaml` skeleton (spec-driven, LF, ends `need clarification.\n`): `name: spec-driven`,
`version: 1`, `description: Default OpenSpec workflow - proposal → specs → tasks (design optional)`,
`artifacts:` in the order **proposal, specs, design, tasks**, then
`apply:\n  requires:\n  - tasks\n  tracks: tasks.md\n  instruction: |`. The embedded built-in YAML
in the binary lists the same order (`embedded.py`: proposal, specs, design, tasks; its
`apply.requires` is flow style `[tasks]`, so the fork is a re-serialisation, not a copy).
no-spec order: proposal, design, tasks.

`diff -r golden/oracle golden/ours` [V p08]: every template is identical; `no-spec` forks differ
only in line 1 (`name:`, deliberate); `spec-driven` forks additionally differ in artifact order —
ours writes **proposal, design, specs, tasks**. That order is the only thing to fix for built-in
forks: take the embedded-YAML order (proposal, specs, design, tasks). With that and the name line
masked, ours would be byte-identical [I: the remaining lines already match].

Fork never touches `config.yaml` (git status shows only the new files) [V p08].

### 3.7 Where the fork is written [V p10 G/H, p12]

`<project root>/<spec_dir>/schemas/<TARGET>/`, root found by the walk in §2.2; `spec_dir` from
`.spectra.yaml` (`docs/spec/schemas/h1`, p10 H). Running the oracle in a jail with no
`.spectra.yaml`/`openspec/` wrote into an **ancestor** `…/tmp/openspec/schemas/g1` (p11); that
stray directory was removed after the probe. Ours refuses (`Not initialized`).

---

## 4. Porting checklist (oracle vs ours, by observable)

| # | area | oracle | ours today |
|---|---|---|---|
| 1 | validate, no NAME | validates `spec-driven` only | every project schema |
| 2 | validate success, human | `✓ Schema '<N>' is valid (<n> artifacts)` | `✓ <N>` |
| 3 | validate success, JSON | object `{artifactCount,name,valid}` | array of `{name,valid,path,issues}` |
| 4 | validate failure, human | stdout empty; stderr `Schema '<N>' is invalid: <ERR>` + `Error: Schema validation failed: <ERR>` | stdout `✗ <N>` + issues |
| 5 | validate failure, JSON | stdout `{error,name,valid:false}`; stderr `Error: Schema validation failed: <ERR>` | array, no stderr |
| 6 | `--verbose` | inert | appends ` (<path>)` |
| 7 | error wording / order | §2.4, §2.5 | different strings, path-prefixed, cycle before apply |
| 8 | accepted schemas | §2.6 (version required; apply, instruction optional; templates/paths unchecked) | rejects missing apply / instruction / templates / `..` / absolute; accepts missing version |
| 9 | fork of built-in | order proposal, specs, design, tasks | proposal, design, specs, tasks |
| 10 | fork of project schema | re-serialise model, only referenced templates, keep name | verbatim tree copy + name rewrite; refuses sources without `instruction`/`apply` |
| 11 | fork template edge cases | §3.5 | §3.5 |
| 12 | fork target names / `--force` | §3.2, §3.3 | §3.2, §3.3 |
| 13 | no `.spectra.yaml` | walks up to `openspec/` | `Not initialized` |

OpenSpectra defects found along the way (independent of fidelity) [V p10]:
- `schema fork no-spec ""`: fails with `Invalid argument (os error 22)` and leaves
  `schemas/..stage-<pid>-0/` behind; `schemas --json` then lists `..stage-<pid>-0`
  (`is_transient_schema_dir` does not recognise the empty-target stage name).
- `schema fork no-spec tf --force` where `schemas/tf` is a regular file: succeeds but leaks
  `schemas/.tf.backup-<pid>-0` (the backup is a file; `remove_dir_all` fails and the error is
  discarded).

---

## 5. Needs a human decision

These are fidelity-vs-safety or architecture trade-offs (CLAUDE.md "Agent conduct"); the probes
only establish what the oracle does.

1. **Template path traversal / absolute templates.** The oracle validates them as fine, *reads*
   files outside the schema dir into `instructions` output, and `fork` *writes* outside the
   target's `templates/` (§2.6, §2.7, §3.5). Ours rejects them everywhere. Port or keep the guard?
2. **Missing / empty templates.** Oracle: valid; fork skips missing, copies empty. Ours: invalid
   and fork refuses. Dropping the check is pure fidelity, but it also removes a useful signal.
3. **Fork target name.** Oracle: no validation (`../esc` escapes `schemas/`, `""` writes into
   `schemas/` itself). Ours: rejects `..` and separators. Keep ours (and fix the `""` leak)?
4. **Fork write strategy.** Oracle: in-place, non-atomic, `--force` merges (stale files survive),
   partial output on failure. Ours: staged + atomic whole-directory replace. Architecture choice.
5. **Fork of a project schema.** Oracle re-serialises (drops comments, unknown keys, extra files,
   unreferenced templates; writes `null`s). Ours copies verbatim. Given ours already deviates on
   `name:` deliberately, decide whether "fork = copy" or "fork = re-serialise" is the contract.
6. **`validate` without NAME.** Porting the oracle means `spectra schema validate` in a project
   whose config says `schema: mine` validates `spec-driven`, not `mine` — surprising but the
   oracle's behaviour (§2.1). Ours currently gives a different (more useful) answer.

---

## 6. Open questions

- User-level schema location and precedence (the error text says "project, user, or built-in");
  not probed to avoid writing under `$HOME`. `schema which no-spec --all --json` from a jail with no
  user schema showed `resolved: null, sources: []` (p11), i.e. no user dir was reported.
- Behaviour with **no** `openspec/`/`.spectra.yaml` anywhere up to `/` could not be probed:
  `…/9eb90dff/tmp/openspec/` (a pre-existing unrelated directory) is an ancestor of every jail.
- The UTF-8 BOM case reports `missing field `version` at line 1 column 2` rather than
  `` `name` `` — the mechanism (how the BOM interacts with the first key) is not understood.
- serde_yaml message texts are pinned only for the inputs listed; other type errors follow the same
  library format [I], so ours must use a serde_yaml version producing identical messages (and
  print the full cause, not only the `parsing <path>` context).
- Fork template write order when several templates fail/escape is [I] file order; p10 C's partial
  tree (only `templates/a.md`, no `m2/outside.md` although artifact `c` precedes the failing `d`)
  is not fully explained — p13 shows each behaviour in isolation.
- Whether `schemas --json`'s per-entry `name` for a project schema that shadows a built-in is the
  YAML name (p10 E suggests yes: two `no-spec`, no `spec-driven`) while non-shadowing project
  schemas use the directory name (p06 name-mismatch) — seen, not isolated; belongs to `schemas`.

## 7. Probe index

| probe | content |
|---|---|
| p01_help.sh | `--help` of schema / validate / fork, both binaries |
| p02_validate_basic.sh | default schema selection, `--json`, `--verbose`, no `.spectra.yaml` |
| p03_validate_notfound.sh | not-found names, stream split, custom `spec_dir`, project shadowing built-ins |
| p04_cases.py (+matrix.py, p04.summary) | 52 cases (1 control + 51 single-fault schemas): validate human/JSON, status, instructions, schemas; ours validate |
| p05_cases.py | multi-fault ordering, serde field order, schema.yaml as a directory |
| p06_consequences.sh | downstream behaviour of "valid but odd" schemas |
| p07_tty.sh | colours on a PTY |
| p08_fork.sh | built-in forks, golden capture into `golden/`, diff vs ours |
| p09_fork_project.sh | project-schema fork (re-serialisation, extras, self fork) |
| p10_fork_edges.sh | `--force` semantics, template edge mix, invalid sources, target names, file target, no project, custom spec_dir |
| p11_find_g1.sh | where the no-project fork went |
| p12_root.sh | project-root discovery |
| p13_fork_templates.sh | fork template cases one per jail, both binaries |
| p14_misc.sh | singular count, JSON trailing bytes, CRLF, BOM, extra positional |
| p15_fork_noapply.sh | fork of a schema without `apply`/templates; empty target with no `schemas/` |
| strings.py, ctx.py, embedded.py, keys.py | binary-string leads and golden skeleton helpers (leads only) |
