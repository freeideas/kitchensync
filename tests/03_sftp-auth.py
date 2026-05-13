#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["cryptography", "paramiko"]
# ///
"""SFTP authentication fallback chain and host key verification (03.65-03.70)."""

from __future__ import annotations

import base64
import errno
import io
import logging
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

import paramiko
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ed25519

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TMP = Path(PROJECT).resolve() / "tmp" / "testks" / "03_sftp-auth"
TEST_USER = "ksauth"
PASSWORD = "sftp_auth_test_x7k2m9"
BAD_PASSWORD = "wrong_sftp_auth_test_x7k2m9"
SPECIAL_PASSWORD = "pass@word"

logging.getLogger("paramiko").setLevel(logging.CRITICAL)


class _AuthLog:
    def __init__(self, key_labels: dict[str, str]) -> None:
        self._key_labels = key_labels
        self._events: list[str] = []
        self._lock = threading.Lock()

    def label_for(self, key: paramiko.PKey) -> str:
        return self._key_labels.get(key.get_base64(), f"unknown:{key.get_name()}")

    def append(self, event: str) -> None:
        with self._lock:
            self._events.append(event)

    def clear(self) -> None:
        with self._lock:
            self._events.clear()

    def events(self) -> list[str]:
        with self._lock:
            return list(self._events)


class _SSHServer(paramiko.ServerInterface):
    def __init__(
        self,
        auth_log: _AuthLog,
        accept_password: str | None = None,
        accept_keys: list[paramiko.PKey] | None = None,
        allow_password_attempts: bool = False,
    ) -> None:
        self._auth_log = auth_log
        self._accept_password = accept_password
        self._accept_keys = accept_keys or []
        self._allow_password_attempts = allow_password_attempts

    def check_channel_request(self, kind, chanid):
        if kind == "session":
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_auth_password(self, username, password):
        accepted = self._accept_password is not None and password == self._accept_password
        self._auth_log.append("password:ok" if accepted else "password:bad")
        return paramiko.AUTH_SUCCESSFUL if accepted else paramiko.AUTH_FAILED

    def check_auth_publickey(self, username, key):
        label = self._auth_log.label_for(key)
        self._auth_log.append(label)
        for accepted_key in self._accept_keys:
            if key == accepted_key:
                return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED

    def get_allowed_auths(self, username):
        methods = []
        if self._accept_password is not None or self._allow_password_attempts:
            methods.append("password")
        if self._accept_keys:
            methods.append("publickey")
        return ",".join(methods) or "none"


class _FilesystemHandle(paramiko.SFTPHandle):
    def stat(self):
        try:
            return paramiko.SFTPAttributes.from_stat(os.fstat(self.readfile.fileno()))
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def chattr(self, attr):
        try:
            paramiko.SFTPServer.set_file_attr(self.filename, attr)
            return paramiko.SFTP_OK
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)


class _FilesystemSFTP(paramiko.SFTPServerInterface):
    def __init__(self, server, root: Path) -> None:
        super().__init__(server)
        self._root = root.resolve()

    def _to_local(self, path: str) -> Path:
        local = Path(path).resolve()
        try:
            local.relative_to(self._root)
        except ValueError:
            raise OSError(errno.EACCES, "path outside test root")
        return local

    def list_folder(self, path):
        try:
            local = self._to_local(path)
            entries = []
            for child in local.iterdir():
                attrs = paramiko.SFTPAttributes.from_stat(child.stat())
                attrs.filename = child.name
                entries.append(attrs)
            return entries
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def stat(self, path):
        try:
            return paramiko.SFTPAttributes.from_stat(self._to_local(path).stat())
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def lstat(self, path):
        try:
            return paramiko.SFTPAttributes.from_stat(self._to_local(path).lstat())
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def open(self, path, flags, attr):
        try:
            local = self._to_local(path)
            mode = getattr(attr, "st_mode", None) or 0o666
            fd = os.open(local, flags, mode)
            if flags & os.O_RDWR:
                file_obj = os.fdopen(fd, "r+b")
            elif flags & os.O_WRONLY:
                file_obj = os.fdopen(fd, "wb")
            else:
                file_obj = os.fdopen(fd, "rb")
            handle = _FilesystemHandle(flags)
            handle.filename = str(local)
            handle.readfile = file_obj
            handle.writefile = file_obj
            return handle
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def remove(self, path):
        try:
            self._to_local(path).unlink()
            return paramiko.SFTP_OK
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def mkdir(self, path, attr):
        try:
            self._to_local(path).mkdir()
            return paramiko.SFTP_OK
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def rmdir(self, path):
        try:
            self._to_local(path).rmdir()
            return paramiko.SFTP_OK
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def rename(self, oldpath, newpath):
        try:
            os.replace(self._to_local(oldpath), self._to_local(newpath))
            return paramiko.SFTP_OK
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def setstat(self, path, attr):
        try:
            paramiko.SFTPServer.set_file_attr(str(self._to_local(path)), attr)
            return paramiko.SFTP_OK
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)


