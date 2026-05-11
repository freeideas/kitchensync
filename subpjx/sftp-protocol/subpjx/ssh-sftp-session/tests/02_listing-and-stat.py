#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises list_dir and stat — req IDs 02.9–02.18, 02.32."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TEST_DIR = Path("/home/ace/Desktop/prjx/kitchensync/tmp/testks/02-listing-stat")
HOST, PORT, USER = "localhost", 22, "ace"


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


def _is_err(result, kind):
    if not isinstance(result, dict):
        return False
    return (result.get("error") == kind
            or result.get(kind) is True
            or result.get("status") == kind
            or result.get("type") == kind
            or result.get("result") == kind)


def _setup():
    import shutil
    if TEST_DIR.exists():
        no_perm = TEST_DIR / "no_perm_dir"
        if no_perm.exists():
            os.chmod(no_perm, 0o755)
        shutil.rmtree(TEST_DIR)
    TEST_DIR.mkdir(parents=True)
    (TEST_DIR / "regular_file.txt").write_text("hello sftp\n")
    (TEST_DIR / "subdir").mkdir()
    (TEST_DIR / "symlink_to_file").symlink_to("regular_file.txt")
    os.mkfifo(TEST_DIR / "named_pipe")
    (TEST_DIR / "no_perm_dir").mkdir()
    os.chmod(TEST_DIR / "no_perm_dir", 0o000)


def _teardown():
    import shutil
    if TEST_DIR.exists():
        no_perm = TEST_DIR / "no_perm_dir"
        if no_perm.exists():
            os.chmod(no_perm, 0o755)
        shutil.rmtree(TEST_DIR, ignore_errors=True)


def main() -> int:
    _setup()
    key = _find_key()
    proc, port = _launch()
    failures = []
    rid = 0

    def nid():
        nonlocal rid
        rid += 1
        return rid

    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            open_r = _call(s, "open_session", {
                "host": HOST, "port": PORT, "user": USER,
                "credentials": [{"type": "PrivateKeyFile", "path": key}],
                "connect_timeout_secs": 10,
            }, nid())
            sid = (open_r.get("session_id") or open_r.get("session")
                   or open_r.get("id") or open_r.get("handle"))
            if not sid:
                print("[open] FATAL: open_session returned no session id")
                failures.append("open_session: no session id in response")
                return 1
            print(f"[open] session opened: {sid}")

            tdir = str(TEST_DIR)
            fpath = str(TEST_DIR / "regular_file.txt")
            dpath = str(TEST_DIR / "subdir")
            lpath = str(TEST_DIR / "symlink_to_file")
            ppath = str(TEST_DIR / "named_pipe")
            npath = str(TEST_DIR / "nonexistent")
            npm_path = str(TEST_DIR / "no_perm_dir")

            # 02.9 — list_dir on existing directory returns immediate children
            r9 = _call(s, "list_dir", {"session": sid, "path": tdir}, nid())
            entries = ((r9.get("entries") if isinstance(r9, dict) else None)
                       or (r9 if isinstance(r9, list) else []))
            names = {e.get("name") for e in entries if isinstance(e, dict)}
            ok9 = "regular_file.txt" in names and "subdir" in names
            print(f"[02.9] list_dir returns children: {ok9} names={sorted(names)}")
            if not ok9:
                failures.append("02.9: list_dir did not return expected children")

            # 02.10 — each entry includes name, is_dir, mod_time, byte_size
            file_entry = next(
                (e for e in entries if isinstance(e, dict) and e.get("name") == "regular_file.txt"),
                None,
            )
            ok10 = (file_entry is not None
                    and all(k in file_entry for k in ("name", "is_dir", "mod_time", "byte_size")))
            print(f"[02.10] entry has required fields: {ok10} entry={file_entry}")
            if not ok10:
                failures.append("02.10: file entry missing name/is_dir/mod_time/byte_size")

            # 02.11 — directory entries report byte_size as -1
            dir_entry = next(
                (e for e in entries if isinstance(e, dict) and e.get("name") == "subdir"),
                None,
            )
            ok11 = dir_entry is not None and dir_entry.get("byte_size") == -1
            print(f"[02.11] subdir byte_size==-1: {ok11} entry={dir_entry}")
            if not ok11:
                failures.append("02.11: directory entry byte_size is not -1")

            # 02.12 — non-regular entries (symlinks, FIFOs) are omitted
            ok12 = "symlink_to_file" not in names and "named_pipe" not in names
            print(f"[02.12] non-regular entries omitted: {ok12} names={sorted(names)}")
            if not ok12:
                failures.append("02.12: list_dir included a symlink or FIFO")

            # 02.13 — list_dir on missing path returns not_found
            r13 = _call(s, "list_dir", {"session": sid, "path": npath}, nid())
            ok13 = _is_err(r13, "not_found")
            print(f"[02.13] list_dir missing→not_found: {ok13} result={r13}")
            if not ok13:
                failures.append("02.13: list_dir on missing path did not return not_found")

            # 02.14 — list_dir on unreadable directory returns permission_denied
            r14 = _call(s, "list_dir", {"session": sid, "path": npm_path}, nid())
            ok14 = _is_err(r14, "permission_denied")
            print(f"[02.14] list_dir no-perm→permission_denied: {ok14} result={r14}")
            if not ok14:
                failures.append("02.14: list_dir on no-perm dir did not return permission_denied")

            # 02.15 — stat on regular file returns mod_time, byte_size, is_dir=false
            r15 = _call(s, "stat", {"session": sid, "path": fpath}, nid())
            ok15 = (isinstance(r15, dict)
                    and not _is_err(r15, "not_found")
                    and r15.get("mod_time") is not None
                    and r15.get("byte_size") == 11
                    and r15.get("is_dir") is False)
            print(f"[02.15] stat file→metadata+is_dir=false: {ok15} result={r15}")
            if not ok15:
                failures.append("02.15: stat on regular file did not return expected fields")

            # 02.16 — stat on directory returns is_dir=true
            r16 = _call(s, "stat", {"session": sid, "path": dpath}, nid())
            ok16 = isinstance(r16, dict) and r16.get("is_dir") is True
            print(f"[02.16] stat dir→is_dir=true: {ok16} result={r16}")
            if not ok16:
                failures.append("02.16: stat on directory did not return is_dir=true")

            # 02.17 — stat on missing path returns not_found
            r17 = _call(s, "stat", {"session": sid, "path": npath}, nid())
            ok17 = _is_err(r17, "not_found")
            print(f"[02.17] stat missing→not_found: {ok17} result={r17}")
            if not ok17:
                failures.append("02.17: stat on missing path did not return not_found")

            # 02.18 — stat on symbolic link returns not_found
            r18 = _call(s, "stat", {"session": sid, "path": lpath}, nid())
            ok18 = _is_err(r18, "not_found")
            print(f"[02.18] stat symlink→not_found: {ok18} result={r18}")
            if not ok18:
                failures.append("02.18: stat on symbolic link did not return not_found")

            # 02.32 — stat on FIFO (non-regular special file) returns not_found
            r32 = _call(s, "stat", {"session": sid, "path": ppath}, nid())
            ok32 = _is_err(r32, "not_found")
            print(f"[02.32] stat fifo→not_found: {ok32} result={r32}")
            if not ok32:
                failures.append("02.32: stat on FIFO did not return not_found")

            _call(s, "close_session", {"session": sid}, nid())

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
