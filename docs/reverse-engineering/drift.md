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
| `<git common dir>/spectra-app/changes/<name>/` | a parked change's directory, moved out of `changes/` (see `park.md`) |

Archived changes use a `YYYY-MM-DD-` name prefix; active change names are
kebab-case (`^[a-z0-9]+(-+[a-z0-9]+)*$`).

## JSON schema (`spectra drift <change> --json`)

```
change_id, created, last_commit, dimensions[], broken_anchors[],
unresolved_anchors[], tasks_maybe_resolved[], tasks_blocked_external[],
commits_since_created, total_score, severity, primary_recommendation
```

* `dimensions[]` = `{ kind, status, score, contributes_to_total }`,
  `kind ∈ Time | Structure | Tasks | Environment`.
* **`total_score = Time + Structure + Tasks`.** `Environment.contributes_to_total = false`
  (display-only, e.g. `"93 commits"`).
* `broken_anchors[]` = `{ anchor, category, reason }`,
  `category ∈ FilePath | Symbol | Function | CliFlag`.
* `unresolved_anchors[]` has the same entry shape and is an OpenSpectra
  extension with no counterpart in the v2.3.1 oracle. Since #119 narrowed #83 it
  holds exactly one class: a FilePath that did not exist at the change's
  `.started` baseline (`forward reference`). That is a *confident* exclusion
  backed by the baseline SHA, not an "unclassifiable" bucket.
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

Sets of **`ANCHOR_CAP = 50`** anchors or fewer are checked whole. Larger sets
are **not truncated**: each category independently keeps an evenly spaced sample
of at most **`ANCHOR_SAMPLE_PER_CATEGORY = 12`**, taking index `i * n / 12` for
`i` in `0..12`. A category holding 12 or fewer anchors is kept whole, so the
reported denominator above the cap is `12` per over-cap category plus the full
count of every under-cap one — e.g. 12, 24, or 36 when each present category is
over the sample size, but `17` for 60 FilePath + 5 Function. It is never a value
between 51 and the raw extracted count.

Pinned by probe against v2.3.1 (`spectra drift <change> --json` over synthetic
designs): pure-FilePath `50` → `50/50` but `51` → `12/12`; `53` and `77` anchors
both reproduce the `i * n / 12` index set exactly; `20` FilePath + `20` Function
+ `20` CliFlag (60 total) → `36`, proving the trigger is the combined total
while the sample is per category; `60` FilePath + `5` Function → `17`, showing
categories under the sample size are kept whole.

> OpenSpectra originally read `ANCHOR_CAP` as a plain `truncate(50)`. That made
> every large design report a denominator of exactly 50 and silently dropped
> anchors past index 50 from the broken set — the miss reported in #119.

Broken anchors are sorted alphabetically. Resolution uses `git ls-files` (file
existence) and `git grep` (symbol/function existence).

**The FilePath stack is Rust/TS-specific.** In Python/Go projects almost no file
paths match, so FilePath anchors are rare and CliFlag dominates the broken set.

**The oracle reports every CliFlag broken** ("not in --help"): there is no
target `--help` to diff design flags against in an arbitrary project, so every
extracted `--flag` is reported broken — even ones written in a *Non-Goal*
section. OpenSpectra matches this.

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

OpenSpectra uses this formula with the oracle's own broken set. The one input
it still withholds is a FilePath that did not exist at the change baseline (see
Deliberate divergences); such anchors stay in the denominator, appear in
`unresolved_anchors`, and do not increment the broken count, score, decay
numerator, `total_score`, or severity short-circuit.

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

## Deliberate divergences

### Forward-reference FilePaths are not broken (#83, narrowed by #119)

Issue #83 was motivated by a real-repository measurement: drift reported 13
broken anchors, but only one was a genuine deletion — an **8% signal rate**.
Eleven of the 13 were CliFlags and none of those was actionable. Direction 1
(maintainer-approved) moved CliFlags, `git grep`-missing Functions, and
not-yet-created FilePaths into a sibling `unresolved_anchors` array, excluded
from Structure scoring.

Issue #119 then measured the cost of that on 26 real changes: the oracle flagged
broken anchors OpenSpectra reported as `0`, and a CI drift gate that stays green
while drift exists is worse than a noisy one. The CliFlag and Function halves of
#83 were reverted; only the FilePath half survives:

* **CliFlag:** broken with reason `not in --help`, matching the oracle. Reverted
  by #119 — this is the noisiest category (11 of #83's 13) but withholding it
  put the score out of step with the oracle on every real change.
* **Function not found by `git grep`:** broken with reason
  `function not found in repo`, matching the oracle. Reverted by #119. Absence
  still cannot distinguish a deleted project function from a builtin or
  dependency symbol such as PostgreSQL's `jsonb_array_length`, so this category
  retains a known false-positive rate.
* **Missing FilePath:** the surviving divergence. Consult the change's
  `.started` baseline SHA. If the path existed at that commit and is now gone,
  it is broken with reason `file does not exist`; if it did not exist, it is
  unresolved with reason `forward reference`, because the change may be
  proposing to create it. The oracle calls both broken. If the baseline SHA is
  absent, malformed, or unavailable locally, OpenSpectra falls back to the
  broken classification rather than hiding a possible deletion.
* **Symbol:** broken when not found, same as the oracle. Its separate extraction
  divergence remains tracked by #8/#51.

