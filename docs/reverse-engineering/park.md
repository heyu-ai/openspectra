# Reverse-engineering `spectra park` / `unpark` / `list --parked`

How the closed-source `spectra` puts a change on hold, and how OpenSpectra
reproduces it.

> Source binary: `Spectra.app/Contents/MacOS/spectra` v2.3.1 (arm64 Mach-O).
> Everything below was pinned by running the binary against throwaway git
> repositories and reading the resulting on-disk state.

## Parking is a move, not a flag

The oracle relocates the **entire change directory** out of the working tree:

```text
<spec_dir>/changes/<name>/   →   <git common dir>/spectra-app/changes/<name>/
```

Nothing is left behind under `<spec_dir>/changes/`, and no marker file is
written. `unpark` moves the directory back.

The store lives beside the oracle's SQLite state (`spectra-app/spectra.db`),
but reading it needs **no database access** — parked changes are plain
directories with the same layout they had while active.

### It is the *common* git dir

Probed from a linked worktree: `park` writes to the shared `.git/spectra-app/`
of the main checkout (`git rev-parse --git-common-dir`), not the worktree's own
`.git/worktrees/<name>/`. A change parked from a worktree is listed from the
main checkout and vice versa. OpenSpectra resolves the same way via
`git::common_dir`.

> OpenSpectra originally kept parked state as an empty
> `.spectra/changes/<name>.parked` marker with the change directory left in
> place. That is invisible to the oracle and blind to anything the oracle
> parked — `list --parked` returned `No parked changes.` on a repository with
> 15 of them (#118).

## Read paths

A parked change is **hidden from the listings but still addressable**:

| command | sees a parked change? |
|---------|----------------------|
| `list`, `list --json` | no |
| `validate` | no |
| `list --parked` | yes (only parked) |
| `status --change <name>` | yes |
| `show <name>` | yes |
| `drift <name>` | yes |
| `instructions <artifact> --change <name>` | yes |

So name resolution has to consult both stores; only the enumerating commands
are split by parked state. OpenSpectra does this in `change::resolve_change_dir`.

## Output contract

```console
$ spectra park my-change
Parked change: my-change

$ spectra park my-change --json
{"parked": "my-change"}

$ spectra unpark my-change
Unparked change: my-change

$ spectra unpark my-change --json
{"unparked": "my-change"}

$ spectra list --parked
Parked:
  • p-one [1/2] — first parked thing
  • p-two — second parked, no tasks      # no [x/y] column without tasks.md

$ spectra list --parked          # empty
No parked changes.
```

`list --parked --json` keys the array on **`parked`**, not `changes`:

```json
{"parked": [{"completedTasks": 1, "name": "p-one", "status": "parked",
             "summary": "first parked thing", "totalTasks": 2}]}
```

`status` is the literal `"parked"` for every entry. Probed: the same change
reporting `"done"` in `list --json` (all tasks ticked) reports `"parked"` once
parked, so completion never surfaces there.

The oracle emits parked entries in directory order; OpenSpectra sorts by name,
which is a superset of the observable contract (the oracle's order is not
stable to reproduce and no consumer can rely on it).

## Errors

| invocation | exit | stderr |
|------------|------|--------|
| `park <unknown>` | 1 | `Error: Change 'X' does not exist` |
| `park <already parked>` | 1 | `Error: Change 'X' does not exist` |
| `unpark <active change>` | 1 | `Error: Change 'X' is already active (not parked)` |
| `unpark <unknown>` | 1 | `Error: Change 'X' is not parked` |
| `park BadName` | 1 | `Error: Change ID 'BadName' must contain only lowercase letters, digits, and hyphens` |

Parking an already-parked change is *not* idempotent: because the directory has
left `changes/`, the second call is indistinguishable from parking something
that never existed, and the oracle says so.

The id charset the oracle enforces here is **looser than the kebab-case rule
elsewhere**: `park 2026-01-01-old` succeeds and the change shows up in
`list --parked`, so the archived-name prefix is not special to this command.
OpenSpectra therefore validates `park`/`unpark` ids against `^[a-z0-9-]+$`
rather than `CHANGE_NAME_RE`, and `list --parked` applies no archived-prefix
filter (`list` still needs one, because `changes/` also contains `archive/`).

## Deliberate divergences

Both are the same ruling OpenSpectra already applied to the hardened atomic
writes in `update` and `config`: diverge from the oracle only where the oracle
destroys data.

**1. No silent clobber.** If a change directory exists under both
`changes/<name>/` and the parked store, the oracle's `park` exits 0 and
replaces the parked copy. Probed directly: a parked `proposal.md` reading
`PARKED-ORIGINAL` read `ACTIVE-NAMESAKE` afterwards, with no warning and no
backup. OpenSpectra refuses:

```text
a parked change named 'X' already exists at <path>; unpark or remove it first
```

**2. `archive` is not a parkable name.** `spectra park archive` exits 0 in the
oracle and moves the whole `changes/archive/` tree — every archived change —
into the parked store. OpenSpectra rejects the name.

## Not reproduced

`spectra.db` and `.migrate.lock` under `spectra-app/` are the oracle's SQLite
state. OpenSpectra neither reads nor writes them; the parked directory listing
is sufficient for every observable behavior above.
