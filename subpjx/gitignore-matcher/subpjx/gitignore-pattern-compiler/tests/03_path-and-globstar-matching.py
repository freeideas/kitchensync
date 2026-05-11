#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Path-shape rule, ** semantics, and pure-body matching."""

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


_rpc_counter = 0


def _rpc(sock, method, params=None):
    global _rpc_counter
    _rpc_counter += 1
    msg = {"jsonrpc": "2.0", "id": _rpc_counter, "method": method}
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


def _call(sock, name, args):
    return _rpc(sock, "tools/call", {"name": name, "arguments": args})


def _compile(sock, text):
    r = _call(sock, "compile-patterns", {"text": text})
    return r["result"]["set"]


def _pattern_at(sock, set_handle, index=0):
    r = _call(sock, "pattern-at", {"set": set_handle, "index": index})
    return r["result"]


def _matches(sock, pattern_handle, path):
    r = _call(sock, "matches", {"pattern": pattern_handle, "path": path})
    return r["result"]["matches"]


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # ── 03.1 – pattern with internal / is matched against full path ──────
            h1 = _compile(s, "src/foo.java")
            p1 = _pattern_at(s, h1)["pattern"]

            ok = _matches(s, p1, "src/foo.java")
            print(f"[03.1a] path-shaped 'src/foo.java' matches full path 'src/foo.java': {ok}")
            if not ok:
                failures.append("03.1a: path-shaped 'src/foo.java' must match 'src/foo.java'")

            ok = _matches(s, p1, "foo.java")
            print(f"[03.1b] path-shaped 'src/foo.java' does not match bare segment 'foo.java': {not ok}")
            if ok:
                failures.append("03.1b: path-shaped 'src/foo.java' must not match bare segment 'foo.java'")

            # ── 03.2 – pattern with no internal / matches a single path segment ──
            h2 = _compile(s, "*.java")
            p2 = _pattern_at(s, h2)["pattern"]

            ok = _matches(s, p2, "Main.java")
            print(f"[03.2a] segment pattern '*.java' matches segment 'Main.java': {ok}")
            if not ok:
                failures.append("03.2a: '*.java' must match segment 'Main.java'")

            ok = _matches(s, p2, "Test.java")
            print(f"[03.2b] segment pattern '*.java' matches segment 'Test.java': {ok}")
            if not ok:
                failures.append("03.2b: '*.java' must match segment 'Test.java'")

            # ── 03.3 – leading **/ matches at any directory depth ─────────────────
            h3 = _compile(s, "**/foo.java")
            p3 = _pattern_at(s, h3)["pattern"]

            for label, path in [
                ("depth 0", "foo.java"),
                ("depth 1", "src/foo.java"),
                ("depth 2", "a/b/foo.java"),
            ]:
                ok = _matches(s, p3, path)
                print(f"[03.3] '**/foo.java' matches '{path}' ({label}): {ok}")
                if not ok:
                    failures.append(f"03.3: '**/foo.java' must match '{path}' ({label})")

            # ── 03.4 – trailing /** matches anything inside the directory ─────────
            h4 = _compile(s, "src/**")
            p4 = _pattern_at(s, h4)["pattern"]

            ok = _matches(s, p4, "src/main.java")
            print(f"[03.4a] 'src/**' matches 'src/main.java': {ok}")
            if not ok:
                failures.append("03.4a: 'src/**' must match 'src/main.java'")

            ok = _matches(s, p4, "src/a/b.java")
            print(f"[03.4b] 'src/**' matches 'src/a/b.java': {ok}")
            if not ok:
                failures.append("03.4b: 'src/**' must match 'src/a/b.java'")

            ok = _matches(s, p4, "other/main.java")
            print(f"[03.4c] 'src/**' does not match 'other/main.java': {not ok}")
            if ok:
                failures.append("03.4c: 'src/**' must not match 'other/main.java'")

            # ── 03.5 – /**/ between segments matches zero or more intermediate dirs
            h5 = _compile(s, "src/**/main.java")
            p5 = _pattern_at(s, h5)["pattern"]

            for label, path, expected in [
                ("zero dirs",  "src/main.java",     True),
                ("one dir",    "src/a/main.java",   True),
                ("two dirs",   "src/a/b/main.java", True),
                ("wrong root", "other/main.java",   False),
            ]:
                ok = _matches(s, p5, path)
                print(f"[03.5] 'src/**/main.java' vs '{path}' ({label}): {ok}")
                if ok != expected:
                    failures.append(
                        f"03.5: 'src/**/main.java' vs '{path}' ({label}): expected {expected}, got {ok}"
                    )

            # ── 03.6 – ** adjacent to non-/ chars degenerates to single * ─────────
            h6 = _compile(s, "**.java")
            p6 = _pattern_at(s, h6)["pattern"]

            ok = _matches(s, p6, "main.java")
            print(f"[03.6a] '**.java' matches 'main.java' (acts as *): {ok}")
            if not ok:
                failures.append("03.6a: '**.java' must match 'main.java' (behaves as *.java)")

            ok = _matches(s, p6, "src/main.java")
            print(f"[03.6b] '**.java' does not match 'src/main.java' (no dir-spanning): {not ok}")
            if ok:
                failures.append("03.6b: '**.java' must not match 'src/main.java' (no dir-spanning)")

            # ── 03.7 – matches() ignores is_anchored and is_dir_only ──────────────
            # anchored pattern: leading / consumed → body=src/foo, is_anchored=true
            h7a = _compile(s, "/src/foo")
            pd7a = _pattern_at(s, h7a)
            if not pd7a.get("is_anchored"):
                failures.append("03.7-pre-a: '/src/foo' must have is_anchored=true")
            print(f"[03.7-pre-a] '/src/foo' is_anchored={pd7a.get('is_anchored')}")

            ok = _matches(s, pd7a["pattern"], "src/foo")
            print(f"[03.7a] anchored '/src/foo': matches() on body 'src/foo' returns {ok} (ignores is_anchored)")
            if not ok:
                failures.append("03.7a: matches() must return true for anchored body 'src/foo' vs 'src/foo'")

            # dir-only pattern: trailing / consumed → body=dir, is_dir_only=true
            h7b = _compile(s, "dir/")
            pd7b = _pattern_at(s, h7b)
            if not pd7b.get("is_dir_only"):
                failures.append("03.7-pre-b: 'dir/' must have is_dir_only=true")
            print(f"[03.7-pre-b] 'dir/' is_dir_only={pd7b.get('is_dir_only')}")

            ok = _matches(s, pd7b["pattern"], "dir")
            print(f"[03.7b] dir-only 'dir/': matches() on body 'dir' returns {ok} (ignores is_dir_only)")
            if not ok:
                failures.append("03.7b: matches() must return true for dir-only body 'dir' vs 'dir'")

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
