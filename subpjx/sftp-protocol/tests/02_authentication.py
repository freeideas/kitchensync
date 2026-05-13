#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["cryptography", "paramiko"]
# ///
"""Exercises SSH authentication method order."""

from __future__ import annotations

import base64
import io
import json
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

TEST_USER = os.environ.get("USER", "testuser")
TEST_PASSWORD = "sftp_auth_test_x7k2m9"
BAD_PASSWORD = "wrong_sftp_auth_test_x7k2m9"


class _MinimalSFTP(paramiko.SFTPServerInterface):
    """Enough SFTP surface for the subsystem handshake to complete."""


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
        *,
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
        self._auth_log.append(self._auth_log.label_for(key))
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

    def check_channel_subsystem_request(self, channel, name):
        if name != "sftp":
            return False
        return super().check_channel_subsystem_request(channel, name)


class _AuthSFTPServer:
    def __init__(
        self,
        host_key: paramiko.PKey,
        key_labels: dict[str, str],
        *,
        accept_password: str | None = None,
        accept_keys: list[paramiko.PKey] | None = None,
        allow_password_attempts: bool = False,
    ) -> None:
        self._host_key = host_key
        self._accept_password = accept_password
        self._accept_keys = accept_keys or []
        self._allow_password_attempts = allow_password_attempts
        self.auth_log = _AuthLog(key_labels)
        self._closed = False
        self._sock = socket.socket()
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
        transport.set_subsystem_handler("sftp", paramiko.SFTPServer, _MinimalSFTP)
        try:
            transport.start_server(
                server=_SSHServer(
                    auth_log=self.auth_log,
                    accept_password=self._accept_password,
                    accept_keys=self._accept_keys,
                    allow_password_attempts=self._allow_password_attempts,
                )
            )
            while transport.is_active():
                time.sleep(0.05)
        except Exception:
            pass
        finally:
            transport.close()


def _known_hosts_line(host_key: paramiko.PKey, port: int) -> str:
    key_b64 = base64.b64encode(host_key.asbytes()).decode("ascii")
    return f"[127.0.0.1]:{port} {host_key.get_name()} {key_b64}"


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


def _write_known_hosts(home: Path, host_key: paramiko.PKey, servers: list[_AuthSFTPServer]) -> None:
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    known_hosts = ssh_dir / "known_hosts"
    known_hosts.write_text(
        "".join(_known_hosts_line(host_key, server.port) + "\n" for server in servers),
        encoding="utf-8",
        newline="\n",
    )
    known_hosts.chmod(0o600)


def _drain(stream) -> None:
    for _ in stream:
        pass


def _launch_mcp(extra_env: dict[str, str | None]) -> tuple[subprocess.Popen, int]:
    env = dict(os.environ)
    for key, value in extra_env.items():
        if value is None:
            env.pop(key, None)
        else:
            env[key] = value
    home = extra_env.get("HOME")
    if home is not None:
        prior = env.get("JAVA_TOOL_OPTIONS", "")
        env["JAVA_TOOL_OPTIONS"] = (prior + " " if prior else "") + f"-Duser.home={home}"
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        env=env,
    )
    port = None
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
        if not line:
            continue
        line = line.strip()
        if line.startswith("MCP_PORT="):
            port = int(line.split("=", 1)[1])
            break
    if port is None:
        proc.terminate()
        _, stderr = proc.communicate(timeout=5)
        raise RuntimeError(f"MCP server did not advertise MCP_PORT: {stderr.strip()}")
    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
    return proc, port


_rpc_seq = 0


def _rpc(sock: socket.socket, method: str, params=None, timeout: float = 30) -> dict:
    global _rpc_seq
    _rpc_seq += 1
    msg = {"jsonrpc": "2.0", "id": _rpc_seq, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + timeout
    while time.time() < deadline:
        sock.settimeout(max(0.1, deadline - time.time()))
        try:
            chunk = sock.recv(8192)
        except socket.timeout:
            break
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    if not line:
        return {"error": {"message": "no response"}}
    return json.loads(line.decode("utf-8"))


def _call(sock: socket.socket, tool: str, args: dict) -> dict:
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args})


