# Reverse-engineering `spectra task done`

How the closed-source `spectra task done` command marks a task complete and
tracks which files it touched, and how OpenSpectra reproduces it.

> Source: `Spectra.app/Contents/MacOS/spectra` v2.3.1 (arm64 Mach-O, symbols
> retained). String mining located the CLI help text and error-message
> fragments; the exact behavior (checkbox indexing, touched-file selection,
> JSON shapes) was confirmed by running the binary as a **golden oracle** in
> scratch git repos, since none of it was covered by `drift.md`'s earlier RE
> pass.

## CLI shape

`task` is a nested subcommand family (`spectra task <COMMAND>`), of which
`done` is currently the only member:

```
spectra task done <TASK_ID> [--change <NAME>] [--json]
```

* `TASK_ID` — 1-based index across **every** checkbox in `tasks.md`, counted
  top-to-bottom in file order, ignoring any `## N.` group headers. A
  `tasks.md` with two `##` groups of two tasks each numbers its checkboxes
  1–4 regardless of the `1.1`/`1.2`/`2.1`/`2.2` labels written in the task
  text itself — those labels are just prose, not the identifier `task done`
  operates on.
* `--change` — same auto-detect semantics as `drift`/`show`/`park`: omit it
  when exactly one active change exists, otherwise it's required.

## Behavior

OpenSpectra's actual evaluation order (the oracle doesn't expose enough to
confirm its own internal order, so this is OpenSpectra's own, documented
here for accuracy rather than presented as oracle-verified):

0. Parse `TASK_ID` as an unsigned integer *before* looking at the change at
   all → `Invalid task ID '<input>': must be a number` on failure.
1. Resolve the change (auto-detect or `--change`; `--change`'s own
   auto-detect errors, e.g. "No active changes...", are reachable here same
   as `drift`/`show`/`park`). If the change doesn't exist, or exists but has
   no `tasks.md`, both report the **same** error:
   `tasks.md not found for change '<name>'` — the oracle doesn't distinguish
   the two cases.
2. Validate the remaining `TASK_ID` cases (inside `tasks::mark_done`, after
   the change/`tasks.md` load above):
   * `0` → `Task ID must be >= 1`
   * greater than the total checkbox count → `Task <id> not found (total: <n>)`
   * already `[x]` → `Task <id> is already done`
3. Flip that checkbox from `[ ]` to `[x]`, rewriting `tasks.md` with every
   other line's content preserved verbatim. (LF line endings; a CRLF
   `tasks.md` is normalized to LF as a side effect of the line-based
   rewrite — not literally byte-for-byte for that input.)
4. Best-effort record newly-dirty files to `.spectra/touched/<name>.json`:
   * "touched files" = `git status --porcelain` output (modified, staged,
     and untracked paths; a rename reports the new path)…
   * … **minus** anything under the change's own artifact directory
     (`<spec_dir>/changes/<name>/` — `tasks.md` itself is always dirty right
     after step 3, and must never show up as a "touched" file) …
   * … **minus** anything under OpenSpectra's own `.spectra/` state directory
     (see the dedicated section below) …
   * … **minus** anything already recorded against an *earlier* task in this
     change's tracking file (confirmed empirically: a file that's still
     dirty after being recorded under task 1 is *not* re-attributed to
     task 2 — attribution is first-task-wins, scoped per change).
   * If the resulting file list is empty, no tracking file is written at
     all (confirmed: marking a task done with zero unrelated dirty files
     never creates `.spectra/`).
5. Print `✓ Task <id> marked as done: <task_desc>` (human, oracle-verified)
   or `{"change","status","task_desc","task_id"}` (`--json`, alphabetical
   key order, **`task_id` rendered as a string**, matching the oracle
   exactly — not a JSON number). **OpenSpectra deliberately omits the `✓`**
   in its own human-readable output — none of `park`/`unpark`/`new change`
   use a checkmark either, and this is a conscious choice to stay
   consistent with those already-shipped commands rather than an oversight.
   The alphabetical `--json` key order is a byproduct of `serde_json`'s
   default `Map` (a `BTreeMap`, since the `preserve_order` feature isn't
   enabled) rather than something OpenSpectra sorts explicitly — pinned by
   a dedicated test so enabling that feature later doesn't silently change
   the shape without a failing test.

## JSON schemas

`spectra task done <id> --json`:

```json
{"change": "try-feature", "status": "done", "task_desc": "1.2 ...", "task_id": "2"}
```

`.spectra/touched/<name>.json` (recovered from the binary's bundled
`/spectra:commit` skill doc, which reads this file to group a commit's dirty
files by task):

```json
{
  "change": "<change-name>",
  "touched": [
    { "task_id": "1", "task_desc": "Task description", "files": ["src/file1.ts", "src/file2.ts"] }
  ]
}
```

Struct shapes match the binary's serde-derive error strings verbatim:
`struct TouchedTracking with 2 elements` (`change`, `touched`) and
`struct TouchedEntry with 3 elements` (`task_id`, `task_desc`, `files`).

## OpenSpectra's `.spectra/` exclusion (not verified against the oracle directly, but load-bearing)

OpenSpectra excludes any dirty path under `.spectra/` from the touched-files
candidate list, in addition to the change's own artifact directory. This
wasn't something the golden-oracle scratch repos needed, because the
original binary's own `spectra init` writes `.spectra/` into `.gitignore` (a
`.gitignore/.vector-search.db*.spectra/` string fragment confirms this), so
`.spectra/` is never dirty from git's perspective when the real CLI runs.
OpenSpectra doesn't implement `init` yet, so nothing gitignores `.spectra/`
for it — without this exclusion, `.spectra/touched/<name>.json` would
recursively record *itself* as a touched file on the very next `task done`
call after its own creation. Caught via manual end-to-end testing (not by
any oracle run), fixed by excluding `.spectra/` unconditionally.

## Known discrepancy: `new change`'s scaffold (affects #7b, not re-litigated here)

Running the real binary's `new change <name>` revealed it does **not**
scaffold `proposal.md`/`design.md`/`tasks.md` — only `.openspec.yaml`. Those
three files are created individually and later, via a separate
`spectra new artifact <type> --change <name>` command, with a much richer
templated `tasks.md` (task-group headers, HTML-comment placeholders) than a
static string constant could produce. `.openspec.yaml` also carries a
`created_by` field (git user identity) that OpenSpectra's `new change`
doesn't set.

OpenSpectra's already-shipped `new change` (see `change.rs::create`)
deliberately does not match this — it scaffolds all four files immediately,
matching the original GitHub issue's literal spec rather than the oracle.
`task done` is implemented against **OpenSpectra's own** `tasks.md` contract
(a flat, ungrouped checkbox list), which is self-consistent and already
covered by tests; it also handles the oracle's real (grouped/numbered)
`tasks.md` shape correctly, since `task done`'s checkbox indexing ignores
group headers entirely. Revisiting `new change`'s scaffold to match
`new artifact`'s split is a separate, larger design decision left for a
future issue — not undertaken here to avoid re-opening an already-merged,
already-mob-reviewed PR.
