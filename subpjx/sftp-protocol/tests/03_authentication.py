#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Auth fallback chain (password → agent → id_ed25519 → id_ecdsa → id_rsa) and host-key verification (03.1–03.7)."""

from __future__ import annotations

import json, os, select, shutil, socket, subprocess, sys, tempfile, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

SSH_HOST = "localhost"
SSH_PORT = 22
SSH_USER = "ace"
DEFAULT_SETTINGS = {"mc": 5, "ct": 15, "ka": 30}


def _drain(stream):
    for _ in stream:
        pass


def _launch(env=None):
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
        env=env if env is not None else dict(os.environ),
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


def _rpc(sock, method, params, rid, timeout=30):
    msg = {"jsonrpc": "2.0", "id": rid, "method": method}
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
        return {"_is_error": True, "_raw": resp}
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


def _endpoint_id(r):
    return r.get("endpoint") or r.get("endpoint_id") or r.get("handle") or r.get("id") or ""


def _connection_id(r):
    return r.get("connection") or r.get("connection_id") or r.get("handle") or r.get("id") or ""


def _is_io_error(r):
    if not isinstance(r, dict):
        return False
    if r.get("_is_error"):
        return True
    err = str(r.get("error", "")).lower()
    status = str(r.get("status", "")).lower()
    return "io" in err or "fail" in err or "error" in err or "io" in status or "fail" in status


def _terminate(proc):
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()


