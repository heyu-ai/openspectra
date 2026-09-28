#!/usr/bin/env python3
"""Parity probe: compare OpenSpectra against the Spectra 3.0.0 oracle.

This is a verification contract, not a printer (see CLAUDE.md). It measures
acceptance criteria A1 and A3 of ``docs/migration-plan.md``:

* **A1 (surface)** -- every subcommand and long flag the oracle's ``--help``
  lists must also be accepted by OpenSpectra.
* **A3 (output)** -- on copies of real projects ("corpus"), the read-only
  commands must agree on exit code and on JSON *semantics* (key order does not
  matter; list order does).

Known divergences live in ``docs/reverse-engineering/golden/parity-known.tsv``
and work as a ratchet: a divergence that is not listed fails the run, and a
listed divergence that no longer occurs on any corpus also fails the run (so
the file can only shrink as the port catches up). ``--write-known`` rewrites
the file from the current observations; review its diff before committing.

Corpus projects are never touched: the Spectra-relevant files are copied into
a temporary git repository first. On failure the sandboxes are kept and their
paths printed.

The oracle is macOS-only. ``--oracle-bin`` overrides ``SPECTRA_BIN``, which
overrides the standard application path.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import NoReturn

EXPECTED_VERSION = "3.0.0"
DEFAULT_ORACLE = "/Applications/Spectra.app/Contents/MacOS/spectra"
REPO = Path(__file__).resolve().parent.parent
KNOWN_FILE = REPO / "docs/reverse-engineering/golden/parity-known.tsv"

# Subcommand paths whose --help flags are compared (A1). Leaf commands only.
SURFACE = [
    ["init"], ["update"], ["list"], ["show"], ["validate"], ["scope"],
    ["analyze"], ["drift"], ["archive"], ["decisions"], ["status"],
    ["instructions"], ["new", "change"], ["new", "artifact"], ["schemas"],
    ["templates"], ["feedback"], ["schema", "init"], ["schema", "fork"],
    ["schema", "validate"], ["schema", "which"], ["config", "path"],
    ["config", "list"], ["config", "get"], ["config", "set"],
    ["config", "unset"], ["config", "reset"], ["config", "edit"],
    ["completion"], ["park"], ["unpark"], ["task", "start"],
    ["task", "done"], ["in-progress", "add"], ["demo"],
]

# Keys whose values legitimately differ between two runs.
VOLATILE_KEYS = {"durationMs"}


def fail(msg: str) -> NoReturn:
    print(f"[FAIL] {msg}", file=sys.stderr)
    sys.exit(1)


def run(binary: str, args: list[str], cwd: Path | None = None) -> tuple[int, str, str]:
    p = subprocess.run(
        [binary, *args], cwd=cwd, capture_output=True, text=True,
        env={**os.environ, "NO_COLOR": "1"}, timeout=300,
    )
    return p.returncode, p.stdout, p.stderr


# ---------------------------------------------------------------- A1 surface

def help_flags(binary: str, path: list[str]) -> set[str] | None:
    rc, out, _ = run(binary, [*path, "--help"])
    if rc != 0:
        return None
    return set(re.findall(r"(?<![\w-])(--[a-z][a-z0-9-]*)", out)) - {"--help", "--version"}


def surface_divergences(oracle: str, oss: str) -> set[tuple[str, str, str]]:
    found = set()
    for path in SURFACE:
        name = " ".join(path)
        a, b = help_flags(oracle, path), help_flags(oss, path)
        if a is None:
            continue  # not an oracle command; OSS-only extras are fine
        if b is None:
            found.add(("surface", name, "missing-command"))
            continue
        for flag in sorted(a - b):
            found.add(("surface", name, f"missing-flag {flag}"))
    return found


# ----------------------------------------------------------------- A3 output

def flatten(obj, prefix=""):
    if isinstance(obj, dict):
        for k, v in obj.items():
            if k in VOLATILE_KEYS:
                continue
            yield from flatten(v, f"{prefix}.{k}")
    elif isinstance(obj, list):
        yield f"{prefix}[]#len", len(obj)
        for i, v in enumerate(obj):
            yield from flatten(v, f"{prefix}[{i}]")
    else:
        yield prefix, obj


def generic(path: str) -> str:
    return re.sub(r"\[\d+\]", "[]", path)


def compare_json(tag: str, a_text: str, b_text: str) -> set[tuple[str, str, str]]:
    try:
        a = json.loads(a_text)
    except json.JSONDecodeError:
        return {("output", tag, "oracle-non-json")}
    try:
        b = json.loads(b_text)
    except json.JSONDecodeError:
        return {("output", tag, "oss-non-json")}
    if a == b:
        return set()
    fa, fb = dict(flatten(a)), dict(flatten(b))
    ga = {generic(k) for k in fa}
    gb = {generic(k) for k in fb}
    found = set()
    for k in ga - gb:
        found.add(("output", tag, f"oracle-only {k}"))
    for k in gb - ga:
        found.add(("output", tag, f"oss-only {k}"))
    for k in fa.keys() & fb.keys():
        if fa[k] != fb[k]:
            found.add(("output", tag, f"value {generic(k)}"))
    return found


def compare_text(tag: str, a: str, b: str) -> set[tuple[str, str, str]]:
    return set() if a == b else {("output", tag, "text-differs")}


def read_spec_dir(project: Path) -> str:
    cfg = project / ".spectra.yaml"
    if not cfg.is_file():
        fail(f"{project}: no .spectra.yaml (corpus projects must be initialized)")
    m = re.search(r"^spec_dir:\s*(\S+)", cfg.read_text(), re.MULTILINE)
    return m.group(1) if m else "openspec"


def make_sandbox(project: Path, root: Path) -> Path:
    box = root / project.name
    box.mkdir(parents=True)
    spec_dir = read_spec_dir(project)
    for rel in [".spectra.yaml", "CLAUDE.md", "AGENTS.md"]:
        if (project / rel).is_file():
            shutil.copyfile(project / rel, box / rel)
    if (project / ".claude").is_dir():
        shutil.copytree(project / ".claude", box / ".claude", symlinks=True,
                        ignore=shutil.ignore_patterns("worktrees"))
    if not (project / spec_dir).is_dir():
        fail(f"{project}: spec_dir {spec_dir!r} does not exist")
    shutil.copytree(project / spec_dir, box / spec_dir, symlinks=True)
    git = ["git", "-C", str(box)]
    subprocess.run([*git, "init", "-q"], check=True)
    subprocess.run([*git, "add", "-A"], check=True)
    subprocess.run([*git, "-c", "user.email=probe@example.invalid", "-c", "user.name=probe",
                    "commit", "-qm", "corpus snapshot"], check=True)
    return box


def output_divergences(oracle: str, oss: str, box: Path) -> set[tuple[str, str, str]]:
    rc, out, err = run(oracle, ["list", "--json"], cwd=box)
    if rc != 0:
        fail(f"{box}: oracle list --json failed: {err.strip()}")
    changes = [c["name"] for c in json.loads(out).get("changes", [])]
    commands: list[tuple[str, list[str], bool]] = [
        ("list --json", ["list", "--json"], True),
        ("list --specs --json", ["list", "--specs", "--json"], True),
        ("list --parked --json", ["list", "--parked", "--json"], True),
        ("schemas --json", ["schemas", "--json"], True),
        ("templates --json", ["templates", "--json"], True),
        ("validate --changes --json", ["validate", "--changes", "--json"], True),
    ]
    for c in changes:
        commands += [
            ("show <change> --json", ["show", c, "--json"], True),
            ("status --change <change> --json", ["status", "--change", c, "--json"], True),
            ("instructions apply --change <change> --json",
             ["instructions", "apply", "--change", c, "--json"], True),
            ("analyze <change> --json", ["analyze", c, "--json"], True),
            ("drift <change> --json", ["drift", c, "--json"], True),
            ("validate <change>", ["validate", c], False),
        ]
    found = set()
    for tag, args, is_json in commands:
        ra, oa, ea = run(oracle, args, cwd=box)
        rb, ob, eb = run(oss, args, cwd=box)
        if ra != rb:
            found.add(("output", tag, f"exit {ra}->{rb}"))
        if "panicked" in eb:
            found.add(("output", tag, "oss-panic"))
        if is_json and ra == 0 and rb == 0:
            found |= compare_json(tag, oa, ob)
        elif not is_json and ra == rb:
            found |= compare_text(tag, oa, ob)
    return found


# ------------------------------------------------------------------ ratchet

def load_known() -> set[tuple[str, str, str]]:
    if not KNOWN_FILE.is_file():
        return set()
    rows = set()
    for line in KNOWN_FILE.read_text().splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) != 3:
            fail(f"{KNOWN_FILE}: malformed row {line!r}")
        rows.add((parts[0], parts[1], parts[2]))
    return rows


def write_known(rows: set[tuple[str, str, str]]) -> None:
    header = (
        "# Known OpenSpectra-vs-oracle 3.0.0 divergences (ratchet; see scripts/parity-probe.py).\n"
        "# Generated by `scripts/parity-probe.py --write-known`; review the diff, do not hand-edit.\n"
        "# kind\tsubject\tdivergence\n"
    )
    body = "".join(f"{k}\t{s}\t{d}\n" for k, s, d in sorted(rows))
    KNOWN_FILE.write_text(header + body)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--oracle-bin", default=os.environ.get("SPECTRA_BIN", DEFAULT_ORACLE))
    ap.add_argument("--oss-bin", default=str(REPO / "target/release/spectra"))
    ap.add_argument("--corpus", action="append", default=[], type=Path,
                    help="initialized project to compare on (repeatable; copied, never modified)")
    ap.add_argument("--surface-only", action="store_true", help="skip corpus output comparison")
    ap.add_argument("--write-known", action="store_true", help="rewrite the known-divergence file")
    args = ap.parse_args()

    for label, binary in (("oracle", args.oracle_bin), ("oss", args.oss_bin)):
        if not Path(binary).is_file():
            fail(f"{label} binary not found: {binary}")
    rc, ver, _ = run(args.oracle_bin, ["--version"])
    if rc != 0 or EXPECTED_VERSION not in ver:
        fail(f"oracle must be {EXPECTED_VERSION}, got {ver.strip()!r}")
    if not args.surface_only and not args.corpus:
        fail("no --corpus given (use --surface-only to check only the CLI surface)")

    observed = surface_divergences(args.oracle_bin, args.oss_bin)
    print(f"[INFO] surface: {len(observed)} divergence(s)")

    keep = False
    tmp = Path(tempfile.mkdtemp(prefix="parity-probe-"))
    try:
        for project in args.corpus:
            box = make_sandbox(project.resolve(), tmp)
            found = output_divergences(args.oracle_bin, args.oss_bin, box)
            print(f"[INFO] {project.name}: {len(found)} output divergence(s)")
            observed |= found

        if args.write_known:
            write_known(observed)
            print(f"[OK] wrote {len(observed)} row(s) to {KNOWN_FILE.relative_to(REPO)}")
            return 0

        known = load_known()
        new = sorted(observed - known)
        # Output rows can only be judged stale when corpora were compared.
        stale = sorted(r for r in known - observed if r[0] == "surface" or args.corpus)
        for r in new:
            print(f"[FAIL] new divergence: {r[0]}\t{r[1]}\t{r[2]}")
        for r in stale:
            print(f"[FAIL] stale known divergence (fixed? remove it): {r[0]}\t{r[1]}\t{r[2]}")
        if new or stale:
            keep = True
            print(f"[FAIL] sandboxes kept at {tmp}")
            return 1
        print(f"[OK] {len(observed)} divergence(s), all known")
        return 0
    finally:
        if not keep:
            shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