class _SFTPServer:
    def __init__(
        self,
        host_key: paramiko.PKey,
        key_labels: dict[str, str],
        root: Path,
        *,
        accept_password: str | None = None,
        accept_keys: list[paramiko.PKey] | None = None,
        allow_password_attempts: bool = False,
    ) -> None:
        self._host_key = host_key
        self._root = root
        self._accept_password = accept_password
        self._accept_keys = accept_keys or []
        self._allow_password_attempts = allow_password_attempts
        self.auth_log = _AuthLog(key_labels)
        self._closed = False
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind(("127.0.0.1", 0))
        self._sock.listen(16)
        self.port = int(self._sock.getsockname()[1])

    def start(self) -> None:
        threading.Thread(target=self._accept_loop, daemon=True).start()

    def close(self) -> None:
        self._closed = True
        try:
            self._sock.close()
        except OSError:
            pass

    def _accept_loop(self) -> None:
        while not self._closed:
            try:
                conn, _ = self._sock.accept()
            except OSError:
                return
            threading.Thread(target=self._serve_conn, args=(conn,), daemon=True).start()

    def _serve_conn(self, conn: socket.socket) -> None:
        transport = paramiko.Transport(conn)
        transport.add_server_key(self._host_key)
        transport.set_subsystem_handler("sftp", paramiko.SFTPServer, _FilesystemSFTP, root=self._root)
        try:
            transport.start_server(
                server=_SSHServer(
                    self.auth_log,
                    accept_password=self._accept_password,
                    accept_keys=self._accept_keys,
                    allow_password_attempts=self._allow_password_attempts,
                )
            )
            while transport.is_active() and not self._closed:
                time.sleep(0.05)
        except Exception:
            pass
        finally:
            transport.close()


def _known_hosts_line(host_key: paramiko.PKey, port: int) -> str:
    key_b64 = base64.b64encode(host_key.asbytes()).decode("ascii")
    return f"[127.0.0.1]:{port} {host_key.get_name()} {key_b64}"


def _write_known_hosts(home: Path, host_key: paramiko.PKey, servers: list[_SFTPServer]) -> None:
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    known_hosts = ssh_dir / "known_hosts"
    known_hosts.write_text(
        "".join(_known_hosts_line(host_key, server.port) + "\n" for server in servers),
        encoding="utf-8",
        newline="\n",
    )
    known_hosts.chmod(0o600)


def _generate_ed25519_key() -> tuple[paramiko.Ed25519Key, str]:
    key = ed25519.Ed25519PrivateKey.generate()
    private_text = key.private_bytes(
        serialization.Encoding.PEM,
        serialization.PrivateFormat.OpenSSH,
        serialization.NoEncryption(),
    ).decode("utf-8")
    return paramiko.Ed25519Key.from_private_key(io.StringIO(private_text)), private_text


def _write_key(key: paramiko.PKey, path: Path, private_text: str | None = None) -> None:
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    if private_text is None:
        key.write_private_key_file(str(path))
    else:
        path.write_text(private_text, encoding="utf-8", newline="\n")
    path.chmod(0o600)


def _start_ssh_agent() -> tuple[str, int]:
    out = subprocess.check_output(["ssh-agent", "-s"], text=True, encoding="utf-8")
    sock_val, pid_val = None, None
    for line in out.splitlines():
        if line.startswith("SSH_AUTH_SOCK="):
            sock_val = line.split("=", 1)[1].split(";")[0]
        elif line.startswith("SSH_AGENT_PID="):
            pid_val = int(line.split("=", 1)[1].split(";")[0])
    if not sock_val or not pid_val:
        raise RuntimeError("could not parse ssh-agent output")
    return sock_val, pid_val


def _stop_ssh_agent(auth_sock: str | None, agent_pid: int | None) -> None:
    if auth_sock is None or agent_pid is None:
        return
    env = {**os.environ, "SSH_AUTH_SOCK": auth_sock, "SSH_AGENT_PID": str(agent_pid)}
    subprocess.run(
        ["ssh-agent", "-k"],
        check=False,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        env=env,
    )


