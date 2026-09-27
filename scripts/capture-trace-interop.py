#!/usr/bin/env python3
"""Verify or recapture how the Spectra 3.0.0 oracle interacts with the trace sidecar.

This is a verification contract, not a printer. Each probe runs in its own
throwaway git repo ("one probe jail, one operation"), records a fixed set of
observables, and the full set is compared with
``docs/reverse-engineering/golden/trace-interop-3.0.0.tsv``. Any difference
or failed step exits non-zero and keeps every jail created so far (their paths
are printed); jails are removed only after the comparison passes. ``--write``
regenerates the TSV and then verifies it again.

Probes (see ``docs/reverse-engineering/archive.md`` "Trace data"):

* ``mixed-archive-clean`` / ``mixed-archive-dirty``: a spec already migrated
  to ``spec.trace.yaml`` by OpenSpectra, then an ADDED + MODIFIED change
  archived by the oracle, with a clean tree and with one uncommitted file.
* ``renamed-validate-<n>``: each RENAMED delta spelling, validated by both
  the oracle and OpenSpectra.
* ``renamed-archive``: the spelling the oracle accepts, archived by the
  oracle, then ``trace migrate --check`` run by OpenSpectra.

The oracle is macOS-only. ``--spectra-bin`` overrides ``SPECTRA_BIN``, which
itself overrides the standard application path. ``--openspectra-bin``
defaults to ``target/release/spectra`` and must be built from clean source.
"""

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import NoReturn

EXPECTED_VERSION = "3.0.0"
REPO = Path(__file__).resolve().parent.parent
GOLDEN = REPO / "docs/reverse-engineering/golden/trace-interop-3.0.0.tsv"
CAP = "my-cap"

SPEC = """# my-cap Specification

## Purpose

Probe capability.

## Requirements

### Requirement: Alpha

The system SHALL do alpha.

#### Scenario: alpha-works -- Alpha works

- **WHEN** alpha runs
- **THEN** it SHALL succeed

<!-- @trace
source: seed-change
updated: 2026-01-01
code:
  - src/alpha.rs
-->

### Requirement: Beta

The system SHALL do beta.

#### Scenario: beta-works -- Beta works

- **WHEN** beta runs
- **THEN** it SHALL succeed
"""

ADDED_MODIFIED = """## ADDED Requirements

### Requirement: Gamma

The system SHALL do gamma.

#### Scenario: gamma-works -- Gamma works

- **WHEN** gamma runs
- **THEN** it SHALL succeed

## MODIFIED Requirements

### Requirement: Beta

The system SHALL do beta, modified.

#### Scenario: beta-works -- Beta works

- **WHEN** beta runs
- **THEN** it SHALL succeed
"""

RENAMED_SPELLINGS = (
    (
        "bullet-backtick-heading",
        "## RENAMED Requirements\n"
        "- FROM: `### Requirement: Alpha`\n"
        "- TO: `### Requirement: Omega`\n",
    ),
    (
        "bullet-backtick-heading-blank-line",
        "## RENAMED Requirements\n\n"
        "- FROM: `### Requirement: Alpha`\n"
        "- TO: `### Requirement: Omega`\n",
    ),
    (
        "bullet-plain-name",
        "## RENAMED Requirements\n\n- FROM: Alpha\n- TO: Omega\n",
    ),
    (
        "bullet-backtick-name",
        "## RENAMED Requirements\n\n- FROM: `Alpha`\n- TO: `Omega`\n",
    ),
    (
        "plain-heading",
        "## RENAMED Requirements\n\n"
        "FROM: ### Requirement: Alpha\n"
        "TO: ### Requirement: Omega\n",
    ),
)
ORACLE_RENAMED = dict(RENAMED_SPELLINGS)["plain-heading"]

PROPOSAL = "## Why\n\nProbe.\n\n## What Changes\n\n- Probe.\n\n## Impact\n\n- my-cap.\n"
TASKS = "## 1. Probe\n\n- [x] 1.1 Run the probe\n"
KEPT_JAILS: list[Path] = []


def fail(message: str) -> NoReturn:
    for jail in KEPT_JAILS:
        print(f"[KEPT] {jail}", file=sys.stderr)
    print(f"[FAIL] {message}", file=sys.stderr)
    raise SystemExit(2)


def run(argv: list[str], cwd: Path) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(
            argv,
            cwd=cwd,
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            check=False,
            timeout=120,
        )
    except subprocess.TimeoutExpired as error:
        fail(f"指令逾時（120 秒）：{argv!r}；{error}。")
    except OSError as error:
        fail(f"無法執行指令：{argv!r}；{error}。")


