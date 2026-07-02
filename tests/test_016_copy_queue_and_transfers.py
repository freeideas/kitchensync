# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko>=3.4", "cryptography"]
# ///
from __future__ import annotations

import os
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path

import paramiko

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")
EXT_SFTP = WORKSPACE / "extart" / "ephemeral-sftp-server.py"


# not reasonably testable: 016.1 requires observing traversal progress before
# the full tree scan, but the allowed surface exposes copy progress and final
# filesystem state, not directory-list timing.
# not reasonably testable: 016.16
# not reasonably testable: 016.17
# not reasonably testable: 016.18
# not reasonably testable: 016.19
# not reasonably testable: 016.20
# not reasonably testable: 016.21
# Deterministic retry ordering needs controllable transfer failures across local
# and SFTP transports; the released CLI exposes only final diagnostics/state.
# not reasonably testable: 016.29
# not reasonably testable: 016.30
# not reasonably testable: 016.31
# not reasonably testable: 016.32
# These require stopping or failing the replace sequence at one internal phase.
# not reasonably testable: 016.33
# not reasonably testable: 016.34
# Bounded buffering is not visible through exit code, stdout/stderr, or peer
# filesystem state without process-memory or transport-level instrumentation.


@dataclass
class RunResult:
    args: list[str]
    code: int
    stdout: str
    stderr: str


class Checks:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def check(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)

    def equal(self, actual: object, expected: object, message: str) -> None:
        if actual != expected:
            self.failures.append(f"{message}: expected {expected!r}, got {actual!r}")


def uv_path() -> Path:
    if sys.platform.startswith("win"):
        return WORKSPACE / "aitc" / "bin" / "uv.exe"
    if sys.platform == "darwin":
        return WORKSPACE / "aitc" / "bin" / "uv.mac"
    return WORKSPACE / "aitc" / "bin" / "uv.linux"


def run_sync(args: list[str], env: dict[str, str] | None = None, timeout: float = 60.0) -> RunResult:
    proc_env = os.environ.copy()
    if env:
        proc_env.update(env)
    completed = subprocess.run(
        [str(EXE), *args],
        cwd=str(WORKSPACE),
        env=proc_env,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    )
    return RunResult(args=args, code=completed.returncode, stdout=completed.stdout, stderr=completed.stderr)


