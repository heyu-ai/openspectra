#!/usr/bin/env python3
"""Verify or recapture the embedded skills from the Spectra 2.3.1 oracle.

This is a verification contract, not a general output tool. By default it
compares the oracle output byte-for-byte with the core assets and validates
their provenance manifest. ``--write`` regenerates both generated artifacts
and then verifies them again.

The oracle is macOS-only. ``--spectra-bin`` overrides ``SPECTRA_BIN``, which
itself overrides the standard application path.
"""

import argparse
import hashlib
import os
import re
import subprocess
import sys
from pathlib import Path

EXPECTED_VERSION = "2.3.1"
SKILLS = (
    "tdd",
    "audit",
    "apply",
    "archive",
    "ask",
    "commit",
    "debug",
    "discuss",
    "drift",
    "ingest",
    "propose",
    "analyze",
    "verify",
    "sync",
    "clarify",
)
KNOWN_ABSENT = (
    "review",
    "plan",
    "test",
    "refactor",
    "spec",
    "design",
    "tasks",
    "proposal",
    "validate",
    "list",
    "update",
    "init",
    "demo",
    "search",
    "new",
    "status",
    "show",
    "park",
    "unpark",
    "config",
    "instructions",
    "schemas",
    "schema",
    "templates",
    "feedback",
    "completion",
    "estimate",
    "research",
    "document",
    "release",
    "deploy",
)


def fail(message: str) -> None:
    print(f"[FAIL] {message}", file=sys.stderr)
    raise SystemExit(2)


def run(binary: Path, args: list[str], cwd: Path) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(
            [binary, *args],
            cwd=cwd,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            check=False,
            timeout=60,
        )
    except subprocess.TimeoutExpired as error:
        fail(f"參考執行檔逾時（60 秒）：{args!r}；{error}。")


def oracle_version(binary: Path, repo: Path) -> str:
    result = run(binary, ["--version"], repo)
    if result.returncode != 0:
        fail(f"參考執行檔的 --version 失敗，結束碼為 {result.returncode}。")
    parts = result.stdout.decode("utf-8", errors="replace").split()
    if len(parts) < 2:
        fail(f"無法解析參考執行檔版本：{result.stdout!r}。")
    return parts[1]


def capture_bodies(binary: Path, repo: Path) -> dict[str, bytes]:
    bodies = {}
    failures = []
    for name in SKILLS:
        result = run(binary, ["instructions", "--skill", name], repo)
        if result.returncode != 0 or result.stderr != b"":
            failures.append(
                f"skill {name} 擷取失敗，結束碼為 {result.returncode}，"
                f"stderr={result.stderr!r}。"
            )
            continue
        bodies[name] = result.stdout

    if failures:
        for failure in failures:
            print(f"[FAIL] {failure}", file=sys.stderr)
        raise SystemExit(2)
    if len(bodies) != len(SKILLS) or len(SKILLS) != 15:
        fail(
            f"skill 數量不符：擷取 {len(bodies)}、registry {len(SKILLS)}，預期 15。"
        )
    return bodies


def verify_unknown_skill(binary: Path, repo: Path) -> None:
    result = run(
        binary, ["instructions", "--skill", "__capture_probe__"], repo
    )
    expected_stderr = b"Error: Unknown skill: __capture_probe__\n"
    if (
        result.returncode != 1
        or result.stdout != b""
        or result.stderr != expected_stderr
    ):
        fail(
            "未知 skill 契約不符：預期結束碼 1、空 stdout 與逐位元相符的 "
            f"stderr，實際結束碼為 {result.returncode}，"
            f"stdout={result.stdout!r}，stderr={result.stderr!r}。"
        )


def verify_known_absent(binary: Path, repo: Path) -> None:
    added = []
    for name in KNOWN_ABSENT:
        result = run(binary, ["instructions", "--skill", name], repo)
        if result.returncode == 0:
            added.append(name)
    if added:
        fail(f"oracle 新增了未 port 的 skill：{', '.join(added)}。")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def diff_summary(path: Path, actual: bytes, expected: bytes, repo: Path) -> str:
    relative = path.relative_to(repo)
    shared = min(len(actual), len(expected))
    offset = next(
        (index for index in range(shared) if actual[index] != expected[index]),
        shared,
    )
    return (
        f"{relative}：內容不符；目前 {len(actual)} bytes／sha256={sha256(actual)}；"
        f"oracle {len(expected)} bytes／sha256={sha256(expected)}；"
        f"第一個差異位元組位置為 {offset}。"
    )


