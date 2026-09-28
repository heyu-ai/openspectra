"""整合測試 fixture 的 oracle 輸出：建立與 decisions_integration.rs 完全相同的專案（含 mtime），
印出 oracle 的 stdout／exit code，作為測試的預期值來源。"""
import os
import shutil
import subprocess
from pathlib import Path

O = "/Applications/Spectra.app/Contents/MacOS/spectra"
J = Path("/Users/howie/.claude/jobs/9eb90dff/tmp/w12/jails/p21")
shutil.rmtree(J, ignore_errors=True)
FILES = {
    ".spectra.yaml": "spec_dir: openspec\n",
    "openspec/config.yaml": "schema: spec-driven\n",
    "openspec/changes/alpha/.openspec.yaml": "schema: spec-driven\ncreated: 2026-09-01\n",
    "openspec/changes/alpha/design.md": (
        "## Context\n\nctx\n\n### Not A Decision In Context\n\ntext\n\n## Decisions\n\n"
        "### Use SQLite\n\nWe pick SQLite because it is embedded.\nSecond line of rationale.\n\n"
        "#### Sub heading\n\nsub text\n\n### Empty Rationale\n\n### Replace Cache\n\n"
        "**Supersedes**: old-change / Use Redis\n\nWe now use an in-process cache.\n\n"
        "### Bad Ref\n\n**Supersedes**: ghost / Nothing\n\nPointing nowhere.\n\n"
        "## Risks / Trade-offs\n\n### Not A Decision In Risks\n\nr\n"
    ),
    "openspec/changes/beta/.openspec.yaml": "created: 2026-02-10\n",
    "openspec/changes/beta/design.md": (
        "## Decisions\n\n### Beta Choice\n\nBeta rationale mentions sqlite lowercase.\n"
    ),
    "openspec/changes/gamma/proposal.md": "## Why\n\nno design\n",
    "openspec/changes/archive/2026-01-15-old-change/.openspec.yaml": "schema: spec-driven\ncreated: 2026-01-10\n",
    "openspec/changes/archive/2026-01-15-old-change/design.md": (
        "## Decisions\n\n### Use Redis\n\nRedis is fast.\n\n### Keep Logs\n\nLogs are kept for 30 days.\n"
    ),
}
MTIMES = {"openspec/changes/alpha": 1_000, "openspec/changes/beta": 2_000, "openspec/changes/gamma": 500}
for rel, text in FILES.items():
    p = J / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)
(J / "openspec/specs").mkdir(parents=True, exist_ok=True)
for rel, secs in MTIMES.items():
    for f in (J / rel).rglob("*"):
        if f.is_file():
            os.utime(f, (secs, secs))
env = {**os.environ, "NO_COLOR": "1", "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_NOSYSTEM": "1"}
for args in (["decisions", "--json"], ["decisions"], ["decisions", "redis"], ["decisions", "nomatch"],
             ["decisions", "nomatch", "--json"]):
    p = subprocess.run([O, *args], cwd=J, capture_output=True, text=True, env=env)
    print(f"### {args} rc={p.returncode} stderr={p.stderr!r}")
    print(repr(p.stdout))
