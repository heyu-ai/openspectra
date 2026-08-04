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
unresolved_anchors[], tasks_maybe_resolved[], tasks_blocked_external[],
commits_since_created, total_score, severity, primary_recommendation
```

* `dimensions[]` = `{ kind, status, score, contributes_to_total }`,
  `kind ∈ Time | Structure | Tasks | Environment`.
* **`total_score = Time + Structure + Tasks`.** `Environment.contributes_to_total = false`
  (display-only, e.g. `"93 commits"`).
* `broken_anchors[]` = `{ anchor, category, reason }`,
  `category ∈ FilePath | Symbol | Function | CliFlag`.
* `unresolved_anchors[]` has the same entry shape. It is an OpenSpectra
  extension for references that cannot be classified reliably as broken; the
  v2.3.1 oracle has no corresponding field.
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
| FilePath | `(?:src-tauri\|src\|crates\|docs)/[\w./-]+\.(?:rs\|ts\|svelte\|md\|toml)` — OpenSpectra prepends `(?:[\w.-]+/)*`, requires a left token boundary, and drops `..`-escaping paths; see Deliberate divergences | tracked / exists on disk | `file does not exist` |
| CliFlag | `--[a-z][a-z0-9-]+` | (none available) | `not in --help` |
| Function | `\b([a-z][a-z0-9]*_[a-z0-9_]+)\(` (snake) and `\b([a-z][a-z0-9]*[A-Z][a-zA-Z0-9]+)\(` (camel) | `git grep` finds it | `function not found in repo` |
| Symbol | `\b[A-Z][a-zA-Z0-9]+\b` minus a small stop-list | `git grep` finds it | `symbol not found in repo` |

Broken anchors are sorted alphabetically. Resolution uses `git ls-files` (file
existence) and `git grep` (symbol/function existence).

#### The anchor budget (`ANCHOR_CAP = 50` is a trigger, not a truncation)

`ANCHOR_CAP` does **not** clamp the checked set to the first 50 anchors. It is
the threshold at which the oracle switches to positional downsampling:

```text
candidates = per-category extraction, deduped, in document order
if sum(len(c) for c in categories) <= 50:
    every candidate is checked
else:
    each category independently keeps the indices  i * n / 12,  i in 0..11
    (integer division; a category with n ≤ 12 survives whole)
```

So the reported total above the cap is `sum(min(n_category, 12))` — 12 when one
category dominates, up to 48 when all four are large. Category order for the
purposes of "document order" is the extraction order (FilePath, CliFlag,
Function, Symbol), and within `Function` the snake-case matches precede the
camel-case ones.

Recovered by probe against v2.3.1 on 2026-08-03 and confirmed on the twelve
cases in `scripts/calibrate-anchor-budget.py`, spanning all four categories at
40–137 candidates, matching the oracle's exact anchor *identities* rather than
only its counts. One case emits interleaved snake/camel calls specifically to
falsify the extraction-order claim below. Worked examples, verbatim
oracle output:

| document | oracle total | kept indices |
|----------|--------------|--------------|
| 50 distinct `--flagNNN` | `50/50` | all (at the boundary, nothing is dropped) |
| 51 distinct `--flagNNN` | `12/12` | 0, 4, 8, 12, 17, 21, 25, 29, 34, 38, 42, 46 |
| 100 distinct `--flagNNN` | `12/12` | 0, 8, 16, 25, 33, 41, 50, 58, 66, 75, 83, 91 |
| 30 CliFlag + 30 FilePath | `24/24` | 12 of each — the trigger reads the *total*, the sampling is per category |
| 45 Symbol + 10 CliFlag | `22/22` | 12 Symbols sampled, all 10 CliFlags kept |

This supersedes the earlier `out.truncate(50)` implementation, which was a
guess that happened to agree with the oracle only below the cap.

**The FilePath stack is Rust/TS-specific.** In Python/Go projects almost no file
paths match, so FilePath anchors are rare and CliFlag dominates the broken set.

**The oracle reports every CliFlag broken** ("not in --help"): there is no
target `--help` to diff design flags against in an arbitrary project, so every
extracted `--flag` is reported broken — even ones written in a *Non-Goal*
section. OpenSpectra deliberately classifies these as unresolved instead; see
Deliberate divergences.

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

OpenSpectra retains this oracle-derived formula but passes only anchors
classified as broken. Unresolved anchors remain in the extracted-anchor
denominator and appear in `unresolved_anchors`; they do not increment the
Structure broken count, score, decay numerator, `total_score`, or severity
short-circuit.

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

### Unresolvable anchors are not broken (#83)

Issue #83 was motivated by a real-repository measurement: drift reported 13
broken anchors, but only one was a genuine deletion — an **8% signal rate**.
Eleven of the 13 were CliFlags and none of those was actionable. The v2.3.1
oracle counts all of the cases below as broken; maintainer-approved direction 1
deliberately reports them in the sibling `unresolved_anchors` array and excludes
them from Structure scoring:

* **CliFlag:** always unresolved with reason `no target --help`. An extracted
  flag has no owning binary, so neither third-party flags such as
  `uv --directory` nor first-party flags can be checked against the right help
  text.
* **Function not found by `git grep`:** unresolved with reason
  `not first-party`. Absence cannot distinguish a deleted project function from
  a builtin or dependency symbol such as PostgreSQL's
  `jsonb_array_length`.
* **Missing FilePath:** consult the change's `.started` baseline SHA. If the
  path existed at that commit and is now gone, it is broken with reason
  `file does not exist`; if it did not exist, it is unresolved with reason
  `forward reference`, because the change may be proposing to create it. If the
  baseline SHA is absent, malformed, or unavailable locally, OpenSpectra falls
  back to the prior broken classification rather than hiding a possible
  deletion.
* **Symbol:** unchanged and still broken when not found. The separate extraction
  divergence that #8/#51 tracked is outside #83 and has since been resolved —
  see "Solved: the Symbol narrowing filter was the anchor budget".

The JSON change is additive: `broken_anchors` is neither renamed nor removed.
Human output renders broken and unresolved references under separate headings,
keeping the actionable deletion class visually distinct. Only the Structure
inputs change; the score formula, severity bands, recommendations, and exit-code
mapping remain untouched.

The four real calibration goldens make the divergence explicit. The oracle
scores remain pinned as `0`, `3`, `7`, and `7`; their recorded broken-category
composition is respectively 0, 3, 9, and 12 CliFlags. With those CliFlags
excluded, the OpenSpectra Structure expectations are `0`, `0`, `0`, and `0`.

### FilePath anchors keep their leading path segments (#123)

The recovered FilePath regex has no left boundary, so it matches from the middle
of a longer path. The oracle turns `frontend/src/services/apiClient.ts` into the
anchor `src/services/apiClient.ts` and then reports `file does not exist` — for
a string that appears nowhere in the design that produced it. The finding is
unactionable by construction: grepping the change for the reported anchor
returns nothing.

**This is oracle-faithful, and the premise of #123 (that OpenSpectra had ported
it wrong) is refuted.** Probed against v2.3.1 on 2026-08-03, three jails, one
operation each:

| jail | on disk | oracle verdict |
|------|---------|----------------|
| design cites `frontend/src/services/apiClient.ts` | — | anchor `src/services/apiClient.ts`, broken |
| same citation | `frontend/src/services/apiClient.ts` **exists** | still broken |
| same citation | only the stripped `src/services/apiClient.ts` exists | **resolved** |

The third row is decisive: the oracle really does resolve against the truncated
path, so this is a defect in the oracle rather than in the port. The same probe
showed `heyuai/docs/yibi/e02.md` reduced to `docs/yibi/e02.md`, which additionally
makes a deliberately repo-*external* reference look like a missing repo-internal
file.

Maintainer-approved direction: diverge, because a merge gate cannot consume a
finding whose text does not exist. On the 29-change corpus in #123 every one of
the six surviving broken anchors was this false positive — a true-positive rate
of 0. Two changes, both confined to extraction:

* `(?:[\w.-]+/)*` prepended, so a monorepo sub-project path is reported **and
  resolved** verbatim. `frontend/src/services/apiClient.ts` now resolves against
  the file that actually exists, which is what removes the false positive.
* a left token-boundary check (`anchors::starts_at_path_boundary`; the `regex`
  crate has no look-behind), so `mysrc/foo.rs` yields **no** anchor rather than a
  phantom `src/foo.rs`.

Every reported FilePath anchor is therefore greppable verbatim in its design.

**Resolution is widened too — this is not a text-only divergence.**
`anchors::path_candidates` resolves a FilePath against the *union* of the path
as written and the oracle's truncated form. Without that union a project rooted
at its own sub-project regresses: a design under `frontend/` citing the
monorepo-relative `frontend/src/x.ts` is looked up at
`frontend/frontend/src/x.ts` and reported broken where the oracle resolves it
(probed: oracle `0/1`, first draft of this fix `1/1`).

The union is strictly more permissive than the oracle, so the mirror case
diverges the other way: with `frontend/src/services/apiClient.ts` present and
cited verbatim, the oracle reports it **broken** and OpenSpectra resolves it
(probed 2026-08-03, both binaries, one jail). That is the accepted trade —
#123's broken-FilePath class measured a 0/6 true-positive rate, so a common
false positive is exchanged for a rarer false negative. Because the union only
adds ways to be present, it cannot manufacture a broken anchor.

A path containing a `..` segment is dropped rather than anchored
(`anchors::escapes_project_root`). Resolution joins the anchor onto the project
root, so `../src/main.rs` would stat the root's *parent*: probed before the
guard existed, an unrelated `<outer>/src/main.rs` silently satisfied a design
under `<outer>/proj`, where the oracle reported its truncated `src/main.rs`
broken. Dropping matches how a leading `/` is already handled — an anchor that
cannot denote a path inside the project is not a checkable reference.

Observed edge cases, all measured rather than reasoned about:

| design text | anchor |
|-------------|--------|
| `https://github.com/org/repo/src/lib.rs` | none — the `//` fails the boundary check |
| `github.com/org/repo/src/lib.rs` (bare) | full string; falls back to `src/lib.rs` for resolution |
| `./src/main.rs` | kept as written (the match starts at the leading `.`); stays inside the root |
| `../src/main.rs` | none — dropped by `escapes_project_root`, else it would stat outside the project |
| `/Users/me/repo/src/main.rs` | none — the leading `/` fails the boundary check |
| `[spec](/docs/spec.md)` (root-relative markdown link) | none — same leading-`/` rejection; a deliberate false negative, noted so it is on the record |

**Ruling on repo-external references** (the open question in #123's acceptance
criteria): no special case is added for them. `heyuai/docs/yibi/e02.md` is now
reported in full, and whether it lands in `broken_anchors` or
`unresolved_anchors` is decided by the existing #83 baseline rule — unresolved
`forward reference` when the change's `.started` SHA shows the path absent at
baseline, broken otherwise. There is no reliable local signal that separates
"deliberately outside this repo" from "not created yet", and inventing one
would trade a visible false positive for an invisible false negative. A change
that means to reference a sibling repo therefore needs a `.started` baseline to
be classified as a forward reference; `change::create` writes
`.spectra/changes/<name>.started` whenever the project root is a git repo, so
this holds for changes created through `spectra` and not for hand-made ones.

## What is verified vs. uncertain

| Area | Status |
|------|--------|
| JSON schema, dimension model, `total_score` rule | ⚠️ additive `unresolved_anchors` divergence; existing fields preserved |
| CliFlag / Function extraction & resolution | ⚠️ extraction exact; resolution deliberately diverges per #83 |
| FilePath extraction | ⚠️ deliberately diverges per #123 (leading segments kept; mid-token and `..`-escaping matches dropped) |
| FilePath **resolution** | ⚠️ deliberately wider than the oracle per #123 — a path resolves if *either* the written form or the oracle truncation is present |
| Anchor budget (`ANCHOR_CAP` trigger + per-category downsampling) | ✅ exact — anchor identities match on 12 probed cases, 40–137 candidates |
| Structure score formula (category-weighted), severity bands, recommendation map | ✅ formula/mappings exact; unresolved inputs deliberately excluded |
| Time score curve + all day boundaries | ✅ exact — pinned via `scripts/calibrate-time.py` (transitions at 7/22/61; `abandoned` scores 4; future dates clamp to 0d) |
| Exit codes (0 on success regardless of severity; 1 on errors) | ✅ exact — probed across the severity space |
| `commits_since_created`, git commands | ✅ exact |
| Symbol extraction narrowing | ✅ solved — it was the anchor budget, not a Symbol rule (see below); `JSON` added to the stop-list |
| Tasks positive-case predicates | ⚠️ uncalibrated (no positive sample); detection gated off |

### Solved: the "Symbol narrowing filter" was the anchor budget (#8)
This was carried for months as the single open RE question: the recovered Symbol
regex matches every capitalised token, but the oracle kept only **12 of ~83**
prose candidates in one Chinese-prose design, and the selection resisted every
content-based explanation — in `Data Model` it kept `Data` and dropped `Model`;
in `ALTER TABLE ADD COLUMN` it kept `ADD` and dropped the rest, identical
context, different outcome. In isolation every token was kept.

There is no Symbol predicate. Every one of those observations is the
[anchor budget](#the-anchor-budget-anchor_cap--50-is-a-trigger-not-a-truncation)
seen from inside one category:

* **"12 of ~83"** — 12 is `ANCHOR_SAMPLE_PER_CATEGORY`, and ~83 candidates is
  over the cap, so the Symbol category was downsampled to exactly 12.
* **Same token, different outcome** — the survivors are chosen by *position*
  (`i * n / 12`), so whether a given token survives depends on where it sits in
  the document, not on what it is or what surrounds it.
* **"In isolation all tokens are kept"** — an isolated token is a document with
  a handful of candidates, which is under the cap, where nothing is dropped.

Confirmed by probing the same downsampling in the three categories that have
nothing to do with symbols: 51+ distinct CliFlags, FilePaths or Functions each
collapse to the same 12 at the same evenly-spaced indices. No disassembly was
needed — the earlier probes had only ever varied a token's *context*, never the
document's total candidate count, which is the variable the rule actually reads.

The one genuine Symbol finding from the same probe round: `JSON` is stop-listed
by the oracle and was missing from the recovered `.rodata` list. It was the last
remaining divergence on a fresh `design.md` scaffold (#51). The "all-caps
acronyms are dropped" theory that `ADD` suggested is **refuted** — `ALTER`,
`TABLE`, `COLUMN`, `README`, `CRITICAL`, `YAML`, `HTTP` and `SQL` are all kept
by the oracle when probed one per repo.

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

   **This is what bounds the CI gate, so state it where operators read it.**
   Probed head-to-head on 2026-08-03 with a committed design citing
   `deleted_helper_fn()` and `DeletedStructName`, neither present anywhere in the
   codebase: *both* binaries report them in neither `broken_anchors` nor
   `unresolved_anchors` — they self-resolve and vanish. Combined with #83
   (CliFlag and unresolvable Function → unresolved), the only category that can
   reach `broken_anchors` on a committed change is **FilePath**, and only when
   the path existed at the baseline. The README gate section says this outright
   rather than implying full-category coverage.

   Note the fix is not a drop-in: excluding the change dir would expose the
   Symbol anchors that a freshly scaffolded `design.md` extracts from its own
   template prose, re-opening #51 in a worse form (~20 broken anchors on an
   untouched scaffold). Any future attempt must land together with a Symbol
   filter that survives that case.
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
   **Vary the document's total candidate count too, not just each token's
   context.** The Symbol "narrowing filter" stayed open for months because every
   probe held the candidate count small while varying context — and the rule the
   oracle actually applies reads only the count. When a per-item rule refuses to
   fit, sweep the size of the collection the item sits in.
   Reproduce with `python3 scripts/calibrate-anchor-budget.py --oracle <path>`.
4. To settle **timezone questions** ("does the binary derive `today` from local
   time or UTC?"), run the oracle under a shifted `TZ` env var (e.g.
   `TZ=Etc/GMT+11`) and watch whether its reported day count moves: it moves →
   process-local time; it stays → UTC or a cached value. One run is decisive —
   this refuted the UTC theory for v2.3.1 (it is local-time, matching
   `Local::now()`).

The `scripts/` probes and the per-change golden JSON used for calibration live
alongside this doc's git history.
