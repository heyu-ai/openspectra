# W12 oracle spec — `decisions`, `demo`, `feedback`, `show --deltas-only/-r/--item-type` (spectra 3.0.0)

Oracle: `/Applications/Spectra.app/Contents/MacOS/spectra` (`spectra 3.0.0 (Apple Silicon)`).
Probe date 2026-09-28 (+0800). Probes live in this directory: `lib.sh` (jail helpers,
`GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1`, identity T/t@x, commit dates
2026-09-01T00:00:00Z), `pNN_*.sh`/`p19_harvest.py`, raw output `pNN.out`. One fresh jail per
scenario under `jails/`. Projects: `.spectra.yaml` (`spec_dir: openspec`),
`openspec/{changes/archive,specs}`, `openspec/config.yaml` (`schema: spec-driven`).

**[V pNN]** verified by the named probe; **[I]** inferred (consistent with probes, not isolated).

## 0. Probe hygiene incident (read first)

`/Users/howie/.claude/jobs/9eb90dff/tmp/` contains another agent's `openspec/` (an npm package
unpacked there) and a `.git`. The oracle walks up and treats any ancestor with an `openspec`
entry as the project root, so the **first** `p15` run (uninitialized dir inside `jails/`) did
**not** hit "Not initialized": `demo` wrote two changes into `tmp/openspec/changes/`. I removed
exactly those two directories and the `changes/` dir the oracle created (nothing else was in it;
`package/` and the `.tgz` are untouched). The same root-discovery explains the `Failed to open
migration lock: Operation not permitted` seen in p02/p03 (sandbox denied writes to `tmp/`).
Uninitialized-dir probes were re-run under `$TMPDIR` with an ancestor-marker check (p15).

## 1. `feedback` — local print only, no network  [V p01]

Safety: `otool -L` shows no networking library (only libiconv, Security, CoreFoundation, libz,
libSystem); `strings` has no reqwest/hyper/ureq/rustls. Every run was inside `sandbox-exec`
with `(deny network*)` and `/usr/bin/open` exec denied (positive controls in p01: `curl` →
rc 6, `open` → rc 71); a second profile also denied all file writes under /Users,
/private/var/folders, /private/tmp and the command still succeeded (rc 0) → it writes nothing.

```
Usage: spectra feedback [OPTIONS] <MESSAGE>
Arguments:  <MESSAGE>  Feedback message
Options:    --body <BODY>  Detailed body / --no-color / -h
```
stdout, exit 0, works outside a project:
```
Thank you for your feedback!
Message: <MESSAGE>
Details: <BODY>            ← only when --body given (printed verbatim, newlines kept)

To submit feedback, visit: https://github.com/kaochenlong/spectra-app/issues
```
Empty message `""` accepted (`Message: `). Missing message / extra positional / `--json` →
clap usage errors, exit 2. On a TTY [V script(1)]: `Thank you for your feedback!` green
(`\e[32m…\e[0m`) and the whole `To submit feedback, visit: <url>` line dim (`\e[2m…\e[0m`);
`Message:`/`Details:` lines plain.

## 2. `decisions [KEYWORD] [--json]`  [V p04–p14, p20]

Needs an initialized project (`Error: Not initialized. Run 'spectra init' to initialize.`,
exit 1 [V p15]); works from a subdirectory [V p14]. Always exit 0 otherwise. Reads only
`design.md` files; writes nothing.

### Sources and order
1. **Active changes**: every directory directly under `<spec_dir>/changes/` except `archive`
   — hidden (`.hidden`), upper-case/underscore and dated-looking names
   (`2026-05-05-activeprefixed`) all count [V p14, p20]. Ordered like `list`'s default
   `modified` sort: newest regular-file mtime (recursive) descending [V p06]; ties in readdir
   order [V p05] (OpenSpectra: by name, the existing deliberate divergence).
2. **Archived changes**: every directory directly under `changes/archive/` (hidden included),
   **after** all active ones, in raw readdir order — neither name nor mtime nor date [V p06,
   p07: two jails with opposite creation order give the same APFS hash order]. OpenSpectra
   cannot reproduce APFS order portably → deliberate divergence, sorted by directory name.
3. Parked changes are **not** included [V p14]. `changes/archive/design.md` and nested dirs are
   ignored [V p20]. A missing `changes/` or `archive/` dir → nothing, no error [V p14].
4. `design.md` that is a directory or not valid UTF-8 → the change contributes nothing,
   silently [V p14].
5. Decisions within one design.md keep document order.

