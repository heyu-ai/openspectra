#!/usr/bin/env python3
"""Unit and integration tests for Phase 1 deterministic workflow tools.

Covers:
1. scripts/verify-capture.py:
   - Captures stdout/stderr, preserves exit code, writes SHA-256 receipt under .scratch/captures/
   - Rejects missing VERIFY_PROJECT_ROOT, cwd outside root, traversal labels, and symlinked .scratch/captures
   - Enforces timeout with process-group termination (exit 124, outcome=timeout)
   - Handles SIGTERM with process-group cleanup and runner_error receipt
2. scripts/workflow-check.py (propose & apply):
   - propose: rejects missing .openspec.yaml or overwritten metadata missing 'schema'
   - propose: enforces no-spec (no delta specs allowed) vs spec-driven (requires delta spec with OpenSpec headers)
   - propose: detects invalid direct binary '--' separator when src/main.rs parses args().nth(1) without a flag library
   - apply: rejects invented CLI subcommands (spectra verify / review / apply / audit) while allowing /spectra:verify
   - apply: requires behavioral RED (test assertion failure, not compile error) followed by GREEN when tdd: true and tasks are marked done
   - apply: allows pending tasks on premise conflict without requiring RED-GREEN
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import unittest

SCRIPTS_DIR = Path(__file__).resolve().parent
VERIFY_CAPTURE = SCRIPTS_DIR / "verify-capture.py"
WORKFLOW_CHECK = SCRIPTS_DIR / "workflow-check.py"


class TestVerifyCapture(unittest.TestCase):
    def test_captures_streams_and_preserves_nonzero_exit(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td).resolve()
            env = dict(os.environ, VERIFY_PROJECT_ROOT=str(root), VERIFY_INPUT_FINGERPRINT="fp-123")
            cmd = [
                sys.executable,
                str(VERIFY_CAPTURE),
                "--label",
                "unit-check_1",
                "--",
                sys.executable,
                "-c",
                "import sys; sys.stdout.write('hello-out\\n'); sys.stderr.write('hello-err\\n'); sys.exit(7)",
            ]
            proc = subprocess.run(cmd, cwd=root, env=env, capture_output=True, text=True, timeout=30)
            self.assertEqual(proc.returncode, 7)
            self.assertEqual(proc.stdout, "hello-out\n")
            self.assertIn("hello-err\n", proc.stderr)
            self.assertIn("verify-capture: receipt .scratch/captures/", proc.stderr)

            receipts = list((root / ".scratch" / "captures").glob("*/result.json"))
            self.assertEqual(len(receipts), 1)
            data = json.loads(receipts[0].read_text(encoding="utf-8"))
            self.assertEqual(data["version"], 1)
            self.assertEqual(data["label"], "unit-check_1")
            self.assertEqual(data["outcome"], "completed")
            self.assertEqual(data["exit_code"], 7)
            self.assertEqual(data["input_fingerprint"], "fp-123")

            out_bytes = (root / data["stdout"]["path"]).read_bytes()
            err_bytes = (root / data["stderr"]["path"]).read_bytes()
            self.assertEqual(out_bytes, b"hello-out\n")
            self.assertEqual(err_bytes, b"hello-err\n")
            self.assertEqual(data["stdout"]["bytes"], 10)
            self.assertEqual(
                data["stdout"]["sha256"],
                "f1d5a778c0023c083bab42c96601d777354841c6299cbfa064532d804a70f323",
            )
            self.assertEqual(data["stderr"]["bytes"], 10)
            self.assertEqual(
                data["stderr"]["sha256"],
                "52c3a85dee445aba5b0c0b190a2313693bb7e2b8bfd924049a26b1939307f6f3",
            )

    def test_rejects_missing_root_outside_cwd_bad_label_and_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            base = Path(td).resolve()
            root = base / "proj"
            outside = base / "outside"
            root.mkdir()
            outside.mkdir()

            # 1. Missing VERIFY_PROJECT_ROOT
            env_no_root = dict(os.environ)
            env_no_root.pop("VERIFY_PROJECT_ROOT", None)
            p1 = subprocess.run(
                [sys.executable, str(VERIFY_CAPTURE), "--", sys.executable, "-c", "print(1)"],
                cwd=root,
                env=env_no_root,
                capture_output=True,
                text=True,
            )
            self.assertEqual(p1.returncode, 2)
            self.assertIn("VERIFY_PROJECT_ROOT is required", p1.stderr)

            # 2. CWD outside VERIFY_PROJECT_ROOT
            env = dict(os.environ, VERIFY_PROJECT_ROOT=str(root))
            p2 = subprocess.run(
                [sys.executable, str(VERIFY_CAPTURE), "--", sys.executable, "-c", "print(1)"],
                cwd=outside,
                env=env,
                capture_output=True,
                text=True,
            )
            self.assertEqual(p2.returncode, 2)
            self.assertIn("inside VERIFY_PROJECT_ROOT", p2.stderr)

            # 3. Traversal label rejected
            p3 = subprocess.run(
                [sys.executable, str(VERIFY_CAPTURE), "--label", "../bad", "--", sys.executable, "-c", "print(1)"],
                cwd=root,
                env=env,
                capture_output=True,
                text=True,
            )
            self.assertEqual(p3.returncode, 2)
            self.assertNotIn("1", p3.stdout)

            # 4. Symlinked .scratch/captures rejected without touching target
            sh_dir = root / ".scratch"
            shutil.rmtree(sh_dir, ignore_errors=True)
            sh_dir.mkdir()
            (sh_dir / "captures").symlink_to(outside, target_is_directory=True)
            p4 = subprocess.run(
                [sys.executable, str(VERIFY_CAPTURE), "--", sys.executable, "-c", "print('escaped')"],
                cwd=root,
                env=env,
                capture_output=True,
                text=True,
            )
            self.assertEqual(p4.returncode, 2)
            self.assertEqual(list(outside.iterdir()), [])

    def test_timeout_and_sigterm_terminate_child_process_group(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td).resolve()
            env = dict(os.environ, VERIFY_PROJECT_ROOT=str(root))

            # Timeout returns 124, terminates child process group, and records outcome=timeout
            timeout_pid_file = root / "timeout_child.pid"
            timeout_child_code = (
                f"import os, time; open({repr(str(timeout_pid_file))}, 'w').write(str(os.getpid())); time.sleep(60)"
            )
            p_timeout = subprocess.run(
                [
                    sys.executable,
                    str(VERIFY_CAPTURE),
                    "--label",
                    "slow-cmd",
                    "--timeout",
                    "0.3",
                    "--",
                    sys.executable,
                    "-c",
                    timeout_child_code,
                ],
                cwd=root,
                env=env,
                capture_output=True,
                text=True,
                timeout=20,
            )
            self.assertEqual(p_timeout.returncode, 124)
            timeout_child_pid = int(timeout_pid_file.read_text(encoding="utf-8"))
            with self.assertRaises(ProcessLookupError):
                os.kill(timeout_child_pid, 0)
            receipts = [json.loads(p.read_text()) for p in (root / ".scratch" / "captures").glob("*/result.json")]
            self.assertTrue(any(r["label"] == "slow-cmd" and r["outcome"] == "timeout" for r in receipts))

            # SIGTERM cleans up child process group
            (root / ".scratch").mkdir(exist_ok=True)
            sock_path = root / ".scratch" / "ready.sock"
            srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            srv.bind(str(sock_path))
            srv.listen(1)
            srv.settimeout(10)
            child_code = (
                "import os, socket, time; s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); "
                f"s.connect({repr(str(sock_path))}); s.sendall(str(os.getpid()).encode()); s.close(); time.sleep(60)"
            )
            proc = subprocess.Popen(
                [sys.executable, str(VERIFY_CAPTURE), "--label", "sigterm-case", "--", sys.executable, "-c", child_code],
                cwd=root,
                env=env,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            conn, _ = srv.accept()
            child_pid = int(conn.recv(64))
            conn.close()
            srv.close()

            proc.send_signal(signal.SIGTERM)
            proc.communicate(timeout=10)
            self.assertEqual(proc.returncode, 2)
            with self.assertRaises(ProcessLookupError):
                os.kill(child_pid, 0)


class TestWorkflowCheck(unittest.TestCase):
    def test_propose_check_metadata_delta_and_cli_args(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td).resolve()
            (root / "src").mkdir()
            (root / "src" / "main.rs").write_text(
                'fn main() { let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap(); println!("{n}"); }\n',
                encoding="utf-8",
            )
            ch = root / "openspec" / "changes" / "my-change"
            ch.mkdir(parents=True)

            # 1. Missing .openspec.yaml fails
            r1 = subprocess.run(
                [sys.executable, str(WORKFLOW_CHECK), "propose", str(ch)],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r1.returncode, 1)
            self.assertIn(".openspec.yaml", r1.stderr)

            # 2. Overwritten .openspec.yaml missing 'schema' fails
            (ch / ".openspec.yaml").write_text("skip_specs: true\n", encoding="utf-8")
            (ch / "proposal.md").write_text("## Why\nTest\n\n## What Changes\nTest\n", encoding="utf-8")
            (ch / "tasks.md").write_text("- [ ] 1.1 Update docs and verify with cargo test\n", encoding="utf-8")
            r2 = subprocess.run(
                [sys.executable, str(WORKFLOW_CHECK), "propose", str(ch)],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r2.returncode, 1)
            self.assertIn("schema", r2.stderr)

            # 3. Valid no-spec passes (including quoted boolean literal)
            (ch / ".openspec.yaml").write_text(
                'schema: no-spec\ncreated: 2026-11-03\ncreated_by: test\nskip_specs: "true"\n',
                encoding="utf-8",
            )
            r3 = subprocess.run(
                [sys.executable, str(WORKFLOW_CHECK), "propose", str(ch)],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r3.returncode, 0, r3.stderr)

            # 4. no-spec with delta spec files fails
            spec_file = ch / "specs" / "cap" / "spec.md"
            spec_file.parent.mkdir(parents=True)
            spec_file.write_text("## ADDED Requirements\n", encoding="utf-8")
            r3_contradict = subprocess.run(
                [sys.executable, str(WORKFLOW_CHECK), "propose", str(ch)],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r3_contradict.returncode, 1)
            self.assertIn("no-spec", r3_contradict.stderr)
            shutil.rmtree(ch / "specs")

            # 5. spec-driven without skip_specs and without delta spec files fails
            (ch / ".openspec.yaml").write_text("schema: spec-driven\ncreated: 2026-11-03\n", encoding="utf-8")
            r4_missing_specs = subprocess.run(
                [sys.executable, str(WORKFLOW_CHECK), "propose", str(ch)],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r4_missing_specs.returncode, 1)
            self.assertIn("no specs/<cap>/spec.md", r4_missing_specs.stderr)

            # 6. spec-driven with spec.md missing OpenSpec delta header fails with relative path
            spec_file.parent.mkdir(parents=True)
            spec_file.write_text("# Capability\n\n### Requirement: R\nThe system SHALL work.\n", encoding="utf-8")
            r4_bad_header = subprocess.run(
                [sys.executable, str(WORKFLOW_CHECK), "propose", str(ch)],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r4_bad_header.returncode, 1)
            self.assertIn("specs/cap/spec.md is missing OpenSpec delta header", r4_bad_header.stderr)

            # 7. Direct binary invocation with '--' on nth(1) parser fails and reports full invocation
            spec_file.write_text(
                "## ADDED Requirements\n\n### Requirement: R\nThe system SHALL work.\n\n#### Scenario: S\n- **WHEN** x\n- **THEN** y\n",
                encoding="utf-8",
            )
            (ch / "tasks.md").write_text("- [ ] 1.1 Verify CLI with ./target/debug/app -- -1\n", encoding="utf-8")
            r4 = subprocess.run(
                [sys.executable, str(WORKFLOW_CHECK), "propose", str(ch)],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r4.returncode, 1)
            self.assertIn("direct binary invocation", r4.stderr)
            self.assertIn("./target/debug/app -- -1", r4.stderr)

            # 8. Fixing command to cargo run -- -1 passes
            (ch / "tasks.md").write_text("- [ ] 1.1 Verify CLI with cargo run -- -1\n", encoding="utf-8")
            r5 = subprocess.run(
                [sys.executable, str(WORKFLOW_CHECK), "propose", str(ch)],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r5.returncode, 0, r5.stderr)

    def test_apply_check_tdd_red_green_and_handoff(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            d = Path(td).resolve()
            transcript = d / "transcript.jsonl"
            final_md = d / "final.md"
            tasks_md = d / "tasks.md"
            bad_cfg = d / "bad.spectra.yaml"

            # 0. Missing required input files fail instead of silently passing
            r_missing = subprocess.run(
                [
                    sys.executable,
                    str(WORKFLOW_CHECK),
                    "apply",
                    "--transcript",
                    str(transcript),
                    "--final",
                    str(final_md),
                    "--tasks",
                    str(tasks_md),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r_missing.returncode, 1)
            self.assertIn("Missing final report file", r_missing.stderr)
            self.assertIn("Missing tasks file", r_missing.stderr)

            # 1. Invented CLI command 'spectra verify' fails
            final_md.write_text("Done! Run `spectra verify feat` next.\n", encoding="utf-8")
            tasks_md.write_text("- [ ] 1.1 Pending due to conflict\n", encoding="utf-8")
            transcript.write_text("", encoding="utf-8")
            r1 = subprocess.run(
                [
                    sys.executable,
                    str(WORKFLOW_CHECK),
                    "apply",
                    "--transcript",
                    str(transcript),
                    "--final",
                    str(final_md),
                    "--tasks",
                    str(tasks_md),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r1.returncode, 1)
            self.assertIn("nonexistent CLI subcommands", r1.stderr)

            # 2. Pending tasks (- [ ]) with valid handoff pass without requiring TDD RED-GREEN
            final_md.write_text("Blocked on premise conflict; run `/spectra:verify feat` after resolution.\n", encoding="utf-8")
            r_pending = subprocess.run(
                [
                    sys.executable,
                    str(WORKFLOW_CHECK),
                    "apply",
                    "--transcript",
                    str(transcript),
                    "--final",
                    str(final_md),
                    "--tasks",
                    str(tasks_md),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r_pending.returncode, 0, r_pending.stderr)

            # 3. Malformed --config YAML is reported as an error
            bad_cfg.write_text("not_a_key_value_line\n", encoding="utf-8")
            r_bad_cfg = subprocess.run(
                [
                    sys.executable,
                    str(WORKFLOW_CHECK),
                    "apply",
                    "--transcript",
                    str(transcript),
                    "--final",
                    str(final_md),
                    "--tasks",
                    str(tasks_md),
                    "--config",
                    str(bad_cfg),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r_bad_cfg.returncode, 1)
            self.assertIn("Invalid YAML", r_bad_cfg.stderr)

            # 4. Compile error followed by GREEN does NOT count as behavioral RED
            tasks_md.write_text("- [x] 1.1 Implemented feature\n", encoding="utf-8")
            events_compile_only = [
                {"message": {"content": [{"type": "tool_use", "id": "u1", "input": {"command": "cargo test"}}]}},
                {
                    "message": {
                        "content": [
                            {
                                "type": "tool_result",
                                "tool_use_id": "u1",
                                "content": "error[E0425]: cannot find value `x` in this scope\nerror: could not compile `app`",
                            }
                        ]
                    }
                },
                {"message": {"content": [{"type": "tool_use", "id": "u2", "input": {"command": "cargo test"}}]}},
                {
                    "message": {
                        "content": [
                            {"type": "tool_result", "tool_use_id": "u2", "content": "test result: ok. 3 passed; 0 failed"}
                        ]
                    }
                },
            ]
            transcript.write_text("\n".join(json.dumps(e) for e in events_compile_only) + "\n", encoding="utf-8")
            r2 = subprocess.run(
                [
                    sys.executable,
                    str(WORKFLOW_CHECK),
                    "apply",
                    "--transcript",
                    str(transcript),
                    "--final",
                    str(final_md),
                    "--tasks",
                    str(tasks_md),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r2.returncode, 1)
            self.assertIn("TDD verification failed", r2.stderr)

            # 5. Genuine behavioral RED followed by GREEN (including structured list content block with multiline ^OK) passes
            events_red_green = [
                {"message": {"content": [{"type": "tool_use", "id": "u1", "input": {"command": "python3 -m unittest"}}]}},
                {
                    "message": {
                        "content": [
                            {
                                "type": "tool_result",
                                "tool_use_id": "u1",
                                "content": "AssertionError: 1 != 2\nFAILED (failures=1)",
                            }
                        ]
                    }
                },
                {"message": {"content": [{"type": "tool_use", "id": "u2", "input": {"command": "python3 -m unittest"}}]}},
                {
                    "message": {
                        "content": [
                            {
                                "type": "tool_result",
                                "tool_use_id": "u2",
                                "content": [{"type": "text", "text": "Ran 2 tests in 0.001s\n\nOK"}],
                            }
                        ]
                    }
                },
            ]
            transcript.write_text("\n".join(json.dumps(e) for e in events_red_green) + "\n", encoding="utf-8")
            r3 = subprocess.run(
                [
                    sys.executable,
                    str(WORKFLOW_CHECK),
                    "apply",
                    "--transcript",
                    str(transcript),
                    "--final",
                    str(final_md),
                    "--tasks",
                    str(tasks_md),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r3.returncode, 0, r3.stderr)

            # 6. RED -> GREEN -> RED regression fails (has_green_after_red is reset on subsequent failure)
            events_red_green_red = events_red_green + [
                {"message": {"content": [{"type": "tool_use", "id": "u3", "input": {"command": "cargo test"}}]}},
                {
                    "message": {
                        "content": [
                            {
                                "type": "tool_result",
                                "tool_use_id": "u3",
                                "content": "test result: FAILED. 1 passed; 1 failed",
                            }
                        ]
                    }
                },
            ]
            transcript.write_text("\n".join(json.dumps(e) for e in events_red_green_red) + "\n", encoding="utf-8")
            r4 = subprocess.run(
                [
                    sys.executable,
                    str(WORKFLOW_CHECK),
                    "apply",
                    "--transcript",
                    str(transcript),
                    "--final",
                    str(final_md),
                    "--tasks",
                    str(tasks_md),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r4.returncode, 1)
            self.assertIn("green_after_red=False", r4.stderr)

            # 7. RED -> GREEN -> compile error also resets has_green_after_red
            events_red_green_compile = events_red_green + [
                {"message": {"content": [{"type": "tool_use", "id": "u4", "input": {"command": "cargo test"}}]}},
                {
                    "message": {
                        "content": [
                            {
                                "type": "tool_result",
                                "tool_use_id": "u4",
                                "content": "error[E0425]: cannot find value `y`\nerror: could not compile `app`",
                            }
                        ]
                    }
                },
            ]
            transcript.write_text("\n".join(json.dumps(e) for e in events_red_green_compile) + "\n", encoding="utf-8")
            r5 = subprocess.run(
                [
                    sys.executable,
                    str(WORKFLOW_CHECK),
                    "apply",
                    "--transcript",
                    str(transcript),
                    "--final",
                    str(final_md),
                    "--tasks",
                    str(tasks_md),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r5.returncode, 1)
            self.assertIn("green_after_red=False", r5.stderr)

            # 8. Missing transcript when TDD is active and tasks are completed fails
            transcript.unlink()
            r6 = subprocess.run(
                [
                    sys.executable,
                    str(WORKFLOW_CHECK),
                    "apply",
                    "--transcript",
                    str(transcript),
                    "--final",
                    str(final_md),
                    "--tasks",
                    str(tasks_md),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r6.returncode, 1)
            self.assertIn("Missing transcript file", r6.stderr)


if __name__ == "__main__":
    unittest.main()
