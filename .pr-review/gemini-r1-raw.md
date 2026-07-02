## Summary
This PR successfully introduces OpenSpec compatibility to Spectra, implementing `init --adopt` and support for MODIFIED, REMOVED, and RENAMED deltas. However, a critical formatting bug in the merge logic strips trailing newlines and whitespace during block replacement, leading to layout corruption in the canonical spec files.

## Findings

### [Critical] Formatting corruption in MODIFIED requirement block replacement
- File: `crates/spectra-core/src/archive.rs:372`
- Issue: When replacing a modified block in the canonical spec, the code calls `content.replace_range(block.start..block.end, modified)`. Because the `modified` text has been trimmed of trailing newlines (via `block_text` calling `trim_end()`), replacing the range `block.start..block.end` (which spans up to the next block's header) strips the blank lines and trailing newlines separating the blocks. This results in the subsequent block/section header being merged directly onto the same line as the modified block's last line (e.g. `...behavior applies### Requirement: Third`), corrupting the markdown spec file.
- Suggested fix: Extract the trailing whitespace of the original requirement block in the canonical spec, and append it to the trimmed replacement content before calling `replace_range`:
  ```rust
  let original_block = &content[block.start..block.end];
  let trailing_whitespace = &original_block[original_block.trim_end().len()..];
  let replacement = format!("{}{}", modified.trim_end(), trailing_whitespace);
  content.replace_range(block.start..block.end, &replacement);
  ```

### [Important] Fragile bullet-point parser for RENAMED section
- File: `crates/spectra-core/src/archive.rs:440`
- Issue: `parse_renamed_requirements` strictly checks for `- FROM:` and `- TO:`. If a user or standard markdown auto-formatter uses `* FROM:` or `* TO:` bullets instead, the rename directives will be silently ignored. This leads to rename validation failures or confusing "requirement does not exist" errors downstream when processing subsequent modifications.
- Suggested fix: Support both `-` and `*` prefixes for list bullets:
  ```rust
  if let Some(raw) = trimmed.strip_prefix("- FROM:").or_else(|| trimmed.strip_prefix("* FROM:")) {
      // ...
  } else if let Some(raw) = trimmed.strip_prefix("- TO:").or_else(|| trimmed.strip_prefix("* TO:")) {
      // ...
  }
  ```

### [Actionable NIT] Incomplete directory validation in detect_adopt_spec_dir
- File: `crates/spectra-core/src/init.rs:83-93`
- Issue: `detect_adopt_spec_dir` runs `is_dir` checks on `openspec/`, `changes/`, and `specs/`, but ignores their returned boolean values. If any of these paths exist as files rather than directories, the checks return `Ok(false)` without raising an error, causing a generic I/O error later during `create_dir_all`.
- Suggested fix: Explicitly validate the directory status and return a descriptive error if the path exists but is not a directory:
  ```rust
  if candidate.exists() && !is_dir(&candidate)? {
      anyhow::bail!("Adoption path '{}' exists but is not a directory", candidate.display());
  }
  ```

## Verdict
- NEEDS_CHANGES