def _acquire(sock: socket.socket, url: str) -> tuple[str | None, dict]:
    response = _call(sock, "acquire", {"url": url})
    handle = (response.get("result") or {}).get("handleId")
    return handle, response


def _release(sock: socket.socket, handle: str | None) -> None:
    if handle is not None:
        _call(sock, "release", {"handleId": handle})


def _terminate(proc: subprocess.Popen) -> None:
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()


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
    try:
        subprocess.run(
            ["ssh-agent", "-k"],
            check=False,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env=env,
        )
    except Exception:
        pass


def _add_key_to_agent(private_text: str, auth_sock: str) -> None:
    with tempfile.NamedTemporaryFile(suffix=".key", delete=False) as f:
        key_path = Path(f.name)
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


def _assert_acquire_attempts(
    failures: list[str],
    sock: socket.socket,
    req_id: str,
    server: _AuthSFTPServer,
    url: str,
    expected: list[str],
    description: str,
) -> None:
    server.auth_log.clear()
    handle, response = _acquire(sock, url)
    actual = _compressed(server.auth_log.events())
    if handle is not None:
        _release(sock, handle)
    if handle is None:
        failures.append(f"{req_id}: acquire failed; response={response}, auth attempts={actual}")
        print(f"[{req_id}] FAIL: acquire failed; attempts={actual}, response={response}")
    elif actual != expected:
        failures.append(f"{req_id}: expected auth attempts {expected}, got {actual}")
        print(f"[{req_id}] FAIL: expected attempts {expected}, got {actual}")
    else:
        print(f"[{req_id}] PASS: {description}")


