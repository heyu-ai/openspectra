#!/usr/bin/env python3
"""Verify or recapture OpenSpec 1.13.2 `validate` output on the rule fixture.

This is a verification contract, not a printer (see CLAUDE.md). Owner ruling
D1 makes OpenSpec 1.13.2 the authority for `spectra validate`'s rules, so the
expected findings for every rule case come from OpenSpec itself, never from
reading its source. The fixture project
``crates/spectra-cli/tests/fixtures/validate_openspec`` holds one change or
main spec per rule (``dNN-*`` changes, ``sNN-*`` specs; see
``docs/reverse-engineering/validate.md``). This script copies it into a
sandbox, runs

    openspec validate --changes|--specs --json [--strict] --no-interactive

for both scopes and both strictness modes, drops the non-deterministic
``durationMs``, and compares the result with
``docs/reverse-engineering/golden/validate-openspec-1.13.2.json``, which
``crates/spectra-cli/tests/validate_openspec_integration.rs`` replays against
``spectra validate``. Any difference exits 1 and keeps the sandbox;
``--write`` regenerates the golden file and then verifies it again.

Requires Node.js and the OpenSpec 1.13.2 package. ``--openspec-js`` overrides
``OPENSPEC_JS``, which overrides the npx cache path this was captured with.
The package version is checked before anything runs.
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

EXPECTED_VERSION = "1.13.2"
DEFAULT_JS = "/Users/howie/.npm/_npx/0aaef5be8686a8bb/node_modules/@fission-ai/openspec/bin/openspec.js"
REPO = Path(__file__).resolve().parent.parent
FIXTURE = REPO / "crates/spectra-cli/tests/fixtures/validate_openspec"
GOLDEN = REPO / "docs/reverse-engineering/golden/validate-openspec-1.13.2.json"
ENV = {
    **os.environ,
    "CI": "1",
    "OPENSPEC_TELEMETRY": "0",
    "DO_NOT_TRACK": "1",
    "NO_COLOR": "1",
}


def fail(msg: str) -> NoReturn:
    print(f"[FAIL] {msg}", file=sys.stderr)
    sys.exit(1)


def openspec(js: str, args: list[str], cwd: Path) -> str:
    p = subprocess.run(["node", js, *args], cwd=cwd, capture_output=True, text=True, env=ENV)
    # 有 invalid item 時 rc 為 1，這是正常結果；只有 stdout 不是 JSON 才算失敗。
    if p.returncode not in (0, 1):
        fail(f"`openspec {' '.join(args)}` exited {p.returncode}: {p.stderr.strip()}")
    return p.stdout


def capture(js: str, sandbox: Path) -> dict:
    project = sandbox / "project"
    shutil.copytree(FIXTURE, project)
    result: dict = {"openspec": EXPECTED_VERSION, "scopes": {}}
    for scope in ("changes", "specs"):
        result["scopes"][scope] = {}
        for mode in ("normal", "strict"):
            args = ["validate", f"--{scope}", "--json", "--no-interactive"]
            if mode == "strict":
                args.append("--strict")
            out = openspec(js, args, project)
            try:
                report = json.loads(out)
            except json.JSONDecodeError:
                fail(f"{scope}/{mode}: stdout is not JSON: {out[:300]!r}")
            items = report.get("items")
            if not isinstance(items, list) or not items:
                fail(f"{scope}/{mode}: no items in the report; the fixture was not discovered")
            result["scopes"][scope][mode] = sorted(
                (
                    {"id": item["id"], "valid": item["valid"], "issues": item["issues"]}
                    for item in items
                ),
                key=lambda item: item["id"],
            )
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    parser.add_argument("--openspec-js", default=os.environ.get("OPENSPEC_JS", DEFAULT_JS))
    parser.add_argument("--write", action="store_true", help="regenerate the golden file")
    args = parser.parse_args()

    if not Path(args.openspec_js).is_file():
        fail(f"OpenSpec CLI not found at {args.openspec_js}")
    version = openspec(args.openspec_js, ["--version"], REPO).strip()
    if version != EXPECTED_VERSION:
        fail(f"expected OpenSpec {EXPECTED_VERSION}, got {version!r}")
    if not FIXTURE.is_dir():
        fail(f"fixture missing: {FIXTURE}")

    sandbox = Path(tempfile.mkdtemp(prefix="capture-validate-openspec-"))
    captured = capture(args.openspec_js, sandbox)
    text = json.dumps(captured, ensure_ascii=False, indent=2) + "\n"
    if args.write:
        GOLDEN.write_text(text)
        print(f"wrote {GOLDEN.relative_to(REPO)}")
        sandbox2 = Path(tempfile.mkdtemp(prefix="capture-validate-openspec-"))
        again = json.dumps(capture(args.openspec_js, sandbox2), ensure_ascii=False, indent=2) + "\n"
        if again != GOLDEN.read_text():
            fail(f"a second capture differs from the one just written (sandbox kept: {sandbox2})")
        shutil.rmtree(sandbox2)
    elif not GOLDEN.is_file() or GOLDEN.read_text() != text:
        drift = sandbox / "captured.json"
        drift.write_text(text)
        fail(f"capture differs from {GOLDEN.relative_to(REPO)}; see {drift} (sandbox kept)")
    shutil.rmtree(sandbox)
    counts = {scope: len(modes["normal"]) for scope, modes in captured["scopes"].items()}
    print(f"[OK] OpenSpec {EXPECTED_VERSION} validate golden matches ({counts})")


if __name__ == "__main__":
    main()
