import os
import subprocess
import sys

WT = "/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w12-commands"
MSG = "/Users/howie/.claude/jobs/9eb90dff/tmp/w12/commit_msg.txt"


def git(*args, env=None):
    p = subprocess.run(["git", *args], cwd=WT, capture_output=True, text=True, env=env)
    print(f"$ git {' '.join(args)} (rc={p.returncode})\n{p.stdout}{p.stderr}")
    return p


branch = git("branch", "--show-current").stdout.strip()
if branch != "feat/w12-remaining-commands":
    print(f"[FAIL] on branch {branch!r}")
    sys.exit(1)
git("add", "-A")
env = {**os.environ, "GIT_COMMITTER_EMAIL": "2318485+howie@users.noreply.github.com",
       "GIT_COMMITTER_NAME": "howie"}
p = git("commit", "--author=howie <2318485+howie@users.noreply.github.com>", "-F", MSG, env=env)
if p.returncode != 0:
    sys.exit(1)
git("log", "-1", "--format=%H%n%an <%ae>%n%cn <%ce>%n%s")
git("status", "--short")
