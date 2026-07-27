# Reverse-engineering `spectra init`

What was measured against Spectra 2.3.1 and which OpenSpectra additions still
have no oracle equivalent.

## Verification status

The default init artifacts and human-readable result are oracle-verified.
Probes P8-P11 and P25/P27/P28 covered:

- the default file set and byte content, including final newlines
- the empty `openspec/changes/archive/` directory and absence of `.gitkeep`
- new, appended, and already-present `.gitignore` cases
- the complete `openspec/config.yaml` and `.spectra.yaml` templates
- default, explicitly-default, and non-default `spec_dir` rendering
- stdout and a successful zero exit code

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
spectra init [--adopt] [--json]
```

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

If any line already equals `.spectra/` after trimming surrounding whitespace,
the oracle leaves the entire file byte-for-byte unchanged. In particular, it
does not add an orphaned blank line or comment.

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
