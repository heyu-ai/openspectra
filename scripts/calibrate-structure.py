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


ANCHOR_CAP = 50  # keep in sync with calibration::ANCHOR_CAP


def sh(args, cwd):
    """Run a command, failing loudly (returncode + stderr) instead of letting a
    silent failure surface later as an opaque JSON error."""
    r = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(f"command {args!r} failed (exit {r.returncode}): {r.stderr.strip()}")
    return r


def oracle_structure_score(oracle, repo, resolved_fp, broken_fp, broken_cf):
    """Build a one-change repo with the given broken composition, return
    (broken, total, score, broken_cf_observed) from the oracle's Structure
    dimension. `broken_cf_observed` is derived from the oracle's own
    broken_anchors categories, not the intended input, so verify-model checks
    against what the oracle actually saw."""
    if resolved_fp < 0:
        raise ValueError(f"negative resolved count ({resolved_fp}); check case construction")
    intended_total = resolved_fp + broken_fp + broken_cf
    if intended_total > ANCHOR_CAP:
        raise ValueError(f"intended total {intended_total} exceeds ANCHOR_CAP={ANCHOR_CAP}")
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
        # --no-gpg-sign: a global commit.gpgsign=true must not break the sweep
        ["git", "commit", "-q", "--no-gpg-sign", "-m", "x"],
    ):
        sh(c, repo)
    # A successful `spectra drift` always exits 0 regardless of severity
    # (probed 2026-07-04; the earlier "non-zero on medium/heavy" note here was
    # wrong), but errors exit 1 with a non-JSON message — so skip sh()'s
    # returncode guard and fail loudly only if stdout isn't JSON.
    out = subprocess.run([oracle, "drift", "c", "--json"], cwd=repo,
                         capture_output=True, text=True)
    try:
        rep = json.loads(out.stdout)
    except json.JSONDecodeError as e:
        raise RuntimeError(
            f"oracle produced no JSON (exit {out.returncode}): {out.stderr.strip() or out.stdout.strip()!r}"
        ) from e
    cf_observed = sum(1 for a in rep.get("broken_anchors", []) if a["category"] == "CliFlag")
    for d in rep["dimensions"]:
        if d["kind"] == "Structure":
            m = re.match(r"(\d+)/(\d+)", d["status"])
            b, t = (int(m.group(1)), int(m.group(2))) if m else (-1, -1)
            return b, t, d["score"], cf_observed
    raise RuntimeError("no Structure dimension in oracle output")


def model(broken, broken_cf, total):
    """The recovered formula — must match the oracle AND the shipped Rust
    `calibration::structure_score`. This is a deliberate re-implementation for
    black-box verification: verify-model checks THIS copy against the oracle. The
    Rust copy is pinned separately by its own unit tests (see calibration.rs);
    if you change the formula, update both and re-run `--mode verify-model`."""
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
            _, _, sc, _ = oracle_structure_score(oracle, repo, 8 - (fp + cf), fp, cf)
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
                _, t, sc, _ = oracle_structure_score(oracle, repo, total - count, fp, cf)
                cells.append(f"{count}/{t}->{sc}")
            print(f"    {cat:<9} " + "  ".join(cells))


def mode_verify_model(oracle, repo):
    """Cross-check the recovered formula against a spread of oracle runs.
    Each case is (broken_fp, broken_cf, resolved_fp) — resolved is passed
    directly (never re-subtracted), and the model is fed the oracle's *observed*
    broken-CliFlag count, so the check exercises the intended totals and does not
    assume its own conclusion."""
    print("== model vs oracle ==")
    bad = 0
    # total=8 mixed grid: resolved = 8 - broken
    cases = [(fp, cf, 8 - fp - cf) for fp in range(6) for cf in range(6)
             if 0 < fp + cf <= 6]
    # iso-decay count sweeps: total = count*frac, so resolved = total - broken
    cases += [(c, 0, c * f - c) for c in (1, 2, 3, 4, 6) for f in (2, 4, 8)]
    cases += [(0, c, c * f - c) for c in (1, 2, 3, 4, 6) for f in (2, 4, 8)]
    for fp, cf, resolved in cases:
        b, t, sc, cf_obs = oracle_structure_score(oracle, repo, resolved, fp, cf)
        if t != resolved + fp + cf:
            bad += 1
            print(f"  COMPOSITION fp={fp} cf={cf} resolved={resolved}: oracle total {t} != intended {resolved + fp + cf}")
            continue
        pred = model(b, cf_obs, t)
        if pred != sc:
            bad += 1
            print(f"  MISMATCH fp={fp} cf={cf} {b}/{t} cf_obs={cf_obs}: oracle={sc} model={pred}")
    print(f"  {len(cases)} cases, {bad} mismatches")
    return bad


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--oracle", required=True, help="path to the reference spectra binary")
    ap.add_argument("--mode", choices=["grid", "iso", "verify-model"], default="verify-model")
    ap.add_argument("--scratch", default=None, help="scratch dir (default: temp)")
    args = ap.parse_args()

    auto_scratch = args.scratch is None
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
        # Remove the whole auto-created temp dir (not just the nested repo), but
        # leave a user-supplied --scratch dir in place.
        cleanup = scratch if auto_scratch else repo
        if cleanup.exists():
            shutil.rmtree(cleanup)


if __name__ == "__main__":
    main()
