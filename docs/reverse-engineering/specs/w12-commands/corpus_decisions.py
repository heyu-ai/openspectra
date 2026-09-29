"""在 corpus 專案的複本上比對 oracle 與 OpenSpectra 的 `decisions`（原專案不動）。"""
import json
import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

O = "/Applications/Spectra.app/Contents/MacOS/spectra"
R = "/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w12-commands/target/release/spectra"
CORPUS = ["/Users/howie/Workspace/github/heyu-ai/yibi-mvp",
          "/Users/howie/Workspace/github/heyu-ai/nextrek-cli",
          "/Users/howie/Workspace/github/yibi-stack"]
env = {**os.environ, "NO_COLOR": "1"}
root = Path(tempfile.mkdtemp(prefix="w12-corpus-"))
for proj in map(Path, CORPUS):
    m = re.search(r"^spec_dir:\s*(\S+)", (proj / ".spectra.yaml").read_text(), re.M)
    spec_dir = m.group(1) if m else "openspec"
    box = root / proj.name
    box.mkdir()
    shutil.copyfile(proj / ".spectra.yaml", box / ".spectra.yaml")
    shutil.copytree(proj / spec_dir, box / spec_dir, symlinks=True)
    for args in (["decisions", "--json"], ["decisions"], ["decisions", "cache", "--json"]):
        a = subprocess.run([O, *args], cwd=box, capture_output=True, env=env)
        b = subprocess.run([R, *args], cwd=box, capture_output=True, env=env)
        tag = f"{proj.name} {' '.join(args)}"
        if a.returncode != b.returncode or a.stderr != b.stderr:
            print(f"DIFF {tag}: rc {a.returncode}/{b.returncode} stderr {a.stderr[:200]!r} / {b.stderr[:200]!r}")
            continue
        if a.stdout == b.stdout:
            print(f"IDENTICAL {tag}" + (f": {len(json.loads(a.stdout))} decisions" if "--json" in args else ""))
            continue
        if "--json" in args:
            ja, jb = json.loads(a.stdout), json.loads(b.stdout)
            key = lambda x: json.dumps(x, sort_keys=True)
            same_set = sorted(map(key, ja)) == sorted(map(key, jb))
            # 使用中的 change 應該在同一段落（前段），只有封存段落的順序可能不同
            print(f"{'ORDER-ONLY' if same_set else 'DIFF'} {tag}: {len(ja)} vs {len(jb)} decisions")
            if not same_set:
                sa, sb = set(map(key, ja)), set(map(key, jb))
                for x in list(sa - sb)[:3]:
                    print("  oracle-only", x[:300])
                for x in list(sb - sa)[:3]:
                    print("  oss-only", x[:300])
        else:
            same_lines = sorted(a.stdout.splitlines()) == sorted(b.stdout.splitlines())
            print(f"{'ORDER-ONLY' if same_lines else 'DIFF'} {tag}")
shutil.rmtree(root)
