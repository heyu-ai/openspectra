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

It also pins the whole ``--json`` envelope for ``--format openspec`` (W9a,
ruling D3): ``ENVELOPES`` runs a list of invocations on
``tests/fixtures/validate_openspec_envelope`` (mixed-case, punctuated and
nested ids, a change and a spec sharing a name) and
``tests/fixtures/validate_openspec_empty`` (nothing to validate), and on the
rule fixture only the ``--all`` item order and summary. Each stdout is kept as
text (key order matters), after three normalizations the replay applies too:
``durationMs`` values become 0, the sandbox path becomes ``<ROOT>``, and a
change and a spec with the same id are put change-first (OpenSpec orders such
a tie by async completion, which is not deterministic).

Requires Node.js and the OpenSpec 1.13.2 package. ``--openspec-js`` overrides
``OPENSPEC_JS``, which overrides the npx cache path this was captured with.
The package version is checked before anything runs.
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

EXPECTED_VERSION = "1.13.2"
DEFAULT_JS = "/Users/howie/.npm/_npx/0aaef5be8686a8bb/node_modules/@fission-ai/openspec/bin/openspec.js"
REPO = Path(__file__).resolve().parent.parent
FIXTURES = REPO / "crates/spectra-cli/tests/fixtures"
FIXTURE = FIXTURES / "validate_openspec"
GOLDEN = REPO / "docs/reverse-engineering/golden/validate-openspec-1.13.2.json"
ENV = {
    **{k: v for k, v in os.environ.items() if k != "FORCE_COLOR"},
    "CI": "1",
    "OPENSPEC_TELEMETRY": "0",
    "DO_NOT_TRACK": "1",
    "NO_COLOR": "1",
}
# (fixture, validate 的參數)。每個都另加 `--json --no-interactive`；重播端改加
# `--json --format openspec`。
ENVELOPES = [
    ("validate_openspec_envelope", ["--all"]),
    ("validate_openspec_envelope", ["--changes"]),
    ("validate_openspec_envelope", ["--specs"]),
    ("validate_openspec_envelope", ["--changes", "--specs"]),
    ("validate_openspec_envelope", ["--all", "--report", "findings"]),
    ("validate_openspec_envelope", ["--changes", "--specs", "--report", "findings"]),
    ("validate_openspec_envelope", ["--changes", "--report", "findings"]),
    ("validate_openspec_envelope", ["--specs", "--report", "full"]),
    ("validate_openspec_envelope", ["zeta"]),
    ("validate_openspec_envelope", ["alpha"]),
    ("validate_openspec_envelope", ["a/b"]),
    ("validate_openspec_envelope", ["only"]),
    ("validate_openspec_envelope", ["only", "--type", "spec"]),
    ("validate_openspec_envelope", ["nope"]),
    # 帶 `line` 的 issue：key 順序是 level, path, line, message。
    ("validate_openspec", ["d13-strayh3"]),
    ("validate_openspec_empty", ["--all"]),
    ("validate_openspec_empty", ["--changes"]),
    ("validate_openspec_empty", ["--specs"]),
    ("validate_openspec_empty", ["--all", "--report", "findings"]),
]
DURATION = re.compile(r'"durationMs": \d+')


def fail(msg: str) -> NoReturn:
    print(f"[FAIL] {msg}", file=sys.stderr)
    sys.exit(1)


def openspec_run(js: str, args: list[str], cwd: Path) -> subprocess.CompletedProcess:
    p = subprocess.run(["node", js, *args], cwd=cwd, capture_output=True, text=True, env=ENV)
    # 有 invalid item 時 rc 為 1，這是正常結果；只有 stdout 不是 JSON 才算失敗。
    if p.returncode not in (0, 1):
        fail(f"`openspec {' '.join(args)}` exited {p.returncode}: {p.stderr.strip()}")
    return p


def openspec(js: str, args: list[str], cwd: Path) -> str:
    return openspec_run(js, args, cwd).stdout


def change_first_on_ties(items: list) -> list:
    """同 id 的相鄰 item 排成 change 在前（OpenSpec 依 async 完成順序排這種平手）。"""
    out = list(items)
    for i in range(len(out) - 1):
        a, b = out[i], out[i + 1]
        if a["id"] == b["id"] and a["type"] == "spec" and b["type"] == "change":
            out[i], out[i + 1] = b, a
    return out


def normalize_envelope(stdout: str, project: Path) -> str:
    """把 `--json` 的 stdout 正規化成重播端會產生的同一份文字（見模組說明）。"""
    try:
        report = json.loads(stdout)
    except json.JSONDecodeError:
        fail(f"envelope stdout is not JSON: {stdout[:300]!r}")
    for key in ("items", "itemFindings"):
        if isinstance(report.get(key), list):
            report[key] = change_first_on_ties(report[key])
    text = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    text = DURATION.sub('"durationMs": 0', text)
    roots = {str(project), str(project.resolve())}
    for root in sorted(roots, key=len, reverse=True):
        text = text.replace(json.dumps(root), json.dumps("<ROOT>"))
    if "<ROOT>" not in text and '"root"' in text:
        fail(f"root path was not normalized in: {text[:300]!r}")
    return text


def capture_envelopes(js: str, sandbox: Path) -> list:
    envelopes = []
    copies: dict[str, Path] = {}
    for fixture, args in ENVELOPES:
        if fixture not in copies:
            copies[fixture] = sandbox / fixture
            shutil.copytree(FIXTURES / fixture, copies[fixture])
        project = copies[fixture]
        p = openspec_run(js, ["validate", *args, "--json", "--no-interactive"], project)
        envelopes.append({
            "fixture": fixture,
            "args": args,
            "rc": p.returncode,
            "stdout": normalize_envelope(p.stdout, project),
            "stderr": p.stderr,
        })
    return envelopes


def capture_rule_fixture_order(js: str, project: Path) -> dict:
    """規則 fixture 上 `--all` 的 item 順序與 summary（item 內容已在 scopes 逐欄比對）。"""
    report = json.loads(openspec(js, ["validate", "--all", "--json", "--no-interactive"], project))
    items = change_first_on_ties(report["items"])
    return {
        "order": [f"{item['type']}/{item['id']}" for item in items],
        "summary": report["summary"],
    }


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
    result["all_order"] = capture_rule_fixture_order(js, project)
    result["envelopes"] = capture_envelopes(js, sandbox)
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
