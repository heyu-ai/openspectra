#!/usr/bin/env python3
"""手動 mutation 驗證：套用單一字串替換 → 跑指定測試 → 還原。

cargo-mutants 不會突變字串字面值與 git 參數這類「值」層級的缺陷，而多數
已回報的 bug 正好屬於這類（例如 `--git-common-dir` 誤寫成 `--git-dir`）。
本腳本補上這個缺口：每個 case 只改一件事，並斷言 anchor 恰好命中一次；
找不到 anchor 或命中多次都 `[FAIL]`，不會靜默略過。

用法：
    scripts/mutate-check.py                 # 跑 scripts/mutations.toml 內所有 case
    scripts/mutate-check.py --only <id>     # 只跑一個 case
    scripts/mutate-check.py --check-anchors # 只檢查 anchor，不 build（秒級）

每個 case 的期望是 mutant 讓指定測試**失敗**（KILLED）。若測試仍通過
（SURVIVED），代表那個回歸測試守不住它宣稱要守的 bug，整體 exit 1。

還原後會 `touch` 原檔，避免 cargo 以 mtime 判斷「未變更」而沿用 mutant
產物（見 CLAUDE.md：PR #84 的 probe 事故）。
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CASES = ROOT / "scripts" / "mutations.toml"
TIMEOUT_SECS = 900


def run_case(case: dict) -> str:
    path = ROOT / case["file"]
    original = path.read_text(encoding="utf-8")
    count = original.count(case["find"])
    if count != 1:
        print(f"[FAIL] {case['id']}: anchor 命中 {count} 次（必須恰好 1 次）: {case['find']!r}")
        return "ANCHOR"
    mutated = original.replace(case["find"], case["replace"], 1)
    path.write_text(mutated, encoding="utf-8")
    try:
        cmd = ["cargo", "test", "-q", "-p", case["package"]]
        cmd += case.get("target", ["--lib"])
        cmd += [case["test"]]
        try:
            result = subprocess.run(
                cmd, cwd=ROOT, capture_output=True, text=True, timeout=TIMEOUT_SECS
            )
        except subprocess.TimeoutExpired:
            # 與 cargo-mutants 相同：timeout 不算 KILLED，視為未驗證
            print(f"[FAIL] {case['id']}: TIMEOUT（超過 {TIMEOUT_SECS}s）")
            return "TIMEOUT"
        out = result.stdout + result.stderr
        if "error[E" in out or "could not compile" in out:
            print(f"[FAIL] {case['id']}: mutant 無法編譯（case 本身寫錯）")
            print(out[-2000:])
            return "UNVIABLE"
        if " 0 passed; 0 failed" in out and "test result" in out and result.returncode == 0:
            print(f"[FAIL] {case['id']}: 測試過濾器 {case['test']!r} 沒有選中任何測試")
            return "NOTEST"
        if result.returncode != 0:
            print(f"[OK]   {case['id']}: KILLED")
            return "KILLED"
        print(f"[FAIL] {case['id']}: SURVIVED -- {case['test']} 在 mutant 下仍通過")
        return "SURVIVED"
    finally:
        path.write_text(original, encoding="utf-8")
        os.utime(path, None)


def check_anchors(cases: list[dict]) -> int:
    """不 build、不改檔：斷言每個 case 的目標檔存在、`find` 恰好命中一次、id 不重複。

    重構移動或改寫程式碼時 anchor 會靜默失效（#229），完整跑 mutate-check
    要很久，這個檢查讓失效在秒級就被發現。
    """
    failed = 0
    seen: set[str] = set()
    for case in cases:
        cid = case.get("id", "<missing id>")
        problems: list[str] = []
        if cid in seen:
            problems.append("id 重複")
        seen.add(cid)
        missing = [k for k in ("id", "file", "find", "replace", "package", "test") if k not in case]
        if missing:
            problems.append(f"缺少欄位 {missing}")
        else:
            if case["find"] == case["replace"]:
                problems.append("find 與 replace 相同，mutant 沒有改任何東西")
            path = ROOT / case["file"]
            if not path.is_file():
                problems.append(f"目標檔不存在: {case['file']}")
            else:
                count = path.read_text(encoding="utf-8").count(case["find"])
                if count != 1:
                    problems.append(f"anchor 在 {case['file']} 命中 {count} 次（必須恰好 1 次）")
        for problem in problems:
            print(f"[FAIL] {cid}: {problem}")
        failed += bool(problems)
    print(f"\n{len(cases) - failed}/{len(cases)} anchors ok")
    return 1 if failed else 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--only")
    parser.add_argument("--cases", type=Path, default=CASES)
    parser.add_argument(
        "--check-anchors",
        action="store_true",
        help="只檢查每個 case 的 anchor 恰好命中一次，不 build、不跑測試",
    )
    args = parser.parse_args()
    cases = tomllib.loads(args.cases.read_text(encoding="utf-8"))["case"]
    if args.check_anchors:
        if args.only:
            cases = [c for c in cases if c.get("id") == args.only]
            if not cases:
                print(f"[FAIL] 找不到 case {args.only!r}")
                return 2
        return check_anchors(cases)
    if args.only:
        cases = [c for c in cases if c["id"] == args.only]
        if not cases:
            print(f"[FAIL] 找不到 case {args.only!r}")
            return 2
    results = {c["id"]: run_case(c) for c in cases}
    bad = [k for k, v in results.items() if v != "KILLED"]
    print(f"\n{len(results) - len(bad)}/{len(results)} killed")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
