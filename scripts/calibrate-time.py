#!/usr/bin/env python3
"""Calibration harness for the Time dimension of `spectra drift`.

Feeds synthetic changes to the closed-source reference binary (the "oracle")
and observes its Time status word + score, to pin the day boundaries exactly
(rather than interpolating from sparse field samples). Companion to
`scripts/calibrate-structure.py`.

## Method

The Time dimension is a pure function of `days_old = today - created` (calendar
days), independent of Structure/Tasks. To isolate it, each synthetic change is
built with:
  * a controlled `created: <today - N days>` in `.openspec.yaml`,
  * an all-lowercase-prose `design.md` (no FilePath/CliFlag/Symbol anchors, so
    Structure = 0), and
  * a single pending task (Tasks = 0; collision detection never fires — see
    `calibration::TASKS_DETECTION_CALIBRATED`).

Sweeping `N` and reading the oracle's Time dimension pins every transition.

## Recovered boundaries (this harness's `--mode boundaries` reproduces them)

    days        status          score
    0..=6       fresh            0
    7..=21      aging            1
    22..=60     stale            2
    61..        abandoned        4    <- note: score jumps 2 -> 4, skipping 3

See `crates/spectra-core/src/calibration.rs::time_bucket` and the Time section
of `docs/reverse-engineering/drift.md`.

## Usage

    python3 scripts/calibrate-time.py --oracle /path/to/spectra [--mode boundaries|sweep] [--max-days 90]

Point --oracle at the reference binary (e.g.
/Applications/Spectra.app/Contents/MacOS/spectra). Requires `git` on PATH.
The `created` dates are computed relative to the machine's *current* date, and
the oracle echoes the day count back as "(Nd)", which the harness asserts
matches N so a clock/timezone skew can never silently mis-attribute a boundary.
"""
import argparse
import datetime
import json
import re
import shutil
import subprocess
import tempfile
from pathlib import Path


def sh(args, cwd):
    """Run a command, failing loudly (returncode + stderr) instead of letting a
    silent failure surface later as an opaque JSON error."""
    r = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"command {args!r} failed (exit {r.returncode}): {r.stderr.strip()}")
    return r


def oracle_time(oracle, repo, days):
    """Build a one-change repo whose only drift signal is age `days`, and return
    the oracle's Time (status_word, score). Asserts the oracle's echoed day
    count matches `days` so a date/timezone skew can't mis-attribute a boundary."""
    if repo.exists():
        shutil.rmtree(repo)
    (repo / "openspec/changes/c").mkdir(parents=True)
    (repo / ".spectra.yaml").write_text("spec_dir: openspec\n")
    created = (datetime.date.today() - datetime.timedelta(days=days)).isoformat()
    (repo / "openspec/changes/c/.openspec.yaml").write_text(
        f"schema: openspec/1\ncreated: {created}\n"
    )
    (repo / "openspec/changes/c/proposal.md").write_text("# p\n")
    (repo / "openspec/changes/c/tasks.md").write_text("- [ ] t\n")
    # all-lowercase prose -> no anchors extracted -> Structure contributes 0
    (repo / "openspec/changes/c/design.md").write_text(
        "# design\n\nplain lowercase prose with no code references at all\n"
    )
    for c in (
        ["git", "init", "-q"],
        ["git", "config", "user.email", "p@e.com"],
        ["git", "config", "user.name", "p"],
        ["git", "add", "-A"],
        ["git", "commit", "-q", "-m", "x"],
    ):
        sh(c, repo)
    # `spectra drift` exits non-zero on medium/heavy drift, so don't guard on
    # returncode here; fail loudly only if stdout isn't JSON.
    out = subprocess.run([oracle, "drift", "c", "--json"], cwd=repo,
                         capture_output=True, text=True)
    try:
        rep = json.loads(out.stdout)
    except json.JSONDecodeError as e:
        raise RuntimeError(
            f"oracle produced no JSON (exit {out.returncode}): {out.stderr.strip() or out.stdout.strip()!r}"
        ) from e
    for d in rep["dimensions"]:
        if d["kind"] == "Time":
            status = d["status"]
            m = re.search(r"\((\d+)d\)", status)
            if not m:
                raise RuntimeError(f"could not parse day count from Time status {status!r}")
            echoed = int(m.group(1))
            if echoed != days:
                raise RuntimeError(
                    f"oracle echoed {echoed}d but harness intended {days}d — "
                    "date/timezone skew; boundaries would be mis-attributed"
                )
            word = status.split(" ", 1)[0]
            return word, d["score"]
    raise RuntimeError("no Time dimension in oracle output")


def mode_sweep(oracle, repo, max_days):
    print(f"== Time sweep, days 0..{max_days} ==")
    print("days  status      score")
    for n in range(max_days + 1):
        word, score = oracle_time(oracle, repo, n)
        print(f"{n:>4}  {word:<10}  {score}")


def mode_boundaries(oracle, repo, max_days):
    """Scan 0..max_days, report each transition day exactly and the score ladder."""
    print(f"== Time boundaries (scanning days 0..{max_days}) ==")
    prev = None
    transitions = []
    for n in range(max_days + 1):
        word, score = oracle_time(oracle, repo, n)
        cur = (word, score)
        if prev is not None and cur != prev:
            transitions.append((n, prev, cur))
            print(f"  day {n}: {prev[0]}({prev[1]}) -> {cur[0]}({cur[1]})")
        prev = cur
    if not transitions:
        print("  (no transitions observed in range)")
    print(f"  {len(transitions)} transition(s) found; final state at "
          f"day {max_days}: {prev[0]}(score {prev[1]})")
    return transitions


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--oracle", required=True, help="path to the reference spectra binary")
    ap.add_argument("--mode", choices=["boundaries", "sweep"], default="boundaries")
    ap.add_argument("--max-days", type=int, default=90,
                    help="highest day count to probe (default 90; abandoned starts at 61)")
    ap.add_argument("--scratch", default=None, help="scratch dir (default: temp)")
    args = ap.parse_args()

    auto_scratch = args.scratch is None
    scratch = Path(args.scratch) if args.scratch else Path(tempfile.mkdtemp(prefix="calib-time-"))
    repo = scratch / "calib-repo"
    try:
        if args.mode == "sweep":
            mode_sweep(args.oracle, repo, args.max_days)
        else:
            mode_boundaries(args.oracle, repo, args.max_days)
    finally:
        cleanup = scratch if auto_scratch else repo
        if cleanup.exists():
            shutil.rmtree(cleanup)


if __name__ == "__main__":
    main()