def write_bytes(path: Path, data: bytes, mtime: float | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    if mtime is not None:
        os.utime(path, (mtime, mtime))


def read_bytes(path: Path) -> bytes:
    return path.read_bytes()


def progress_lines(stdout: str) -> list[str]:
    return [line for line in stdout.splitlines() if re.match(r"^[CX] ", line)]


def slot_values(stdout: str) -> list[tuple[int, int]]:
    values: list[tuple[int, int]] = []
    for line in stdout.splitlines():
        match = re.search(r"copy-slots active=(\d+)/(\d+)", line)
        if match:
            values.append((int(match.group(1)), int(match.group(2))))
    return values


def assert_clean_success(checks: Checks, result: RunResult, label: str) -> None:
    checks.equal(result.code, 0, f"{label} exits 0\nstdout:\n{result.stdout}\nstderr:\n{result.stderr}")
    checks.equal(result.stderr, "", f"{label} writes no stderr")


def assert_slot_limit(checks: Checks, result: RunResult, expected_limit: int, label: str) -> None:
    slots = slot_values(result.stdout)
    checks.check(slots, f"{label} emits trace copy-slot lines")
    for active, maximum in slots:
        checks.equal(maximum, expected_limit, f"{label} trace reports configured copy limit")
        checks.check(
            active <= expected_limit,
            f"{label} active copy slots exceeded {expected_limit}: active={active}",
        )


def make_home_with_known_hosts(port: int, host_key: str) -> tempfile.TemporaryDirectory[str]:
    home = tempfile.TemporaryDirectory(prefix="ks-home-")
    ssh_dir = Path(home.name) / ".ssh"
    ssh_dir.mkdir(parents=True, exist_ok=True)
    known_hosts = ssh_dir / "known_hosts"
    known_hosts.write_text(f"[127.0.0.1]:{port} {host_key}\n", encoding="ascii", newline="\n")
    return home


def reader_thread(stream, out: "queue.Queue[str | None]") -> threading.Thread:
    def run() -> None:
        try:
            for line in stream:
                out.put(line.rstrip("\r\n"))
        finally:
            out.put(None)

    thread = threading.Thread(target=run, daemon=True)
    thread.start()
    return thread


class SftpServer:
    def __init__(self, user: str, password: str) -> None:
        self.user = user
        self.password = password
        self.proc: subprocess.Popen[str] | None = None
        self.port: int | None = None
        self.host_key: str | None = None
        self.home: tempfile.TemporaryDirectory[str] | None = None
        self._stdout_q: "queue.Queue[str | None]" = queue.Queue()
        self._stderr_q: "queue.Queue[str | None]" = queue.Queue()

    def start(self) -> None:
        self.proc = subprocess.Popen(
            [
                str(uv_path()),
                "run",
                "--script",
                str(EXT_SFTP),
                "--user",
                self.user,
                "--password",
                self.password,
            ],
            cwd=str(WORKSPACE),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        assert self.proc.stdout is not None
        assert self.proc.stderr is not None
        reader_thread(self.proc.stdout, self._stdout_q)
        reader_thread(self.proc.stderr, self._stderr_q)

        try:
            line = self._stdout_q.get(timeout=20.0)
        except queue.Empty as exc:
            raise RuntimeError("SFTP server did not print a port") from exc
        if line is None or not line.isdigit():
            raise RuntimeError(f"SFTP server printed invalid port line: {line!r}")
        self.port = int(line)

        deadline = time.monotonic() + 20.0
        stderr_seen: list[str] = []
        while time.monotonic() < deadline:
            try:
                err_line = self._stderr_q.get(timeout=0.2)
            except queue.Empty:
                continue
            if err_line is None:
                break
            stderr_seen.append(err_line)
            if err_line.startswith("host key: "):
                self.host_key = err_line.removeprefix("host key: ")
                break
        if self.host_key is None:
            raise RuntimeError("SFTP server did not print a host key: " + "\n".join(stderr_seen))
        self.home = make_home_with_known_hosts(self.port, self.host_key)

    def url(self, root: str) -> str:
        assert self.port is not None
        return f"sftp://{self.user}:{self.password}@127.0.0.1:{self.port}{root}"

    def env(self) -> dict[str, str]:
        assert self.home is not None
        return {
            "HOME": self.home.name,
            "USERPROFILE": self.home.name,
            "SSH_AUTH_SOCK": "",
        }

    def connect(self) -> tuple[paramiko.SSHClient, paramiko.SFTPClient]:
        assert self.port is not None
        assert self.home is not None
        client = paramiko.SSHClient()
        client.load_host_keys(str(Path(self.home.name) / ".ssh" / "known_hosts"))
        client.set_missing_host_key_policy(paramiko.RejectPolicy())
        client.connect(
            "127.0.0.1",
            port=self.port,
            username=self.user,
            password=self.password,
            allow_agent=False,
            look_for_keys=False,
            timeout=15.0,
        )
        return client, client.open_sftp()

    def stop(self) -> None:
        if self.proc is not None and self.proc.poll() is None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=10.0)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(timeout=10.0)
        if self.home is not None:
            self.home.cleanup()


def sftp_mkdirs(sftp: paramiko.SFTPClient, path: str) -> None:
    current = ""
    for part in path.strip("/").split("/"):
        if not part:
            continue
        current += "/" + part
        try:
            sftp.mkdir(current)
        except OSError:
            pass


def sftp_write(server: SftpServer, path: str, data: bytes) -> None:
    client, sftp = server.connect()
    try:
        parent = "/" + "/".join(path.strip("/").split("/")[:-1])
        if parent != "/":
            sftp_mkdirs(sftp, parent)
        with sftp.open(path, "wb") as handle:
            handle.write(data)
    finally:
        sftp.close()
        client.close()


def sftp_read(server: SftpServer, path: str) -> bytes:
    client, sftp = server.connect()
    try:
        with sftp.open(path, "rb") as handle:
            return handle.read()
    finally:
        sftp.close()
        client.close()


def combined_env(*servers: SftpServer) -> dict[str, str]:
    if not servers:
        return {}
    env = servers[0].env()
    if len(servers) == 1:
        return env
    known_hosts = Path(env["HOME"]) / ".ssh" / "known_hosts"
    with known_hosts.open("a", encoding="ascii", newline="\n") as out:
        for server in servers[1:]:
            assert server.port is not None
            assert server.host_key is not None
            out.write(f"[127.0.0.1]:{server.port} {server.host_key}\n")
    return env


def test_local_replacement_and_archive(checks: Checks) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-016-replace-") as tmp_name:
        tmp = Path(tmp_name)
        src = tmp / "src"
        dst = tmp / "dst"
        now = time.time()
        write_bytes(src / "replace.txt", b"new replacement\n", now - 10)
        write_bytes(src / "fresh.txt", b"new file\n", now - 20)
        write_bytes(dst / "replace.txt", b"old destination\n", now - 1000)

        result = run_sync(["--verbosity", "trace", "--max-copies", "2", f"+{src}", str(dst)])
        assert_clean_success(checks, result, "local replacement run")
        assert_slot_limit(checks, result, 2, "local replacement run")

        checks.equal(read_bytes(dst / "replace.txt"), b"new replacement\n", "016.24 final path receives SWAP new content")
        checks.equal(read_bytes(dst / "fresh.txt"), b"new file\n", "016.27 new destination file is copied")
        checks.check(
            abs((dst / "replace.txt").stat().st_mtime - (src / "replace.txt").stat().st_mtime) <= 5.0,
            "016.25 destination modification time matches the winning source time within tolerance",
        )

        bak_files = list((dst / ".kitchensync" / "BAK").glob("*/replace.txt"))
        checks.equal(len(bak_files), 1, "016.26 replaced old file is archived to BAK")
        if bak_files:
            checks.equal(bak_files[0].read_bytes(), b"old destination\n", "016.23 archived BAK file contains old destination content")
        fresh_bak = list((dst / ".kitchensync" / "BAK").glob("*/fresh.txt"))
        checks.equal(fresh_bak, [], "016.27 new destination path creates no BAK entry")

        swap_root = dst / ".kitchensync" / "SWAP"
        lingering_swap = [p for p in swap_root.rglob("*")] if swap_root.exists() else []
        checks.equal(lingering_swap, [], "016.28 successful transfer removes empty SWAP directories")
        checks.check(
            not (dst / ".kitchensync" / "SWAP" / "replace.txt" / "new").exists(),
            "016.22 replacement SWAP new is not left after success",
        )
        checks.check(
            bak_files and (dst / "replace.txt").exists(),
            "016.35 local-to-local copy replaced through recoverable staging rather than losing the old file",
        )


def test_copy_slot_limits_and_non_copy_work(checks: Checks) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-016-slots-") as tmp_name:
        tmp = Path(tmp_name)
        source = tmp / "source"
        target_default = tmp / "target-default"
        target_limited = tmp / "target-limited"
        target_dirs = tmp / "target-dirs"
        for idx in range(12):
            write_bytes(source / f"file-{idx:02d}.bin", bytes([idx]) * 65536)

        default_run = run_sync(["--verbosity", "trace", f"+{source}", str(target_default)])
        assert_clean_success(checks, default_run, "default copy-limit run")
        assert_slot_limit(checks, default_run, 10, "016.2 default copy-limit run")

        limited_run = run_sync(["--verbosity", "trace", "--max-copies", "3", f"+{source}", str(target_limited)])
        assert_clean_success(checks, limited_run, "explicit copy-limit run")
        assert_slot_limit(checks, limited_run, 3, "016.3 explicit copy-limit run")

        for idx in range(12):
            checks.equal(
                read_bytes(target_limited / f"file-{idx:02d}.bin"),
                bytes([idx]) * 65536,
                f"016.5 file:// to file:// copy completed for file-{idx:02d}.bin",
            )

        dirs_src = tmp / "dirs-src"
        (dirs_src / "only-dir" / "nested").mkdir(parents=True)
        old_meta = target_dirs / ".kitchensync"
        (old_meta / "BAK" / "2000-01-01_00-00-00_000000Z").mkdir(parents=True)
        (old_meta / "TMP" / "2000-01-01_00-00-00_000000Z").mkdir(parents=True)
        no_copy_run = run_sync(
            [
                "--verbosity",
                "trace",
                "--max-copies",
                "1",
                "--keep-bak-days",
                "1",
                "--keep-tmp-days",
                "1",
                f"+{dirs_src}",
                str(target_dirs),
            ]
        )
        assert_clean_success(checks, no_copy_run, "non-copy metadata run")
        checks.equal(slot_values(no_copy_run.stdout), [], "016.9/016.10/016.11/016.12 metadata work uses no copy slots")
        checks.equal(progress_lines(no_copy_run.stdout), [], "directory creation, snapshots, and cleanup emit no C/X progress lines")


def test_global_limit_across_destinations(checks: Checks) -> None:
    with tempfile.TemporaryDirectory(prefix="ks-016-global-") as tmp_name:
        tmp = Path(tmp_name)
        src = tmp / "src"
        dst_a = tmp / "dst-a"
        dst_b = tmp / "dst-b"
        dst_c = tmp / "dst-c"
        for idx in range(8):
            write_bytes(src / f"shared-{idx}.dat", b"x" * 131072)

        result = run_sync(["--verbosity", "trace", "--max-copies", "4", f"+{src}", str(dst_a), str(dst_b), str(dst_c)])
        assert_clean_success(checks, result, "multi-destination copy run")
        assert_slot_limit(checks, result, 4, "016.4 global active-copy limit")
        checks.check(slot_values(result.stdout), "016.13/016.14/016.15 same source peer can use the global slot pool")
        for dst in (dst_a, dst_b, dst_c):
            for idx in range(8):
                checks.equal(read_bytes(dst / f"shared-{idx}.dat"), b"x" * 131072, f"multi-destination copy reached {dst.name}")


def test_sftp_scheme_copy_slots(checks: Checks) -> None:
    server_a = SftpServer("ace", "pw")
    server_b = SftpServer("ace", "pw")
    try:
        server_a.start()
        server_b.start()
        with tempfile.TemporaryDirectory(prefix="ks-016-sftp-") as tmp_name:
            tmp = Path(tmp_name)

            local_src = tmp / "local-src"
            write_bytes(local_src / "local-to-sftp.txt", b"local to sftp\n")
            local_to_sftp = run_sync(
                [
                    "--verbosity",
                    "trace",
                    "--max-copies",
                    "1",
                    f"+{local_src}",
                    server_a.url("/local-dst"),
                ],
                env=server_a.env(),
            )
            assert_clean_success(checks, local_to_sftp, "file to sftp run")
            assert_slot_limit(checks, local_to_sftp, 1, "016.6 file:// to sftp:// copy")
            checks.equal(sftp_read(server_a, "/local-dst/local-to-sftp.txt"), b"local to sftp\n", "016.6 file to SFTP destination content")

            sftp_write(server_a, "/remote-src/sftp-to-local.txt", b"sftp to local\n")
            local_dst = tmp / "local-dst"
            sftp_to_local = run_sync(
                [
                    "--verbosity",
                    "trace",
                    "--max-copies",
                    "1",
                    f"+{server_a.url('/remote-src')}",
                    str(local_dst),
                ],
                env=server_a.env(),
            )
            assert_clean_success(checks, sftp_to_local, "sftp to file run")
            assert_slot_limit(checks, sftp_to_local, 1, "016.7 sftp:// to file:// copy")
            checks.equal(read_bytes(local_dst / "sftp-to-local.txt"), b"sftp to local\n", "016.7 SFTP to local destination content")

            sftp_write(server_a, "/sftp-src/sftp-to-sftp.txt", b"sftp to sftp\n")
            sftp_to_sftp = run_sync(
                [
                    "--verbosity",
                    "trace",
                    "--max-copies",
                    "1",
                    f"+{server_a.url('/sftp-src')}",
                    server_b.url("/sftp-dst"),
                ],
                env=combined_env(server_a, server_b),
            )
            assert_clean_success(checks, sftp_to_sftp, "sftp to sftp run")
            assert_slot_limit(checks, sftp_to_sftp, 1, "016.8 sftp:// to sftp:// copy")
            checks.equal(sftp_read(server_b, "/sftp-dst/sftp-to-sftp.txt"), b"sftp to sftp\n", "016.8 SFTP to SFTP destination content")
    finally:
        server_b.stop()
        server_a.stop()


def main() -> int:
    checks = Checks()
    checks.check(EXE.exists(), f"released executable exists at {EXE}")
    tests = [
        test_local_replacement_and_archive,
        test_copy_slot_limits_and_non_copy_work,
        test_global_limit_across_destinations,
        test_sftp_scheme_copy_slots,
    ]
    for test in tests:
        try:
            test(checks)
        except Exception as exc:  # noqa: BLE001 -- collect every test section failure
            checks.failures.append(f"{test.__name__} raised {type(exc).__name__}: {exc}")

    if checks.failures:
        print("FAIL test_016_copy_queue_and_transfers")
        for index, failure in enumerate(checks.failures, 1):
            print(f"{index}. {failure}")
        return 1
    print("PASS test_016_copy_queue_and_transfers")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
