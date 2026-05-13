#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko"]
# ///
"""Pool acquire, release, idle keep-alive, and shutdown."""

from __future__ import annotations

import base64
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

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TEST_USER = "pooltest"
TEST_PASSWORD = "pool_test_password"


class _MinimalSFTP(paramiko.SFTPServerInterface):
    """Only the SFTP protocol handshake is needed for pool tests."""


class _SSHServer(paramiko.ServerInterface):
    def check_channel_request(self, kind, chanid):
        return (
            paramiko.OPEN_SUCCEEDED
            if kind == "session"
            else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED
        )

    def check_auth_password(self, username, password):
        return (
            paramiko.AUTH_SUCCESSFUL
            if password == TEST_PASSWORD
            else paramiko.AUTH_FAILED
        )

    def get_allowed_auths(self, username):
        return "password"


class _CountingSFTPServer:
    def __init__(self, host_key: paramiko.PKey) -> None:
        self._host_key = host_key
        self._sock = socket.socket()
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind(("127.0.0.1", 0))
        self._sock.listen(64)
        self.port = int(self._sock.getsockname()[1])
        self._lock = threading.Lock()
        self._accepted = 0
        self._active = 0
        self._closed = False

    def start(self) -> None:
        threading.Thread(target=self._accept_loop, daemon=True).start()

    def close(self) -> None:
        self._closed = True
        try:
            self._sock.close()
        except OSError:
            pass

    def accepted_count(self) -> int:
        with self._lock:
            return self._accepted

    def active_count(self) -> int:
        with self._lock:
            return self._active

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
        counted = False
        try:
            transport.start_server(server=_SSHServer())
            while transport.is_active():
                if not counted and transport.is_authenticated():
                    with self._lock:
                        self._accepted += 1
                        self._active += 1
                    counted = True
                time.sleep(0.02)
        except Exception:
            pass
        finally:
            try:
                transport.close()
            finally:
                if counted:
                    with self._lock:
                        self._active -= 1


class _BlackholeServer:
    def __init__(self) -> None:
        self._sock = socket.socket()
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind(("127.0.0.1", 0))
        self._sock.listen(8)
        self.port = int(self._sock.getsockname()[1])
        self._closed = False
        self._connections: list[socket.socket] = []

    def start(self) -> None:
        threading.Thread(target=self._accept_loop, daemon=True).start()

    def close(self) -> None:
        self._closed = True
        for conn in self._connections:
            try:
                conn.close()
            except OSError:
                pass
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
            self._connections.append(conn)


def _known_hosts_line(host_key: paramiko.PKey, port: int) -> str:
    key_b64 = base64.b64encode(host_key.asbytes()).decode("ascii")
    return f"[127.0.0.1]:{port} {host_key.get_name()} {key_b64}"


def _drain(stream):
    for _ in stream:
        pass


def _launch(home: Path):
    java_opts = os.environ.get("JAVA_TOOL_OPTIONS", "")
    env = {
        **os.environ,
        "HOME": str(home),
        "JAVA_TOOL_OPTIONS": f"{java_opts} -Duser.home={home}".strip(),
    }
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
        raise RuntimeError("MCP server did not advertise MCP_PORT")
    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
    return proc, port


_rpc_lock = threading.Lock()
_rpc_id = 0


def _rpc(sock, method, params=None, timeout=15):
    global _rpc_id
    with _rpc_lock:
        _rpc_id += 1
        rpc_id = _rpc_id
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + timeout
    while time.time() < deadline:
        sock.settimeout(max(0.05, deadline - time.time()))
        try:
            chunk = sock.recv(8192)
        except (socket.timeout, OSError):
            break
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    if b"\n" not in buf:
        return None
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, args=None, timeout=15):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args or {}}, timeout=timeout)


def _result(resp):
    return (resp or {}).get("result") or {}


def _handle_id(resp):
    return _result(resp).get("handleId")


def _conn(port: int):
    return socket.create_connection(("127.0.0.1", port), timeout=15)


def _wait_for(predicate, timeout: float) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if predicate():
            return True
        time.sleep(0.05)
    return predicate()


def _release(sock, handle_id: str | None) -> None:
    if handle_id is not None:
        _call(sock, "release", {"handleId": handle_id}, timeout=10)


def _shutdown(sock) -> None:
    _call(sock, "shutdown", {}, timeout=10)


