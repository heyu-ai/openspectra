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
| unwritable target (e.g. read-only parent dir) | stderr | `Error: Permission denied (os error 13)` — **bare**, no path | 1 |

stderr is empty on every successful run. The unwritable-target row is why the
`update` write path deliberately does *not* attach path context to its I/O
errors, unlike the rest of `spectra-core`: here the oracle's exact string is the
contract (AC-2), and the crate-wide "attach the path" convention applies to
commands with no oracle string to match.

Color (pty capture): only the leading symbol is wrapped — `✓` in green
(`\x1b[32m✓\x1b[0m`), `!` in yellow (`\x1b[33m!\x1b[0m`). The error line is
never colored. Piped output is uncolored (TTY detection).

## Write semantics

For every detected tool, `update` **unconditionally rewrites its whole
file set on every run** (probed via mtime: all managed files get a fresh
mtime even when content is unchanged; content is idempotent **except for the
orphan-START shape** — see "Replacement region" below).
Missing files are recreated; files the tool set doesn't own (e.g. a user's
own `.claude/skills/my-own/`) are never touched or deleted.

Three per-file strategies exist. **Every marker-shaped template's strategy is
probed, not inferred**: `capture-update-templates.py` seeds a sandbox, appends a
sentinel after the END marker, re-runs `update`, and classifies by whether the
sentinel survives — 22 candidates, splitting 12 Managed / 10 Plain. Guessing
from the template text ("does it start with the START marker?") is wrong — see
the kilocode note below. The remaining 423 entries are **not** probed: 422 are
assigned `Plain` on the unprobed premise that a template which is not a marker
block cannot be merge-updated, and the 423rd is `.claude/settings.json`, whose
`ClaudeSettings` strategy is assigned by path. Neither premise is verified
against the oracle.

