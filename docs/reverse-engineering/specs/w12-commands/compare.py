"""在既有 probe jail 上比對 oracle 與 OpenSpectra 的 decisions／show／feedback 輸出。

每個 jail 跑一組唯讀指令；stdout、stderr、exit code 逐位元組比較。
封存 change 的順序是已知的刻意分歧（oracle 用 APFS readdir 序），另外以「排序後相同」標示。
"""
import json
import os
import subprocess
import sys
from pathlib import Path

W12 = Path("/Users/howie/.claude/jobs/9eb90dff/tmp/w12")
O = "/Applications/Spectra.app/Contents/MacOS/spectra"
R = "/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w12-commands/target/release/spectra"
env = {**os.environ, "NO_COLOR": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1"}

DECISION_ARGS = [["decisions"], ["decisions", "--json"], ["decisions", "sqlite"],
                 ["decisions", "Redis", "--json"], ["decisions", "nomatch"], ["decisions", "FAST"],
                 ["decisions", ""], ["decisions", "äbc"], ["decisions", "istanbul"]]
SHOW_ARGS = [["show", "ch", "--deltas-only"], ["show", "ch", "--deltas-only", "--json"],
             ["show", "ch", "-r"], ["show", "ch", "-r", "--json"], ["show", "cap-a", "-r"],
             ["show", "cap-a", "--deltas-only", "--json"], ["show", "ch", "--item-type", "change"],
             ["show", "ch", "--item-type", "spec"], ["show", "cap-a", "--item-type", "change"],
             ["show", "both"], ["show", "both", "--item-type", "spec"],
             ["show", "both", "--item-type", "spec", "--json"],
             ["show", "both", "--item-type", "change", "--json"], ["show", "ch", "--item-type", "bogus"],
             ["show", "ch", "--item-type", "Change"], ["show", "--item-type", "spec"],
             ["show", "--item-type", "bogus"], ["show", "ch", "--item-type="], ["show"],
             ["show", "ghost", "--deltas-only"], ["show", "ghost", "--item-type", "bogus"],
             ["show", "ch", "--deltas-only", "-r", "--json"]]
FEEDBACK_ARGS = [["feedback", "hello"], ["feedback", "hello", "--body", "multi\nline"],
                 ["feedback", ""], ["feedback", "hi", "--no-color"]]

jails = sorted(p for p in (W12 / "jails").iterdir() if p.is_dir() and p.name.startswith(
    ("p04", "p05", "p06", "p07", "p08", "p09", "p10", "p11", "p12", "p13", "p14", "p20")))


def run(binary, args, cwd):
    p = subprocess.run([binary, *args], cwd=cwd, capture_output=True, env=env)
    return p.returncode, p.stdout, p.stderr


def normalize_order(out: bytes) -> object:
    try:
        return sorted(json.dumps(x, sort_keys=True) for x in json.loads(out))
    except Exception:
        return sorted(out.decode(errors="replace").splitlines())


total = same = order_only = 0
diffs = []
for jail in jails:
    cases = [(a, jail) for a in DECISION_ARGS]
    if jail.name == "p16":
        cases = [(a, jail) for a in SHOW_ARGS]
    for args, cwd in cases:
        total += 1
        a, b = run(O, args, cwd), run(R, args, cwd)
        if a == b:
            same += 1
        elif a[0] == b[0] and a[2] == b[2] and normalize_order(a[1]) == normalize_order(b[1]):
            order_only += 1
            diffs.append(("ORDER-ONLY", jail.name, args))
        else:
            diffs.append(("DIFF", jail.name, args, a, b))
for args in SHOW_ARGS:
    total += 1
    a, b = run(O, args, W12 / "jails/p16"), run(R, args, W12 / "jails/p16")
    if a == b:
        same += 1
    else:
        diffs.append(("DIFF", "p16", args, a, b))
for args in FEEDBACK_ARGS:
    total += 1
    a = subprocess.run(["sandbox-exec", "-f", str(W12 / "nonet_ro.sb"), O, *args],
                       cwd=W12 / "jails/p01", capture_output=True, env=env)
    a = (a.returncode, a.stdout, a.stderr)
    b = run(R, args, W12 / "jails/p01")
    if a == b:
        same += 1
    else:
        diffs.append(("DIFF", "feedback", args, a, b))
print(f"total={total} identical={same} order-only={order_only} other={total - same - order_only}")
for d in diffs:
    if d[0] == "DIFF":
        print("DIFF", d[1], d[2])
        print("  oracle:", d[3][0], d[3][1][:600], d[3][2][:300])
        print("  oss   :", d[4][0], d[4][1][:600], d[4][2][:300])
    else:
        print(*d)
sys.exit(0 if total == same + order_only else 1)
