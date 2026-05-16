#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import shutil
import shlex
import subprocess
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path


PROJECT_DIR = Path("/home/ace/Desktop/prjx/kitchensync")
JAVA = Path("/home/ace/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java")
JAR = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.jar")
TMP = Path(tempfile.gettempdir()) / "kitchensync_03_peer_connect"
REMOTE_BASE = "/tmp/testks/03_peer_connect"
REMOTE_RUN = f"{REMOTE_BASE}/run-{time.monotonic_ns()}"


@dataclass
class RunResult:
    returncode: int
    stdout: str
    stderr: str
    seconds: float
    timed_out: bool = False


def run_cli(*args: str, timeout: float = 60.0) -> RunResult:
    started = time.monotonic()
    try:
        completed = subprocess.run(
            [str(JAVA), "-jar", str(JAR), *args],
            cwd=str(PROJECT_DIR),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            check=False,
        )
        return RunResult(
            completed.returncode,
            completed.stdout,
            completed.stderr,
            time.monotonic() - started,
        )
    except subprocess.TimeoutExpired as exc:
        return RunResult(
            124,
            exc.stdout or "",
            exc.stderr or "",
            time.monotonic() - started,
            timed_out=True,
        )


def ssh(*remote_commands: str, timeout: float = 20.0) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            "ssh",
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "ace@ordinarydata.com",
            " && ".join(remote_commands),
        ],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )


def reset_state() -> None:
    if TMP.exists():
        shutil.rmtree(TMP)
    TMP.mkdir(parents=True)
    ssh(f"rm -rf -- {shlex.quote(REMOTE_BASE)}")


def write_source(path: Path, name: str, body: str) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)
    (path / name).write_text(body, encoding="utf-8", newline="\n")


def add_check(failures: list[str], condition: bool, message: str, detail: str = "") -> None:
    if not condition:
        failures.append(f"{message}{chr(10) + detail if detail else ''}")


def result_detail(result: RunResult) -> str:
    return (
        f"exit={result.returncode} timed_out={result.timed_out} "
        f"seconds={result.seconds:.2f}\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}"
    )


def test_file_root_creation(failures: list[str]) -> None:
    source = TMP / "file-source"
    peer_root = TMP / "missing" / "parents" / "file-peer"
    write_source(source, "alpha.txt", "file root creation\n")

    result = run_cli(f"+{source}", peer_root.as_uri())

    add_check(
        failures,
        result.returncode == 0,
        "03.86 file:// missing peer root should connect successfully after creation.",
        result_detail(result),
    )
    add_check(
        failures,
        peer_root.is_dir(),
        "03.86 file:// peer root and missing parents were not created.",
        str(peer_root),
    )
    add_check(
        failures,
        (peer_root / "alpha.txt").is_file(),
        "03.86 file:// peer did not receive synced content after root creation.",
        str(peer_root / "alpha.txt"),
    )


def test_failed_creation_falls_back_to_sftp(failures: list[str]) -> None:
    source = TMP / "fallback-source"
    blocked_parent = TMP / "not-a-directory"
    bad_file_root = blocked_parent / "child"
    remote_root = f"{REMOTE_RUN}/fallback-sftp/missing/peer"
    write_source(source, "bravo.txt", "fallback to sftp\n")
    blocked_parent.write_text("parent path is a file\n", encoding="utf-8", newline="\n")

    result = run_cli(
        f"+{source}",
        f"[{bad_file_root.as_uri()},sftp://ace@ordinarydata.com{remote_root}]",
    )
    probe = ssh(
        f"test -d {shlex.quote(remote_root)}",
        f"test -f {shlex.quote(remote_root + '/bravo.txt')}",
    )

    add_check(
        failures,
        result.returncode == 0,
        "03.87 failed file:// root creation should make KitchenSync try the fallback URL.",
        result_detail(result),
    )
    add_check(
        failures,
        probe.returncode == 0,
        "03.86 sftp:// missing peer root should be created before the fallback URL is accepted.",
        f"ssh exit={probe.returncode}\nstdout:\n{probe.stdout}\nstderr:\n{probe.stderr}",
    )


def test_failed_creation_without_fallback_is_unreachable(failures: list[str]) -> None:
    source = TMP / "unreachable-source"
    blocked_parent = TMP / "blocked-alone"
    bad_file_root = blocked_parent / "child"
    write_source(source, "charlie.txt", "unreachable peer\n")
    blocked_parent.write_text("parent path is a file\n", encoding="utf-8", newline="\n")

    result = run_cli(f"+{source}", bad_file_root.as_uri())

    add_check(
        failures,
        result.returncode != 0,
        "03.87 URL with uncreatable root and no fallback should make the peer unreachable.",
        result_detail(result),
    )
    add_check(
        failures,
        not bad_file_root.exists(),
        "03.87 uncreatable peer root unexpectedly exists.",
        str(bad_file_root),
    )


def test_failed_sftp_creation_falls_back_to_file(failures: list[str]) -> None:
    source = TMP / "sftp-fallback-source"
    remote_blocked_parent = f"{REMOTE_RUN}/blocked-parent"
    remote_bad_root = f"{remote_blocked_parent}/child"
    file_fallback_root = TMP / "sftp-failure-file-fallback"
    write_source(source, "delta.txt", "sftp creation fallback\n")

    setup = ssh(
        f"mkdir -p -- {shlex.quote(str(Path(remote_blocked_parent).parent))}",
        f"printf %s {shlex.quote('parent path is a file')} > {shlex.quote(remote_blocked_parent)}",
    )
    add_check(
        failures,
        setup.returncode == 0,
        "Test setup could not create remote SFTP blocked parent.",
        f"ssh exit={setup.returncode}\nstdout:\n{setup.stdout}\nstderr:\n{setup.stderr}",
    )
    if setup.returncode != 0:
        return

    result = run_cli(
        f"+{source}",
        f"[sftp://ace@ordinarydata.com{remote_bad_root},{file_fallback_root.as_uri()}]",
    )

    add_check(
        failures,
        result.returncode == 0,
        "03.87 failed sftp:// root creation should make KitchenSync try the fallback URL.",
        result_detail(result),
    )
    add_check(
        failures,
        (file_fallback_root / "delta.txt").is_file(),
        "03.87 file:// fallback did not receive synced content after sftp:// creation failure.",
        str(file_fallback_root / "delta.txt"),
    )


def main() -> int:
    failures: list[str] = []
    reset_state()
    try:
        test_file_root_creation(failures)
        test_failed_creation_falls_back_to_sftp(failures)
        test_failed_creation_without_fallback_is_unreachable(failures)
        test_failed_sftp_creation_falls_back_to_file(failures)
        # 03.93 is an implementation-structure requirement: startup peer connection
        # attempts must be issued through a concurrent join/gather/parallel construct.
        # That is not reasonably testable through the public CLI without inspecting
        # source or relying on artificial timing, so this root behavior test does not
        # assert it.
    finally:
        ssh(f"rm -rf -- {shlex.quote(REMOTE_BASE)}")

    if failures:
        print(f"{len(failures)} check(s) failed:")
        for index, failure in enumerate(failures, 1):
            print(f"\n[{index}] {failure}")
        return 1
    print("03_peer-connect checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
