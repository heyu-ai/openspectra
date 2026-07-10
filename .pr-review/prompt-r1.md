You are a senior code reviewer. Review the following PR diff independently.

Output ONLY the review in the format below. Do not narrate your actions or write any
preamble (no "I will...", "Let me...", "I'm going to...", "I have written..."), and do
not announce file reads. Your first line must be the "## Summary" heading.

Base branch: main
PR #: 41
Diff: see /Users/howie/Workspace/github/side-project/openspectra/.claude/worktrees/fix-39-archive-nested-specs/.pr-review/diff.patch
Changed files: see /Users/howie/Workspace/github/side-project/openspectra/.claude/worktrees/fix-39-archive-nested-specs/.pr-review/changed-files.txt

Context: this is a Rust reimplementation of a closed-source `spectra` CLI (spec-driven-development
tooling). The PR fixes issue #39: `spectra archive` iterated only the immediate children of a
change's `specs/` dir, so a nested-capability delta (`specs/<Epic>/<Feature>/spec.md`) that
`spectra validate` accepts was silently ignored during archive. The fix lifts a recursive
`collect_delta_specs` walk into `fsutil.rs` and has both `archive` and `validate` call it, so
their traversal (and its symlink-cycle safety) cannot drift. The shared collector unifies on
strict-UTF-8 capability ids (error, not lossy) because `archive` turns the id into a write target.

Output format (strictly follow, for downstream aggregation):

## Summary
<1-2 sentence overall assessment>

## Findings

### [Critical] <short title>
- File: <path:line>
- Issue: <description>
- Suggested fix: <how to fix>

### [Important] <short title>
...

### [Actionable NIT] <short title>
- Must be a concrete, actionable small fix (naming, comment error, import order, etc.), not subjective preference

## Verdict
- LGTM / NEEDS_CHANGES

Severity (RFC 2119 — grade by merge consequence, not by how bad it feels):
- [Critical] = MUST fix, blocks merge: logic / functional error, security hole, secret or PII in logs, data loss, explicit baseline violation
- [Important] = SHOULD fix (defer only with a documented reason): test gap on a changed critical path, silent failure / swallowed exception, naming / structure inconsistency, misleading doc or comment
- [Actionable NIT] = MAY fix: a concrete, actionable small fix (naming, comment typo, import order) — never a subjective preference

Focus on:
- Logic errors, race conditions, security holes, silent failures, resource leaks
- Behavior changes vs the removed single-level walk (e.g. symlink-following, capability-id derivation, empty/edge capability ids)
- Test coverage gaps (critical paths not tested)
- Documentation / comment inconsistency with implementation
- Do NOT list "code style preferences" or "subjective aesthetics" — non-actionable items only
- Be skeptical, be terse, no compliments
