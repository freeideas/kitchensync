#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko"]
# ///
"""Three-category error surface: not-found, permission-denied, and I/O error."""

from __future__ import annotations

import base64, json, os, shutil, socket, subprocess, sys, threading, time
from pathlib import Path

import paramiko

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")
PROJECT_PATH = Path(PROJECT).resolve()
TEST_ROOT = PROJECT_PATH / "tmp" / "testks" / "03_error-categories"
TEST_USER = "sftp_error_test"
TEST_PASSWORD = "sftp_error_test_password"


def _drain(stream):
    for _ in stream:
        pass


def _launch(extra_env=None):
    env = dict(os.environ)
    for key, value in (extra_env or {}).items():
        if value is None:
            env.pop(key, None)
        else:
            env[key] = value
    home = (extra_env or {}).get("HOME")
    if home is not None:
        prior = env.get("JAVA_TOOL_OPTIONS", "")
        env["JAVA_TOOL_OPTIONS"] = (prior + " " if prior else "") + f"-Duser.home={home}"
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


def _rpc(sock, method, params=None, rpc_id=1):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + 30
    while time.time() < deadline:
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
        return {"error": {"code": -1, "message": "no JSON-RPC response"}}
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, args, rpc_id=1):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args}, rpc_id)


def _has_category(resp, category):
    """Return (True, '') if resp is a -32000 error whose message contains the category string."""
    err = resp.get("error")
    if not err:
        return False, f"expected error, got result: {json.dumps(resp.get('result', {}))[:120]}"
    if err.get("code") != -32000:
        return False, f"expected JSON-RPC code -32000, got: {err.get('code')!r}"
    msg = (err.get("message") or "").lower()
    if category.lower() not in msg:
        return False, f"expected '{category}' in error message, got: {err.get('message')!r}"
    return True, ""


def _find_prop(tool_schema, *candidates):
    """Return first candidate present in the tool's inputSchema properties, else first candidate."""
    props = (tool_schema.get("inputSchema") or {}).get("properties", {})
    for c in candidates:
        if c in props:
            return c
    return candidates[0] if candidates else None


def _extract_handle(acq_resp):
    """Pull the handle ID out of an acquire result regardless of key name."""
    r = acq_resp.get("result") or {}
    for k in ("handle-id", "handleId", "handle_id", "handle", "id"):
        if k in r:
            return r[k]
    return None


class _ErrorSFTP(paramiko.SFTPServerInterface):
    def __init__(self, server, transport=None):
        super().__init__(server)
        self._transport = transport

    def stat(self, path):
        if path == "/server-failure":
            return paramiko.SFTP_FAILURE
        return paramiko.SFTP_NO_SUCH_FILE

    def lstat(self, path):
        if path == "/server-failure":
            return paramiko.SFTP_FAILURE
        return paramiko.SFTP_NO_SUCH_FILE

    def open(self, path, flags, attr):
        if path == "/drop-during-read":
            return _DropReadHandle(self._transport)
        return paramiko.SFTP_PERMISSION_DENIED


class _DropReadHandle(paramiko.SFTPHandle):
    def __init__(self, transport):
        super().__init__(flags=0)
        self._transport = transport

    def read(self, offset, length):
        if self._transport is not None:
            self._transport.close()
        time.sleep(0.1)
        return b""


class _PasswordSSHServer(paramiko.ServerInterface):
    def check_channel_request(self, kind, chanid):
        return paramiko.OPEN_SUCCEEDED if kind == "session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def check_auth_password(self, username, password):
        if username == TEST_USER and password == TEST_PASSWORD:
            return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED

    def get_allowed_auths(self, username):
        return "password"

    def check_channel_subsystem_request(self, channel, name):
        if name != "sftp":
            return False
        return super().check_channel_subsystem_request(channel, name)


def _start_error_sftp_server(host_key):
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", 0))
    srv.listen(10)
    port = srv.getsockname()[1]

    def _accept_loop():
        while True:
            try:
                conn, _ = srv.accept()
            except OSError:
                return
            threading.Thread(target=_serve_conn, args=(conn,), daemon=True).start()

    def _serve_conn(conn):
        transport = paramiko.Transport(conn)
        transport.add_server_key(host_key)
        transport.set_subsystem_handler("sftp", paramiko.SFTPServer, _ErrorSFTP, transport)
        try:
            transport.start_server(server=_PasswordSSHServer())
            while transport.is_active():
                time.sleep(0.05)
        except Exception:
            pass
        finally:
            transport.close()

    threading.Thread(target=_accept_loop, daemon=True).start()
    return srv, port


def _known_hosts_line(host_key, port):
    key_b64 = base64.b64encode(host_key.asbytes()).decode("ascii")
    return f"[127.0.0.1]:{port} {host_key.get_name()} {key_b64}\n"


def _write_known_hosts(home, host_key, port):
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(parents=True, exist_ok=True)
    known_hosts = ssh_dir / "known_hosts"
    known_hosts.write_text(_known_hosts_line(host_key, port), encoding="utf-8")
    known_hosts.chmod(0o644)


