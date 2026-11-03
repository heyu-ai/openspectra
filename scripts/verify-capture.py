#!/usr/bin/env python3
"""Deterministic verification command capture helper (Phase 1).

Executes a single command without a shell (`shell=False`), streams stdout and
stderr to an exclusive directory under `$VERIFY_PROJECT_ROOT/.scratch/captures/`,
computes SHA-256 digests and byte lengths, replays output to host streams, and
writes a structured `result.json` receipt.

See docs/research/skill-evaluation-2026-11.md and docs/adr/0001-openspectra-quality-workflow-upgrade.md.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
import re
import signal
import subprocess
import sys
import time
import uuid

CHUNK_SIZE = 64 * 1024
SLUG = re.compile(r"[a-z0-9][a-z0-9_-]{0,63}\Z")


class CaptureError(Exception):
    pass


class Parser(argparse.ArgumentParser):
    def error(self, message: str):
        raise CaptureError(message)


def timestamp() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="microseconds")


def parse_command(arguments: list[str]):
    parser = Parser(
        prog="verify-capture",
        usage="%(prog)s [--label SAFE_SLUG] [--timeout SECONDS] -- PROGRAM ARGS...",
        description="Capture a command in VERIFY_PROJECT_ROOT/.scratch/captures.",
        allow_abbrev=False,
    )
    parser.add_argument("--label")
    parser.add_argument("--timeout", type=float, default=120.0)
    if arguments in (["-h"], ["--help"]):
        parser.print_help()
        return None
    if "--" not in arguments:
        raise CaptureError("required '--' separator is missing")
    boundary = arguments.index("--")
    options = parser.parse_args(arguments[:boundary])
    command = arguments[boundary + 1 :]
    if not command or not command[0]:
        raise CaptureError("PROGRAM is required after '--'")
    if options.label is not None and not SLUG.fullmatch(options.label):
        raise CaptureError("label must match [a-z0-9][a-z0-9_-]{0,63}")
    if not math.isfinite(options.timeout) or not 0 < options.timeout <= 600:
        raise CaptureError("timeout must be a finite number greater than 0 and at most 600 seconds")
    return options, command


def directory_flags() -> int:
    if not hasattr(os, "O_DIRECTORY") or not hasattr(os, "O_NOFOLLOW"):
        raise CaptureError("this helper requires POSIX O_DIRECTORY and O_NOFOLLOW")
    if os.open not in os.supports_dir_fd or os.mkdir not in os.supports_dir_fd:
        raise CaptureError("this helper requires open/mkdir dir_fd support")
    return os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW


def open_directory(parent: int, name: str, flags: int) -> int:
    try:
        os.mkdir(name, mode=0o700, dir_fd=parent)
    except FileExistsError:
        pass
    return os.open(name, flags, dir_fd=parent)


def create_storage(capture_id: str) -> tuple[int, str]:
    flags = directory_flags()
    configured_root = os.environ.get("VERIFY_PROJECT_ROOT")
    if not configured_root:
        raise CaptureError("VERIFY_PROJECT_ROOT is required")
    root = os.path.realpath(configured_root)
    cwd = os.path.realpath(os.getcwd())
    if os.path.commonpath((root, cwd)) != root:
        raise CaptureError("current working directory must be inside VERIFY_PROJECT_ROOT")
    root_fd = os.open(root, flags)
    try:
        scratch_fd = open_directory(root_fd, ".scratch", flags)
        try:
            captures_fd = open_directory(scratch_fd, "captures", flags)
            try:
                os.mkdir(capture_id, mode=0o700, dir_fd=captures_fd)
                capture_fd = os.open(capture_id, flags, dir_fd=captures_fd)
            finally:
                os.close(captures_fd)
        finally:
            os.close(scratch_fd)
    finally:
        os.close(root_fd)
    return capture_fd, cwd


def create_file(directory: int, name: str) -> int:
    return os.open(
        name,
        os.O_RDWR | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
        0o600,
        dir_fd=directory,
    )


def write_all(fd: int, data: bytes) -> None:
    view = memoryview(data)
    while view:
        count = os.write(fd, view)
        if count == 0:
            raise OSError("zero-length write")
        view = view[count:]


def stream_metadata(fd: int, relative_path: str) -> dict[str, object]:
    digest = hashlib.sha256()
    length = os.fstat(fd).st_size
    remaining = length
    while remaining:
        chunk = os.pread(fd, min(CHUNK_SIZE, remaining), length - remaining)
        if not chunk:
            raise OSError("capture was truncated while hashing")
        remaining -= len(chunk)
        digest.update(chunk)
    return {"path": relative_path, "bytes": length, "sha256": digest.hexdigest()}


def replay(fd: int, target_fd: int, length: int) -> None:
    remaining = length
    while remaining:
        chunk = os.pread(fd, min(CHUNK_SIZE, remaining), length - remaining)
        if not chunk:
            raise OSError("capture was truncated during replay")
        write_all(target_fd, chunk)
        remaining -= len(chunk)


def stop_group(process: subprocess.Popen) -> None:
    if process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    process.wait()


def announce(message: str) -> None:
    try:
        write_all(2, (message + "\n").encode("utf-8", errors="backslashreplace"))
    except OSError:
        pass


def main(arguments: list[str] | None = None) -> int:
    arguments = list(sys.argv[1:] if arguments is None else arguments)
    started_at = timestamp()
    started_clock = time.monotonic()
    capture_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%fZ-") + uuid.uuid4().hex
    relative_dir = ".scratch/captures/" + capture_id
    record: dict[str, object] = {
        "version": 1,
        "capture_id": capture_id,
        "label": None,
        "argv": arguments[arguments.index("--") + 1 :] if "--" in arguments else [],
        "cwd": None,
        "started_at": started_at,
        "finished_at": None,
        "duration_seconds": None,
        "outcome": "runner_error",
        "exit_code": None,
        "stdout": None,
        "stderr": None,
    }
    if "VERIFY_INPUT_FINGERPRINT" in os.environ:
        record["input_fingerprint"] = os.environ["VERIFY_INPUT_FINGERPRINT"]
    directory = result_fd = stdout_fd = stderr_fd = None
    process = None
    receipt_finalized = False
    helper_exit = 2
    errors: list[str] = []
    try:
        parse_error = None
        try:
            parsed = parse_command(arguments)
        except CaptureError as exc:
            parse_error = exc
            parsed = None
        if parsed is None and parse_error is None:
            return 0
        directory, record["cwd"] = create_storage(capture_id)
        result_fd = create_file(directory, "result.json")
        stdout_fd = create_file(directory, "stdout.txt")
        stderr_fd = create_file(directory, "stderr.txt")
        if parse_error is not None:
            raise parse_error
        options, command = parsed
        record["label"] = options.label
        process = subprocess.Popen(
            command,
            stdin=None,
            stdout=stdout_fd,
            stderr=stderr_fd,
            cwd=str(record["cwd"]),
            shell=False,
            start_new_session=True,
            close_fds=True,
        )
        try:
            process.wait(timeout=options.timeout)
        except subprocess.TimeoutExpired:
            stop_group(process)
            record["outcome"] = "timeout"
            helper_exit = 124
        else:
            record["outcome"] = "completed"
            helper_exit = process.returncode
            if helper_exit < 0:
                helper_exit = 128 - helper_exit
        record["exit_code"] = process.returncode
    except (CaptureError, OSError, ValueError, KeyboardInterrupt) as exc:
        errors.append(str(exc) or type(exc).__name__)
        record["outcome"] = "runner_error"
        helper_exit = 2
        if process is not None:
            try:
                stop_group(process)
                record["exit_code"] = process.returncode
            except (OSError, KeyboardInterrupt) as stop_error:
                errors.append("child cleanup: " + (str(stop_error) or type(stop_error).__name__))
    finally:
        for name, fd, target in (("stdout", stdout_fd, 1), ("stderr", stderr_fd, 2)):
            if fd is None:
                continue
            try:
                meta = stream_metadata(fd, relative_dir + "/" + name + ".txt")
                record[name] = meta
                replay(fd, target, int(meta["bytes"]))
            except (OSError, KeyboardInterrupt) as exc:
                errors.append(name + " capture/replay: " + (str(exc) or type(exc).__name__))
                record["outcome"] = "runner_error"
                helper_exit = 2
        record["finished_at"] = timestamp()
        record["duration_seconds"] = time.monotonic() - started_clock
        if errors:
            record["error"] = "; ".join(errors)
        if result_fd is not None:
            try:
                payload = (json.dumps(record, ensure_ascii=True, indent=2) + "\n").encode("utf-8")
                write_all(result_fd, payload)
                os.fsync(result_fd)
                receipt_finalized = True
            except (OSError, KeyboardInterrupt) as exc:
                helper_exit = 2
                record["outcome"] = "runner_error"
                errors.append("cannot finalize receipt: " + (str(exc) or type(exc).__name__))
        for fd in (stdout_fd, stderr_fd, result_fd, directory):
            if fd is not None:
                try:
                    os.close(fd)
                except OSError:
                    pass
    if errors:
        announce("verify-capture: " + "; ".join(errors))
    if result_fd is not None:
        suffix = "" if receipt_finalized else " (incomplete; finalization failed)"
        announce("verify-capture: receipt " + relative_dir + "/result.json" + suffix)
    announce(
        "verify-capture: outcome="
        + str(record["outcome"])
        + " command_exit="
        + str(record["exit_code"])
        + " helper_exit="
        + str(helper_exit)
    )
    return helper_exit


def interrupted(signum: int, _frame) -> None:
    raise KeyboardInterrupt(f"signal {signum}")


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, interrupted)
    sys.exit(main())
