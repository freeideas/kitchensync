#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko==3.5.1"]
# ///

from __future__ import annotations

import errno
import getpass
import os
import shutil
import socket
import stat
import subprocess
import sys
import threading
import time
from pathlib import Path

import paramiko
from paramiko import SFTPAttributes, SFTPHandle, SFTPServerInterface
from paramiko.sftp import SFTP_FAILURE, SFTP_NO_SUCH_FILE, SFTP_OK


PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")

WORK_DIR = PROJECT_DIR / "tests" / ".tmp" / "02_url_normalization"
LOCAL_CWD = WORK_DIR / "cwd"
LOCAL_PEER = LOCAL_CWD / "peer"
LOCAL_SINK = WORK_DIR / "sink"

REMOTE_LOGIN_USER = getpass.getuser()
REMOTE_PASSWORD = "pw"
REMOTE_CASE = f"kitchensync_02_url_normalization_{os.getpid()}"
REMOTE_BASE = f"/tmp/testks/{REMOTE_CASE}"
REMOTE_PEER = f"{REMOTE_BASE}/remote"
REMOTE_HOME_RELATIVE_PEER = f"/home/{REMOTE_LOGIN_USER}/tmp/testks/{REMOTE_CASE}/remote"


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
        if username == REMOTE_LOGIN_USER and password == REMOTE_PASSWORD:
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
        return f"[localhost]:{self.port} {self.host_key.get_name()} {self.host_key.get_base64()}\n"

    def url(self, path: str, host: str = "localhost") -> str:
        return f"sftp://{REMOTE_LOGIN_USER}:{REMOTE_PASSWORD}@{host}:{self.port}{path}"

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


