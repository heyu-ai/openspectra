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

`fmt`/`clippy` are `continue-on-error` in CI (see the comment in `ci.yml`),
but `build` + `test` are the only checks CI actually enforces — treat `fmt`
and `clippy` as hard gates locally regardless. In practice `cargo clippy`
currently passes clean, and `cargo fmt --check` failures are real formatting
diffs, not a toolchain gap — run `cargo fmt --all` rather than ignoring the
check. If a clippy finding is a false positive, suppress it narrowly with a
comment explaining why; never add a blanket `#[allow]` just to make the
check pass.

## Testing conventions

- Unit tests live alongside the module (`#[cfg(test)] mod tests` in the same
  `.rs` file).
- Integration tests live in `crates/<crate>/tests/*.rs`
  (e.g. `drift_integration.rs`).
- Golden-fixture comparisons calibrate against the closed-source binary's
  actual output — see `docs/reverse-engineering/drift.md` ("Reproducing the
  oracle") before changing scoring constants.

## Applicable skills for this repo

None of these are Rust-specific or bundled with this repo — availability
depends on the operator's own Claude Code setup — but if present, reach for
them proactively:

- `tdd-kentbeck` — TDD/Tidy-First discipline for `spectra-core` logic changes.
- `ci-triage` — a generic fmt/lint/test-failure triage funnel; not
  Rust-specific but applicable to `cargo fmt`/`clippy`/`test` failures.
- `verify` — run the built CLI against a real project before claiming a fix
  works (`./target/release/spectra drift`, etc.), not just `cargo test`.
- `run` — launch/drive the CLI binary to observe a change working.

## Issue tracking

GitHub Issues (as of writing, this repo has no Jira/Linear project
configured).
