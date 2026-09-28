#!/usr/bin/env python3
"""Verify or recapture the `spectra schema validate` / `schema fork` golden from the 3.0.0 oracle.

This is a verification contract, not a printer. Every directory under
`crates/spectra-cli/tests/fixtures/schema_validate/<case>/` is one project
schema (the W7g probe inputs: a valid control, single-fault and multi-fault
schemas, CRLF, BOM, ...). For each case a fresh scratch project is built
(`.spectra.yaml`, `openspec/{config.yaml,changes/archive,specs}`, the case
copied to `openspec/schemas/m/`) and the reference binary runs:

    schema validate m | schema validate m --json | schema validate m --verbose
    schema fork m m2

recording exit code, stdout and stderr (the scratch path is replaced by
`<ROOT>`). It also forks each built-in (`schema fork spec-driven f`,
`schema fork no-spec f`) in an empty project and records every file the
fork wrote, byte for byte (`builtinForks`). By default the capture is compared with the committed golden
`docs/reverse-engineering/golden/schema-validate-3.0.0.json`; any drift, an
empty fixture tree, or an oracle of another version exits non-zero and keeps
the scratch dir. ``--write`` captures twice and replaces the golden only when
both captures agree. `crates/spectra-cli/tests/schema_validate_golden_integration.rs`
replays the golden against OpenSpectra. When a schema rule changes, add a
fixture case and recapture -- never hand-edit the golden.

The oracle is macOS-only. ``--spectra-bin`` overrides ``SPECTRA_BIN``, which
itself overrides the standard application path.
"""

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
GOLDEN_REL = Path("docs/reverse-engineering/golden/schema-validate-3.0.0.json")
FIXTURE_REL = Path("crates/spectra-cli/tests/fixtures/schema_validate")
ROOT_TOKEN = "<ROOT>"
BUILTINS = ["spec-driven", "no-spec"]
RUNS = [
    ["schema", "validate", "m"],
    ["schema", "validate", "m", "--json"],
    ["schema", "validate", "m", "--verbose"],
    ["schema", "fork", "m", "m2"],
]


def fail(message: str) -> NoReturn:
    print(f"[FAIL] {message}", file=sys.stderr)
    sys.exit(1)


def run(argv: list, cwd: Path) -> subprocess.CompletedProcess:
    env = {k: v for k, v in os.environ.items() if k not in ("NO_COLOR", "FORCE_COLOR")}
    return subprocess.run(argv, cwd=cwd, env=env, capture_output=True, stdin=subprocess.DEVNULL)


def oracle_version(binary: Path, work: Path) -> str:
    result = run([str(binary), "--version"], work)
    if result.returncode != 0:
        fail(f"參考執行檔的 --version 失敗，結束碼 {result.returncode}。")
    parts = result.stdout.decode().split()
    if len(parts) < 2 or parts[0] != "spectra":
        fail(f"無法解析參考執行檔版本：{result.stdout!r}")
    return parts[1]


def make_project(root: Path, case_dir: Path) -> None:
    (root / "openspec/changes/archive").mkdir(parents=True)
    (root / "openspec/specs").mkdir(parents=True)
    (root / ".spectra.yaml").write_text("spec_dir: openspec\n")
    (root / "openspec/config.yaml").write_text("schema: spec-driven\n")
    (root / "openspec/schemas").mkdir()
    shutil.copytree(case_dir, root / "openspec/schemas/m")


def scrub(data: bytes, root: Path) -> str:
    text = data.decode("utf-8")
    # 先換較長的 realpath（macOS 的 /private/var/...），否則 /var/... 先被換掉會留下 `/private<ROOT>`。
    for form in sorted({str(root), os.path.realpath(root)}, key=len, reverse=True):
        text = text.replace(form, ROOT_TOKEN)
    return text


