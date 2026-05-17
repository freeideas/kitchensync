#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko==3.5.1"]
# ///

from __future__ import annotations

import errno
import os
import posixpath
import re
import shutil
import socket
import stat
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from pathlib import Path

import paramiko
from paramiko import SFTPAttributes, SFTPHandle, SFTPServerInterface
from paramiko.sftp import SFTP_FAILURE, SFTP_NO_SUCH_FILE, SFTP_OK

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")

SFTP_USER = "ace"
SFTP_PASSWORD = "pw"
SFTP_BASE = "/tmp/testks"


def errno_to_sftp(error: int) -> int:
    if error == errno.ENOENT:
        return SFTP_NO_SUCH_FILE
    return SFTP_FAILURE


class RootedSFTPHandle(SFTPHandle):
    def stat(self):
        try:
            file_obj = self.readfile or self.writefile
            return SFTPAttributes.from_stat(os.fstat(file_obj.fileno()))
        except OSError as exc:
            return errno_to_sftp(exc.errno)


class RootedSFTPServer(SFTPServerInterface):
    def __init__(self, server, root: Path):
        super().__init__(server)
        self.root = root.resolve()

    def _local(self, path: str) -> Path:
        resolved = (self.root / path.lstrip("/")).resolve()
        if resolved != self.root and self.root not in resolved.parents:
            raise OSError(errno.EACCES, "path escapes SFTP root")
        return resolved

    def canonicalize(self, path: str) -> str:
        return "/" + self._local(path).relative_to(self.root).as_posix()

    def list_folder(self, path: str):
        try:
            entries = []
            for name in os.listdir(self._local(path)):
                attrs = SFTPAttributes.from_stat(os.stat(self._local(path) / name))
                attrs.filename = name
                entries.append(attrs)
            return entries
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def stat(self, path: str):
        try:
            return SFTPAttributes.from_stat(os.stat(self._local(path)))
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def lstat(self, path: str):
        try:
            return SFTPAttributes.from_stat(os.lstat(self._local(path)))
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def open(self, path: str, flags: int, attr):
        try:
            local = self._local(path)
            local.parent.mkdir(parents=True, exist_ok=True)
            fd = os.open(local, flags, getattr(attr, "st_mode", None) or 0o666)
            handle = RootedSFTPHandle(flags)
            if flags & os.O_RDWR:
                file_obj = os.fdopen(fd, "r+b", buffering=0)
                handle.readfile = file_obj
                handle.writefile = file_obj
            elif flags & os.O_WRONLY:
                handle.writefile = os.fdopen(fd, "wb", buffering=0)
            else:
                handle.readfile = os.fdopen(fd, "rb", buffering=0)
            return handle
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def remove(self, path: str):
        try:
            os.remove(self._local(path))
            return SFTP_OK
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def rename(self, oldpath: str, newpath: str):
        try:
            old = self._local(oldpath)
            new = self._local(newpath)
            new.parent.mkdir(parents=True, exist_ok=True)
            os.replace(old, new)
            return SFTP_OK
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def mkdir(self, path: str, attr):
        try:
            os.mkdir(self._local(path), getattr(attr, "st_mode", None) or 0o777)
            return SFTP_OK
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def rmdir(self, path: str):
        try:
            os.rmdir(self._local(path))
            return SFTP_OK
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def chmod(self, path: str, attr):
        try:
            if attr.st_mode is not None:
                os.chmod(self._local(path), stat.S_IMODE(attr.st_mode))
            return SFTP_OK
        except OSError as exc:
            return errno_to_sftp(exc.errno)

    def chattr(self, path: str, attr):
        try:
            local = self._local(path)
            if attr.st_mode is not None:
                os.chmod(local, stat.S_IMODE(attr.st_mode))
            if attr.st_atime is not None and attr.st_mtime is not None:
                os.utime(local, (attr.st_atime, attr.st_mtime))
            return SFTP_OK
        except OSError as exc:
            return errno_to_sftp(exc.errno)