The JSON shape is unchanged: `broken_anchors` and `unresolved_anchors` both
remain, and human output still renders them under separate headings. The score
formula, severity bands, recommendations, and exit-code mapping are untouched.

The four real calibration goldens record oracle scores `0`, `3`, `7`, `7`, with
broken-category compositions of 0, 3, 9, and 12 CliFlags respectively. Under #83
OpenSpectra's expectation for all four was `0`. #119 makes those
`(broken, broken_cliflags, total)` triples *reachable* again — under #83 no
resolver run could yield a non-zero `broken_cliflags` — and that is all it
establishes.

> **The goldens are not replayed, and cannot be as they stand.** No test reads
> `golden/drift-*.json`; `calibration.rs`'s golden test asserts
> `structure_score(...)` on hand-copied literals. The fixtures capture the
> oracle's *output* only — the input repositories and `design.md` files were
> never preserved, so there is nothing to feed `drift::analyze`, and building
> synthetic lookalikes would test equivalent examples rather than replay these
> four. Closing it needs input snapshots captured first (#132).
>
> This is narrower than "the downstream chain is untested". For #119's changed
> behavior the score → severity → recommendation chain *is* exercised through
> the real `drift::analyze` in `drift_integration.rs` (on synthetic repos), for
> both the CliFlag/Function reclassification and the over-cap sampling. The
> exit-code end is *not* covered for anchors specifically:
> `cli_integration.rs::drift_exits_zero_even_when_severity_is_medium_or_higher`
> reaches `medium` through the **Time** dimension (`created: 2020-01-01`), so it
> pins the always-exit-0 contract but says nothing about Structure. What is
> missing is any assertion anchored to these four captured fixtures.

## What is verified vs. uncertain

| Area | Status |
|------|--------|
| JSON schema, dimension model, `total_score` rule | ⚠️ additive `unresolved_anchors` divergence; existing fields preserved |
| FilePath / CliFlag / Function extraction & resolution | ⚠️ extraction exact; CliFlag/Function resolution exact since #119; missing-FilePath still diverges on forward references |
| Over-cap anchor sampling (per category, `i * n / 12`) | ✅ exact — probed at the 50/51 boundary and at n = 53, 60, 77, and across 1–3 categories |
| Structure score formula (category-weighted), severity bands, recommendation map | ✅ formula/mappings exact; #119 makes the goldens' triples reachable again, but the fixtures are output-only and never replayed (#132) |
| Time score curve + all day boundaries | ✅ exact — pinned via `scripts/calibrate-time.py` (transitions at 7/22/61; `abandoned` scores 4; future dates clamp to 0d) |
| Exit codes (0 on success regardless of severity; 1 on errors) | ✅ exact — probed across the severity space |
| `commits_since_created`, git commands | ✅ exact |
| Symbol extraction | ✅ exact — the apparent "narrowing" was the over-cap sampling, not a semantic filter (#8/#51, see below) |
| Tasks positive-case predicates | ⚠️ uncalibrated (no positive sample); detection gated off |

### Solved: the "Symbol narrowing filter" was the over-cap sampling (#8/#51)

For several releases this was recorded as the single open reverse-engineering
question: the recovered Symbol regex matches every capitalised token, yet the
oracle appeared to keep only a small semantic subset — **12 of ~83** prose
candidates in one Chinese-prose design — and the selection resisted every
predicate tried (code-context, frequency, position, pairing). `Data Model` kept
`Data` and dropped `Model`; `ALTER TABLE ADD COLUMN` kept `ADD` and dropped the
rest, from identical context.

There is **no semantic filter**. The narrowing is the per-category over-cap
sampling documented above, and the old investigation's own decisive observation
— *"in isolation all tokens are kept, so the rule is global to the document"* —
is exactly the signature of a document-global cap, which is what it is. Probed
on v2.3.1 with a design of `N` bare `WidgetNN` tokens and nothing else:

| Symbol candidates | total anchors | oracle keeps |
|---|---|---|
| 30 | 30 (≤ cap) | **all 30**, including `Data`, `Model`, and `The` in the `Data Model` probe |
| 83 | 83 (> cap) | exactly 12, at indices `floor(i * 83 / 12)` → `Widget{01,07,14,21,28,35,42,49,56,63,70,77}` |

`12 of ~83` is `ANCHOR_SAMPLE_PER_CATEGORY` of 83. The apparently arbitrary
`Data`-but-not-`Model` outcomes were positional: those designs exceeded the cap,
so which token survived depended only on its index in the extracted set.
OpenSpectra reproduces both rows exactly.

Consequence: **Symbol extraction was never divergent**, and the previously
recorded over-count (OpenSpectra "inflating `total` and reading Structure
decay/score *lower* than the oracle" on prose-dense designs) was a symptom of
the missing sampling, fixed by #119. All four categories are now exact on
extraction, and only the missing-FilePath forward-reference case diverges on
resolution.

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
4. To settle **timezone questions** ("does the binary derive `today` from local
   time or UTC?"), run the oracle under a shifted `TZ` env var (e.g.
   `TZ=Etc/GMT+11`) and watch whether its reported day count moves: it moves →
   process-local time; it stays → UTC or a cached value. One run is decisive —
   this refuted the UTC theory for v2.3.1 (it is local-time, matching
   `Local::now()`).

The `scripts/` probes and the per-change golden JSON used for calibration live
alongside this doc's git history.