def main() -> int:
    failures: list[str] = []
    procs: list[subprocess.Popen] = []
    servers: list[_AuthSFTPServer] = []
    agent_sock: str | None = None
    agent_pid: int | None = None
    tmpdir = Path(tempfile.mkdtemp(prefix="sftp_auth_test_"))

    try:
        host_key = paramiko.RSAKey.generate(bits=2048)
        ed25519_user, ed25519_private = _generate_ed25519_key()
        agent_user, agent_private = _generate_ed25519_key()
        ecdsa_user = paramiko.ECDSAKey.generate()
        rsa_user = paramiko.RSAKey.generate(bits=2048)
        key_labels = {
            ed25519_user.get_base64(): "id_ed25519",
            ecdsa_user.get_base64(): "id_ecdsa",
            rsa_user.get_base64(): "id_rsa",
            agent_user.get_base64(): "agent",
        }

        password_only = _AuthSFTPServer(
            host_key,
            key_labels,
            accept_password=TEST_PASSWORD,
            accept_keys=[],
        )
        ed25519_only = _AuthSFTPServer(
            host_key,
            key_labels,
            accept_keys=[ed25519_user],
        )
        ecdsa_only = _AuthSFTPServer(
            host_key,
            key_labels,
            accept_keys=[ecdsa_user],
        )
        rsa_only = _AuthSFTPServer(
            host_key,
            key_labels,
            accept_keys=[rsa_user],
        )
        first_wins = _AuthSFTPServer(
            host_key,
            key_labels,
            accept_password=TEST_PASSWORD,
            accept_keys=[ed25519_user, ecdsa_user, rsa_user],
        )
        servers.extend([password_only, ed25519_only, ecdsa_only, rsa_only, first_wins])
        for server in servers:
            server.start()

        home1 = tmpdir / "home1"
        ssh1 = home1 / ".ssh"
        ssh1.mkdir(mode=0o700, parents=True, exist_ok=True)
        _write_known_hosts(home1, host_key, servers)
        _write_key(ed25519_user, ssh1 / "id_ed25519", ed25519_private)
        _write_key(ecdsa_user, ssh1 / "id_ecdsa")
        _write_key(rsa_user, ssh1 / "id_rsa")

        proc1, mcp_port1 = _launch_mcp({"HOME": str(home1), "SSH_AUTH_SOCK": None})
        procs.append(proc1)

        with socket.create_connection(("127.0.0.1", mcp_port1), timeout=10) as s1:
            _assert_acquire_attempts(
                failures,
                s1,
                "02.10",
                password_only,
                f"sftp://{TEST_USER}:{TEST_PASSWORD}@127.0.0.1:{password_only.port}/",
                ["password:ok"],
                "inline URL password authenticated against a password-only server",
            )
            _assert_acquire_attempts(
                failures,
                s1,
                "02.12",
                ed25519_only,
                f"sftp://127.0.0.1:{ed25519_only.port}/",
                ["id_ed25519"],
                "id_ed25519 was the first key-file method tried and succeeded",
            )
            _assert_acquire_attempts(
                failures,
                s1,
                "02.13",
                ecdsa_only,
                f"sftp://127.0.0.1:{ecdsa_only.port}/",
                ["id_ed25519", "id_ecdsa"],
                "id_ecdsa was tried after id_ed25519 failed",
            )
            _assert_acquire_attempts(
                failures,
                s1,
                "02.14",
                rsa_only,
                f"sftp://127.0.0.1:{rsa_only.port}/",
                ["id_ed25519", "id_ecdsa", "id_rsa"],
                "id_rsa was tried after id_ed25519 and id_ecdsa failed",
            )
            _assert_acquire_attempts(
                failures,
                s1,
                "02.15",
                first_wins,
                f"sftp://{TEST_USER}:{TEST_PASSWORD}@127.0.0.1:{first_wins.port}/",
                ["password:ok"],
                "authentication stopped after the inline password succeeded",
            )

        _terminate(proc1)
        procs.remove(proc1)

        agent_no_password = _AuthSFTPServer(
            host_key,
            key_labels,
            accept_keys=[agent_user],
        )
        agent_after_bad_password = _AuthSFTPServer(
            host_key,
            key_labels,
            accept_keys=[agent_user],
            allow_password_attempts=True,
        )
        servers.extend([agent_no_password, agent_after_bad_password])
        agent_no_password.start()
        agent_after_bad_password.start()

        home2 = tmpdir / "home2"
        (home2 / ".ssh").mkdir(mode=0o700, parents=True, exist_ok=True)
        _write_known_hosts(home2, host_key, [agent_no_password, agent_after_bad_password])

        try:
            agent_sock, agent_pid = _start_ssh_agent()
            _add_key_to_agent(agent_private, agent_sock)

            proc2, mcp_port2 = _launch_mcp({"HOME": str(home2), "SSH_AUTH_SOCK": agent_sock})
            procs.append(proc2)
            with socket.create_connection(("127.0.0.1", mcp_port2), timeout=10) as s2:
                _assert_acquire_attempts(
                    failures,
                    s2,
                    "02.11",
                    agent_no_password,
                    f"sftp://127.0.0.1:{agent_no_password.port}/",
                    ["agent"],
                    "SSH agent was tried when the URL had no inline password",
                )
                _assert_acquire_attempts(
                    failures,
                    s2,
                    "02.11",
                    agent_after_bad_password,
                    f"sftp://{TEST_USER}:{BAD_PASSWORD}@127.0.0.1:{agent_after_bad_password.port}/",
                    ["password:bad", "agent"],
                    "SSH agent was tried after the inline password was rejected",
                )

            _terminate(proc2)
            procs.remove(proc2)
        except FileNotFoundError:
            failures.append(
                "02.11: ssh-agent and ssh-add are required to exercise SSH_AUTH_SOCK auth"
            )
            print("[02.11] FAIL: ssh-agent or ssh-add not found")
        except subprocess.CalledProcessError as ex:
            failures.append(f"02.11: ssh-add failed ({ex}); could not load key into agent")
            print("[02.11] FAIL: could not add key to SSH agent")

    finally:
        for proc in list(procs):
            _terminate(proc)
        _stop_ssh_agent(agent_sock, agent_pid)
        for server in servers:
            server.close()
        shutil.rmtree(tmpdir, ignore_errors=True)

    if failures:
        print("\nFAILURES:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