def main() -> int:
    failures: list[str] = []
    tmpdir = Path(tempfile.mkdtemp(prefix="sftp_pool_test_"))
    proc = None
    server = None
    blackhole = None

    try:
        host_key = paramiko.RSAKey.generate(bits=2048)
        server = _CountingSFTPServer(host_key)
        server.start()
        blackhole = _BlackholeServer()
        blackhole.start()

        home = tmpdir / "home"
        ssh_dir = home / ".ssh"
        ssh_dir.mkdir(parents=True)
        (ssh_dir / "known_hosts").write_text(
            _known_hosts_line(host_key, server.port) + "\n",
            encoding="utf-8",
            newline="\n",
        )

        proc, mcp_port = _launch(home)
        url = f"sftp://{TEST_USER}:{TEST_PASSWORD}@127.0.0.1:{server.port}/"
        blackhole_url = f"sftp://{TEST_USER}:{TEST_PASSWORD}@127.0.0.1:{blackhole.port}/"

        with _conn(mcp_port) as s:
            timeout_result: dict[str, object] = {}

            def default_timeout_acquire() -> None:
                started = time.monotonic()
                with _conn(mcp_port) as st:
                    resp = _call(st, "acquire", {"url": blackhole_url}, timeout=45)
                timeout_result["elapsed"] = time.monotonic() - started
                timeout_result["resp"] = resp

            timeout_thread = threading.Thread(target=default_timeout_acquire, daemon=True)
            timeout_thread.start()

            # 02.2 / 02.5: first acquire opens a session; release + acquire reuses it.
            before = server.accepted_count()
            r1 = _call(s, "acquire", {"url": url}, timeout=20)
            h1 = _handle_id(r1)
            opened_first = h1 is not None and _wait_for(
                lambda: server.accepted_count() == before + 1,
                3,
            )
            if not opened_first:
                failures.append(f"02.2: first acquire did not open one session; response={r1}")
                print(f"[02.2] FAIL: accepted_count={server.accepted_count()}, response={r1}")
            else:
                print("[02.2] PASS: first acquire opened a new SSH+SFTP session")

            _release(s, h1)

            before_reuse = server.accepted_count()
            r2 = _call(s, "acquire", {"url": url}, timeout=20)
            h2 = _handle_id(r2)
            reused = h2 is not None and server.accepted_count() == before_reuse
            if not reused:
                failures.append(
                    f"02.5: re-acquire before idle expiry opened a new session; response={r2}"
                )
                print(f"[02.5] FAIL: accepted_count changed from {before_reuse} to {server.accepted_count()}")
            else:
                print("[02.5] PASS: re-acquire before idle expiry reused the cached session")

            # 02.3 / 02.9: default max_connections is 10, so the 11th held acquire blocks.
            held = [h2]
            for _ in range(9):
                resp = _call(s, "acquire", {"url": url}, timeout=20)
                hid = _handle_id(resp)
                if hid is None:
                    failures.append(f"02.9: could not acquire 10 default-cap handles; response={resp}")
                    break
                held.append(hid)

            eleventh_done = threading.Event()
            eleventh_result: list[dict | None] = []

            def acquire_eleventh() -> None:
                with _conn(mcp_port) as s2:
                    eleventh_result.append(_call(s2, "acquire", {"url": url}, timeout=20))
                eleventh_done.set()

            t = threading.Thread(target=acquire_eleventh, daemon=True)
            t.start()
            time.sleep(0.75)
            blocked_at_cap = not eleventh_done.is_set()

            _release(s, held.pop(0) if held else None)
            t.join(timeout=8)
            eleventh_handle = _handle_id(eleventh_result[0]) if eleventh_result else None
            if eleventh_handle is not None:
                held.append(eleventh_handle)

            if not blocked_at_cap:
                failures.append("02.3: 11th acquire returned before a default-cap slot was released")
                print("[02.3] FAIL: acquire did not block when 10 default sessions were busy")
            elif eleventh_handle is None:
                failures.append(f"02.3: blocked acquire did not succeed after release; result={eleventh_result}")
                print(f"[02.3] FAIL: blocked acquire did not return a handle; result={eleventh_result}")
            else:
                print("[02.3] PASS: acquire blocked at the pool cap and resumed after release")
                print("[02.9] PASS: max_connections defaults to 10")

            release_started = time.monotonic()
            for hid in list(held):
                _release(s, hid)
            held.clear()

            # 02.6 / 02.43: default idle_keepalive_seconds is 30; released sessions expire.
            while time.monotonic() - release_started < 31.0:
                time.sleep(0.1)
            idle_closed = _wait_for(lambda: server.active_count() == 0, 5)
            before_after_idle = server.accepted_count()
            r_after_idle = _call(s, "acquire", {"url": url}, timeout=20)
            h_after_idle = _handle_id(r_after_idle)
            opened_after_idle = h_after_idle is not None and _wait_for(
                lambda: server.accepted_count() == before_after_idle + 1,
                3,
            )
            if not idle_closed or not opened_after_idle:
                failures.append(
                    "02.6/02.43: idle sessions did not expire after the 30s default "
                    f"(active={server.active_count()}, response={r_after_idle})"
                )
                print(
                    f"[02.6] FAIL: idle_closed={idle_closed}, "
                    f"accepted_count={server.accepted_count()}, response={r_after_idle}"
                )
            else:
                print("[02.6] PASS: idle session was torn down and next acquire opened a new one")
                print("[02.43] PASS: idle_keepalive_seconds defaults to 30")

            timeout_thread.join(timeout=10)
            elapsed = timeout_result.get("elapsed")
            timeout_resp = timeout_result.get("resp")
            timed_out_near_default = (
                isinstance(elapsed, float)
                and 25.0 <= elapsed <= 40.0
                and isinstance(timeout_resp, dict)
                and "error" in timeout_resp
            )
            if not timed_out_near_default:
                failures.append(
                    f"02.42: connect timeout default expected about 30s; "
                    f"elapsed={elapsed!r}, response={timeout_resp!r}"
                )
                print(f"[02.42] FAIL: elapsed={elapsed!r}, response={timeout_resp!r}")
            else:
                print(f"[02.42] PASS: connect_timeout_seconds defaults to about 30s ({elapsed:.1f}s)")

            # 02.8: shutdown closes both cached and in-use sessions.
            r_shutdown_cached = _call(s, "acquire", {"url": url}, timeout=20)
            h_shutdown_cached = _handle_id(r_shutdown_cached)
            _release(s, h_shutdown_cached)
            _shutdown(s)
            if not _wait_for(lambda: server.active_count() == 0, 5):
                failures.append(f"02.8: shutdown left {server.active_count()} session(s) open")
                print(f"[02.8] FAIL: active_count={server.active_count()} after shutdown")
            else:
                print("[02.8] PASS: shutdown closed cached and in-use sessions")

            # 02.7: reusing a session before expiry resets the idle timer.
            cfg = {
                "maxConnections": 2,
                "connectTimeoutSeconds": 5,
                "idleKeepaliveSeconds": 3,
            }
            cfg_resp = _call(s, "configure", cfg, timeout=10)
            if "result" not in (cfg_resp or {}):
                failures.append(f"02.7: configure for idle reset failed: {cfg_resp}")
            before_reset = server.accepted_count()
            r_reset_1 = _call(s, "acquire", {"url": url}, timeout=20)
            h_reset_1 = _handle_id(r_reset_1)
            _release(s, h_reset_1)
            time.sleep(2.0)
            r_reset_2 = _call(s, "acquire", {"url": url}, timeout=20)
            h_reset_2 = _handle_id(r_reset_2)
            mid_reused = h_reset_2 is not None and server.accepted_count() == before_reset + 1
            _release(s, h_reset_2)
            time.sleep(2.0)
            r_reset_3 = _call(s, "acquire", {"url": url}, timeout=20)
            h_reset_3 = _handle_id(r_reset_3)
            still_reused = h_reset_3 is not None and server.accepted_count() == before_reset + 1

            if not mid_reused:
                failures.append(f"02.7: mid-test acquire did not reuse the idle session; response={r_reset_2}")
                print("[02.7] FAIL: mid-test acquire was not a reuse")
            elif not still_reused:
                failures.append(f"02.7: idle timer was not reset by reuse; response={r_reset_3}")
                print("[02.7] FAIL: session expired from the original timer instead of the reset timer")
            else:
                print("[02.7] PASS: reuse reset the idle timer")
            _release(s, h_reset_3)
            _shutdown(s)

    finally:
        if proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        if server is not None:
            server.close()
        if blackhole is not None:
            blackhole.close()
        shutil.rmtree(tmpdir, ignore_errors=True)

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
