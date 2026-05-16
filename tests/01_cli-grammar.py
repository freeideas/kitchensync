#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import re
import shlex
import shutil
import socket
import sqlite3
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path


PROJECT_DIR = Path(__file__).resolve().parents[1]
JAVA = PROJECT_DIR / "tools" / "compiler" / "jdk" / "bin" / "java"
JAR = PROJECT_DIR / "released" / "kitchensync.jar"
WORK_DIR = PROJECT_DIR / "tests" / ".tmp" / "01_cli_grammar"

REMOTE_USER = "ace"
REMOTE_HOST = "ordinarydata.com"
REMOTE_BASE = f"/tmp/testks/01_cli_grammar_{os.getpid()}"


@dataclass(frozen=True)
class CliResult:
    returncode: int
    stdout: str
    stderr: str
    elapsed: float


class StalledSshEndpoint:
    def __init__(self) -> None:
        self.accepted = 0
        self._stop = threading.Event()
        self._ready = threading.Event()
        self._sock: socket.socket | None = None
        self._thread = threading.Thread(target=self._serve, daemon=True)

    def __enter__(self) -> StalledSshEndpoint:
        self._thread.start()
        if not self._ready.wait(timeout=5):
            raise RuntimeError("stalled SSH endpoint did not start")
        return self

    def __exit__(self, _exc_type, _exc, _tb) -> None:
        self._stop.set()
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
        self._thread.join(timeout=5)

    @property
    def port(self) -> int:
        if self._sock is None:
            raise RuntimeError("stalled SSH endpoint has no socket")
        return int(self._sock.getsockname()[1])

    def _serve(self) -> None:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as server:
            server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            server.bind(("127.0.0.1", 0))
            server.listen()
            server.settimeout(0.2)
            self._sock = server
            self._ready.set()
            clients: list[socket.socket] = []
            try:
                while not self._stop.is_set():
                    try:
                        client, _addr = server.accept()
                    except socket.timeout:
                        continue
                    except OSError:
                        break
                    self.accepted += 1
                    clients.append(client)
            finally:
                for client in clients:
                    try:
                        client.close()
                    except OSError:
                        pass


def run_cli(*args: str, timeout: int = 90) -> CliResult:
    start = time.monotonic()
    result = subprocess.run(
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
    return CliResult(
        returncode=result.returncode,
        stdout=result.stdout,
        stderr=result.stderr,
        elapsed=time.monotonic() - start,
    )


def run_ssh(command: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            "ssh",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=accept-new",
            f"{REMOTE_USER}@{REMOTE_HOST}",
            command,
        ],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=30,
        check=False,
    )


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def payload_tree(root: Path) -> dict[str, str]:
    files: dict[str, str] = {}
    for path in sorted(root.rglob("*")):
        if path.is_file() and ".kitchensync" not in path.parts:
            files[path.relative_to(root).as_posix()] = path.read_text(encoding="utf-8")
    return files


def progress_lines(stdout: str, marker: str) -> list[str]:
    return [line for line in stdout.splitlines() if line.startswith(f"{marker} ")]


def describe_result(result: CliResult) -> str:
    return (
        f"exit={result.returncode} elapsed={result.elapsed:.2f}s\n"
        f"stdout:\n{result.stdout[-2000:]}\n"
        f"stderr:\n{result.stderr[-2000:]}"
    )


def check_success(failures: list[str], req_id: str, label: str, result: CliResult) -> None:
    if result.returncode != 0:
        failures.append(f"{req_id}: {label} should exit 0; {describe_result(result)}")


def timestamp(days_from_now: int) -> str:
    value = datetime.now(timezone.utc) + timedelta(days=days_from_now)
    return value.strftime("%Y-%m-%d_%H-%M-%S_") + f"{value.microsecond:06d}Z"


def make_peer_pair(root: Path) -> tuple[Path, Path]:
    source = root / "source"
    dest = root / "dest"
    write_text(source / "alpha.txt", "alpha\n")
    write_text(source / "nested" / "beta.txt", "beta\n")
    dest.mkdir(parents=True, exist_ok=True)
    return source, dest


def make_retention_dirs(level: Path, *, old_xd: str, fresh_xd: str, old_bd: str, fresh_bd: str) -> dict[str, Path]:
    paths = {
        "old_tmp": level / ".kitchensync" / "TMP" / old_xd,
        "fresh_tmp": level / ".kitchensync" / "TMP" / fresh_xd,
        "old_bak": level / ".kitchensync" / "BAK" / old_bd,
        "fresh_bak": level / ".kitchensync" / "BAK" / fresh_bd,
    }
    write_text(paths["old_tmp"] / "uuid-old" / "gone.txt", "old tmp\n")
    write_text(paths["fresh_tmp"] / "uuid-fresh" / "kept.txt", "fresh tmp\n")
    write_text(paths["old_bak"] / "gone.txt", "old bak\n")
    write_text(paths["fresh_bak"] / "kept.txt", "fresh bak\n")
    return paths