### Change name and date
- Active: name = dir name; date = raw `created` of `.openspec.yaml` **only when it has both
  `schema` and `created`** (the `show` rule; scalars stringified: `20260210`,
  `2026-02-10T10:00:00Z`, `yesterday`, quoted → unquoted); otherwise `""` [V p05, p14].
- Archived dir `d`: name = `d[11..]` when `len(d) > 11 && d[4] == '-'`, else `d`
  (`2026-5-5-short` → `ort`, `2026-abcdefgh` → `gh`, `abcd-efghijkl` → `kl`,
  `2026-05-05x` → unchanged, `12345-abcdefg` → unchanged) [V p07, p12, p13].
  date = `d[..10]` when `len(d) > 10` and `d[..10]` matches `\d{4}-\d{2}-\d{2}` (no calendar
  check: `2026-13-45`, `9999-99-99`, `0000-00-00` accepted), else the `.openspec.yaml` rule
  above (`2026-x-arch-yaml` → `2021-01-01`), else `""` [V p05, p12, p13, p14]. A valid prefix
  wins over `created` [V p05].

### Parsing design.md (fence-aware)  [V p08, p09]
Lines via Rust `lines()` (CRLF → LF). A fence toggle is any line whose `trim_start()` starts
with ```` ``` ```` or `~~~` (one shared flag; ```` ```` ```` opens and ```` ``` ```` closes;
```` ```rust ```` closes) [V p09]. Fenced lines are never section/heading lines (a `## Decisions`
inside a fence does not open the section) but they **are** rationale text.
- Section start: unfenced line with `trim_end() == "## Decisions"` (`## Decisions  ` and
  `## Decisions\t` yes; ` ## Decisions`, `## decisions`, `## Key Decisions`, `## Decisions:`,
  `# Decisions`, `### Decisions`, BOM-prefixed: no). Several sections all count.
- Section end: unfenced line starting with `## ` (`## ` alone ends it; `##`, `##\tX`,
  `##Nospace`, `# Top`, `####` do not).
- Decision heading: unfenced in-section line starting with `### ` → heading = rest `.trim()`
  (`### ` → `""`, `###   Spaced   ` → `Spaced`, `### Foo ###` → `Foo ###`). `###A`, `###\tX`,
  `  ### X`, bare `###` are not headings (they are rationale text if inside a decision).
- Rationale: all lines after the heading up to the next heading / section end / EOF, joined
  with `\n`, then `.trim()` of the whole string (inner indentation kept) [V p08 ws-rationale].
  `####`, HTML comments, fenced blocks included verbatim.
- Duplicate headings produce separate entries.

### Supersession  [V p10, p11]
- Field: the **first** rationale line whose `trim_start()` starts with `**Supersedes**:`
  (anywhere in the rationale, not only directly under the heading; fenced lines count;
  `- **Supersedes**:`, `**Supersedes:**`, `Supersedes:`, `**supersedes**:` do not match). If
  that first line fails to parse, no later line is tried.
- Value = rest after `**Supersedes**:`; `split_once('/')`; both sides `.trim()`; either side
  empty or no `/` → no field at all (`supersedes: null`, `unresolvableSupersession: null`).
- Resolution: exact, case-sensitive match of (change name, heading) against all collected
  decisions (active and archived names; a dated `2026-01-15-old` does not resolve). Self and
  same-change references resolve.
  - resolved → `supersedes: {"change", "heading"}` and the target gets `superseded: true`. When
    several decisions share the target (change, heading), only the **last** one in listing
    order is marked [V p11 dup-target].
  - unresolved → `unresolvableSupersession: "<change> / <heading>"` (reformatted from the
    trimmed parts: `ghost/X` → `ghost / X`).
- Supersession is computed over all decisions **before** the keyword filter [V p12].

### Keyword filter  [V p04, p12, p13]
Keeps decisions whose heading or rationale contains the keyword after `to_lowercase()` on both
sides (so `straße`≠`strasse`, `İstanbul` matches `i̇stanbul` not `istanbul`, `ǅ/Ǆ/ǆ` match).
Not trimmed (`"  replace  "` → none). Change name and date are not searched. `""` keeps all.

### JSON  [V p04]
A top-level array (`[]` when empty, pretty-printed), keys in declaration order:
`heading, change, date, rationale, supersedes ({change, heading} | null), superseded (bool),
unresolvableSupersession (string | null)`.

