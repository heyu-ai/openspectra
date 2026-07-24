# Reverse-engineering `spectra init`

How OpenSpectra's `init` command was designed, and what's still unverified
against the closed-source reference.

> **Not oracle-verified.** Unlike `drift.md`/`archive.md`/`task.md`, this
> command's behavior was **not** confirmed against
> `Spectra.app/Contents/MacOS/spectra`. Every other OpenSpectra command's
> error message says `Not initialized. Run 'spectra init' first.`, which
> implies the reference CLI has an `init` (or equivalent) subcommand, but no
> golden-oracle session covering it exists yet. OpenSpectra's `init` was
> instead designed from:
>
> - the error message wording above (all commands point at it)
> - `README.md`'s description of the on-disk layout (`.spectra.yaml`,
>   `<spec_dir>/changes/`, `<spec_dir>/specs/`)
> - the PR #19 self-recording bug, whose root cause was a project that had
>   never been `init`-ed and so had no `.spectra/` entry in `.gitignore`
>
> Treat every detail below as OpenSpectra's own design choice, not a
> confirmed match to the reference binary.

## CLI shape

```
spectra init [--adopt] [--json]
```

`init` is the only command that does **not** require
`Config::is_initialized` to already be true — every other subcommand calls
`require_initialized` first and fails with `Not initialized. Run 'spectra
init' first.` if `.spectra.yaml` is missing.

## Behavior

1. If `.spectra.yaml` already exists at the target root, fail loudly with an
   error containing `already initialized` and touch nothing else. There is no
   re-init or `--force` escape hatch.
2. Otherwise, produce the following **in this order** — `.spectra.yaml` is
   written last on purpose, since it's the file `Config::is_initialized`
   checks: if any earlier step fails, nothing has marked the project
   initialized yet, so a fixed retry can complete normally instead of
   immediately hitting step 1's "already initialized" bail:
   - `openspec/changes/` and `openspec/specs/` (empty directories; git won't
     track them until something is written inside)
   - a `.gitignore` entry for `.spectra/` (OpenSpectra's own per-change
     sidecar state directory — baseline SHAs, parked markers, in-progress
     markers, touched-file tracking — which must never be committed):
     - no `.gitignore` exists → create one containing just `.spectra/`
     - `.gitignore` exists but lacks the entry → append it, first inserting a
       trailing newline if the file didn't already end in one, so existing
       content is never corrupted
     - `.gitignore` already has a `.spectra/` line (exact match after
       trimming surrounding whitespace) → leave the file untouched
   - `.spectra.yaml` containing `spec_dir: openspec\n` (the `spec_dir` name
     is currently always the `config::DEFAULT_SPEC_DIR` constant — there's no
     `--spec-dir` flag yet)
3. `--json` output: `{"root": "<absolute path>", "spec_dir": "openspec",
   "adopted": <bool>, "gitignore_updated": <bool>}`. `adopted` is `false`
   for this plain init path. `gitignore_updated` reflects whether step 2
   actually wrote to `.gitignore` (`false` when an entry was already present).

## OpenSpectra Phase 2 addition: `--adopt`

`spectra init --adopt` is an OpenSpectra compatibility addition for real
OpenSpec projects, documented in `docs/openspec-compat.md`. It is still not
oracle-verified against `Spectra.app`; it exists because OpenSpec projects
have `openspec/` content but no root `.spectra.yaml`, while every
OpenSpectra command uses `.spectra.yaml` to find and load the project.

Adopt mode keeps the same already-initialized refusal: if `.spectra.yaml`
exists, it fails with an error containing `already initialized` and touches
nothing else. The `spec_dir` is always the default `openspec` — adopt does
**not** inspect the directory's contents to choose a name (configurable
spec-dir discovery is future work). Its one content check is a guard: if
`openspec` already exists as a *file* rather than a directory, adoption fails
with `cannot adopt: … exists but is not a directory` instead of letting the
later `create_dir_all` surface a generic I/O error.

Adopt mode is deliberately non-destructive. It creates
`openspec/changes/` and `openspec/specs/` with `create_dir_all` (a no-op when
they already exist), ensures the `.spectra/` `.gitignore` entry, and writes
`.spectra.yaml` last with `spec_dir: openspec\n`. It never creates or
overwrites OpenSpec-owned `project.md`, `AGENTS.md`, `config.yaml`, existing
changes, or existing canonical specs.

Mechanically, plain `init` and `--adopt` perform the same filesystem work
(both create the two dirs idempotently, both refuse only when `.spectra.yaml`
exists); `--adopt` differs in **intent and messaging** — it announces
`Adopted …` and is the self-documenting path for a project that already holds
OpenSpec content. A plain `init` on such a project (with no `.spectra.yaml`
yet) also succeeds.

## Files produced (for future oracle comparison)

If a golden-oracle session for `init` ever becomes possible, compare against
this file set and wording:

| Path | Content |
|---|---|
| `<spec_dir>/changes/` | empty directory |
| `<spec_dir>/specs/` | empty directory |
| `.gitignore` | `.spectra/` line ensured present |
| `.spectra.yaml` | `spec_dir: openspec\n` (written last — see "Behavior" above) |

Open questions an oracle session would need to answer:

- Does the reference CLI print a human-readable confirmation message, and if
  so, what's its exact wording?
- Is `spec_dir` ever anything other than `openspec` by default, or
  configurable via a flag?
