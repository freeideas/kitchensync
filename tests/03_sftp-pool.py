#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "paramiko>=3.4,<4",
# ]
# ///

from __future__ import annotations

import shutil
import socket
import re
import stat
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import quote

import paramiko


PROJECT_DIR = Path(__file__).resolve().parents[1]
JAVA = PROJECT_DIR / "tools" / "compiler" / "jdk" / "bin" / "java"
JAR = PROJECT_DIR / "released" / "kitchensync.jar"
WORK_DIR = PROJECT_DIR / "tests" / ".tmp" / "03_sftp_pool"

SSH_HOST = "ordinarydata.com"
SSH_USER = "ace"
REMOTE_ROOT = "/tmp/testks/03_sftp_pool"


@dataclass(frozen=True)
class CliResult:
    args: list[str]
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
        args=[str(JAVA), "-jar", str(JAR), *args],
        returncode=result.returncode,
        stdout=result.stdout,
        stderr=result.stderr,
        elapsed=time.monotonic() - start,
    )


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def write_bytes(path: Path, size: int, seed: int) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    block = bytes(((index + seed) % 251 for index in range(8192)))
    remaining = size
    with path.open("wb") as handle:
        while remaining > 0:
            chunk = block[: min(len(block), remaining)]
            handle.write(chunk)
            remaining -= len(chunk)


def visible_local_files(root: Path) -> dict[str, bytes]:
    files: dict[str, bytes] = {}
    for path in sorted(root.rglob("*")):
        if path.is_file() and ".kitchensync" not in path.parts:
            files[path.relative_to(root).as_posix()] = path.read_bytes()
    return files


def remote_url(path: str, query: str = "", port: int | None = None) -> str:
    host = SSH_HOST if port is None else f"{SSH_HOST}:{port}"
    encoded = quote(path, safe="/")
    suffix = f"?{query}" if query else ""
    return f"sftp://{SSH_USER}@{host}{encoded}{suffix}"


def local_url(path: Path) -> str:
    return path.resolve().as_uri()


def ssh_client() -> paramiko.SSHClient:
    client = paramiko.SSHClient()
    client.load_system_host_keys()
    client.set_missing_host_key_policy(paramiko.WarningPolicy())
    client.connect(
        SSH_HOST,
        username=SSH_USER,
        timeout=15,
        banner_timeout=15,
        auth_timeout=15,
        look_for_keys=True,
        allow_agent=True,
    )
    return client


def remote_rm_rf(sftp: paramiko.SFTPClient, path: str) -> None:
    try:
        entries = sftp.listdir_attr(path)
    except FileNotFoundError:
        return
    for entry in entries:
        child = f"{path.rstrip('/')}/{entry.filename}"
        if entry.st_mode is not None and stat.S_ISDIR(entry.st_mode):
            remote_rm_rf(sftp, child)
        else:
            try:
                sftp.remove(child)
            except FileNotFoundError:
                pass
            except OSError:
                remote_rm_rf(sftp, child)
    try:
        sftp.rmdir(path)
    except FileNotFoundError:
        pass


def remote_mkdir_p(sftp: paramiko.SFTPClient, path: str) -> None:
    parts = [part for part in path.split("/") if part]
    current = ""
    for part in parts:
        current += f"/{part}"
        try:
            sftp.mkdir(current)
        except OSError:
            pass


def remote_read_tree(sftp: paramiko.SFTPClient, root: str) -> dict[str, bytes]:
    files: dict[str, bytes] = {}

    def walk(path: str, rel: str) -> None:
        for entry in sftp.listdir_attr(path):
            child = f"{path.rstrip('/')}/{entry.filename}"
            child_rel = f"{rel}/{entry.filename}" if rel else entry.filename
            if ".kitchensync" in child_rel.split("/"):
                continue
            if entry.st_mode is not None and stat.S_ISDIR(entry.st_mode):
                walk(child, child_rel)
            else:
                with sftp.open(child, "rb") as handle:
                    files[child_rel] = handle.read()

    try:
        walk(root, "")
    except FileNotFoundError:
        return {}
    return files


def remote_read_tree_fresh(root: str) -> dict[str, bytes]:
    client = ssh_client()
    try:
        with client.open_sftp() as sftp:
            return remote_read_tree(sftp, root)
    finally:
        client.close()


