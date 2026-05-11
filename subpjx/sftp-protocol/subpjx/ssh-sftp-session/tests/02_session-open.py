#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises open_session: session establishment, host-key verification, credential variants, timeout."""

from __future__ import annotations

import json, os, select, shutil, socket, subprocess, sys, tempfile, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

SSH_HOST = "localhost"
SSH_PORT = 22
SSH_USER = "ace"


def _find_key():
    for name in ("id_ed25519", "id_rsa", "id_ecdsa"):
        p = Path.home() / ".ssh" / name
        if p.exists():
            return str(p)
    raise RuntimeError("No SSH private key found in ~/.ssh/")


def _drain(stream):
    for _ in stream:
        pass


def _launch():
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
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


def _rpc(sock, method, params=None, rpc_id=1, timeout=30):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + timeout
    while time.time() < deadline:
        remaining = deadline - time.time()
        if remaining <= 0:
            break
        ready = select.select([sock], [], [], remaining)
        if not ready[0]:
            break
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, args, rid, timeout=30):
    resp = _rpc(sock, "tools/call", {"name": tool, "arguments": args}, rid, timeout)
    if "error" in resp:
        return {"_jsonrpc_error": True, "_is_error": True}
    result = resp.get("result") or {}
    is_err = result.get("isError", False)
    for c in result.get("content", []):
        if c.get("type") == "text":
            try:
                parsed = json.loads(c["text"])
                if isinstance(parsed, dict):
                    if is_err:
                        parsed.setdefault("_is_error", True)
                    return parsed
                return {"_raw": parsed, "_is_error": is_err}
            except json.JSONDecodeError:
                return {"_raw": c["text"], "_is_error": is_err}
    return {"_is_error": is_err}


def _session_id(r):
    return (r.get("session_id") or r.get("session") or r.get("id") or r.get("handle") or "")


def _is_io_failure(r):
    if not isinstance(r, dict):
        return False
    if r.get("_jsonrpc_error") or r.get("_is_error"):
        return True
    return (r.get("error") == "io_failure"
            or r.get("io_failure") is True
            or r.get("status") == "io_failure"
            or r.get("type") == "io_failure"
            or r.get("result") == "io_failure")


