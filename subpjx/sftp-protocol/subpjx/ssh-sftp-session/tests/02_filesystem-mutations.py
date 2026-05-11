#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises filesystem-mutation operations: rename, delete_file, delete_dir, create_dir, set_mod_time."""

from __future__ import annotations

import json, os, shutil, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TEST_ROOT = Path("/home/ace/Desktop/prjx/kitchensync/tmp/testks/02-mutations")
SSH_HOST = "localhost"
SSH_PORT = 22
SSH_USER = "ace"


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


def _rpc(sock, method, params, rpc_id):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method, "params": params}
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + 30
    while time.time() < deadline:
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool_name, arguments, rpc_id):
    return _rpc(sock, "tools/call", {"name": tool_name, "arguments": arguments}, rpc_id)


def _credentials():
    creds = []
    auth_sock = os.environ.get("SSH_AUTH_SOCK")
    if auth_sock:
        creds.append({"type": "Agent", "socket_path": auth_sock})
    for key_name in ("id_ed25519", "id_rsa", "id_ecdsa"):
        p = Path.home() / ".ssh" / key_name
        if p.exists():
            creds.append({"type": "PrivateKeyFile", "path": str(p)})
    return creds


def _is_not_found(resp):
    if resp.get("error"):
        msg = str(resp["error"].get("message", "")).lower()
        return "not_found" in msg or "no such" in msg or "no_such" in msg
    result = resp.get("result") or {}
    if isinstance(result, dict):
        return result.get("not_found") is True or result.get("type") == "not_found"
    return False


def _stat_mod_time(resp):
    result = resp.get("result") or {}
    if isinstance(result, dict):
        return result.get("mod_time")
    return None


def _stat_is_dir(resp):
    result = resp.get("result") or {}
    if isinstance(result, dict):
        return result.get("is_dir") is True
    return False