def git(jail: Path, *args: str) -> None:
    result = run(
        ["git", "-c", "user.email=probe@example.invalid", "-c", "user.name=probe", *args],
        jail,
    )
    if result.returncode != 0:
        fail(f"git {' '.join(args)} 失敗：{result.stderr.strip()}")


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def spec_path(jail: Path) -> Path:
    return jail / "openspec/specs" / CAP / "spec.md"


def sidecar_path(jail: Path) -> Path:
    return jail / "openspec/specs" / CAP / "spec.trace.yaml"


def headings(jail: Path) -> str:
    names = re.findall(r"^### Requirement: (.+)$", spec_path(jail).read_text(), re.M)
    return ",".join(names)


def inline_footers(jail: Path) -> int:
    return sum(
        1
        for line in spec_path(jail).read_text().splitlines()
        if line.strip() == "<!-- @trace"
    )


def new_jail(openspectra: Path, label: str) -> Path:
    """A committed repo whose spec has already been migrated to the sidecar."""
    jail = Path(tempfile.mkdtemp(prefix=f"trace-interop-{label}-"))
    KEPT_JAILS.append(jail)
    (jail / ".spectra.yaml").write_text("spec_dir: openspec\nworktree: false\n")
    spec_path(jail).parent.mkdir(parents=True)
    spec_path(jail).write_text(SPEC)
    (jail / "openspec/changes").mkdir(parents=True)
    git(jail, "init", "--quiet")
    git(jail, "add", "-A")
    git(jail, "commit", "--quiet", "-m", "seed")
    migrate = run([str(openspectra), "trace", "migrate", "--no-color"], jail)
    if migrate.returncode != 0 or not sidecar_path(jail).is_file():
        fail(f"{label}: openspectra trace migrate 沒有產生 sidecar：{migrate.stderr.strip()}")
    if inline_footers(jail) != 0:
        fail(f"{label}: migrate 後 spec.md 仍有 inline footer，前提不成立。")
    git(jail, "add", "-A")
    git(jail, "commit", "--quiet", "-m", "migrated")
    return jail


def write_change(jail: Path, name: str, delta: str) -> None:
    root = jail / "openspec/changes" / name
    (root / "specs" / CAP).mkdir(parents=True)
    (root / "proposal.md").write_text(PROPOSAL)
    (root / "tasks.md").write_text(TASKS)
    (root / "specs" / CAP / "spec.md").write_text(delta)
    git(jail, "add", "-A")
    git(jail, "commit", "--quiet", "-m", name)


def first_line(text: str) -> str:
    line = next((raw.strip() for raw in text.splitlines() if raw.strip()), "")
    line = re.sub(r"\d{4}-\d{2}-\d{2}-", "<date>-", line)
    return line.replace("\t", " ")


def probe_mixed_archive(
    oracle: Path, openspectra: Path, dirty: bool
) -> list[tuple[str, str, str]]:
    """``dirty`` leaves an uncommitted file in the tree before the oracle archives."""
    label = "mixed-archive-dirty" if dirty else "mixed-archive-clean"
    jail = new_jail(openspectra, label)
    before = sha256(sidecar_path(jail))
    write_change(jail, "probe", ADDED_MODIFIED)
    if dirty:
        (jail / "src").mkdir()
        (jail / "src/dirty.rs").write_text("// uncommitted\n")
    result = run([str(oracle), "archive", "probe", "--yes", "--no-color"], jail)
    applied = next((l for l in result.stdout.splitlines() if l.startswith("Specs applied")), "")
    spec = spec_path(jail).read_text()
    rows = [
        (label, "oracle_archive_rc", str(result.returncode)),
        (label, "oracle_specs_applied", applied.strip()),
        (label, "sidecar_unchanged", str(sha256(sidecar_path(jail)) == before).lower()),
        (label, "pointer_kept", str("<!-- @trace-sidecar: spec.trace.yaml -->" in spec).lower()),
        (label, "inline_footers_after", str(inline_footers(jail))),
        (label, "footer_lists_dirty_file", str("src/dirty.rs" in spec).lower()),
        (label, "headings_after", headings(jail)),
    ]
    if dirty:
        check = run([str(openspectra), "trace", "migrate", "--check", "--no-color"], jail)
        rows.append((label, "openspectra_check_rc", str(check.returncode)))
    return rows


