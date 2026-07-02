# OpenSpectra

An open-source, CI-friendly reimplementation of the [Spectra](https://github.com/kaochenlong/spectra-app)
spec-driven development CLI — reverse-engineered from the closed-source binary,
starting with the `drift` command.

> **Status:** early. `spectra init` / `drift` (+ minimal `list` / `show` /
> `park` / `unpark` / `new change` / `task done` / `archive`) is implemented
> in Rust and runs on Linux/macOS with zero runtime dependencies (just `git`
> on `PATH`). The reverse-engineering write-ups are in
> [`docs/reverse-engineering/drift.md`](docs/reverse-engineering/drift.md),
> [`docs/reverse-engineering/task.md`](docs/reverse-engineering/task.md),
> [`docs/reverse-engineering/archive.md`](docs/reverse-engineering/archive.md),
> and [`docs/reverse-engineering/init.md`](docs/reverse-engineering/init.md)
> (the last one is **not** oracle-verified — see that doc).

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
spectra init  [--json]            # scaffolds .spectra.yaml + <spec_dir>/{changes,specs}/ in the current directory
spectra drift [CHANGE] [--json]   # auto-detects if one active change
spectra list  [--json]            # lists active changes
spectra list  --changes [--json]  # same as above, explicitly (mutually exclusive with --specs/--parked)
spectra list  --specs [--json]    # lists capability specs instead of changes
spectra list  --parked [--json]   # lists parked changes instead of active ones
spectra show  <CHANGE|SPEC> [--json]   # prints the change's proposal, or a spec's content
spectra park   <CHANGE> [--json]  # marks a change on hold (excluded from the active listing)
spectra unpark <CHANGE> [--json]  # resumes a parked change
spectra new change <NAME> [--json]  # scaffolds a new change directory (kebab-case name)
spectra task done <TASK_ID> [--change <NAME>] [--json]  # marks a tasks.md checkbox done, records touched files
spectra archive [CHANGE] [--skip-specs] [--mark-tasks-complete]  # moves a change to changes/archive/<date>-<name>, applies added spec requirements
```

Exit codes: `0` light · `1` medium · `2` heavy · `3` error.

`spectra drift`'s human-readable conclusion line is colored by severity
(green/yellow/red) when stdout is a terminal; `--no-color` or the
[`NO_COLOR`](https://no-color.org) env var disables it.

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
crates/spectra-core/   # change discovery, spec discovery, anchors, git, drift scoring (library)
crates/spectra-cli/    # clap CLI: init / drift / list / show / park / unpark / new change / task done / archive
docs/reverse-engineering/   # how the original works (and, for init, what's still unverified)
scripts/               # oracle calibration probes
```

## License

MIT
