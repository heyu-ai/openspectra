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
| 7–21 | `aging (Nd)` | 1 |
| 22–60 | `stale (Nd)` | 2 |
| ≥61 | `abandoned (Nd)` | 4 |
| — | `no created date` / `invalid created date` | 0 |

All three boundaries and the `abandoned` score are now **pinned exactly** (not
interpolated) by sweeping synthetic changes with controlled `created` dates
through the oracle — `scripts/calibrate-time.py --mode boundaries` reproduces
the transitions at days **7, 22, and 61**. This corrected three earlier guesses:
the `aging|stale` edge was coded `<21` (should be `<22`; day 21 is still
`aging`), the `stale|abandoned` edge was coded `<60` (should be `<61`; day 60 is
still `stale`), and `abandoned` was scored `3` when the oracle actually scores it
**`4`** — the score ladder jumps `2 → 4`, skipping 3. Source line numbers do not
matter; see `calibration::time_bucket`.

Two further probed edge behaviors: **no transition exists above 61** (probed
out to 3650 days — all `abandoned`, score 4), and a **future `created` date is
clamped to 0** (created = today+1/+30/+365 all report `fresh (0d)`, never a
negative day count).

`today` is derived from the **process-local timezone**, not UTC: running the
oracle with a shifted `TZ` env var (e.g. `TZ=Etc/GMT+11`) moves its reported
day count accordingly, and OpenSpectra's `Local::now()` moves identically in
every probed zone.

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

Score is **category-weighted**, not a pure function of decay. Recovered exactly
via the calibration harness (`scripts/calibrate-structure.py`):

```text
decay = broken / total
D = 0 if decay < 10% | 1 if 10% ≤ decay < 30% | 2 if decay ≥ 30%
score = min(2·D + 3,  2·D + broken_cliflag_count)
```

A broken **CliFlag** adds 1 each (capped by `2·D+3`, global max 7); a broken
**FilePath** contributes *only* through the decay band `D`. Isolation evidence
(harness): `1/7` FilePath → 2 but `1/7` CliFlag → 3; `3/40` FilePath → 0 but
`3/40` CliFlag → 3. Same decay, different score ⇒ category matters. The `D`
boundaries are exact: `10%` (`10.0%` → D1, `9.1%` → D0) and `30%` (= the heavy
short-circuit). Verified against every real golden: `0/16` cf0 → 0, `3/40`
cf3 → 3, `9/29` cf9 → 7, `12/25` cf12 → 7.

> An earlier revision modelled this as a decay-only ladder `0,1,3,5,7`. That fit
> the goldens **only by accident**: every golden's broken anchors were CliFlags,
> so `min` always saturated at the `2·D+3` cap. It scored FilePath-broken and
> mixed changes wrong (e.g. returned 3 for `3/40` FilePath, oracle returns 0).
> Reproduce the derivation: `python3 scripts/calibrate-structure.py --oracle
> <path> --mode iso` (count isolation) and `--mode verify-model` (cross-check).
> `>30%` decay also forces `heavy` severity — `calibration::structure_score`.

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

## Exit codes

A successful `drift` run **always exits 0**, regardless of severity — probed
across the full severity space (light / medium / heavy, JSON and human output
modes). Severity is *not* mapped to the exit code. Operational errors (e.g.
`Error: Change 'x' not found.`) exit **1**. OpenSpectra originally guessed a
0/1/2 severity mapping (and 3 for errors); both were refuted by probe and now
match the oracle.

## What is verified vs. uncertain

| Area | Status |
|------|--------|
| JSON schema, dimension model, `total_score` rule | ✅ exact |
| FilePath / CliFlag / Function extraction & resolution | ✅ exact (byte-for-byte on golden runs) |
| Structure score formula (category-weighted), severity bands, recommendation map | ✅ exact — harness-recovered + golden-verified |
| Time score curve + all day boundaries | ✅ exact — pinned via `scripts/calibrate-time.py` (transitions at 7/22/61; `abandoned` scores 4; future dates clamp to 0d) |
| Exit codes (0 on success regardless of severity; 1 on errors) | ✅ exact — probed across the severity space |
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
   CliFlags broken). The scripted harness (`scripts/calibrate-structure.py`)
   works *with* this behaviour: it commits `design.md` but writes it in
   all-lowercase prose so the Symbol/Function regexes extract nothing, leaving
   `total` = resolved + broken FilePaths + broken CliFlags. (The separate manual
   probe below instead leaves `design.md` **untracked** to force *every* anchor
   broken and reveal the full set — a different technique for a different goal.)
   A future fix would exclude the change/spec dir from grep (`-- ':!<change-dir>'`);
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
