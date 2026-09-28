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
  order, and update idempotency are all re-checked, and any mismatch
  exits non-zero keeping its sandboxes. Never hand-edit its outputs.
- `scripts/capture-skills.py` — same constraints (macOS + reference binary,
  version-pinned to 3.0.0); the default application path can be overridden by
  `--spectra-bin` or `SPECTRA_BIN`. It verifies all 20 embedded skill assets
  byte-exact against the oracle, checks their byte lengths and SHA-256 values
  against `docs/reverse-engineering/golden/skills-3.0.0.tsv`, verifies the
  oracle still rejects the known-absent enumeration candidates (fails loud if
  any of those candidates becomes a skill — detection is bounded by that
  wordlist), probes the behavior contract (`--skill` precedence outside and
  inside an initialized project, `--json` inertness), cross-checks the Rust
  registry, and pins the unknown-skill stderr/exit contract. The assets are the oracle captures and
  the TSV pins their provenance; both are generated artifacts. Any drift exits
  non-zero; `--write` regenerates both and re-verifies. Never hand-edit them.
- `scripts/capture-task-done.py` — same constraints (macOS + reference binary,
  version-pinned to 3.0.0, `--spectra-bin`/`SPECTRA_BIN` override). Runs the
  `task done`/`task start` scenarios in scratch git repos and writes the
  self-describing `docs/reverse-engineering/golden/task-done-3.0.0.json`,
  which `task_done_golden_integration.rs` replays. Drift exits non-zero and
  keeps the scratch repos; `--write` regenerates and re-verifies. The golden
  is generated — never hand-edit it. Since D7 (per-task baselines, #190) the
  replay compares every field of every scenario byte for byte, including the
  files under `.spectra/`; there is no divergence ledger any more.

- `scripts/mutate-check.py` + `scripts/mutations.toml` — value-level mutation
  contract for previously fixed bugs. Each case reverts one fix to its original
  buggy shape and asserts the named regression test fails; missing/duplicate
  anchors, unviable mutants, empty test filters, and survivors all exit
  non-zero. It edits files in place (restoring and `touch`-ing afterwards), so
  never run it alongside another cargo process or while editing sources. When
  you fix a bug, add its case in the same PR. See
  `docs/testing/regression-catalog.md`.

- `scripts/capture-schemas.py` — same constraints (macOS + reference binary,
  version-pinned to 3.0.0). Captures each built-in workflow schema listed in
  its `SCHEMAS` (currently `no-spec` and `spec-driven`) through `schemas`/`status`/
  `instructions` into `crates/spectra-core/assets/schemas/<name>-3.0.0.json`,
  using a sentinel `spec_dir` to recover the `{{SPEC_DIR}}` placeholder. Any
  drift exits non-zero keeping the sandbox; `--write` regenerates and
  re-verifies. Never hand-edit the assets.
- `scripts/capture-validate-openspec.py` — needs Node.js and OpenSpec 1.13.2
  (`--openspec-js`/`OPENSPEC_JS`; the version is checked), not the oracle.
  Owner ruling D1 makes OpenSpec 1.13.2 the authority for `validate`'s rules,
  so it runs OpenSpec on the rule fixture
  `crates/spectra-cli/tests/fixtures/validate_openspec` (`--changes`/`--specs`,
  normal and `--strict`) and compares with
  `docs/reverse-engineering/golden/validate-openspec-1.13.2.json`, which
  `validate_openspec_integration.rs` replays field for field. Drift exits
  non-zero keeping the sandbox; `--write` regenerates and re-verifies. When a
  validate rule changes, add a fixture case and recapture — never hand-edit
  the golden.
- `scripts/capture-analyze.py` — same constraints (macOS + reference binary,
  version-pinned to 3.0.0, `--spectra-bin`/`SPECTRA_BIN` override). Builds each
  `analyze` scenario (plus the `conNumericClaimMismatch` cases in
  `scripts/capture-analyze-numeric-cases.json`) as a scratch project, records
  exit code, stdout and stderr (`tty` runs on a pseudo-terminal for colours),
  and compares with the self-describing
  `docs/reverse-engineering/golden/analyze-3.0.0.json`, which
  `analyze_golden_integration.rs` replays byte for byte. Two oracle
  nondeterminisms are normalised as documented divergences: `params` keys are
  sorted, and a fixture whose directory order is observable must list in
  byte-sorted order. Drift exits non-zero keeping the scratch projects;
  `--write` captures twice and writes only when both agree. Never hand-edit
  the golden.
- `scripts/parity-probe.py` — same constraints (macOS + reference binary,
  version-pinned to 3.0.0). Measures migration acceptance A1 (every oracle
  subcommand/flag is accepted) and A3 (read-only commands agree on exit code,
  JSON semantics, human stdout byte-for-byte, and stderr, on sandboxed copies
  of real projects passed via `--corpus`). Known divergences in
  `docs/reverse-engineering/golden/parity-known.tsv` are a ratchet: an
  unlisted divergence fails, and a listed one that no longer occurs fails too,
  so remove its row in the PR that fixes it. `--write-known` regenerates the
  file; review the diff. The CI-side counterpart for A2 is
  `crates/spectra-cli/src/template_cli_check.rs` (every `spectra ...` call in
  the embedded templates/skills must parse; its `KNOWN_GAPS` ratchets the
  same way). See `docs/migration-plan.md`.
- `scripts/shadow-report.py` — summarizes the implementation-switch logs
  (`shadow.jsonl`, `errors.jsonl` under `$XDG_STATE_HOME/openspectra`) as
  Markdown; see README "Switching between the reference `spectra` and
  OpenSpectra". Any CLI integration test helper that spawns the binary must set
  `OPENSPECTRA_IMPL=oss`, or a developer's own switch setting would hand the
  test suite to the reference binary.

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
`build-and-test` job (macOS skips the `#[cfg(target_os = "linux")]`-gated
tests, which is expected, not a failure). If a clippy
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
  severity-mapped exit codes as an unverified guess). The same goes for a
  shared parser: grep every caller before writing the CHANGELOG's list of
  affected commands (PR #177 changed `tasks::parse` and listed four commands,
  missing `validate`'s archived-task check at `validate.rs:433`, which five
  reviewers then caught independently).
