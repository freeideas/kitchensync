#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises is_file_uri and looks_like_bare_path detection predicates."""

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


def _bool_result(resp):
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

            # 02.1 — is_file_uri returns true for strings beginning with file: scheme
            resp = _call(s, "is-file-uri", {"s": "file:///foo/bar"}, rpc_id); rpc_id += 1
            val = _bool_result(resp)
            print(f"[02.1] is-file-uri('file:///foo/bar') = {val}")
            if val is not True:
                failures.append("02.1: is-file-uri('file:///foo/bar') expected true")

            # 02.2 — is_file_uri matches scheme case-insensitively
            for variant in ["FILE:///foo", "File:///foo", "fIlE:///foo"]:
                resp = _call(s, "is-file-uri", {"s": variant}, rpc_id); rpc_id += 1
                val = _bool_result(resp)
                print(f"[02.2] is-file-uri({variant!r}) = {val}")
                if val is not True:
                    failures.append(f"02.2: is-file-uri({variant!r}) expected true")

            # 02.3 — is_file_uri returns false for non-file: strings
            for non_file in ["http://example.com", "/foo/bar", "sftp://host/path", ""]:
                resp = _call(s, "is-file-uri", {"s": non_file}, rpc_id); rpc_id += 1
                val = _bool_result(resp)
                print(f"[02.3] is-file-uri({non_file!r}) = {val}")
                if val is not False:
                    failures.append(f"02.3: is-file-uri({non_file!r}) expected false")

            # 02.4 — looks_like_bare_path returns true for POSIX absolute paths
            resp = _call(s, "looks-like-bare-path", {"s": "/foo"}, rpc_id); rpc_id += 1
            val = _bool_result(resp)
            print(f"[02.4] looks-like-bare-path('/foo') = {val}")
            if val is not True:
                failures.append("02.4: looks-like-bare-path('/foo') expected true")

            # 02.5 — looks_like_bare_path returns true for POSIX relative paths
            for relpath in ["./foo", "foo/bar"]:
                resp = _call(s, "looks-like-bare-path", {"s": relpath}, rpc_id); rpc_id += 1
                val = _bool_result(resp)
                print(f"[02.5] looks-like-bare-path({relpath!r}) = {val}")
                if val is not True:
                    failures.append(f"02.5: looks-like-bare-path({relpath!r}) expected true")

            # 02.6 — looks_like_bare_path returns true for Windows DOS-style paths
            for winpath in ["c:\\foo", "c:foo", "C:/foo"]:
                resp = _call(s, "looks-like-bare-path", {"s": winpath}, rpc_id); rpc_id += 1
                val = _bool_result(resp)
                print(f"[02.6] looks-like-bare-path({winpath!r}) = {val}")
                if val is not True:
                    failures.append(f"02.6: looks-like-bare-path({winpath!r}) expected true")

            # 02.7 — looks_like_bare_path returns true for Windows UNC paths
            resp = _call(s, "looks-like-bare-path", {"s": "\\\\server\\share"}, rpc_id); rpc_id += 1
            val = _bool_result(resp)
            print(f"[02.7] looks-like-bare-path('\\\\\\\\server\\\\share') = {val}")
            if val is not True:
                failures.append("02.7: looks-like-bare-path('\\\\server\\share') expected true")

            # 02.8 — looks_like_bare_path returns false for strings with a recognised URI scheme
            for uri in ["file:///foo", "http://example.com", "sftp://host/path", "ftp://ftp.example.com/"]:
                resp = _call(s, "looks-like-bare-path", {"s": uri}, rpc_id); rpc_id += 1
                val = _bool_result(resp)
                print(f"[02.8] looks-like-bare-path({uri!r}) = {val}")
                if val is not False:
                    failures.append(f"02.8: looks-like-bare-path({uri!r}) expected false")

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