def probe_renamed_validate(oracle: Path, openspectra: Path) -> list[tuple[str, str, str]]:
    rows = []
    for label, delta in RENAMED_SPELLINGS:
        for tool, binary in (("oracle", oracle), ("openspectra", openspectra)):
            jail = new_jail(openspectra, f"renamed-{label}-{tool}")
            write_change(jail, "probe", delta)
            result = run([str(binary), "validate", "probe", "--no-color"], jail)
            rows.append((f"renamed-validate-{label}", f"{tool}_validate_rc", str(result.returncode)))
    return rows


def probe_renamed_archive(oracle: Path, openspectra: Path) -> list[tuple[str, str, str]]:
    jail = new_jail(openspectra, "renamed-archive")
    before = sha256(sidecar_path(jail))
    write_change(jail, "probe", ORACLE_RENAMED)
    result = run([str(oracle), "archive", "probe", "--yes", "--no-color"], jail)
    applied = next((l for l in result.stdout.splitlines() if l.startswith("Specs applied")), "")
    check = run([str(openspectra), "trace", "migrate", "--check", "--no-color"], jail)
    stale = next((l for l in (check.stdout + check.stderr).splitlines() if "names requirement(s) not in spec.md" in l), "")
    rows = [
        ("renamed-archive", "oracle_archive_rc", str(result.returncode)),
        ("renamed-archive", "oracle_specs_applied", applied.strip()),
        ("renamed-archive", "headings_after", headings(jail)),
        ("renamed-archive", "sidecar_unchanged", str(sha256(sidecar_path(jail)) == before).lower()),
        ("renamed-archive", "openspectra_check_rc", str(check.returncode)),
        ("renamed-archive", "openspectra_check_stale", first_line(stale)),
    ]
    return rows


def render(rows: list[tuple[str, str, str]]) -> str:
    lines = ["probe\tobservable\tvalue"]
    lines += ["\t".join(row) for row in rows]
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="重寫 golden TSV 後再驗證一次。")
    parser.add_argument(
        "--spectra-bin",
        default=os.environ.get("SPECTRA_BIN", "/Applications/Spectra.app/Contents/MacOS/spectra"),
        help="參考執行檔路徑（也可設定 SPECTRA_BIN）。",
    )
    parser.add_argument(
        "--openspectra-bin",
        default=str(REPO / "target/release/spectra"),
        help="OpenSpectra 執行檔路徑（需由乾淨原始碼建置）。",
    )
    args = parser.parse_args()
    oracle = Path(args.spectra_bin)
    openspectra = Path(args.openspectra_bin)
    for binary in (oracle, openspectra):
        if not binary.is_file():
            fail(f"找不到執行檔：{binary}")
    version = run([str(oracle), "--version"], REPO)
    if version.returncode != 0 or f"spectra {EXPECTED_VERSION}" not in version.stdout:
        fail(f"參考執行檔版本不是 {EXPECTED_VERSION}：{version.stdout.strip()!r}")
    if run([str(openspectra), "trace", "--help"], REPO).returncode != 0:
        fail(f"{openspectra} 不支援 trace 子指令，可能不是 OpenSpectra 執行檔。")

    rows = [("oracle", "version", EXPECTED_VERSION)]
    rows += probe_mixed_archive(oracle, openspectra, dirty=False)
    rows += probe_mixed_archive(oracle, openspectra, dirty=True)
    rows += probe_renamed_validate(oracle, openspectra)
    rows += probe_renamed_archive(oracle, openspectra)
    actual = render(rows)

    if args.write:
        GOLDEN.write_text(actual)
        print(f"[WRITE] {GOLDEN.relative_to(REPO)}（{len(rows)} 列）")
    if not GOLDEN.is_file():
        fail(f"{GOLDEN.relative_to(REPO)} 不存在；先用 --write 產生。")
    expected = GOLDEN.read_text()
    if expected != actual:
        diff = [
            f"  golden: {e!r}\n  actual: {a!r}"
            for e, a in zip(expected.splitlines(), actual.splitlines())
            if e != a
        ]
        if len(expected.splitlines()) != len(actual.splitlines()):
            diff.append(f"  列數不同：golden {len(expected.splitlines())}、actual {len(actual.splitlines())}")
        fail("oracle 行為與 golden 不符：\n" + "\n".join(diff))
    print(f"[OK] {len(rows)} 個觀察值與 {GOLDEN.relative_to(REPO)} 相符")
    for jail in KEPT_JAILS:
        shutil.rmtree(jail)


if __name__ == "__main__":
    main()
