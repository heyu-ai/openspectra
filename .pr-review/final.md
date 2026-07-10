# Final Aggregated Review — PR #41 (fix #39: archive nested-capability specs)

## Mode
group-review (3/3 voices active: Claude 4-subagent / Codex / agy-Gemini)

Per-voice R1 verdicts: **Claude** NEEDS_CHANGES · **Codex** LGTM · **Gemini** NEEDS_CHANGES.

## Consensus Important (must fix)

1. **Symlinked capability directory is silently skipped** — `fsutil.rs` `is_real_dir` /
   `collect_delta_specs_into`. Flagged by 5/6 voices (Claude code-reviewer, silent-failure-hunter,
   comment-analyzer, pr-test-analyzer, Gemini). The switch from `fs::metadata` (old archive
   `is_dir`, follows symlinks) to `symlink_metadata` (no-follow) is correct for cycle-safety, but a
   capability whose dir is a directory *symlink* (`specs/Invoices -> ../shared/Invoices`) was
   applied by old archive and is now dropped with **no error and no warning** — the same silent-drop
   class #39 set out to kill, via a new door.
   **Resolution**: make the drop *loud* — emit a stderr warning when a symlinked directory is
   skipped during descent, and document it. Keep skipping (do **not** follow) for cycle-safety;
   whether to *support* symlinked capability dirs (follow with visited-set cycle tracking) is a
   larger semantic decision deferred to the human, noted in the PR.

2. **Stray root-level `specs/spec.md` → empty capability id → malformed canonical write** —
   `fsutil.rs` `capability_id`, `archive.rs` `apply_spec_deltas`. Flagged by 4 voices (Claude
   code-reviewer Important, silent-failure-hunter NIT, pr-test-analyzer NIT, lead). A `spec.md`
   directly under `specs/` yields `capability == ""`; archive writes `<spec_dir>/specs/spec.md` with
   a `#  Specification` header (doubled space) and reports a blank `Specs applied:` name. Old archive
   skipped it. `validate` already special-cases the empty id in `spec_rel_path` — archive has no
   matching guard.
   **Resolution**: hard-error in the shared collector when a `spec.md` sits directly under
   `specs/` (a delta must live under a named capability dir), symmetric fail-loud for both commands.

## Consensus Important (test gaps — must fix)

3. **No `fsutil` unit tests; mixed flat+nested layout, sort order, and empty-cap all unpinned**
   (pr-test-analyzer Important + NITs, Gemini NIT). Add a `#[cfg(test)] mod tests` to `fsutil.rs`
   covering: a parent-and-child both with `spec.md` (2 capabilities), deterministic sort order, the
   new empty-cap error, and the symlinked-dir skip+warn.

## Actionable NIT (must fix)

4. **Redundant `results.sort_by` in `apply_spec_deltas`** (Gemini) — the shared collector already
   yields sorted entries. Remove it, once the collector sort is pinned by a unit test (finding 3).

5. **Inconsistent symlink-cycle example in `fsutil.rs` doc comments** (comment-analyzer) —
   `specs/loop -> .` (line ~107) vs `specs/loop -> specs` (line ~45); the test uses `-> specs`.
   Standardize on `specs/loop -> specs`.

## Refuted (checked, dropped)

- **Gemini: "missing test for UTF-8 capability validation"** — REFUTED by Claude pr-test-analyzer
  (which read the full test file, not just the diff): `archive_errors_on_a_non_utf8_capability_directory_name`
  (archive.rs) already exercises `capability_id`'s error branch. No gap. Cross-review win.

## Disputed / deferred (pre-existing, out of scope — noted in PR, not fixed here)

- **Gemini: "crash when a directory named `spec.md` exists" (EISDIR)** — pre-existing (old
  `validate`/`archive` did the same `read_optional(dir/spec.md)`); it fails *loud* with context
  ("Is a directory"), not a silent crash. A directory literally named `spec.md` is malformed. Not a
  regression of this PR; left as-is (fail-loud is acceptable).
- **silent-failure-hunter: relative-symlink `spec.md` validates clean pre-move then dangles
  post-move** — real but pre-existing: archive's pre-move-validate / post-move-apply split predates
  this PR, and it requires `spec.md` itself to be a relative symlink crossing the moved dir
  (exotic). Not introduced here; deferred.

## Voices unavailable
- None.
