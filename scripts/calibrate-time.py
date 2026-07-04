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
  * an all-lowercase-prose `design.md` (no FilePath/CliFlag/Function/Symbol
    anchors, so Structure = 0), and
  * a single pending task (Tasks = 0; collision detection never fires — see
    `calibration::TASKS_DETECTION_CALIBRATED`).

Sweeping `N` and reading the oracle's Time dimension pins every transition.

## Recovered boundaries (`--mode boundaries` verifies these; exits non-zero on
## drift from the pinned values or when the scan does not cover all of them)

    days        status          score
    0..=6       fresh            0
    7..=21      aging            1
    22..=60     stale            2
    61..        abandoned        4    <- note: score jumps 2 -> 4, skipping 3

No further transition exists above 61: probed out to 3650 days, all abandoned
score 4. A future `created` (negative N) is clamped by the oracle to "fresh
(0d)". See `crates/spectra-core/src/calibration.rs::time_bucket` and the Time
section of `docs/reverse-engineering/drift.md`.

## Usage

    python3 scripts/calibrate-time.py --oracle /path/to/spectra \
        [--mode boundaries|sweep] [--max-days 90] [--scratch DIR]

Point --oracle at the reference binary (e.g.
/Applications/Spectra.app/Contents/MacOS/spectra). Requires `git` on PATH.
The `created` dates are computed relative to the machine's *current* date, and
the oracle echoes the day count back as "(Nd)", which the harness asserts
matches N so a clock skew can never silently mis-attribute a boundary (one
retry absorbs a local-midnight rollover mid-sweep). On failure the synthetic
repo is preserved and its path printed, for inspection.
"""
import argparse
import datetime
import json
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

# The pinned transitions `--mode boundaries` verifies against:
# first day of each new state -> (status word, score).
EXPECTED_TRANSITIONS = {7: ("aging", 1), 22: ("stale", 2), 61: ("abandoned", 4)}


def sh(args, cwd):
    """Run a command, failing loudly (returncode + diagnostics) instead of
    letting a silent failure surface later as an opaque JSON error."""
    r = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        detail = r.stderr.strip() or r.stdout.strip()
        raise RuntimeError(f"command {args!r} failed (exit {r.returncode}): {detail}")
    return r


def nonnegative_int(value):
    n = int(value)
    if n < 0:
        raise argparse.ArgumentTypeError(f"must be >= 0, got {n}")
    return n


def build_repo(repo, days):
    """(Re)build a one-change repo whose only drift signal is age `days`."""
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
        # --no-gpg-sign: a global commit.gpgsign=true must not break the sweep
        ["git", "commit", "-q", "--no-gpg-sign", "-m", "x"],
    ):
        sh(c, repo)


def oracle_time_once(oracle, repo, days):
    """One probe: build the repo and return the oracle's Time (word, score).
    Raises RuntimeError with full oracle context on any unexpected shape."""
    build_repo(repo, days)
    # A successful `spectra drift` always exits 0 (probed across all
    # severities), but errors exit 1 with a non-JSON message, so fail loudly
    # on unparseable stdout rather than guarding the returncode.
    out = subprocess.run([oracle, "drift", "c", "--json"], cwd=repo,
                         capture_output=True, text=True)
    try:
        rep = json.loads(out.stdout)
    except json.JSONDecodeError as e:
        raise RuntimeError(
            f"oracle produced no JSON (exit {out.returncode}): {out.stderr.strip() or out.stdout.strip()!r}"
        ) from e
    dims = rep.get("dimensions") if isinstance(rep, dict) else None
    if not isinstance(dims, list):
        raise RuntimeError(
            f"oracle JSON has no 'dimensions' list (exit {out.returncode}): "
            f"{out.stderr.strip() or json.dumps(rep)[:200]!r}"
        )
    for d in dims:
        if isinstance(d, dict) and d.get("kind") == "Time":
            status = d.get("status", "")
            m = re.search(r"\((-?\d+)d\)", status)
            if not m:
                raise RuntimeError(
                    f"could not parse day count from Time status {status!r} "
                    f"(exit {out.returncode})"
                )
            echoed = int(m.group(1))
            word = status.split(" ", 1)[0]
            return word, d.get("score"), echoed
    raise RuntimeError(
        f"no Time dimension in oracle output (exit {out.returncode}): "
        f"{out.stderr.strip() or json.dumps(rep)[:200]!r}"
    )


def oracle_time(oracle, repo, days):
    """Probe with the echoed-day assertion, retrying once so a local-midnight
    rollover between repo build and oracle run aborts nothing. The oracle
    clamps future dates to 0d, so a (hypothetical) negative-days probe expects
    an echo of 0, not the negative N — the CLI itself only sweeps 0..max_days."""
    for attempt in (1, 2):
        word, score, echoed = oracle_time_once(oracle, repo, days)
        if echoed == max(0, days):
            return word, score
        if attempt == 1:
            continue  # rebuild against the (possibly new) current date
    raise RuntimeError(
        f"oracle echoed {echoed}d but harness intended {days}d twice in a row "
        "— persistent date/timezone skew; boundaries would be mis-attributed"
    )


def mode_sweep(oracle, repo, max_days):
    print(f"== Time sweep, days 0..{max_days} ==")
    print("days  status      score")
    for n in range(max_days + 1):
        word, score = oracle_time(oracle, repo, n)
        print(f"{n:>4}  {word:<10}  {score}")
    return 0


def mode_boundaries(oracle, repo, max_days):
    """Scan 0..max_days, report each transition day, and verify the recovered
    transitions against EXPECTED_TRANSITIONS. Returns a process exit code:
    non-zero when the oracle's boundaries drifted from the pinned values."""
    print(f"== Time boundaries (scanning days 0..{max_days}) ==")
    prev = None
    transitions = {}
    for n in range(max_days + 1):
        word, score = oracle_time(oracle, repo, n)
        cur = (word, score)
        if prev is not None and cur != prev:
            transitions[n] = cur
            print(f"  day {n}: {prev[0]}({prev[1]}) -> {cur[0]}({cur[1]})")
        prev = cur
    if prev is None:
        print("  (no days probed)")
        return 1
    print(f"  {len(transitions)} transition(s) found; final state at "
          f"day {max_days}: {prev[0]}(score {prev[1]})")

    expected_in_range = {d: s for d, s in EXPECTED_TRANSITIONS.items() if d <= max_days}
    if transitions != expected_in_range:
        print(f"  [MISMATCH] expected {expected_in_range}, got {transitions} "
              "— oracle boundaries drifted from calibration.rs; recalibrate")
        return 1
    if len(expected_in_range) < len(EXPECTED_TRANSITIONS):
        # A truncated scan must not read as "verified": exit non-zero so a
        # scripted run can't mistake partial coverage for a full verification.
        print(f"  [FAIL] scan truncated: --max-days {max_days} covers only "
              f"{len(expected_in_range)}/{len(EXPECTED_TRANSITIONS)} pinned transitions; "
              f"use --max-days >= {max(EXPECTED_TRANSITIONS)} for a full verification")
        return 1
    print(f"  [OK] transitions match the pinned values ({sorted(expected_in_range)})")
    return 0


