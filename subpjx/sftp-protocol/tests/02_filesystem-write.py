#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises chunked write, rename, delete, create_dir, and set_mod_time (reqs 02.30-02.36)."""

from __future__ import annotations

import base64, json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

SFTP_USER = "ace"
SFTP_HOST = "localhost"
TEST_ROOT = "/home/ace/Desktop/prjx/kitchensync/tmp/testks/sftp-write-02"


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
    deadline = time.time() + 30
    while time.time() < deadline:
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, args, rpc_id):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args}, rpc_id=rpc_id)


def _b64(data: bytes) -> str:
    return base64.b64encode(data).decode("ascii")


def _not_found(r: dict) -> bool:
    """True when the response signals a not-found condition."""
    if "error" in r:
        msg = r["error"].get("message", "").lower()
        return "not found" in msg or "no such" in msg
    if "result" in r:
        return r["result"].get("not_found", False) is True
    return False


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            tl = _rpc(s, "tools/list", rpc_id=rid); rid += 1
            tools = (tl.get("result") or {}).get("tools", [])
            print(f"[setup] tools/list returned {len(tools)} tool(s)")
            if not tools:
                failures.append("setup: tools/list empty")
                return 1

            r = _call(s, "open-endpoint", {
                "user": SFTP_USER, "host": SFTP_HOST, "mc": 4, "ct": 30, "ka": 60,
            }, rid); rid += 1
            if "error" in r:
                print(f"[setup] open-endpoint failed: {r['error']['message']}")
                failures.append("setup: open-endpoint failed")
                return 1
            endpoint_id = r["result"]["endpoint_id"]

            r = _call(s, "acquire", {"endpoint_id": endpoint_id}, rid); rid += 1
            if "error" in r:
                print(f"[setup] acquire failed: {r['error']['message']}")
                failures.append("setup: acquire failed")
                return 1
            conn_id = r["result"]["connection_id"]

            # Ensure test root exists (idempotent).
            _call(s, "create-dir", {"connection_id": conn_id, "path": TEST_ROOT}, rid); rid += 1

            # ── 02.30: open_write / write / close_write ───────────────────────────
            # Write two chunks; verify the finalised file is their concatenation.
            path_30 = f"{TEST_ROOT}/req30/hello.txt"
            _call(s, "delete-file", {"connection_id": conn_id, "path": path_30}, rid); rid += 1
            r_ow = _call(s, "open-write", {"connection_id": conn_id, "path": path_30}, rid); rid += 1
            ok_30 = "result" in r_ow and "handle_id" in r_ow.get("result", {})
            if ok_30:
                h = r_ow["result"]["handle_id"]
                r_w1 = _call(s, "write", {"handle_id": h, "data": _b64(b"hello ")}, rid); rid += 1
                r_w2 = _call(s, "write", {"handle_id": h, "data": _b64(b"world")}, rid); rid += 1
                r_cw = _call(s, "close-write", {"handle_id": h}, rid); rid += 1
                ok_30 = "result" in r_w1 and "result" in r_w2 and "result" in r_cw
            if ok_30:
                r_st = _call(s, "stat", {"connection_id": conn_id, "path": path_30}, rid); rid += 1
                ok_30 = (
                    "result" in r_st
                    and r_st["result"].get("byte_size") == 11
                    and r_st["result"].get("is_dir") is False
                )
            print(f"[02.30] open_write/write/close_write writes and finalizes file: {'PASS' if ok_30 else 'FAIL'}")
            if not ok_30:
                failures.append("02.30: chunked write did not produce the expected 11-byte file")

            # ── 02.31: open_write creates missing parent directories ───────────────
            path_31 = f"{TEST_ROOT}/req31/deep/nested/dir/file.txt"
            _call(s, "delete-file", {"connection_id": conn_id, "path": path_31}, rid); rid += 1
            r_ow = _call(s, "open-write", {"connection_id": conn_id, "path": path_31}, rid); rid += 1
            ok_31 = "result" in r_ow and "handle_id" in r_ow.get("result", {})
            if ok_31:
                h = r_ow["result"]["handle_id"]
                _call(s, "write", {"handle_id": h, "data": _b64(b"x")}, rid); rid += 1
                _call(s, "close-write", {"handle_id": h}, rid); rid += 1
                r_st = _call(s, "stat", {"connection_id": conn_id, "path": path_31}, rid); rid += 1
                ok_31 = (
                    "result" in r_st
                    and not _not_found(r_st)
                    and r_st["result"].get("is_dir") is False
                )
            print(f"[02.31] open_write creates missing parent directories: {'PASS' if ok_31 else 'FAIL'}")
            if not ok_31:
                failures.append("02.31: open_write did not create missing parent directories")

            # ── 02.32: rename moves src to dst ────────────────────────────────────
            path_32_src = f"{TEST_ROOT}/req32/source.txt"
            path_32_dst = f"{TEST_ROOT}/req32/dest.txt"
            _call(s, "delete-file", {"connection_id": conn_id, "path": path_32_src}, rid); rid += 1
            _call(s, "delete-file", {"connection_id": conn_id, "path": path_32_dst}, rid); rid += 1
            r_ow = _call(s, "open-write", {"connection_id": conn_id, "path": path_32_src}, rid); rid += 1
            if "result" in r_ow:
                h = r_ow["result"]["handle_id"]
                _call(s, "write", {"handle_id": h, "data": _b64(b"rename me")}, rid); rid += 1
                _call(s, "close-write", {"handle_id": h}, rid); rid += 1
            r_rn = _call(s, "rename", {
                "connection_id": conn_id, "src": path_32_src, "dst": path_32_dst,
            }, rid); rid += 1
            ok_32 = "result" in r_rn
            if ok_32:
                r_dst = _call(s, "stat", {"connection_id": conn_id, "path": path_32_dst}, rid); rid += 1
                r_src = _call(s, "stat", {"connection_id": conn_id, "path": path_32_src}, rid); rid += 1
                ok_32 = (
                    "result" in r_dst and not _not_found(r_dst)
                    and _not_found(r_src)
                )
            print(f"[02.32] rename moves entry from src to dst: {'PASS' if ok_32 else 'FAIL'}")
            if not ok_32:
                failures.append("02.32: rename did not move src to dst")

            # ── 02.33: delete_file removes a regular file ─────────────────────────
            path_33 = f"{TEST_ROOT}/req33/todelete.txt"
            _call(s, "delete-file", {"connection_id": conn_id, "path": path_33}, rid); rid += 1
            r_ow = _call(s, "open-write", {"connection_id": conn_id, "path": path_33}, rid); rid += 1
            if "result" in r_ow:
                h = r_ow["result"]["handle_id"]
                _call(s, "write", {"handle_id": h, "data": _b64(b"bye")}, rid); rid += 1
                _call(s, "close-write", {"handle_id": h}, rid); rid += 1
            r_del = _call(s, "delete-file", {"connection_id": conn_id, "path": path_33}, rid); rid += 1
            ok_33 = "result" in r_del
            if ok_33:
                r_st = _call(s, "stat", {"connection_id": conn_id, "path": path_33}, rid); rid += 1
                ok_33 = _not_found(r_st)
            print(f"[02.33] delete_file removes the file: {'PASS' if ok_33 else 'FAIL'}")
            if not ok_33:
                failures.append("02.33: delete_file did not remove the file")

            # ── 02.34: delete_dir removes an empty directory ──────────────────────
            path_34 = f"{TEST_ROOT}/req34/emptydir"
            # create-dir is idempotent; ensures the dir exists regardless of prior state.
            _call(s, "create-dir", {"connection_id": conn_id, "path": path_34}, rid); rid += 1
            r_del = _call(s, "delete-dir", {"connection_id": conn_id, "path": path_34}, rid); rid += 1
            ok_34 = "result" in r_del
            if ok_34:
                r_st = _call(s, "stat", {"connection_id": conn_id, "path": path_34}, rid); rid += 1
                ok_34 = _not_found(r_st)
            print(f"[02.34] delete_dir removes an empty directory: {'PASS' if ok_34 else 'FAIL'}")
            if not ok_34:
                failures.append("02.34: delete_dir did not remove the empty directory")

            # ── 02.35: create_dir creates a directory and any missing parents ──────
            path_35 = f"{TEST_ROOT}/req35/a/b/c/d"
            # Delete leaf to ensure it's freshly created by the call under test.
            _call(s, "delete-dir", {"connection_id": conn_id, "path": path_35}, rid); rid += 1
            r_mk = _call(s, "create-dir", {"connection_id": conn_id, "path": path_35}, rid); rid += 1
            ok_35 = "result" in r_mk
            if ok_35:
                r_st = _call(s, "stat", {"connection_id": conn_id, "path": path_35}, rid); rid += 1
                ok_35 = "result" in r_st and r_st["result"].get("is_dir") is True
            print(f"[02.35] create_dir creates directory and all missing parents: {'PASS' if ok_35 else 'FAIL'}")
            if not ok_35:
                failures.append("02.35: create_dir did not create the directory with missing parents")

            # ── 02.36: set_mod_time sets the modification time ────────────────────
            path_36 = f"{TEST_ROOT}/req36/timestamped.txt"
            _call(s, "delete-file", {"connection_id": conn_id, "path": path_36}, rid); rid += 1
            r_ow = _call(s, "open-write", {"connection_id": conn_id, "path": path_36}, rid); rid += 1
            ok_36 = "result" in r_ow
            if ok_36:
                h = r_ow["result"]["handle_id"]
                _call(s, "write", {"handle_id": h, "data": _b64(b"t")}, rid); rid += 1
                _call(s, "close-write", {"handle_id": h}, rid); rid += 1
                target_mtime = 1_700_000_000
                r_smt = _call(s, "set-mod-time", {
                    "connection_id": conn_id, "path": path_36, "mod_time": target_mtime,
                }, rid); rid += 1
                ok_36 = "result" in r_smt
                if ok_36:
                    r_st = _call(s, "stat", {"connection_id": conn_id, "path": path_36}, rid); rid += 1
                    ok_36 = (
                        "result" in r_st
                        and r_st["result"].get("mod_time") == target_mtime
                    )
            print(f"[02.36] set_mod_time sets file modification time: {'PASS' if ok_36 else 'FAIL'}")
            if not ok_36:
                failures.append("02.36: set_mod_time did not set the modification time")

            _call(s, "release", {"connection_id": conn_id}, rid); rid += 1
            _call(s, "close-endpoint", {"endpoint_id": endpoint_id}, rid); rid += 1

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


if __name__ == "__main__":
    sys.exit(main())
