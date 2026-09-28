# Reverse-engineering `spectra scope`

How the closed-source `spectra scope` command (new in 3.0.0, #165) lists the
implementation scope of a change read-only, and how OpenSpectra reproduces it.

> Source: `Spectra.app/Contents/MacOS/spectra` 3.0.0 (Apple Silicon), built
> with git2-0.21.0 / libgit2-sys 0.18.8 (libgit2 1.9.x). Probed 2026-09-28 in
> scratch jails, one fresh jail per scenario, with `GIT_CONFIG_GLOBAL=/dev/null`,
> `GIT_CONFIG_NOSYSTEM=1`, a fixed identity and dates, and `NO_COLOR=1`. The base
> tree is the one `task.md` uses (`.spectra.yaml` with `spec_dir: docs/spectra`,
> `.gitignore` containing `.spectra/`, change `demo` with four tasks, `src/a.rs`,
> `src/b.rs`), whose base commit is `ab81de440d47494d78249705771d9f5f585e7084`.
> Scenario ids `pNN` below name those probe scripts. The snapshot preimage (§5)
> was recovered by running a re-signed copy of the binary under lldb and
> capturing every `git_odb_hash` input (p11, p12), then confirmed by forging ids
> that `--check-snapshot` accepts (p24).

Legend: **[V pNN]** verified by probe, **[I]** inferred.

---

## 0. Pipeline and error precedence  [V p07, p22, p23]

Capture mode (no `--check-snapshot`), in order; first failure wins, `Error: <msg>\n` on stderr, exit 1, empty stdout:

1. clap parsing (exit 2; e.g. `spectra scope extra` → `error: unexpected argument 'extra' found\n\nUsage: spectra scope [OPTIONS]\n\nFor more information, try '--help'.\n`; `--change -x` is also a clap error, `--change=-x` reaches the validator).
2. Change-ID syntax (shared validator):
   - `--change ''` → `Change ID must not be empty`
   - `--change BAD`, `.`, `a b` → `Change ID 'BAD' must contain only lowercase letters, digits, and hyphens`
   - `--change ../specs`, `..`, `demo/`, `archive/x` → `Change ID '../specs' contains illegal characters (path separators or '..')`
   - `--change=-x` → `Change ID '-x' must not start or end with a hyphen`
3. Open git repo (discovery upward from the project root):
   `Git unavailable for scope: could not find repository at '<abs project root>'; class=Repository (6); code=NotFound (-3)`.
   Bare repo → `Git scope requires a worktree` [V p18g].
4. Change existence: `Change "nope" not found` (Debug-quoted). Existence = directory `<spec_dir>/changes/<id>`
   exists; `tasks.md` not required; `--change archive` is accepted (the archive dir is a directory) [V p07g, p21].
   A plain file `changes/fileonly` → not found.
5. Tracking file `.spectra/touched/<change>.json` (only with `--change`; absent = no tracking):
   - parse error → `Failed to parse touched tracking <ABS PATH>: <serde_json error>` e.g.
     `... key must be a string at line 1 column 2`, `... EOF while parsing a value at line 1 column 0`,
     numeric `task_id` → `... invalid type: integer \`1\`, expected a string at line 5 column 18` [V p05e, p07h].
   - `change` mismatch → `Touched tracking belongs to "other", expected "demo"`.
6. Snapshot capture (status + reading every dirty worktree file):
   - unreadable file → `Cannot read source "src/a.rs": Permission denied (os error 13)` (tracked or untracked) [V p04h/i]
   - tracked path replaced by a FIFO → `Unsupported source file type at "src/a.rs"` [V p25a] (untracked FIFOs are skipped silently [V p17e])
   - dirty path under a directory symlink → `Scope path "src/sub/x.rs" traverses a directory symlink; target was not read` [V p17f]
7. Explicit `--base` resolution (revparse + peel to commit):
   - `Invalid comparison base "nosuchrev": revspec 'nosuchrev' not found; class=Reference (4); code=NotFound (-3)`
   - `Invalid comparison base "": failed to parse revision specifier - Invalid pattern ''; class=Invalid (3); code=InvalidSpec (-12)`
   - tree/blob → `Invalid comparison base "HEAD:src": the git_object of id '<oid>' can not be successfully peeled into a commit (git_object_t=1).; class=Object (11); code=InvalidSpec (-12)`
   - unborn HEAD with `--base HEAD` → `Invalid comparison base "HEAD": revspec 'HEAD' not found; class=Reference (4); code=NotFound (-3)`
   - base resolves but HEAD unborn → `Comparison base requires a committed HEAD` [V p18c]
   - base not an ancestor of HEAD (descendant, unrelated orphan) → `Comparison base "<raw arg>" is not in HEAD's history`
   Abbreviated oids and annotated tags are accepted (peeled) [V p07a, p19d].
   Explicit-base errors are fatal; review_base problems are *limitations* (§3).

Precedence verified: `--change BAD --base nosuch` → ID error; non-git + `--change nope` → git error;
`--change nope --base nosuch` → not found; corrupt tracking + bad base → tracking error; unreadable file +
bad base → `Cannot read source` [V p22].

Project root: found like other commands (walks up to a `.spectra.yaml`/`openspec` marker — a stray
`tmp/openspec` made a no-project jail resolve to `tmp`, p07e). Repo discovered upward from the project root.
**Project inside a larger repo** (`repo/proj/.spectra.yaml`): status/diffs restricted to the project subtree,
`files[].path`/`diffs[].path` and preimage paths are **project-relative** (`src/a.rs`), while the patch text uses
**repo-relative** paths (`diff --git i/proj/src/a.rs w/proj/src/a.rs`) [V p18e, p19a]. **Linked worktree**:
running from a linked worktree reports the *main* checkout's state (project root maps to the main worktree) [V p19b].

---

## 1. JSON output (`--json`)  [V p01…]

serde_json pretty (2-space), **trailing `\n`**, raw UTF-8 (no `\u` escaping of non-ASCII). Key order:

```
{
  "schema_version": 1,
  "snapshot_id": "<40 hex>",
  "scope_source": "...",
  "status": "empty|resolved|insufficient",
  "base_revision": "<oid>"|null,
  "base_source": "explicit"|"review_base"|null,
  "head_revision": "<oid>"|null,          // null on unborn HEAD
  "files": [ FILE... ],
  "limitations": [ {"path": <str|null>, "code": "...", "reason": "..."} ... ]
}
```
FILE: `path`, `old_path`, `change_kinds`, `provenance`, `content_kind`, `inspectable`, `diffs`.
DIFF: `kind`, `status`, `path`, `old_path`, `patch` (string or null).
Limitation key order: `path`, `code`, `reason`.

Exact clean-repo bytes (293 B):
```
{\n  "schema_version": 1,\n  "snapshot_id": "7b448c3aed23074dcd35ff3c0aaff393f9c11180",\n  "scope_source": "current_worktree",\n  "status": "empty",\n  "base_revision": null,\n  "base_source": null,\n  "head_revision": "ab81de440d47494d78249705771d9f5f585e7084",\n  "files": [],\n  "limitations": []\n}\n
```

### 1.1 scope_source / base_source  [V p01, p05, p06, p19c]

Let *T* = the tracking file's `touched` list (empty when no file), *explicit* = `--base` given.

| `--change` | touched entries | base | scope_source | base_source |
|---|---|---|---|---|
| no | – | no `--base` | `current_worktree` | null |
| no | – | `--base` | `explicit_base` | `explicit` |
| yes | T non-empty, all entries have `provenance` | any | `touched_tracking` | explicit > valid review_base > null |
| yes | T non-empty, ≥1 entry without `provenance` (legacy) | any | `approximated_tracking` | same |
| yes | T empty/absent | explicit or valid review_base | `approximated_base` | `explicit` / `review_base` |
| yes | T empty/absent | none valid | `approximated_worktree` | null |

`--base` without `--change` never reads tracking. With `--change`, `--base` wins over `review_base`
("takes precedence over stored metadata") and then no `invalid_review_base` is reported [V p19c].
`review_base` is *valid* when its `head_revision` is non-null, HEAD is born, and it is an ancestor of (or equal
to) HEAD. `base_revision` = the validated base oid (explicit → peeled oid; review_base → its head_revision).

### 1.2 status  [V]
- `insufficient` iff `missing_comparison_base` is among the limitations (i.e. `--change` given and no valid base).
- else `empty` iff `files` is empty; else `resolved`.
Binary/non-UTF-8/submodule/approximated limitations do **not** make it insufficient.

### 1.3 Which files are included  [V p01, p05, p06, p13, p18e]
The candidate set is the union of:
- **committed**: tree diff validated-base → HEAD (only when a validated base exists; none when base == HEAD);
- **staged**: HEAD tree (empty tree when unborn) → index;
- **unstaged / untracked**: index → worktree, untracked files included (dirs recursed), ignored excluded,
  FIFOs skipped, empty dirs absent, a nested repo appears once as `inner`.
All three diffs use rename detection (libgit2 `find_similar`, renames incl. untracked-as-target, default 50%
threshold, no copies); see §4.3.
- No `--change`, or approximated_base/approximated_worktree: **every** candidate, including the spec dir, other
  changes' dirs, `.spectra.yaml`, and `.spectra/` files when not gitignored (no exclusions at all) [V p03j, p13a].
- touched_tracking / approximated_tracking: only candidates whose `path` or `old_path` (either end of a rename)
  equals a touched `files[]` string exactly (no directory-prefix expansion: `--file src` matches nothing) [V p05g,
  p06h/h2]. Untouched dirty/committed files are silently dropped (no limitation).
- Project-in-subdir: only paths under the project dir.

### 1.4 files[] fields  [V]
- Ordering: by `path` bytes (`B` < `Z/y2` < `a-b` < `a.b` < `a/b` < `a0`) [V p17a].
- Grouping: one FILE per path; rename chains are merged: staged `b→b2` + unstaged `b2→b3` gives FILE
  `path "src/b3.rs"`, `old_path "src/b.rs"` with two diffs keeping their own `path/old_path` [V p18d].
  `old_path` null unless a rename is involved.
- `change_kinds`: subset in fixed order `committed`, `staged`, `unstaged`, `untracked`. Union of the diffs' kinds;
  an unstaged rename whose target is untracked adds both `unstaged` and `untracked` (1 diff, kind `unstaged`)
  [V p02j, p18d]. `git rm --cached` → `["staged","untracked"]` (2 diffs) [V p20b].
- `provenance`: `["git_diff"]` for non-tracking sources; in tracking modes the provenances of all touched entries
  naming the path, deduplicated in touched-entry order: `task_baseline`, `explicit_files`, `legacy_unverified`
  (entry without provenance) [V p05j, p17b, p06b].
- `diffs`: ordered by kind (committed, staged, unstaged, untracked); a worktree typechange produces two
  `unstaged` diffs, `deleted` then `added` [V p03h, p17d].
- diff `status`: `modified`, `added`, `deleted`, `renamed`, `untracked` (untracked diffs always `untracked`).
  Staged new file = `added`; intent-to-add = staged `added` (empty blob) + unstaged `modified` [V p02o].
- `content_kind` per FILE, the max over its diffs with precedence
  `submodule`/`binary` > `unavailable` (non-UTF-8) > `symlink` > `text` [V p17c/d]. A diff is binary if either
  side has a NUL in its first 8000 bytes (libgit2/git rule; NUL at 9000 → text) [V p04c/d]; non-UTF-8 if
  either side is not valid UTF-8 [V p04g]. Symlink if either side has mode 120000.
- `inspectable` = every diff has a non-null patch (false for binary/unavailable/submodule) [V].
- `patch`: null for binary, non-UTF-8 and submodule diffs; empty string `""` for a nested repo (content_kind
  `text`, inspectable true) [V p03i].

---

## 2. Limitations  [V]

| code | path | reason (exact) | when |
|---|---|---|---|
| `invalid_review_base` | null | `Comparison base "<oid>" is not in HEAD's history` | review_base head not ancestor of HEAD, no `--base` [p05c, p06e] |
| `invalid_review_base` | null | `Comparison base requires a committed HEAD` | review_base head set but HEAD unborn [p18b/c] |
| `binary_content` | file | `Binary content is not content-verified` | per binary diff [p03a-c] |
| `non_utf8_content` | file | `Non-UTF-8 content is not content-verified` | per non-UTF-8 text diff [p03d/e] |
| `submodule_content` | file | `Submodule contents require separate inspection` | per submodule diff [p04j] |
| `missing_comparison_base` | null | `No validated pre-implementation base; provide --base explicitly. Current differences do not establish complete historical coverage.` | `--change` and no valid base |
| `approximated_attribution` | null | `Scope is approximated: trusted touched attribution is unavailable; this is not proof of hunk ownership.` | scope_source starts with `approximated_` |
| `preexisting_dirty` | file | `This path was dirty before task start; its hunks have only approximate attribution.` | `--change`, tracking has `review_base`, and the FILE's path is in `review_base.dirty_fingerprints` (emitted even if the file did not change since, even if review_base is invalid/null-head) [p05a, p05d, p06d, p18b] |

A review_base with `head_revision: null` (captured on unborn HEAD) gives only `missing_comparison_base`, no
`invalid_review_base` [V p05d].

Order [V p06f, p17c, p18b]:
1. `invalid_review_base`;
2. content limitations, **one per diff** (no dedup), grouped by diff kind (committed, staged, unstaged,
   untracked), path order within a kind — e.g. staged u(non_utf8), v, x, then unstaged t(non_utf8), u, v, x;
3. `missing_comparison_base`; 4. `approximated_attribution`; 5. `preexisting_dirty` in file order.

Strings present but never triggered: `unreadable_content` / `No inspectable patch was available`,
`Binary or non-UTF-8 content is not content-verified`, `Diff contains a path without an identity`,
`Cannot resolve rename identities: `, `Source type changed at ; refresh scope` (race), `Scope path is empty`,
`Scope path cannot be represented losslessly in JSON` / `Non-UTF-8 relative path` (macOS APFS refuses non-UTF-8
names, so untestable here).

---

## 3. Human output  [V]

stdout: `Scope: <scope_source> (<Status>)\n` where Status is `Empty`/`Resolved`/`Insufficient`, then one line per
FILE: `  <Rust Debug of path>: [<Kinds>]\n`, Kinds = Debug of the enum list, e.g. `[Staged, Unstaged]`,
`[Committed, Staged, Unstaged]`. Debug quoting: `"src/a b\tc.rs"`, `"src/ctl\u{1}.rs"`, `"src/q\"b\\s.rs"`,
non-ASCII printed raw (`"src/é中.rs"`). No old_path, no patches.
stderr: one line per limitation `<code>: <reason>\n` (path not printed). No colour even on a TTY; `--no-color`
changes nothing. Exit 0. Examples:
```
Scope: current_worktree (Empty)
Scope: approximated_worktree (Insufficient)        + stderr missing_comparison_base…, approximated_attribution…
Scope: touched_tracking (Resolved)\n  "src/a.rs": [Unstaged]\n
```

---

## 4. Patch format  [V p02, p03, p14, p15, p16, p20]

libgit2 patch output (`git_patch_to_buf`), one patch per delta.
- Prefixes: unstaged/untracked `i/` → `w/`; staged `c/` → `i/`; committed `c/` → `c/`. Controlled by repo config:
  `diff.mnemonicPrefix=false` → `a/`/`b/`; `diff.noprefix=true` → no prefix [V p16]. (So the oracle chooses the
  mnemonic prefixes itself and defaults `diff.mnemonicPrefix` to true.) [I on the exact config logic]
- Context 3, function-context hunk headers (`@@ -29,7 +28,7 @@ fn beta() {`) — equal to git. `diff.context`,
  `diff.interHunkContext`, `diff.algorithm`, `diff.renames` are **ignored** [V p16].
- `index <old7>..<new7> <mode>`; abbrev is **always 7** (no uniqueness extension; git prints 8 on a 7-hex
  collision — verified divergence p20a); `core.abbrev=12` is honoured [V p16].
- `\ No newline at end of file`, CRLF bytes preserved, `old mode/new mode` (mode-only: no index line; mode+content:
  `index a..b` without mode), `deleted file mode`, `new file mode`, `--- /dev/null`, rename header
  `similarity index N%\nrename from X\nrename to Y\n`; quoted paths with octal escapes (`"i/src/caf\303\251 \"q\".rs"`).
  `core.filemode=false` hides mode changes; `core.autocrlf=true` filters worktree content (untracked index oid is
  the filtered blob) [V p16].
- Untracked: `diff --git i/P w/P\nnew file mode 100644\nindex 0000000..<abbrev>\n--- /dev/null\n+++ w/P\n@@ -0,0 +1,N @@\n...`.

Byte-equality with git CLI (git 2.x, `GIT_CONFIG_GLOBAL=/dev/null`):
- unstaged == `git -c diff.mnemonicPrefix=true diff`; staged == `git -c diff.mnemonicPrefix=true diff --cached`;
  committed == `git diff --src-prefix=c/ --dst-prefix=c/ <base> HEAD`; untracked == intent-to-add in a temp
  index (`GIT_INDEX_FILE=tmp git add -N` + mnemonic `git diff`). EQUAL for: modify, multi-hunk, func context,
  no-EOL, CRLF, chmod, chmod+edit, delete, staged delete, typechange split, symlink, 100% staged rename, committed
  rename/chmod, tab-in-name quoting [V p14, p15, p16a].
- **Divergences from git** [V]: (1) empty added file (untracked and staged): libgit2 still emits
  `--- /dev/null\n+++ w/P\n` (git omits; the empty *deleted* case is [I]); (2) path containing a space: git appends `\t` to `---`/`+++` lines, libgit2 does not;
  (3) intent-to-add: libgit2 = staged `added` empty blob + unstaged `modified`, git shows one new-file diff;
  (4) worktree rename (`D` + `??`) detected by libgit2 (`similarity index 100%`); (5) rename similarity differs —
  a 50%-similar pair is a rename in libgit2 (`similarity index 50%`), delete+add in git; (6) abbrev collision;
  (7) nested repo → `patch: ""`.

---

## 5. snapshot_id  [V p08, p09, p11, p12, p13b, p24]

`snapshot_id = git blob id` (`sha1("blob <len>\0" + P)`, i.e. `Oid::hash_object(Blob, P)`) of P = compact JSON
array (serde, no spaces):

```
[ change|null, base_arg|null, base_candidate|null, head|null, index_id|null, tracking_id|null, entries ]
```
- `change`: `--change` value as given; `base_arg`: `--base` string **as given** (so `HEAD` ≠ full oid).
- `base_candidate`: explicit base → its peeled oid (null if it fails to resolve, which can only happen in check
  mode); else, with `--change`, `review_base.head_revision` **raw** (even when invalid); else null. No
  ancestry check here.
- `head`: HEAD oid or null (unborn).
- `index_id`: blob id of the raw bytes of `.git/index` (null when no index file). Hence any index rewrite
  (read-tree, refresh stat data, stage) changes the id even with identical content; fresh jails with equal
  content differ; mtime-only/chmod-only worktree changes do not [V p08b, p09].
- `tracking_id`: blob id of the raw bytes of `.spectra/touched/<change>.json` when `--change` given and the file
  exists, else null.
- `entries`: `[[path, status, worktree_identity], ...]` from the libgit2 status list over the **whole**
  worktree (project subtree when nested; paths project-relative), sorted by path; **no** spec-dir/.spectra
  exclusion (but ignored files absent), **no** rename detection (`src/a.rs` 512 + `src/z.rs` 128),
  **submodules included** (`["mod",256,null]`), nested repo `["inner",128,null]`. `status` = git_status_t bits
  as in task.md; `worktree_identity` = task.md's fingerprint worktree identity (raw-byte blob id,
  `symlink:<oid>`, null for deleted/dir/submodule). Index identity is **not** included.
