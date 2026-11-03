#!/usr/bin/env python3
"""Deterministic workflow gate for OpenSpectra propose and apply outputs (Phase 1).

Dependency-free Python 3 script enforcing the mechanical invariants validated in
docs/research/skill-evaluation-2026-11.md and ADR-0001:

1. `propose <change_dir>`:
   - Verifies `.openspec.yaml` exists and retains a valid `schema` key
     (guards against single-key overwrite with `skip_specs: true`).
   - Enforces `no-spec` vs `spec-driven` delta spec presence and OpenSpec section headers.
   - Guards against direct binary invocations passing `--` (`./target/... -- <arg>`)
     when the target project's `src/main.rs` parses `args().nth(1)` directly without
     a flag parser (`clap`, `lexopt`, `argh`, `pico_args`, `getopts`).

2. `apply --transcript <jsonl> --final <md> --tasks <tasks.md> [--config <.spectra.yaml>]`:
   - Rejects nonexistent CLI subcommands in the final handoff (`spectra verify`,
     `spectra review`, `spectra apply`, `spectra audit`) while allowing `/spectra:*` skills.
   - When TDD is active and tasks are marked completed (`- [x]`), verifies that the
     execution transcript contains an observed behavioral test failure (RED, excluding
     compile/syntax errors) followed by a passing test run (GREEN).
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import sys

DELTA_HEADER_RE = re.compile(
    r"^##\s+(?:ADDED|MODIFIED|REMOVED|RENAMED)\s+Requirements\b",
    re.IGNORECASE | re.MULTILINE,
)
DIRECT_BIN_DASH_RE = re.compile(r"(?:\./\S+|target/(?:debug|release)/\S+)\s+--\s+(\S+)")
FLAG_CRATES_RE = re.compile(r"\b(?:clap|lexopt|argh|pico_args|getopts|structopt)\b")
INVENTED_SUBCMD_RE = re.compile(r"(?<![/\w:-])spectra\s+(verify|review|apply|audit)\b")
COMPLETED_TASK_RE = re.compile(r"^\s*[-*+]\s*\[[xX]\]\s+\S+", re.MULTILINE)
COMPILE_ERROR_RE = re.compile(
    r"(?:could not compile|error\[E\d+\]:|SyntaxError:|IndentationError:|cannot find (?:value|function|type|macro))",
    re.IGNORECASE,
)
BEHAVIORAL_FAIL_RE = re.compile(
    r"(?:test result:\s*FAILED|FAILED\s*\(failures=|\bAssertionError\b|panicked at|failures:)",
    re.IGNORECASE,
)
PASS_RESULT_RE = re.compile(
    r"(?:test result:\s*ok\b|^OK\b|\b\d+\s+passed\b)",
    re.IGNORECASE | re.MULTILINE,
)


def parse_flat_yaml(text: str) -> dict[str, object]:
    """Parse flat key: value YAML files (.openspec.yaml / .spectra.yaml) using stdlib."""
    data: dict[str, object] = {}
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        if ":" not in line:
            raise ValueError(f"Expected key: value line, got: {raw_line!r}")
        key, val = line.split(":", 1)
        key = key.strip()
        val = val.strip()
        if not key:
            raise ValueError(f"Empty YAML key in line: {raw_line!r}")
        if (val.startswith('"') and val.endswith('"')) or (val.startswith("'") and val.endswith("'")):
            parsed: object = val[1:-1]
        elif val.lower() == "true":
            parsed = True
        elif val.lower() == "false":
            parsed = False
        else:
            parsed = val
        data[key] = parsed
    return data


def find_project_main_rs(change_dir: Path) -> Path | None:
    cur = change_dir.resolve()
    while cur.parent != cur:
        candidate = cur / "src" / "main.rs"
        if candidate.is_file():
            return candidate
        cur = cur.parent
    return None


def check_propose(change_dir: Path) -> list[str]:
    errors: list[str] = []
    meta_file = change_dir / ".openspec.yaml"
    meta: dict[str, object] = {}
    if not meta_file.is_file():
        errors.append("Missing .openspec.yaml in change directory")
    else:
        try:
            meta = parse_flat_yaml(meta_file.read_text(encoding="utf-8"))
        except Exception as exc:
            errors.append(f"Invalid YAML in .openspec.yaml: {exc}")
        else:
            schema = meta.get("schema")
            if not isinstance(schema, str) or not schema:
                errors.append(".openspec.yaml is missing required 'schema' key (metadata was overwritten)")

    tasks_file = change_dir / "tasks.md"
    if not tasks_file.is_file():
        errors.append("Missing tasks.md in change directory")
    else:
        tasks_text = tasks_file.read_text(encoding="utf-8")
        bad_calls = DIRECT_BIN_DASH_RE.findall(tasks_text)
        if bad_calls:
            main_rs = find_project_main_rs(change_dir)
            if main_rs is not None:
                main_text = main_rs.read_text(encoding="utf-8")
                if "nth(1)" in main_text and not FLAG_CRATES_RE.search(main_text):
                    errors.append(
                        f"tasks.md contains direct binary invocation passing '--' to direct argv[1] parser: {bad_calls}"
                    )

    schema_val = meta.get("schema")
    skip_specs = meta.get("skip_specs") is True
    specs_dir = change_dir / "specs"
    spec_files = sorted(specs_dir.rglob("spec.md")) if specs_dir.is_dir() else []

    if schema_val == "no-spec":
        if spec_files:
            errors.append("Change uses schema 'no-spec' but created delta spec files under specs/")
    elif schema_val == "spec-driven" and not skip_specs:
        if not spec_files:
            errors.append("Change uses 'spec-driven' without skip_specs: true, but has no specs/<cap>/spec.md")
        else:
            for sf in spec_files:
                body = sf.read_text(encoding="utf-8")
                if not DELTA_HEADER_RE.search(body):
                    errors.append(f"{sf.name} is missing OpenSpec delta header (## ADDED/MODIFIED/REMOVED/RENAMED Requirements)")

    return errors


def check_apply(transcript_file: Path, final_md: Path, tasks_file: Path, config_file: Path | None = None) -> list[str]:
    errors: list[str] = []

    if final_md.is_file():
        final_text = final_md.read_text(encoding="utf-8")
        invented = sorted(set(INVENTED_SUBCMD_RE.findall(final_text)))
        if invented:
            errors.append(
                f"Final report suggests nonexistent CLI subcommands {invented} (use /spectra:<skill> instead)"
            )

    tdd_required = True
    if config_file is not None and config_file.is_file():
        try:
            cfg = parse_flat_yaml(config_file.read_text(encoding="utf-8"))
            if "tdd" in cfg:
                tdd_required = cfg["tdd"] is True
        except Exception:
            pass

    tasks_completed = False
    if tasks_file.is_file():
        tasks_completed = bool(COMPLETED_TASK_RE.search(tasks_file.read_text(encoding="utf-8")))

    if tdd_required and tasks_completed:
        has_behavioral_red = False
        has_green_after_red = False
        if transcript_file.is_file():
            calls_by_id: dict[str, dict[str, object]] = {}
            for line in transcript_file.read_text(encoding="utf-8").splitlines():
                if not line.strip():
                    continue
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue
                msg = event.get("message", {})
                if not isinstance(msg, dict):
                    continue
                content_blocks = msg.get("content", [])
                if not isinstance(content_blocks, list):
                    continue
                for block in content_blocks:
                    if not isinstance(block, dict):
                        continue
                    btype = block.get("type")
                    if btype == "tool_use" and isinstance(block.get("id"), str):
                        calls_by_id[block["id"]] = block
                    elif btype == "tool_result":
                        call = calls_by_id.get(str(block.get("tool_use_id")), {})
                        inp = call.get("input", {})
                        cmd = inp.get("command", "") if isinstance(inp, dict) else ""
                        if not isinstance(cmd, str) or not re.search(r"\b(?:cargo\s+test|pytest|unittest)\b", cmd):
                            continue
                        raw = block.get("content", "")
                        out = raw if isinstance(raw, str) else json.dumps(raw, ensure_ascii=False)
                        if COMPILE_ERROR_RE.search(out) and not BEHAVIORAL_FAIL_RE.search(out):
                            continue
                        if BEHAVIORAL_FAIL_RE.search(out):
                            has_behavioral_red = True
                        elif PASS_RESULT_RE.search(out) and has_behavioral_red:
                            has_green_after_red = True

        if not (has_behavioral_red and has_green_after_red):
            errors.append(
                f"TDD verification failed: did not observe behavioral RED followed by GREEN "
                f"(behavioral_red={has_behavioral_red}, green_after_red={has_green_after_red})"
            )

    return errors


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="OpenSpectra deterministic workflow checker")
    sub = parser.add_subparsers(dest="subcommand", required=True)

    p_prop = sub.add_parser("propose", help="Check propose change directory invariants")
    p_prop.add_argument("change_dir", type=Path, help="Path to change directory")

    p_app = sub.add_parser("apply", help="Check apply execution transcript and handoff invariants")
    p_app.add_argument("--transcript", type=Path, required=True, help="Path to transcript.jsonl")
    p_app.add_argument("--final", type=Path, required=True, help="Path to final report markdown")
    p_app.add_argument("--tasks", type=Path, required=True, help="Path to change tasks.md")
    p_app.add_argument("--config", type=Path, default=None, help="Optional path to .spectra.yaml")

    args = parser.parse_args(argv)
    if args.subcommand == "propose":
        errs = check_propose(args.change_dir.resolve())
    else:
        errs = check_apply(args.transcript, args.final, args.tasks, args.config)

    if errs:
        for err in errs:
            print(f"[FAIL] {err}", file=sys.stderr)
        return 1
    print(f"[OK] workflow-check ({args.subcommand}) passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