def _add_key_to_agent(private_text: str, auth_sock: str) -> None:
    with tempfile.NamedTemporaryFile(suffix=".key", delete=False) as file:
        key_path = Path(file.name)
    try:
        key_path.write_text(private_text, encoding="utf-8", newline="\n")
        key_path.chmod(0o600)
        subprocess.check_call(
            ["ssh-add", str(key_path)],
            env={**os.environ, "SSH_AUTH_SOCK": auth_sock},
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
    finally:
        key_path.unlink(missing_ok=True)


def _compressed(events: list[str]) -> list[str]:
    result: list[str] = []
    for event in events:
        if not result or result[-1] != event:
            result.append(event)
    return result


def _repeats_sequence(events: list[str], expected: list[str]) -> bool:
    if not expected or len(events) % len(expected) != 0:
        return False
    return all(events[i:i + len(expected)] == expected for i in range(0, len(events), len(expected)))


def _env(home: Path, auth_sock: str | None = None) -> dict[str, str]:
    env = dict(os.environ)
    env["HOME"] = str(home)
    prior = env.get("JAVA_TOOL_OPTIONS", "")
    env["JAVA_TOOL_OPTIONS"] = (prior + " " if prior else "") + f"-Duser.home={home}"
    if auth_sock is None:
        env.pop("SSH_AUTH_SOCK", None)
    else:
        env["SSH_AUTH_SOCK"] = auth_sock
    return env


def invoke(*args, timeout=25, env=None):
    return subprocess.run(
        [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT, *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=timeout,
        env=env,
    )


def _sync_assertion(
    failures: list[str],
    req_id: str,
    server: _SFTPServer,
    home: Path,
    src: Path,
    dst: Path,
    expected_events: list[str],
    *,
    password: str | None = None,
    auth_sock: str | None = None,
) -> None:
    src.mkdir(parents=True)
    (src / f"file{req_id}.txt").write_text(f"content{req_id}", encoding="utf-8", newline="\n")
    server.auth_log.clear()
    userinfo = TEST_USER + (f":{password}" if password is not None else "")
    proc = invoke(
        "+" + src.as_uri(),
        f"sftp://{userinfo}@127.0.0.1:{server.port}{dst}",
        env=_env(home, auth_sock),
    )
    events = _compressed(server.auth_log.events())
    print(f"[{req_id}] exit={proc.returncode}, auth={events}")
    if proc.returncode != 0:
        failures.append(
            f"{req_id}: expected exit 0, got {proc.returncode}\n"
            f"  stdout: {proc.stdout!r}\n  stderr: {proc.stderr!r}\n  auth: {events!r}"
        )
    elif not (dst / f"file{req_id}.txt").exists():
        failures.append(f"{req_id}: synced file missing at destination")
    elif not _repeats_sequence(events, expected_events):
        failures.append(f"{req_id}: expected auth sequence {expected_events!r}, got {events!r}")


def main() -> int:
    if TMP.exists():
        shutil.rmtree(TMP)
    TMP.mkdir(parents=True)

    failures: list[str] = []
    servers: list[_SFTPServer] = []
    agent_sock: str | None = None
    agent_pid: int | None = None
    temp_home_root = Path(tempfile.mkdtemp(prefix="ks_sftp_auth_home_"))

    try:
        host_key = paramiko.RSAKey.generate(bits=2048)
        ed25519_key, ed25519_private = _generate_ed25519_key()
        ecdsa_key = paramiko.ECDSAKey.generate()
        rsa_key = paramiko.RSAKey.generate(bits=2048)
        agent_key, agent_private = _generate_ed25519_key()
        key_labels = {
            ed25519_key.get_base64(): "id_ed25519",
            ecdsa_key.get_base64(): "id_ecdsa",
            rsa_key.get_base64(): "id_rsa",
            agent_key.get_base64(): "agent",
        }

        password_server = _SFTPServer(
            host_key,
            key_labels,
            TMP,
            accept_password=PASSWORD,
            accept_keys=[ed25519_key],
        )
        agent_server = _SFTPServer(
            host_key,
            key_labels,
            TMP,
            accept_keys=[agent_key, ed25519_key, ecdsa_key, rsa_key],
            allow_password_attempts=True,
        )
        identity_server = _SFTPServer(host_key, key_labels, TMP, accept_keys=[rsa_key])
        known_host_server = _SFTPServer(host_key, key_labels, TMP, accept_password=PASSWORD)
        unknown_host_server = _SFTPServer(host_key, key_labels, TMP, accept_password=PASSWORD)
        percent_server = _SFTPServer(host_key, key_labels, TMP, accept_password=SPECIAL_PASSWORD)
        servers.extend([
            password_server,
            agent_server,
            identity_server,
            known_host_server,
            unknown_host_server,
            percent_server,
        ])
        for server in servers:
            server.start()

        password_home = temp_home_root / "password"
        _write_known_hosts(password_home, host_key, [password_server])
        _write_key(ed25519_key, password_home / ".ssh" / "id_ed25519", ed25519_private)

        agent_home = temp_home_root / "agent"
        _write_known_hosts(agent_home, host_key, [agent_server])
        _write_key(ed25519_key, agent_home / ".ssh" / "id_ed25519", ed25519_private)
        _write_key(ecdsa_key, agent_home / ".ssh" / "id_ecdsa")
        _write_key(rsa_key, agent_home / ".ssh" / "id_rsa")
        try:
            agent_sock, agent_pid = _start_ssh_agent()
            _add_key_to_agent(agent_private, agent_sock)
        except FileNotFoundError:
            failures.append("03.66: ssh-agent and ssh-add are required to exercise SSH_AUTH_SOCK auth")
            agent_sock = None
        except subprocess.CalledProcessError as exc:
            failures.append(f"03.66: ssh-add failed ({exc}); could not load key into agent")
            agent_sock = None

        identity_home = temp_home_root / "identity"
        _write_known_hosts(identity_home, host_key, [identity_server])
        _write_key(ed25519_key, identity_home / ".ssh" / "id_ed25519", ed25519_private)
        _write_key(ecdsa_key, identity_home / ".ssh" / "id_ecdsa")
        _write_key(rsa_key, identity_home / ".ssh" / "id_rsa")

        known_home = temp_home_root / "known"
        _write_known_hosts(known_home, host_key, [known_host_server])

        unknown_home = temp_home_root / "unknown"
        _write_known_hosts(unknown_home, host_key, [])

        percent_home = temp_home_root / "percent"
        _write_known_hosts(percent_home, host_key, [percent_server])

        _sync_assertion(
            failures,
            "03.65",
            password_server,
            password_home,
            TMP / "03.65_src",
            TMP / "03.65_dst",
            ["password:ok"],
            password=PASSWORD,
        )

        if agent_sock is not None:
            _sync_assertion(
                failures,
                "03.66-no-inline",
                agent_server,
                agent_home,
                TMP / "03.66_no_inline_src",
                TMP / "03.66_no_inline_dst",
                ["agent"],
                auth_sock=agent_sock,
            )
            _sync_assertion(
                failures,
                "03.66-after-bad-password",
                agent_server,
                agent_home,
                TMP / "03.66_after_bad_password_src",
                TMP / "03.66_after_bad_password_dst",
                ["password:bad", "agent"],
                password=BAD_PASSWORD,
                auth_sock=agent_sock,
            )

        _sync_assertion(
            failures,
            "03.67",
            identity_server,
            identity_home,
            TMP / "03.67_src",
            TMP / "03.67_dst",
            ["id_ed25519", "id_ecdsa", "id_rsa"],
        )

        _sync_assertion(
            failures,
            "03.68",
            known_host_server,
            known_home,
            TMP / "03.68_src",
            TMP / "03.68_dst",
            ["password:ok"],
            password=PASSWORD,
        )

        src_69 = TMP / "03.69_src"
        dst_69 = TMP / "03.69_dst"
        src_69.mkdir(parents=True)
        (src_69 / "file03.69.txt").write_text("content03.69", encoding="utf-8", newline="\n")
        unknown_host_server.auth_log.clear()
        proc = invoke(
            "+" + src_69.as_uri(),
            f"sftp://{TEST_USER}:{PASSWORD}@127.0.0.1:{unknown_host_server.port}{dst_69}",
            env=_env(unknown_home),
        )
        events = _compressed(unknown_host_server.auth_log.events())
        print(f"[03.69] exit={proc.returncode}, auth={events}")
        if proc.returncode == 0:
            failures.append("03.69: expected non-zero exit for host key absent from known_hosts")
        elif events:
            failures.append(f"03.69: host-key rejection should happen before auth, got {events!r}")

        _sync_assertion(
            failures,
            "03.70",
            percent_server,
            percent_home,
            TMP / "03.70_src",
            TMP / "03.70_dst",
            ["password:ok"],
            password="pass%40word",
        )

    finally:
        _stop_ssh_agent(agent_sock, agent_pid)
        for server in servers:
            server.close()
        shutil.rmtree(temp_home_root, ignore_errors=True)
        shutil.rmtree(TMP, ignore_errors=True)

    if failures:
        print("\nFAILURES:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