def main():
    ap = argparse.ArgumentParser()
    # Resolved to an absolute path because probes run with cwd=<synthetic repo>,
    # where a relative path like target/release/spectra would no longer resolve.
    ap.add_argument("--oracle", required=True, type=lambda p: str(Path(p).resolve()),
                    help="path to the reference spectra binary")
    ap.add_argument("--mode", choices=["boundaries", "sweep"], default="boundaries")
    ap.add_argument("--max-days", type=nonnegative_int, default=90,
                    help="highest day count to probe (default 90; abandoned starts at 61)")
    ap.add_argument("--scratch", default=None, help="scratch dir (default: temp)")
    args = ap.parse_args()

    auto_scratch = args.scratch is None
    scratch = Path(args.scratch) if args.scratch else Path(tempfile.mkdtemp(prefix="calib-time-"))
    repo = scratch / "calib-repo"
    try:
        if args.mode == "sweep":
            code = mode_sweep(args.oracle, repo, args.max_days)
        else:
            code = mode_boundaries(args.oracle, repo, args.max_days)
    except BaseException:
        # Preserve the failing synthetic repo: its created dates are relative
        # to today, so a later manual rebuild would not reproduce it exactly.
        print(f"[FAIL] scratch preserved for inspection: {scratch}")
        raise
    if code != 0:
        # Verification failures (mismatch / truncated scan) preserve the
        # scratch too — the docstring promises inspectability on any failure.
        print(f"[FAIL] scratch preserved for inspection: {scratch}")
        raise SystemExit(code)
    cleanup = scratch if auto_scratch else repo
    if cleanup.exists():
        shutil.rmtree(cleanup)
    raise SystemExit(code)


if __name__ == "__main__":
    main()