def builtin_fork(binary: Path, work: Path, source: str) -> dict:
    root = work / f"builtin-{source}"
    (root / "openspec/changes/archive").mkdir(parents=True)
    (root / "openspec/specs").mkdir(parents=True)
    (root / ".spectra.yaml").write_text("spec_dir: openspec\n")
    (root / "openspec/config.yaml").write_text("schema: spec-driven\n")
    result = run([str(binary), "schema", "fork", source, "f"], root)
    if result.returncode != 0:
        fail(f"fork {source} 失敗：{result.stderr!r}；scratch 目錄保留於 {work}")
    target = root / "openspec/schemas/f"
    files = {
        str(path.relative_to(target)): path.read_text(encoding="utf-8")
        for path in sorted(target.rglob("*"))
        if path.is_file()
    }
    if "schema.yaml" not in files:
        fail(f"fork {source} 沒有寫出 schema.yaml；scratch 目錄保留於 {work}")
    return {"stdout": result.stdout.decode(), "stderr": result.stderr.decode(), "files": files}


def capture(binary: Path, fixtures: Path, work: Path) -> bytes:
    version = oracle_version(binary, work)
    if version != EXPECTED_VERSION:
        fail(f"參考執行檔版本為 {version}，本腳本固定 {EXPECTED_VERSION}。")
    cases = sorted(p.name for p in fixtures.iterdir() if p.is_dir())
    if not cases:
        fail(f"{FIXTURE_REL} 底下沒有任何案例，無法作為對照。")
    out = []
    for case in cases:
        runs = []
        for argv in RUNS:
            # 一個 jail 一個操作：fork 會寫檔，validate 的結果不能被前一步影響。
            root = work / f"{case}-{len(runs)}"
            make_project(root, fixtures / case)
            result = run([str(binary), *argv], root)
            runs.append(
                {
                    "args": argv,
                    "exitCode": result.returncode,
                    "stdout": scrub(result.stdout, root),
                    "stderr": scrub(result.stderr, root),
                }
            )
        out.append({"name": case, "runs": runs})
    doc = {
        "oracleVersion": version,
        "fixtureRoot": str(FIXTURE_REL),
        "cases": out,
        "builtinForks": {name: builtin_fork(binary, work, name) for name in BUILTINS},
    }
    return (json.dumps(doc, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--spectra-bin", default=os.environ.get("SPECTRA_BIN", DEFAULT_BIN))
    parser.add_argument("--write", action="store_true", help="regenerate the golden, then verify")
    args = parser.parse_args()

    binary = Path(args.spectra_bin)
    if not os.access(binary, os.X_OK):
        fail(f"找不到可執行的參考執行檔：{binary}（以 --spectra-bin 或 SPECTRA_BIN 指定）")
    repo_root = Path(__file__).resolve().parent.parent
    fixtures = repo_root / FIXTURE_REL
    golden_path = repo_root / GOLDEN_REL
    if not fixtures.is_dir():
        fail(f"找不到 fixture 目錄 {FIXTURE_REL}")

    work = Path(tempfile.mkdtemp(prefix="capture-schema-validate-"))
    actual = capture(binary, fixtures, work)
    if args.write:
        candidate = work / "candidate.json"
        candidate.write_bytes(actual)
        recheck_work = Path(tempfile.mkdtemp(prefix="capture-schema-validate-"))
        recheck = capture(binary, fixtures, recheck_work)
        if recheck != actual:
            recheck_path = recheck_work / "actual.json"
            recheck_path.write_bytes(recheck)
            fail(f"兩次捕獲不一致，未覆寫 {GOLDEN_REL}。candidate：{candidate}；重新捕獲：{recheck_path}")
        golden_path.write_bytes(actual)
        print(f"[OK] 已寫入 {GOLDEN_REL}")
        shutil.rmtree(recheck_work)

    if not golden_path.exists():
        fail(f"{GOLDEN_REL} 不存在；以 --write 產生。scratch 目錄保留於 {work}")
    if actual != golden_path.read_bytes():
        drift_path = work / "actual.json"
        drift_path.write_bytes(actual)
        fail(f"oracle 捕獲與 {GOLDEN_REL} 不一致。實際輸出：{drift_path}；scratch 目錄保留於 {work}")
    count = len(json.loads(actual)["cases"])
    shutil.rmtree(work)
    print(f"[OK] {count} 個案例、{count * len(RUNS)} 次執行與 oracle {EXPECTED_VERSION} 一致")


if __name__ == "__main__":
    main()