def _start_sshd_unknown_key():
    tmpdir = Path(tempfile.mkdtemp())
    subprocess.run(
        ["ssh-keygen", "-t", "ed25519", "-f", str(tmpdir / "host_key"), "-N", ""],
        check=True, capture_output=True,
    )
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        sshd_port = probe.getsockname()[1]
    config = "\n".join([
        f"Port {sshd_port}",
        "ListenAddress 127.0.0.1",
        f"HostKey {tmpdir}/host_key",
        "AuthorizedKeysFile none",
        "PasswordAuthentication no",
        "ChallengeResponseAuthentication no",
        "UsePAM no",
        "StrictModes no",
        "LogLevel ERROR",
        f"PidFile {tmpdir}/sshd.pid",
    ])
    (tmpdir / "sshd_config").write_text(config)
    sshd_bin = shutil.which("sshd") or "/usr/sbin/sshd"
    sshd_proc = subprocess.Popen(
        [sshd_bin, "-D", "-e", "-f", str(tmpdir / "sshd_config")],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    deadline = time.time() + 5
    while time.time() < deadline:
        time.sleep(0.1)
        if sshd_proc.poll() is not None:
            err = sshd_proc.stderr.read().decode(errors="replace")
            raise RuntimeError(f"sshd exited early: {err[:200]}")
        try:
            with socket.create_connection(("127.0.0.1", sshd_port), timeout=0.5):
                break
        except OSError:
            pass
    else:
        sshd_proc.terminate()
        raise RuntimeError("sshd did not start in time")
    return sshd_proc, sshd_port, tmpdir


def main() -> int:
    failures = []
    _rid = [0]

    def rid():
        _rid[0] += 1
        return _rid[0]

    ssh_home = Path.home() / ".ssh"
    key_ed25519 = ssh_home / "id_ed25519"
    key_ecdsa = ssh_home / "id_ecdsa"
    key_rsa = ssh_home / "id_rsa"

    # Env without SSH agent for key-file auth tests.
    env_no_agent = {k: v for k, v in os.environ.items() if k != "SSH_AUTH_SOCK"}

    proc1 = None
    sshd_proc = None
    sshd_tmpdir = None

    try:
        proc1, port1 = _launch(env=env_no_agent)

        with socket.create_connection(("127.0.0.1", port1), timeout=10) as s1:

            # 03.1 — inline password is tried first
            test_password = os.environ.get("SFTP_TEST_PASSWORD")
            if test_password:
                r = _call(s1, "open-endpoint", {
                    "user": SSH_USER, "host": SSH_HOST, "port": SSH_PORT,
                    "password": test_password, "settings": DEFAULT_SETTINGS,
                }, rid())
                ep = _endpoint_id(r)
                if ep:
                    r_acq = _call(s1, "acquire", {"endpoint": ep}, rid(), timeout=20)
                    conn = _connection_id(r_acq)
                    ok = bool(conn) and not _is_io_error(r_acq)
                    print(f"[03.1] inline password → acquire: {'PASS' if ok else 'FAIL'} conn={conn!r}")
                    if not ok:
                        failures.append(f"03.1: inline password auth failed; result={r_acq}")
                    if conn:
                        _call(s1, "release", {"connection": conn}, rid())
                    _call(s1, "close-endpoint", {"endpoint": ep}, rid())
                else:
                    failures.append(f"03.1: open-endpoint failed; result={r}")
                    print(f"[03.1] FAIL open-endpoint: {r}")
            else:
                print("[03.1] SKIP: SFTP_TEST_PASSWORD not set")

            # 03.3 — id_ed25519 tried after agent (no agent, no password)
            if key_ed25519.exists():
                r = _call(s1, "open-endpoint", {
                    "user": SSH_USER, "host": SSH_HOST, "port": SSH_PORT,
                    "password": None, "settings": DEFAULT_SETTINGS,
                }, rid())
                ep = _endpoint_id(r)
                if ep:
                    r_acq = _call(s1, "acquire", {"endpoint": ep}, rid(), timeout=20)
                    conn = _connection_id(r_acq)
                    ok = bool(conn) and not _is_io_error(r_acq)
                    print(f"[03.3] id_ed25519 auth (no agent, no password) → {'PASS' if ok else 'FAIL'}")
                    if not ok:
                        failures.append(f"03.3: id_ed25519 auth failed; result={r_acq}")
                    if conn:
                        _call(s1, "release", {"connection": conn}, rid())
                    _call(s1, "close-endpoint", {"endpoint": ep}, rid())
                else:
                    failures.append(f"03.3: open-endpoint failed; result={r}")
                    print(f"[03.3] FAIL open-endpoint: {r}")
            else:
                print("[03.3] SKIP: ~/.ssh/id_ed25519 not present")

            # 03.4 — id_ecdsa tried after id_ed25519 is unavailable
            # Only independently verifiable when id_ed25519 is absent.
            if key_ecdsa.exists() and not key_ed25519.exists():
                r = _call(s1, "open-endpoint", {
                    "user": SSH_USER, "host": SSH_HOST, "port": SSH_PORT,
                    "password": None, "settings": DEFAULT_SETTINGS,
                }, rid())
                ep = _endpoint_id(r)
                if ep:
                    r_acq = _call(s1, "acquire", {"endpoint": ep}, rid(), timeout=20)
                    conn = _connection_id(r_acq)
                    ok = bool(conn) and not _is_io_error(r_acq)
                    print(f"[03.4] id_ecdsa auth (no agent, no ed25519) → {'PASS' if ok else 'FAIL'}")
                    if not ok:
                        failures.append(f"03.4: id_ecdsa auth failed; result={r_acq}")
                    if conn:
                        _call(s1, "release", {"connection": conn}, rid())
                    _call(s1, "close-endpoint", {"endpoint": ep}, rid())
                else:
                    failures.append(f"03.4: open-endpoint failed; result={r}")
            else:
                print("[03.4] SKIP: id_ecdsa not present or id_ed25519 present (can't isolate)")

            # 03.5 — id_rsa tried after id_ecdsa is unavailable
            # Only independently verifiable when both id_ed25519 and id_ecdsa are absent.
            if key_rsa.exists() and not key_ed25519.exists() and not key_ecdsa.exists():
                r = _call(s1, "open-endpoint", {
                    "user": SSH_USER, "host": SSH_HOST, "port": SSH_PORT,
                    "password": None, "settings": DEFAULT_SETTINGS,
                }, rid())
                ep = _endpoint_id(r)
                if ep:
                    r_acq = _call(s1, "acquire", {"endpoint": ep}, rid(), timeout=20)
                    conn = _connection_id(r_acq)
                    ok = bool(conn) and not _is_io_error(r_acq)
                    print(f"[03.5] id_rsa auth (no agent, no ed25519, no ecdsa) → {'PASS' if ok else 'FAIL'}")
                    if not ok:
                        failures.append(f"03.5: id_rsa auth failed; result={r_acq}")
                    if conn:
                        _call(s1, "release", {"connection": conn}, rid())
                    _call(s1, "close-endpoint", {"endpoint": ep}, rid())
                else:
                    failures.append(f"03.5: open-endpoint failed; result={r}")
            else:
                print("[03.5] SKIP: id_rsa not present or earlier key present (can't isolate)")

            # 03.6 — auth stops at first success and connection proceeds
            # Verify by acquiring a connection and performing a stat on the remote home dir.
            first_key = next((k for k in [key_ed25519, key_ecdsa, key_rsa] if k.exists()), None)
            if first_key:
                r = _call(s1, "open-endpoint", {
                    "user": SSH_USER, "host": SSH_HOST, "port": SSH_PORT,
                    "password": None, "settings": DEFAULT_SETTINGS,
                }, rid())
                ep = _endpoint_id(r)
                if ep:
                    r_acq = _call(s1, "acquire", {"endpoint": ep}, rid(), timeout=20)
                    conn = _connection_id(r_acq)
                    if conn and not _is_io_error(r_acq):
                        r_stat = _call(s1, "stat", {
                            "connection": conn, "path": f"/home/{SSH_USER}",
                        }, rid(), timeout=10)
                        ok = not _is_io_error(r_stat) and ("is_dir" in r_stat or "mod_time" in r_stat or "modTime" in r_stat)
                        print(f"[03.6] auth stops at first success; connection proceeds → {'PASS' if ok else 'FAIL'} stat={r_stat}")
                        if not ok:
                            failures.append(f"03.6: connection not usable after auth; stat result={r_stat}")
                        _call(s1, "release", {"connection": conn}, rid())
                    else:
                        failures.append(f"03.6: acquire failed; result={r_acq}")
                        print(f"[03.6] FAIL acquire: {r_acq}")
                    _call(s1, "close-endpoint", {"endpoint": ep}, rid())
                else:
                    failures.append(f"03.6: open-endpoint failed; result={r}")
                    print(f"[03.6] FAIL open-endpoint: {r}")
            else:
                print("[03.6] SKIP: no key file available to test")

            # 03.7 — unknown host key causes connection failure as I/O error
            try:
                sshd_proc, sshd_port, sshd_tmpdir = _start_sshd_unknown_key()
                r = _call(s1, "open-endpoint", {
                    "user": SSH_USER, "host": "127.0.0.1", "port": sshd_port,
                    "password": None, "settings": DEFAULT_SETTINGS,
                }, rid())
                ep = _endpoint_id(r)
                if ep:
                    r_acq = _call(s1, "acquire", {"endpoint": ep}, rid(), timeout=20)
                    ok = _is_io_error(r_acq)
                    print(f"[03.7] unknown host key → io_error: {'PASS' if ok else 'FAIL'} result={r_acq}")
                    if not ok:
                        failures.append(f"03.7: acquire on unknown-key host did not fail with io_error; result={r_acq}")
                    _call(s1, "close-endpoint", {"endpoint": ep}, rid())
                else:
                    ok = _is_io_error(r)
                    print(f"[03.7] unknown host key → io_error (at open-endpoint): {'PASS' if ok else 'FAIL'} result={r}")
                    if not ok:
                        failures.append(f"03.7: neither open-endpoint nor acquire rejected unknown-key host; result={r}")
            except RuntimeError as exc:
                print(f"[03.7] SKIP: could not start fresh sshd ({exc})")
            finally:
                if sshd_proc is not None:
                    _terminate(sshd_proc)
                    sshd_proc = None
                if sshd_tmpdir is not None:
                    shutil.rmtree(sshd_tmpdir, ignore_errors=True)
                    sshd_tmpdir = None

    finally:
        if proc1 is not None:
            _terminate(proc1)
            proc1 = None

    # 03.2 — SSH agent at $SSH_AUTH_SOCK tried after password (no inline password, agent has key)
    # Launch a separate MCP server with SSH_AUTH_SOCK pointing to a fresh agent.
    real_key = next((k for k in [key_ed25519, key_ecdsa, key_rsa] if k.exists()), None)
    agent_sock = None
    agent_pid = None
    proc2 = None

    if real_key is None:
        print("[03.2] SKIP: no key file to load into agent")
    else:
        try:
            agent_out = subprocess.run(
                ["ssh-agent", "-s"], capture_output=True, text=True, check=True,
            ).stdout
            for line in agent_out.splitlines():
                if "SSH_AUTH_SOCK=" in line:
                    agent_sock = line.split("SSH_AUTH_SOCK=")[1].split(";")[0]
                if "SSH_AGENT_PID=" in line:
                    try:
                        agent_pid = int(line.split("SSH_AGENT_PID=")[1].split(";")[0])
                    except ValueError:
                        pass
        except (subprocess.CalledProcessError, FileNotFoundError):
            pass

        if agent_sock:
            subprocess.run(
                ["ssh-add", str(real_key)],
                env={**os.environ, "SSH_AUTH_SOCK": agent_sock},
                capture_output=True,
            )
            env_with_agent = {k: v for k, v in os.environ.items() if k != "SSH_AUTH_SOCK"}
            env_with_agent["SSH_AUTH_SOCK"] = agent_sock
            try:
                proc2, port2 = _launch(env=env_with_agent)
                with socket.create_connection(("127.0.0.1", port2), timeout=10) as s2:
                    r = _call(s2, "open-endpoint", {
                        "user": SSH_USER, "host": SSH_HOST, "port": SSH_PORT,
                        "password": None, "settings": DEFAULT_SETTINGS,
                    }, rid())
                    ep = _endpoint_id(r)
                    if ep:
                        r_acq = _call(s2, "acquire", {"endpoint": ep}, rid(), timeout=20)
                        conn = _connection_id(r_acq)
                        ok = bool(conn) and not _is_io_error(r_acq)
                        print(f"[03.2] SSH agent auth (no inline password, agent has key) → {'PASS' if ok else 'FAIL'}")
                        if not ok:
                            failures.append(f"03.2: agent auth failed; result={r_acq}")
                        if conn:
                            _call(s2, "release", {"connection": conn}, rid())
                        _call(s2, "close-endpoint", {"endpoint": ep}, rid())
                    else:
                        failures.append(f"03.2: open-endpoint failed; result={r}")
                        print(f"[03.2] FAIL open-endpoint: {r}")
            finally:
                if proc2 is not None:
                    _terminate(proc2)
        else:
            print("[03.2] SKIP: could not start ssh-agent")

        if agent_pid is not None:
            subprocess.run(
                ["ssh-agent", "-k"],
                env={**os.environ, "SSH_AGENT_PID": str(agent_pid)},
                capture_output=True,
            )

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