def insert_tombstone_fixture(peer: Path, old_time: str, fresh_time: str) -> tuple[str, str]:
    old_id = "default_td_old_tombstone"
    fresh_id = "default_td_fresh_tombstone"
    database = peer / ".kitchensync" / "snapshot.db"
    with sqlite3.connect(database) as connection:
        connection.row_factory = sqlite3.Row
        template = connection.execute("SELECT * FROM snapshot LIMIT 1").fetchone()
        if template is None:
            raise RuntimeError("snapshot fixture did not contain a template row")
        columns = list(template.keys())
        placeholders = ",".join("?" for _ in columns)
        sql = f"INSERT OR REPLACE INTO snapshot ({','.join(columns)}) VALUES ({placeholders})"
        for row_id, basename, row_time in (
            (old_id, "__default_td_old__.txt", old_time),
            (fresh_id, "__default_td_fresh__.txt", fresh_time),
        ):
            values = dict(template)
            values.update(
                {
                    "id": row_id,
                    "basename": basename,
                    "mod_time": row_time,
                    "byte_size": 1,
                    "last_seen": row_time,
                    "deleted_time": row_time,
                }
            )
            connection.execute(sql, [values[column] for column in columns])
        connection.commit()
    return old_id, fresh_id


def snapshot_ids(peer: Path) -> set[str]:
    database = peer / ".kitchensync" / "snapshot.db"
    with sqlite3.connect(database) as connection:
        return {str(row[0]) for row in connection.execute("SELECT id FROM snapshot").fetchall()}


def check_01_24_default_mc(failures: list[str]) -> None:
    source = WORK_DIR / "default_mc" / "source"
    write_text(source / "remote.txt", "default mc\n")
    remote = f"sftp://{REMOTE_USER}@{REMOTE_HOST}{REMOTE_BASE}/default-mc"

    result = run_cli("-vl", "trace", f"+{source}", f"-{remote}", timeout=120)
    check_success(failures, "01.24", "omitted --mc SFTP trace sync", result)
    lines = [line for line in result.stdout.splitlines() if "endpoint=" in line and "connections=" in line]
    if not any(re.search(r"endpoint=ace@ordinarydata\.com:22 connections=\d+/10\b", line) for line in lines):
        failures.append(f"01.24: omitted --mc should create an SFTP pool with max 10; pool lines={lines!r}")


def check_01_29_default_ct(failures: list[str]) -> None:
    source, fallback_dest = make_peer_pair(WORK_DIR / "default_ct")
    with StalledSshEndpoint() as stalled:
        peer = f"[sftp://{REMOTE_USER}@127.0.0.1:{stalled.port}/tmp/testks/never,{fallback_dest.resolve().as_uri()}]"
        result = run_cli(f"+{source}", peer, timeout=50)
        check_success(failures, "01.29", "omitted --ct stalled SFTP fallback sync", result)
        if stalled.accepted < 1:
            failures.append("01.29: stalled SFTP endpoint was not contacted")
        if result.elapsed < 25:
            failures.append(f"01.29: omitted --ct should wait close to the 30 second default; elapsed={result.elapsed:.2f}s")
        if result.elapsed > 45:
            failures.append(f"01.29: omitted --ct exceeded the expected 30 second default window; elapsed={result.elapsed:.2f}s")
    expected = {"alpha.txt": "alpha\n", "nested/beta.txt": "beta\n"}
    if payload_tree(fallback_dest) != expected:
        failures.append(f"01.29: fallback file peer did not receive payload after default ct timeout: {payload_tree(fallback_dest)!r}")


def check_01_31_default_verbosity(failures: list[str]) -> None:
    source, omitted_dest = make_peer_pair(WORK_DIR / "default_verbosity" / "omitted")
    info_dest = WORK_DIR / "default_verbosity" / "explicit-info" / "dest"
    info_dest.mkdir(parents=True, exist_ok=True)

    omitted = run_cli(f"+{source}", f"-{omitted_dest}")
    explicit = run_cli("-vl", "info", f"+{source}", f"-{info_dest}")
    check_success(failures, "01.31", "omitted -vl sync", omitted)
    check_success(failures, "01.31", "explicit -vl info sync", explicit)

    expected_progress = ["C alpha.txt", "C nested/beta.txt"]
    omitted_progress = sorted(progress_lines(omitted.stdout, "C"))
    explicit_progress = sorted(progress_lines(explicit.stdout, "C"))
    if omitted_progress != expected_progress:
        failures.append(f"01.31: omitted -vl should emit info copy lines; got {omitted_progress!r}")
    if explicit_progress != expected_progress:
        failures.append(f"01.31: explicit -vl info fixture should emit info copy lines; got {explicit_progress!r}")


