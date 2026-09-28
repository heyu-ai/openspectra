"""show_demo_feedback_integration.rs 的 show fixture：在 oracle 上跑同一組指令，
與 Rust 測試裡的常數逐位元組比對（常數從測試原始檔抽出）。"""
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

O = "/Applications/Spectra.app/Contents/MacOS/spectra"
TEST = Path("/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w12-commands/crates/spectra-cli/tests/show_demo_feedback_integration.rs")
J = Path("/Users/howie/.claude/jobs/9eb90dff/tmp/w12/jails/p22")
shutil.rmtree(J, ignore_errors=True)
c = "openspec/changes/ch"
FILES = {
    ".spectra.yaml": "spec_dir: openspec\n",
    "openspec/config.yaml": "schema: spec-driven\n",
    f"{c}/.openspec.yaml": "schema: spec-driven\ncreated: 2026-09-01\n",
    f"{c}/proposal.md": "## Why\n\nwhy text\n",
    f"{c}/design.md": "## Decisions\n\n### D\n\nr\n",
    f"{c}/tasks.md": "## 1. T\n\n- [ ] 1.1 x\n",
    f"{c}/specs/cap-a/spec.md": "## ADDED Requirements\n\n### Requirement: Alpha\n\nThe system SHALL alpha.\n",
    f"{c}/specs/cap-b/spec.md": "## REMOVED Requirements\n\n### Requirement: Old\n\n**Reason**: x\n",
    f"{c}/specs/cap-b/sub/notes.md": "extra\n",
    "openspec/specs/cap-a/spec.md": "# cap-a Specification\n\n## Purpose\n\nPurpose text.\n",
    "openspec/specs/cap-a/design-notes.md": "extra spec file\n",
    "openspec/changes/both/.openspec.yaml": "schema: spec-driven\ncreated: 2026-09-01\n",
    "openspec/changes/both/proposal.md": "## Why\n\nchange both\n",
    "openspec/specs/both/spec.md": "# both\n\n## Purpose\n\np\n",
}
for rel, text in FILES.items():
    p = J / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)
(J / "openspec/changes/archive").mkdir(parents=True, exist_ok=True)
env = {**os.environ, "NO_COLOR": "1"}


def run(args):
    p = subprocess.run([O, *args], cwd=J, capture_output=True, text=True, env=env)
    return p.returncode, p.stdout, p.stderr


src = TEST.read_text()


def const(name):
    m = re.search(rf'const {name}: &str = "(.*?)";\n', src, re.S)
    return m.group(1).encode().decode("unicode_escape").encode("latin-1").decode("utf-8")


ok = True
checks = [
    (["show", "ch", "--deltas-only"], (0, const("CH_HUMAN"), "")),
    (["show", "ch", "-r"], (0, const("CH_HUMAN"), "")),
    (["show", "ch", "--requirements"], (0, const("CH_HUMAN"), "")),
    (["show", "ch", "--deltas-only", "-r"], (0, const("CH_HUMAN"), "")),
    (["show", "ch", "--item-type", "change"], (0, const("CH_HUMAN"), "")),
    (["show", "ch", "--deltas-only", "--json"], (0, const("CH_JSON"), "")),
    (["show", "ch", "-r", "--json"], (0, const("CH_JSON"), "")),
    (["show", "ch", "--deltas-only", "-r", "--json"], (0, const("CH_JSON"), "")),
    (["show", "cap-a", "-r"], (0, const("CAP_A_HUMAN"), "")),
    (["show", "cap-a", "--deltas-only"], (0, const("CAP_A_HUMAN"), "")),
    (["show", "cap-a", "--item-type", "spec"], (0, const("CAP_A_HUMAN"), "")),
    (["show", "both", "--item-type", "spec", "--json"], (0, '{\n  "files": [\n    {\n      "content": "# both\\n\\n## Purpose\\n\\np\\n",\n      "name": "spec.md"\n    }\n  ],\n  "name": "both"\n}\n', "")),
    (["show", "both"], (0, "Change: both\nSchema: spec-driven\nCreated: 2026-09-01\n\n--- Proposal ---\n## Why\n\nchange both\n\n", "")),
    (["show", "ch", "--item-type", "spec"], (1, "", "Error: Spec 'ch' not found.\n")),
    (["show", "cap-a", "--item-type", "change"], (1, "", "Error: Change 'cap-a' not found.\n")),
    (["show", "ch", "--item-type", "bogus"], (1, "", "Error: Unknown type: bogus. Use 'change' or 'spec'.\n")),
    (["show", "ghost", "--item-type", "Change"], (1, "", "Error: Unknown type: Change. Use 'change' or 'spec'.\n")),
    (["show", "ch", "--item-type="], (1, "", "Error: Unknown type: . Use 'change' or 'spec'.\n")),
    (["show", "ghost", "--deltas-only"], (1, "", "Error: Item 'ghost' not found as a change or spec.\n")),
    (["show"], (1, "", "Error: Please specify an item name.\n")),
    (["show", "--item-type", "bogus"], (1, "", "Error: Please specify an item name.\n")),
]
for args, expected in checks:
    got = run(args)
    if got != expected:
        ok = False
        print(f"[DIFF] {args}\n  oracle  {got!r}\n  test    {expected!r}")
print("[OK]" if ok else "[FAIL]", len(checks), "checks")
sys.exit(0 if ok else 1)
