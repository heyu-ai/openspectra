# `spectra decisions` (oracle 3.0.0)

`spectra decisions [KEYWORD] [--json]` lists the architecture decisions recorded
as `###` headings under `## Decisions` in every change's `design.md`. The
built-in design template (`assets/schemas/spec-driven-3.0.0.json`) tells authors
that `spectra decisions` lists these headings and that a replacement declares
`**Supersedes**: <change-name> / <decision heading>`.

Implementation: `crates/spectra-core/src/decisions.rs`, human rendering in
`render_decisions_human` (`crates/spectra-cli/src/main.rs`). Everything below
was probed on oracle 3.0.0 in throw-away git jails (W12 probes p04–p14, p20,
p21; `GIT_CONFIG_GLOBAL=/dev/null`); the regression tests quote the oracle's
bytes for the same fixtures.

## Scope

* Needs an initialized project (`Error: Not initialized. Run 'spectra init' to
  initialize.`, exit 1); works from any subdirectory. Otherwise always exit 0.
* Reads `design.md` files only and writes nothing.
* **Active changes**: every directory directly under `<spec_dir>/changes/`
  except `archive`. Hidden (`.x`), upper-case/underscore and dated-looking names
  (`2026-05-05-foo`) all count — note that OpenSpectra's `list` hides the
  dated-looking ones, which the oracle's `list` does not (pre-existing, not
  changed here).
* **Archived changes**: every directory directly under `changes/archive/`,
  hidden ones included. `changes/archive/design.md` and nested directories are
  ignored.
* Parked changes are not included. A missing `changes/` or `archive/` is an
  empty list, not an error.
* A `design.md` that is a directory or not valid UTF-8 contributes nothing,
  silently.

## Order

Active decisions come first, archived ones after them; within one design.md,
document order.

* Active changes follow `list`'s default `modified` sort: newest regular-file
  mtime anywhere under the change directory, in whole seconds, descending.
* The oracle breaks mtime ties by raw `readdir` order, and orders the **whole**
  archive by raw `readdir` order too (neither name, mtime nor date: two jails
  built in opposite creation order list identically, i.e. APFS hash order).
  **Deliberate divergence:** OpenSpectra breaks active ties by name and sorts
  archived directories by name (ascending), as `list` already does for its
  ties. On copies of the three corpus projects (yibi-mvp, nextrek-cli,
  yibi-stack: 664 / 4 / 90 decisions) every decision matches the oracle
  field-for-field; nextrek-cli is byte-identical, the other two differ only in
  the order of tied and archived entries.

## Change name and date

* Active: the name is the directory name. The date is the raw `created` value
  of `.openspec.yaml`, but only when the file parses and has **both** `schema`
  and `created` (the `show` rule; scalars are stringified, so `20260210`,
  `2026-02-10T10:00:00Z` and `yesterday` pass through). Otherwise `""`.
* Archived directory `d`:
  * name = `d[11..]` when `d` is longer than 11 bytes and byte 4 is `-`,
    otherwise `d` unchanged. The rest of the prefix is not checked:
    `2026-5-5-short` → `ort`, `2026-abcdefgh` → `gh`, `abcd-efghijkl` → `kl`;
    `2026-05-05x` (11 bytes) and `12345-abcdefg` are unchanged.
  * date = `d[..10]` when `d` is longer than 10 bytes and those 10 bytes have
    the `DDDD-DD-DD` digit pattern (no calendar check: `2026-13-45` and
    `0000-00-00` are accepted; `2026-05-05` alone is not, it is only 10 bytes).
    Otherwise the `.openspec.yaml` rule above, otherwise `""`. A valid prefix
    wins over `created`.

## Parsing `design.md`

Lines come from Rust `lines()`, so CRLF becomes LF.

* **Fences.** A line whose `trim_start()` begins with three backticks or
  `~~~` toggles one shared in-fence flag (fence length and kind are not
  matched: four backticks open and three close; a backtick fence with an info
  string closes an open one). Fenced lines never start or end a section or a
  heading, but they are part of a rationale.
* **Section start**: an unfenced line with `trim_end() == "## Decisions"`.
  Not matched: leading spaces, other case, `## Key Decisions`,
  `## Decisions:`, `# Decisions`, `### Decisions`, a UTF-8 BOM. A document can
  have several such sections.
* **Section end**: an unfenced line starting with `## ` (`## ` alone ends it;
  `##`, `##\tX`, `##Nospace`, `# Top` and `####` do not).
* **Decision heading**: an unfenced line inside a section starting with `### `;
  the heading is the rest, `trim()`-ed (`### ` alone is a decision with an
  empty heading, `### Foo ###` keeps the trailing hashes). `###A`, `###\tA`,
  bare `###` and indented `  ### A` are not headings. Duplicate headings stay
  separate entries.
* **Rationale**: every line after the heading up to the next heading, section
  end or end of file, joined with `\n`, then `trim()`-ed as a whole (inner
  indentation is kept). `####` headings, HTML comments and fenced blocks are
  included verbatim.

## Supersession

* The field is the **first** rationale line whose `trim_start()` begins with
  `**Supersedes**:` — anywhere in the rationale (not only right under the
  heading), fenced or not. `- **Supersedes**:`, `**Supersedes:**`,
  `Supersedes:` and `**supersedes**:` do not count. If that first line does
  not parse, later lines are not tried.
* The value after the colon is split at the first `/` and both sides are
  trimmed. No `/`, or an empty side, means there is no field at all.
* Resolution is an exact, case-sensitive match of (change name, heading)
  against every collected decision, active and archived (so the dated
  `2026-01-15-old` never resolves; use `old`). Self and same-change references
  resolve.
  * Resolved: `supersedes` is `{"change", "heading"}` and the target gets
    `superseded: true`. When several decisions share the target's (change,
    heading), only the **last** in listing order is marked.
  * Unresolved: `unresolvableSupersession` is `"<change> / <heading>"`, rebuilt
    from the trimmed parts (`ghost/X` → `ghost / X`).
* Supersession is resolved over all decisions **before** the keyword filter, so
  a filtered-down listing still shows `superseded by a later decision`.

## Keyword

Keeps decisions whose heading or rationale contains the keyword, both sides
lowered with Unicode `to_lowercase()` (not case folding: `straße` does not
match `strasse`; `İstanbul` matches `i̇stanbul` but not `istanbul`). The keyword
is not trimmed; `""` keeps everything. The change name and the date are not
searched.

## Output

JSON: a pretty-printed top-level array (`[]` when empty) of objects whose keys
keep declaration order: `heading`, `change`, `date`, `rationale`, `supersedes`
(`{"change", "heading"}` or `null`), `superseded`, `unresolvableSupersession`
(string or `null`).

Human, per decision:

```
<heading>
  <change> · <date>
  supersedes <change> / <heading>
  unresolvable supersedes: <change> / <heading>
  superseded by a later decision
```

` · <date>` is omitted when the date is empty; the last three lines appear only
when they apply, in that order. Then a blank line and `N decisions` — always
plural, `1 decisions` included. With nothing to show: `No decisions found.`

On a terminal the heading is bold (`\e[1m`), the change/date line and the
footer (or `No decisions found.`) dim (`\e[2m`), the word `supersedes` and the
whole `superseded by a later decision` yellow (`\e[33m`), and
`unresolvable supersedes:` red (`\e[31m`); references stay uncolored. No color
when piped, with `--no-color`, or with `NO_COLOR` set.