def _bad_ssh_server():
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", 0))
    srv.listen(5)
    port = srv.getsockname()[1]

    def _serve():
        while True:
            try:
                conn, _ = srv.accept()
            except OSError:
                return
            conn.sendall(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
            conn.close()

    threading.Thread(target=_serve, daemon=True).start()
    return srv, port


def main() -> int:
    shutil.rmtree(TEST_ROOT, ignore_errors=True)
    tmp_home = TEST_ROOT / "home"
    tmp_home.mkdir(parents=True)
    host_key = paramiko.RSAKey.generate(bits=2048)
    error_srv, error_port = _start_error_sftp_server(host_key)
    _write_known_hosts(tmp_home, host_key, error_port)

    proc, mcp_port = _launch({"HOME": str(tmp_home), "SSH_AUTH_SOCK": None})
    bad_ssh_srv = None
    sftp_url = f"sftp://{TEST_USER}:{TEST_PASSWORD}@127.0.0.1:{error_port}/"
    try:
        with socket.create_connection(("127.0.0.1", mcp_port), timeout=10) as s:
            failures = []
            rpc_id = 0

            rpc_id += 1
            tl = _rpc(s, "tools/list", rpc_id=rpc_id)
            tools = {t["name"]: t for t in (tl.get("result") or {}).get("tools", [])}
            print(f"[setup] {len(tools)} tool(s): {sorted(tools)}")

            acquire_t = "acquire" if "acquire" in tools else None
            release_t = "release" if "release" in tools else None
            stat_t = "stat" if "stat" in tools else None
            open_read_t = "open-read" if "open-read" in tools else None
            read_t = "read" if "read" in tools else None

            acq_url_p = _find_prop(tools.get(acquire_t or "", {}), "url")
            stat_hdl_p = _find_prop(tools.get(stat_t or "", {}), "handle-id", "handleId", "handle_id")
            stat_path_p = _find_prop(tools.get(stat_t or "", {}), "path")
            or_hdl_p = _find_prop(tools.get(open_read_t or "", {}), "handle-id", "handleId", "handle_id")
            or_path_p = _find_prop(tools.get(open_read_t or "", {}), "path")
            read_rh_p = _find_prop(tools.get(read_t or "", {}), "read-handle-id", "readHandleId", "read_handle_id")
            read_max_p = _find_prop(tools.get(read_t or "", {}), "max-bytes", "maxBytes", "max_bytes")
            rel_hdl_p = _find_prop(tools.get(release_t or "", {}), "handle-id", "handleId", "handle_id")

            def _release(handle):
                nonlocal rpc_id
                if release_t and handle:
                    rpc_id += 1
                    _call(s, release_t, {rel_hdl_p: handle}, rpc_id)

            def _probe_io(label, acq_resp):
                """Check acquire response or probe first op for I/O error. Returns (ok, msg)."""
                nonlocal rpc_id
                if acq_resp.get("error"):
                    ok, msg = _has_category(acq_resp, "i/o error")
                    if not ok:
                        ok, msg = _has_category(acq_resp, "io error")
                    return ok, msg
                # Lazy connect: error surfaces on first operation.
                h = _extract_handle(acq_resp)
                if stat_t and h:
                    rpc_id += 1
                    op = _call(s, stat_t, {stat_hdl_p: h, stat_path_p: "/"}, rpc_id)
                    _release(h)
                    ok, msg = _has_category(op, "i/o error")
                    if not ok:
                        ok, msg = _has_category(op, "io error")
                    return ok, msg
                return False, f"{label}: acquire succeeded but no stat tool to probe the I/O error"

            # --- 03.1: non-existent path → "not found" error ---
            nonexistent = "/not-found"

            if acquire_t and stat_t:
                rpc_id += 1
                acq = _call(s, acquire_t, {acq_url_p: sftp_url}, rpc_id)
                if acq.get("error"):
                    failures.append(f"03.1: acquire(test server) failed: {acq['error']['message']!r}")
                    print(f"[03.1] FAIL — acquire failed: {acq['error']['message']!r}")
                else:
                    h = _extract_handle(acq)
                    rpc_id += 1
                    resp = _call(s, stat_t, {stat_hdl_p: h, stat_path_p: nonexistent}, rpc_id)
                    ok, msg = _has_category(resp, "not found")
                    print(f"[03.1] {'PASS' if ok else 'FAIL'} — stat of nonexistent path surfaced 'not found'"
                          + ("" if ok else f": {msg}"))
                    if not ok:
                        failures.append(f"03.1: {msg}")
                    _release(h)
            else:
                failures.append(f"03.1: missing tools (acquire={acquire_t}, stat={stat_t})")
                print(f"[03.1] FAIL — required tools not found")

            # --- 03.2: authorization refusal → "permission denied" error ---
            if acquire_t and open_read_t:
                rpc_id += 1
                acq = _call(s, acquire_t, {acq_url_p: sftp_url}, rpc_id)
                if acq.get("error"):
                    failures.append(f"03.2: acquire(test server) failed: {acq['error']['message']!r}")
                    print(f"[03.2] FAIL — acquire failed: {acq['error']['message']!r}")
                else:
                    h = _extract_handle(acq)
                    rpc_id += 1
                    resp = _call(s, open_read_t, {or_hdl_p: h, or_path_p: "/permission-denied"}, rpc_id)
                    ok, msg = _has_category(resp, "permission denied")
                    print(f"[03.2] {'PASS' if ok else 'FAIL'} — authorization refusal surfaced 'permission denied'"
                          + ("" if ok else f": {msg}"))
                    if not ok:
                        failures.append(f"03.2: {msg}")
                    _release(h)
            else:
                failures.append(f"03.2: missing tools (acquire={acquire_t}, open-read={open_read_t})")
                print(f"[03.2] FAIL — required tools not found")

            # --- 03.3: failed SSH handshake → "I/O error" ---
            bad_ssh_srv, bad_ssh_port = _bad_ssh_server()

            if acquire_t:
                rpc_id += 1
                acq = _call(s, acquire_t, {acq_url_p: f"sftp://127.0.0.1:{bad_ssh_port}/"}, rpc_id)
                ok, msg = _probe_io("03.3", acq)
                print(f"[03.3] {'PASS' if ok else 'FAIL'} — failed SSH handshake surfaced as 'I/O error'"
                      + ("" if ok else f": {msg}"))
                if not ok:
                    failures.append(f"03.3: {msg}")
            else:
                failures.append("03.3: acquire tool not found")
                print("[03.3] FAIL — acquire tool missing")

            # --- 03.5: server-side / protocol-level failure → "I/O error" ---
            if acquire_t and stat_t:
                rpc_id += 1
                acq = _call(s, acquire_t, {acq_url_p: sftp_url}, rpc_id)
                if acq.get("error"):
                    failures.append(f"03.5: acquire(test server) failed: {acq['error']['message']!r}")
                    print(f"[03.5] FAIL — acquire failed: {acq['error']['message']!r}")
                else:
                    h = _extract_handle(acq)
                    rpc_id += 1
                    resp = _call(s, stat_t, {stat_hdl_p: h, stat_path_p: "/server-failure"}, rpc_id)
                    ok, msg = _has_category(resp, "i/o error")
                    if not ok:
                        ok, msg = _has_category(resp, "io error")
                    print(f"[03.5] {'PASS' if ok else 'FAIL'} — server-side SFTP failure surfaced as 'I/O error'"
                          + ("" if ok else f": {msg}"))
                    if not ok:
                        failures.append(f"03.5: {msg}")
                    _release(h)
            else:
                failures.append(f"03.5: missing tools (acquire={acquire_t}, stat={stat_t})")
                print(f"[03.5] FAIL — required tools not found")

            # --- 03.4: network failure during in-progress operation → "I/O error" ---
            if acquire_t and open_read_t and read_t:
                rpc_id += 1
                acq = _call(s, acquire_t, {acq_url_p: sftp_url}, rpc_id)
                if acq.get("error"):
                    failures.append(f"03.4: acquire(test server) failed: {acq['error']['message']!r}")
                    print(f"[03.4] FAIL — acquire failed: {acq['error']['message']!r}")
                else:
                    h = _extract_handle(acq)
                    rpc_id += 1
                    opened = _call(s, open_read_t, {or_hdl_p: h, or_path_p: "/drop-during-read"}, rpc_id)
                    read_handle = ((opened.get("result") or {}).get("readHandleId")
                                   or (opened.get("result") or {}).get("read-handle-id")
                                   or (opened.get("result") or {}).get("read_handle_id"))
                    if opened.get("error") or not read_handle:
                        msg = opened.get("error", {}).get("message", f"missing read handle in {opened!r}")
                        failures.append(f"03.4: open-read setup failed: {msg!r}")
                        print(f"[03.4] FAIL — open-read setup failed: {msg!r}")
                        _release(h)
                    else:
                        rpc_id += 1
                        resp = _call(s, read_t, {read_rh_p: read_handle, read_max_p: 1}, rpc_id)
                        ok, msg = _has_category(resp, "i/o error")
                        if not ok:
                            ok, msg = _has_category(resp, "io error")
                        print(f"[03.4] {'PASS' if ok else 'FAIL'} — connection drop during read surfaced as 'I/O error'"
                              + ("" if ok else f": {msg}"))
                        if not ok:
                            failures.append(f"03.4: {msg}")
            else:
                failures.append(f"03.4: missing tools (acquire={acquire_t}, open-read={open_read_t}, read={read_t})")
                print(f"[03.4] FAIL — required tools not found")

            if failures:
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1
            print("\nAll assertions passed.")
            return 0
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        for res in (bad_ssh_srv, error_srv):
            if res is not None:
                try:
                    res.close()
                except OSError:
                    pass
        shutil.rmtree(TEST_ROOT, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
