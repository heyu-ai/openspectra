## Summary
The PR successfully unifies the recursive spec traversal between the `validate` and `archive` flows by extracting a shared collector `[collect_delta_specs](file:///Users/howie/Workspace/github/side-project/openspectra/.claude/worktrees/fix-39-archive-nested-specs/crates/spectra-core/src/fsutil.rs#L55)`. However, it introduces potential crash/hard-failure scenarios for specific directory structures, silently excludes non-cyclic directory symlinks, and lacks test coverage for strict UTF-8 capability name validation.

## Findings

### [Important] Silent exclusion of non-cyclic directory symlinks
- File: [crates/spectra-core/src/fsutil.rs:114](file:///Users/howie/Workspace/github/side-project/openspectra/.claude/worktrees/fix-39-archive-nested-specs/crates/spectra-core/src/fsutil.rs#L114)
- Issue: The `[is_real_dir](file:///Users/howie/Workspace/github/side-project/openspectra/.claude/worktrees/fix-39-archive-nested-specs/crates/spectra-core/src/fsutil.rs#L114)` helper uses `std::fs::symlink_metadata` to block directory symlinks and prevent cycles. While cycle-avoidance is required, this implementation unconditionally skips all directory symlinks, including non-cyclic ones pointing to shared capability paths elsewhere. These directories and their specs are silently skipped during validation and archiving without raising warnings or errors.
- Suggested fix: Track visited directories (using device/inode numbers on Unix or canonical paths) to detect cycles during the recursive descent, or log a warning to stderr when skipping a symlinked directory.

### [Important] Hard error/crash when encountering a directory named `spec.md`
- File: [crates/spectra-core/src/fsutil.rs:74](file:///Users/howie/Workspace/github/side-project/openspectra/.claude/worktrees/fix-39-archive-nested-specs/crates/spectra-core/src/fsutil.rs#L74)
- Issue: Inside `[collect_delta_specs_into](file:///Users/howie/Workspace/github/side-project/openspectra/.claude/worktrees/fix-39-archive-nested-specs/crates/spectra-core/src/fsutil.rs#L62)`, the collector unconditionally attempts to read `dir.join("spec.md")` as a file before verifying its file type. If a directory named `spec.md` exists (e.g. if the user names a capability `spec.md`), `std::fs::read_to_string` fails with a hard `EISDIR` error (which is not mapped to `ErrorKind::NotFound`), propagating the error and crashing the whole CLI execution.
- Suggested fix: Verify that `dir.join("spec.md")` exists and is a file (using metadata/symlink_metadata) before calling `[read_optional](file:///Users/howie/Workspace/github/side-project/openspectra/.claude/worktrees/fix-39-archive-nested-specs/crates/spectra-core/src/fsutil.rs#L17)`.

### [Important] Missing test coverage for strict UTF-8 capability ID validation
- File: [crates/spectra-core/src/fsutil.rs:90](file:///Users/howie/Workspace/github/side-project/openspectra/.claude/worktrees/fix-39-archive-nested-specs/crates/spectra-core/src/fsutil.rs#L90)
- Issue: The `[capability_id](file:///Users/howie/Workspace/github/side-project/openspectra/.claude/worktrees/fix-39-archive-nested-specs/crates/spectra-core/src/fsutil.rs#L90)` helper enforces strict UTF-8 paths, raising a hard error rather than fallback lossy conversion to ensure `archive` does not write to corrupted target paths. Although this is a critical design change, there are no tests verifying that invalid UTF-8 capability directory names successfully trigger this error path.
- Suggested fix: Add a test (e.g., under Unix-specific cfg) that creates a non-UTF-8 directory name under `specs/` and asserts that validation/archiving fails with the expected error.

### [Actionable NIT] Redundant sorting of applied results
- File: [crates/spectra-core/src/archive.rs:232](file:///Users/howie/Workspace/github/side-project/openspectra/.claude/worktrees/fix-39-archive-nested-specs/crates/spectra-core/src/archive.rs#L232)
- Issue: In `[apply_spec_deltas](file:///Users/howie/Workspace/github/side-project/openspectra/.claude/worktrees/fix-39-archive-nested-specs/crates/spectra-core/src/archive.rs#L207)`, the final results are sorted via `results.sort_by(|a, b| a.capability.cmp(&b.capability))`. However, the shared collector `[collect_delta_specs](file:///Users/howie/Workspace/github/side-project/openspectra/.claude/worktrees/fix-39-archive-nested-specs/crates/spectra-core/src/fsutil.rs#L55)` already yields entries sorted by capability ID, rendering the second sort redundant.
- Suggested fix: Remove the redundant `results.sort_by` sorting line.

## Verdict
- NEEDS_CHANGES
