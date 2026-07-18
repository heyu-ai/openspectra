# Changelog

All notable changes to this project will be documented in this file.

This project follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Because
OpenSpectra reimplements the closed-source `spectra` CLI, deliberate
divergences from the v2.3.1 oracle are tracked here alongside user-visible
changes.

## [Unreleased]

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

### Divergences from the v2.3.1 oracle (deliberate, documented)

- `spectra instructions --skill` always answers `Unknown skill` — the
  oracle prints proprietary embedded skill bodies OpenSpectra does not ship.
- `instructions` apply-mode `contextFiles` key order and `analyze` spec-file
  / params ordering are fixed (schema/path order) where the oracle is
  hash-map/readdir nondeterministic between its own runs.

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

[Unreleased]: https://github.com/howie/openspectra/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/howie/openspectra/releases/tag/v0.2.0
[0.1.0]: https://github.com/howie/openspectra/releases/tag/v0.1.0
