#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises list_dir, stat, and chunked streaming read — req IDs 02.20–02.27."""

from __future__ import annotations

import base64, json, os, shutil, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TEST_DIR = Path("/home/ace/Desktop/prjx/kitchensync/tmp/testks/sftp-protocol-02")
HOST = "localhost"
SSH_PORT = 22
USER = "ace"
POOL_SETTINGS = {"mc": 2, "ct": 10, "ka": 30}
FILE_CONTENT = b"hello sftp read test content"


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


def _setup():
    if TEST_DIR.exists():
        for p in sorted(TEST_DIR.rglob("*"), reverse=True):
            try:
                os.chmod(p, 0o755)
            except OSError:
                pass
        shutil.rmtree(TEST_DIR)
    TEST_DIR.mkdir(parents=True)
    (TEST_DIR / "file.txt").write_bytes(FILE_CONTENT)
    (TEST_DIR / "subdir").mkdir()
    (TEST_DIR / "symlink").symlink_to("file.txt")
    os.mkfifo(TEST_DIR / "named_pipe")


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
            tl = _rpc(s, "tools/list", rpc_id=nid())
            tools = [t["name"] for t in (tl.get("result") or {}).get("tools", [])]
            print(f"[setup] tools: {tools}")

            # Open endpoint — prerequisite for all assertions
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

            # Acquire connection
            conn_r = _call(s, "acquire", {"endpoint": endpoint}, nid())
            conn = (conn_r.get("connection") or conn_r.get("connection_id")
                    or conn_r.get("id") or conn_r.get("handle"))
            if not conn:
                print(f"[setup] FATAL: acquire returned no connection: {conn_r}")
                failures.append("setup: acquire returned no connection")
                return 1
            print(f"[setup] connection: {conn!r}")

            tdir = str(TEST_DIR)
            fpath = str(TEST_DIR / "file.txt")
            dpath = str(TEST_DIR / "subdir")
            lpath = str(TEST_DIR / "symlink")
            ppath = str(TEST_DIR / "named_pipe")
            npath = str(TEST_DIR / "nonexistent_xyz")

            # 02.20 — list_dir returns immediate children
            r20 = _call(s, "list_dir", {"connection": conn, "path": tdir}, nid())
            entries20 = ((r20.get("entries") if isinstance(r20, dict) else None)
                         or (r20 if isinstance(r20, list) else []))
            names20 = {e.get("name") for e in entries20 if isinstance(e, dict)}
            ok20 = "file.txt" in names20 and "subdir" in names20
            print(f"[02.20] list_dir children: {ok20}  names={sorted(names20)}")
            if not ok20:
                failures.append("02.20: list_dir did not return expected children")

            # 02.21 — each entry has name, is_dir, mod_time, byte_size;
            #          regular file byte_size==file size, directory byte_size==-1
            file_e = next((e for e in entries20 if isinstance(e, dict) and e.get("name") == "file.txt"), None)
            dir_e = next((e for e in entries20 if isinstance(e, dict) and e.get("name") == "subdir"), None)
            ok21_fields = (file_e is not None and dir_e is not None
                           and all(k in file_e for k in ("name", "is_dir", "mod_time", "byte_size"))
                           and all(k in dir_e for k in ("name", "is_dir", "mod_time", "byte_size")))
            ok21_sizes = (file_e is not None and file_e.get("byte_size") == len(FILE_CONTENT)
                          and dir_e is not None and dir_e.get("byte_size") == -1)
            ok21 = ok21_fields and ok21_sizes
            print(f"[02.21] entry fields+sizes: {ok21}  file={file_e}  dir={dir_e}")
            if not ok21:
                failures.append("02.21: list_dir entry fields or byte_size incorrect")

            # 02.22 — symlinks, FIFOs, and other non-regular entries are silently omitted
            ok22 = "symlink" not in names20 and "named_pipe" not in names20
            print(f"[02.22] non-regular entries omitted: {ok22}  names={sorted(names20)}")
            if not ok22:
                failures.append("02.22: list_dir included symlink or FIFO")

            # 02.23 — stat on regular file returns mod_time, byte_size, is_dir=false
            r23f = _call(s, "stat", {"connection": conn, "path": fpath}, nid())
            ok23f = (isinstance(r23f, dict) and not _is_not_found(r23f)
                     and "mod_time" in r23f and "byte_size" in r23f
                     and r23f.get("is_dir") is False)
            print(f"[02.23a] stat file: {ok23f}  result={r23f}")
            if not ok23f:
                failures.append("02.23: stat on regular file did not return expected fields")

            # 02.23 — stat on directory returns is_dir=true
            r23d = _call(s, "stat", {"connection": conn, "path": dpath}, nid())
            ok23d = (isinstance(r23d, dict) and not _is_not_found(r23d)
                     and "mod_time" in r23d and "byte_size" in r23d
                     and r23d.get("is_dir") is True)
            print(f"[02.23b] stat dir: {ok23d}  result={r23d}")
            if not ok23d:
                failures.append("02.23: stat on directory did not return is_dir=true")

            # 02.24 — stat on non-existent path returns not_found
            r24n = _call(s, "stat", {"connection": conn, "path": npath}, nid())
            ok24n = _is_not_found(r24n)
            print(f"[02.24a] stat missing→not_found: {ok24n}  result={r24n}")
            if not ok24n:
                failures.append("02.24: stat on non-existent path did not return not_found")

            # 02.24 — stat on symlink returns not_found
            r24l = _call(s, "stat", {"connection": conn, "path": lpath}, nid())
            ok24l = _is_not_found(r24l)
            print(f"[02.24b] stat symlink→not_found: {ok24l}  result={r24l}")
            if not ok24l:
                failures.append("02.24: stat on symlink did not return not_found")

            # 02.24 — stat on FIFO (special file) returns not_found
            r24p = _call(s, "stat", {"connection": conn, "path": ppath}, nid())
            ok24p = _is_not_found(r24p)
            print(f"[02.24c] stat fifo→not_found: {ok24p}  result={r24p}")
            if not ok24p:
                failures.append("02.24: stat on FIFO did not return not_found")

            # 02.25, 02.26, 02.27 — open_read / read / close_read
            rh_r = _call(s, "open_read", {"connection": conn, "path": fpath}, nid())
            rh = rh_r.get("handle") or rh_r.get("read_handle") or rh_r.get("id")
            if not rh:
                print(f"[02.25] FATAL: open_read returned no handle: {rh_r}")
                failures.append("02.25: open_read returned no handle")
            else:
                print(f"[02.25] read handle acquired: {rh!r}")
                accumulated = b""
                MAX = 7  # small enough to force multiple chunks over 28-byte file
                chunk_sizes = []
                for _ in range(200):
                    rd_r = _call(s, "read", {"handle": rh, "max_bytes": MAX}, nid())
                    eof = rd_r.get("eof", False) or rd_r.get("EOF", False)
                    raw = rd_r.get("bytes") or rd_r.get("data") or ""
                    if not raw:
                        break
                    decoded = base64.b64decode(raw)
                    chunk_sizes.append(len(decoded))
                    accumulated += decoded
                    if eof:
                        break

                # 02.25 — accumulated reads reproduce original file bytes in order
                ok25 = accumulated == FILE_CONTENT
                print(f"[02.25] read reproduces file content: {ok25}  got={accumulated!r}")
                if not ok25:
                    failures.append(
                        f"02.25: content mismatch — expected {FILE_CONTENT!r}, got {accumulated!r}"
                    )

                # 02.27 — each read call returned at most max_bytes bytes
                ok27 = all(sz <= MAX for sz in chunk_sizes)
                print(f"[02.27] each chunk <= {MAX}: {ok27}  sizes={chunk_sizes}")
                if not ok27:
                    failures.append(f"02.27: a read chunk exceeded max_bytes={MAX}: {chunk_sizes}")

                # 02.26 — read returns EOF once all bytes delivered
                eof_r = _call(s, "read", {"handle": rh, "max_bytes": 64}, nid())
                ok26 = (eof_r.get("eof", False) or eof_r.get("EOF", False)
                        or not (eof_r.get("bytes") or eof_r.get("data")))
                print(f"[02.26] read→EOF after exhaustion: {ok26}  result={eof_r}")
                if not ok26:
                    failures.append("02.26: read did not return EOF after file fully consumed")

                _call(s, "close_read", {"handle": rh}, nid())

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