- **Touched paths** (only with `--change` and a tracking file): every string in the
  tracking file's `touched[].files` that is not already in the status list is added with
  status `0` (`GIT_STATUS_CURRENT`) and its current worktree identity (null when deleted),
  then the whole list is sorted. So rewriting an already-committed touched file still
  invalidates the snapshot. Holds for an untouched-since-start path, a deleted path, and an
  invalid review_base alike; committed paths that are not touched are **not** added, and
  neither are the committed paths of an explicit base [V p26a-f; found by the differential
  run on p05b].

Example (clean, no args): `[null,null,null,"ab81de440d47494d78249705771d9f5f585e7084","a5cebf26b457d5c2462aae68fb58bd0cc075385f",null,[]]`
→ `79d5324bc449ed6ee210016543e92aa81cd6e0eb`. Forged ids computed in Python from this formula are accepted by
`--check-snapshot` [V p24]. Not time-dependent; not path-dependent (moving the repo keeps the id) [V p09a].

---

## 6. `--check-snapshot <ID>`  [V p07a, p08a, p13c, p18f, p23, p24]

Recomputes P (§5) with the given `--change`/`--base` ("reuse its change/base arguments" = the caller must pass
the same ones) and compares with `<ID>` as an exact, case-sensitive string. Nothing is stored anywhere; no
`.spectra/` state is read besides the tracking file.
- match: human `Scope snapshot is current: <ID>\n`; `--json` → `{"snapshot_id":"<ID>","status":"current"}\n`
  (compact); exit 0.
