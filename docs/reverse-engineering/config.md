# Reverse-engineering `spectra config`

How the closed-source `spectra config` command group manages the **global**
user configuration, and how OpenSpectra reproduces it.

> Source binary: `Spectra.app/Contents/MacOS/spectra` v2.3.1 (arm64 Mach-O).
> Behaviour was probed by running the binary as a golden oracle over fresh
> `HOME` jails and a synthetic `spectra init` project, then byte-diffing
> transcripts of identical command sequences (oracle vs openspectra — see
> "Reproducing the oracle" below).
>
> **Every row below marked (probed) was measured.** Rows that are conventions
> rather than measurements are marked **(inferred)** and named in
> "Inferred, not measured".

## TL;DR

`config` manages a **global, per-user** flat YAML mapping — it does *not*
touch the project's `.spectra.yaml` (probed: sets inside an initialized
project leave `.spectra.yaml` untouched and land in the user config file).
No subcommand requires an initialized project. Implementation lives in
`crates/spectra-core/src/global_config.rs` + `cmd_config` in
`crates/spectra-cli/src/main.rs`.

| Subcommand | Behaviour (probed) |
|------------|--------------------|
| `path` | prints the config file path; creates nothing; exit 0 even when the file is missing |
| `list` | sorted `key = value` lines; `No configuration set.` when empty; `--json` = 2-space pretty JSON preserving scalar types (`{}` when empty) |
| `get <KEY>` | prints the rendered value; missing key → stderr `Error: Key '<KEY>' not found.`, exit 1 |
| `set <KEY> <VALUE>` | YAML-parses the value (see typing); echoes `✓ <KEY> = <raw input>`; `--string` forces a string; `--allow-unknown` is **inert** (2.3.1 accepts unknown keys either way) |
| `unset <KEY>` | `✓ Removed key: <KEY>`, exit 0 **even when the key is absent** — and creates a `{}` file when none existed |
| `reset` | **truncates** the file to `{}` — creating it (and its directory) when absent. Does *not* delete. Idempotent |
| `reset --all` | **deletes** the file. Idempotent. `--all` is **not** an inert flag — it selects delete instead of truncate |
| `edit` | creates the config dir and seeds a missing file with `# OpenSpec global config\n`, then spawns the editor on that path; a non-zero editor exit → stderr `Error: Editor exited with error.`, exit 1 |

`-y` **is** inert: neither `reset` mode ever prompts, on a TTY or piped
(verified under `script(1)`).

## File location

`config path` prints (macOS, probed):

```text
$HOME/Library/Application Support/openspec/config.yaml
```

* Follows the `HOME` env var — an overridden `HOME` moves the whole tree
  (probed), which is what makes the integration tests jailable.
* `XDG_CONFIG_HOME` is **ignored on macOS** (probed).
* Note the app dir is `openspec`, not `spectra`.

## Value typing (`set`)

The raw CLI argument is parsed as a YAML document; on parse failure it falls
back to the literal string. All probed:

| Input | Stored as |
|-------|-----------|
| `true` / `TRUE` | bool `true` |
| `42` | int |
| `2.5` | float |
| `null`, `""` (empty) | null |
| `[1, 2, 3]` | sequence |
| `{a: 1}` | mapping |
| `hello world` | string |
| `{unclosed`, `a: b: c` (unparseable) | string (fallback) |
| anything with `--string` | string (a forced `"true"` is written quoted: `tdd: 'true'`) |

The confirmation line echoes the **raw input** (`✓ parallel_tasks = TRUE`),
not the parsed value; `✓` is green (SGR 32) only on a TTY.

Keys are **flat literals**: `set claude_effort.apply high` writes the single
key `claude_effort.apply: high` — dotted paths are *not* nested (probed),
even though `spectra init`'s commented `.spectra.yaml` template suggests a
nested `claude_effort:` block.

A **hyphen-leading** key or value is rejected by the argument parser on both
sides, byte-identically (`config set answer -5` → `error: unexpected argument
'-5' found` plus the `tip: to pass '-5' as a value, use '-- -5'` line, exit 2).
Both binaries are clap-based, so this is parity, not a limitation to "fix" —
adding `allow_hyphen_values` would *create* a divergence.

## Value rendering (`get` / `list`)

Recovered rule (byte-matched):

