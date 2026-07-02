# Reverse-engineering `spectra init`

How OpenSpectra bootstraps a project, and how that relates (as far as we can
tell) to the closed-source `spectra init`.

> **Oracle status: NOT verified.** The reference `spectra` CLI ships only as a
> macOS arm64 app bundle; at the time this was written the project's calibration
> workflow had no captured `init` golden output to compare against. Unlike
> `drift`/`archive`/`task` (see the sibling docs), the file set and message
> wording below are **designed from first principles**, driven by the error
> messages every other subcommand emits and by what `Config` and the rest of the
> codebase require to exist. Treat this as a *deliberate, documented divergence
> candidate*: if an oracle later becomes available, reconcile this file and the
> `init` module against it in the same PR (per `CLAUDE.md`).

## Why `init` had to exist at all

Every other subcommand refuses to run on an un-initialized tree with:

```
Not initialized. Run 'spectra init' first.
```

…but until this feature landed there *was no* `spectra init` subcommand, so a
Linux user cloning a fresh project had no way to bootstrap — a hard dead-end
(roadmap Phase 1, "已知缺口"). `init` closes that loop.

## What `init` writes

Given a target directory (the **current working directory** — `init` is the one
command that does *not* walk up to an ancestor `.spectra.yaml`, so it can't
refuse-as-already-initialized from inside an unrelated parent project):

| Path | Contents / purpose |
|------|--------------------|
| `.spectra.yaml` | `spec_dir: openspec\n` — the project config, and the sentinel `Config::is_initialized` keys off. Written **last** (see below). |
| `<spec_dir>/changes/` | empty change directory (`spec_dir` defaults to `openspec`) |
| `<spec_dir>/specs/` | empty canonical-spec directory |
| `.gitignore` | ensures a `.spectra/` line (created if absent, appended if missing, left alone if already present) |

### Why `.spectra/` must be git-ignored

`.spectra/` is OpenSpectra's own state directory (baseline SHAs under
`changes/<name>.started`, parked markers, touched-file tracking under
`touched/<name>.json`). PR #19's "self-recording" bug — where the tool recorded
its *own* state files as touched implementation files — traced back to a project
that had never git-ignored `.spectra/`. Adding it at `init` time is the
structural fix.

The `.gitignore` handling is deliberately conservative:

- **Idempotent**: a line equal (trimmed) to `.spectra/` *or* a bare `.spectra`
  counts as already-covered — no duplicate entry is appended.
- **Comment-safe**: a commented-out `# .spectra/` does *not* count as covered.
- **No substring false-positives**: `.spectra-backup/` does not count.
- **Newline-safe**: appending to a `.gitignore` whose last line lacks a trailing
  newline inserts one first, so `target` never becomes `target.spectra/`.

## Ordering / crash-safety

The three write phases run **skeleton dirs → `.gitignore` → `.spectra.yaml`**,
config last on purpose. `.spectra.yaml` is the "initialized" sentinel, so
writing it only after the other steps succeed means an interrupted run leaves
the project *un*-initialized (and therefore safely re-runnable) rather than
initialized-but-incomplete. Each step is individually idempotent, so re-running
after removing `.spectra.yaml` completes the job cleanly.

## Errors

- **Already initialized** (`.spectra.yaml` exists in the target dir): `init`
  bails with `already initialized: <path> exists`. `init` is a bootstrap, not a
  reconcile — adopting an existing `openspec/` tree that lacks the sidecar
  config is Phase 2's `init --adopt`, intentionally out of scope here.

## `--json` shape

```
{ "spec_dir", "config", "changes_dir", "specs_dir", "gitignore_updated" }
```

`gitignore_updated` is `false` only when the `.spectra/` pattern was already
present (a re-init on a hand-configured tree), never because the step was
skipped.

## Open questions for a future oracle pass

- Does the reference `init` write additional keys to `.spectra.yaml`
  (e.g. `locale`, a `schema`/version marker)? OpenSpectra writes only `spec_dir`.
- Does it emit any files beyond the config + two dirs (a `project.md`, a README
  stub)? OpenSpec (Fission-AI) projects have `openspec/project.md`; whether
  `spectra init` scaffolds one is a Phase 2 compatibility question.
- Exact human-readable success wording and whether it prints next-step guidance.