- mismatch (incl. malformed/empty/upper-case ID): stderr
  `Error: Scope changed since capture; discard the old snapshot and refresh scope before reporting\n`, exit 1,
  same with `--json`.
- Check mode does **not** check change existence nor resolve/validate the base as errors: a nonexistent change or
  an unresolvable base just feeds the preimage (→ normally a mismatch). Still fatal (same messages as §0):
  git unavailable, change-ID syntax, tracking parse/mismatch, unreadable/unsupported source.

---

## 7. Read-only  [V p08a, p17g]
Whole jail tree including `.git/` (size, mtime_ns, sha1 per file) unchanged across `scope`, `--change`,
`--base`, `--check-snapshot`, and human runs, including with stale index stat data (no index refresh written).

---

## 8. Open / inferred
- Exact config lookup for prefixes (probably `diff.noprefix` then `diff.mnemonicPrefix` default true) [I].
- Behaviour on non-UTF-8 paths (APFS rejects them); `Scope path cannot be represented losslessly in JSON` path.
- Triggers for `unreadable_content`, `Source type changed`, `Cannot resolve rename identities`.
- `content_kind` precedence between `symlink` and `unavailable` in the same file, and `submodule` vs others,
  only partially probed.
- `.gitattributes` (`-diff`, `binary`, custom drivers, `text eol=`) and rename-limit behaviour not probed.
- Whether committed diffs in tracking modes use rename detection across base..HEAD for matching (p05b/p15m show
  renames rendered; touched matching by old_path verified only for staged renames).
