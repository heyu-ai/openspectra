#!/usr/bin/env python3
"""Calibration harness for the anchor budget of `spectra drift`.

Feeds synthetic changes to the closed-source reference binary (the "oracle")
and checks the recovered budget model against what it actually reports.

## Method

Anchors that resolve are invisible in `drift --json` -- only broken ones are
listed. So each jail leaves `design.md` UNTRACKED: `git grep` then resolves
nothing, every extracted anchor is reported, and the oracle's otherwise-opaque
anchor set becomes observable (see docs/reverse-engineering/drift.md,
"Reproducing the oracle", step 2). One jail per case, used once, so no later
step can overwrite the state an earlier one produced.

## Recovered model (this harness verifies it end-to-end)

    candidates = per-category extraction, deduped, in document order
    if sum(len(c) for c in categories) <= ANCHOR_CAP:   # 50
        every candidate is checked
    else:
        each category keeps indices  i * n // 12,  i in 0..11

`ANCHOR_CAP` is a TRIGGER, not a truncation length: 51 candidates yield 12
checked anchors, not 50. A category with fewer than 12 candidates survives
whole, so the total above the cap is `sum(min(n_category, 12))` -- the 12-21
range seen in real oracle output. See
`crates/spectra-core/src/calibration.rs::ANCHOR_SAMPLE_PER_CATEGORY` and
`anchors::apply_anchor_budget`.

This is a verification contract, not a printer: every case asserts on the
oracle's exact anchor *identities* (not just counts), the run exits non-zero on
the first divergence, and the failing jail is preserved for inspection.

## Usage

    python3 scripts/calibrate-anchor-budget.py --oracle /path/to/spectra

Requires macOS + the reference binary + `git` on PATH; not runnable in CI.
"""
import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# keep in sync with calibration::ANCHOR_CAP / ANCHOR_SAMPLE_PER_CATEGORY
ANCHOR_CAP = 50
PER_CATEGORY = 12


def sh(args, cwd):
    """Run a command, failing loudly instead of letting a silent failure
    surface later as an opaque JSON error."""
    r = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"command {args!r} failed (exit {r.returncode}): {r.stderr.strip()}")
    return r


def oracle_anchors(oracle, body, keep_on_failure):
    """Build a one-change jail whose design.md is `body`, return the set of
    (category, anchor) pairs the oracle reports."""
    repo = Path(tempfile.mkdtemp(prefix="spectra-anchor-budget-"))
    (repo / "src").mkdir(parents=True)
    (repo / "openspec/changes/c").mkdir(parents=True)
    (repo / ".spectra.yaml").write_text("spec_dir: openspec\n")
    (repo / "openspec/changes/c/.openspec.yaml").write_text(
        "schema: openspec/1\ncreated: 2026-06-28\n"
    )
    (repo / "openspec/changes/c/proposal.md").write_text("# p\n")
    (repo / "openspec/changes/c/tasks.md").write_text("- [ ] t\n")
    (repo / "src/keep.rs").write_text("fn keep(){}\n")
    for c in (
        ["git", "init", "-q"],
        ["git", "config", "user.email", "p@e.com"],
        ["git", "config", "user.name", "p"],
        ["git", "add", "-A"],
        # --no-gpg-sign: a global commit.gpgsign=true must not break the sweep
        ["git", "commit", "-q", "--no-gpg-sign", "-m", "x"],
    ):
        sh(c, repo)
    # written after the commit, so design.md stays untracked on purpose
    (repo / "openspec/changes/c/design.md").write_text(body)

    # A successful `spectra drift` always exits 0 regardless of severity, but
    # errors exit 1 with a non-JSON message -- so fail loudly only on non-JSON.
    out = subprocess.run([oracle, "drift", "c", "--json"], cwd=repo,
                         capture_output=True, text=True)
    try:
        rep = json.loads(out.stdout)
    except json.JSONDecodeError as e:
        raise RuntimeError(
            f"oracle produced no JSON (exit {out.returncode}): "
            f"{(out.stderr or out.stdout).strip()!r}\njail kept at {repo}"
        ) from e
    if keep_on_failure:
        print(f"    (jail: {repo})")
    else:
        shutil.rmtree(repo)
    return {(a["category"], a["anchor"]) for a in rep.get("broken_anchors", [])}


