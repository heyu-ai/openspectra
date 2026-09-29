#!/usr/bin/env python3
"""W9 p08b：p08 jail 的逐 item 三方表（OpenSpectra 的 changes 逐一跑，因為 d04 讓 bulk 整體失敗）。"""
import json
import os
import subprocess
from pathlib import Path

W = Path("/Users/howie/.claude/jobs/9eb90dff/tmp/w9re")
J = W / "jails/p08"
S = "/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w8-locale/target/release/spectra"
env = dict(os.environ, NO_COLOR="1", GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1")


def oracle(scope):
    key = "change" if scope == "changes" else "spec"
    return {i[key]: i for i in json.loads((W / f"p08/{scope}.oracle.json").read_text())}


def openspec(scope):
    return {i["id"]: i for i in json.loads((W / f"p08/{scope}.openspec.json").read_text())["items"]}


def ospectra(scope, ids):
    if scope == "specs":
        return {i["id"]: i for i in json.loads((W / "p08/specs.os.json").read_text())["items"]}
    out = {}
    for i in ids:
        p = subprocess.run([S, "validate", i, "--json"], cwd=J, env=env, capture_output=True, text=True)
        if p.stdout.strip():
            out[i] = json.loads(p.stdout)["items"][0]
        else:
            out[i] = {"valid": None, "issues": [{"level": "FATAL", "path": "-", "message": p.stderr.strip()}]}
    return out


for scope in ["changes", "specs"]:
    o = oracle(scope)
    op = openspec(scope)
    ids = sorted(set(o) | set(op))
    osx = ospectra(scope, ids)
    print(f"\n################ --{scope}  (oracle order: {list(o)})")
    for i in ids:
        print(f"\n== {i}: oracle={o.get(i, {}).get('valid')} openspec={op.get(i, {}).get('valid')} openspectra={osx.get(i, {}).get('valid')}")
        for e in o.get(i, {}).get("errors", []):
            print(f"   ORACLE   error: {e}")
        for w in o.get(i, {}).get("warnings", []):
            print(f"   ORACLE   warn:  {w}")
        for x in op.get(i, {}).get("issues", []):
            print(f"   OPENSPEC {x['level']} [{x['path']}] line={x.get('line')}: {x['message'][:330]}")
        for x in osx.get(i, {}).get("issues", []):
            print(f"   OSPECTRA {x['level']} [{x['path']}] line={x.get('line')}: {x['message'][:330]}")
