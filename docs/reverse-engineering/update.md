# Reverse-engineering `spectra update`

How `spectra update` (update instruction files for detected AI tools) was
probed against the closed-source reference binary and ported. Unlike
`init.md`, everything below **is oracle-verified**: each rule was probed
against `Spectra.app/Contents/MacOS/spectra` 2.3.1, and the full per-tool
output trees are pinned byte-for-byte in
`golden/update-trees-2.3.1.tsv` (445 files across 23 tools).

## CLI shape

```
Update instruction files

Usage: spectra update [OPTIONS] [PATH]

Arguments:
  [PATH]  Project path (defaults to current directory)

Options:
      --force     Overwrite existing files
      --no-color  Disable colored output
```

No `--json` flag exists; output is text only.

## Path resolution (asymmetric)

- **No `[PATH]`**: walks up from the cwd to the nearest ancestor containing
  `.spectra.yaml` (same as every other command's `find_root`). Probed:
  running from `<root>/sub/inner` updates `<root>`.
- **Explicit `[PATH]`**: must be exactly the project root. Probed: passing
  `<root>/sub` fails with `Error: Not initialized. Run 'spectra init'
  first.` (exit 1, stderr) even though an initialized ancestor exists —
  the oracle does **not** walk up from an explicit path.

## Tool detection

Detection is purely filesystem-based: a tool is "configured" when its
detection path **exists** under the project root. Notes:

- `exists()`, not `is_dir()`: a plain *file* named `.claude` still triggers
  detection, after which the oracle's `create_dir_all` fails with the raw
  io error `Error: File exists (os error 17)` (exit 1). OpenSpectra
  reproduces this, deliberately not wrapping the error in context.
- Root-level instruction files do **not** trigger detection: a bare
  `CLAUDE.md`, `.cursorrules`, `.windsurfrules`, `AGENTS.md`,
  `CONVENTIONS.md`, `.goosehints`, or `.github/copilot-instructions.md`
  alone yields `! No AI tool configurations found.`.
- The `tools:` key in `.spectra.yaml` is *not* consulted (it stays
  commented-out even after `init --tools claude`); detection is re-derived
  from the filesystem on every run.

Full matrix (probed one directory at a time; also recoverable from the
binary's strings table). **Registry order matters**: the success message
lists tools in this fixed order, not detection or alphabetical order.

| # | tool id | detection path |
|---|---------|----------------|
| 1 | `claude` | `.claude` |
| 2 | `cursor` | `.cursor` |
| 3 | `windsurf` | `.windsurf` |
| 4 | `cline` | `.clinerules` (NOT `.cline`, though files are written there) |
| 5 | `gemini` | `.gemini` |
| 6 | `github-copilot` | `.github/prompts` (`.github` alone is not enough) |
| 7 | `kiro` | `.kiro` |
| 8 | `roocode` | `.roo` |
| 9 | `continue` | `.continue` |
| 10 | `opencode` | `.opencode` |
| 11 | `codebuddy` | `.codebuddy` |
| 12 | `costrict` | `.cospec` (NOT `.costrict`) |
| 13 | `antigravity` | `.agent` |
| 14 | `auggie` | `.augment` |
| 15 | `amazon-q` | `.amazonq` |
| 16 | `kilocode` | `.kilocode` |
| 17 | `factory` | `.factory` |
| 18 | `iflow` | `.iflow` |
| 19 | `qoder` | `.qoder` |
| 20 | `qwen` | `.qwen` |
| 21 | `codex` | `.agents` |
| 22 | `crush` | `.crush` |
| 23 | `trae` | `.trae` |

Probed non-triggers: `.codex`, `.junie`, `.zed`, `.goose`, `.aider`,
`.cline`, `.costrict`.

## Output and exit codes

| Situation | Stream | Text | Exit |
|---|---|---|---|
| ≥1 tool detected | stdout | `✓ Updated instruction files for: <ids, comma-joined, registry order>` | 0 |
| no tools detected | stdout | `! No AI tool configurations found. Use 'spectra init --tools' to set up.` | 0 |
| not initialized | stderr | `Error: Not initialized. Run 'spectra init' first.` | 1 |

Color (pty capture): only the leading symbol is wrapped — `✓` in green
(`\x1b[32m✓\x1b[0m`), `!` in yellow (`\x1b[33m!\x1b[0m`). The error line is
never colored. Piped output is uncolored (TTY detection).

## Write semantics

For every detected tool, `update` **unconditionally rewrites its whole
file set on every run** (probed via mtime: all managed files get a fresh
mtime even when content is unchanged; content itself is idempotent).
Missing files are recreated; files the tool set doesn't own (e.g. a user's
own `.claude/skills/my-own/`) are never touched or deleted.

Three per-file strategies exist:

1. **Plain** (skills, commands, workflows, prompts): full overwrite.
2. **Managed marker files** (root-level `CLAUDE.md`, `GEMINI.md`,
   `QWEN.md`, `CLINE.md`, `CODEBUDDY.md`, `COSTRICT.md`, `IFLOW.md`,
   `QODER.md`, `AGENTS.md`, `.cursorrules`, `.windsurfrules`): only the
   `<!-- SPECTRA:START v1.0.2 -->` … `<!-- SPECTRA:END -->` block is
   managed. Probed algorithm:
   - *File missing or empty*: write just the block.
   - *Valid pair* (a line starting with `<!-- SPECTRA:START` followed
     later by a `<!-- SPECTRA:END -->` line): replace those lines in
     place; content before START and after END survives. The START match
     is prefix-based, so an old version suffix (e.g. `v0.9.0`) is still
     found and upgraded.
   - *START but no END after it*: original content is left untouched and a
     fresh complete block is **appended** at EOF after a blank line.
   - *No START* (even if an orphan END exists): fresh block is
     **prepended**, followed by a blank line and the original content.
3. **`.claude/settings.json`** (claude only): JSON object merge. Managed
   keys are forced to managed values (`includeGitInstructions: false`
   even if the user set `true`), unknown user keys survive, keys end up
   alphabetically sorted (the oracle exhibits exactly serde_json's default
   BTreeMap behavior), 2-space indent, **no trailing newline**. Invalid
   JSON or a non-object (`[1,2]`) is silently replaced with the default
   template.

## `{{SPEC_DIR}}` substitution

Templates embed the project's `spec_dir`: with `init --dir docs/specs`,
written files read `docs/specs/changes/` etc. Captured via a two-sandbox
diff (default `openspec` vs a unique token spec-dir) so substitution sites
are unambiguous — a plain search for "openspec" would false-match the
"OpenSpec" brand name.

**Oracle bug preserved**: the cursor `spectra-ask.md` frontmatter contains
a literal, never-substituted `{{SPEC_DIR}}documents` in the oracle's own
output (both sandboxes byte-identical there). OpenSpectra's templates
escape such literals as `{{RAW_SPEC_DIR}}` at capture time and restore
them at render time, so the bug survives byte-for-byte.

## The codex × gemini suppression quirk

Pairwise-probed: when **gemini** is also detected, **codex** writes only
`AGENTS.md` — its whole `.agents/skills/*` set is silently skipped. No
other tool pair behaves this way; notably `antigravity` (which also writes
`GEMINI.md`) does *not* suppress codex, so the trigger is the gemini tool
(equivalently `.gemini` existing), not the `GEMINI.md` file. Since
detection *is* existence, "gemini detected" and "`.gemini` exists" are
behaviorally indistinguishable; OpenSpectra implements the former.
`scripts/capture-update-templates.py` re-verifies this quirk on every
capture.

## `--force` has no observable effect

Probed with and without `--force` across: modified plain files (both
restore), modified managed blocks (both restore the block, both preserve
outside content), settings.json user keys (both merge identically), and
full-tree diffs. The flag is accepted for CLI parity but changes nothing
in 2.3.1 — plausibly vestigial sharing of `init`'s flag set. OpenSpectra
accepts and ignores it the same way.

## What update does NOT touch

`.spectra.yaml`, `.gitignore`, `<spec_dir>/config.yaml`, canonical specs,
changes — all byte-identical across update runs (hash-compared).

## Reproducing the oracle

`scripts/capture-update-templates.py` (macOS-only, needs the reference
binary) regenerates:

- `crates/spectra-core/assets/update/` — 170 deduped template blobs
  (2.3 MB; 445 tool-files share content heavily, e.g. one 10-skill set is
  shared verbatim by cursor/windsurf/qwen/gemini/…),
- `crates/spectra-core/src/update_manifest.rs` — the generated registry
  (tool → detection path → file specs), and
- `golden/update-trees-2.3.1.tsv` — sha256 of every oracle output file
  with the default spec_dir, which CI's
  `every_tool_tree_matches_the_oracle_golden_byte_for_byte` integration
  test verifies without needing the oracle.

The script is a verification contract, not a printer: it round-trips every
template (token capture → `{{SPEC_DIR}}` → re-resolve → must equal the
default capture byte-for-byte), pins each tool's stdout, re-verifies the
registry order and the codex×gemini quirk, and exits non-zero keeping the
sandboxes on any mismatch. The round-trip check caught the
`{{SPEC_DIR}}documents` oracle bug on its first run.

## Open questions

- Whether any oracle config (`.spectra.yaml` `tools:` uncommented) alters
  detection — not probed; the key ships commented-out and `update`
  demonstrably re-detects from the filesystem.
- Behavior on markers with trailing same-line content (e.g. text after
  `<!-- SPECTRA:END -->` on the same line) — unprobed edge; OpenSpectra
  treats the marker line as ending at its newline.
- Why gemini suppresses codex's skills (intentional dedup for gemini-cli's
  `.agents` support, or a tool-definition bug) — behavior is pinned
  either way.
