# Changelog

All notable changes to this project will be documented in this file.

This project follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Because
OpenSpectra reimplements the closed-source `spectra` CLI, deliberate
divergences from the v2.3.1 oracle are tracked here alongside user-visible
changes.

## [Unreleased]

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

[Unreleased]: https://github.com/howie/openspectra/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/howie/openspectra/releases/tag/v0.1.0