def wait_remote_tree(root: str, expected: dict[str, bytes], timeout: float = 10.0) -> dict[str, bytes]:
    deadline = time.monotonic() + timeout
    actual: dict[str, bytes] = {}
    while time.monotonic() < deadline:
        actual = remote_read_tree_fresh(root)
        if actual == expected:
            return actual
        time.sleep(0.5)
    return actual


def describe_result(result: CliResult) -> str:
    return (
        f"exit={result.returncode} elapsed={result.elapsed:.2f}s "
        f"stdout={result.stdout[-1200:]!r} stderr={result.stderr[-1200:]!r}"
    )


def pool_events(stdout: str) -> list[tuple[str, int, int]]:
    events: list[tuple[str, int, int]] = []
    for match in re.finditer(r"endpoint=(\S+)\s+connections=(\d+)/(\d+)", stdout):
        events.append((match.group(1), int(match.group(2)), int(match.group(3))))
    return events


def require_success(failures: list[str], req_ids: str, name: str, result: CliResult) -> None:
    if result.returncode != 0:
        failures.append(f"{req_ids} {name}: expected exit 0; {describe_result(result)}")


def check_local_file_peer_flags() -> list[str]:
    failures: list[str] = []
    root = WORK_DIR / "local_flags"
    source = root / "source"
    dest = root / "dest"
    write_text(source / "alpha.txt", "local alpha\n")
    write_text(source / "nested" / "beta.txt", "local beta\n")
    dest.mkdir(parents=True, exist_ok=True)

    result = run_cli("--mc", "1", "--ct", "1", "--ka", "1", f"+{source}", str(dest))
    require_success(failures, "03.63", "file peers ignore SFTP pool flags", result)
    expected = visible_local_files(source)
    actual = visible_local_files(dest)
    if actual != expected:
        failures.append(f"03.63 file peers ignore SFTP pool flags: expected {sorted(expected)}, got {sorted(actual)}")
    return failures


def check_handshake_timeout_fallback() -> list[str]:
    failures: list[str] = []
    root = WORK_DIR / "handshake_timeout"
    source = root / "source"
    fallback_dest = root / "fallback_dest"
    write_text(source / "fallback.txt", "fallback after stalled ssh\n")
    fallback_dest.mkdir(parents=True, exist_ok=True)

    with StalledSshEndpoint() as stalled:
        peer = (
            f"[sftp://{SSH_USER}@127.0.0.1:{stalled.port}/tmp/testks/never"
            f"?ct=1&mc=1&ka=1,{local_url(fallback_dest)}]"
        )
        result = run_cli("--ct", "30", "--mc", "5", "--ka", "5", f"+{source}", peer, timeout=45)
        require_success(
            failures,
            "03.59 03.62",
            "per-URL ct overrides global ct and failed SFTP fallback is tried",
            result,
        )
        if stalled.accepted < 1:
            failures.append("03.62 SFTP handshake timeout: stalled SSH endpoint was never contacted")
        if result.elapsed >= 12:
            failures.append(
                "03.59 03.62 per-URL ct override: expected fallback well before global --ct 30; "
                f"elapsed={result.elapsed:.2f}s"
            )
    if visible_local_files(fallback_dest) != {"fallback.txt": b"fallback after stalled ssh\n"}:
        failures.append("03.62 SFTP handshake timeout: fallback file peer did not receive source payload")
    return failures


def check_default_and_explicit_sftp_ports(sftp: paramiko.SFTPClient) -> list[str]:
    failures: list[str] = []
    root = WORK_DIR / "ports"
    source_default = root / "source_default"
    source_explicit = root / "source_explicit"
    default_remote = f"{REMOTE_ROOT}/ports/default22"
    explicit_remote = f"{REMOTE_ROOT}/ports/explicit22"
    write_text(source_default / "default.txt", "default port 22\n")
    write_text(source_explicit / "explicit.txt", "explicit port 22\n")

    default_result = run_cli(f"+{source_default}", remote_url(default_remote, "mc=2&ct=20&ka=3"))
    explicit_result = run_cli(f"+{source_explicit}", remote_url(explicit_remote, "mc=2&ct=20&ka=3", port=22))
    require_success(failures, "03.59 03.100", "omitted SFTP port connects to default port 22", default_result)
    require_success(failures, "03.59 03.100", "explicit SFTP port 22 connects to SSH port 22", explicit_result)

    if wait_remote_tree(default_remote, {"default.txt": b"default port 22\n"}) != {"default.txt": b"default port 22\n"}:
        failures.append("03.100 omitted/default port: remote payload was not written through port 22")
    if wait_remote_tree(explicit_remote, {"explicit.txt": b"explicit port 22\n"}) != {"explicit.txt": b"explicit port 22\n"}:
        failures.append("03.100 explicit port: remote payload was not written through explicit port 22")
    return failures


