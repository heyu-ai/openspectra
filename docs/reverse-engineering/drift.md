# Reverse-engineering `spectra drift`

How the closed-source `spectra drift` command decides that a change's spec
artifacts have drifted from the codebase, and how OpenSpectra reproduces it.

> Source binary: `Spectra.app/Contents/MacOS/spectra` v2.3.1 (arm64 Mach-O,
> **symbols retained** → string mining was highly effective). Behaviour was
> confirmed by running the binary as a **golden oracle** over real changes in
> several projects and over purpose-built synthetic inputs.

## TL;DR

`drift` is **pure git + filesystem + regex heuristics — no AI/embeddings**, so
it is inherently cross-platform and CI-friendly (the `search` command is the one
that uses vector/BM25 indexing, not `drift`). Internally it lives in
`crates/spectra-core/src/drift.rs` and scores a change across four dimensions.

## On-disk layout it reads

| Path | Meaning |
|------|---------|
| `.spectra.yaml` | project config; `spec_dir` (default `openspec`), `locale` |
| `<spec_dir>/changes/<name>/` | change dir: `proposal.md`, `design.md`, `tasks.md`, `specs/<cap>/spec.md` |
| `<spec_dir>/changes/<name>/.openspec.yaml` | `schema, created (YYYY-MM-DD), created_by, created_with, archived_by, archived_at` |
| `.spectra/changes/<name>.started` | baseline git SHA recorded when work began (optional) |
| `.spectra/changes/<name>.parked` | parked marker |

Archived changes use a `YYYY-MM-DD-` name prefix; active change names are
kebab-case (`^[a-z0-9]+(-+[a-z0-9]+)*$`).

## JSON schema (`spectra drift <change> --json`)

```
change_id, created, last_commit, dimensions[], broken_anchors[],
tasks_maybe_resolved[], tasks_blocked_external[],
commits_since_created, total_score, severity, primary_recommendation
```

* `dimensions[]` = `{ kind, status, score, contributes_to_total }`,
  `kind ∈ Time | Structure | Tasks | Environment`.
* **`total_score = Time + Structure + Tasks`.** `Environment.contributes_to_total = false`
  (display-only, e.g. `"93 commits"`).
* `broken_anchors[]` = `{ anchor, category, reason }`,
  `category ∈ FilePath | Symbol | Function | CliFlag`.
