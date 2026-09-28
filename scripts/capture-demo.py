#!/usr/bin/env python3
"""Verify or recapture the `spectra demo` sample content from the Spectra 3.0.0 oracle.

This is a verification contract, not a printer (see CLAUDE.md). `demo` picks a
random `spx-<adjective>-<pokemon>` name and one of several themes, so the script
runs it ``--runs`` times (default 600) in a sandbox project and checks:

* every run's stdout has the pinned three-line shape;
* each theme's files (`proposal.md`, `design.md`, `tasks.md`,
  `specs/<theme>/spec.md`) are byte-identical across all of its runs, and the
  file set is exactly that (plus `.openspec.yaml`);
* `.openspec.yaml` is `schema: spec-driven` / `created: <date>` /
  `created_by: <identity>` even though the sandbox's config names another schema;
* the observed themes, adjectives and Pokemon equal the pinned sets below and
  the lists in `crates/spectra-core/src/demo.rs` (a set not fully observed means
  the sample was too small: raise ``--runs``);
* the captured files equal `crates/spectra-core/assets/demo/`.

Any mismatch exits 1 and keeps the sandbox. ``--write`` regenerates the assets
and then verifies them again.

Safety: every oracle run is wrapped in ``sandbox-exec`` with network access
denied, ``/usr/bin/open`` denied, and file writes allowed only inside the
sandbox directory (a positive control proves the write denial works). The
sandbox lives under ``$TMPDIR`` and the script refuses to run when any ancestor
holds an ``openspec`` entry or ``.spectra.yaml``: the oracle walks up to the
nearest such marker and would write the demo change there instead.

The oracle is macOS-only. ``--spectra-bin`` overrides ``SPECTRA_BIN``, which
overrides the standard application path.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import NoReturn

EXPECTED_VERSION = "3.0.0"
DEFAULT_BIN = "/Applications/Spectra.app/Contents/MacOS/spectra"
REPO = Path(__file__).resolve().parent.parent
ASSETS = REPO / "crates/spectra-core/assets/demo"
DEMO_RS = REPO / "crates/spectra-core/src/demo.rs"

THEMES = {
    "access-control", "audit-trail", "batch-export", "keyboard-macros",
    "real-time-sync", "smart-search", "snapshot-restore", "theme-engine",
}
ADJECTIVES = {
    "bold", "bright", "calm", "cool", "dark", "deep", "eager", "fast", "gentle", "happy",
    "keen", "light", "neat", "proud", "quick", "rare", "sharp", "tall", "vivid", "warm",
}
POKEMON = {
    "absol", "arcanine", "bulbasaur", "charizard", "charmander", "dragonite", "eevee",
    "gardevoir", "gengar", "gyarados", "jigglypuff", "lapras", "lucario", "mewtwo",
    "pikachu", "rayquaza", "snorlax", "squirtle", "togekiss", "umbreon",
}
IDENTITY = "Probe <probe@example.invalid>"
STDOUT_RE = re.compile(
    r"✓ Created demo change: spx-([a-z]+)-([a-z]+)\n  Theme: ([a-z-]+)\n  Path: (.+)\n"
)
META_RE = re.compile(
    r"schema: spec-driven\ncreated: \d{4}-\d{2}-\d{2}\ncreated_by: " + re.escape(IDENTITY) + r"\n"
)


def fail(msg: str) -> NoReturn:
    print(f"[FAIL] {msg}", file=sys.stderr)
    sys.exit(1)


def rust_list(name: str) -> set[str]:
    m = re.search(rf"pub const {name}: \[&str; \d+\] = \[(.*?)\];", DEMO_RS.read_text(), re.S)
    if not m:
        fail(f"{DEMO_RS}: cannot find `{name}`")
    return set(re.findall(r'"([a-z-]+)"', m.group(1)))


def rust_themes() -> set[str]:
    return set(re.findall(r'theme!\("([a-z-]+)"\)', DEMO_RS.read_text()))


def profile(root: Path) -> str:
    return (
        "(version 1)\n(allow default)\n(deny network*)\n"
        '(deny process-exec (literal "/usr/bin/open"))\n'
        '(deny file-write* (subpath "/Users") (subpath "/private/var/folders") '
        '(subpath "/private/tmp"))\n'
        f'(allow file-write* (subpath "{root}"))\n'
    )


def capture(binary: str, runs: int, root: Path) -> dict[str, dict[str, bytes]]:
    for ancestor in root.parents:
        if (ancestor / "openspec").exists() or (ancestor / ".spectra.yaml").exists():
            fail(f"{ancestor} holds a project marker; the oracle would write there")
    prof = root / "sandbox.sb"
    prof.write_text(profile(root))
    outside = root.parent / f"{root.name}-outside-probe"
    rc = subprocess.run(["sandbox-exec", "-f", str(prof), "/usr/bin/touch", str(outside)],
                        capture_output=True).returncode
    if rc == 0 or outside.exists():
        outside.unlink(missing_ok=True)
        fail("sandbox did not deny a write outside the sandbox directory")

    project = root / "project"
    (project / "openspec/changes/archive").mkdir(parents=True)
    (project / "openspec/specs").mkdir(parents=True)
    (project / ".spectra.yaml").write_text("spec_dir: openspec\n")
    (project / "openspec/config.yaml").write_text("schema: custom-x\n")
    env = {**os.environ, "NO_COLOR": "1", "GIT_CONFIG_NOSYSTEM": "1"}
    subprocess.run(["git", "init", "-q"], cwd=project, check=True, env=env)
    name, email = IDENTITY.removesuffix(">").split(" <")
    subprocess.run(["git", "config", "user.name", name], cwd=project, check=True, env=env)
    subprocess.run(["git", "config", "user.email", email], cwd=project, check=True, env=env)

    themes: dict[str, dict[str, bytes]] = {}
    adjectives, pokemon = set(), set()
    changes = project / "openspec/changes"
    for i in range(runs):
        p = subprocess.run(["sandbox-exec", "-f", str(prof), binary, "demo"], cwd=project,
                           capture_output=True, env=env)
        out = p.stdout.decode()
        m = STDOUT_RE.fullmatch(out)
        if p.returncode != 0 or not m:
            fail(f"run {i}: exit {p.returncode}, stdout {out!r}, stderr {p.stderr!r}")
        adj, mon, theme, path = m.groups()
        cdir = changes / f"spx-{adj}-{mon}"
        if Path(path) != cdir.resolve() and Path(path) != cdir:
            fail(f"run {i}: printed path {path} is not {cdir}")
        meta = (cdir / ".openspec.yaml").read_text()
        if not META_RE.fullmatch(meta):
            fail(f"run {i}: unexpected .openspec.yaml {meta!r}")
        files = {f.relative_to(cdir).as_posix(): f.read_bytes()
                 for f in sorted(cdir.rglob("*")) if f.is_file() and f.name != ".openspec.yaml"}
        expected = {"proposal.md", "design.md", "tasks.md", f"specs/{theme}/spec.md"}
        if set(files) != expected:
            fail(f"run {i}: theme {theme} wrote {sorted(files)}, expected {sorted(expected)}")
        if theme in themes and themes[theme] != files:
            fail(f"run {i}: theme {theme} content differs from an earlier run")
        themes.setdefault(theme, files)
        adjectives.add(adj)
        pokemon.add(mon)
        shutil.rmtree(cdir)
    for label, seen, pinned in [("themes", set(themes), THEMES),
                                ("adjectives", adjectives, ADJECTIVES),
                                ("pokemon", pokemon, POKEMON)]:
        if seen != pinned:
            fail(f"{label}: observed {sorted(seen ^ pinned)} differ from the pinned set "
                 f"(missing ones may need more --runs)")
    for label, rust, pinned in [("THEMES", rust_themes(), THEMES),
                                ("ADJECTIVES", rust_list("ADJECTIVES"), ADJECTIVES),
                                ("POKEMON", rust_list("POKEMON"), POKEMON)]:
        if rust != pinned:
            fail(f"demo.rs {label} differs from the pinned set: {sorted(rust ^ pinned)}")
    return themes


def on_disk() -> dict[str, dict[str, bytes]]:
    if not ASSETS.is_dir():
        return {}
    return {
        t.name: {f.relative_to(t).as_posix(): f.read_bytes()
                 for f in sorted(t.rglob("*")) if f.is_file()}
        for t in sorted(ASSETS.iterdir()) if t.is_dir()
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--spectra-bin", default=os.environ.get("SPECTRA_BIN", DEFAULT_BIN))
    ap.add_argument("--runs", type=int, default=600)
    ap.add_argument("--write", action="store_true", help="regenerate the assets, then verify")
    args = ap.parse_args()
    version = subprocess.run([args.spectra_bin, "--version"], capture_output=True,
                             text=True).stdout.strip()
    if f" {EXPECTED_VERSION} " not in f" {version} ":
        fail(f"expected spectra {EXPECTED_VERSION}, got {version!r}")

    root = Path(tempfile.mkdtemp(prefix="capture-demo-")).resolve()
    captured = capture(args.spectra_bin, args.runs, root)
    if args.write:
        shutil.rmtree(ASSETS, ignore_errors=True)
        for theme, files in captured.items():
            for rel, data in files.items():
                dest = ASSETS / theme / rel
                dest.parent.mkdir(parents=True, exist_ok=True)
                dest.write_bytes(data)
    if on_disk() != captured:
        fail(f"{ASSETS} differs from the oracle capture (sandbox kept at {root}); "
             "rerun with --write and review the diff")
    shutil.rmtree(root)
    print(f"[OK] {len(captured)} demo themes match the oracle ({args.runs} runs)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
