## Summary
Solid, idiomatic PR, but the MODIFIED delta path has a real data-corruption bug (next
requirement header glued onto the modified block, then dropped by the `^### Requirement:`
regex), masked by tests that assert only `.contains(header)`. Two docs also describe
`init --adopt` behavior the code doesn't implement.

## Findings

### [Critical] MODIFIED replacement glues the next header onto the previous line
- File: crates/spectra-core/src/archive.rs:399
- Issue: `content.replace_range(block.start..block.end, modified)` — `block.end` is the byte
  offset of the *next* `### Requirement:`/`## ` header, so the replaced canonical range
  includes the trailing `\n\n`/`\n---\n` separator, but `modified` (from `block_text`) is
  `trim_end()`'d. The next header is concatenated onto the modified block's last line
  (`...(Previously: ...)### Requirement: Third`); no longer at line-start, the multiline
  `^### Requirement:` regex stops matching it, so the requirement silently disappears from
  `list`/`drift`/future `archive`. Also breaks two consecutive MODIFIEDs (second's
  `find_requirement_block` returns None → "cannot MODIFY ... does not exist"). REMOVED
  (replaces with `""`) and RENAMED (replaces only the header line) are unaffected.
- Suggested fix: preserve the original block's trailing separator, e.g. re-append the
  whitespace that `[block.start..block.end]` had after its trimmed content, or replace only
  the trimmed span. Then assert line-start headers in tests (re-parse, don't `.contains`).

### [Critical] openspec-compat.md claims plain `init` errors on existing OpenSpec content, but the code succeeds
- File: docs/openspec-compat.md:98-102 (vs init.rs `init_with_options`)
- Issue: doc says "Unlike plain `init` (which errors if the spec dirs are absent-then-created),
  `--adopt` succeeds precisely because the directories already exist" and "Plain `init` on a
  directory that already has `openspec/` content still errors". Both false: `init_with_options`
  uses idempotent `create_dir_all` in *both* modes and only refuses when `.spectra.yaml`
  exists. The only real difference is the `adopted` flag/message.
- Suggested fix: rewrite to state both modes create dirs idempotently and both only refuse when
  `.spectra.yaml` exists; `--adopt` differs only in intent/messaging + being explicitly
  non-destructive toward OpenSpec-owned files.

### [Important] MODIFIED/REMOVED/RENAMED section present but parsing to zero blocks is silently dropped
- File: crates/spectra-core/src/archive.rs (parse_requirement_delta / merge_spec_delta has_changes gate)
- Issue: a correctly-cased `## MODIFIED/REMOVED/RENAMED Requirements` header whose body yields
  no parseable `### Requirement:` block (prose-only, wrong level, non-backticked FROM/TO)
  parses to an empty vec; if it's the only section, `has_changes == 0` → `Ok((None,...))`,
  archive moves the dir and prints all-zero counts, exit 0. Strictly worse than the old loud
  `UNSUPPORTED_HEADER_RE` reject, and there's no unarchive.
- Suggested fix: when a delta section header is present but produced zero entries, error naming
  the capability + section (preserve the loud-failure guarantee).

### [Important] Docs describe content-sensitive `--adopt` spec-dir auto-detection the code doesn't do
- File: docs/reverse-engineering/init.md:74-76; docs/openspec-compat.md:91-92 (vs init.rs detect_adopt_spec_dir)
- Issue: both docs describe conditional detection ("prefer openspec if it contains
  changes/specs, else fallback"); `detect_adopt_spec_dir` discards every probe and always
  returns DEFAULT ("openspec"). The branching is fictional.
- Suggested fix: align docs with the honest init.rs comment — probes only surface I/O errors;
  `spec_dir` is always `openspec` today; contents-sensitive discovery is future work.

### [Important] Round-trip (spectra-produced spec) MODIFIED/REMOVED not tested; malformed RENAMED parser untested
- File: archive.rs tests + openspec_compat.rs
- Issue: all canonical fixtures are hand-written without `---`/`@trace` footers, so MODIFY/REMOVE
  on a *spectra-produced* spec (the feature's own output) is never exercised — exactly where the
  Critical glue bug bites. Also the 5 malformed-RENAMED error branches
  (FROM-without-TO, TO-without-FROM, missing/broken backtick, non-`### Requirement:` content)
  have zero coverage.
- Suggested fix: add a round-trip test (ADD two reqs, then MODIFY first + REMOVE last; assert
  coherent separators, no glued headers, decide @trace survival) and a table test over each
  malformed-RENAMED form.

### [Actionable NIT] MODIFIED tests assert only `.contains(header)` so they pass on corrupted output
- File: archive.rs test `archive_applies_a_modified_requirements_delta` et al.
- Fix: assert `\n### Requirement: X` (line-start) or re-parse and assert the recovered header set.

### [Actionable NIT] RENAMED bullets not in exact `- FROM:`/`- TO:` form silently ignored
- File: archive.rs parse_renamed_requirements
- Fix: error if `## RENAMED Requirements` present but zero renames parsed (covered by the
  present-but-empty fix above).

### [Actionable NIT] Normalized-whitespace matching tested only for MODIFIED; add a REMOVED/RENAMED case
### [Actionable NIT] openspec_compat.rs: assert the superseded MODIFIED text ("30 minutes") is gone, and tighten the drift assertion
### [Actionable NIT] init.rs detect_adopt_spec_dir probe/error path untested

## Verdict
- NEEDS_CHANGES