* `last_commit` is **always `null`** in observed v2.3.1 output (it is not the
  change dir's last commit — that is non-null in git but the field stays null).
  Semantics of any non-null value are undetermined; OpenSpectra emits `null`.

## The four dimensions

### 1. Time — dormancy from `created`
`days_old = today − created` (calendar days). Status word + score, calibrated
against the oracle:

| days | status | score |
|------|--------|-------|
| 0–6 | `fresh (Nd)` | 0 |
| 7–20 | `aging (Nd)` | 1 |
| 21–59 | `stale (Nd)` | 2 |
| ≥60 (unobserved) | `abandoned (Nd)` | 3 |
| — | `no created date` / `invalid created date` | 0 |

`fresh|aging` boundary lies in 5↔7; `aging|stale` in 19↔25; `stale|abandoned`
is unobserved (60 is a guess). Source line numbers do not matter; only the
constants are uncertain — see `calibration::time_bucket`.

### 2. Structure — broken design anchors
Anchors are references in `design.md` to code artifacts, extracted by four
regexes recovered verbatim from `.rodata`:

| category | regex | resolution check | broken reason |
|----------|-------|------------------|---------------|
| FilePath | `(?:src-tauri\|src\|crates\|docs)/[\w./-]+\.(?:rs\|ts\|svelte\|md\|toml)` | tracked / exists on disk | `file does not exist` |
| CliFlag | `--[a-z][a-z0-9-]+` | (none available) | `not in --help` |
| Function | `\b([a-z][a-z0-9]*_[a-z0-9_]+)\(` (snake) and `\b([a-z][a-z0-9]*[A-Z][a-zA-Z0-9]+)\(` (camel) | `git grep` finds it | `function not found in repo` |
| Symbol | `\b[A-Z][a-zA-Z0-9]+\b` minus a small stop-list | `git grep` finds it | `symbol not found in repo` |

Checks are capped at **`ANCHOR_CAP = 50`**. Broken anchors are sorted
alphabetically. Resolution uses `git ls-files` (file existence) and `git grep`
(symbol/function existence).

**The FilePath stack is Rust/TS-specific.** In Python/Go projects almost no file
paths match, so FilePath anchors are rare and CliFlag dominates the broken set.

**CliFlag is always broken** ("not in --help"): there is no target `--help` to
diff design flags against in an arbitrary project, so every extracted `--flag`
is reported broken — even ones written in a *Non-Goal* section. OpenSpectra
reproduces this faithfully (see Known divergences for the improvement path).

Score is a function of **decay = broken / total** (fits every field sample):

| decay | score |
|-------|-------|
| 0 | 0 |
| <6% | 0 |
| <7% | 1 |
| <25% | 3 |
| <30% | 5 |
| ≥30% | 7 |

The score ladder is `0,1,3,5,7` (odd). `>30%` decay also forces `heavy`
severity. Inner boundaries (the tight 6%/7% band) are interpolated between
sparse samples — `calibration::structure_score`.

### 3. Tasks — collisions with external work
Parses `tasks.md` checkboxes (`- [ ]` / `- [x]`) and the inline backtick file
paths each task names. For pending tasks:
* `tasks_blocked_external` — a referenced file was changed by external commits
  since the `.started` baseline.
* `tasks_maybe_resolved` — pending tasks whose verb+target keywords match a
  commit subject since `created` ("maybe done elsewhere").

**Every captured oracle sample reported `0 blocked, 0 maybe-done`** — including
in-progress changes with many pending tasks and 100+ intervening commits that
*did* touch the referenced files. With no positive sample, the exact predicates
cannot be verified, and each heuristic tried (file-touched-since-baseline,
file-missing, commit-names-change) produced false positives the oracle never
emits. OpenSpectra therefore keeps Tasks detection **off** behind
`calibration::TASKS_DETECTION_CALIBRATED = false`, matching 100% of observed
behaviour. The parser and data model are complete and tested; flip the flag once
a positive sample is captured.

### 4. Environment — display only
`commits_since_created = git rev-list --count --since=<created> HEAD`. Shown as
`"N commits"`, `contributes_to_total = false`.

## Severity & recommendation

| severity | total | recommendation |
|----------|-------|----------------|
| light | 0–3 | `/spectra-apply <change>` |
| medium | 4–8 | `/spectra-ingest <change>` |
| heavy | >8 **or** decay >30% | `spectra archive <change> --skip-specs` |

(light/medium recommend a slash-command; heavy recommends the real CLI.)

## What is verified vs. uncertain

| Area | Status |
|------|--------|
| JSON schema, dimension model, `total_score` rule | ✅ exact |
| FilePath / CliFlag / Function extraction & resolution | ✅ exact (byte-for-byte on golden runs) |
| Structure score curve, severity bands, recommendation map | ✅ exact on all samples |
| Time score curve | ✅ matches all samples; outer day boundaries `CALIBRATE` |
| `commits_since_created`, git commands | ✅ exact |
| **Symbol extraction narrowing** | ⚠️ **open** — see below |
| Tasks positive-case predicates | ⚠️ uncalibrated (no positive sample); detection gated off |

### Open problem: the Symbol narrowing filter
The recovered Symbol regex matches every capitalised token, but the oracle keeps
only a small subset (e.g. **12 of ~83** prose candidates in one Chinese-prose
design). The selection is **not** explained by code-context (backtick/fence),
frequency, position, or pairing: in `Data Model` it keeps `Data`, drops `Model`;
in `ALTER TABLE ADD COLUMN` it keeps `ADD`, drops the rest — identical context,
different outcome. In isolation all tokens are kept, so the rule is **global to
the document** and not visible in strings. Cracking it needs disassembly of the
extraction routine in `spectra-core::drift`. Until then OpenSpectra extracts the
full regex set, which **over-counts Symbols on prose-dense designs**, inflating
`total` and reading Structure decay/score *lower* than the oracle there.
FilePath/Function/CliFlag — the categories that actually drive real drift
signals — are exact.

### Known limitations (resolution side, deferred)
Two resolution behaviours are carried as-is pending a positive oracle decision
(`spectra-core::git`, `anchors::Resolver`):

1. **`git grep` self-matches the change's own `design.md`.** Symbol/Function
   anchors are resolved with `git grep` over *all* tracked files, which includes
   the very `design.md` they were extracted from — so a committed change's
   Symbol/Function anchor always finds itself and is never reported broken; only
   FilePath/CliFlag anchors drive Structure on a committed change. This is
   consistent with the oracle's observed output (golden samples only ever showed
   CliFlags broken) and with the calibration harness below, which deliberately
   leaves `design.md` **untracked** precisely so anchors resolve to nothing. A
   future fix would exclude the change/spec dir from grep (`-- ':!<change-dir>'`);
   it is deferred until an oracle run confirms the intended behaviour.
2. **FilePath resolves while still in the git index.** A path is considered
   present if it is tracked *or* on disk, so a working-tree `rm` without `git rm`
   still reads as resolved until staged. `drift` describes "current codebase
   state", so this can hide a just-deleted file; kept for now as the
   lower-risk, likely-oracle-faithful behaviour.

## Reproducing the oracle (calibration harness)

The probe technique that produced these tables, reusable to crack the remaining
unknowns:
1. `git init` a temp repo with `.spectra.yaml` and a change dir.
2. To reveal the **full** anchor set (resolved anchors are hidden in normal
   output), leave `design.md` **untracked** so `git grep` resolves nothing →
   every anchor is reported broken and thus listed.
3. Vary one input at a time (a symbol in prose vs. backtick vs. fence; a flag in
   each context) and diff the oracle's `broken_anchors` to isolate each rule.

The `scripts/` probes and the per-change golden JSON used for calibration live
alongside this doc's git history.
