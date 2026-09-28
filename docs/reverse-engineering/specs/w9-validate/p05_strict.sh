#!/bin/bash
# W9 p05：OpenSpectra --strict 與 OpenSpec --strict 的逐 item 判定比對
S=/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w8-locale/target/release/spectra
W=/Users/howie/.claude/jobs/9eb90dff/tmp/w9re
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 NO_COLOR=1
for d in yibi-mvp nextrek-cli yibi-stack; do
  cd "$W/corpus/$d" || exit 2
  for scope in changes specs; do
    "$S" validate --$scope --json --strict >| "$W/p02/$d.$scope.os-strict.json" 2>/dev/null
  done
done
python3 - <<'EOF'
import json
from pathlib import Path
R = Path("/Users/howie/.claude/jobs/9eb90dff/tmp/w9re/p02")
for d in ["yibi-mvp", "nextrek-cli", "yibi-stack"]:
    for s in ["changes", "specs"]:
        a = {i["id"]: i["valid"] for i in json.loads((R / f"{d}.{s}.openspec-strict.json").read_text())["items"]}
        b = {i["id"]: i["valid"] for i in json.loads((R / f"{d}.{s}.os-strict.json").read_text())["items"]}
        diff = sorted(k for k in a if a[k] != b.get(k))
        print(f"{d} --{s} --strict: openspec invalid={sum(not v for v in a.values())} openspectra invalid={sum(not v for v in b.values())} differ={len(diff)}")
        for k in diff:
            print(f"   {k}: openspec={a[k]} openspectra={b.get(k)}")
EOF