def check_shared_pool_transfer_behavior(sftp: paramiko.SFTPClient) -> list[str]:
    failures: list[str] = []
    root = WORK_DIR / "shared_pool"
    source = root / "source"
    dest_a = f"{REMOTE_ROOT}/shared_pool/path_a"
    dest_b = f"{REMOTE_ROOT}/shared_pool/path_b"
    for index in range(5):
        write_bytes(source / f"blob-{index}.bin", 256 * 1024, index)

    result: CliResult | None = None
    for attempt in range(3):
        if attempt:
            shutil.rmtree(source / ".kitchensync", ignore_errors=True)
            remote_rm_rf(sftp, dest_a)
            remote_rm_rf(sftp, dest_b)
        result = run_cli(
            "-vl",
            "trace",
            "--mc",
            "9",
            "--ka",
            "9",
            f"+{source}",
            remote_url(dest_a, "mc=1&ka=2&ct=20"),
            remote_url(dest_b, "mc=7&ka=20&ct=20", port=22),
            timeout=120,
        )
        if "unreachable peer" not in result.stdout:
            break
    if result is None:
        raise RuntimeError("shared pool sync did not run")
    require_success(
        failures,
        "03.58 03.59 03.60 03.64 03.96 03.97 03.107",
        "same user@host:port remote paths share the first configured capped transfer pool and complete queued copies",
        result,
    )
    expected = visible_local_files(source)
    actual_a = wait_remote_tree(dest_a, expected, timeout=30.0)
    actual_b = wait_remote_tree(dest_b, expected, timeout=30.0)
    if actual_a != expected:
        failures.append(f"03.58 shared pool path A: expected payload files {sorted(expected)}, got {sorted(actual_a)}")
    if actual_b != expected:
        failures.append(f"03.58 shared pool path B: expected payload files {sorted(expected)}, got {sorted(actual_b)}")

    events = pool_events(result.stdout)
    if not events:
        failures.append("03.60 shared pool cap: trace output did not expose any SFTP transfer-pool acquire/release events")
        return failures

    ordinary_events = [event for event in events if event[0] == f"{SSH_USER}@{SSH_HOST}:22"]
    if not ordinary_events:
        failures.append(f"03.96 shared pool identity: expected normalized endpoint {SSH_USER}@{SSH_HOST}:22 in trace events, got {sorted({event[0] for event in events})}")
        return failures
    if any(open_count > max_count for _endpoint, open_count, max_count in ordinary_events):
        failures.append(f"03.60 shared pool cap: trace reported open connections above cap: {ordinary_events}")
    if {max_count for _endpoint, _open_count, max_count in ordinary_events} != {1}:
        failures.append(f"03.59 03.97 03.107 pool settings: first same-endpoint URL mc=1 should set the shared pool cap, got {ordinary_events}")
    return failures


def check_available_transfer_connections_are_used(sftp: paramiko.SFTPClient) -> list[str]:
    failures: list[str] = []
    root = WORK_DIR / "available_connections"
    source = root / "source"
    dest = f"{REMOTE_ROOT}/available_connections/dest"
    for index in range(8):
        write_bytes(source / f"large-{index}.bin", 1024 * 1024, index)

    result = run_cli("-vl", "trace", f"+{source}", remote_url(dest, "mc=2&ct=20&ka=3"), timeout=180)
    require_success(
        failures,
        "03.59 03.60 03.64 03.101",
        "multiple enqueued copies use available destination SFTP transfer connections up to mc",
        result,
    )
    expected = visible_local_files(source)
    actual = wait_remote_tree(dest, expected, timeout=45.0)
    if actual != expected:
        failures.append(f"03.64 transfer connections returned after copies: expected payload files {sorted(expected)}, got {sorted(actual)}")

    ordinary_events = [event for event in pool_events(result.stdout) if event[0] == f"{SSH_USER}@{SSH_HOST}:22"]
    if not ordinary_events:
        failures.append("03.101 transfer concurrency: no trace events for destination transfer pool")
        return failures
    max_open = max(open_count for _endpoint, open_count, _max_count in ordinary_events)
    if max_open < 2:
        failures.append(f"03.101 transfer concurrency: expected trace to show two concurrent destination transfer connections, got {ordinary_events}")
    if any(max_count != 2 for _endpoint, _open_count, max_count in ordinary_events):
        failures.append(f"03.59 per-URL mc override: expected destination pool cap 2, got {ordinary_events}")
    if any(open_count > 2 for _endpoint, open_count, _max_count in ordinary_events):
        failures.append(f"03.60 transfer pool cap: trace reported more than two open destination transfer connections, got {ordinary_events}")
    return failures


