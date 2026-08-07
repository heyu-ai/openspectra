# Changelog

All notable changes to this project will be documented in this file.

This project follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Because
OpenSpectra reimplements the closed-source `spectra` CLI, deliberate
divergences from the v2.3.1 oracle are tracked here alongside user-visible
changes.

## [Unreleased]

### Added

- **Artifact `instructions --json` now surfaces `context:` and per-artifact
  `rules:` from `<spec_dir>/config.yaml`** (#127), between `instruction` and
  `locale`, matching the oracle's probed contract: context trimmed (a blank
  value omits the key), rules flattened to the requested artifact's list,
  both keys entirely absent when unset or unmatched, and apply/human output
  unchanged. Projects that filled in the tutorial comments `spectra init`
  itself writes now actually reach the agent, closing the last silent-loss
  gap in the #117 family.
- `scripts/calibrate-anchor-budget.py` — a verification contract for the
  over-cap sampling model, not a printer. Asserts the oracle's exact anchor
  *identities* (not just counts) over twelve cases spanning all four categories
  at 40–137 candidates, including one that would fail if `Function` were
  sampled in document order rather than extraction order; exits non-zero on
  divergence and preserves the failing jail.

### Fixed

- **`spectra drift` now matches the oracle's Symbol stop-list** (#133). A
  fresh-jail v2.3.1 sweep found 20 probe-confirmed omissions across Rust
  standard-library types and traits, Rust keywords and idiom, formats and
  languages, and Gherkin terms. Designs using these tokens now produce a smaller Symbol anchor
  set, aligning the Structure denominator with the oracle.

