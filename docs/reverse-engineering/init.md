# Reverse-engineering `spectra init`

What was measured against Spectra 2.3.1 and which OpenSpectra additions still
have no oracle equivalent.

## Verification status

The default init artifacts, human-readable result, and `--tools` behavior are
oracle-verified. Probes P8-P11 and P25/P27/P28 (the issue #94 probe session;
reproduction recipe below under "Reproducing the oracle") covered:

- the default file set and byte content, including final newlines
- the empty `openspec/changes/archive/` directory and absence of `.gitkeep`
- new, appended, and already-present `.gitignore` cases
- the complete `openspec/config.yaml` and `.spectra.yaml` templates
- default, explicitly-default, and non-default `spec_dir` rendering
- stdout and a successful zero exit code

The `--tools` matrix and probes P29-P33 additionally cover:

- single, comma-separated, and repeated tool selection
- unknown and space-separated values as successful tool-file no-ops
- mixed valid and unknown ids
- user-input message ordering
- reuse of update's write strategies and `spec_dir` rendering
- the `claude_slash_commands` boundary owned by issue #92

The reference CLI's `--dir` probes showed that `config.yaml` follows the
resolved spec directory. They also showed that a non-default directory replaces
line 6 of `.spectra.yaml` in place, while default `openspec` leaves the example
commented. OpenSpectra does not yet expose the reference CLI's `--dir` flag, so
its non-default renderer is covered in a unit test but is not reachable through
the current CLI.

The following remain **unverified** against the oracle:

- already-initialized and filesystem-error wording and side effects
- operation ordering and interrupted-init retry behavior
- CRLF preservation when appending to `.gitignore`
- file modes and umask behavior
- `--adopt` and `--json`, which are OpenSpectra extensions
- preservation of a pre-existing `<spec_dir>/config.yaml`

The last item is intentionally retained as OpenSpectra's non-destructive
adoption policy; the empty-project oracle probe cannot establish overwrite
behavior for an existing OpenSpec project.

## CLI shape

```text
spectra init [--adopt] [--json] [--tools <TOOLS>]
```

The reference CLI additionally accepts `[PATH]`, `--force`, and `--dir`; those
surfaces remain outside OpenSpectra's current CLI. `--adopt` and `--json` are
OpenSpectra extensions.

`init` is the only command that does **not** require
`Config::is_initialized` to already be true. Every other subcommand calls
`require_initialized` first and fails with `Not initialized. Run 'spectra
init' first.` if `.spectra.yaml` is missing.

For a plain default init, the verified human-readable output is one line:

```text
✓ Initialized at <absolute-project-root>/openspec
```

The process exits 0 and writes nothing to stderr. `--adopt` retains its
OpenSpectra-specific `Adopted …` message because the reference CLI has no such
flag.

## Oracle-verified `--tools`

`--tools` is a comma-delimited, repeatable option. The selected ids are echoed
in user-input order, with comma-separated values normalized to `, `:

```text
✓ Initialized at <absolute-project-root>/openspec
Generated files for: claude, cursor
```

The process exits 0 and writes nothing to stderr. Selection is deliberately
permissive:

| Input | Tool-file result | Second stdout line |
|---|---|---|
| `--tools claude` | Claude's 14 files | `Generated files for: claude` |
| `--tools claude,cursor` | 35 files | `Generated files for: claude, cursor` |
| `--tools claude --tools cursor` | same 35 files | `Generated files for: claude, cursor` |
| `--tools "claude cursor"` | no tool files | `Generated files for: claude cursor` |
| `--tools definitely-not-a-tool` | no tool files | `Generated files for: definitely-not-a-tool` |
| `--tools claude,bogus` | Claude's 14 files; unknown id ignored | `Generated files for: claude, bogus` |

The message is not a report of recognized tools: it echoes every parsed value.
This is why the unknown and space-separated cases still print a success line.
Tool ids are not sorted into update's registry order; P32's deliberately
reversed `cursor,claude` input remained `cursor, claude`.

For every recognized id, the output bytes and paths are the same as detecting
that tool with `spectra update`. The original single-Claude comparison was
14/14 byte-identical. Init therefore uses update's generated registry,
`Plain`/`Managed`/`ClaudeSettings` strategies, symlink stance, bare I/O errors,
`{{SPEC_DIR}}` renderer, and codex-with-gemini suppression rather than owning a
second template or writer. The existing update golden TSV is the byte-level
contract for both commands.

The `tools:` key in `.spectra.yaml` remains commented after init. `--tools`
selects files for that invocation directly; it does not persist the selection.

### P29-P33 probe records

All probes used
`/Applications/Spectra.app/Contents/MacOS/spectra` 2.3.1, inherited the normal
process environment without overrides, and passed `--no-color`. Each row used
a fresh `mktemp -d` jail, exactly one oracle invocation, then read-only
inspection. All five commands exited 0 with empty stderr.

