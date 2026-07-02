## Gemini R1

**Verdict**: NEEDS_CHANGES
**Summary**: Delivers OpenSpec compat, but a serious formatting bug in the merge logic corrupts
the canonical spec layout.

### [critical] Block formatting corruption (MODIFIED)
- File: crates/spectra-core/src/archive.rs:372-399
- Issue: `content.replace_range(block.start..block.end, modified)` — `modified` is trim_end()'d,
  so replacing removes the blank line / trailing newline between blocks and glues the next
  block's header onto the modified block's last line, corrupting the markdown spec.
- Fix: capture the original block's trailing whitespace and re-append it to the trimmed
  replacement before `replace_range`. (Same root cause Claude flagged.)

### [important] Fragile RENAMED parser
- File: crates/spectra-core/src/archive.rs:440
- Issue: `parse_renamed_requirements` only accepts `- FROM:` / `- TO:`. A `* FROM:` / `* TO:`
  bullet (user or markdown auto-formatter) is silently ignored → rename validation failure or
  a silent no-op downstream.
- Fix: accept both `-` and `*` list bullets.

### [actionable_nit] Incomplete directory validation
- File: crates/spectra-core/src/init.rs:83-93
- Issue: `detect_adopt_spec_dir` runs `is_dir` probes but discards the bool; if `openspec`
  exists as a file, the probe returns Ok(false) without erroring, then `create_dir_all` fails
  with a generic I/O error.
- Fix: if the path exists but isn't a directory, return a descriptive error.