* strings print raw; bools/numbers via their scalar form — no trailing
  newline of their own;
* null, sequences, and mappings go through the YAML serializer, whose
  trailing newline is passed straight through.

So `get` on a null key prints `null\n` **plus** the print newline
(`null\n\n`, probed via `xxd`), and a null in `list` produces a blank
separator line. Sequences render block-style: `arr = - 1\n- 2\n- 3\n\n`.

`list` sorts by key. `list --json` preserves scalar types
(`"tdd": "true"` for a forced string vs `"tdd": true` for a bool).

## `edit`: editor resolution and side effects

Probed precedence: **`$EDITOR` → `$VISUAL` → `vi`**.

* An **empty** `$EDITOR` is *not* treated as unset: it reaches spawn and fails
  with `Error: Failed to open editor '': No such file or directory (os error 2)`,
  exit 1.
* The final fallback is **`vi`, not `vim`** — with a `PATH` containing only a
  `vim`, the oracle errors `Failed to open editor 'vi'`; with only a `vi` it
  runs. (On macOS `vi` *is* vim, which is why a naive probe reading the
  terminal banner mistakes one for the other. Use stub scripts on a fake
  `PATH` to observe the exec'd name.)
* `$EDITOR` is **not** shell-split: `EDITOR="code --wait"` fails on both sides
  with `Failed to open editor 'code --wait'`. Do not add shell splitting — it
  would both diverge and open a shell-injection surface.
* Before spawning, the oracle creates `.../openspec/` and, when the file is
  absent, seeds it with exactly `# OpenSpec global config\n` (25 bytes,
  xxd-confirmed). An **existing** file is left untouched. Without this the
  editor is handed a path inside a directory that does not exist, so a real
  editor's save fails — and because most editors still exit 0 after reporting
  that, the user's edits would vanish with no signal from the CLI.

## Key-order divergence (deliberate)

The oracle's file writes and `list --json` key order are **random per run**
(hash-map iteration; three consecutive probe runs produced three different
orders — its own file style otherwise matches serde_yaml 0.9 exactly:
zero-indent block sequences, `{}` for an empty mapping, quoted forced
strings). Since no consumer can depend on a random order, openspectra emits
a deterministic instance of it instead: file keys in insertion order
(`serde_yaml::Mapping`), JSON keys sorted (`serde_json`'s default map). The
human `list` is sorted in both implementations.

## Degenerate inputs

All probed:

* **Corrupt YAML file** → every reader treats it as empty
  (`No configuration set.`, exit 0); the next `set` silently **overwrites**
  it. No backup is made — mirrored as-is (unlike e.g. `archive`'s corrupt
  `openspec.yaml` backup, this matches the oracle).
* **Non-mapping file** (e.g. a bare list) → same as corrupt.
* **Non-UTF-8 bytes** → same as corrupt on both sides (both overwrite). This
  is a *content* problem, not an I/O fault, and the distinction is easy to lose:
  `std::fs::read_to_string` reports non-UTF-8 as `io::ErrorKind::InvalidData`,
  indistinguishable from a real read failure, so `load` reads bytes and decodes
  separately. Classifying it as I/O made `set` refuse where the oracle
  overwrites (caught in re-review; pinned by
  `non_utf8_config_reads_as_empty_and_is_overwritten_by_set`).
* **Unreadable file** (mode 000) → the *read* paths are lenient like a corrupt
  file (`list` → `No configuration set.`; `get k` → `Error: Key 'k' not found.`,
  exit 1), but every *write* path refuses: `set`, `unset`, and plain `reset`
  all print `Error: Permission denied (os error 13)`, exit 1, and **leave the
  file intact**. `reset --all` is the exception — it does not read first, so it
  deletes the file and exits 0.
  This asymmetry is load-bearing for openspectra: `set`/`unset` are
  load-modify-save, and `write_atomically`'s temp+rename needs only the
  *directory* write bit, so flattening the read error into "empty" would
  replace the whole config with a file holding just the new key. See
  `global_config::load` vs `load_for_display`.
* **`unset` last key** → file left as `{}`.

## Known divergences (measured, deliberate, not defects)

* **Write mechanics.** The oracle writes the config file **in place**, so with
  a read-only *directory* (mode 555) holding a writable config it still
  succeeds (`✓ beta = 2`, exit 0). openspectra writes atomically (temp +
  rename, shared with `init`), which needs the directory write bit and
  therefore fails there. Atomicity protects against a crash mid-write, which
  the oracle does not; the trade was made knowingly. This is the only case
  where the oracle succeeds and openspectra does not.
* **A symlinked config file.** Same root cause, opposite symptom: the oracle
  writes *through* a `config.yaml` symlink and leaves the link in place, while
  the temp+rename replaces the link with a regular file — so a dotfile
  manager's tracked target silently stops receiving updates. Both exit 0, so
  only the on-disk result differs. Measured: with `config.yaml -> real.yaml`
  containing `a: 1`, `config set k v` gives oracle `real.yaml == "k: v\na: 1\n"`
  and a still-symlinked `config.yaml`; openspectra leaves `real.yaml`
  untouched and `config.yaml` a regular file. `edit` behaves the same way on a
  dangling link. Accepted for the same reason as the row above; see
  `init::write_atomically`'s rationale.
* **`HOME` unset.** The oracle still resolves the account's home via the OS
  password database and prints a path; openspectra errors
  (`could not determine the config directory ...`, exit 1). Matching would
  require a `getpwuid`-backed dependency (e.g. `dirs`) that this workspace
  does not carry — see Follow-ups.
* **`--no-color` help wording.** openspectra's global flag appends
  `(also respects the NO_COLOR env var)`. Pre-existing repo-wide divergence,
  not specific to `config`.

## Inferred, not measured

* **The Linux path layout** (`$XDG_CONFIG_HOME/openspec/config.yaml`, falling
  back to `~/.config/openspec/config.yaml`) is the standard config-dir
  convention. It **cannot be probed** — the oracle is a macOS-only binary that
  ignores XDG. openspectra additionally requires `XDG_CONFIG_HOME` to be
  **absolute**, per the XDG spec: honoring a relative value would make the
  global config location depend on the process's working directory.
* **Key validation.** 2.3.1 accepts any key, so `--allow-unknown` ships inert
  (flag parity for help-text fidelity). If a later oracle version starts
  rejecting unknown keys, wire the flag up then.

## Reproducing the oracle

With the reference binary on macOS, jail each side in its own fresh `HOME`
and byte-diff the transcripts of an identical command sequence:

```sh
ORACLE=/Applications/Spectra.app/Contents/MacOS/spectra
OURS=target/release/spectra
# for BIN in "$ORACLE" "$OURS": run the same sequence with HOME=<fresh dir>,
# recording stdout/stderr/exit per step (sed the HOME prefix out of `path`,
# key-sort `list --json` — its key order is random on the oracle), then diff.
```

The calibration covered, in two passes:

1. **Happy path (40 steps):** `path`; empty/populated `list` (+`--json`); every
   typing row above, round-tripped through `set`/`get`; the missing-key error;
   `--allow-unknown`; double `unset`; `unset` on a missing file; double
   `reset`; and every `--help` surface. Sole diff: the `--no-color` help
   wording noted above.
2. **`edit` and degenerate I/O (added after review found the first pass had
   never exercised them):** editor resolution with stub scripts on a fake
   `PATH` (`EDITOR`, `VISUAL`, neither, empty `EDITOR`, unspawnable `EDITOR`,
   `EDITOR` with arguments); post-`edit` file state on a virgin and an existing
   `HOME`; post-`reset` and post-`reset --all` file bytes; and `set`/`unset`/
   `reset`/`reset --all` against a mode-000 config, a non-UTF-8 config, a
   config path that is a directory, and a read-only directory.

> **Method note.** The first pass concluded that `--all` was inert because it
> ran `reset` and `reset --all` back to back and only inspected the file
> afterwards — the truncate was invisible behind the later delete. Probe one
> operation per jail; a conflated sequence yields a confident wrong answer.

The behaviours above are pinned by
`crates/spectra-cli/tests/config_integration.rs` (byte-golden stdout/stderr
and exit codes, per-test `HOME` jails, stub editors that record their argv)
and the unit tests in `crates/spectra-core/src/global_config.rs`. Seven of
them are mutation-verified — reverting the I/O-error propagation, the
`$VISUAL` lookup, the empty-`EDITOR` handling, the `edit` seed, the
`reset`/`reset --all` split, the JSON pretty-printing, or the path passed to
the editor each turns the corresponding test red.