def run(
    args: list[str],
    *,
    cwd: Path = PROJECT_DIR,
    env: dict[str, str] | None = None,
    timeout: float = 90.0,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=str(cwd),
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


def run_cli(
    *args: str,
    cwd: Path = PROJECT_DIR,
    home: Path | None = None,
    timeout: float = 90.0,
) -> subprocess.CompletedProcess[str]:
    env = None
    command = [str(JAVA), "-jar", str(JAR), *args]
    if home is not None:
        env = {**os.environ, "HOME": str(home), "USERPROFILE": str(home)}
        env.pop("SSH_AUTH_SOCK", None)
        env.pop("SSH_AGENT_PID", None)
        command = [str(JAVA), f"-Duser.home={home}", "-jar", str(JAR), *args]
    return run(command, cwd=cwd, env=env, timeout=timeout)


def reset_dir(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    path.mkdir(parents=True)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def output(result: subprocess.CompletedProcess[str]) -> str:
    return f"exit={result.returncode} stdout={result.stdout!r} stderr={result.stderr!r}"


def launcher_failed(result: subprocess.CompletedProcess[str]) -> bool:
    combined = result.stdout + result.stderr
    return "Unable to access jarfile" in combined or "Could not find or load main class" in combined


def expect_success(
    failures: list[str],
    req_ids: str,
    name: str,
    result: subprocess.CompletedProcess[str],
) -> None:
    if result.returncode != 0:
        failures.append(f"{req_ids} {name}: expected exit 0; {output(result)}")
    if launcher_failed(result):
        failures.append(f"{req_ids} {name}: Java launcher failed before KitchenSync ran; {output(result)}")


def expect_duplicate_peer_failure(
    failures: list[str],
    req_ids: str,
    name: str,
    result: subprocess.CompletedProcess[str],
) -> None:
    if result.returncode == 0:
        failures.append(
            f"{req_ids} {name}: expected nonzero exit because equivalent URL spellings must collapse "
            f"to one peer identity; {output(result)}"
        )
    if "unreachable" in result.stdout + result.stderr:
        failures.append(f"{req_ids} {name}: peer URL was unreachable instead of duplicated; {output(result)}")
    if launcher_failed(result):
        failures.append(f"{req_ids} {name}: Java launcher failed before KitchenSync ran; {output(result)}")


def reset_local() -> None:
    reset_dir(WORK_DIR)
    LOCAL_PEER.mkdir(parents=True)
    LOCAL_SINK.mkdir(parents=True)
    write_text(LOCAL_PEER / "from-peer.txt", "from normalized local peer\n")


def encoded_file_peer() -> str:
    return LOCAL_PEER.as_uri().replace("peer", "%70eer")


def check_local_identity_normalization(failures: list[str]) -> None:
    reset_local()

    result = run_cli(f"+peer//", f"{encoded_file_peer()}/?mc=5", cwd=LOCAL_CWD)

    expect_duplicate_peer_failure(
        failures,
        "02.12, 02.15, 02.16, 02.32, 02.33",
        "bare relative path and encoded file URL identify the same local peer",
        result,
    )


def check_bare_path_resolves_from_cwd(failures: list[str]) -> None:
    reset_local()

    result = run_cli("+peer", str(LOCAL_SINK), cwd=LOCAL_CWD)

    expect_success(failures, "02.12", "bare path resolves from process cwd", result)
    copied = LOCAL_SINK / "from-peer.txt"
    copied_text = copied.read_text(encoding="utf-8") if copied.exists() else None
    if copied_text != "from normalized local peer\n":
        failures.append(
            "02.12 bare path resolves from process cwd: expected sink to receive "
            f"{copied}, got {copied_text!r}"
        )


def check_sftp_identity_normalization(failures: list[str]) -> None:
    reset_local()
    remote_root = WORK_DIR / "sftp_identity_root"
    home = WORK_DIR / "sftp_identity_home"
    reset_dir(remote_root)
    write_known_hosts(home, None)

    with SFTPFixture(remote_root) as fixture:
        write_known_hosts(home, fixture)
        reset_remote(remote_root)
        variant = (
            f"SFTP://{REMOTE_LOGIN_USER}:{REMOTE_PASSWORD}@LOCALHOST:{fixture.port}"
            f"//tmp//testks//{REMOTE_CASE}//remo%74e/?mc=5&ct=5"
        )
        canonical = fixture.url(REMOTE_PEER)
        result = run_cli(f"+{variant}", canonical, home=home, timeout=60)

    expect_duplicate_peer_failure(
        failures,
        "02.13, 02.15, 02.16, 02.32, 02.33",
        "SFTP URL variants identify the same remote peer",
        result,
    )


def write_known_hosts(home: Path, fixture: SFTPFixture | None) -> None:
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(parents=True, exist_ok=True)
    known_hosts = ssh_dir / "known_hosts"
    known_hosts.write_text("" if fixture is None else fixture.known_hosts_line(), encoding="utf-8", newline="\n")
    known_hosts.chmod(0o600)


def reset_remote(root: Path) -> None:
    for path in (root / REMOTE_BASE.lstrip("/"), root / REMOTE_HOME_RELATIVE_PEER.lstrip("/")):
        if path.exists():
            shutil.rmtree(path)
    (root / REMOTE_PEER.lstrip("/")).mkdir(parents=True)


def check_sftp_absolute_path(failures: list[str]) -> None:
    reset_local()
    remote_root = WORK_DIR / "sftp_absolute_root"
    home = WORK_DIR / "sftp_absolute_home"
    reset_dir(remote_root)
    write_known_hosts(home, None)

    source = WORK_DIR / "remote_source"
    reset_dir(source)
    write_text(source / "absolute.txt", "absolute remote path payload\n")
    with SFTPFixture(remote_root) as fixture:
        write_known_hosts(home, fixture)
        reset_remote(remote_root)
        remote_url = fixture.url(REMOTE_PEER)
        result = run_cli(f"+{source}", remote_url, home=home, timeout=120)
    expect_success(failures, "02.46", "copy to SFTP absolute path", result)

    absolute_file = remote_root / REMOTE_PEER.lstrip("/") / "absolute.txt"
    if absolute_file.read_text(encoding="utf-8") != "absolute remote path payload\n":
        failures.append(f"02.46 absolute SFTP filesystem path contains copied file: missing {absolute_file}")

    home_relative_file = remote_root / REMOTE_HOME_RELATIVE_PEER.lstrip("/") / "absolute.txt"
    if home_relative_file.exists():
        failures.append("02.46 SFTP path was interpreted relative to the remote user's home directory")


def check_sftp_username_normalization(failures: list[str]) -> None:
    reset_local()
    remote_root = WORK_DIR / "sftp_username_root"
    home = WORK_DIR / "sftp_username_home"
    reset_dir(remote_root)
    write_known_hosts(home, None)

    with SFTPFixture(remote_root) as fixture:
        write_known_hosts(home, fixture)
        reset_remote(remote_root)
        # URL with explicit OS username
        with_user = fixture.url(REMOTE_PEER)
        # URL without username -- must normalize to include current OS user
        no_user = f"sftp://localhost:{fixture.port}{REMOTE_PEER}"
        result = run_cli(f"+{with_user}", no_user, home=home, timeout=60)

    expect_duplicate_peer_failure(
        failures,
        "02.17",
        "sftp URL without username normalizes to include OS user",
        result,
    )


def run_check(name: str, failures: list[str], check) -> None:
    try:
        check(failures)
        print(f"CHECK {name}")
    except subprocess.TimeoutExpired as exc:
        failures.append(f"{name}: command timed out: {exc}")
    except Exception as exc:
        failures.append(f"{name}: unexpected test error: {exc!r}")


def main() -> int:
    failures: list[str] = []

    # 02.14 requires comparing an omitted SFTP port with explicit :22. The local
    # fixture cannot bind port 22 without elevated privileges.

    checks = [
        ("local identity normalization", check_local_identity_normalization),
        ("bare path cwd resolution", check_bare_path_resolves_from_cwd),
        ("SFTP identity normalization", check_sftp_identity_normalization),
        ("SFTP absolute path semantics", check_sftp_absolute_path),
        ("SFTP username normalization", check_sftp_username_normalization),
    ]

    for name, check in checks:
        run_check(name, failures, check)

    if failures:
        print("FAIL tests/02_url-normalization.py")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS tests/02_url-normalization.py (02.12, 02.13, 02.15, 02.16, 02.17, 02.32, 02.33, 02.46; 02.14 not reasonably testable)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