- Calibration scripts are verification contracts, not printers: compare the
  recovered values against the pinned expectations, exit non-zero on drift or
  on a scan too short to cover them, and preserve the failing synthetic repo
  for inspection (see `scripts/calibrate-time.py --mode boundaries`).
- Before an oracle probe, confirm the binary was rebuilt from clean source.
  Mutation checks (a subagent's or your own) edit source → build → restore
  source, but `target/` keeps the mutated artifact and `git status` is clean,
  so the probe silently exercises the mutant. `touch` the source and rebuild,
  or `strings target/release/spectra | grep <mutation-marker>` to confirm the
  mutant string is gone, before trusting probe output (PR #84: a whole round
  of editor-resolution probes ran against a `pr-test-analyzer` leftover mutant
  — `.arg("/nonexistent/…")` — and every conclusion had to be thrown out).
- One probe jail, one operation. Running several steps and only then inspecting
  the final on-disk state lets a later step overwrite the state an earlier one
  produced, yielding a confident wrong conclusion (PR #84: plain `reset`
  truncates the file to `{}`, but running it back-to-back with `reset --all`
  and checking afterwards showed only the delete, so `--all` was misread as an
  inert flag — it is not; `reset` truncates and `--all` deletes).
- A comment or doc that states an invariant is a claim to verify like code —
  including while *fixing* another comment, since a fix can introduce a new
  false invariant (the most common real defect in the PR #100-#104 reviews).
  When you touch a comment, check every claim it makes against the
  implementation before committing.
- Probe the oracle before acting on a review finding, in either direction. A
  reviewer's "this mutation survives, add a test that locks it" or "this is an
  unaccepted silent failure" is reasoned from the code, not from the oracle,
  and in a port the surviving mutant can be the oracle-correct behavior (PR
  #177: the untested `is_task_line` guard in `mark_all_done` was the
  divergence — oracle v3.0.0 `archive --mark-tasks-complete` flips
  blank-description checkboxes too, so locking the guard would have fossilized
  it; #178 removed it instead. The same review's "`task done` on `[~]` still
  records touched files" finding turned out to be oracle behavior as well).
- PTY tests via `script(1)`: keep the child's stdin open until it exits. If the
  test writes its answer and closes stdin immediately, `script` sends `^D` to
  the PTY and the program may read EOF instead of the answer — so an
  "answering `n` aborts" test passes whether or not `n` was ever read (PR #182:
  the old `archive_prompts_and_aborts_on_a_terminal` survived a mutant that
  accepted `n`). Use the shared `run_on_terminal` helper in `cli_integration.rs`.
- A test that derives its expected value from the production helper it guards
  (e.g. building the parked path via `parked_root()`) cannot detect that
  helper being changed — the expectation moves with it. Assert the literal
  oracle path/output instead. Reading a test and judging it "pinned" is not
  evidence; revert the fix and watch it fail (PR #182: rows judged pinned by
  reading — #118's parked-store location, #160-4's already-synced MODIFIED
  count, `list --specs` wiring — turned out unguarded or cited the wrong test).

## Agent conduct

Most work here is "align behavior to the oracle, one probe at a time," and that
faithful-port momentum makes it easy to cross a boundary that belongs to a
human. Two classes of decision must **stop and surface** rather than be taken
silently or filed as an after-the-fact note:

- **Whether the task should still be done.** When a probe or investigation
  refutes an issue's premise (e.g. "some consumer needs this command" turns out
  to have zero consumers), report that finding and let the human rule on
  scope *before* porting the whole surface — do not finish the port and bury
  "no consumer" as an aside in the PR description (PR #84: a repo-wide scan
  found no plugin calls `spectra config`, yet the full interface was ported
  anyway).
- **Architecture trade-offs.** When oracle fidelity conflicts with another
  engineering value (atomicity, a new dependency), a PR that claims the choice
  is "left to the human" must not also arrive merge-ready with that choice
  already baked in — claiming a decision is open while shipping the decided
  code is having it both ways (PR #84: `write_atomically`'s atomic write vs the
  oracle's in-place write).

Cross-PR signal: when the control-log `autonomy_ratio` runs high (>70%), these
two are the usual sources of overreach — bias toward asking on them.

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