def check_01_32_33_34_retention_defaults(failures: list[str]) -> None:
    root = WORK_DIR / "retention_defaults"
    peer_a = root / "peer-a"
    peer_b = root / "peer-b"
    write_text(peer_a / "root.txt", "root\n")
    write_text(peer_a / "nested" / "child.txt", "child\n")
    peer_b.mkdir(parents=True, exist_ok=True)

    setup = run_cli(f"+{peer_a}", f"-{peer_b}")
    check_success(failures, "01.32/01.33/01.34", "retention fixture setup", setup)
    if not (peer_a / ".kitchensync" / "snapshot.db").is_file():
        failures.append("01.34: retention fixture did not create peer-a snapshot.db")
        return

    old_xd = timestamp(-3)
    fresh_xd = timestamp(-1)
    old_bd = timestamp(-91)
    fresh_bd = timestamp(-89)
    paths_by_level = {
        "root": make_retention_dirs(peer_a, old_xd=old_xd, fresh_xd=fresh_xd, old_bd=old_bd, fresh_bd=fresh_bd),
        "nested": make_retention_dirs(peer_a / "nested", old_xd=old_xd, fresh_xd=fresh_xd, old_bd=old_bd, fresh_bd=fresh_bd),
    }

    old_td = timestamp(-181)
    fresh_td = timestamp(-179)
    old_id, fresh_id = insert_tombstone_fixture(peer_a, old_td, fresh_td)

    result = run_cli(str(peer_a), str(peer_b))
    check_success(failures, "01.32/01.33/01.34", "omitted retention flags sync", result)

    for level, paths in paths_by_level.items():
        if paths["old_tmp"].exists():
            failures.append(f"01.32: omitted --xd should remove stale {level} TMP older than 2 days")
        if not paths["fresh_tmp"].is_dir():
            failures.append(f"01.32: omitted --xd should keep fresh {level} TMP newer than 2 days")
        if paths["old_bak"].exists():
            failures.append(f"01.33: omitted --bd should remove stale {level} BAK older than 90 days")
        if not paths["fresh_bak"].is_dir():
            failures.append(f"01.33: omitted --bd should keep fresh {level} BAK newer than 90 days")

    ids_after = snapshot_ids(peer_a)
    if old_id in ids_after:
        failures.append("01.34: omitted --td should purge tombstones older than 180 days")
    if fresh_id not in ids_after:
        failures.append("01.34: omitted --td should keep tombstones newer than 180 days")


def main() -> int:
    failures: list[str] = []
    shutil.rmtree(WORK_DIR, ignore_errors=True)
    WORK_DIR.mkdir(parents=True, exist_ok=True)

    reset_remote = run_ssh(f"rm -rf {shlex.quote(REMOTE_BASE)} && mkdir -p {shlex.quote(REMOTE_BASE)}")
    if reset_remote.returncode != 0:
        failures.append(
            "01.24: remote SFTP fixture reset failed: "
            f"exit={reset_remote.returncode} stdout={reset_remote.stdout!r} stderr={reset_remote.stderr!r}"
        )

    checks = [
        check_01_24_default_mc,
        check_01_29_default_ct,
        check_01_31_default_verbosity,
        check_01_32_33_34_retention_defaults,
    ]
    try:
        for check in checks:
            if failures and check is check_01_24_default_mc:
                continue
            check(failures)
    except subprocess.TimeoutExpired as exc:
        failures.append(f"command timed out: {exc}")
    except Exception as exc:
        failures.append(f"unexpected test error: {exc!r}")
    finally:
        run_ssh(f"rm -rf {shlex.quote(REMOTE_BASE)}")

    print(
        "SKIP 01.30: the --ka default is not reasonably testable through the CLI; "
        "the public surface has no way to insert an idle interval inside one sync run and observe whether "
        "an SFTP connection expires after the default 30 seconds."
    )

    if failures:
        print("\nFAILURES:")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS tests/01_cli-grammar.py")
    return 0


if __name__ == "__main__":
    sys.exit(main())
