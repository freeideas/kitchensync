#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises is_dir_only, anchored, and unanchored pattern-matching (02.1–02.7)."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

_rpc_id = 0


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


def _rpc(sock, method, params=None):
    global _rpc_id
    _rpc_id += 1
    msg = {"jsonrpc": "2.0", "id": _rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + 10
    while time.time() < deadline:
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, arguments=None):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": arguments or {}})


def _result(resp):
    return resp.get("result") or {}


def _pat(body, is_anchored=False, is_dir_only=False, is_negation=False):
    return {
        "body": body,
        "is_anchored": is_anchored,
        "is_dir_only": is_dir_only,
        "is_negation": is_negation,
    }


def _empty_matcher(s):
    r = _call(s, "empty-matcher")
    if r.get("error"):
        raise RuntimeError(f"empty-matcher failed: {r['error']}")
    return _result(r)["matcher"]


def _push(s, m, scope_dir, patterns):
    r = _call(s, "push-scope", {"matcher": m, "scope_dir": scope_dir, "pattern_set": patterns})
    if r.get("error"):
        raise RuntimeError(f"push-scope failed: {r['error']}")
    return _result(r)["matcher"]


def _is_ignored(s, m, path, is_dir):
    r = _call(s, "is-ignored", {"matcher": m, "path": path, "is_dir": is_dir})
    if r.get("error"):
        raise RuntimeError(f"is-ignored failed: {r['error']}")
    res = _result(r)
    for key in ("ignored", "is_ignored"):
        if key in res:
            return res[key]
    raise RuntimeError(f"is-ignored result missing 'ignored' key: {res}")


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # --- 02.1: no pattern in any layer, no built-in exclude → is_ignored returns false ---
            try:
                m = _empty_matcher(s)
                r = _is_ignored(s, m, "some/path", False)
                print(f"[02.1] empty matcher, is_ignored('some/path', false) = {r!r}")
                if r is not False:
                    failures.append(f"02.1: expected false with no patterns, got {r!r}")
            except Exception as e:
                failures.append(f"02.1: {e}")
                print(f"[02.1] exception: {e}")

            # --- 02.2: is_dir_only pattern does not apply when is_dir is false ---
            try:
                m = _push(s, _empty_matcher(s), "", [_pat("foo", is_dir_only=True)])
                r = _is_ignored(s, m, "foo", False)
                print(f"[02.2] is_dir_only pattern, is_dir=false → {r!r}")
                if r is not False:
                    failures.append(f"02.2: expected false for is_dir_only when is_dir=false, got {r!r}")
            except Exception as e:
                failures.append(f"02.2: {e}")
                print(f"[02.2] exception: {e}")

            # --- 02.3: is_dir_only pattern applies when is_dir is true ---
            try:
                m = _push(s, _empty_matcher(s), "", [_pat("foo", is_dir_only=True)])
                r = _is_ignored(s, m, "foo", True)
                print(f"[02.3] is_dir_only pattern, is_dir=true → {r!r}")
                if r is not True:
                    failures.append(f"02.3: expected true for is_dir_only when is_dir=true, got {r!r}")
            except Exception as e:
                failures.append(f"02.3: {e}")
                print(f"[02.3] exception: {e}")

            # --- 02.4: anchored body matched against suffix below scope_dir ---
            # scope_dir="a/b/c", body="target": suffix of "a/b/c/target" below "a/b/c" is "target" → match
            try:
                m = _push(s, _empty_matcher(s), "a/b/c", [_pat("target", is_anchored=True)])
                r = _is_ignored(s, m, "a/b/c/target", False)
                print(f"[02.4] anchored 'target' at 'a/b/c', is_ignored('a/b/c/target') = {r!r}")
                if r is not True:
                    failures.append(f"02.4: expected true (suffix 'target' matches body), got {r!r}")
            except Exception as e:
                failures.append(f"02.4: {e}")
                print(f"[02.4] exception: {e}")

            # --- 02.5: anchored pattern does not apply when path is outside scope_dir ---
            try:
                m = _push(s, _empty_matcher(s), "sub", [_pat("foo", is_anchored=True)])
                r = _is_ignored(s, m, "other/foo", False)
                print(f"[02.5] anchored 'foo' at 'sub', is_ignored('other/foo') = {r!r}")
                if r is not False:
                    failures.append(f"02.5: expected false (path outside scope_dir), got {r!r}")
            except Exception as e:
                failures.append(f"02.5: {e}")
                print(f"[02.5] exception: {e}")

            # --- 02.6: unanchored body with no internal / matches any single segment ---
            try:
                m = _push(s, _empty_matcher(s), "", [_pat("foo")])
                r_hit  = _is_ignored(s, m, "a/b/foo", False)
                r_miss = _is_ignored(s, m, "a/b/c",   False)
                print(f"[02.6] unanchored 'foo': is_ignored('a/b/foo')={r_hit!r}, is_ignored('a/b/c')={r_miss!r}")
                if r_hit is not True:
                    failures.append(f"02.6: expected true when segment 'foo' present, got {r_hit!r}")
                if r_miss is not False:
                    failures.append(f"02.6: expected false when no segment 'foo', got {r_miss!r}")
            except Exception as e:
                failures.append(f"02.6: {e}")
                print(f"[02.6] exception: {e}")

            # --- 02.7: unanchored body with internal / matches full path below scope_dir ---
            try:
                m = _push(s, _empty_matcher(s), "", [_pat("a/b")])
                r_hit  = _is_ignored(s, m, "a/b",   False)
                r_miss = _is_ignored(s, m, "x/a/b", False)
                print(f"[02.7] unanchored 'a/b': is_ignored('a/b')={r_hit!r}, is_ignored('x/a/b')={r_miss!r}")
                if r_hit is not True:
                    failures.append(f"02.7: expected true for full-path match 'a/b', got {r_hit!r}")
                if r_miss is not False:
                    failures.append(f"02.7: expected false ('x/a/b' does not fully match body 'a/b'), got {r_miss!r}")
            except Exception as e:
                failures.append(f"02.7: {e}")
                print(f"[02.7] exception: {e}")

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