def check_explicit_nondefault_sftp_ports() -> list[str]:
    failures: list[str] = []
    root = WORK_DIR / "port_identity"
    source = root / "source"
    fallback_dest = root / "fallback_dest"
    write_text(source / "port.txt", "different port has separate endpoint identity\n")
    fallback_dest.mkdir(parents=True, exist_ok=True)

    with StalledSshEndpoint() as first, StalledSshEndpoint() as second:
        first_endpoint = f"sftp://{SSH_USER}@127.0.0.1:{first.port}/tmp/testks/unused?ct=1&mc=1&ka=1"
        second_endpoint = f"sftp://{SSH_USER}@127.0.0.1:{second.port}/tmp/testks/unused?ct=1&mc=1&ka=1"
        peer = f"[{first_endpoint},{second_endpoint},{local_url(fallback_dest)}]"
        result = run_cli(f"+{source}", peer, timeout=45)
        require_success(failures, "03.100", "explicit non-default SFTP ports are contacted as SSH ports", result)
        if first.accepted < 1:
            failures.append("03.100 explicit non-default port: first local SFTP port was not contacted")
        if second.accepted < 1:
            failures.append("03.100 explicit non-default port: second local SFTP port was not contacted")
    if visible_local_files(fallback_dest) != {"port.txt": b"different port has separate endpoint identity\n"}:
        failures.append("03.100 explicit non-default port fallback: fallback peer did not receive payload")
    return failures


# 03.61 and 03.106 require observing the identity and idle timer of a returned
# SSH+SFTP connection inside one CLI process. The public CLI exposes pool counts
# at trace level, not connection identity or keep-alive timer resets, so those
# exact timer semantics are not reasonably testable in this root black-box test.
#
# 03.112 and 03.114 distinguish startup/listing connections from lazily-created
# transfer-pool connections. Trace output makes transfer-pool counts observable
# during copies, but it does not expose startup connection bookkeeping directly;
# asserting that lifecycle boundary here would invent an implementation detail.
#
# 03.96 is tested for same-endpoint sharing with omitted port and explicit :22.
# The "different ports do not share a pool" half would require two reachable,
# trusted SFTP services for the same user+host on different SSH ports; the root
# SFTP fixture only provides ordinarydata.com:22.


def main() -> int:
    failures: list[str] = []
    if WORK_DIR.exists():
        shutil.rmtree(WORK_DIR)
    WORK_DIR.mkdir(parents=True, exist_ok=True)

    try:
        client = ssh_client()
    except Exception as exc:
        print(f"FAIL tests/03_sftp-pool.py: could not connect to SFTP fixture {SSH_USER}@{SSH_HOST}: {exc!r}")
        return 1

    try:
        with client.open_sftp() as sftp:
            remote_rm_rf(sftp, REMOTE_ROOT)
            remote_mkdir_p(sftp, REMOTE_ROOT)

            checks = [
                ("03.63", check_local_file_peer_flags),
                ("03.59/03.62", check_handshake_timeout_fallback),
                ("03.59/03.100", lambda: check_default_and_explicit_sftp_ports(sftp)),
                (
                    "03.58/03.59/03.60/03.64/03.96/03.97/03.107",
                    lambda: check_shared_pool_transfer_behavior(sftp),
                ),
                ("03.59/03.60/03.64/03.101", lambda: check_available_transfer_connections_are_used(sftp)),
                ("03.100", check_explicit_nondefault_sftp_ports),
            ]

            for label, check in checks:
                try:
                    check_failures = check()
                except subprocess.TimeoutExpired as exc:
                    check_failures = [f"{label}: command timed out: {exc}"]
                except Exception as exc:
                    check_failures = [f"{label}: unexpected test error: {exc!r}"]
                if check_failures:
                    failures.extend(check_failures)
                else:
                    print(f"PASS {label}")
    finally:
        client.close()

    if failures:
        print("\nFAILURES:")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS tests/03_sftp-pool.py")
    return 0


if __name__ == "__main__":
    sys.exit(main())
