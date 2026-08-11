#!/usr/bin/env python3
"""驗證或重新擷取 Spectra 2.3.1 內嵌 skill 內容。

這是驗證契約，不是一般輸出工具。預設只比較 oracle 輸出、core 靜態資產與
golden fixture；只有明確傳入 ``--write`` 才會重寫兩組產物，重寫後仍會再次
逐位元驗證。

僅支援 macOS，且必須透過 ``SPECTRA_BIN`` 指定 closed-source 參考執行檔。
"""

import argparse
import hashlib
import os
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
)


def fail(message: str) -> None:
    print(f"[FAIL] {message}", file=sys.stderr)
    raise SystemExit(2)


def run(binary: Path, args: list[str], cwd: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [binary, *args], cwd=cwd, capture_output=True, check=False
    )


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
        if result.returncode != 0:
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
    return bodies


def verify_unknown_skill(binary: Path, repo: Path) -> None:
    result = run(
        binary, ["instructions", "--skill", "__capture_probe__"], repo
    )
    if result.returncode != 1 or b"Unknown skill" not in result.stderr:
        fail(
            "未知 skill 契約不符：預期結束碼 1 且 stderr 包含 "
            f"Unknown skill，實際結束碼為 {result.returncode}，"
            f"stdout={result.stdout!r}，stderr={result.stderr!r}。"
        )


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


def verify_files(repo: Path, bodies: dict[str, bytes]) -> list[str]:
    mismatches = []
    for name, body in bodies.items():
        paths = (
            repo / "crates/spectra-core/assets/skills" / f"{name}.md",
            repo
            / "docs/reverse-engineering/golden/skills"
            / f"{name}-{EXPECTED_VERSION}.md",
        )
        for path in paths:
            if not path.is_file():
                mismatches.append(f"{path.relative_to(repo)}：檔案不存在。")
                continue
            actual = path.read_bytes()
            if actual != body:
                mismatches.append(diff_summary(path, actual, body, repo))
    return mismatches


def write_files(repo: Path, bodies: dict[str, bytes]) -> None:
    assets_dir = repo / "crates/spectra-core/assets/skills"
    golden_dir = repo / "docs/reverse-engineering/golden/skills"
    assets_dir.mkdir(parents=True, exist_ok=True)
    golden_dir.mkdir(parents=True, exist_ok=True)
    for name, body in bodies.items():
        (assets_dir / f"{name}.md").write_bytes(body)
        (golden_dir / f"{name}-{EXPECTED_VERSION}.md").write_bytes(body)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--write",
        action="store_true",
        help="重寫 core 靜態資產與 golden fixture，然後再次驗證。",
    )
    args = parser.parse_args()

    if sys.platform != "darwin":
        fail("此擷取腳本僅支援 macOS。")

    binary_value = os.environ.get("SPECTRA_BIN")
    if not binary_value:
        fail("必須透過 SPECTRA_BIN 指定 closed-source 參考執行檔。")
    binary = Path(binary_value).expanduser().resolve()
    if not binary.is_file():
        fail(f"SPECTRA_BIN 指定的參考執行檔不存在：{binary}。")

    repo = Path(__file__).resolve().parent.parent
    version = oracle_version(binary, repo)
    if version != EXPECTED_VERSION:
        fail(f"oracle 版本必須是 {EXPECTED_VERSION}，實際為 {version}。")

    bodies = capture_bodies(binary, repo)
    verify_unknown_skill(binary, repo)
    if args.write:
        write_files(repo, bodies)

    mismatches = verify_files(repo, bodies)
    if mismatches:
        for mismatch in mismatches:
            print(f"[FAIL] {mismatch}", file=sys.stderr)
        raise SystemExit(2)

    mode = "已重新產生並驗證" if args.write else "已驗證"
    print(f"[OK] {mode} {len(SKILLS)} 個 skill 的兩組逐位元產物。")
    print("[OK] 未知 skill 的 stderr 與結束碼契約符合預期。")


if __name__ == "__main__":
    main()