- Linked-worktree mapping to the main checkout is a project-root rule shared with other commands [I].

---

## 9. OpenSpectra implementation

`crates/spectra-core/src/scope.rs` (`capture`, `check`, `render_human`) and the
`Scope` subcommand in `crates/spectra-cli/src/main.rs`. D8
(`docs/migration-plan.md`) ruled to keep the **git CLI** rather than add the
`git2` crate, so every libgit2 call above is re-expressed with `git`:

| oracle (libgit2) | OpenSpectra (git CLI) |
|---|---|
| status list for snapshot entries | `git status --porcelain=v2 -z --no-renames --untracked-files=all --ignore-submodules=none`, bits mapped as in `task.md` (`fingerprint::snapshot_entries`) |
| committed / staged / unstaged deltas | `git diff --raw -z --no-abbrev -M50%` (`<base> <head>` / `--cached` / none), typechange `T` split into deleted + added |
| untracked deltas and patches | `git add -N` into a **copy** of the index (`GIT_INDEX_FILE`) plus a scratch object directory, then the unstaged `git diff` against that index |
| `git_patch_to_buf` | `git diff --no-ext-diff --no-color --no-textconv -U3 --inter-hunk-context=0 --diff-algorithm=myers --abbrev=<core.abbrev or 7>` with the mnemonic prefixes of §4, then `libgit2_headers` / `typechange_section` post-processing |
| `Oid::hash_object` | `git hash-object --no-filters --stdin` |

