#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko==3.5.1", "pywin32==310; sys_platform == 'win32'"]
# ///

from __future__ import annotations

import errno
import os
import shutil
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path

import paramiko
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ed25519
from paramiko import SFTPAttributes, SFTPHandle, SFTPServerInterface
from paramiko.sftp import SFTP_FAILURE, SFTP_NO_SUCH_FILE, SFTP_OK


PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
WORK_DIR = PROJECT_DIR / "tests" / ".tmp" / "03_sftp_auth"
USER = "ace"
PASSWORD = "p@ss:word"
WRONG_PASSWORD = "wrong-password"


@dataclass(frozen=True)
class AuthAttempt:
    method: str
    accepted: bool
    value: str


class AuthLog:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._attempts: list[AuthAttempt] = []

    def add(self, method: str, accepted: bool, value: str) -> None:
        with self._lock:
            self._attempts.append(AuthAttempt(method, accepted, value))

    def snapshot(self) -> list[AuthAttempt]:
        with self._lock:
            return list(self._attempts)


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
        relative = path.lstrip("/")
        resolved = (self.root / relative).resolve()
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

    def utime(self, path: str, times):
        try:
            os.utime(self._local(path), times)
            return SFTP_OK
        except OSError as exc:
            return errno_to_sftp(exc.errno)


class AuthServer(paramiko.ServerInterface):
    def __init__(self, log: AuthLog, accepted_password: str | None, accepted_keys: set[str]):
        self.log = log
        self.accepted_password = accepted_password
        self.accepted_keys = accepted_keys

    def check_auth_password(self, username: str, password: str) -> int:
        accepted = username == USER and password == self.accepted_password
        self.log.add("password", accepted, password)
        return paramiko.AUTH_SUCCESSFUL if accepted else paramiko.AUTH_FAILED

    def check_auth_publickey(self, username: str, key: paramiko.PKey) -> int:
        value = key.get_base64()
        accepted = username == USER and value in self.accepted_keys
        self.log.add("publickey", accepted, value)
        return paramiko.AUTH_SUCCESSFUL if accepted else paramiko.AUTH_FAILED

    def get_allowed_auths(self, username: str) -> str:
        return "password,publickey"

    def check_channel_request(self, kind: str, chanid: int) -> int:
        if kind == "session":
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED


class SFTPFixture:
    def __init__(self, root: Path, accepted_password: str | None, accepted_keys: set[str]):
        self.root = root
        self.accepted_password = accepted_password
        self.accepted_keys = accepted_keys
        self.host_key = paramiko.RSAKey.generate(2048)
        self.log = AuthLog()
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

    def __exit__(self, exc_type, exc, tb) -> None:
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

    def url(self, password: str | None = None) -> str:
        if password is None:
            return f"sftp://{USER}@127.0.0.1:{self.port}/"
        return f"sftp://{USER}:{password}@127.0.0.1:{self.port}/"

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
            transport.start_server(server=AuthServer(self.log, self.accepted_password, self.accepted_keys))
            while not self._stop.is_set() and transport.is_active():
                time.sleep(0.05)
        except Exception:
            pass
        finally:
            transport.close()