1. **Plain** (432 of 445 tool-files: skills, commands, prompts, and — despite
   appearances — kilocode's workflows): full overwrite. Mechanically the oracle
   **unlinks and recreates**: the inode changes even for an unchanged writable
   file. Two observable consequences follow, both reproduced:
   - a read-only (0400) existing file is replaced successfully and comes back
     0644 (unlink needs directory permission, not file permission);
   - a **symlinked** path is *not* followed — the link itself is removed and
     replaced by a regular file, leaving the link target untouched.
2. **Managed marker files** (12 entries / 11 distinct paths: root-level
   `CLAUDE.md`, `GEMINI.md`, `QWEN.md`, `CLINE.md`, `CODEBUDDY.md`,
   `COSTRICT.md`, `IFLOW.md`, `QODER.md`, `AGENTS.md`, `.cursorrules`,
   `.windsurfrules`; `GEMINI.md` is written by both gemini and antigravity):
   only the `<!-- SPECTRA:START v1.0.2 -->` … `<!-- SPECTRA:END -->` block is
   managed. See "Replacement region" below for the exact rule.

   > **kilocode's 10 `.kilocode/workflows/spectra-*.md` are *not* Managed**,
   > even though each template *is* a complete marker block. The oracle
   > full-overwrites them. This is precisely why the classification is probed:
   > a text-prefix heuristic classifies all 10 as Managed and nothing catches
   > it, because on a fresh sandbox both strategies emit identical bytes.
3. **`.claude/settings.json`** (claude only): JSON object merge. Managed
   keys are forced to managed values (`includeGitInstructions: false`
   even if the user set `true`), unknown user keys survive, keys end up
   alphabetically sorted (the oracle exhibits exactly serde_json's default
   BTreeMap behavior), 2-space indent, **no trailing newline**. Invalid
   JSON or a non-object (`[1,2]`) is silently replaced with the default
   template. (A/B'd against the oracle on 15 adversarial inputs — `{}`,
   empty, whitespace-only, `null`, `[1,2]`, `"hi"`, malformed, duplicate keys,
   trailing garbage, deep nesting, unicode keys, bigint, `1.0`, wrong-typed
   managed key, UTF-8 BOM — all byte-identical.)

### Replacement region (Managed)

The replaced span is a **plain substring splice with no line anchoring at
all**. Characterised by planting sentinels around the markers and reading which
survive:

```text
replace [ byte offset of the literal "<!-- SPECTRA:START",
          byte offset just past the literal "<!-- SPECTRA:END -->"
          (+1 further byte if that byte is '\n') ]
```

One model explains every observation:

| Existing content | Result |
|---|---|
| `AAA\n\t  <START>…<END>\nZZZ\n` | the `\t  ` indentation **survives** — the splice starts at the marker offset, not the line start |
| `PREFIX <START>…` | `PREFIX ` survives |
| `<START> SUFFIX\n…` | ` SUFFIX` is consumed |
| `…PREEND <END>` | `PREEND ` is consumed |
| `…<END> POSTEND\n` | ` POSTEND` **survives** |
| `AAA <START> MID <END> ZZZ\n` | `AAA ` and ` ZZZ` survive, `MID` is consumed |
| CRLF file | the `\r` after `<END>` is *not* consumed (it is not `\n`), so an orphan `\r\n` remains — this quirk is the `+1` clause, not a separate rule |
| empty body (`<END>` right after `<START>`) | replaced in place, idempotent |

The other two shapes:

- *START but no END after it*: original content is left untouched and a
  fresh complete block is **appended** at EOF after a blank line.

  > **This shape is not idempotent, and that is oracle behavior.** On the
  > *second* run the appended block's END pairs with the orphan START, so the
  > splice swallows everything between them — user content written after the
  > unpaired marker is deleted. Probed on 2.3.1: run 1 keeps it, run 2 removes
  > it, byte-identical to OpenSpectra on both runs; run 3 onward is a fixed
  > point. Pinned by
  > `managed_orphan_start_swallows_content_on_the_second_run_matching_the_oracle`
  > so that "converging" it later has to be a deliberate divergence rather than
  > an accident.
- *No START* (even if an orphan END exists): fresh block is **prepended**,
  followed by a blank line and the original content. A missing or empty file
  yields just the block.

The START match is a plain substring search, so an old version suffix
(e.g. `v0.9.0`) is still found and upgraded, and a marker inside a fenced code
block *is* matched (the oracle has no fence awareness). With two complete
blocks present, the first one wins.

> OpenSpectra shipped a line-anchored variant of this in its first draft; PR #86's
> mob review found four divergences from the rule above, two of which deleted
> user content. The regression tests in `update.rs` pin each shape in the table.

### Existing files the oracle cannot read or write

- **Unreadable as UTF-8** (e.g. a latin-1 `CLAUDE.md`): treated as *absent*.
  The original bytes are discarded and a fresh block is written — no lossy
  transcoding, no splice. This holds even when the file contains a valid
  marker pair.
- **Read-only `Plain` file**: replaced (see the unlink+recreate note above).
- **Read-only `Managed` file**: the oracle writes in place, so it **fails**,
  exiting 1 with `Error: Permission denied (os error 13)`.
- **Read-only parent directory**: exits 1 with the same message, leaving a
  partially written tree. Partial write sets on a hard failure are oracle
  behavior, not a defect to paper over.

## `{{SPEC_DIR}}` substitution

Templates embed the project's `spec_dir`: with `init --dir docs/specs`,
written files read `docs/specs/changes/` etc. Captured via a two-sandbox
diff (default `openspec` vs a unique token spec-dir) so substitution sites
are unambiguous — a plain search for "openspec" would false-match the
"OpenSpec" brand name.

**Oracle bug preserved**: a literal, never-substituted `{{SPEC_DIR}}documents`
appears in the oracle's own output (both sandboxes byte-identical there). It is
not a one-off: it is in **every tool's `spectra-ask` command/prompt file — 19 of
the 445 tool-files, across 18 tools, deduped to 9 blobs** — e.g. cursor's
`.cursor/commands/spectra-ask.md` frontmatter and gemini's
`.gemini/commands/spectra/ask.toml` `description =` value. OpenSpectra's
templates escape such literals as `{{RAW_SPEC_DIR}}` at capture time and restore
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
  (2.3 MB; 445 tool-files share content heavily, e.g. 8 of the 10 skill
  templates are byte-identical across cursor/windsurf/qwen/gemini —
  `spectra-ingest` and `spectra-propose` have per-tool variants),
- `crates/spectra-core/src/update_manifest.rs` — the generated registry
  (tool → detection path → file specs → **probed** `FileKind`), and
- `golden/update-trees-2.3.1.tsv` — sha256 of every oracle output file
  with the default spec_dir, which CI's
  `every_tool_tree_matches_the_oracle_golden_byte_for_byte` integration
  test verifies without needing the oracle.

The script is a verification contract, not a printer: it derives each tool's
write set by **diffing the sandbox before and after `update`** (and fails if
`update` touched anything `init` created), round-trips every template (token
capture → `{{SPEC_DIR}}` → re-resolve → must equal the default capture
byte-for-byte), probes every marker-shaped template's `FileKind` against the
oracle rather than guessing from its text, pins each tool's stdout, re-verifies
the registry order and the codex×gemini quirk, asserts blob filenames cannot
collide at their 12-hex prefix, and exits non-zero keeping the sandboxes on any
mismatch. The round-trip check caught the `{{SPEC_DIR}}documents` oracle bug on
its first run.

**What the contract does *not* cover** — stated plainly rather than implied
away:

- **Registry additions.** The all-tools sandbox creates detection directories
  from the script's own `TOOLS` list and compares stdout against a string built
  from that same list, so a tool the oracle *gains* is never detected and the
  check still passes. No CLI closure check exists: `init --tools bogus-tool-xyz`
  exits 0 and prints `Generated files for: bogus-tool-xyz` rather than rejecting
  an unknown id. The script pins `len(TOOLS) == 23` and the weekly
  upstream-watch flags oracle releases; discovering an added tool is a manual
  read of the release, not something this script detects.
- **The golden TSV covers the fresh-write path only** — 445 rows of
  first-write bytes. The merge paths (`Managed`, `ClaudeSettings`) are pinned by
  unit tests against probed oracle behavior, not by golden bytes.

## Deliberate divergences from the oracle

Everything else in this document is parity. These two are not, and both are
security rulings this repo already made elsewhere:

| Behavior | Oracle | OpenSpectra | Why |
|---|---|---|---|
| Symlinked `Managed` / `ClaudeSettings` path (e.g. a dotfile-managed `CLAUDE.md`) | follows the link and overwrites the target outside the project | replaces the link with a regular file; the target is untouched | `artifact.rs`'s `force_write_through_a_symlinked_artifact_path_cannot_escape_the_change_dir` already ruled that the oracle's link-following is "a shared vulnerability" this CLI does not copy. `update` writes 445 paths inside a user's project, so the exposure is larger, not smaller. |
| Read-only `Managed` file | exits 1, `Permission denied (os error 13)` | succeeds (temp file + rename needs directory permission only) | falls out of the atomic write above; not independently motivated |

`Plain` paths need no divergence: the oracle's own unlink+recreate already
declines to follow symlinks, so parity and safety coincide for 432 of 445
files.

The atomic path preserves an existing file's permission bits before renaming,
because the oracle writes in place and therefore keeps them: without that, a
`0600` `.claude/settings.json` would come back `0644` after an update, exposing
the very user keys the merge preserves. The temp file is created with `O_EXCL`,
so a pre-created symlink at the predictable temp path cannot be written through.

### Newly created file modes

For a newly created file, the oracle uses the standard regular-file base mode
filtered by the process umask: `0666 & ~umask`. Direct probes measured `0666`
at umask `000`, `0664` at `002`, `0644` at `022`, and `0600` at `077`.
OpenSpectra's `Plain` strategy already gets those modes from
`std::fs::write`; the atomic `Managed` / `ClaudeSettings` strategy now keeps
its temp file at `0600` while writing sensitive content, then applies the
calculated create mode immediately before the rename. Existing regular targets
still retain their current mode instead.

A **symlinked (or otherwise non-regular) target** is neither of those cases:
the rename replaces the link itself (the documented security stance above),
and the replacement's content may have been read *through* the link from a
secret-bearing file. The oracle — which writes through the link and never
creates an entry there — offers no parity mode for the replacing file, so
OpenSpectra pins the replacement at `0600` (an explicit fchmod, so the mode is
umask-independent) rather than treating it as a fresh create (PR #100 mob
review ruling; folding it into the create case would have turned a symlinked
`0600` settings.json into a `0644` regular file holding the same keys).

Reading the umask is itself a set/restore dance (`umask(2)` has no read-only
form). The transient value is `0o777`: a file a sibling thread creates inside
that window comes out narrower, never wider, than intended — fail-closed.

Probe pitfall: umask `022` and `077` cannot distinguish an `0666` creation base
from a hard-coded `0644` base, because both bases collapse to `0644` and `0600`
respectively under those masks. A parity probe must include umask `002` or
`000`; otherwise it can incorrectly conclude that the oracle's base is `0644`.

### Residual risk: symlinked *ancestor* directories

Only the final path component is defended. If a **parent** is a symlink —
e.g. `.claude -> ~/dotfiles/claude` — detection accepts it and every file is
written into the link target, outside the project root. Measured: the oracle
does exactly the same (13 files into the external directory, byte-identical to
ours), and symlinking your own `.claude` at a dotfile manager is a legitimate
setup rather than an attack. This is therefore documented rather than blocked:
`artifact.rs`'s no-escape baseline is scoped to a *change directory*, which is
untrusted input, whereas the project root here is the user's own. Defending
ancestors would need beneath-root/no-follow directory handles and would break
that workflow while diverging further from the reference binary.

Atomicity for `Managed` / `ClaudeSettings` is a second reason for the same
mechanism: those are read-modify-write over a user-owned file, and a plain
truncate-then-write interrupted mid-way would empty a `CLAUDE.md` — destroying
exactly the surrounding content the merge logic exists to preserve. Atomicity
is invisible to byte-for-byte parity, so oracle equivalence is no argument
against it.

## Open questions

- Whether any oracle config (`.spectra.yaml` `tools:` uncommented) alters
  detection — not probed; the key ships commented-out and `update`
  demonstrably re-detects from the filesystem.
- Why gemini suppresses codex's skills (intentional dedup for gemini-cli's
  `.agents` support, or a tool-definition bug) — behavior is pinned
  either way.
- What the oracle does when a `Managed` file's *parent* is a symlinked
  directory (only the file itself was probed).
