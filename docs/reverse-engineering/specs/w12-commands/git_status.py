import subprocess
import sys

WT = "/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w12-commands"
for args in (["branch", "--show-current"], ["status", "--short"], ["log", "--oneline", "-3"]):
    p = subprocess.run(["git", *args], cwd=WT, capture_output=True, text=True)
    print(f"$ git {' '.join(args)} (rc={p.returncode})\n{p.stdout}{p.stderr}")
