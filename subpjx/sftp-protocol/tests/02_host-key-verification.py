#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko"]
# ///
"""Tests 02_host-key-verification: SSH host key verification against ~/.ssh/known_hosts."""

from __future__ import annotations

import base64, json, os, shutil, socket, subprocess, sys, tempfile, threading, time
from pathlib import Path

import paramiko

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TEST_USER = os.environ.get("USER", "testuser")
TEST_PASSWORD = "sftp_host_key_test_x7k2m9"

_rpc_id = 0


class _MinimalSFTP(paramiko.SFTPServerInterface):
    """Minimal SFTP surface: enough for the SFTP subsystem handshake to succeed."""


class _SSHServer(paramiko.ServerInterface):
    def check_channel_request(self, kind, chanid):
        return paramiko.OPEN_SUCCEEDED if kind == "session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_auth_password(self, username, password):
        return paramiko.AUTH_SUCCESSFUL if password == TEST_PASSWORD else paramiko.AUTH_FAILED

    def get_allowed_auths(self, username):
        return "password"

    def check_channel_subsystem_request(self, channel, name):
        if name != "sftp":
            return False
        return super().check_channel_subsystem_request(channel, name)


def _start_test_server(host_key: paramiko.PKey) -> tuple[int, socket.socket]:
    srv_sock = socket.socket()
    srv_sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv_sock.bind(("127.0.0.1", 0))
    port = srv_sock.getsockname()[1]
    srv_sock.listen(10)

    def _accept_loop():
        while True:
            try:
                conn, _ = srv_sock.accept()
            except Exception:
                return
            threading.Thread(target=_serve_conn, args=(conn,), daemon=True).start()

    def _serve_conn(conn):
        transport = paramiko.Transport(conn)
        transport.add_server_key(host_key)
        transport.set_subsystem_handler("sftp", paramiko.SFTPServer, _MinimalSFTP)
        try:
            transport.start_server(server=_SSHServer())
            while transport.is_active():
                time.sleep(0.05)
        except Exception:
            pass
        finally:
            transport.close()

    threading.Thread(target=_accept_loop, daemon=True).start()
    return port, srv_sock


def _known_hosts_line(host_key: paramiko.PKey, port: int) -> str:
    key_b64 = base64.b64encode(host_key.asbytes()).decode("ascii")
    return f"[127.0.0.1]:{port} {host_key.get_name()} {key_b64}"


def _set_known_hosts(home: Path, content: str) -> None:
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    known_hosts = ssh_dir / "known_hosts"
    known_hosts.write_text(content, encoding="utf-8", newline="\n")
    known_hosts.chmod(0o600)


def _drain(stream):
    for _ in stream:
        pass


def _launch(home: Path):
    env = dict(os.environ)
    env.pop("SSH_AUTH_SOCK", None)
    env["HOME"] = str(home)
    prior_java_opts = env.get("JAVA_TOOL_OPTIONS", "")
    env["JAVA_TOOL_OPTIONS"] = (prior_java_opts + " " if prior_java_opts else "") + f"-Duser.home={home}"

    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
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
        raise RuntimeError("MCP server did not advertise MCP_PORT")
    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
    return proc, port


