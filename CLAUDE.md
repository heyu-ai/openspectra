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
- `scripts/capture-update-templates.py` — same constraints (macOS + reference
  binary); regenerates `crates/spectra-core/assets/update/`, the generated
  `update_manifest.rs`, and the `update` golden TSV. It is a verification
  contract, not a printer: template round-trip, per-tool stdout, registry
  order, and the codex×gemini quirk are all re-checked, and any mismatch
  exits non-zero keeping its sandboxes. Never hand-edit its outputs.

## Build / verify (mirrors `.github/workflows/ci.yml`)

Run before claiming a change is done or pushing:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo build --release --locked
cargo test --all
```

`fmt` and `clippy` are hard gates in CI too (the `lint` job in `ci.yml`, no
`continue-on-error`), so a local fmt/clippy failure will also fail the PR.
`build` + `test` run on a `[ubuntu-latest, macos-latest]` matrix in the
`build-and-test` job (macOS runs 2 fewer tests — the `#[cfg(target_os =
"linux")]`-gated ones — which is expected, not a failure). If a clippy
finding is a false positive, suppress it narrowly with a comment explaining
why; never add a blanket `#[allow]` just to make the check pass.

## Testing conventions

- Unit tests live alongside the module (`#[cfg(test)] mod tests` in the same
  `.rs` file).
- Integration tests live in `crates/<crate>/tests/*.rs`
  (e.g. `drift_integration.rs`).
- Golden-fixture comparisons calibrate against the closed-source binary's
  actual output — see `docs/reverse-engineering/drift.md` ("Reproducing the
  oracle") before changing scoring constants.
- When pinning an RE'd constant against the oracle, also probe its downstream
  observable chain (score → severity → exit code → CI gate), not just the
  constant itself: a locally-correct fix can widen a latent divergence in a
  behavior that consumes it (PR #35: the abandoned score fix exposed the
  severity-mapped exit codes as an unverified guess).
- Calibration scripts are verification contracts, not printers: compare the
  recovered values against the pinned expectations, exit non-zero on drift or
  on a scan too short to cover them, and preserve the failing synthetic repo
  for inspection (see `scripts/calibrate-time.py --mode boundaries`).

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
