# Final Aggregated Review — PR #32

## Mode
group-review (3/3 voices active: Claude 4-subagent, Codex, Gemini/agy)

## Consensus Critical (must fix)
1. **MODIFIED replacement glues the next requirement header onto the modified block.**
   `archive.rs:399` — `replace_range(block.start..block.end, modified)` where `block.end`
   includes the trailing separator but `modified` is `trim_end()`'d. The next
   `### Requirement:` is concatenated onto the modified block's last line; no longer at
   line-start, the `^### Requirement:` regex drops it → the requirement silently disappears,
   and two consecutive MODIFIEDs error "cannot MODIFY ... does not exist".
   **Flagged independently by Claude (code-reviewer) AND Gemini; confirmed by lead code-read.
   Codex missed it (LGTM).** REMOVED/RENAMED unaffected.
   Fix: re-append the original block's trailing whitespace to the trimmed replacement.

## Consensus / code-confirmed Important (must fix)
2. **Doc contradiction (Critical-graded by Claude, downgraded to Important — docs only, no
   runtime impact):** `openspec-compat.md:98-102` claims plain `init` errors on existing
   OpenSpec content; the code succeeds (idempotent `create_dir_all`, only refuses when
   `.spectra.yaml` exists). Fix the doc.
3. **Present-but-empty recognized delta section silently dropped** (Claude
   silent-failure-hunter; overlaps Gemini/Codex "fragile RENAMED"). A correctly-cased
   `## MODIFIED/REMOVED/RENAMED Requirements` header that parses to zero entries becomes a
   silent no-op + archive move + exit 0 — strictly worse than the loud reject it replaced.
   Fix: error when a section header is present but produced zero entries.
4. **Fictional `--adopt` auto-detection in docs** (Claude comment-analyzer):
   `init.md:74-76` + `openspec-compat.md:91-92` describe content-sensitive spec-dir detection;
   `detect_adopt_spec_dir` always returns `openspec`. Fix docs to match; make the probe error
   cleanly if `openspec` exists as a non-dir (Codex NIT).
5. **RENAMED bullet fragility** (Gemini + Codex): accept `* FROM:`/`* TO:` as well as `-`.
   (Subsumed by #3's loud-failure fix, but adding `*` support is the robust choice.)
6. **Test gaps** (Claude pr-test-analyzer): no round-trip test on a spectra-produced spec
   (with `---`/`@trace`) — exactly where the Critical bites; malformed-RENAMED branches
   untested. Add both (the round-trip test is the Critical's regression test).

## Actionable NIT (must fix — convention: clean up all NITs)
- MODIFIED tests assert only `.contains(header)` → assert line-start `\n### Requirement:`.
- Normalized-whitespace matching tested only for MODIFIED → add a REMOVED/RENAMED case.
- `openspec_compat.rs`: assert the superseded MODIFIED text ("30 minutes") is gone.
- `detect_adopt_spec_dir` probe/non-dir path untested → cover it.

## Disputed (user decides)
- None. The Critical is code-confirmed consensus (Claude + Gemini), not contested. Codex's
  LGTM is a miss, not a DISAGREE.

## Voices unavailable
- None.

## Note on process
R2 cross-debate skipped: the sole Critical is code-confirmed consensus (not a dispute), and
the Important/NIT items are additive and code-verified. The meaningful cross-model check is the
Step 7 re-review of the FIX, which runs all three voices again.
