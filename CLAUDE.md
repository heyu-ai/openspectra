# OpenSpectra

Rust reimplementation of the closed-source `spectra` CLI's `drift` command
(spec-driven-development drift detection). See `README.md` for what it does;
this file is agent-facing operational context.

## Workspace layout

- `crates/spectra-core` — pure logic (drift scoring, anchors, git, tasks,
  calibration). No CLI/IO concerns; keep it unit-testable without a binary.
- `crates/spectra-cli` — thin `clap` wrapper over `spectra-core`. New
  behavior belongs in `spectra-core`; the CLI crate should stay a thin shell.
- `docs/reverse-engineering/` — write-ups documenting how closed-source
  `spectra` behavior was reverse-engineered (e.g. `drift.md`). Any change to
  RE'd constants/heuristics must update the matching write-up in the same PR.
- `scripts/capture-golden.sh` — macOS-only, requires the closed-source
  reference binary (`SPECTRA_BIN`); regenerates golden fixtures used to
  calibrate constants. Not runnable in CI.

## Build / verify (mirrors `.github/workflows/ci.yml`)

Run before claiming a change is done or pushing:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo build --release --locked
cargo test --all
```

`fmt`/`clippy` are `continue-on-error` in CI right now (the toolchain that
produced this tree predates rustfmt/clippy components being reliably
available), but treat them as hard gates locally — fix findings rather than
relying on CI leniency. `build` + `test` are the real CI gate.

## Testing conventions

- Unit tests live alongside the module (`#[cfg(test)] mod tests` in the same
  `.rs` file).
- Integration tests live in `crates/<crate>/tests/*.rs`
  (e.g. `drift_integration.rs`).
- Golden-fixture comparisons calibrate against the closed-source binary's
  actual output — see `docs/reverse-engineering/drift.md` ("Reproducing the
  oracle") before changing scoring constants.

## Applicable skills for this repo

No Rust-specific skill is installed; these general-purpose skills already
cover this repo's workflow and should be reached for proactively:

- `tdd-kentbeck` — TDD/Tidy-First discipline for `spectra-core` logic changes.
- `ci-triage` — funnel for diagnosing `cargo fmt`/`clippy`/`test` CI failures.
- `verify` — run the built CLI against a real project before claiming a fix
  works (`./target/release/spectra drift`, etc.), not just `cargo test`.
- `run` — launch/drive the CLI binary to observe a change working.

## Issue tracking

GitHub Issues (this repo has no Jira/Linear project configured).
