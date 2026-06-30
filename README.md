# OpenSpectra

An open-source, CI-friendly reimplementation of the [Spectra](https://github.com/kaochenlong/spectra-app)
spec-driven development CLI — reverse-engineered from the closed-source binary,
starting with the `drift` command.

> **Status:** early. `spectra drift` (+ minimal `list` / `show`) is implemented
> in Rust and runs on Linux/macOS with zero runtime dependencies (just `git` on
> `PATH`). The reverse-engineering write-up is in
> [`docs/reverse-engineering/drift.md`](docs/reverse-engineering/drift.md).

## Why

The upstream Spectra CLI is closed-source and updated infrequently. `drift` —
which detects when a change's spec artifacts have diverged from the codebase —
is the most valuable piece to run in CI as a gate. It turns out to be **pure
git + filesystem + regex heuristics** (no AI), so it reimplements cleanly and
runs anywhere.

## What `drift` does

Given a change under `<spec_dir>/changes/<name>/`, it scores drift across four
dimensions and prints a single recommended next step:

* **Time** — days since the change was created (fresh → aging → stale → abandoned).
* **Structure** — design-doc anchors (file paths, functions, symbols, CLI flags)
  that no longer resolve against the codebase.
* **Tasks** — pending tasks that collide with external work *(detection gated
  off pending calibration — see the RE doc)*.
* **Environment** — commit volume since creation (display only).

It outputs a human-readable, conclusion-first report or `--json` for tooling,
and exits non-zero on medium/heavy drift so CI can gate on it.

## Usage

```sh
spectra drift [CHANGE] [--json] [--no-color]   # auto-detects if one active change
spectra list  [--changes] [--json]
spectra show  <CHANGE> [--json]
```

Exit codes: `0` light · `1` medium · `2` heavy (or error).

### CI gate example

```yaml
# .github/workflows/spec-drift.yml
- run: cargo install --git https://github.com/howie/openspectra spectra-cli
- run: spectra drift --json   # non-zero exit fails the job on medium/heavy drift
```

## Build

```sh
cargo build --release        # produces target/release/spectra
cargo test                   # unit + integration tests
```

## Fidelity

Verified byte-for-byte against the v2.3.1 oracle for FilePath/Function/CliFlag
detection, the scoring curves, severity bands, and recommendations. One open
item — the Symbol-anchor narrowing filter — over-counts symbols on prose-dense
designs; details and the calibration method are in the RE doc. The reference
binary is used as a golden oracle to calibrate constants
(`crates/spectra-core/src/calibration.rs`).

## Layout

```
crates/spectra-core/   # change discovery, anchors, git, drift scoring (library)
crates/spectra-cli/    # clap CLI: drift / list / show
docs/reverse-engineering/drift.md   # how the original works, and what's still open
scripts/               # oracle calibration probes
```

## License

MIT
