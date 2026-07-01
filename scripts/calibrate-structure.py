#!/usr/bin/env python3
"""Calibration harness for the Structure dimension of `spectra drift`.

Feeds synthetic changes to the closed-source reference binary (the "oracle")
and observes its Structure score, to reverse-engineer the scoring formula.

## Method

Only two anchor categories can be *forced broken* on a committed change:
  * FilePath  — reference a nonexistent `src/ghostN.rs` (filesystem/index lookup)
  * CliFlag   — any `--flagN` is always "not in --help"
Function/Symbol anchors self-match the tracked `design.md` (the oracle greps
its own design file), so they can never be broken on a committed change. The
generated `design.md` is all-lowercase prose so the Symbol regex extracts
nothing and `total` is exactly (resolved + broken FilePaths + broken CliFlags).

## Recovered formula (this harness verifies it end-to-end)

    decay = broken / total
    D = 0 if decay < 10% | 1 if 10% <= decay < 30% | 2 if decay >= 30%
    structure_score = min(2*D + 3,  2*D + broken_cliflag_count)

FilePath (non-CliFlag) broken anchors contribute only through `D`; each broken
CliFlag adds 1 to the additive term, capped by 2*D + 3 (global max 7). See
`crates/spectra-core/src/calibration.rs::structure_score`.

## Usage

    python3 scripts/calibrate-structure.py --oracle /path/to/spectra [--mode grid|iso|verify-model]

Requires a scratch dir (default: a temp dir) and `git` on PATH. Point --oracle
at the reference binary (e.g. /Applications/Spectra.app/Contents/MacOS/spectra).
"""
import argparse
import json
import re
import shutil
import subprocess
import tempfile
from pathlib import Path


def sh(args, cwd):
    return subprocess.run(args, cwd=cwd, capture_output=True, text=True)


def oracle_structure_score(oracle, repo, resolved_fp, broken_fp, broken_cf):
    """Build a one-change repo with the given broken composition, return
    (broken, total, score) from the oracle's Structure dimension."""
    if repo.exists():
        shutil.rmtree(repo)
    (repo / "src").mkdir(parents=True)
    (repo / "openspec/changes/c").mkdir(parents=True)
    (repo / ".spectra.yaml").write_text("spec_dir: openspec\n")
    (repo / "openspec/changes/c/.openspec.yaml").write_text(
        "schema: openspec/1\ncreated: 2026-06-28\n"
    )
    (repo / "openspec/changes/c/proposal.md").write_text("# p\n")
    (repo / "openspec/changes/c/tasks.md").write_text("- [ ] t\n")

    resolved = []
    for i in range(resolved_fp):
        p = f"src/real{i}.rs"
        (repo / p).write_text("fn f(){}\n")
        resolved.append(p)
    broken_paths = [f"src/ghost{i}.rs" for i in range(broken_fp)]
    flags = [f"--flag{i}" for i in range(broken_cf)]
    # all-lowercase prose -> Symbol regex ( \b[A-Z]... ) matches nothing
    body = ["# design", "", "refs:"] + resolved + broken_paths
    body.append("flags: " + " ".join(flags))
    (repo / "openspec/changes/c/design.md").write_text("\n".join(body) + "\n")

    for c in (
        ["git", "init", "-q"],
        ["git", "config", "user.email", "p@e.com"],
        ["git", "config", "user.name", "p"],
        ["git", "add", "-A"],
        ["git", "commit", "-q", "-m", "x"],
    ):
        sh(c, repo)
    out = sh([oracle, "drift", "c", "--json"], repo)
    rep = json.loads(out.stdout)
    for d in rep["dimensions"]:
        if d["kind"] == "Structure":
            m = re.match(r"(\d+)/(\d+)", d["status"])
            b, t = (int(m.group(1)), int(m.group(2))) if m else (-1, -1)
            return b, t, d["score"]
    raise RuntimeError("no Structure dimension in oracle output")


def model(broken, broken_cf, total):
    """The recovered formula — must match the oracle."""
    if total == 0 or broken == 0:
        return 0
    decay = broken / total
    d = 0 if decay < 0.10 else (1 if decay < 0.30 else 2)
    return min(2 * d + 3, 2 * d + broken_cf)


def mode_grid(oracle, repo):
    print("== total=8 grid: rows=broken FilePath, cols=broken CliFlag ==")
    print("fp\\cf " + "".join(f"{cf:>4}" for cf in range(6)))
    for fp in range(6):
        row = []
        for cf in range(6):
            if fp + cf == 0 or fp + cf > 6:
                row.append("   -")
                continue
            _, _, sc = oracle_structure_score(oracle, repo, 8 - (fp + cf), fp, cf)
            row.append(f"{sc:>4}")
        print(f"{fp:>4} " + "".join(row))


def mode_iso(oracle, repo):
    print("== iso-decay sweeps (count isolated at fixed decay) ==")
    for label, frac in [("12.5%", 8), ("25%", 4), ("50%", 2)]:
        print(f"  decay {label}:")
        for cat, (bfp, bcf) in [("FilePath", (1, 0)), ("CliFlag", (0, 1))]:
            cells = []
            for count in [1, 2, 3, 4, 6]:
                total = count * frac
                fp, cf = (count, 0) if cat == "FilePath" else (0, count)
                _, t, sc = oracle_structure_score(oracle, repo, total - count, fp, cf)
                cells.append(f"{count}/{t}->{sc}")
            print(f"    {cat:<9} " + "  ".join(cells))


def mode_verify_model(oracle, repo):
    """Cross-check the recovered formula against a spread of oracle runs."""
    print("== model vs oracle ==")
    bad = 0
    cases = [(fp, cf, 8 - fp - cf) for fp in range(6) for cf in range(6)
             if 0 < fp + cf <= 6]
    cases += [(c, 0, c * f) for c in (1, 2, 3, 4, 6) for f in (2, 4, 8)]
    cases += [(0, c, c * f) for c in (1, 2, 3, 4, 6) for f in (2, 4, 8)]
    for fp, cf, total in cases:
        b, t, sc = oracle_structure_score(oracle, repo, total - fp - cf, fp, cf)
        pred = model(b, cf, t)
        if pred != sc:
            bad += 1
            print(f"  MISMATCH fp={fp} cf={cf} {b}/{t}: oracle={sc} model={pred}")
    print(f"  {len(cases)} cases, {bad} mismatches")
    return bad


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--oracle", required=True, help="path to the reference spectra binary")
    ap.add_argument("--mode", choices=["grid", "iso", "verify-model"], default="verify-model")
    ap.add_argument("--scratch", default=None, help="scratch dir (default: temp)")
    args = ap.parse_args()

    scratch = Path(args.scratch) if args.scratch else Path(tempfile.mkdtemp(prefix="calib-"))
    repo = scratch / "calib-repo"
    try:
        if args.mode == "grid":
            mode_grid(args.oracle, repo)
        elif args.mode == "iso":
            mode_iso(args.oracle, repo)
        else:
            raise SystemExit(1 if mode_verify_model(args.oracle, repo) else 0)
    finally:
        if repo.exists():
            shutil.rmtree(repo)


if __name__ == "__main__":
    main()
