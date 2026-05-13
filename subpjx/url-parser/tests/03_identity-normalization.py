#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Identity normalization: exercises 03.12–03.20 against the url-parser MCP wrapper."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
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


def _normalize(sock, url, cwd, default_user, rpc_id):
    return _rpc(sock, "tools/call", {
        "name": "normalize",
        "arguments": {"url": url, "cwd": cwd, "default_user": default_user},
    }, rpc_id=rpc_id)


def _parse(sock, text, cwd, default_user, rpc_id):
    return _rpc(sock, "tools/call", {
        "name": "parse",
        "arguments": {"text": text, "cwd": cwd, "default_user": default_user},
    }, rpc_id=rpc_id)


def _payload(r):
    content = r.get("result", {}).get("content", [])
    if not content:
        return None
    return json.loads(content[0].get("text", "null"))


def _identity(r):
    payload = _payload(r)
    return payload if isinstance(payload, str) else ""


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            # 03.12 — scheme is lowercased
            r = _normalize(s, "SFTP://host/path", "/home/u", "ace", rid); rid += 1
            got = _identity(r)
            print(f"[03.12] scheme lowercase: {got!r}")
            if not got.startswith("sftp://"):
                failures.append(f"03.12: expected identity starting with 'sftp://'; got {got!r}")

            # 03.13 — sftp host is lowercased
            r = _normalize(s, "sftp://HOST/path", "/home/u", "ace", rid); rid += 1
            got = _identity(r)
            print(f"[03.13] host lowercase: {got!r}")
            if got != "sftp://ace@host/path":
                failures.append(f"03.13: expected 'sftp://ace@host/path'; got {got!r}")

            # 03.14 — default_user inserted when userinfo absent
            r = _normalize(s, "sftp://host/path", "/home/u", "ace", rid); rid += 1
            got = _identity(r)
            print(f"[03.14] default_user inserted: {got!r}")
            if got != "sftp://ace@host/path":
                failures.append(f"03.14: expected 'sftp://ace@host/path'; got {got!r}")

            # 03.15 — default port 22 omitted
            r = _normalize(s, "sftp://host:22/path", "/home/u", "ace", rid); rid += 1
            got = _identity(r)
            print(f"[03.15] default port omitted: {got!r}")
            if ":22" in got:
                failures.append(f"03.15: port 22 not omitted; got {got!r}")

            # 03.16 — consecutive slashes collapsed
            r = _normalize(s, "sftp://host//docs", "/home/u", "ace", rid); rid += 1
            got = _identity(r)
            print(f"[03.16] consecutive slashes collapsed: {got!r}")
            if got != "sftp://ace@host/docs":
                failures.append(f"03.16: expected 'sftp://ace@host/docs'; got {got!r}")

            # 03.17a — trailing slash removed from non-root path
            r = _normalize(s, "sftp://host/path/", "/home/u", "ace", rid); rid += 1
            got = _identity(r)
            print(f"[03.17a] trailing slash removed: {got!r}")
            if got != "sftp://ace@host/path":
                failures.append(f"03.17a: expected 'sftp://ace@host/path'; got {got!r}")

            # 03.17b — path of only "/" not reduced
            r = _normalize(s, "sftp://host/", "/home/u", "ace", rid); rid += 1
            got = _identity(r)
            print(f"[03.17b] root slash preserved: {got!r}")
            if got != "sftp://ace@host/":
                failures.append(f"03.17b: expected 'sftp://ace@host/'; got {got!r}")

            # 03.18 — unreserved chars percent-decoded (%41 = 'A')
            r = _normalize(s, "sftp://host/%41path", "/home/u", "ace", rid); rid += 1
            got = _identity(r)
            print(f"[03.18] percent-decode unreserved: {got!r}")
            if got != "sftp://ace@host/Apath":
                failures.append(f"03.18: expected 'sftp://ace@host/Apath'; got {got!r}")

            # 03.19 — query excluded from identity, preserved in ParsedUrl.query
            r = _parse(s, "sftp://host/path?mc=5", "/home/u", "ace", rid); rid += 1
            urls = (_payload(r) or {}).get("urls", [])
            if urls:
                url0 = urls[0]
                identity = url0.get("identity", "")
                query = url0.get("query", {})
                print(f"[03.19] query excluded: identity={identity!r} query={query!r}")
                if "?" in identity or "mc" in identity:
                    failures.append(f"03.19: query present in identity; got {identity!r}")
                if query.get("mc") != "5":
                    failures.append(f"03.19: mc not preserved in ParsedUrl.query; got {query!r}")
            else:
                print(f"[03.19] query excluded: no urls in result; r={r!r}")
                failures.append("03.19: parse returned no urls")

            # 03.20 — relative bare path resolved against cwd
            r = _parse(s, "./data", "/home/u", "ace", rid); rid += 1
            urls = (_payload(r) or {}).get("urls", [])
            if urls:
                identity = urls[0].get("identity", "")
                print(f"[03.20] relative path resolved: identity={identity!r}")
                if identity != "file:///home/u/data":
                    failures.append(f"03.20: expected 'file:///home/u/data'; got {identity!r}")
            else:
                print(f"[03.20] relative path resolved: no urls in result; r={r!r}")
                failures.append("03.20: parse returned no urls")

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