Read-only (§7) needed two measures that the oracle gets for free:

- Every git call runs with `GIT_OPTIONAL_LOCKS=0` (no opportunistic index
  refresh) and `-c gc.auto=0 -c maintenance.auto=false`.
- That is not enough for `git diff` itself: an index-to-worktree `git diff`
  writes refreshed stat data back to the index (`refresh_index_quietly`)
  regardless of `GIT_OPTIONAL_LOCKS` when the stat data is stale. So every
  unstaged diff (raw and patch) runs against a copy of the index
  (`GIT_INDEX_FILE`), and the snapshot's `index_id` still hashes the untouched
  real index. Caught by the differential run on p17g; pinned by
  `stale_index_stat_is_not_written_back`.
- `git add -N` writes the empty blob. Even with `GIT_OBJECT_DIRECTORY` pointed
  at a scratch directory, git *freshens* (utime) an object it finds in an
  alternate, and the empty blob is usually already present (any committed
  empty file, e.g. `.gitkeep`). So `add -N` runs with the scratch object
  directory and **no** alternates; only the later read-only `git diff` gets
  `GIT_ALTERNATE_OBJECT_DIRECTORIES` pointing at the real object store.
  Caught by the differential run (`.git/objects/e6/9de29…` mtime changed);
  pinned by `scope_is_read_only_even_with_untracked_files`.

