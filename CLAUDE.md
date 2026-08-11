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
- `scripts/capture-skills.py` — same constraints (macOS + reference binary,
  version-pinned to 2.3.1); the default application path can be overridden by
  `--spectra-bin` or `SPECTRA_BIN`. It verifies all 15 embedded skill assets
  byte-exact against the oracle, checks their byte lengths and SHA-256 values
  against `docs/reverse-engineering/golden/skills-2.3.1.tsv`, rejects known
  absent enumeration candidates, cross-checks the Rust registry, and pins the
  unknown-skill stderr/exit contract. The assets are the oracle captures and
  the TSV pins their provenance; both are generated artifacts. Any drift exits
  non-zero; `--write` regenerates both and re-verifies. Never hand-edit them.

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
  including while *fixing* another comment. The PR #100-#104 mob reviews'
  most-hit real-defect class was stated-but-false invariants (4 findings: a
  security-rationale constraint the code didn't hold, a lock instruction other
  tests couldn't follow, a test's false no-lock justification, probe records
  attributed to cases they didn't cover), and one was reintroduced *by the fix*
  for another. When you touch a comment, check every claim it makes against
  the implementation before committing.

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