class PasswordServer(paramiko.ServerInterface):
    def check_auth_password(self, username: str, password: str) -> int:
        if username == SFTP_USER and password == SFTP_PASSWORD:
            return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED

    def get_allowed_auths(self, username: str) -> str:
        return "password"

    def check_channel_request(self, kind: str, chanid: int) -> int:
        if kind == "session":
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED


class SFTPFixture:
    def __init__(self, root: Path):
        self.root = root
        self.host_key = paramiko.RSAKey.generate(2048)
        self._stop = threading.Event()
        self._threads: list[threading.Thread] = []
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind(("127.0.0.1", 0))
        self._sock.listen(100)
        self.port = self._sock.getsockname()[1]
        self._accept_thread = threading.Thread(target=self._accept_loop, daemon=True)

    def __enter__(self) -> SFTPFixture:
        self.root.mkdir(parents=True, exist_ok=True)
        self._accept_thread.start()
        return self

    def __exit__(self, _exc_type, _exc, _tb) -> None:
        self.close()

    def close(self) -> None:
        self._stop.set()
        try:
            self._sock.close()
        except OSError:
            pass
        self._accept_thread.join(timeout=2)
        for thread in self._threads:
            thread.join(timeout=2)

    def known_hosts_line(self) -> str:
        return f"[127.0.0.1]:{self.port} {self.host_key.get_name()} {self.host_key.get_base64()}\n"

    def url(self, path: str) -> str:
        return f"sftp://{SFTP_USER}:{SFTP_PASSWORD}@127.0.0.1:{self.port}{path}"

    def pool_re(self) -> re.Pattern[str]:
        return re.compile(rf"endpoint={SFTP_USER}@127\.0\.0\.1:{self.port} connections=(\d+)/(\d+)")

    def _accept_loop(self) -> None:
        while not self._stop.is_set():
            try:
                client, _addr = self._sock.accept()
            except OSError:
                break
            thread = threading.Thread(target=self._serve_client, args=(client,), daemon=True)
            self._threads.append(thread)
            thread.start()

    def _serve_client(self, client: socket.socket) -> None:
        transport = paramiko.Transport(client)
        try:
            transport.add_server_key(self.host_key)
            transport.set_subsystem_handler("sftp", paramiko.SFTPServer, RootedSFTPServer, self.root)
            transport.start_server(server=PasswordServer())
            while not self._stop.is_set() and transport.is_active():
                time.sleep(0.05)
        except Exception:
            pass
        finally:
            transport.close()