def sample(names):
    """The recovered model for one category: evenly-spaced downsample to <=12."""
    n = len(names)
    return [names[j] for j in sorted({(i * n) // PER_CATEGORY for i in range(PER_CATEGORY)})]


def model(categories):
    """categories: list of (category_name, [anchor strings in document order])."""
    if sum(len(v) for _, v in categories) <= ANCHOR_CAP:
        return {(k, name) for k, v in categories for name in v}
    return {(k, name) for k, v in categories for name in sample(v)}


def render(categories):
    """Emit a design.md that produces exactly `categories` as candidates."""
    lines = ["# design", ""]
    for kind, names in categories:
        if kind == "Function":
            lines.append(" ".join(f"{n}()" for n in names))
        else:
            lines.append(" ".join(names))
    return "\n".join(lines) + "\n"


def flags(n):
    return ("CliFlag", [f"--flag{i:03d}" for i in range(n)])


def paths(n):
    return ("FilePath", [f"src/g{i:03d}.rs" for i in range(n)])


def syms(n):
    return ("Symbol", [f"Zsym{i:03d}" for i in range(n)])


def fns(n):
    return ("Function", [f"zfn_{i:03d}" for i in range(n)])


CASES = [
    # the trigger boundary: at the cap nothing is dropped, one past it the
    # whole category collapses to 12 -- this is what rules out "truncate(50)"
    ("boundary-at-cap", [flags(ANCHOR_CAP)]),
    ("boundary-past-cap", [flags(ANCHOR_CAP + 1)]),
    # every category downsamples the same way
    ("cliflag-100", [flags(100)]),
    ("filepath-60", [paths(60)]),
    ("symbol-83", [syms(83)]),
    ("function-70", [fns(70)]),
    # the trigger reads the TOTAL; the sampling is per category
    ("mixed-under-cap", [flags(20), paths(20)]),
    ("mixed-over-cap", [flags(26), paths(25)]),
    # a category below 12 survives whole while a large one is sampled
    ("uneven-45sym-10cf", [syms(45), flags(10)]),
    ("all-four-20each", [paths(20), flags(20), fns(20), syms(20)]),
    # sizes that stress the integer division
    ("cliflag-137", [flags(137)]),
]


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--oracle", required=True,
                    help="path to the reference binary (e.g. ~/.local/bin/spectra)")
    args = ap.parse_args()

    failures = []
    for label, categories in CASES:
        expected = model(categories)
        observed = oracle_anchors(args.oracle, render(categories), keep_on_failure=False)
        ok = expected == observed
        total = sum(len(v) for _, v in categories)
        print(f"{'PASS' if ok else 'FAIL'}  {label:<20} candidates={total:<4} "
              f"model={len(expected):<3} oracle={len(observed)}")
        if not ok:
            failures.append(label)
            print(f"        model-only : {sorted(expected - observed)[:12]}")
            print(f"        oracle-only: {sorted(observed - expected)[:12]}")
            # re-run this case keeping the jail, so the divergence is inspectable
            oracle_anchors(args.oracle, render(categories), keep_on_failure=True)

    print()
    if failures:
        print(f"MODEL REJECTED -- {len(failures)} case(s) diverged: {failures}")
        print("Update calibration::ANCHOR_SAMPLE_PER_CATEGORY / "
              "anchors::apply_anchor_budget and the RE doc together.")
        return 1
    print(f"MODEL CONFIRMED on all {len(CASES)} cases "
          f"(anchor identities, not just counts).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