| Probe | Cwd and exact command | Setup before the one operation | Observed stdout and state |
|---|---|---|---|
| P29 `--force` with existing files | cwd `/tmp/spectra-init89-p1-force.WNYZyJ`; `/Applications/Spectra.app/Contents/MacOS/spectra init --no-color --tools claude --force` | Seeded `CLAUDE.md` with `USER BEFORE`, an old complete managed block, and `USER AFTER`; seeded `.claude/settings.json` with `userKey` and `includeGitInstructions: true`; seeded the drift skill with a plain sentinel. | stdout was `✓ Initialized at /private/tmp/spectra-init89-p1-force.WNYZyJ/openspec\nGenerated files for: claude\n`. The managed block was upgraded while both user lines survived; settings became sorted pretty JSON with `includeGitInstructions: false` and `userKey: "keep-me"`; the plain skill was replaced. The final tree contained all 14 Claude files. Thus init `--force` uses update's three write strategies rather than forcing every file through full overwrite. |
| P30 `--dir` substitution | cwd `/tmp/spectra-init89-p2-dir.lXQv1r`; `/Applications/Spectra.app/Contents/MacOS/spectra init --no-color --tools cursor --dir docs/myspecs` | Empty jail. | stdout was `✓ Initialized at /private/tmp/spectra-init89-p2-dir.lXQv1r/docs/myspecs\nGenerated files for: cursor\n`. `.spectra.yaml` line 6 was `spec_dir: docs/myspecs`; `config.yaml` was under `docs/myspecs/`; generated rules, skills, and commands used `docs/myspecs`. The known update quirk remained: `.cursor/commands/spectra-ask.md` retained literal `description: Query {{SPEC_DIR}}documents and answer questions`, while ordinary placeholders were substituted. |
| P31 mixed unknown and valid ids | cwd `/tmp/spectra-init89-p3-mixed.rSo0si`; `/Applications/Spectra.app/Contents/MacOS/spectra init --no-color --tools claude,bogus` | Empty jail. | stdout was `✓ Initialized at /private/tmp/spectra-init89-p3-mixed.rSo0si/openspec\nGenerated files for: claude, bogus\n`. Exactly the 14 Claude tool files were written; `bogus` wrote nothing and did not block the valid id. Including the three default init files, the tree had 17 files. |
| P32 message ordering | cwd `/tmp/spectra-init89-p4-order.9q4J5l`; `/Applications/Spectra.app/Contents/MacOS/spectra init --no-color --tools cursor,claude` | Empty jail. | stdout was `✓ Initialized at /private/tmp/spectra-init89-p4-order.9q4J5l/openspec\nGenerated files for: cursor, claude\n`. Both valid sets were written (35 tool files; 38 including default init files). The message follows user input, not registry order. |
| P33 issue #92 boundary | cwd `/tmp/spectra-init89-p5-slash.iLfFDM`; `/Applications/Spectra.app/Contents/MacOS/spectra init --no-color --tools claude --force` | Seeded `.spectra.yaml` with exactly `claude_slash_commands: true\n`; `--force` was necessary because the config already marked the jail initialized. | stdout was `✓ Initialized at /private/tmp/spectra-init89-p5-slash.iLfFDM/openspec\nGenerated files for: claude\n`. The pre-existing config remained byte-identical and all 10 `.claude/commands/spectra/{apply,archive,ask,audit,commit,debug,discuss,drift,ingest,propose}.md` files were written in addition to the default Claude set. The gate belongs to issue #92; this branch routes init through the shared update writer so that gate composes after rebase. |

## Verified default artifacts

Plain `spectra init` creates:

| Path | Byte content or state |
|---|---|
| `openspec/changes/archive/` | empty directory; no `.gitkeep` |
| `openspec/specs/` | empty directory |
| `openspec/config.yaml` | template below, ending in `\n` |
| `.gitignore` | `# Spectra app data\n.spectra/\n` |
| `.spectra.yaml` | template below, ending in `\n` |

`openspec/changes/` is the parent of `archive/`; no other file is created
inside it.

### `openspec/config.yaml`

```yaml
schema: spec-driven

# Project context (optional)
# This is shown to AI when creating artifacts.
# Add your tech stack, conventions, style guides, domain knowledge, etc.
# Example:
#   context: |
#     Tech stack: TypeScript, React, Node.js
#     We use conventional commits
#     Domain: e-commerce platform

# Per-artifact rules (optional)
# Add custom rules for specific artifacts.
# Example:
#   rules:
#     proposal:
#       - Keep proposals under 500 words
#       - Always include a "Non-goals" section
#     tasks:
#       - Break tasks into chunks of max 2 hours
```

### `.spectra.yaml`

```yaml
# Spectra application config
# See: https://github.com/spectra-app/spectra

# OpenSpec directory path (relative to project root)
# Changing this requires rebuilding the vector search index.
# spec_dir: docs/specs

# Language for AI-generated artifacts
# locale: tw

# Workflow toggles
# tdd: true
# audit: true
# parallel_tasks: true

# Claude slash commands (set true to also generate /spectra:X commands)
# claude_slash_commands: true

# Enable git worktree support for isolated change branches
# worktree: true

# Custom git worktrees directory
# worktrees_dir: .spectra/worktrees

# Claude Code skill effort levels (low/medium/high/xhigh/max)
# claude_effort:
#   apply: high

# AI tools to generate instruction files for
# tools:
#   - claude
#   - cursor
```

