#!/usr/bin/env python3
"""Verify or recapture built-in workflow schemas from the Spectra 3.0.0 oracle.

This is a verification contract, not a printer (see CLAUDE.md). For each
schema in ``SCHEMAS`` it builds a sandbox project, creates a change with that
schema, and reads everything the oracle exposes about it:

* ``schemas --json`` -- the one-line description and the listing's artifact order;
* ``status --change <c> --json`` -- artifact ids in dependency order and
  ``applyRequires``;
* ``instructions <artifact> --change <c> --json`` -- each artifact's output
  path, description, dependency ids, instruction, and template;
* ``instructions apply --change <c> --json`` -- the apply instruction.

The sandbox's ``spec_dir`` is the sentinel ``__SPECDIR__``: the oracle renders
the ``{{SPEC_DIR}}`` placeholder in instruction text as ``<spec_dir>/``, so
every ``__SPECDIR__/`` in the output is mapped back to the placeholder, and a
leftover sentinel anywhere else fails the capture.

By default the capture is compared with ``crates/spectra-core/assets/schemas/
<name>-3.0.0.json`` and any difference exits 1, keeping the sandbox.
``--write`` regenerates the asset files and then verifies them again.

The oracle is macOS-only. ``--spectra-bin`` overrides ``SPECTRA_BIN``, which
overrides the standard application path.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import NoReturn

EXPECTED_VERSION = "3.0.0"
DEFAULT_BIN = "/Applications/Spectra.app/Contents/MacOS/spectra"
REPO = Path(__file__).resolve().parent.parent
ASSETS = REPO / "crates/spectra-core/assets/schemas"
SENTINEL = "__SPECDIR__"
SCHEMAS = ("no-spec",)


def fail(msg: str) -> NoReturn:
    print(f"[FAIL] {msg}", file=sys.stderr)
    sys.exit(1)


def run(binary: str, args: list[str], cwd: Path) -> str:
    p = subprocess.run([binary, *args], cwd=cwd, capture_output=True, text=True,
                       env={**os.environ, "NO_COLOR": "1"})
    if p.returncode != 0:
        fail(f"`spectra {' '.join(args)}` exited {p.returncode}: {p.stderr.strip()}")
    return p.stdout


def unrender(text: str, where: str) -> str:
    text = text.replace(f"{SENTINEL}/", "{{SPEC_DIR}}")
    if SENTINEL in text:
        fail(f"{where}: sentinel spec_dir appears outside a {{{{SPEC_DIR}}}}/ position")
    return text


def capture(binary: str, name: str, root: Path) -> dict:
    project = root / name
    (project / SENTINEL / "changes" / "archive").mkdir(parents=True)
    (project / SENTINEL / "specs").mkdir(parents=True)
    (project / ".spectra.yaml").write_text(f"spec_dir: {SENTINEL}\n")
    (project / SENTINEL / "config.yaml").write_text("")
    subprocess.run(["git", "-C", str(project), "init", "-q"], check=True)
    run(binary, ["new", "change", "demo", "--schema", name], project)

    listing = [s for s in json.loads(run(binary, ["schemas", "--json"], project)) if s["name"] == name]
    if len(listing) != 1 or listing[0]["source"] != "package":
        fail(f"{name}: expected exactly one package schema in `schemas --json`, got {listing}")
    status = json.loads(run(binary, ["status", "--change", "demo", "--json"], project))
    if status["schemaName"] != name:
        fail(f"{name}: change resolved to schema {status['schemaName']!r}")

    artifacts = []
    for entry in status["artifacts"]:
        aid = entry["id"]
        data = json.loads(run(binary, ["instructions", aid, "--change", "demo", "--json"], project))
        if "context" in data or "rules" in data:
            fail(f"{name}/{aid}: sandbox config must not add context or rules")
        artifacts.append({
            "id": aid,
            "outputPath": data["outputPath"],
            "description": data["description"],
            "deps": [d["id"] for d in data["dependencies"]],
            "instruction": unrender(data["instruction"], f"{name}/{aid} instruction"),
            "template": unrender(data["template"], f"{name}/{aid} template"),
        })

    # apply 需要 applyRequires 的 artifact 存在才會進入 apply 模式。
    change_dir = project / SENTINEL / "changes" / "demo"
    for aid in status["applyRequires"]:
        path = next(a["outputPath"] for a in artifacts if a["id"] == aid)
        (change_dir / path).write_text("- [ ] 1.1 task\n")
    apply = json.loads(run(binary, ["instructions", "apply", "--change", "demo", "--json"], project))

    return {
        "name": name,
        "description": listing[0]["description"],
        "artifactOrder": listing[0]["artifacts"],
        "applyRequires": status["applyRequires"],
        "applyInstruction": unrender(apply["instruction"], f"{name}/apply instruction"),
        "artifacts": artifacts,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--spectra-bin", default=os.environ.get("SPECTRA_BIN", DEFAULT_BIN))
    ap.add_argument("--write", action="store_true", help="regenerate the asset files")
    args = ap.parse_args()

    if not Path(args.spectra_bin).is_file():
        fail(f"oracle binary not found: {args.spectra_bin}")
    ver = subprocess.run([args.spectra_bin, "--version"], capture_output=True, text=True).stdout
    if EXPECTED_VERSION not in ver:
        fail(f"oracle must be {EXPECTED_VERSION}, got {ver.strip()!r}")

    tmp = Path(tempfile.mkdtemp(prefix="spectra-schemas-"))
    keep = False
    try:
        for name in SCHEMAS:
            captured = capture(args.spectra_bin, name, tmp)
            text = json.dumps(captured, indent=2, ensure_ascii=False) + "\n"
            asset = ASSETS / f"{name}-{EXPECTED_VERSION}.json"
            if args.write:
                ASSETS.mkdir(parents=True, exist_ok=True)
                asset.write_text(text)
            if not asset.is_file() or asset.read_text() != text:
                keep = True
                fail(f"{asset.relative_to(REPO)} differs from the oracle capture (sandbox kept at {tmp})")
            print(f"[OK] {name}: {len(captured['artifacts'])} artifacts match the oracle")
        return 0
    finally:
        if not keep:
            shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