def main() -> int:
    if TEST_ROOT.exists():
        shutil.rmtree(TEST_ROOT)
    TEST_ROOT.mkdir(parents=True, exist_ok=True)

    proc, port = _launch()
    failures = []
    rpc_id = 1

    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            creds = _credentials()

            resp = _call(s, "open_session", {
                "host": SSH_HOST,
                "port": SSH_PORT,
                "user": SSH_USER,
                "credentials": creds,
                "connect_timeout_secs": 30,
            }, rpc_id)
            rpc_id += 1

            result = resp.get("result") or {}
            session = result.get("session") or result.get("session_id") or result.get("id")
            if resp.get("error") or session is None:
                print(f"[setup] open_session FAILED: {resp}")
                failures.append("setup: open_session failed")
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1
            print(f"[setup] open_session OK")

            # --- 02.25: rename moves src to dst ---
            src_path = str(TEST_ROOT / "rename_src.txt")
            dst_path = str(TEST_ROOT / "rename_dst.txt")
            Path(src_path).write_text("content")

            resp = _call(s, "rename", {"session": session, "src": src_path, "dst": dst_path}, rpc_id)
            rpc_id += 1
            if resp.get("error"):
                failures.append(f"02.25: rename error: {resp['error']}")
                print(f"[02.25] FAIL: rename error: {resp['error']}")
            else:
                stat_src = _call(s, "stat", {"session": session, "path": src_path}, rpc_id)
                rpc_id += 1
                stat_dst = _call(s, "stat", {"session": session, "path": dst_path}, rpc_id)
                rpc_id += 1
                if not _is_not_found(stat_src):
                    failures.append(f"02.25: src still accessible after rename: {stat_src}")
                    print(f"[02.25] FAIL: src still accessible after rename")
                elif _stat_mod_time(stat_dst) is None:
                    failures.append(f"02.25: dst not accessible after rename: {stat_dst}")
                    print(f"[02.25] FAIL: dst not accessible after rename")
                else:
                    print(f"[02.25] PASS: rename moved src to dst")

            # --- 02.26: delete_file removes a regular file ---
            file_path = str(TEST_ROOT / "to_delete.txt")
            Path(file_path).write_text("delete me")

            resp = _call(s, "delete_file", {"session": session, "path": file_path}, rpc_id)
            rpc_id += 1
            if resp.get("error"):
                failures.append(f"02.26: delete_file error: {resp['error']}")
                print(f"[02.26] FAIL: delete_file error: {resp['error']}")
            else:
                stat_resp = _call(s, "stat", {"session": session, "path": file_path}, rpc_id)
                rpc_id += 1
                if not _is_not_found(stat_resp):
                    failures.append(f"02.26: file still accessible after delete_file: {stat_resp}")
                    print(f"[02.26] FAIL: file still accessible after delete_file")
                else:
                    print(f"[02.26] PASS: delete_file removed the file")

            # --- 02.27: delete_dir removes an empty directory ---
            dir_path = str(TEST_ROOT / "empty_dir")
            Path(dir_path).mkdir()

            resp = _call(s, "delete_dir", {"session": session, "path": dir_path}, rpc_id)
            rpc_id += 1
            if resp.get("error"):
                failures.append(f"02.27: delete_dir error: {resp['error']}")
                print(f"[02.27] FAIL: delete_dir error: {resp['error']}")
            else:
                stat_resp = _call(s, "stat", {"session": session, "path": dir_path}, rpc_id)
                rpc_id += 1
                if not _is_not_found(stat_resp):
                    failures.append(f"02.27: dir still accessible after delete_dir: {stat_resp}")
                    print(f"[02.27] FAIL: dir still accessible after delete_dir")
                else:
                    print(f"[02.27] PASS: delete_dir removed the directory")

            # --- 02.28: create_dir creates a directory ---
            new_dir = str(TEST_ROOT / "new_dir")

            resp = _call(s, "create_dir", {"session": session, "path": new_dir}, rpc_id)
            rpc_id += 1
            if resp.get("error"):
                failures.append(f"02.28: create_dir error: {resp['error']}")
                print(f"[02.28] FAIL: create_dir error: {resp['error']}")
            else:
                stat_resp = _call(s, "stat", {"session": session, "path": new_dir}, rpc_id)
                rpc_id += 1
                if not _stat_is_dir(stat_resp):
                    failures.append(f"02.28: directory not accessible after create_dir: {stat_resp}")
                    print(f"[02.28] FAIL: directory not accessible after create_dir")
                else:
                    print(f"[02.28] PASS: create_dir created the directory")

            # --- 02.29: create_dir creates missing parent directories ---
            deep_dir = str(TEST_ROOT / "a" / "b" / "c")

            resp = _call(s, "create_dir", {"session": session, "path": deep_dir}, rpc_id)
            rpc_id += 1
            if resp.get("error"):
                failures.append(f"02.29: create_dir (deep) error: {resp['error']}")
                print(f"[02.29] FAIL: create_dir deep error: {resp['error']}")
            else:
                stat_resp = _call(s, "stat", {"session": session, "path": deep_dir}, rpc_id)
                rpc_id += 1
                if not _stat_is_dir(stat_resp):
                    failures.append(f"02.29: deep directory not accessible after create_dir: {stat_resp}")
                    print(f"[02.29] FAIL: deep directory not accessible after create_dir")
                else:
                    print(f"[02.29] PASS: create_dir created directory with missing parents")

            # --- 02.30: set_mod_time updates mod_time on a regular file ---
            mtime_file = str(TEST_ROOT / "mtime_file.txt")
            Path(mtime_file).write_text("mtime test")
            target_mtime = 946684800  # 2000-01-01 00:00:00 UTC

            resp = _call(s, "set_mod_time", {"session": session, "path": mtime_file, "time": target_mtime}, rpc_id)
            rpc_id += 1
            if resp.get("error"):
                failures.append(f"02.30: set_mod_time error: {resp['error']}")
                print(f"[02.30] FAIL: set_mod_time error: {resp['error']}")
            else:
                stat_resp = _call(s, "stat", {"session": session, "path": mtime_file}, rpc_id)
                rpc_id += 1
                mod_time = _stat_mod_time(stat_resp)
                if mod_time is None:
                    failures.append(f"02.30: stat returned no mod_time: {stat_resp}")
                    print(f"[02.30] FAIL: stat returned no mod_time after set_mod_time")
                elif abs(int(mod_time) - target_mtime) > 5:
                    failures.append(f"02.30: mod_time mismatch: got {mod_time}, expected {target_mtime}")
                    print(f"[02.30] FAIL: mod_time mismatch: got {mod_time}, expected {target_mtime}")
                else:
                    print(f"[02.30] PASS: set_mod_time updated file mod_time")

            # --- 02.31: set_mod_time updates mod_time on a directory ---
            mtime_dir = str(TEST_ROOT / "mtime_dir")
            Path(mtime_dir).mkdir(exist_ok=True)
            target_mtime_dir = 978307200  # 2001-01-01 00:00:00 UTC

            resp = _call(s, "set_mod_time", {"session": session, "path": mtime_dir, "time": target_mtime_dir}, rpc_id)
            rpc_id += 1
            if resp.get("error"):
                failures.append(f"02.31: set_mod_time (dir) error: {resp['error']}")
                print(f"[02.31] FAIL: set_mod_time dir error: {resp['error']}")
            else:
                stat_resp = _call(s, "stat", {"session": session, "path": mtime_dir}, rpc_id)
                rpc_id += 1
                mod_time = _stat_mod_time(stat_resp)
                if mod_time is None:
                    failures.append(f"02.31: stat returned no mod_time for dir: {stat_resp}")
                    print(f"[02.31] FAIL: stat returned no mod_time for dir after set_mod_time")
                elif abs(int(mod_time) - target_mtime_dir) > 5:
                    failures.append(f"02.31: dir mod_time mismatch: got {mod_time}, expected {target_mtime_dir}")
                    print(f"[02.31] FAIL: dir mod_time mismatch: got {mod_time}, expected {target_mtime_dir}")
                else:
                    print(f"[02.31] PASS: set_mod_time updated directory mod_time")

            _call(s, "close_session", {"session": session}, rpc_id)
            rpc_id += 1
            print(f"[teardown] close_session OK")

    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