When the resolved directory is the default `openspec`, line 6 remains
`# spec_dir: docs/specs`. Explicitly selecting `openspec` in the reference CLI
is byte-identical to omitting `--dir`. For a non-default value such as
`docs/myspecs`, only line 6 changes:

```yaml
spec_dir: docs/myspecs
```

The line is replaced, not appended, and `config.yaml` is written to
`docs/myspecs/config.yaml`.

## `.gitignore`

The verified new-file content is:

```gitignore
# Spectra app data
.spectra/
```

For an existing non-empty file, init inserts one blank line and then the same
two-line block. P9 started with `node_modules/\n*.log\n` and observed:

```gitignore
node_modules/
*.log

# Spectra app data
.spectra/
```

When the file already contains an exact `.spectra/` line, the oracle leaves
the entire file byte-for-byte unchanged — in particular it does not add an
orphaned blank line or comment. That is the probed case. OpenSpectra's
predicate additionally treats a whitespace-padded line (`.spectra/ `) as
already present; the padded variant is tested locally
(`init_does_not_duplicate_an_entry_with_trailing_whitespace`) but is an
OpenSpectra design choice, **oracle-unverified**.

OpenSpectra additionally preserves an existing CRLF line-ending style. That
compatibility behavior is tested locally but remains oracle-unverified.

## OpenSpectra extension: `--adopt`

`spectra init --adopt` is an OpenSpectra compatibility addition for existing
OpenSpec projects, documented in `docs/openspec-compat.md`; it is not an
oracle-parity claim.

Adopt mode keeps the already-initialized refusal. Its current resolver always
uses the default `openspec` directory and does not inspect other directories;
configurable discovery is future work. If `openspec` exists as a file rather
than a directory, adoption fails with `cannot adopt: … exists but is not a
directory`.

Adoption creates missing scaffold directories, including `changes/archive/`,
and creates the verified `config.yaml` template only when that file is absent.
It never overwrites existing `project.md`, `AGENTS.md`, `config.yaml`, change
content, or canonical specs. It ensures the `.spectra/` ignore block and writes
`.spectra.yaml` last.

## OpenSpectra extension: `--json`

The JSON shape remains:

```json
{
  "root": "<absolute path>",
  "spec_dir": "openspec",
  "adopted": false,
  "gitignore_updated": true
}
```

`gitignore_updated` records whether init wrote `.gitignore`; the human-readable
oracle-compatible path no longer prints a second `.gitignore` status line.

## Reproducing the oracle

The probe IDs above (P8-P11, P25/P27/P28) come from the issue #94 probe
session; each probe is one fresh jail and one operation, following CLAUDE.md's
probe discipline. The recipe, re-runnable on any macOS machine with the
reference binary:

```sh
# one probe = one fresh jail, one operation, then inspect
JAIL=$(mktemp -d /tmp/probe-oracle-init.XXXXXX)
git -C "$JAIL" init -q
/Applications/Spectra.app/Contents/MacOS/spectra init "$JAIL"

# inspect: tree, byte counts, trailing bytes
find "$JAIL" -not -path "$JAIL/.git" -not -path "$JAIL/.git/*" | sort
# expect exactly 8 lines: the jail root, .gitignore, .spectra.yaml, openspec,
# openspec/changes, openspec/changes/archive, openspec/config.yaml,
# openspec/specs
wc -c "$JAIL/.spectra.yaml"        # 761 bytes
wc -l "$JAIL/.spectra.yaml"        # 32 newline-terminated lines
tail -c 24 "$JAIL/.spectra.yaml" | xxd   # ends "#   - cursor\n", no blank line

# byte-parity check against a clean openspectra build (separate jail;
# openspectra has no [PATH] arg -- run from inside the jail)
JAIL2=$(mktemp -d /tmp/probe-openspectra-init.XXXXXX)
git -C "$JAIL2" init -q
(cd "$JAIL2" && /path/to/openspectra/target/release/spectra init)
diff -r --exclude=.git "$JAIL" "$JAIL2"   # empty output = byte parity
```

Seeded `.gitignore` variants (P9-P11) pre-write the file into the jail before
the single init operation. Two counting pitfalls recorded from the PR #101
review: `find -not -path '*/.git*'` also filters `.gitignore` (glob prefix
collision) — exclude the `.git` directory explicitly as above; and the template
is 32 `\n`-terminated lines — an editor's 33rd empty display line after the
final `\n` is not a file line (`wc -l` is authoritative).

Re-verified 2026-07-27 against Spectra 2.3.1 (Apple Silicon): full-tree
`diff -r` between the oracle jail and an openspectra jail is empty —
byte-identical, including `.gitignore`, `.spectra.yaml`, and
`openspec/config.yaml`.
