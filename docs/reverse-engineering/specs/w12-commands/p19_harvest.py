"""demo 取樣：在 sandbox（只可寫 jail、無網路）裡跑 N 次 demo，收集名稱成分與各主題檔案內容。

每個主題的檔案內容必須在所有取樣間逐位元組一致（.openspec.yaml 除外），否則 [FAIL]。
輸出：harvest/<theme>/{proposal.md,design.md,tasks.md,specs/<cap>/spec.md} 與 harvest.json。
"""
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

W12 = Path("/Users/howie/.claude/jobs/9eb90dff/tmp/w12")
O = "/Applications/Spectra.app/Contents/MacOS/spectra"
N = int(sys.argv[1]) if len(sys.argv) > 1 else 400
jail = W12 / "jails" / "p19"
shutil.rmtree(jail, ignore_errors=True)
(jail / "openspec/changes/archive").mkdir(parents=True)
(jail / "openspec/specs").mkdir(parents=True)
(jail / ".spectra.yaml").write_text("spec_dir: openspec\n")
(jail / "openspec/config.yaml").write_text("schema: spec-driven\n")
env = dict(os.environ, GIT_CONFIG_GLOBAL="/dev/null", GIT_CONFIG_NOSYSTEM="1")
subprocess.run(["git", "init", "-q", "-b", "main"], cwd=jail, check=True, env=env)
subprocess.run(["git", "config", "user.name", "T"], cwd=jail, check=True, env=env)
subprocess.run(["git", "config", "user.email", "t@x"], cwd=jail, check=True, env=env)
prof = W12 / "demo_p19.sb"
prof.write_text(
    '(version 1)\n(allow default)\n(deny network*)\n'
    '(deny process-exec (literal "/usr/bin/open"))\n'
    '(deny file-write* (subpath "/Users") (subpath "/private/var/folders") (subpath "/private/tmp"))\n'
    f'(allow file-write* (subpath "{jail}"))\n'
)
out_dir = W12 / "harvest"
shutil.rmtree(out_dir, ignore_errors=True)
adjs, mons, themes = {}, {}, {}
theme_files: dict[str, dict[str, bytes]] = {}
line_re = re.compile(
    r"^✓ Created demo change: spx-([a-z]+)-([a-z]+)\n  Theme: ([a-z-]+)\n  Path: (.+)\n$"
)
for i in range(N):
    r = subprocess.run(["sandbox-exec", "-f", str(prof), O, "demo"], cwd=jail,
                       capture_output=True, env=env)
    if r.returncode != 0:
        print(f"[FAIL] run {i}: rc={r.returncode} {r.stderr!r}")
        sys.exit(1)
    m = line_re.match(r.stdout.decode())
    if not m:
        print(f"[FAIL] run {i}: unexpected stdout {r.stdout!r}")
        sys.exit(1)
    adj, mon, theme, path = m.groups()
    adjs[adj] = adjs.get(adj, 0) + 1
    mons[mon] = mons.get(mon, 0) + 1
    themes[theme] = themes.get(theme, 0) + 1
    cdir = Path(path)
    files = {}
    for f in sorted(cdir.rglob("*")):
        if f.is_file():
            rel = f.relative_to(cdir).as_posix()
            if rel != ".openspec.yaml":
                files[rel] = f.read_bytes()
    meta = (cdir / ".openspec.yaml").read_text()
    if not re.fullmatch(r"schema: spec-driven\ncreated: \d{4}-\d{2}-\d{2}\ncreated_by: T <t@x>\n", meta):
        print(f"[FAIL] run {i}: meta {meta!r}")
        sys.exit(1)
    if theme in theme_files:
        if theme_files[theme] != files:
            print(f"[FAIL] theme {theme} content differs between runs")
            sys.exit(1)
    else:
        theme_files[theme] = files
    shutil.rmtree(cdir)
for theme, files in theme_files.items():
    for rel, data in files.items():
        p = out_dir / theme / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
summary = {"runs": N, "adjectives": dict(sorted(adjs.items())),
           "pokemon": dict(sorted(mons.items())), "themes": dict(sorted(themes.items())),
           "files": {t: sorted(f) for t, f in sorted(theme_files.items())}}
(W12 / "harvest.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps({k: (len(v) if isinstance(v, dict) else v) for k, v in summary.items()}))
print("adjectives:", " ".join(sorted(adjs)))
print("pokemon:", " ".join(sorted(mons)))
print("themes:", summary["themes"])
