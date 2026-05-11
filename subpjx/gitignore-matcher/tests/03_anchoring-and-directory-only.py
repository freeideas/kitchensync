#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Leading-slash anchoring and trailing-slash directory-only restrict pattern scope (03.7–03.10)."""

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


_rpc_id = 0


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
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, args):
    resp = _rpc(sock, "tools/call", {"name": tool, "arguments": args})
    result = resp.get("result") or {}
    content = result.get("content", [])
    if content and isinstance(content, list):
        text = content[0].get("text", "")
        try:
            return json.loads(text)
        except Exception:
            return text
    return result


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # --- 03.7: anchored pattern (leading /) matches at scope_dir, not in nested subdir ---
            try:
                ps_anchored = _call(s, "compile_patterns", {"text": "/foo\n"})
                empty = _call(s, "empty_matcher", {})
                m = _call(s, "push_scope", {"parent": empty, "scope_dir": "sub", "pattern_set": ps_anchored})
                r_direct = _call(s, "is_ignored", {"matcher": m, "path": "sub/foo", "is_dir": False})
                r_nested = _call(s, "is_ignored", {"matcher": m, "path": "sub/deep/foo", "is_dir": False})
                print(f"[03.7] anchored '/foo' at 'sub': is_ignored('sub/foo')={r_direct!r}, is_ignored('sub/deep/foo')={r_nested!r}")
                if r_direct is not True:
                    failures.append(f"03.7: expected is_ignored('sub/foo')=true for anchored pattern at scope_dir, got {r_direct!r}")
                if r_nested is not False:
                    failures.append(f"03.7: expected is_ignored('sub/deep/foo')=false for anchored pattern (no match in nested subdir), got {r_nested!r}")
            except Exception as e:
                failures.append(f"03.7: {e}")
                print(f"[03.7] exception: {e}")

            # --- 03.8: unanchored pattern matches name at any depth within scope_dir ---
            try:
                ps_unanchored = _call(s, "compile_patterns", {"text": "foo\n"})
                empty = _call(s, "empty_matcher", {})
                m = _call(s, "push_scope", {"parent": empty, "scope_dir": "sub", "pattern_set": ps_unanchored})
                r_direct = _call(s, "is_ignored", {"matcher": m, "path": "sub/foo", "is_dir": False})
                r_nested = _call(s, "is_ignored", {"matcher": m, "path": "sub/deep/foo", "is_dir": False})
                print(f"[03.8] unanchored 'foo' at 'sub': is_ignored('sub/foo')={r_direct!r}, is_ignored('sub/deep/foo')={r_nested!r}")
                if r_direct is not True:
                    failures.append(f"03.8: expected is_ignored('sub/foo')=true for unanchored pattern, got {r_direct!r}")
                if r_nested is not True:
                    failures.append(f"03.8: expected is_ignored('sub/deep/foo')=true for unanchored pattern at any depth, got {r_nested!r}")
            except Exception as e:
                failures.append(f"03.8: {e}")
                print(f"[03.8] exception: {e}")

            # --- 03.9: directory-only pattern (trailing /) matches when is_dir=true ---
            try:
                ps_dir_only = _call(s, "compile_patterns", {"text": "foo/\n"})
                empty = _call(s, "empty_matcher", {})
                m = _call(s, "push_scope", {"parent": empty, "scope_dir": "", "pattern_set": ps_dir_only})
                r_dir = _call(s, "is_ignored", {"matcher": m, "path": "foo", "is_dir": True})
                print(f"[03.9] dir-only 'foo/' at root: is_ignored('foo', is_dir=true)={r_dir!r}")
                if r_dir is not True:
                    failures.append(f"03.9: expected is_ignored('foo', is_dir=true)=true for dir-only pattern, got {r_dir!r}")
            except Exception as e:
                failures.append(f"03.9: {e}")
                print(f"[03.9] exception: {e}")

            # --- 03.10: directory-only pattern does not match a file (is_dir=false) ---
            try:
                ps_dir_only = _call(s, "compile_patterns", {"text": "foo/\n"})
                empty = _call(s, "empty_matcher", {})
                m = _call(s, "push_scope", {"parent": empty, "scope_dir": "", "pattern_set": ps_dir_only})
                r_file = _call(s, "is_ignored", {"matcher": m, "path": "foo", "is_dir": False})
                print(f"[03.10] dir-only 'foo/' at root: is_ignored('foo', is_dir=false)={r_file!r}")
                if r_file is not False:
                    failures.append(f"03.10: expected is_ignored('foo', is_dir=false)=false for dir-only pattern, got {r_file!r}")
            except Exception as e:
                failures.append(f"03.10: {e}")
                print(f"[03.10] exception: {e}")

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