class SshAgentFixture:
    def __init__(self, sock_path: Path, key: paramiko.PKey):
        self.sock_path = sock_path
        self.key = key
        self.key_blob = key.asbytes()
        self.env: dict[str, str] = {}
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._sock: socket.socket | None = None
        self._pipe_name: str | None = None

    def __enter__(self) -> SshAgentFixture:
        if hasattr(socket, "AF_UNIX"):
            return self._enter_unix_socket()
        if sys.platform == "win32":
            return self._enter_windows_pipe()
        raise RuntimeError("this Python runtime cannot create an SSH_AUTH_SOCK agent endpoint")

    def _enter_unix_socket(self) -> SshAgentFixture:
        if self.sock_path.exists():
            self.sock_path.unlink()
        self.sock_path.parent.mkdir(parents=True, exist_ok=True)
        self._sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self._sock.bind(str(self.sock_path))
        self._sock.listen(20)
        self._thread = threading.Thread(target=self._accept_loop, daemon=True)
        self._thread.start()
        self.env = {"SSH_AUTH_SOCK": str(self.sock_path)}
        return self

    def _enter_windows_pipe(self) -> SshAgentFixture:
        self._pipe_name = rf"\\.\pipe\kitchensync-test-agent-{os.getpid()}-{time.time_ns()}"
        self._thread = threading.Thread(target=self._accept_pipe_loop, daemon=True)
        self._thread.start()
        self.env = {"SSH_AUTH_SOCK": self._pipe_name}
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self._stop.set()
        if self._sock is not None:
            try:
                self._sock.close()
            except OSError:
                pass
        if self._pipe_name is not None:
            self._unblock_pipe_accept()
        if self._thread is not None:
            self._thread.join(timeout=2)
        try:
            self.sock_path.unlink()
        except OSError:
            pass

    def _accept_loop(self) -> None:
        assert self._sock is not None
        while not self._stop.is_set():
            try:
                client, _addr = self._sock.accept()
            except OSError:
                break
            threading.Thread(target=self._serve_client, args=(client,), daemon=True).start()

    def _accept_pipe_loop(self) -> None:
        import pywintypes
        import win32con
        import win32file
        import win32pipe

        assert self._pipe_name is not None
        while not self._stop.is_set():
            pipe = win32pipe.CreateNamedPipe(
                self._pipe_name,
                win32con.PIPE_ACCESS_DUPLEX,
                win32con.PIPE_TYPE_BYTE | win32con.PIPE_READMODE_BYTE | win32con.PIPE_WAIT,
                20,
                65536,
                65536,
                0,
                None,
            )
            try:
                win32pipe.ConnectNamedPipe(pipe, None)
            except pywintypes.error as exc:
                if exc.winerror != 535:
                    win32file.CloseHandle(pipe)
                    if self._stop.is_set():
                        return
                    continue
            if self._stop.is_set():
                win32file.CloseHandle(pipe)
                return
            threading.Thread(target=self._serve_pipe, args=(pipe,), daemon=True).start()

    def _unblock_pipe_accept(self) -> None:
        import pywintypes
        import win32con
        import win32file

        assert self._pipe_name is not None
        try:
            handle = win32file.CreateFile(
                self._pipe_name,
                win32con.GENERIC_READ | win32con.GENERIC_WRITE,
                0,
                None,
                win32con.OPEN_EXISTING,
                0,
                None,
            )
            win32file.CloseHandle(handle)
        except pywintypes.error:
            pass

    def _serve_client(self, client: socket.socket) -> None:
        with client:
            while not self._stop.is_set():
                header = self._recv_exact(client, 4)
                if not header:
                    return
                length = struct.unpack(">I", header)[0]
                body = self._recv_exact(client, length)
                if len(body) != length:
                    return
                response = self._handle_message(body)
                client.sendall(struct.pack(">I", len(response)) + response)

    def _serve_pipe(self, pipe) -> None:
        import pywintypes
        import win32file
        import win32pipe

        try:
            while not self._stop.is_set():
                header = self._read_pipe_exact(pipe, 4)
                if not header:
                    return
                length = struct.unpack(">I", header)[0]
                body = self._read_pipe_exact(pipe, length)
                if len(body) != length:
                    return
                response = self._handle_message(body)
                win32file.WriteFile(pipe, struct.pack(">I", len(response)) + response)
        except pywintypes.error:
            pass
        finally:
            try:
                win32pipe.DisconnectNamedPipe(pipe)
            except pywintypes.error:
                pass
            win32file.CloseHandle(pipe)

    def _handle_message(self, body: bytes) -> bytes:
        message_type = body[0]
        if message_type == 11:
            return b"\x0c" + struct.pack(">I", 1) + self._string(self.key_blob) + self._string(b"kitchensync-test-agent")
        if message_type == 13:
            key_blob, offset = self._read_string(body, 1)
            data, offset = self._read_string(body, offset)
            flags = struct.unpack(">I", body[offset : offset + 4])[0]
            if key_blob != self.key_blob:
                return b"\x05"
            algorithm = None
            if flags & 4:
                algorithm = "rsa-sha2-512"
            elif flags & 2:
                algorithm = "rsa-sha2-256"
            signature = self.key.sign_ssh_data(data, algorithm=algorithm).asbytes()
            return b"\x0e" + self._string(signature)
        return b"\x05"

    @staticmethod
    def _recv_exact(client: socket.socket, length: int) -> bytes:
        data = b""
        while len(data) < length:
            chunk = client.recv(length - len(data))
            if not chunk:
                break
            data += chunk
        return data

    @staticmethod
    def _read_pipe_exact(pipe, length: int) -> bytes:
        import pywintypes
        import win32file

        data = b""
        while len(data) < length:
            try:
                _err, chunk = win32file.ReadFile(pipe, length - len(data), None)
            except pywintypes.error:
                break
            if not chunk:
                break
            data += chunk
        return data

    @staticmethod
    def _string(value: bytes) -> bytes:
        return struct.pack(">I", len(value)) + value

    @staticmethod
    def _read_string(body: bytes, offset: int) -> tuple[bytes, int]:
        length = struct.unpack(">I", body[offset : offset + 4])[0]
        offset += 4
        return body[offset : offset + length], offset + length


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def write_key(path: Path, key: paramiko.PKey) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    key.write_private_key_file(str(path))
    path.chmod(0o600)


