# Reverse-engineering `spectra in-progress add`

How the closed-source `spectra in-progress add` command records a write-only
marker, and how OpenSpectra reproduces its CLI-observable behavior.

> Source: `Spectra.app/Contents/MacOS/spectra` v2.3.1 (arm64 Mach-O). The
> behavior below was recovered by running the binary as a **golden oracle** in
> initialized and uninitialized scratch projects, capturing stdout, stderr,
> and exit status byte-for-byte, then inspecting its SQLite database before
> and after each command.

## CLI shape

`in-progress` is a nested subcommand family with one member:

```
spectra in-progress add <NAME>
```

There is no `--json` option. `spectra in-progress add <NAME> --json` is a clap
parse error and exits 2. There is also no removal command: `remove`, `rm`, and
`clear` are all unrecognized subcommands and exit 2.

In an initialized project, a successful add exits 0 and writes exactly zero
bytes to stdout. Repeating the command for the same name is idempotent and
remains silent. In an uninitialized directory, the command takes the ordinary
operational-error path and exits 1.

## Probe methodology and findings

Each probe began from a fresh scratch project. For state comparisons, the
same `list`, `list --parked`, `status`, `analyze`, and `show` commands were
captured before and after `in-progress add`, and their output and exit status
were compared. Separate probes used an existing active change, an already
marked change, a nonexistent name, a parked change, and an archived change.
The oracle's `.git/spectra-app/spectra.db` was inspected separately to confirm
that apparently silent calls had persisted state.

The probes established four load-bearing behaviors:

1. **Zero existence validation.** Adding an existing change and adding a name
   with no corresponding change both exit 0 without output. The nonexistent
   case still inserts a marker, so OpenSpectra intentionally preserves this
   "ghost-change" behavior instead of reusing `park`'s existence validation.
2. **No removal path.** `remove`, `rm`, and `clear` are parser errors rather
   than hidden aliases or operational commands. The marker is write-only.
3. **Archive leaves an orphan.** Archiving a marked change does not delete its
   row from the oracle database. No CLI read path exposes that orphan, and
   there is no CLI command to remove it.
4. **The `list --json` string is a name collision.** `list --json` derives a
   task status named `"in-progress"` unless a change has at least one task and
   all of them are done — a change with zero tasks reports `"in-progress"` too
   (`list_change_items` in `crates/spectra-cli/src/main.rs`). That derived
   string existed independently and is completely unrelated to the
   `in-progress add` marker. Adding the marker does not influence it.

The full read-path matrix was negative: human and JSON listings, parked
listings, status, analysis, and show output were unchanged after adding the
marker. Parked and in-progress are independent booleans in the oracle, so
both can coexist; adding either does not clear the other.

## Storage divergence

The oracle stores one `change_id` per marker in the SQLite table
`in_progress_change` inside `.git/spectra-app/spectra.db`. OpenSpectra does not
copy that internal mechanism. It writes an empty sidecar instead:

```
.spectra/changes/<name>.in-progress
```

This mirrors the existing `.spectra/changes/<name>.parked` convention and
avoids introducing SQLite solely for opaque state. Because the marker has no
read path, the choice of storage is not observable through the CLI.

## Deliberate OpenSpectra differences

The oracle creates a fake `.git/` directory when run in a non-git project as
a side effect of opening its database. OpenSpectra does not copy that side
effect; its sidecar lives under `.spectra/` and does not create `.git/`.

OpenSpectra also adds two defensive behaviors that were not established by
the oracle probes:

* `mark_in_progress` rejects names that are not a single safe path component,
  including traversal such as `../evil`. The oracle was not probed with
  traversal names; OpenSpectra is deliberately stricter at this security
  boundary.
* `clear_stale_sidecar_state` removes the `.in-progress` sidecar during
  archive and before recreating a same-named change. This differs from the
  oracle's orphan-on-archive behavior, but is consistent with OpenSpectra's
  existing defensive clearing of `.parked`, `.started`, and touched-file
  sidecars. A newly created change therefore cannot inherit opaque state from
  an older change with the same name.