def _start_sshd_unknown_key():
    """Start a fresh sshd on an ephemeral port with a generated host key not in known_hosts."""
    tmpdir = Path(tempfile.mkdtemp())
    subprocess.run(
        ["ssh-keygen", "-t", "ed25519", "-f", str(tmpdir / "host_key"), "-N", ""],
        check=True, capture_output=True,
    )
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        sshd_port = s.getsockname()[1]
    config = (
        f"Port {sshd_port}\n"
        "ListenAddress 127.0.0.1\n"
        f"HostKey {tmpdir}/host_key\n"
        "AuthorizedKeysFile none\n"
        "PasswordAuthentication no\n"
        "ChallengeResponseAuthentication no\n"
        "UsePAM no\n"
        "StrictModes no\n"
        "LogLevel ERROR\n"
        f"PidFile {tmpdir}/sshd.pid\n"
    )
    config_file = tmpdir / "sshd_config"
    config_file.write_text(config)
    sshd_bin = shutil.which("sshd") or "/usr/sbin/sshd"
    if not Path(sshd_bin).exists():
        raise RuntimeError(f"sshd not found at {sshd_bin}")
    sshd_proc = subprocess.Popen(
        [sshd_bin, "-D", "-e", "-f", str(config_file)],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    deadline = time.time() + 5
    while time.time() < deadline:
        time.sleep(0.1)
        if sshd_proc.poll() is not None:
            err = sshd_proc.stderr.read().decode(errors="replace")
            raise RuntimeError(f"sshd exited early: {err[:300]}")
        try:
            with socket.create_connection(("127.0.0.1", sshd_port), timeout=0.5):
                break
        except OSError:
            pass
    else:
        sshd_proc.terminate()
        raise RuntimeError("sshd did not start in time")
    return sshd_proc, sshd_port, tmpdir


def _start_blackhole():
    """TCP server that accepts connections but never sends data (hung handshake)."""
    srv = socket.socket()
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", 0))
    srv.listen(5)
    port = srv.getsockname()[1]
    conns = []

    def _accept():
        srv.settimeout(30)
        try:
            while True:
                try:
                    conn, _ = srv.accept()
                    conns.append(conn)
                except OSError:
                    break
        finally:
            for c in conns:
                try:
                    c.close()
                except OSError:
                    pass

    threading.Thread(target=_accept, daemon=True).start()
    return srv, port


def main() -> int:
    key = _find_key()
    proc, port = _launch()
    failures = []
    rid = 0

    def nid():
        nonlocal rid
        rid += 1
        return rid

    sshd_proc = None
    sshd_tmpdir = None
    blackhole_srv = None

    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:

            # 02.1 — open_session returns a usable session given reachable host + working credential
            r1 = _call(s, "open_session", {
                "host": SSH_HOST, "port": SSH_PORT, "user": SSH_USER,
                "credentials": [{"type": "PrivateKeyFile", "path": key}],
                "connect_timeout_secs": 15,
            }, nid())
            sid1 = _session_id(r1)
            ok1 = bool(sid1) and not _is_io_failure(r1)
            print(f"[02.1] open_session with valid credential: {'PASS' if ok1 else 'FAIL'} sid={sid1!r}")
            if not ok1:
                failures.append(f"02.1: open_session did not return a usable session; result={r1}")
            else:
                _call(s, "close_session", {"session": sid1}, nid())

            # 02.2 — open_session returns io_failure when host key not in known_hosts
            try:
                sshd_proc, sshd_port, sshd_tmpdir = _start_sshd_unknown_key()
                r2 = _call(s, "open_session", {
                    "host": "127.0.0.1", "port": sshd_port, "user": SSH_USER,
                    "credentials": [{"type": "PrivateKeyFile", "path": key}],
                    "connect_timeout_secs": 10,
                }, nid())
                ok2 = _is_io_failure(r2)
                print(f"[02.2] unknown host key → io_failure: {'PASS' if ok2 else 'FAIL'} result={r2}")
                if not ok2:
                    failures.append(f"02.2: open_session on unknown-key host did not return io_failure; result={r2}")
            except RuntimeError as exc:
                print(f"[02.2] SKIP: could not start fresh sshd ({exc})")
            finally:
                if sshd_proc is not None:
                    sshd_proc.terminate()
                    try:
                        sshd_proc.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        sshd_proc.kill()
                        sshd_proc.wait()
                    sshd_proc = None
                if sshd_tmpdir is not None:
                    shutil.rmtree(sshd_tmpdir, ignore_errors=True)
                    sshd_tmpdir = None

            # 02.3 — open_session succeeds when any one credential works (bad first, good second)
            r3 = _call(s, "open_session", {
                "host": SSH_HOST, "port": SSH_PORT, "user": SSH_USER,
                "credentials": [
                    {"type": "Password", "value": "DEFINITELYWRONGPASSWORD99999"},
                    {"type": "PrivateKeyFile", "path": key},
                ],
                "connect_timeout_secs": 15,
            }, nid())
            sid3 = _session_id(r3)
            ok3 = bool(sid3) and not _is_io_failure(r3)
            print(f"[02.3] fallback credential succeeds: {'PASS' if ok3 else 'FAIL'} sid={sid3!r}")
            if not ok3:
                failures.append(f"02.3: open_session with one valid credential did not return session; result={r3}")
            else:
                _call(s, "close_session", {"session": sid3}, nid())

            # 02.4 — open_session returns io_failure when no supplied credential authenticates
            r4 = _call(s, "open_session", {
                "host": SSH_HOST, "port": SSH_PORT, "user": SSH_USER,
                "credentials": [
                    {"type": "Password", "value": "WRONGPASSWORD00000"},
                    {"type": "PrivateKeyFile", "path": "/tmp/__nonexistent_key_for_test__"},
                ],
                "connect_timeout_secs": 15,
            }, nid())
            ok4 = _is_io_failure(r4)
            print(f"[02.4] no valid credential → io_failure: {'PASS' if ok4 else 'FAIL'} result={r4}")
            if not ok4:
                failures.append(f"02.4: open_session with no valid credential did not return io_failure; result={r4}")

            # 02.5 — open_session returns io_failure when handshake doesn't complete within timeout
            blackhole_srv, blackhole_port = _start_blackhole()
            try:
                r5 = _call(s, "open_session", {
                    "host": "127.0.0.1", "port": blackhole_port, "user": SSH_USER,
                    "credentials": [{"type": "PrivateKeyFile", "path": key}],
                    "connect_timeout_secs": 2,
                }, nid(), timeout=15)
                ok5 = _is_io_failure(r5)
                print(f"[02.5] handshake timeout → io_failure: {'PASS' if ok5 else 'FAIL'} result={r5}")
                if not ok5:
                    failures.append(f"02.5: open_session on hung server did not return io_failure; result={r5}")
            finally:
                blackhole_srv.close()
                blackhole_srv = None

            # 02.6 — Password credential authenticates with the user's password
            test_password = os.environ.get("SFTP_TEST_PASSWORD")
            if test_password:
                r6 = _call(s, "open_session", {
                    "host": SSH_HOST, "port": SSH_PORT, "user": SSH_USER,
                    "credentials": [{"type": "Password", "value": test_password}],
                    "connect_timeout_secs": 15,
                }, nid())
                sid6 = _session_id(r6)
                ok6 = bool(sid6) and not _is_io_failure(r6)
                print(f"[02.6] Password credential: {'PASS' if ok6 else 'FAIL'} sid={sid6!r}")
                if not ok6:
                    failures.append(f"02.6: Password credential did not open session; result={r6}")
                else:
                    _call(s, "close_session", {"session": sid6}, nid())
            else:
                print("[02.6] SKIP: SFTP_TEST_PASSWORD not set — cannot verify Password credential success")

            # 02.7 — PrivateKeyFile credential authenticates using a local key file
            r7 = _call(s, "open_session", {
                "host": SSH_HOST, "port": SSH_PORT, "user": SSH_USER,
                "credentials": [{"type": "PrivateKeyFile", "path": key}],
                "connect_timeout_secs": 15,
            }, nid())
            sid7 = _session_id(r7)
            ok7 = bool(sid7) and not _is_io_failure(r7)
            print(f"[02.7] PrivateKeyFile credential: {'PASS' if ok7 else 'FAIL'} sid={sid7!r}")
            if not ok7:
                failures.append(f"02.7: PrivateKeyFile credential did not open session; result={r7}")
            else:
                _call(s, "close_session", {"session": sid7}, nid())

            # 02.8 — Agent credential authenticates using a key offered by the SSH agent
            agent_sock = os.environ.get("SSH_AUTH_SOCK")
            own_agent_pid = None
            if not agent_sock:
                try:
                    agent_out = subprocess.run(
                        ["ssh-agent", "-s"], capture_output=True, text=True, check=True,
                    ).stdout
                    for line in agent_out.splitlines():
                        if "SSH_AUTH_SOCK=" in line:
                            agent_sock = line.split("SSH_AUTH_SOCK=")[1].split(";")[0]
                        if "SSH_AGENT_PID=" in line:
                            own_agent_pid = int(line.split("SSH_AGENT_PID=")[1].split(";")[0])
                    if agent_sock:
                        subprocess.run(
                            ["ssh-add", key],
                            env={**os.environ, "SSH_AUTH_SOCK": agent_sock},
                            capture_output=True,
                        )
                except (subprocess.CalledProcessError, FileNotFoundError, ValueError):
                    pass

            if agent_sock:
                r8 = _call(s, "open_session", {
                    "host": SSH_HOST, "port": SSH_PORT, "user": SSH_USER,
                    "credentials": [{"type": "Agent", "socket_path": agent_sock}],
                    "connect_timeout_secs": 15,
                }, nid())
                sid8 = _session_id(r8)
                ok8 = bool(sid8) and not _is_io_failure(r8)
                print(f"[02.8] Agent credential: {'PASS' if ok8 else 'FAIL'} sid={sid8!r}")
                if not ok8:
                    failures.append(f"02.8: Agent credential did not open session; result={r8}")
                else:
                    _call(s, "close_session", {"session": sid8}, nid())
                if own_agent_pid is not None:
                    subprocess.run(
                        ["ssh-agent", "-k"],
                        env={**os.environ, "SSH_AGENT_PID": str(own_agent_pid)},
                        capture_output=True,
                    )
            else:
                print("[02.8] SKIP: no SSH agent available and ssh-agent could not be started")

        if failures:
            print("\nFAILURES:")
            for f in failures:
                print(f"  - {f}")
            return 1
        print("\nAll assertions passed.")
        return 0

    finally:
        if blackhole_srv is not None:
            try:
                blackhole_srv.close()
            except OSError:
                pass
        if sshd_proc is not None:
            sshd_proc.terminate()
            try:
                sshd_proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                sshd_proc.kill()
                sshd_proc.wait()
        if sshd_tmpdir is not None:
            shutil.rmtree(sshd_tmpdir, ignore_errors=True)
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


if __name__ == "__main__":
    sys.exit(main())