def write_ed25519_key(path: Path) -> paramiko.Ed25519Key:
    path.parent.mkdir(parents=True, exist_ok=True)
    key = ed25519.Ed25519PrivateKey.generate()
    path.write_bytes(
        key.private_bytes(
            encoding=serialization.Encoding.PEM,
            format=serialization.PrivateFormat.OpenSSH,
            encryption_algorithm=serialization.NoEncryption(),
        )
    )
    path.chmod(0o600)
    return paramiko.Ed25519Key.from_private_key_file(str(path))


def prepare_source(path: Path) -> None:
    if path.exists():
        shutil.rmtree(path)
    write_text(path / "payload.txt", "sftp auth payload\n")


def write_known_hosts(home: Path, fixture: SFTPFixture | None) -> None:
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(parents=True, exist_ok=True)
    known_hosts = ssh_dir / "known_hosts"
    known_hosts.write_text("" if fixture is None else fixture.known_hosts_line(), encoding="utf-8", newline="\n")
    known_hosts.chmod(0o600)


def run_cli(
    home: Path,
    source: Path,
    url: str,
    extra_env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    env = {
        **os.environ,
        "HOME": str(home),
        "USERPROFILE": str(home),
    }
    if extra_env is None:
        env.pop("SSH_AUTH_SOCK", None)
        env.pop("SSH_AGENT_PID", None)
    else:
        env.update(extra_env)
    return subprocess.run(
        [str(JAVA), f"-Duser.home={home}", "-jar", str(JAR), f"+{source}", url],
        cwd=str(PROJECT_DIR),
        env=env,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )


def payload_synced(remote_root: Path) -> bool:
    payload = remote_root / "payload.txt"
    return payload.exists() and payload.read_text(encoding="utf-8") == "sftp auth payload\n"


def attempt_summary(attempts: list[AuthAttempt]) -> str:
    return ", ".join(f"{a.method}:{'ok' if a.accepted else 'fail'}:{a.value[:16]}" for a in attempts)


def require_success(failures: list[str], req_ids: str, result: subprocess.CompletedProcess[str]) -> None:
    if result.returncode != 0:
        failures.append(
            f"{req_ids}: expected sync to succeed, got exit {result.returncode}; "
            f"stdout={result.stdout!r} stderr={result.stderr!r}"
        )


def check_inline_password_and_decoding(root: Path) -> list[str]:
    failures: list[str] = []
    scenario = root / "inline_password"
    source = scenario / "source"
    remote = scenario / "remote"
    home = scenario / "home"
    prepare_source(source)

    with SFTPFixture(remote, PASSWORD, set()) as fixture:
        write_known_hosts(home, fixture)
        result = run_cli(home, source, fixture.url("p%40ss%3Aword"))
        attempts = fixture.log.snapshot()

    require_success(failures, "03.65/03.68/03.70", result)
    if not payload_synced(remote):
        failures.append("03.65/03.70: inline password authentication did not transfer payload")
    if not attempts:
        failures.append("03.65: expected at least one password authentication attempt")
    elif attempts[0] != AuthAttempt("password", True, PASSWORD):
        failures.append(
            "03.65/03.70: expected decoded inline password to be tried first and accepted; "
            f"attempts={attempt_summary(attempts)}"
        )
    if any(attempt.method == "publickey" for attempt in attempts):
        failures.append(
            "03.65: expected successful inline password to stop fallback before public key auth; "
            f"attempts={attempt_summary(attempts)}"
        )
    return failures


def check_ssh_agent_after_no_password_or_failed_password(root: Path) -> list[str]:
    failures: list[str] = []
    scenario = root / "ssh_agent"
    source = scenario / "source"
    remote = scenario / "remote"
    home = scenario / "home"
    agent_key = paramiko.RSAKey.generate(2048)
    prepare_source(source)

    with SFTPFixture(remote, None, {agent_key.get_base64()}) as fixture:
        write_known_hosts(home, fixture)
        with SshAgentFixture(scenario / "agent.sock", agent_key) as agent:
            result = run_cli(home, source, fixture.url(), agent.env)
        attempts = fixture.log.snapshot()

    require_success(failures, "03.66", result)
    if not payload_synced(remote):
        failures.append("03.66: SSH agent authentication without an inline password did not transfer payload")
    if not attempts:
        failures.append("03.66: expected SSH agent authentication attempt when no inline password is present")
    elif attempts[0].method != "publickey" or not attempts[0].accepted or attempts[0].value != agent_key.get_base64():
        failures.append(
            "03.66: expected SSH agent public key to be accepted when no inline password is present; "
            f"attempts={attempt_summary(attempts)}"
        )

    source = scenario / "source_after_password_failure"
    remote = scenario / "remote_after_password_failure"
    home = scenario / "home_after_password_failure"
    prepare_source(source)

    with SFTPFixture(remote, None, {agent_key.get_base64()}) as fixture:
        write_known_hosts(home, fixture)
        with SshAgentFixture(scenario / "agent_after_password_failure.sock", agent_key) as agent:
            result = run_cli(home, source, fixture.url(WRONG_PASSWORD), agent.env)
        attempts = fixture.log.snapshot()

    require_success(failures, "03.66", result)
    if not payload_synced(remote):
        failures.append("03.66: SSH agent fallback did not transfer payload")
    if len(attempts) < 2:
        failures.append(f"03.66: expected password failure followed by agent public key; attempts={attempt_summary(attempts)}")
    else:
        if attempts[0] != AuthAttempt("password", False, WRONG_PASSWORD):
            failures.append(
                "03.66: expected failed inline password before agent auth; "
                f"attempts={attempt_summary(attempts)}"
            )
        accepted_public = [i for i, attempt in enumerate(attempts) if attempt.method == "publickey" and attempt.accepted]
        if not accepted_public:
            failures.append(f"03.66: expected SSH agent public key to be accepted; attempts={attempt_summary(attempts)}")
        elif attempts[accepted_public[0]].value != agent_key.get_base64():
            failures.append(f"03.66: accepted public key was not the key loaded in SSH agent; attempts={attempt_summary(attempts)}")
    return failures


def check_identity_file_order(root: Path) -> list[str]:
    failures: list[str] = []
    scenario = root / "identity_file_order"
    source = scenario / "source"
    remote = scenario / "remote"
    home = scenario / "home"
    ssh_dir = home / ".ssh"
    prepare_source(source)

    ed25519_file_key = write_ed25519_key(ssh_dir / "id_ed25519")
    ecdsa_file_key = paramiko.ECDSAKey.generate(bits=256)
    rsa_file_key = paramiko.RSAKey.generate(2048)
    write_key(ssh_dir / "id_ecdsa", ecdsa_file_key)
    write_key(ssh_dir / "id_rsa", rsa_file_key)

    with SFTPFixture(remote, None, {rsa_file_key.get_base64()}) as fixture:
        write_known_hosts(home, fixture)
        result = run_cli(home, source, fixture.url(), {"SSH_AUTH_SOCK": str(scenario / "missing-agent.sock")})
        attempts = fixture.log.snapshot()

    require_success(failures, "03.67", result)
    if not payload_synced(remote):
        failures.append("03.67: identity file fallback did not transfer payload")

    public_keys = [attempt.value for attempt in attempts if attempt.method == "publickey"]
    expected = [
        ed25519_file_key.get_base64(),
        ecdsa_file_key.get_base64(),
        rsa_file_key.get_base64(),
    ]
    cursor = 0
    for key in public_keys:
        if cursor < len(expected) and key == expected[cursor]:
            cursor += 1
    if cursor != len(expected):
        failures.append(
            "03.67: expected identity files to be tried in id_ed25519, id_ecdsa, id_rsa order; "
            f"attempts={attempt_summary(attempts)}"
        )
    if not any(attempt.accepted and attempt.value == rsa_file_key.get_base64() for attempt in attempts):
        failures.append(f"03.67: expected id_rsa to be the accepted fallback identity; attempts={attempt_summary(attempts)}")

    source = scenario / "source_after_agent_failure"
    remote = scenario / "remote_after_agent_failure"
    home = scenario / "home_after_agent_failure"
    ssh_dir = home / ".ssh"
    rejected_agent_key = paramiko.RSAKey.generate(2048)
    prepare_source(source)

    ed25519_file_key = write_ed25519_key(ssh_dir / "id_ed25519")
    ecdsa_file_key = paramiko.ECDSAKey.generate(bits=256)
    rsa_file_key = paramiko.RSAKey.generate(2048)
    write_key(ssh_dir / "id_ecdsa", ecdsa_file_key)
    write_key(ssh_dir / "id_rsa", rsa_file_key)

    with SFTPFixture(remote, None, {rsa_file_key.get_base64()}) as fixture:
        write_known_hosts(home, fixture)
        with SshAgentFixture(scenario / "rejected-agent.sock", rejected_agent_key) as agent:
            result = run_cli(home, source, fixture.url(), agent.env)
        attempts = fixture.log.snapshot()

    require_success(failures, "03.67", result)
    if not payload_synced(remote):
        failures.append("03.67: identity file fallback after failed agent authentication did not transfer payload")
    rejected_agent_indexes = [
        i
        for i, attempt in enumerate(attempts)
        if attempt.method == "publickey" and not attempt.accepted and attempt.value == rejected_agent_key.get_base64()
    ]
    accepted_identity_indexes = [
        i for i, attempt in enumerate(attempts) if attempt.accepted and attempt.value == rsa_file_key.get_base64()
    ]
    if not rejected_agent_indexes:
        failures.append(f"03.67: expected rejected SSH agent key before identity fallback; attempts={attempt_summary(attempts)}")
    elif not accepted_identity_indexes or rejected_agent_indexes[0] > accepted_identity_indexes[0]:
        failures.append(f"03.67: expected rejected SSH agent key before accepted identity file; attempts={attempt_summary(attempts)}")
    if not accepted_identity_indexes:
        failures.append(f"03.67: expected identity file to be accepted after failed agent authentication; attempts={attempt_summary(attempts)}")

    public_keys = [attempt.value for attempt in attempts if attempt.method == "publickey"]
    expected = [
        ed25519_file_key.get_base64(),
        ecdsa_file_key.get_base64(),
        rsa_file_key.get_base64(),
    ]
    cursor = 0
    for key in public_keys:
        if cursor < len(expected) and key == expected[cursor]:
            cursor += 1
    if cursor != len(expected):
        failures.append(
            "03.67: expected identity files to be tried in order after failed agent authentication; "
            f"attempts={attempt_summary(attempts)}"
        )
    return failures


def check_unknown_host_rejected(root: Path) -> list[str]:
    failures: list[str] = []
    scenario = root / "unknown_host"
    source = scenario / "source"
    remote = scenario / "remote"
    home = scenario / "home"
    prepare_source(source)

    with SFTPFixture(remote, PASSWORD, set()) as fixture:
        write_known_hosts(home, None)
        result = run_cli(home, source, fixture.url("p%40ss%3Aword"))
        attempts = fixture.log.snapshot()

    if result.returncode == 0:
        failures.append("03.69: expected unknown SFTP host key to make the sync fail, but exit code was 0")
    if payload_synced(remote):
        failures.append("03.69: payload transferred even though host key was absent from known_hosts")
    if attempts:
        failures.append(
            "03.68/03.69: expected host key rejection before authentication; "
            f"attempts={attempt_summary(attempts)}"
        )
    return failures


def main() -> int:
    if WORK_DIR.exists():
        shutil.rmtree(WORK_DIR)
    WORK_DIR.mkdir(parents=True, exist_ok=True)
    tempfile.tempdir = str(WORK_DIR / "tmp")
    Path(tempfile.gettempdir()).mkdir(parents=True, exist_ok=True)

    if not JAR.is_file():
        print("FAIL tests/03_sftp-auth.py")
        print(f"- released product artifact is missing: {JAR}")
        return 1

    checks = [
        ("inline password and percent decoding", check_inline_password_and_decoding),
        ("SSH agent after no password or failed password", check_ssh_agent_after_no_password_or_failed_password),
        ("identity file order", check_identity_file_order),
        ("unknown host rejection", check_unknown_host_rejected),
    ]

    failures: list[str] = []
    for name, check in checks:
        try:
            check_failures = check(WORK_DIR)
            if check_failures:
                failures.extend(check_failures)
            else:
                print(f"PASS {name}")
        except subprocess.TimeoutExpired as exc:
            failures.append(f"{name}: command timed out: {exc}")
        except Exception as exc:
            failures.append(f"{name}: unexpected test error: {exc!r}")

    if failures:
        print("FAIL tests/03_sftp-auth.py")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("PASS tests/03_sftp-auth.py")
    return 0


if __name__ == "__main__":
    sys.exit(main())
