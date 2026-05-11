#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises the closed-set error-category contract — req ID 04.1."""

from __future__ import annotations

import json, os, shutil, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TEST_DIR = Path("/home/ace/Desktop/prjx/kitchensync/tmp/testks/sftp-protocol-04")
HOST = "localhost"
SSH_PORT = 22
USER = "ace"
POOL_SETTINGS = {"mc": 2, "ct": 10, "ka": 30}


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


def _rpc(sock, method, params=None, rpc_id=1):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + 15
    while time.time() < deadline:
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, args, rid):
    resp = _rpc(sock, "tools/call", {"name": tool, "arguments": args}, rid)
    result = resp.get("result") or {}
    is_err = result.get("isError", False)
    for c in result.get("content", []):
        if c.get("type") == "text":
            try:
                parsed = json.loads(c["text"])
                if isinstance(parsed, dict) and is_err:
                    parsed.setdefault("_is_error", True)
                return parsed
            except json.JSONDecodeError:
                return {"_raw": c["text"], "_is_error": is_err}
    return {"_is_error": is_err}


def _is_not_found(r):
    if not isinstance(r, dict):
        return False
    for kind in ("not_found", "not found"):
        if (r.get("error") == kind
                or r.get(kind) is True
                or r.get("status") == kind
                or r.get("type") == kind
                or r.get("result") == kind):
            return True
    return False


def _is_permission_denied(r):
    if not isinstance(r, dict):
        return False
    for kind in ("permission_denied", "permission denied"):
        if (r.get("error") == kind
                or r.get(kind) is True
                or r.get("status") == kind
                or r.get("type") == kind
                or r.get("result") == kind):
            return True
    return False


def _setup():
    if TEST_DIR.exists():
        for p in sorted(TEST_DIR.rglob("*"), reverse=True):
            try:
                os.chmod(p, 0o755)
            except OSError:
                pass
        shutil.rmtree(TEST_DIR)
    TEST_DIR.mkdir(parents=True)
    noperm = TEST_DIR / "noperm.txt"
    noperm.write_bytes(b"secret")
    noperm.chmod(0o000)


def _teardown():
    if TEST_DIR.exists():
        for p in sorted(TEST_DIR.rglob("*"), reverse=True):
            try:
                os.chmod(p, 0o755)
            except OSError:
                pass
        shutil.rmtree(TEST_DIR, ignore_errors=True)


def main() -> int:
    _setup()
    proc, port = _launch()
    failures = []
    rid = 0

    def nid():
        nonlocal rid
        rid += 1
        return rid

    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            ep_r = _call(s, "open_endpoint", {
                "user": USER, "host": HOST, "port": SSH_PORT,
                "password": None,
                "settings": POOL_SETTINGS,
            }, nid())
            endpoint = (ep_r.get("endpoint") or ep_r.get("endpoint_id")
                        or ep_r.get("id") or ep_r.get("handle"))
            if not endpoint:
                print(f"[setup] FATAL: open_endpoint returned no handle: {ep_r}")
                failures.append("setup: open_endpoint returned no handle")
                return 1
            print(f"[setup] endpoint: {endpoint!r}")

            conn_r = _call(s, "acquire", {"endpoint": endpoint}, nid())
            conn = (conn_r.get("connection") or conn_r.get("connection_id")
                    or conn_r.get("id") or conn_r.get("handle"))
            if not conn:
                print(f"[setup] FATAL: acquire returned no connection: {conn_r}")
                failures.append("setup: acquire returned no connection")
                return 1
            print(f"[setup] connection: {conn!r}")

            missing_path = str(TEST_DIR / "nonexistent_04_xyz")
            noperm_path = str(TEST_DIR / "noperm.txt")

            # 04.1 — stat on missing path reports failure as "not found" category
            r_nf = _call(s, "stat", {"connection": conn, "path": missing_path}, nid())
            ok_nf = _is_not_found(r_nf)
            print(f"[04.1a] stat missing→not_found: {ok_nf}  result={r_nf}")
            if not ok_nf:
                failures.append("04.1: stat on non-existent path did not return 'not found' category")

            # 04.1 — open_read on no-permission file reports failure as "permission denied" category
            r_pd = _call(s, "open_read", {"connection": conn, "path": noperm_path}, nid())
            ok_pd = _is_permission_denied(r_pd)
            print(f"[04.1b] open_read denied→permission_denied: {ok_pd}  result={r_pd}")
            if not ok_pd:
                failures.append("04.1: open_read on no-permission file did not return 'permission denied' category")

            _call(s, "release", {"connection": conn}, nid())
            _call(s, "close_endpoint", {"endpoint": endpoint}, nid())

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
        _teardown()


if __name__ == "__main__":
    sys.exit(main())
