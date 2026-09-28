# `spectra demo` and `spectra feedback` (oracle 3.0.0)

Both commands were probed with the oracle inside `sandbox-exec` (W12 probes
p01, p02, p15, p18, p19): network access denied, `/usr/bin/open` denied, and —
for `demo` — file writes allowed only inside the probe project. Positive
controls confirmed each denial (`curl` failed, `open` failed, a write outside
the project failed).

**Probe hygiene.** The oracle treats the nearest ancestor holding an `openspec`
entry or `.spectra.yaml` as the project root. A first uninitialized-directory
probe ran under a scratch tree whose ancestor had an unrelated `openspec/`
directory, and `demo` wrote its change there instead of failing. Probe
uninitialized behaviour only under a directory whose ancestors carry no
marker; `scripts/capture-demo.py` refuses to run otherwise.

## `feedback <MESSAGE> [--body <BODY>]`

Local only. The oracle binary links no networking library (`otool -L`: libiconv,
Security, CoreFoundation, libz, libSystem), and with the network denied and
all file writes under `/Users`, `/private/var/folders` and `/private/tmp`
denied it still succeeds: it neither sends nor stores anything. It does not
need an initialized project.

```
Thank you for your feedback!
Message: <MESSAGE>
Details: <BODY>

To submit feedback, visit: https://github.com/kaochenlong/spectra-app/issues
```

`Details:` appears only with `--body`, printed verbatim (newlines kept). An
empty message is accepted. A missing message, an extra positional or `--json`
are clap errors (exit 2). On a terminal the first line is green (`\e[32m`) and
the whole `To submit feedback, visit: …` line dim (`\e[2m`).

OpenSpectra prints the same text, including the upstream issue URL. Whether an
OpenSpectra build should point somewhere else — and whether "Thank you for
your feedback!" is acceptable wording for a command that submits nothing — is
an open question for the maintainer, not a fidelity question.

## `demo`

Needs an initialized project (`Error: Not initialized. Run 'spectra init' to
initialize.`, exit 1). Writes only `<root>/<spec_dir>/changes/<name>/` — no
`.spectra/` state, unlike OpenSpectra's own `new change`, which also records a
`.started` baseline — and downloads nothing.

* **Name**: `spx-<adjective>-<pokemon>`, picked uniformly at random. 600
  sampled runs showed 20 adjectives and 20 Pokémon, listed in
  `crates/spectra-core/src/demo.rs`. An existing directory makes it pick again;
  after 20 attempts it fails with `Could not generate a unique change name
  after 20 attempts.` (from the binary's strings; not reproduced).
* **Theme**: one of eight, picked at random: `access-control`, `audit-trail`,
  `batch-export`, `keyboard-macros`, `real-time-sync`, `smart-search`,
  `snapshot-restore`, `theme-engine`.
* **Files**: `.openspec.yaml`, `proposal.md`, `design.md`, `tasks.md` and
  `specs/<theme>/spec.md`. Apart from `.openspec.yaml` the content depends only
  on the theme: it was byte-identical across every sampled run. The captures
  live in `crates/spectra-core/assets/demo/<theme>/` and are generated and
  verified by `scripts/capture-demo.py`; never edit them by hand.
* **`.openspec.yaml`**: `schema: spec-driven` — always, even when
  `config.yaml` names another schema — then `created: <local date>` and
  `created_by: <identity>`, with the identity taken as for `new change`.
* **Output** (exit 0):

  ```
  ✓ Created demo change: <name>
    Theme: <theme>
    Path: <root>/<spec_dir>/changes/<name>
  ```

  On a terminal only the `✓` is colored (green, `\e[32m`). An extra positional
  or `--json` is a clap error (exit 2).

The oracle reads the creator identity from the user's real git configuration
even when `GIT_CONFIG_GLOBAL` points elsewhere (it did so for `new change` as
well); OpenSpectra's shared helper runs `git config`, which honours the
override. This is a pre-existing difference shared with `new change`.

## Reproducing

`python3 scripts/capture-demo.py` (macOS, oracle 3.0.0; `--spectra-bin` or
`SPECTRA_BIN` to override the path) runs `demo` 600 times in a sandboxed
temporary project and fails if the output shape, the file set, a theme's
content, the observed name/theme sets, the lists in `demo.rs`, or the assets
drift. `--write` regenerates the assets and verifies them again.
