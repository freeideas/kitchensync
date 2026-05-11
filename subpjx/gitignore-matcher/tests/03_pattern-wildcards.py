#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Wildcard metacharacters *, ?, [...], **, and /**/ match per gitignore semantics (03.1–03.6)."""

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


def _compile(s, text):
    r = _call(s, "compile-patterns", {"text": text})
    if r.get("error"):
        raise RuntimeError(f"compile-patterns failed: {r['error']}")
    result = r.get("result") or {}
    for key in ("set", "pattern_set"):
        if key in result:
            return result[key]
    raise RuntimeError(f"compile-patterns result missing set key: {result}")


def _make_matcher(s, pattern_text, scope_dir=""):
    ps = _compile(s, pattern_text)
    em_r = _call(s, "empty-matcher")
    if em_r.get("error"):
        raise RuntimeError(f"empty-matcher failed: {em_r['error']}")
    matcher = (em_r.get("result") or {}).get("matcher")
    push_r = _call(s, "push-scope", {"matcher": matcher, "scope_dir": scope_dir, "set": ps})
    if push_r.get("error"):
        raise RuntimeError(f"push-scope failed: {push_r['error']}")
    return (push_r.get("result") or {}).get("matcher")


def _is_ignored(s, m, path, is_dir):
    r = _call(s, "is-ignored", {"matcher": m, "path": path, "is_dir": is_dir})
    if r.get("error"):
        raise RuntimeError(f"is-ignored failed: {r['error']}")
    res = r.get("result") or {}
    for key in ("ignored", "is_ignored"):
        if key in res:
            return res[key]
    raise RuntimeError(f"is-ignored result has no 'ignored' key: {res}")


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # --- 03.1: * matches any run of chars except / within one path component ---
            try:
                m = _make_matcher(s, "*.log")
                ok = _is_ignored(s, m, "access.log", False)
                print(f"[03.1a] '*.log' matches segment 'access.log': {ok}")
                if ok is not True:
                    failures.append("03.1a: '*.log' must match 'access.log'")

                ok = _is_ignored(s, m, "server.log", False)
                print(f"[03.1b] '*.log' matches segment 'server.log': {ok}")
                if ok is not True:
                    failures.append("03.1b: '*.log' must match 'server.log'")

                ok = _is_ignored(s, m, "notlog", False)
                print(f"[03.1c] '*.log' does not match 'notlog': {not ok}")
                if ok is not False:
                    failures.append("03.1c: '*.log' must not match 'notlog'")
            except Exception as e:
                failures.append(f"03.1: {e}")
                print(f"[03.1] exception: {e}")

            # --- * does not cross / (anchored pattern at root) ---
            try:
                m = _make_matcher(s, "/a*c")
                ok = _is_ignored(s, m, "axc", False)
                print(f"[03.1d] anchored '/a*c' matches 'axc': {ok}")
                if ok is not True:
                    failures.append("03.1d: anchored '/a*c' must match 'axc'")

                ok = _is_ignored(s, m, "a/c", False)
                print(f"[03.1e] anchored '/a*c' does not match 'a/c' (* won't cross /): {not ok}")
                if ok is not False:
                    failures.append("03.1e: anchored '/a*c' must not match 'a/c'")
            except Exception as e:
                failures.append(f"03.1de: {e}")
                print(f"[03.1de] exception: {e}")

            # --- 03.2: ? matches exactly one char other than / ---
            try:
                m = _make_matcher(s, "f?o")
                ok = _is_ignored(s, m, "foo", False)
                print(f"[03.2a] 'f?o' matches 'foo': {ok}")
                if ok is not True:
                    failures.append("03.2a: 'f?o' must match 'foo'")

                ok = _is_ignored(s, m, "fxo", False)
                print(f"[03.2b] 'f?o' matches 'fxo': {ok}")
                if ok is not True:
                    failures.append("03.2b: 'f?o' must match 'fxo'")

                ok = _is_ignored(s, m, "fo", False)
                print(f"[03.2c] 'f?o' does not match 'fo' (needs exactly one char): {not ok}")
                if ok is not False:
                    failures.append("03.2c: 'f?o' must not match 'fo'")
            except Exception as e:
                failures.append(f"03.2: {e}")
                print(f"[03.2] exception: {e}")

            # --- ? does not match / (anchored pattern at root) ---
            try:
                m = _make_matcher(s, "/f?o")
                ok = _is_ignored(s, m, "f/o", False)
                print(f"[03.2d] anchored '/f?o' does not match 'f/o' (? won't match /): {not ok}")
                if ok is not False:
                    failures.append("03.2d: anchored '/f?o' must not match 'f/o'")
            except Exception as e:
                failures.append(f"03.2d: {e}")
                print(f"[03.2d] exception: {e}")

            # --- 03.3: [abc] matches one character from the listed class ---
            try:
                m = _make_matcher(s, "[abc].txt")
                for char, expected in [("a", True), ("b", True), ("c", True), ("d", False)]:
                    ok = _is_ignored(s, m, f"{char}.txt", False)
                    label = "matches" if expected else "does not match"
                    print(f"[03.3] '[abc].txt' {label} '{char}.txt': {ok}")
                    if ok is not expected:
                        failures.append(f"03.3: '[abc].txt' vs '{char}.txt': expected {expected}, got {ok}")
            except Exception as e:
                failures.append(f"03.3: {e}")
                print(f"[03.3] exception: {e}")

            # --- 03.4: leading **/ allows matching at any depth ---
            try:
                m = _make_matcher(s, "**/foo.txt")
                for label, path in [
                    ("depth 0", "foo.txt"),
                    ("depth 1", "dir/foo.txt"),
                    ("depth 2", "a/b/foo.txt"),
                ]:
                    ok = _is_ignored(s, m, path, False)
                    print(f"[03.4] '**/foo.txt' matches '{path}' ({label}): {ok}")
                    if ok is not True:
                        failures.append(f"03.4: '**/foo.txt' must match '{path}' ({label})")

                ok = _is_ignored(s, m, "bar.txt", False)
                print(f"[03.4] '**/foo.txt' does not match 'bar.txt': {not ok}")
                if ok is not False:
                    failures.append("03.4: '**/foo.txt' must not match 'bar.txt'")
            except Exception as e:
                failures.append(f"03.4: {e}")
                print(f"[03.4] exception: {e}")

            # --- 03.5: trailing /** matches any path inside the named directory ---
            try:
                m = _make_matcher(s, "src/**")
                for label, path, expected in [
                    ("direct child",    "src/file.txt", True),
                    ("nested",          "src/a/b.txt",  True),
                    ("outside",         "other/file",   False),
                ]:
                    ok = _is_ignored(s, m, path, False)
                    print(f"[03.5] 'src/**' vs '{path}' ({label}): {ok}")
                    if ok is not expected:
                        failures.append(
                            f"03.5: 'src/**' vs '{path}' ({label}): expected {expected}, got {ok}"
                        )
            except Exception as e:
                failures.append(f"03.5: {e}")
                print(f"[03.5] exception: {e}")

            # --- 03.6: /**/ within a pattern matches zero or more intermediate dirs ---
            try:
                m = _make_matcher(s, "src/**/file.txt")
                for label, path, expected in [
                    ("zero dirs",  "src/file.txt",       True),
                    ("one dir",    "src/a/file.txt",     True),
                    ("two dirs",   "src/a/b/file.txt",   True),
                    ("wrong root", "other/file.txt",     False),
                ]:
                    ok = _is_ignored(s, m, path, False)
                    print(f"[03.6] 'src/**/file.txt' vs '{path}' ({label}): {ok}")
                    if ok is not expected:
                        failures.append(
                            f"03.6: 'src/**/file.txt' vs '{path}' ({label}): expected {expected}, got {ok}"
                        )
            except Exception as e:
                failures.append(f"03.6: {e}")
                print(f"[03.6] exception: {e}")

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
