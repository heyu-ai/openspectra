#!/usr/bin/env python3
"""Summarize OpenSpectra's implementation-switch logs as Markdown.

Reads ``shadow.jsonl`` (``OPENSPECTRA_IMPL=shadow``: read-only calls whose
OpenSpectra result differed from the oracle's) and ``errors.jsonl``
(``OPENSPECTRA_IMPL=oss``: non-zero exits and panics) from
``$XDG_STATE_HOME/openspectra`` (default ``~/.local/state/openspectra``) and
groups them so new divergence classes stand out:

* shadow differences by subcommand and by difference kind (array indices are
  collapsed, ``.changes[3].name`` -> ``.changes[].name``);
* errors by subcommand and first line of the summary, panics listed first.

``--since YYYY-MM-DD`` keeps only records on or after that date (UTC).
``--state-dir`` overrides the log directory. A malformed line fails the run
(exit 1) instead of being skipped, so a corrupted log is noticed.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import re
import sys
from pathlib import Path


def default_state_dir() -> Path:
    base = os.environ.get("XDG_STATE_HOME")
    root = Path(base) if base and Path(base).is_absolute() else Path.home() / ".local" / "state"
    return root / "openspectra"


def load(path: Path, since: str | None) -> list[dict]:
    if not path.is_file():
        return []
    records = []
    for n, line in enumerate(path.read_text().splitlines(), 1):
        if not line.strip():
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError as e:
            print(f"[FAIL] {path}:{n}: malformed line ({e})", file=sys.stderr)
            sys.exit(1)
        if since and record.get("ts", "")[:10] < since:
            continue
        records.append(record)
    return records


def subcommand(argv: list[str]) -> str:
    words = [a for a in argv if not a.startswith("-")]
    return " ".join(words[:2]) if words[:1] in (["task"], ["schema"], ["config"], ["new"]) else (words[0] if words else "(none)")


def generic(diff: str) -> str:
    return re.sub(r"\[\d+\]", "[]", diff)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--state-dir", type=Path, default=default_state_dir())
    ap.add_argument("--since", help="only records on or after YYYY-MM-DD (UTC)")
    args = ap.parse_args()

    shadow = load(args.state_dir / "shadow.jsonl", args.since)
    errors = load(args.state_dir / "errors.jsonl", args.since)
    print(f"# OpenSpectra implementation-switch report\n")
    print(f"Logs: `{args.state_dir}`" + (f", since {args.since}" if args.since else "") + "\n")

    print(f"## Shadow differences ({len(shadow)} call(s))\n")
    if shadow:
        by_cmd: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
        calls = collections.Counter()
        for r in shadow:
            cmd = subcommand(r.get("argv", []))
            calls[cmd] += 1
            for d in set(generic(x) for x in r.get("diffs", [])):
                by_cmd[cmd][d] += 1
        for cmd, count in calls.most_common():
            print(f"### `{cmd}` — {count} call(s)\n")
            print("| difference | calls |\n|---|---|")
            for d, c in by_cmd[cmd].most_common():
                print(f"| `{d}` | {c} |")
            print()
    else:
        print("None.\n")

    print(f"## Errors ({len(errors)} call(s))\n")
    if errors:
        grouped = collections.Counter()
        panics = [r for r in errors if r.get("panic")]
        for r in errors:
            first = (r.get("summary") or "").splitlines()[0] if r.get("summary") else f"exit {r.get('exit')}"
            grouped[(subcommand(r.get("argv", [])), first)] += 1
        if panics:
            print(f"**{len(panics)} panic(s)** — highest priority:\n")
            for r in panics:
                print(f"- `{' '.join(r.get('argv', []))}` in `{r.get('cwd')}` at {r.get('ts')}")
            print()
        print("| command | first line | calls |\n|---|---|---|")
        for (cmd, first), c in grouped.most_common():
            print(f"| `{cmd}` | {first} | {c} |")
        print()
    else:
        print("None.\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