### Human  [V p04, TTY bytes via script(1)]
Per decision:
```
<heading>                       bold  \e[1m…\e[0m
  <change> · <date>             dim   \e[2m…\e[0m   (" · <date>" omitted when date is "")
  supersedes <c> / <h>          "supersedes" yellow \e[33m…\e[0m, rest plain
  unresolvable supersedes: <s>  "unresolvable supersedes:" red \e[31m…\e[0m, rest plain
  superseded by a later decision   whole phrase yellow
```
(the three optional lines in that order), then a blank line and `N decisions` dim — always
plural (`1 decisions`). Empty: `No decisions found.` dim. No color when piped, with
`--no-color`, or with `NO_COLOR` set.

## 3. `show` flags  [V p16, p17]

- `[ITEM]` is optional; missing → `Error: Please specify an item name.` exit 1 (checked before
  `--item-type` validation: `show --item-type bogus` gives this error).
- `--deltas-only`, `-r/--requirements`: accepted and **inert** for both changes and specs,
  human and JSON, alone or combined. Repeating any flag is a clap error (exit 2).
- `--item-type <type>`: exactly `change` or `spec` (case-sensitive); otherwise
  `Error: Unknown type: <v>. Use 'change' or 'spec'.` exit 1 (`--item-type=` → `Unknown type: .`),
  checked before existence. `change` → only changes (`Error: Change '<n>' not found.`),
  `spec` → only specs (`Error: Spec '<n>' not found.`). A same-named spec is reachable only
  via `--item-type spec`. Missing value → clap error exit 2.
- Uninitialized → the usual "Not initialized" error [V p15].

## 4. `demo`  [V p02, p18, p19]

Safety: probed only under `sandbox-exec` with network denied, `/usr/bin/open` denied and file
writes allowed only inside the jail. Nothing is downloaded; it writes only
`<root>/<spec_dir>/changes/<name>/` (no `.spectra/` state) [V p02, p18].

- Uninitialized → `Error: Not initialized. Run 'spectra init' to initialize.` exit 1 [V p15].
- Name `spx-<adjective>-<pokemon>`, uniformly random; 600 runs saw 20 adjectives
  (bold bright calm cool dark deep eager fast gentle happy keen light neat proud quick rare sharp
  tall vivid warm) and 20 Pokémon (absol arcanine bulbasaur charizard charmander dragonite eevee
  gardevoir gengar gyarados jigglypuff lapras lucario mewtwo pikachu rayquaza snorlax squirtle
  togekiss umbreon) [V p19]. Collision: retries; after 20 attempts
  `Could not generate a unique change name after 20 attempts.` [I strings].
- Theme: one of 8, random (p19 counts 64–91 each of 600): access-control, audit-trail,
  batch-export, keyboard-macros, real-time-sync, smart-search, snapshot-restore, theme-engine.
- Files: `.openspec.yaml` (`schema: spec-driven` — **always**, even when config.yaml names a
  custom schema — `created: <local date>`, `created_by: <same identity as new change>`),
  `proposal.md`, `design.md`, `tasks.md`, `specs/<theme>/spec.md`. Content is a pure function
  of the theme: byte-identical across all 600 runs [V p19]; captured to `harvest/<theme>/`.
- stdout (exit 0):
  ```
  ✓ Created demo change: <name>        ✓ green \e[32m✓\e[0m on a TTY
    Theme: <theme>
    Path: <root>/<spec_dir>/changes/<name>
  ```
- Extra positional or `--json` → clap error exit 2.

## Needs a human decision

1. **`feedback` says "Thank you for your feedback!" but sends nothing.** Ported verbatim
   (local print + the upstream issue URL). The owner may prefer an OpenSpectra-specific URL
   or wording; that is a product decision, not a fidelity one.
2. **`decisions` ordering of archived changes** is APFS readdir order in the oracle; the port
   sorts archived dirs by name (ascending) as a deliberate, documented divergence (same class as
   the existing `list` tie-break). If a different stable order is preferred (e.g. newest date
   first), say so.
3. **`list` hides dated-looking active changes but the oracle shows them** (p14: oracle `list`
   prints `2026-05-05-activeprefixed`; OpenSpectra's `walk_names_in` filters
   `^\d{4}-\d{2}-\d{2}-`). Pre-existing, out of W12 scope; `decisions` follows the oracle.
4. **`demo` vs OpenSpectra's `new change` sidecar**: OpenSpectra's `new change` writes
   `.spectra/changes/<name>.started`; the oracle's `demo` writes no `.spectra/` state. The port
   follows the oracle (no `.started`).
5. **`created_by` identity source**: with `GIT_CONFIG_GLOBAL=/dev/null` the oracle still used the
   real `~/.gitconfig` identity for both `new change` and `demo` (p18), i.e. it does not honour
   git's env overrides; OpenSpectra's shared helper runs `git config`, which does. Pre-existing
   and shared with `new change`; not changed here.