def manifest_bytes(bodies: dict[str, bytes]) -> bytes:
    lines = ["skill\tbytes\tsha256"]
    lines.extend(
        f"{name}\t{len(bodies[name])}\t{sha256(bodies[name])}"
        for name in SKILLS
    )
    return ("\n".join(lines) + "\n").encode()


def verify_asset_listing(repo: Path) -> list[str]:
    assets_dir = repo / "crates/spectra-core/assets/skills"
    expected = {f"{name}.md" for name in SKILLS}
    actual = {path.name for path in assets_dir.glob("*.md") if path.is_file()}
    if actual == expected:
        return []
    missing = sorted(expected - actual)
    extra = sorted(actual - expected)
    return [
        "crates/spectra-core/assets/skills：skill 檔案清單不符；"
        f"缺少={missing}，多出={extra}。"
    ]


def verify_rust_registry(repo: Path) -> list[str]:
    path = repo / "crates/spectra-core/src/skills.rs"
    text = path.read_text(encoding="utf-8")
    entries = tuple(re.findall(r'^\s*\("([^"]+)",', text, re.MULTILINE))
    if entries == SKILLS:
        return []
    return [
        f"{path.relative_to(repo)}：Rust registry 不符；"
        f"實際={entries}，預期={SKILLS}。"
    ]


def verify_files(repo: Path, bodies: dict[str, bytes]) -> list[str]:
    mismatches = verify_asset_listing(repo)
    for name in SKILLS:
        body = bodies[name]
        path = repo / "crates/spectra-core/assets/skills" / f"{name}.md"
        if not path.is_file():
            mismatches.append(f"{path.relative_to(repo)}：檔案不存在。")
            continue
        actual = path.read_bytes()
        if actual != body:
            mismatches.append(diff_summary(path, actual, body, repo))

    manifest_path = (
        repo
        / "docs/reverse-engineering/golden"
        / f"skills-{EXPECTED_VERSION}.tsv"
    )
    expected_manifest = manifest_bytes(bodies)
    if not manifest_path.is_file():
        mismatches.append(f"{manifest_path.relative_to(repo)}：檔案不存在。")
    else:
        actual_manifest = manifest_path.read_bytes()
        if actual_manifest != expected_manifest:
            mismatches.append(
                diff_summary(
                    manifest_path, actual_manifest, expected_manifest, repo
                )
            )
    return mismatches


def write_files(repo: Path, bodies: dict[str, bytes]) -> None:
    assets_dir = repo / "crates/spectra-core/assets/skills"
    assets_dir.mkdir(parents=True, exist_ok=True)
    for name in SKILLS:
        body = bodies[name]
        (assets_dir / f"{name}.md").write_bytes(body)
    manifest_path = (
        repo
        / "docs/reverse-engineering/golden"
        / f"skills-{EXPECTED_VERSION}.tsv"
    )
    manifest_path.write_bytes(manifest_bytes(bodies))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write",
        action="store_true",
        help="重寫 core 靜態資產與 golden manifest，然後再次驗證。",
    )
    parser.add_argument(
        "--spectra-bin",
        default=os.environ.get(
            "SPECTRA_BIN", "/Applications/Spectra.app/Contents/MacOS/spectra"
        ),
        help="參考執行檔路徑（也可設定 SPECTRA_BIN）。",
    )
    args = parser.parse_args()

    if sys.platform != "darwin":
        fail("此擷取腳本僅支援 macOS。")

    binary = Path(args.spectra_bin).expanduser().resolve()
    if not binary.is_file():
        fail(f"參考執行檔不存在：{binary}。")

    repo = Path(__file__).resolve().parent.parent
    registry_mismatches = verify_rust_registry(repo)
    if registry_mismatches:
        for mismatch in registry_mismatches:
            print(f"[FAIL] {mismatch}", file=sys.stderr)
        raise SystemExit(2)

    version = oracle_version(binary, repo)
    if version != EXPECTED_VERSION:
        fail(f"oracle 版本必須是 {EXPECTED_VERSION}，實際為 {version}。")

    bodies = capture_bodies(binary, repo)
    verify_unknown_skill(binary, repo)
    verify_known_absent(binary, repo)
    if args.write:
        write_files(repo, bodies)

    mismatches = verify_files(repo, bodies)
    if mismatches:
        for mismatch in mismatches:
            print(f"[FAIL] {mismatch}", file=sys.stderr)
        raise SystemExit(2)

    mode = "已重新產生並驗證" if args.write else "已驗證"
    print(f"[OK] {mode} {len(SKILLS)} 個 skill 的資產與 digest manifest。")
    print("[OK] 未知 skill 的 stderr 與結束碼契約符合預期。")
    print(f"[OK] {len(KNOWN_ABSENT)} 個候選名稱仍不是 oracle skill。")


if __name__ == "__main__":
    main()
