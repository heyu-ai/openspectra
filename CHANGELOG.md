# Changelog

All notable changes to this project will be documented in this file.

This project follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Because
OpenSpectra reimplements the closed-source `spectra` CLI, deliberate
divergences from the v2.3.1 oracle are tracked here alongside user-visible
changes.

## [Unreleased]

### Added

- **`spectra in-progress add <NAME>`** (#61) records a write-only in-progress
  marker. The oracle keeps this state in a SQLite table
  (`.git/spectra-app/spectra.db`); OpenSpectra stores it as a
  `.spectra/changes/<name>.in-progress` sidecar instead, which is
  indistinguishable through the CLI because **no command reads the marker** —
  `list`, `list --json`, `list --parked`, `status`, `analyze`, and `show` are
  byte-identical before and after, and a regression test locks that. Other
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

[Unreleased]: https://github.com/howie/openspectra/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/howie/openspectra/releases/tag/v0.4.0
[0.3.0]: https://github.com/howie/openspectra/releases/tag/v0.3.0
[0.2.0]: https://github.com/howie/openspectra/releases/tag/v0.2.0
[0.1.0]: https://github.com/howie/openspectra/releases/tag/v0.1.0