- **`spectra drift` no longer caps the Structure denominator at 50** (#119).
  `ANCHOR_CAP` was implemented as `truncate(50)`, so every design with more
  than 50 anchors reported exactly `N/50` and silently dropped everything past
  index 50 from the broken set. The oracle does not truncate: above the cap it
  keeps an evenly spaced sample of at most 12 anchors *per category* (index
  `i * n / 12`), leaving a category of 12 or fewer whole — so an over-cap
  denominator is 12 per over-cap category plus the full count of each
  under-cap one (12/24/36, but 17 for 60 FilePath + 5 Function), never a
  value between 51 and the raw count. Pinned by probe at the 50/51
  boundary, at n = 53/60/77, and across one to three categories.

  This also closes the repo's longest-standing reverse-engineering unknown
  (#8/#51): the oracle's apparent "Symbol narrowing filter" — 12 of ~83
  prose candidates, with no semantic predicate that explained the selection —
  is this same per-category sampling. Probed: 30 Symbol candidates keep all
  30 (including the `Data`/`Model` pair that motivated the mystery), 83 keep
  exactly 12 at `floor(i * 83 / 12)`. The apparent narrowing was not a separate
  semantic filter; #133 later closed an independent stop-list gap.

- **`spectra drift` FilePath anchors are reported as written, and no longer
  name a string absent from the design** (#123). The recovered regex has no
  left boundary, so the oracle turns `frontend/src/services/apiClient.ts` into
  the anchor `src/services/apiClient.ts` and reports "file does not exist" for
  text that appears nowhere in the change — a finding nobody can grep for, and
  the cause of all six surviving broken anchors on a 29-change corpus (a 0/6
  true-positive rate). Probed and confirmed oracle-faithful before diverging:
  with the full path present on disk the oracle still reports it broken, and it
  resolves only when the truncated path exists. Extraction now keeps leading
  path segments, drops matches that begin mid-token (`mysrc/foo.rs` no longer
  yields a phantom `src/foo.rs`), and drops paths with a `..` segment, which
  would otherwise let a file *outside* the project satisfy a design reference.
  Resolution is deliberately widened to the union of the written path and the
  oracle's truncation, so a project rooted at its own sub-project keeps
  resolving as before. The same union is applied to the `.started` baseline
  probe, so both questions use one resolution rule — which does mean a path
  whose *truncated* form resolved at the baseline is reported broken rather than
  as a forward reference (a false positive in a repo-root layout). Both layouts
  are probed and recorded in the RE doc: restricting the baseline probe instead
  turns a real nested-root deletion into a missed one, so the union is kept.

- **A freshly scaffolded `design.md` matches the oracle exactly** (#51). `JSON`
  was missing from the recovered Symbol stop-list — the last divergence on an
  untouched template, now `0/20` on both binaries. Probed per token; the
  "all-caps acronyms are dropped" theory is refuted (`ALTER`, `TABLE`,
  `COLUMN`, `README`, `CRITICAL`, `YAML`, `HTTP`, `SQL` are all kept).

- **`park` / `unpark` / `list --parked` now interoperate with the oracle**
  (#118). Parking is a *move*, not a flag: the oracle relocates the whole
  change directory to `<git common dir>/spectra-app/changes/<name>/`, and
  OpenSpectra's empty `.spectra/changes/<name>.parked` marker was invisible to
  it and blind to it — `list --parked` reported `No parked changes.` on a
  repository with 15. OpenSpectra now uses the same store, so changes parked
  by either binary are seen by both, including from a linked git worktree
  (the *common* git dir, not `.git/worktrees/<name>/`).

  A parked change stays addressable by `status`, `show`, `drift`, and
  `instructions`, matching the oracle; only `list` and `validate` hide it.
  `list --parked --json` now keys its array on `parked` (was `changes`) with
  `status: "parked"` on every entry; `park`/`unpark` print
  `Parked change: <name>` / `Unparked change: <name>` and emit
  `{"parked": "<name>"}` / `{"unparked": "<name>"}`. Error strings and exit
  codes match the oracle for unknown, already-active, and already-parked
  names. `park`/`unpark` accept the oracle's looser id charset
  (`^[a-z0-9-]+$`), so archived-prefixed names such as `2026-01-01-old` park
  and list like any other. See
  [`docs/reverse-engineering/park.md`](docs/reverse-engineering/park.md).

  **Two deliberate divergences**, both where the oracle destroys data: `park`
  refuses to overwrite an existing parked change of the same name (the oracle
  replaces it silently — probed), and refuses the name `archive` (the oracle
  moves the entire `changes/archive/` tree into the parked store).

  **Migration:** a pre-existing `.spectra/changes/<name>.parked` marker is no
  longer read. Nothing is lost — the change is still in
  `<spec_dir>/changes/` — but it lists as active again; re-run
  `spectra park <name>` to move it into the shared store.
- **A custom `schema:` in `<spec_dir>/config.yaml` no longer silently falls
  back to the built-in workflow** (#117). That file had no reader at all, so a
  project selecting its own schema ran `spec-driven` with exit 0, no warning,
  and normal-looking output — while `instructions` emitted the generic text
  with the project's own artifact instructions missing entirely. `status`,
  `instructions` now resolve the selector (`--schema` first, then the change's
  own `.openspec.yaml`, then the project `config.yaml`, then the built-in — the
  change-level key dominates, and since `spectra new change` stamps it, it is
  set on essentially every real change) and refuse to run on anything else:
  - a name that resolves nowhere reports the oracle's message byte for byte
    (`Schema not found: Schema 'X' not found in project, user, or built-in
    locations`);
  - a name whose `<spec_dir>/schemas/<name>/schema.yaml` *does* exist gets a
    distinct message naming that file — claiming it was "not found in project
    … locations" would be false.

  Resolution runs **after** the change is loaded, matching the oracle's probed
  order (`Change 'X' not found.` precedes any schema error). `new artifact` is
  deliberately **not** gated: probed, the oracle never errors on the selector
  there and the command has no `--schema` flag. It does resolve it for template
  lookup though — with a resolvable custom schema the oracle writes *that*
  schema's template, and with an unresolvable one it writes a **0-byte** file
  where OpenSpectra writes the built-in template. Both are recorded in
  `schemas.md`; the custom-template half is #126.

  **`spectra new change` now stamps the project's configured schema** instead of
  hardcoding `spec-driven`, matching the oracle. That is what keeps the gate
  reachable: the change-level key outranks `config.yaml`, so the old hardcode
  meant every change OpenSpectra created in a custom-schema project recorded a
  schema it does not use and then passed the gate — silently re-opening the
  fallback this entry is about.

  This is the stopgap half of #117. Actually loading a custom schema (and
  listing project schemas in `spectra schemas`, which the oracle does) is
  tracked by #126; the sibling `context:` / `rules:` keys the oracle also reads
  from `config.yaml` by #127.

### Changed

- **The recommended CI drift gate filters to `FilePath` broken anchors.** The
  README previously gated on `severity == "light"`; both that and an
  unfiltered `broken_anchors` check are unusable, and measured to be: a healthy
  committed change that merely *mentions* `--json` and `--force` in prose
  reports `2/4 anchors broken` and severity `heavy`, because every extracted
  flag is unconditionally `not in --help`. Filtering to `FilePath` leaves the
  one verdict that consults the change's `.started` baseline, so the gate means
  "does not resolve now, and either resolved at the baseline or has no usable
  baseline". Verified both directions. Note `spectra init` gitignores
  `.spectra/`, so CI has no baseline unless the workflow rebuilds one. The
  README recipe does, from the merge-base, and is pull-request-only for that
  reason: on a push to the default branch the merge-base is HEAD itself and the
  gate could never fail. The README now also states what the gate cannot
  catch: on a committed change `git grep` self-matches the design, so a deleted
  function or type is invisible to *both* binaries.

- **Broken CliFlag and Function anchors are reported as broken again** (#119),
  reverting that half of #83. Measured over 26 real changes, withholding them
  put every Structure score out of step with the oracle and let a CI drift
  gate stay green while drift existed. `--flag` anchors are broken with reason
  `not in --help` and `git grep`-missing functions with
  `function not found in repo`, both matching the v2.3.1 oracle byte for byte.
  The remaining #83 divergence is narrower: a missing FilePath that did not
  exist at the change's `.started` baseline is still `forward reference` in
  `unresolved_anchors` rather than broken. `unresolved_anchors` keeps its shape
  and place in the JSON.

## [0.6.0] - 2026-08-02

### Added

- **`spectra init --tools <list>`** (#89) writes the instruction files for an
  explicit set of AI tools at init time, reusing the `update` write path
  instead of a second implementation. This is the last piece of #55: a brand
  new project can now be bootstrapped with openspectra alone.

### Changed

- **`spectra drift` now separates unresolvable design anchors from broken
  anchors** (#83). JSON reports them in a new additive
  `unresolved_anchors` array with the same `{ anchor, category, reason }`
  shape as `broken_anchors`; human output uses a separate heading. CliFlags,
  repository-missing Functions, and FilePaths that never existed at the
  change's `.started` baseline are unresolved. A missing FilePath that did
  exist at baseline remains broken, and a missing/unusable baseline preserves
  the prior broken fallback. Unresolved anchors no longer increment the
  Structure broken count or contribute to its score, `total_score`, severity,
  or exit-code inputs. This is a deliberate divergence from the v2.3.1 oracle,
  which counts these references as broken.

### Fixed

- **`spectra update` no longer silently skips the Claude slash commands**
  (#92). With `claude_slash_commands: true` the 10 gated
  `.claude/commands/spectra/*.md` files were never written -- the flag was
  read and then ignored, so the whole write set was a silent no-op. They are
  now written through the same gated path as the other tool files.
- **`spectra init` default artifacts match the v2.3.1 oracle** (#94), closing
  6 divergences -- notably the missing `config.yaml` and `changes/archive/`,
  and a `.spectra.yaml` that was written as a single line.
- **Newly created files get the oracle's `0666 & ~umask` mode** (#93).
  `write_via_temp` created its temp file `0600` and only applied the target's
  mode when the target already existed, so every *new* file kept `0600`. The
  mode is now probed against the oracle at four umask values (000/002/022/077);
  022 and 077 alone cannot distinguish an `0666` base from `0644`, so the base
  was only pinned once 000 and 002 were measured. Existing targets keep their
  mode, and a symlinked target is no longer replaced at the widened mode.

## [0.5.0] - 2026-07-24

### Added

- **`spectra update [PATH] [--force]`** (#55) rewrites the instruction files of
  every detected AI tool — 23 of them (`.claude/`, `.cursor/`, …,
  `.github/prompts`) — to the current schema. Detection is filesystem
  existence of each tool's path; the write set, stdout, exit codes, and error
  strings are byte-for-byte aligned with the v2.3.1 oracle, verified in CI
  against a captured golden tree without needing the closed-source binary.

  Per-file behavior is **probed, not inferred**: skills/commands/prompts are
  full-overwrite (`Plain`), 11 root-level marker files (`CLAUDE.md`,
  `.cursorrules`, …) are merged only within their
  `<!-- SPECTRA:START … -->`/`<!-- SPECTRA:END -->` block via a plain substring
  splice with no line anchoring, and `.claude/settings.json` is a key-sorted
  JSON merge. Two oracle bugs are preserved deliberately (an unsubstituted
  `{{SPEC_DIR}}` literal in every `spectra-ask` file; a bare io error when a
  detection path is a regular file), both documented in
  [`docs/reverse-engineering/update.md`](docs/reverse-engineering/update.md).

  Two deliberate divergences, both security rulings this repo already made for
  change directories: writes to a `Managed`/`ClaudeSettings` path use a
  hardened atomic write (`O_EXCL` temp + exact-mode `fchmod` + `rename`), so a
  symlinked target is replaced rather than followed and an interrupted run
  cannot truncate a user's `CLAUDE.md`. A read-only `Managed` file therefore
  succeeds where the oracle exits 1. Symlinked *ancestor* directories are still
  followed, matching the oracle (recorded as a residual risk).

- **`spectra config <path|list|get|set|unset|reset|edit>`** (#57) manages the
  global user config file (`~/Library/Application Support/openspec/config.yaml`
  on macOS, `${XDG_CONFIG_HOME:-~/.config}/openspec/config.yaml` elsewhere,
  absolute XDG paths only) and needs no initialized project. Output shapes,
  key ordering, and error strings are pinned against the v2.3.1 oracle; see
  [`docs/reverse-engineering/config.md`](docs/reverse-engineering/config.md).

  Deliberate divergences, all documented: writes go through the same hardened
  atomic write as `update` (in place → temp+rename), so a mode-555 config
  *directory* fails where the oracle writes in place and succeeds, and a
  symlinked `config.yaml` is replaced rather than written through. `HOME` unset
  or empty errors instead of resolving via the OS password database (avoids a
  new dependency).

- **`spectra completion generate|install|uninstall`** (#60) produces and
  installs shell completion scripts, built on `clap_complete`. `generate`
  supports bash, zsh, fish, elvish, and powershell, and detects the shell from
  `$SHELL` when the argument is omitted.

  **Deliberately more capable than the oracle**: v2.3.1's `install`/`uninstall`
  were probed and found to be no-op stubs that print a hint and write nothing.
  Issue #60 explicitly waives byte-for-byte alignment here (the artifact is a
  shell script, not a data contract), so OpenSpectra actually writes and
  removes the files — bash into `$XDG_DATA_HOME/bash-completion/completions/`,
  fish into `$XDG_CONFIG_HOME/fish/completions/`, and zsh into
  `$ZDOTDIR/.zfunc/`. **No rc file is ever modified**; zsh instead prints a
  one-time hint naming the directory it actually wrote to (so a `ZDOTDIR`
  user is not sent to `~/.zfunc`), single-quoted so a path containing spaces
  does not word-split when pasted. `install`/`uninstall` cover bash/zsh/fish
  only — elvish and powershell report a clear error pointing at `generate`.

  Writes go through a `create_new` temp file plus `rename`, so a symlink at
  either the completion path or the temp path is replaced rather than
  followed — a plain write would truncate whatever it pointed at, which is
  how an early revision could overwrite a user's `.bashrc`. Environment
  overrides (`XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `ZDOTDIR`, `HOME`) are
  honoured only when absolute; empty or relative values are treated as unset
  per the XDG spec, rather than resolving against the current directory.

- **`spectra in-progress add <NAME>`** (#61) records a write-only in-progress
  marker. The oracle keeps this state in a SQLite table
  (`.git/spectra-app/spectra.db`); OpenSpectra stores it as a
  `.spectra/changes/<name>.in-progress` sidecar instead, which is
  indistinguishable through the CLI because **no command reads the marker** —
  `list`, `list --json`, `list --parked`, `status`, `analyze`, and `show` are
  byte-identical before and after, and a regression test locks all six of
  those invocations against a fixture whose `list --json` reads `"done"`
  beforehand (otherwise a marker leaking into that field would produce
  identical bytes and the lock would prove nothing). Other
  oracle behaviors reproduced deliberately: success prints nothing and exits 0,
  a name with no corresponding change is accepted and recorded (ghost change),
  repeated adds are idempotent, and there is no `--json` flag and no removal
  subcommand (both exit 2). See
  [`docs/reverse-engineering/in-progress.md`](docs/reverse-engineering/in-progress.md).

  Two deliberate divergences, both documented: traversal names are rejected,
  and `archive`/`new change` clear a stale marker (the oracle orphans it).

## [0.4.0] - 2026-07-20

### Added

- **`spectra schemas [--json]`** (#56) lists the built-in workflow schema
  registry (only `spec-driven`). An sdd drop-in requirement — the sdd plugin
  shells out to `spectra schemas`. Text, `--json`, and colored-TTY output are
  pinned byte-for-byte against the v2.3.1 oracle
  (`docs/reverse-engineering/schemas.md` + `golden/schemas-2.3.1.{json,txt}`).

### Fixed

Four `analyze`/`instructions` fidelity gaps found by a mob review of the 0.3.0
work but not landed with it, each re-pinned against the v2.3.1 oracle:

- **`analyze` now flags all nine weak-language words**, not five. `consider`,
  `possibly`, `TODO`, and `TKTK` (alongside `should`/`may`/`might`/`TBD`/`???`)
  are flagged as `ambWeakLanguage`, matching the oracle and the word list the
  `specs` instruction text already advertised.
- **`analyze` scans only the literal `spec.md` in each capability directory.**
  It previously walked every `*.md` under `specs/`, so a sidecar like
  `notes.md` produced phantom findings the oracle never emits.
- **An empty-description checkbox (`- [ ] ` with only trailing whitespace) is
  no longer counted as a task.** `instructions apply` dropped it from its task
  list but `tasks.md` parsing (`spectra tasks`, `task done <id>`) still counted
  it, so an id taken from `instructions apply` could silently mark the wrong
  line done. All checkbox call sites now share one predicate and drop the line,
  matching the oracle.

### Security

- Added a regression test pinning that `new artifact --force` cannot overwrite
  a file outside the change directory through a pre-planted symlink at the
  artifact path. The 0.3.0 atomic temp-file + `rename` install already replaces
  the symlink entry rather than following it (unlike the oracle, which follows
  the link); the test locks that guarantee against a future change to the
  write path.

## [0.3.0] - 2026-07-18

### Added

- **The artifact workflow surface the sdd tooling depends on** (#43, PR #48),
  all pinned against the v2.3.1 oracle with side-by-side captures:
  - `spectra status [--change] [--schema] [--json]` — artifact DAG status
    (`proposal → {design, specs} → tasks`, derived purely from file
    existence). See `docs/reverse-engineering/artifact-workflow.md`.
  - `spectra new artifact <TYPE> [CAPABILITY] [--stdin] [--force] [--json]`
    — creates one artifact from stdin or its built-in template, with
    per-type content validation and oracle-verbatim error strings.
  - `spectra instructions [ARTIFACT] [--change] [--json]` — authoring
    instruction + template per artifact; apply mode (tasks progress +
    preflight: missing/drifted file refs, staleness) once all artifacts are
    done.
  - `spectra analyze [CHANGE] [--json]` — 4-dimension, 10-finding artifact
    consistency report; always exits `0`. See
    `docs/reverse-engineering/analyze.md`.

### Changed

- **BREAKING: `spectra new change` no longer scaffolds `proposal.md` /
  `design.md` / `tasks.md`.** Matching the oracle, it creates only the
  change directory and `.openspec.yaml` (now with `schema`, `created`,
  `created_by`); artifact files are created by `spectra new artifact` as
  the workflow advances. The old scaffold made `status` report everything
  done up front and forced `--force` on every `new artifact`.

### Fixed (PR #48 mob review)

- `.openspec.yaml` is serialized through `serde_yaml`, so a git identity
  containing YAML-special characters (`:`, ` #`, …) round-trips instead of
  producing an unparseable file that silently degraded metadata to defaults.
- `spectra new artifact` writes are race-safe: content goes to a
  uniquely-named temp file, then installs atomically — no-clobber
  `hard_link` without `--force` (a concurrent creator gets the
  oracle-aligned "already exists" error instead of silently overwriting),
  temp-file + atomic `rename` with `--force`. A failed write can no longer
  leave a partial artifact that `status` misreads as done.
- The `specs` done-check now follows directory symlinks (matching the
  oracle's `specs/**/*.md` glob, probed 2026-07-18), is symlink-cycle-safe,
  and — order-independently — propagates I/O errors (e.g. permission
  denied) instead of silently reporting "not done".
- Docs and golden fixtures no longer embed a private email address or
  machine-specific home-directory paths.

### Divergences from the v2.3.1 oracle (deliberate, documented)

- `spectra instructions --skill` always answers `Unknown skill` — the
  oracle prints proprietary embedded skill bodies OpenSpectra does not ship.
- `instructions` apply-mode `contextFiles` key order and `analyze` spec-file
  / params ordering are fixed (schema/path order) where the oracle is
  hash-map/readdir nondeterministic between its own runs.
- Multiple-active-changes error wording/ordering (issue #50, ruling
  pending) and fresh-design-scaffold drift anchors (issue #51) are known,
  tracked divergences — see `docs/reverse-engineering/artifact-workflow.md`.

## [0.2.1] - 2026-07-10

### Fixed

- **`spectra archive` now traverses nested-capability specs like `spectra
  validate` does.** Previously `archive` walked only the immediate children of
  a change's `specs/`, so a nested-capability delta
  (`specs/<Epic>/<Feature>/spec.md`) that `validate` accepts was **silently
  ignored** — the change was archived with the requirement never merged into
  the canonical spec and no error reported. Both commands now share one
  recursive collector (`fsutil::collect_delta_specs`), so their traversal and
  its symlink-cycle safety can't drift apart. As part of the shared walk, a
  `spec.md` placed directly under `specs/` (no capability directory) and a
  non-UTF-8 capability directory name are now hard errors, and a symlinked
  capability directory is skipped with a stderr warning rather than silently
  dropped. (#39)

## [0.2.0] - 2026-07-10

### Added

- `spectra validate [CHANGE] [--changes] [--strict] [--json]` — an OpenSpec
  structural gate. Unlike the OSS `@fission-ai/openspec` validator, it
  traverses nested-capability layouts (`specs/<Epic>/<Feature>/spec.md`)
  instead of reporting them as "no deltas found". A change needs at least one
  requirement delta; with `--strict`, each ADDED/MODIFIED requirement also
  needs a normative `SHALL`/`MUST` and a `#### Scenario:` block. It is a
  pass/fail gate: exit `0` when every change is valid, `1` otherwise (gate on
  the JSON `summary.totals.failed`). The delta parser is fenced-code-block
  aware and its directory walk is symlink-cycle safe.

### Changed

- **`spectra drift` now always exits `0` on a successful run**, regardless of
  severity (matching the reference binary and the documented contract). The
  previous `0`/`1`/`2` severity-to-exit-code mapping (shipped in 0.1.0)
  reddened downstream CI on the `spectra` process itself before callers could
  gate on the JSON `severity` field. Gate on the `severity` field, not the
  exit code. (#37)

## [0.1.0] - 2026-07-03

### Added

- Drift detection for FilePath, Function, CliFlag, and Symbol anchors with a
  category-weighted structure score.
- `spectra init`, including `--adopt` support for existing OpenSpec-style
  layouts.
- Change and spec workflows: `list`, `show`, `park`, `unpark`, `new change`,
  `task done`, and `archive`.
- JSON output for automation and CI integrations.
- Exit codes by drift severity: `0` for light, `1` for medium, `2` for heavy,
  and `3` for command errors.
- OpenSpec ecosystem compatibility for the `.spectra.yaml`, changes, specs,
  tasks, and archive layout.
- Distribution: prebuilt static Linux (`x86_64`/`aarch64` musl) and macOS
  (`x86_64`/`aarch64`) release tarballs with SHA-256 checksums, a
  `ghcr.io/howie/openspectra` Docker image (linux/amd64), and crates.io
  publishing (`cargo install spectra-cli`).

[Unreleased]: https://github.com/howie/openspectra/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/howie/openspectra/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/howie/openspectra/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/howie/openspectra/releases/tag/v0.4.0
[0.3.0]: https://github.com/howie/openspectra/releases/tag/v0.3.0
[0.2.0]: https://github.com/howie/openspectra/releases/tag/v0.2.0
[0.1.0]: https://github.com/howie/openspectra/releases/tag/v0.1.0