Post-processing that brings git's patch text to libgit2's:

- `typechange_section`: for a typechange git prints the deleted and the new
  section in one patch; the oracle gives each of its two deltas only its own
  section.
- `libgit2_headers`: strips the trailing `\t` git appends to `---`/`+++` for
  paths containing a space, and adds `--- /dev/null` / `+++ <dst><path>` for an
  empty added file. The empty *deleted* file is unverified on the oracle and
  left as git prints it.

## 10. Deliberate divergences

Found by the differential run (§11); each is a D8 consequence or an earlier
ruling, not an oversight.

| Case | Oracle | OpenSpectra | Why |
|---|---|---|---|
| worktree rename (`D` + `??`) | one `renamed` diff, kinds `[unstaged, untracked]` | `deleted` + `untracked` | D8: `git diff` has no rename detection between index and untracked files |
| intent-to-add entry | staged `added` (empty blob) + unstaged `modified` | one unstaged `added` | D8: git renders ITA as a new file |
| rename similarity near 50% | `renamed` (`similarity index 50%`) | `deleted` + `added` | D8: git and libgit2 score similarity differently |
| abbrev collision | always 7 hex | git extends to stay unique | D8 |
| legacy tracking with numeric `task_id` | `Failed to parse touched tracking …: invalid type: integer` | accepted | W4 kept reading tracking files written by older OpenSpectra (`touched.rs`, `task_id_string`) |
| project root marked only by an `openspec/` directory | treated as initialized (`spec_dir: openspec`) | `Not initialized` | cross-command root discovery, not scope-specific; queued under W14 |

