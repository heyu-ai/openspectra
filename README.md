# OpenSpectra

An open-source, CI-friendly reimplementation of the [Spectra](https://github.com/kaochenlong/spectra-app)
spec-driven development CLI — reverse-engineered from the closed-source binary,
starting with the `drift` command.

> **Status:** early. `spectra init` / `drift` / `validate` / `update` (+ minimal
> `list` / `show` / `park` / `unpark` / `new change` / `task done` /
> `in-progress add` / `completion` / `archive`) is implemented in Rust and runs
> on Linux/macOS with zero runtime dependencies
> (just `git` on `PATH`). The reverse-engineering write-ups are in
> [`docs/reverse-engineering/drift.md`](docs/reverse-engineering/drift.md),
> [`docs/reverse-engineering/task.md`](docs/reverse-engineering/task.md),
> [`docs/reverse-engineering/archive.md`](docs/reverse-engineering/archive.md),
> [`docs/reverse-engineering/update.md`](docs/reverse-engineering/update.md),
> [`docs/reverse-engineering/in-progress.md`](docs/reverse-engineering/in-progress.md),
> and [`docs/reverse-engineering/init.md`](docs/reverse-engineering/init.md)
> (init's default artifacts and stdout are oracle-verified; its `--adopt` /
> `--json` extensions and a few edge cases are not — see that doc). `validate` is
> likewise **not** oracle-verified: it matches the OSS `openspec validate`
> contract, not the macOS binary — see
> [`docs/reverse-engineering/validate.md`](docs/reverse-engineering/validate.md).

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

It outputs a human-readable, conclusion-first report or `--json` for tooling.
Like the reference binary, a successful run always exits `0` regardless of
severity — CI gates on the `severity` field of the JSON output (see below).

## Usage

```sh
spectra init  [--json]            # scaffolds .spectra.yaml + <spec_dir>/config.yaml + <spec_dir>/{changes/archive,specs}/ at the resolved project root (nearest ancestor with .spectra.yaml, else cwd)
spectra drift [CHANGE] [--json]   # auto-detects if one active change
spectra validate [CHANGE] [--changes] [--strict] [--json]  # OpenSpec structural gate; exits non-zero when any change is invalid
spectra list  [--json]            # lists active changes
spectra list  --changes [--json]  # same as above, explicitly (mutually exclusive with --specs/--parked)
spectra list  --specs [--json]    # lists capability specs instead of changes
spectra list  --parked [--json]   # lists parked changes instead of active ones
spectra show  <CHANGE|SPEC> [--json]   # prints the change's proposal, or a spec's content
spectra park   <CHANGE> [--json]  # marks a change on hold (excluded from the active listing)
spectra unpark <CHANGE> [--json]  # resumes a parked change
spectra new change <NAME> [--json]  # creates a change dir + .openspec.yaml only (kebab-case name; artifact files come later via `new artifact`)
spectra new artifact <TYPE> [CAPABILITY] [--change <NAME>] [--stdin] [--force] [--json]  # creates one artifact (proposal|design|tasks|spec) from stdin or its built-in template, with per-type validation
spectra schemas [--json]          # lists the built-in workflow schema registry (only spec-driven)
spectra status [--change <NAME>] [--schema <NAME>] [--json]   # shows the artifact DAG (proposal → {design, specs} → tasks) as done/ready/blocked
spectra instructions [ARTIFACT] [--change <NAME>] [--json]  # prints the artifact's authoring instruction + template; with all artifacts done, switches to apply mode (tasks progress + preflight)
spectra analyze [CHANGE] [--json]   # 4-dimension artifact consistency report (Coverage/Consistency/Ambiguity/Gaps); always exits 0
spectra task done <TASK_ID> [--change <NAME>] [--json]  # marks a tasks.md checkbox done, records touched files
spectra in-progress add <NAME>    # records a write-only in-progress marker (no --json, no removal path, no effect on any listing)
spectra completion generate [SHELL]              # prints a shell completion script (bash|zsh|fish|elvish|powershell; detects $SHELL when omitted)
spectra completion install   [SHELL] [--verbose]  # writes it to the shell's user completion dir (bash|zsh|fish; never edits your rc files)
spectra completion uninstall [SHELL] [-y]         # removes that file
spectra archive [CHANGE] [--skip-specs] [--mark-tasks-complete]  # moves a change to changes/archive/<date>-<name>, applies added spec requirements
spectra update [PATH] [--force]   # rewrites instruction files for every detected AI tool (.claude/, .cursor/, … 23 tools); oracle-verified byte-for-byte
spectra config <path|list|get|set|unset|reset|edit>  # manages the global user config (~/Library/Application Support/openspec/config.yaml on macOS, ${XDG_CONFIG_HOME:-~/.config}/openspec/config.yaml elsewhere, absolute XDG paths only); needs no project
```

Exit codes for `drift` (pinned against the reference binary): `0` on any
successful run — severity does **not** map to the exit code — and `1` on errors
(e.g. change not found). To gate CI on drift severity, check the JSON
`severity` field.

`validate` is the deliberate exception: it is a pass/fail gate, so it exits
`0` when every validated change is valid and `1` when any is invalid (also
`1` on an operational error, e.g. not initialized). The JSON is still
authoritative — gate on `summary.totals.failed`. See
[`docs/reverse-engineering/validate.md`](docs/reverse-engineering/validate.md)
for the rule set (it matches the OSS `openspec validate` contract, not the
macOS Spectra binary, which can't be probed for `validate` from Linux CI).

`spectra drift`'s human-readable conclusion line is colored by severity
(green/yellow/red) when stdout is a terminal; `--no-color` or the
[`NO_COLOR`](https://no-color.org) env var disables it.

## Install

From crates.io:

```sh
cargo install spectra-cli
```

Release tarballs are attached to GitHub releases for Linux and macOS:

```sh
curl -L https://github.com/howie/openspectra/releases/download/v0.2.1/spectra-v0.2.1-x86_64-unknown-linux-musl.tar.gz \
  | tar xz
sudo mv spectra-v0.2.1-x86_64-unknown-linux-musl/spectra /usr/local/bin/spectra
```

Substitute the latest release tag from https://github.com/howie/openspectra/releases.

Docker images are published as `ghcr.io/howie/openspectra:<tag>` and
`ghcr.io/howie/openspectra:latest` (linux/amd64 only; on other platforms use
the release tarballs). The image bundles `git` and the `spectra` binary, with
`spectra` as its entrypoint:

```sh
docker run --rm -v "$PWD:/repo" -w /repo ghcr.io/howie/openspectra:latest drift --json
```

### CI gate example

```yaml
# .github/workflows/spec-drift.yml
name: Spec Drift

on:
  pull_request:
  push:
    branches: [main]

jobs:
  drift:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - name: Install spectra
        run: |
          # Pin to a release tag; see https://github.com/howie/openspectra/releases for the latest.
          SPECTRA_VERSION=v0.2.1
          curl -L "https://github.com/howie/openspectra/releases/download/${SPECTRA_VERSION}/spectra-${SPECTRA_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
            | tar xz
          sudo mv "spectra-${SPECTRA_VERSION}-x86_64-unknown-linux-musl/spectra" /usr/local/bin/spectra
      - name: Check spec drift
        run: |
          spectra drift --json | tee drift.json
          jq -e '.severity == "light"' drift.json > /dev/null
```

`spectra drift` itself always exits `0` on a successful run (matching the
reference binary), so the gate is the explicit `jq` check on the JSON
`severity` field — the job fails when a change has drifted to `medium` or
`heavy`.

For a structural gate, `spectra validate --changes --strict` fails the job
directly on its own exit code (no `jq` needed) — it exits non-zero when any
change lacks a delta, or (under `--strict`) has a requirement missing a
normative `SHALL`/`MUST` or a `#### Scenario:` block:

```yaml
      - name: Validate OpenSpec changes
        run: spectra validate --changes --strict
```

Unlike the OSS `@fission-ai/openspec` validator, this traverses nested
`specs/<Epic>/<Feature>/spec.md` layouts, so nested-capability changes are
validated instead of skipped as "no deltas found".

#### Using the Docker image

Instead of installing the tarball, invoke `spectra` through the published image
(it bundles `git` + the binary). Run it as a `docker run` step **after** a
normal checkout — do **not** use it as a job-level `container:`. The image is
Alpine/musl, and GitHub's container-job runtime injects a glibc Node.js to run
JavaScript actions, which `actions/checkout` can't execute on musl:

```yaml
jobs:
  validate:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Validate OpenSpec changes
        # Pin to a release tag; :latest tracks the newest stable release.
        run: |
          docker run --rm -v "$PWD:/repo" -w /repo \
            ghcr.io/howie/openspectra:v0.2.1 validate --changes --strict
```

`spectra validate` gates on its own exit code, so no `jq` is needed. For the
`drift` + `jq` severity gate, use the tarball job above (the runner has `jq`
preinstalled; the minimal image does not ship it).

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
crates/spectra-cli/    # clap CLI: init / drift / validate / update / list / show / park / unpark / new change / task done / in-progress add / completion / archive
crates/spectra-core/assets/update/   # instruction-file templates captured verbatim from the oracle (generated; see update.md)
docs/reverse-engineering/   # how the original works (each doc separates probed from still-unverified behavior)
scripts/               # oracle calibration probes + template capture (capture-update-templates.py)
```

## License

MIT
