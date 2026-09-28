"""worktree 隔離下的檔案搬運：pull 把 worktree 檔案複製到 mirror，push 把 mirror 寫回 worktree。

用法：python3 sync.py pull <rel>...   |   python3 sync.py push [<rel>...]（省略則推送 mirror 內全部檔案）
push 前比對 worktree 現況與上次 pull 的快照，不一致就 [FAIL]（避免覆蓋別人的改動）。
寫回用 open().write（不保留 mtime），讓 cargo 看得到變更。
"""
import hashlib
import json
import sys
from pathlib import Path

WT = Path("/Users/howie/Workspace/github/heyu-ai/openspectra/.claude/worktrees/w12-commands")
MIRROR = Path("/Users/howie/.claude/jobs/9eb90dff/tmp/w12/wt")
STATE = Path("/Users/howie/.claude/jobs/9eb90dff/tmp/w12/sync-state.json")


def digest(p: Path) -> str | None:
    return hashlib.sha256(p.read_bytes()).hexdigest() if p.exists() else None


def main() -> int:
    mode, rels = sys.argv[1], sys.argv[2:]
    state = json.loads(STATE.read_text()) if STATE.exists() else {}
    if mode == "pull":
        for rel in rels:
            src, dst = WT / rel, MIRROR / rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            dst.write_bytes(src.read_bytes())
            state[rel] = digest(src)
            print(f"pulled {rel}")
    elif mode == "push":
        if not rels:
            rels = sorted(p.relative_to(MIRROR).as_posix() for p in MIRROR.rglob("*") if p.is_file())
        for rel in rels:
            src, dst = MIRROR / rel, WT / rel
            if digest(dst) != state.get(rel):
                print(f"[FAIL] {rel}: worktree changed since last pull/push")
                return 1
            data = src.read_bytes()
            if dst.exists() and dst.read_bytes() == data:
                continue
            dst.parent.mkdir(parents=True, exist_ok=True)
            with open(dst, "wb") as f:
                f.write(data)
            state[rel] = digest(dst)
            print(f"pushed {rel}")
    else:
        print("[FAIL] mode must be pull or push")
        return 2
    STATE.write_text(json.dumps(state, indent=1))
    return 0


sys.exit(main())
