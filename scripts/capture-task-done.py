#!/usr/bin/env python3
"""Verify or recapture the `spectra task done` golden from the 3.0.0 oracle.

This is a verification contract, not a printer. Every scenario below is run
in a fresh scratch git repo against the closed-source reference binary; each
`spectra` step records exit code, stdout, stderr, the resulting `tasks.md`
bytes, and the resulting `.spectra/touched/<change>.json` bytes (or absence).
By default the capture is compared against the committed golden and any drift
exits non-zero, keeping the scratch repos for inspection. ``--write``
regenerates the golden and then re-verifies it.

The golden is self-describing: it carries the base tree and every setup step,
so `crates/spectra-cli/tests/task_done_golden_integration.rs` replays the
exact same scenarios against OpenSpectra without re-deriving them.

The oracle is macOS-only. ``--spectra-bin`` overrides ``SPECTRA_BIN``, which
itself overrides the standard application path.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import NoReturn

EXPECTED_VERSION = "3.0.0"
DEFAULT_BIN = "/Applications/Spectra.app/Contents/MacOS/spectra"
GOLDEN_REL = Path("docs/reverse-engineering/golden/task-done-3.0.0.json")
ROOT_TOKEN = "<ROOT>"
CHANGE = "demo"
CHANGE_DIR = f"docs/spectra/changes/{CHANGE}"
TASKS_MD = f"{CHANGE_DIR}/tasks.md"
TOUCHED_JSON = f".spectra/touched/{CHANGE}.json"

# Isolate git from the operator's global/system config (excludesfile, signing,
# hooks) so capture and replay see the same porcelain.
GIT_ENV = {
    "GIT_CONFIG_GLOBAL": "/dev/null",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "Golden",
    "GIT_AUTHOR_EMAIL": "golden@example.com",
    "GIT_COMMITTER_NAME": "Golden",
    "GIT_COMMITTER_EMAIL": "golden@example.com",
    "GIT_AUTHOR_DATE": "2026-01-01T00:00:00Z",
    "GIT_COMMITTER_DATE": "2026-01-01T00:00:00Z",
}

GROUPED_TASKS = (
    "## 1. Core\n"
    "\n"
    "- [ ] 1.1 first core task\n"
    "- [ ] 1.2 second core task\n"
    "\n"
    "## 2. Polish\n"
    "\n"
    "- [ ] 2.1 first polish task\n"
    "- [ ] 2.2 second polish task\n"
)

BASE_FILES = {
    ".spectra.yaml": "spec_dir: docs/spectra\n",
    ".gitignore": ".spectra/\n",
    "docs/spectra/config.yaml": "schema: spec-driven\n",
    "docs/spectra/specs/.gitkeep": "",
    "docs/spectra/changes/archive/.gitkeep": "",
    f"{CHANGE_DIR}/.openspec.yaml": (
        "schema: spec-driven\ncreated: 2026-01-01\ncreated_by: Golden <golden@example.com>\n"
    ),
    TASKS_MD: GROUPED_TASKS,
    "src/a.rs": "fn a() {}\n",
    "src/b.rs": "fn b() {}\n",
    "src/old.rs": "fn old() {}\n",
    "docs/gone.md": "gone\n",
}


def w(path: str, content: str) -> dict:
    return {"op": "write", "path": path, "content": content}


def rm(path: str) -> dict:
    return {"op": "remove", "path": path}


def g(*args: str) -> dict:
    return {"op": "git", "args": list(args)}


def s(*args: str) -> dict:
    return {"op": "spectra", "args": list(args)}


def done(task_id: str, *extra: str) -> dict:
    return s("task", "done", task_id, *extra)


SCENARIOS = [
    {
        "name": "grouped-numbering-ignores-headers-and-labels",
        "description": "IDs count checkboxes top-to-bottom across ## groups; "
        "the 1.1/2.1 labels are prose. Clean tree: no tracking file.",
        "steps": [
            done("3", "--json"),
            done("1"),
            done("3"),
            done("4", "--json"),
        ],
    },
    {
        "name": "invalid-task-ids",
        "description": "Non-numeric, zero, out-of-range, and label-shaped IDs.",
        "steps": [
            done("abc"),
            done("0"),
            done("5"),
            done("99", "--json"),
            done("1.1"),
            done("01", "--json"),
        ],
    },
    {
        "name": "evaluation-order-id-vs-change",
        "description": "Which error wins when both the ID and the change are bad, "
        "and when the ID is bad but tasks.md is missing.",
        "steps": [
            done("abc", "--change", "nope"),
            done("0", "--change", "nope"),
            done("99", "--change", "nope"),
            done("1", "--change", "nope"),
            rm(TASKS_MD),
            done("0"),
            done("abc"),
            done("1"),
        ],
    },
    {
        "name": "marker-variants",
        "description": "[x]/[X] are already done; [~]/[-] succeed leaving the line; "
        "*/+ bullets are tasks; ordered-list and blank descriptions are not.",
        "base_overrides": {
            TASKS_MD: (
                "- [x] done lower\n"
                "- [X] done upper\n"
                "- [~] in progress\n"
                "- [-] cancelled\n"
                "* [ ] star bullet\n"
                "+ [ ] plus bullet\n"
                "1. [ ] ordered list\n"
                "- [ ] \n"
                "  - [ ] nested child\n"
                "- [ ] trailing spaces   \n"
            ),
        },
        "steps": [
            done("1"),
            done("2"),
            done("3", "--json"),
            done("4", "--json"),
            done("5", "--json"),
            done("6", "--json"),
            done("7", "--json"),
            done("8", "--json"),
            done("9"),
        ],
    },
    {
        "name": "line-endings-and-final-newline",
        "description": "CRLF tasks.md and a tasks.md without a trailing newline.",
        "base_overrides": {
            TASKS_MD: "- [ ] crlf one\r\n- [ ] crlf two\r\n",
        },
        "steps": [
            done("2", "--json"),
            w(TASKS_MD, "- [ ] no eol one\n- [ ] no eol two"),
            done("2", "--json"),
        ],
    },
    {
        "name": "dirty-path-kinds",
        "description": "Unstaged, staged, untracked (incl. nested new dir), renamed, "
        "and deleted paths, plus files inside the change dir and another change.",
        "steps": [
            w("src/a.rs", "fn a() { /* edited */ }\n"),
            w("src/b.rs", "fn b() { /* staged */ }\n"),
            g("add", "src/b.rs"),
            w("src/new.rs", "fn new() {}\n"),
            w("pkg/deep/nested.rs", "fn nested() {}\n"),
            g("mv", "src/old.rs", "src/renamed.rs"),
            rm("docs/gone.md"),
            w(f"{CHANGE_DIR}/design.md", "# design\n"),
            w("docs/spectra/changes/other/proposal.md", "# other\n"),
            w("docs/spectra/specs/cap/spec.md", "# spec\n"),
            s("task", "start", "1", "--change", CHANGE, "--json"),
            w("src/after-start.rs", "fn after() {}\n"),
            w("src/a.rs", "fn a() { /* edited after start */ }\n"),
            done("1", "--change", CHANGE, "--json"),
            done("2", "--change", CHANGE, "--json"),
        ],
    },
    {
        "name": "task-start-baseline",
        "description": "v3.0.0: task start captures a per-task baseline; task done "
        "records only files changed since it. A file dirty before start and "
        "untouched after is not attributed; review_base pins it.",
        "steps": [
            w("src/old.rs", "fn old() { /* dirty before start */ }\n"),
            s("task", "start", "1", "--json"),
            w("src/a.rs", "fn a() { 1 }\n"),
            w("src/new.rs", "fn new() {}\n"),
            done("1", "--json"),
            s("task", "start", "2"),
            w("src/a.rs", "fn a() { 2 }\n"),
            w("src/b.rs", "fn b() { 2 }\n"),
            done("2"),
            s("task", "start", "3", "--json"),
            s("task", "start", "3", "--json"),
            done("3", "--json"),
            s("task", "start", "1", "--json"),
            s("task", "start", "99", "--json"),
            s("task", "start", "abc"),
        ],
    },
    {
        "name": "baseline-exclusions",
        "description": "v3.0.0 with a baseline and .spectra/ NOT ignored: are "
        ".spectra/ paths, the change dir, another change, and canonical specs "
        "written after task start attributed?",
        "base_overrides": {".gitignore": None},
        "steps": [
            s("task", "start", "1"),
            w(".spectra/other-state.txt", "tool state\n"),
            w(f"{CHANGE_DIR}/design.md", "# design\n"),
            w("docs/spectra/changes/archive/2026-01-01-old/proposal.md", "# old\n"),
            w("docs/spectra/specs/cap/spec.md", "# spec\n"),
            w("src/a.rs", "fn a() { 1 }\n"),
            done("1", "--json"),
            s("task", "start", "2"),
            w("src/b.rs", "fn b() { 2 }\n"),
            done("2", "--json"),
        ],
    },
    {
        "name": "explicit-file-attribution",
        "description": "v3.0.0: --file attributes named paths (existing or not, "
        "repeatable, duplicates, change-dir paths) without a baseline.",
        "steps": [
            w("src/a.rs", "fn a() { 1 }\n"),
            done("1", "--file", "src/a.rs", "--file", "src/missing.rs", "--json"),
            done("2", "--file", "src/a.rs", "--file", "src/b.rs", "--file", "src/b.rs", "--json"),
            done("3", "--file", f"{CHANGE_DIR}/tasks.md", "--file", "./src/c.rs", "--json"),
            done("4", "--file", "src/d.rs"),
        ],
    },
    {
        "name": "first-task-wins-attribution",
        "description": "A file recorded under task 1 is not re-attributed when still "
        "dirty (or edited again) at task 2; a no-new-files task adds no entry.",
        "steps": [
            w("src/a.rs", "fn a() { 1 }\n"),
            done("1"),
            w("src/a.rs", "fn a() { 2 }\n"),
            w("src/c.rs", "fn c() {}\n"),
            done("2"),
            done("3"),
            w("src/b.rs", "fn b() { 4 }\n"),
            done("4", "--json"),
        ],
    },
    {
        "name": "spectra-dir-not-gitignored",
        "description": "With .spectra/ NOT ignored, is the tracking file itself "
        "(dirty after task 1) recorded under task 2?",
        "base_overrides": {".gitignore": None},
        "steps": [
            w("src/a.rs", "fn a() { 1 }\n"),
            done("1"),
            w("src/b.rs", "fn b() { 2 }\n"),
            done("2"),
            w(".spectra/other-state.txt", "tool state\n"),
            w("src/c.rs", "fn c() {}\n"),
            done("3"),
        ],
    },
    {
        "name": "spectra-dir-committed",
        "description": "A tracked .spectra/ file modified by hand: tracked-dirty, "
        "not untracked-ignored.",
        "base_overrides": {".gitignore": None, ".spectra/keep.txt": "v1\n"},
        "steps": [
            w(".spectra/keep.txt", "v2\n"),
            w("src/a.rs", "fn a() { 1 }\n"),
            done("1"),
        ],
    },
    {
        "name": "not-a-git-repo",
        "description": "Outside git, task done still flips the checkbox and records nothing.",
        "git": False,
        "steps": [
            w("src/a.rs", "fn a() { 1 }\n"),
            done("1", "--json"),
        ],
    },
    {
        "name": "change-autodetect",
        "description": "Two active changes: --change is required; with it, the "
        "named change's tasks.md is the one flipped.",
        "base_overrides": {
            "docs/spectra/changes/second/.openspec.yaml": (
                "schema: spec-driven\ncreated: 2026-01-01\ncreated_by: Golden <golden@example.com>\n"
            ),
            "docs/spectra/changes/second/tasks.md": "- [ ] other change task\n",
        },
        "steps": [
            done("1"),
            done("1", "--change", CHANGE, "--json"),
        ],
    },
]


def fail(message: str) -> NoReturn:
    print(f"[FAIL] {message}", file=sys.stderr)
    sys.exit(1)


def env() -> dict:
    e = dict(os.environ)
    e.update(GIT_ENV)
    e["NO_COLOR"] = "1"
    return e


def run(args: list[str], cwd: Path) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(
            args, cwd=cwd, env=env(), capture_output=True, timeout=60, stdin=subprocess.DEVNULL
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        fail(f"無法執行 {args!r}：{error}")


def git(repo: Path, args: list[str]) -> None:
    result = run(["git", *args], repo)
    if result.returncode != 0:
        fail(f"git {args!r} 在 {repo} 失敗：{result.stderr.decode(errors='replace')}")


def base_tree(scenario: dict) -> dict:
    tree = dict(BASE_FILES)
    for path, content in scenario.get("base_overrides", {}).items():
        if content is None:
            tree.pop(path, None)
        else:
            tree[path] = content
    return tree


def write(repo: Path, rel: str, content: str) -> None:
    path = repo / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content.encode())


def read_opt(path: Path) -> str | None:
    if not path.exists():
        return None
    return path.read_bytes().decode()


def state_files(repo: Path) -> list[str]:
    state = repo / ".spectra"
    if not state.is_dir():
        return []
    return sorted(p.relative_to(repo).as_posix() for p in state.rglob("*") if p.is_file())


def normalize(text: bytes, repo: Path) -> str:
    return text.decode().replace(str(repo), ROOT_TOKEN)


def run_scenario(binary: Path, scenario: dict, work: Path) -> dict:
    repo = work / scenario["name"]
    repo.mkdir(parents=True)
    repo = repo.resolve()
    tree = base_tree(scenario)
    for rel, content in tree.items():
        write(repo, rel, content)
    use_git = scenario.get("git", True)
    if use_git:
        git(repo, ["init", "-q", "-b", "main"])
        git(repo, ["add", "-A"])
        git(repo, ["commit", "-q", "-m", "base"])

    steps = []
    for step in scenario["steps"]:
        op = step["op"]
        if op == "write":
            write(repo, step["path"], step["content"])
            steps.append(step)
        elif op == "remove":
            (repo / step["path"]).unlink()
            steps.append(step)
        elif op == "git":
            git(repo, step["args"])
            steps.append(step)
        elif op == "spectra":
            result = run([str(binary), *step["args"]], repo)
            steps.append(
                {
                    **step,
                    "expect": {
                        "exit": result.returncode,
                        "stdout": normalize(result.stdout, repo),
                        "stderr": normalize(result.stderr, repo),
                        "tasks_md": read_opt(repo / TASKS_MD),
                        "touched": read_opt(repo / TOUCHED_JSON),
                        "spectra_state_files": state_files(repo),
                    },
                }
            )
        else:
            fail(f"未知的 step op：{op!r}")
    spectra_steps = sum(1 for st in steps if st["op"] == "spectra")
    if spectra_steps == 0:
        fail(f"情境 {scenario['name']} 沒有任何 spectra step，無法作為對照。")
    return {
        "name": scenario["name"],
        "description": scenario["description"],
        "git": use_git,
        "base": tree,
        "steps": steps,
    }


def oracle_version(binary: Path, work: Path) -> str:
    result = run([str(binary), "--version"], work)
    if result.returncode != 0:
        fail(f"參考執行檔的 --version 失敗，結束碼 {result.returncode}。")
    parts = result.stdout.decode().split()
    if len(parts) < 2 or parts[0] != "spectra":
        fail(f"無法解析參考執行檔版本：{result.stdout!r}")
    return parts[1]


def capture(binary: Path, work: Path) -> bytes:
    version = oracle_version(binary, work)
    if version != EXPECTED_VERSION:
        fail(f"參考執行檔版本為 {version}，本腳本固定 {EXPECTED_VERSION}。")
    golden = {
        "oracle_version": version,
        "change": CHANGE,
        "tasks_md": TASKS_MD,
        "touched_json": TOUCHED_JSON,
        "root_token": ROOT_TOKEN,
        "git_env": GIT_ENV,
        "scenarios": [run_scenario(binary, sc, work) for sc in SCENARIOS],
    }
    return (json.dumps(golden, indent=2, ensure_ascii=False) + "\n").encode()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--spectra-bin", default=os.environ.get("SPECTRA_BIN", DEFAULT_BIN))
    parser.add_argument("--write", action="store_true", help="regenerate the golden, then verify")
    args = parser.parse_args()

    binary = Path(args.spectra_bin)
    if not os.access(binary, os.X_OK):
        fail(f"找不到可執行的參考執行檔：{binary}（以 --spectra-bin 或 SPECTRA_BIN 指定）")
    repo_root = Path(__file__).resolve().parent.parent
    golden_path = repo_root / GOLDEN_REL

    work = Path(tempfile.mkdtemp(prefix="capture-task-done-"))
    actual = capture(binary, work)
    if args.write:
        golden_path.write_bytes(actual)
        print(f"[OK] 已寫入 {GOLDEN_REL}")
        shutil.rmtree(work)
        work = Path(tempfile.mkdtemp(prefix="capture-task-done-"))
        actual = capture(binary, work)

    if not golden_path.exists():
        fail(f"{GOLDEN_REL} 不存在；以 --write 產生。scratch repo 保留於 {work}")
    expected = golden_path.read_bytes()
    if actual != expected:
        drift_path = work / "actual.json"
        drift_path.write_bytes(actual)
        fail(
            f"oracle 捕獲與 {GOLDEN_REL} 不一致。實際輸出：{drift_path}；"
            f"scratch repo 保留於 {work}"
        )
    count = sum(
        1 for sc in json.loads(actual)["scenarios"] for st in sc["steps"] if st["op"] == "spectra"
    )
    shutil.rmtree(work)
    print(f"[OK] {len(SCENARIOS)} 個情境、{count} 個 spectra step 與 oracle {EXPECTED_VERSION} 一致")


if __name__ == "__main__":
    main()
