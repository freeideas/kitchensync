#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises path_to_file_uri (02.9–02.17): POSIX/Windows paths to file:// URIs."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY",
                              "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")


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
    deadline = time.time() + 10
    while time.time() < deadline:
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, args, rpc_id):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args}, rpc_id=rpc_id)


def _uri_result(resp):
    result = resp.get("result")
    if result is None:
        return None
    return result.get("result")


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rpc_id = 1

            # 02.9 — POSIX absolute path produces file:///path
            resp = _call(s, "path-to-file-uri", {"path": "/foo/bar", "cwd": "/cwd"}, rpc_id); rpc_id += 1
            got = _uri_result(resp)
            print(f"[02.9] POSIX absolute: got={got!r}")
            if got != "file:///foo/bar":
                failures.append(f"02.9: expected 'file:///foo/bar', got {got!r}")

            # 02.10 — POSIX relative path resolved against cwd
            resp = _call(s, "path-to-file-uri", {"path": "bar", "cwd": "/foo"}, rpc_id); rpc_id += 1
            got = _uri_result(resp)
            print(f"[02.10] POSIX relative: got={got!r}")
            if got != "file:///foo/bar":
                failures.append(f"02.10: expected 'file:///foo/bar', got {got!r}")

            # 02.11 — Windows DOS-style path produces file:///drive:/rest
            resp = _call(s, "path-to-file-uri", {"path": "c:/foo/bar", "cwd": "/cwd"}, rpc_id); rpc_id += 1
            got = _uri_result(resp)
            print(f"[02.11] Windows DOS path: got={got!r}")
            if got != "file:///c:/foo/bar":
                failures.append(f"02.11: expected 'file:///c:/foo/bar', got {got!r}")

            # 02.12 — Backslashes in Windows path converted to forward slashes
            resp = _call(s, "path-to-file-uri", {"path": "c:\\foo\\bar", "cwd": "/cwd"}, rpc_id); rpc_id += 1
            got = _uri_result(resp)
            print(f"[02.12] Backslash conversion: got={got!r}")
            if got != "file:///c:/foo/bar":
                failures.append(f"02.12: expected 'file:///c:/foo/bar', got {got!r}")

            # 02.13 — Drive-letter path missing separator resolved against cwd
            resp = _call(s, "path-to-file-uri", {"path": "c:foo", "cwd": "/base"}, rpc_id); rpc_id += 1
            got = _uri_result(resp)
            print(f"[02.13] Drive-letter no separator: got={got!r}")
            if got != "file:///base/foo":
                failures.append(f"02.13: expected 'file:///base/foo', got {got!r}")

            # 02.14 — Windows UNC path produces file://server/share/rest
            resp = _call(s, "path-to-file-uri", {"path": "\\\\server\\share\\rest", "cwd": "/cwd"}, rpc_id); rpc_id += 1
            got = _uri_result(resp)
            print(f"[02.14] UNC path: got={got!r}")
            if got != "file://server/share/rest":
                failures.append(f"02.14: expected 'file://server/share/rest', got {got!r}")

            # 02.15 — Unreserved characters not percent-encoded
            resp = _call(s, "path-to-file-uri", {"path": "/foo-bar_baz.txt~", "cwd": "/cwd"}, rpc_id); rpc_id += 1
            got = _uri_result(resp)
            print(f"[02.15] Unreserved chars: got={got!r}")
            if got != "file:///foo-bar_baz.txt~":
                failures.append(f"02.15: expected 'file:///foo-bar_baz.txt~', got {got!r}")

            # 02.16 — Reserved/disallowed characters percent-encoded (space → %20)
            resp = _call(s, "path-to-file-uri", {"path": "/foo bar", "cwd": "/cwd"}, rpc_id); rpc_id += 1
            got = _uri_result(resp)
            print(f"[02.16] Percent-encoding: got={got!r}")
            if got != "file:///foo%20bar":
                failures.append(f"02.16: expected 'file:///foo%20bar', got {got!r}")

            # 02.17 — '/' separators preserved (not percent-encoded)
            resp = _call(s, "path-to-file-uri", {"path": "/a/b/c", "cwd": "/cwd"}, rpc_id); rpc_id += 1
            got = _uri_result(resp)
            print(f"[02.17] Slash separators preserved: got={got!r}")
            if got != "file:///a/b/c":
                failures.append(f"02.17: expected 'file:///a/b/c', got {got!r}")

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