Non-UTF-8 paths could not be probed (APFS rejects them), so that branch has no
oracle evidence either way.

## 11. Verification

- `crates/spectra-cli/tests/scope_integration.rs`: base tree, identity and
  dates equal the RE harness, so HEAD is the oracle's `ab81de44…`; expected
  strings are oracle observations. `snapshot_id` is recomputed independently
  from the §5 formula with `git hash-object`, because the index bytes differ
  per repo. Each regression test was mutation-checked (the fix removed, the
  test fails).
- Differential run: the RE scenario scripts `p01`–`p25` replayed with every
  `spectra scope` invocation executed by both binaries on the same jail,
  comparing exit code, stdout and stderr byte-for-byte, and snapshotting the
  whole jail (including `.git/`: size, mtime, content) around the OpenSpectra
  run. Final run (2026-09-28): 244 invocations, 228 byte-identical; the 16
  that differ are all §10 rows (10 D8 edge cases, 4 legacy numeric
  `task_id`, 2 `openspec/`-only project root), and OpenSpectra changed
  nothing in any jail. The non-UTF-8 path scenario could not be built (APFS).
  The run found and fixed, before this write-up: the snapshot's touched-path
  entries (§5), both read-only leaks (§9), the typechange and empty-file
  patch headers, and the unreadable-untracked and directory-symlink errors.