def run_cli(*args: str, home: Path | None = None, timeout: int = 60) -> subprocess.CompletedProcess[str]:
    command = [str(JAVA), "-jar", str(JAR), *args]
    env = None
    if home is not None:
        env = {**os.environ, "HOME": str(home), "USERPROFILE": str(home)}
        env.pop("SSH_AUTH_SOCK", None)
        env.pop("SSH_AGENT_PID", None)
        command = [str(JAVA), f"-Duser.home={home}", "-jar", str(JAR), *args]
    try:
        return subprocess.run(
            command,
            cwd=str(PROJECT_DIR),
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        stdout = exc.stdout if isinstance(exc.stdout, str) else (exc.stdout or b"").decode("utf-8", "replace")
        stderr = exc.stderr if isinstance(exc.stderr, str) else (exc.stderr or b"").decode("utf-8", "replace")
        return subprocess.CompletedProcess(command, 124, stdout, stderr)


def local_peer(path: Path, canon: bool = False) -> str:
    rendered = str(path)
    return f"+{rendered}" if canon else rendered


def sftp_url(path: str, canon: bool = False) -> str:
    raise RuntimeError("use SFTPFixture.url")


def sftp_peer(fixture: SFTPFixture, path: str, canon: bool = False) -> str:
    url = fixture.url(path)
    return f"+{url}" if canon else url


def write_known_hosts(home: Path, fixture: SFTPFixture) -> None:
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(parents=True, exist_ok=True)
    known_hosts = ssh_dir / "known_hosts"
    known_hosts.write_text(fixture.known_hosts_line(), encoding="utf-8", newline="\n")
    known_hosts.chmod(0o600)


def prepare_remote_fixture(root: Path, remote_root: str) -> None:
    local_root = root / remote_root.lstrip("/")
    if local_root.exists():
        shutil.rmtree(local_root)
    for name in ("trace", "info", "debug", "error"):
        (local_root / name).mkdir(parents=True)
    (local_root / "not_a_directory").write_text("not a directory\n", encoding="utf-8")


def lines(text: str) -> list[str]:
    return [line.strip() for line in text.splitlines() if line.strip()]


def check(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def count_line(proc: subprocess.CompletedProcess[str], expected: str) -> int:
    return lines(proc.stdout).count(expected)


def make_local_copy_case(root: Path) -> tuple[Path, Path, Path]:
    src = root / "src"
    dst_a = root / "dst_a"
    dst_b = root / "dst_b"
    for path in (src, dst_a, dst_b):
        path.mkdir(parents=True, exist_ok=True)
    (src / "copy_only.txt").write_text("copy\n", encoding="utf-8")
    (src / "replace.txt").write_text("new\n", encoding="utf-8")
    (dst_a / "replace.txt").write_text("old a\n", encoding="utf-8")
    (dst_b / "replace.txt").write_text("old b\n", encoding="utf-8")
    return src, dst_a, dst_b


def make_local_copy_only_case(root: Path) -> tuple[Path, Path]:
    src = root / "src"
    dst = root / "dst"
    src.mkdir(parents=True, exist_ok=True)
    dst.mkdir(parents=True, exist_ok=True)
    (src / "fresh.txt").write_text("fresh\n", encoding="utf-8")
    return src, dst


def record_process(failures: list[str], name: str, proc: subprocess.CompletedProcess[str]) -> None:
    check(failures, proc.stderr == "", f"{name}: stderr must be empty, got {proc.stderr!r}")


def check_list_dir_failure_output(
    failures: list[str],
    proc: subprocess.CompletedProcess[str],
    level: str,
    affected_directory: str,
) -> None:
    failure_output = proc.stdout
    check(failures, failure_output != "", f"03.85: list_dir failure at {level} verbosity must produce a stdout log line")
    check(failures, "127.0.0.1" in failure_output and SFTP_USER in failure_output, f"03.85: list_dir failure must identify the affected peer, got {failure_output!r}")
    check(failures, affected_directory in failure_output, f"03.85: list_dir failure must identify the affected directory, got {failure_output!r}")


def exercise_local_logging(failures: list[str], root: Path) -> None:
    src, dst_a, dst_b = make_local_copy_case(root / "info")
    info = run_cli("-vl", "info", local_peer(src, canon=True), local_peer(dst_a), local_peer(dst_b))
    record_process(failures, "local info sync", info)
    check(failures, info.returncode == 0, f"local info sync exited {info.returncode}: {info.stdout!r}")
    check(failures, count_line(info, "C copy_only.txt") == 1, "03.78: copy decision must log 'C copy_only.txt' exactly once for two destination peers")
    check(failures, count_line(info, "C replace.txt") == 1, "03.78: replacement copy decision must log 'C replace.txt' exactly once for two destination peers")
    check(failures, count_line(info, "X replace.txt") == 1, "03.79: displacement decision must log 'X replace.txt' exactly once for two affected peers")

    src, dst = make_local_copy_only_case(root / "error")
    error = run_cli("-vl", "error", local_peer(src, canon=True), local_peer(dst))
    record_process(failures, "local error sync", error)
    check(failures, error.returncode == 0, f"local error sync exited {error.returncode}: {error.stdout!r}")
    check(failures, not any(line.startswith(("C ", "X ")) for line in lines(error.stdout)), "03.81: C/X progress lines must not appear at -vl error")

    src, dst = make_local_copy_only_case(root / "debug_info")
    debug_info = run_cli("-vl", "info", local_peer(src, canon=True), local_peer(dst))
    record_process(failures, "debug comparison info sync", debug_info)
    src, dst = make_local_copy_only_case(root / "debug")
    debug = run_cli("-vl", "debug", local_peer(src, canon=True), local_peer(dst))
    record_process(failures, "debug sync", debug)
    check(failures, debug.returncode == 0, f"debug sync exited {debug.returncode}: {debug.stdout!r}")
    check(failures, lines(debug.stdout) == ["C fresh.txt"], f"03.98: debug output must contain only info-level progress lines, got {lines(debug.stdout)!r}")
    check(failures, lines(debug.stdout) == lines(debug_info.stdout), "03.98: -vl debug output must be observationally identical to -vl info for the same copy scenario")


def exercise_sftp_logging(
    failures: list[str],
    root: Path,
    remote_root: str,
    fixture: SFTPFixture,
    home: Path,
) -> None:
    pool_re = fixture.pool_re()
    for level in ("info", "debug", "error", "trace"):
        src = root / f"sftp_{level}_src"
        src.mkdir(parents=True, exist_ok=True)
        filename = f"{level}.txt"
        (src / filename).write_text(f"{level}\n", encoding="utf-8")
        proc = run_cli(
            "--ct",
            "10",
            "-vl",
            level,
            local_peer(src, canon=True),
            sftp_peer(fixture, posixpath.join(remote_root, level)),
            home=home,
            timeout=60,
        )
        record_process(failures, f"sftp {level} sync", proc)
        check(failures, proc.returncode == 0, f"sftp {level} sync exited {proc.returncode}: stdout={proc.stdout!r}")
        pool_matches = pool_re.findall(proc.stdout)
        if level == "trace":
            check(failures, count_line(proc, f"C {filename}") == 1, "03.105: trace verbosity must include info-level C progress lines")
            check(failures, bool(pool_matches), "03.82: trace verbosity must emit SFTP pool acquire/release events")
            check(failures, any(int(used) > 0 for used, _ in pool_matches), "03.109: trace output must include an SFTP pool acquire line with an active connection")
            check(failures, any(int(used) == 0 for used, _ in pool_matches), "03.109: trace output must include an SFTP pool release line returning to zero active connections")
            check(failures, all(int(maximum) == 10 for _, maximum in pool_matches), "03.84: pool lines must report connections as <n>/<max> using the default max of 10")
        else:
            check(failures, not pool_matches, f"03.83: pool acquire/release events must not appear at -vl {level}")
            if level in ("info", "debug"):
                check(failures, count_line(proc, f"C {filename}") == 1, f"03.105: {level} verbosity must include info-level C progress lines")
            else:
                check(failures, not any(line.startswith(("C ", "X ")) for line in lines(proc.stdout)), "03.81: error verbosity must suppress C/X progress lines for SFTP syncs too")

    missing_dir = posixpath.join(remote_root, "not_a_directory")
    for level in ("error", "info", "debug", "trace"):
        src = root / f"list_error_{level}_src"
        src.mkdir(parents=True, exist_ok=True)
        (src / "probe.txt").write_text("probe\n", encoding="utf-8")
        failure = run_cli(
            "--ct",
            "10",
            "-vl",
            level,
            local_peer(src, canon=True),
            sftp_peer(fixture, missing_dir),
            home=home,
            timeout=60,
        )
        record_process(failures, f"sftp list_dir failure at {level}", failure)
        check_list_dir_failure_output(failures, failure, level, "not_a_directory")
        if level == "error":
            check(failures, not any(line.startswith(("C ", "X ")) for line in lines(failure.stdout)), "03.81: list_dir error output at -vl error must not include C/X progress lines")


def main() -> int:
    failures: list[str] = []
    run_id = f"ks03_logging_{uuid.uuid4().hex}"
    remote_root = posixpath.join(SFTP_BASE, run_id)

    with tempfile.TemporaryDirectory(prefix="ks03_logging_", dir=str(PROJECT_DIR / "tests")) as tmp:
        local_root = Path(tmp)
        try:
            exercise_local_logging(failures, local_root / "local")
            remote_fs = local_root / "sftp_fs"
            home = local_root / "home"
            prepare_remote_fixture(remote_fs, remote_root)
            with SFTPFixture(remote_fs) as fixture:
                write_known_hosts(home, fixture)
                exercise_sftp_logging(failures, local_root / "remote", remote_root, fixture, home)
        except Exception as exc:
            failures.append(f"test setup or execution raised {type(exc).__name__}: {exc}")

    if failures:
        print("FAILURES:")
        for failure in failures:
            print(f"- {failure}")
        return 1
    print("03_logging passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