def _recv(sock, timeout=15):
    buf = b""
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            sock.settimeout(max(0.1, deadline - time.time()))
            chunk = sock.recv(8192)
        except socket.timeout:
            break
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    if not buf:
        return None
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _rpc(sock, method, params=None, timeout=15):
    global _rpc_id
    _rpc_id += 1
    msg = {"jsonrpc": "2.0", "id": _rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    return _recv(sock, timeout=timeout)


def _call(sock, tool, args=None, timeout=15):
    resp = _rpc(sock, "tools/call", {"name": tool, "arguments": args or {}}, timeout=timeout)
    if resp is None:
        return None
    if "error" in resp:
        return {"__error__": resp["error"]}
    result = resp.get("result") or {}
    if result.get("isError"):
        content = result.get("content", [])
        text = content[0].get("text", "") if content else ""
        return {"__error__": text}
    content = result.get("content", [])
    if content and content[0].get("type") == "text":
        text = content[0]["text"]
        try:
            return json.loads(text)
        except (json.JSONDecodeError, ValueError):
            return {"text": text}
    return result


def _extract_id(obj, *keys):
    if not isinstance(obj, dict):
        return None
    for k in keys:
        if k in obj:
            return obj[k]
    return None


def _is_error(obj):
    return isinstance(obj, dict) and "__error__" in obj


def _error_message(obj) -> str:
    if not _is_error(obj):
        return ""
    err = obj["__error__"]
    if isinstance(err, dict):
        return str(err.get("message") or "")
    return str(err)


def _is_io_error(obj) -> bool:
    msg = _error_message(obj).lower()
    return _is_error(obj) and ("io error" in msg or "i/o error" in msg)


def _configure_pool(sock):
    return _call(sock, "configure", {
        "maxConnections": 1,
        "connectTimeoutSeconds": 10,
        "idleKeepaliveSeconds": 1,
    })


def main() -> int:
    tmpdir = Path(tempfile.mkdtemp(prefix="sftp_host_key_test_"))
    host_key = paramiko.RSAKey.generate(bits=2048)
    wrong_host_key = paramiko.RSAKey.generate(bits=2048)
    server_socks: list[socket.socket] = []
    proc: subprocess.Popen | None = None

    try:
        port16, sock16 = _start_test_server(host_key)
        port17, sock17 = _start_test_server(host_key)
        port18, sock18 = _start_test_server(host_key)
        server_socks.extend([sock16, sock17, sock18])
        home = tmpdir / "home"
        proc, mcp_port = _launch(home)

        with socket.create_connection(("127.0.0.1", mcp_port), timeout=10) as s:
            failures = []
            sftp_url16 = f"sftp://{TEST_USER}:{TEST_PASSWORD}@127.0.0.1:{port16}/"
            sftp_url17 = f"sftp://{TEST_USER}:{TEST_PASSWORD}@127.0.0.1:{port17}/"
            sftp_url18 = f"sftp://{TEST_USER}:{TEST_PASSWORD}@127.0.0.1:{port18}/"

            # --- 02.16: matching known_hosts entry -> connection accepted ---
            _set_known_hosts(home, _known_hosts_line(host_key, port16) + "\n")
            cfg16 = _configure_pool(s)
            if _is_error(cfg16):
                print(f"[02.16] configure failed: {cfg16}")
                failures.append(f"02.16: configure failed: {cfg16['__error__']}")
            else:
                h16 = _call(s, "acquire", {"url": sftp_url16}, timeout=20)
                print(f"[02.16] acquire with matching known_hosts entry: {h16}")
                if _is_error(h16):
                    failures.append(f"02.16: expected connection accepted with matching key, got: {h16['__error__']}")
                else:
                    hid16 = _extract_id(h16, "handleId", "id")
                    if not hid16:
                        failures.append(f"02.16: accepted connection did not return a handleId, got: {h16}")
                    else:
                        _call(s, "release", {"handleId": hid16})
                _call(s, "shutdown")

            # --- 02.17: no entry in known_hosts -> connection rejected ---
            _set_known_hosts(home, "")
            cfg17 = _configure_pool(s)
            if _is_error(cfg17):
                print(f"[02.17] configure failed: {cfg17}")
                failures.append(f"02.17: configure failed: {cfg17['__error__']}")
            else:
                h17 = _call(s, "acquire", {"url": sftp_url17}, timeout=20)
                print(f"[02.17] acquire with no known_hosts entry: {h17}")
                if not _is_io_error(h17):
                    failures.append(
                        "02.17: expected unknown host to be rejected as an I/O connection failure, "
                        f"got: {h17}"
                    )
                _call(s, "shutdown")

            # --- 02.18: wrong key in known_hosts -> connection rejected ---
            _set_known_hosts(home, _known_hosts_line(wrong_host_key, port18) + "\n")
            cfg18 = _configure_pool(s)
            if _is_error(cfg18):
                print(f"[02.18] configure failed: {cfg18}")
                failures.append(f"02.18: configure failed: {cfg18['__error__']}")
            else:
                h18 = _call(s, "acquire", {"url": sftp_url18}, timeout=20)
                print(f"[02.18] acquire with mismatched known_hosts entry: {h18}")
                if not _is_io_error(h18):
                    failures.append(
                        "02.18: expected mismatched host key to be rejected as an I/O connection failure, "
                        f"got: {h18}"
                    )
                _call(s, "shutdown")

            if failures:
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1
            print("\nAll assertions passed.")
            return 0
    finally:
        if proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        for server_sock in server_socks:
            server_sock.close()
        shutil.rmtree(tmpdir, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
