# Reverse-engineering `spectra config`

How the closed-source `spectra config` command group manages the **global**
user configuration, and how OpenSpectra reproduces it.

> Source binary: `Spectra.app/Contents/MacOS/spectra` v2.3.1 (arm64 Mach-O).
> Behaviour was probed by running the binary as a golden oracle over fresh
> `HOME` jails and a synthetic `spectra init` project, then byte-diffing a
> 40-step transcript (identical command sequence, oracle vs openspectra —
> see "Reproducing the oracle" below).

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
| `unset <KEY>` | `✓ Removed key: <KEY>`, exit 0 **even when the key/file is absent** — and creates a `{}` file when none existed |
| `reset` | deletes the file; `✓ Config reset.`; idempotent; `--all` and `-y` are **inert** (no confirmation prompt even on a TTY, probed via `script(1)`) |
| `edit` | spawns `$EDITOR` on the path (unset falls back to vim); a non-zero editor exit → stderr `Error: Editor exited with error.`, exit 1 |

## File location

`config path` prints (macOS, probed):

```text
$HOME/Library/Application Support/openspec/config.yaml
```

* Follows the `HOME` env var — an overridden `HOME` moves the whole tree
  (probed), which is what makes the integration tests jailable.
* `XDG_CONFIG_HOME` is **ignored on macOS** (probed). The Linux layout
  (`$XDG_CONFIG_HOME/openspec/config.yaml`, falling back to
  `~/.config/openspec/config.yaml`) is the standard config-dir convention
  (`dirs::config_dir()` shape) and **cannot be probed** — the oracle is a
  macOS-only binary. Note the app dir is `openspec`, not `spectra`.

## Value typing (`set`)

The raw CLI argument is parsed as a YAML document; on parse failure it falls
back to the literal string. All probed:

| Input | Stored as |
|-------|-----------|
| `true` / `TRUE` | bool `true` |
| `42` | int |
| `3.14` | float |
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
* **`unset` last key** → file left as `{}`.
* Openspectra writes atomically (temp + rename, shared with `init`); the
  oracle's write mechanics were not probed — only the resulting bytes.

## Not ported / unverified

* `edit`'s `$VISUAL` handling is unprobed; openspectra consults only
  `$EDITOR` and falls back to `vi` (the oracle fell back to vim when both
  were unset; `vi` is the portable spelling).
* The oracle's `--no-color` help line reads `Disable colored output`;
  openspectra's global flag appends `(also respects the NO_COLOR env var)`
  on every command — a pre-existing repo-wide divergence, not specific to
  `config`.
* Key validation: 2.3.1 accepts any key, so `--allow-unknown` ships inert
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

The calibration sequence used for this port covered: `path`; empty/populated
`list` (+`--json`); every typing row from the table above (round-tripped
through `set`/`get`); the missing-key error; inert-flag variants
(`--allow-unknown`, `reset --all -y`); double `unset`; `unset` on a missing
file; double `reset`; and the `--help` surfaces. The only diff was the
pre-existing `--no-color` help wording noted above.

The full behaviours above are pinned by
`crates/spectra-cli/tests/config_integration.rs` (byte-golden stdout/stderr
and exit codes, per-test `HOME` jails) and the unit tests in
`crates/spectra-core/src/global_config.rs`. The sort-wiring and the
serializer-newline rendering were additionally mutation-verified (dropping
`entries.sort()` / trimming the serializer newline each fail the pinned
tests).
