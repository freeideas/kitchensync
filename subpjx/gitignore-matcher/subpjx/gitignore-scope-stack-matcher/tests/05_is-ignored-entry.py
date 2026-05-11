#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Tests is_ignored_entry: file/dir delegate to is_ignored; symlink/special always true."""

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
        chunk = sock.recv(65536)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, tool, arguments=None, rpc_id=1):
    resp = _rpc(sock, "tools/call",
                {"name": tool, "arguments": arguments or {}},
                rpc_id=rpc_id)
    if "error" in resp:
        raise RuntimeError(f"Tool '{tool}' RPC error: {resp['error']}")
    content = (resp.get("result") or {}).get("content", [])
    text = next((c["text"] for c in content if c.get("type") == "text"), None)
    if text is None:
        raise RuntimeError(f"Tool '{tool}' returned no text content; resp={resp}")
    return json.loads(text)


def _find_tool(tool_names, *candidates):
    for c in candidates:
        if c in tool_names:
            return c
    return None


def _get(obj, *keys):
    for k in keys:
        if isinstance(obj, dict) and k in obj:
            return obj[k]
    return None


def _bool(raw):
    """Extract a boolean from a tool result that may be a bare bool or a dict."""
    if isinstance(raw, bool):
        return raw
    return _get(raw, "ignored", "result", "value")


def _is_ignored(sock, tool, matcher, path, is_dir, rpc_id):
    """Call is_ignored, trying snake_case then camelCase param names."""
    try:
        return _call(sock, tool,
                     {"matcher": matcher, "path": path, "is_dir": is_dir},
                     rpc_id=rpc_id)
    except RuntimeError:
        return _call(sock, tool,
                     {"matcher": matcher, "path": path, "isDir": is_dir},
                     rpc_id=rpc_id)


def _is_ignored_entry(sock, tool, matcher, path, kind, rpc_id):
    return _call(sock, tool,
                 {"matcher": matcher, "path": path, "kind": kind},
                 rpc_id=rpc_id)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            _counter = [1]

            def nid():
                v = _counter[0]
                _counter[0] += 1
                return v

            tl = _rpc(s, "tools/list", rpc_id=nid())
            tools = (tl.get("result") or {}).get("tools", [])
            tool_names = {t["name"] for t in tools}

            t_empty   = _find_tool(tool_names, "empty_matcher", "emptyMatcher")
            t_ignored = _find_tool(tool_names, "is_ignored", "isIgnored")
            t_entry   = _find_tool(tool_names, "is_ignored_entry", "isIgnoredEntry")

            # Obtain an empty matcher (no user patterns; only built-in excludes apply).
            m = None
            if t_empty is None:
                failures.append("setup: empty_matcher tool not found")
                print("[setup] empty_matcher tool missing")
            else:
                m = _call(s, t_empty, {}, rpc_id=nid())
                print(f"[setup] empty_matcher ok, type={type(m).__name__}")

            if t_ignored is None:
                failures.append("setup: is_ignored tool not found")
                print("[setup] is_ignored tool missing")

            if t_entry is None:
                failures.append("setup: is_ignored_entry tool not found")
                print("[setup] is_ignored_entry tool missing")

            # --- 05.1: is_ignored_entry(m, path, "file") == is_ignored(m, path, false) ---
            # Use two paths: one ignored by the built-in .kitchensync rule (true),
            # one not ignored by any rule (false).  Both calls must agree.
            if m is not None and t_ignored is not None and t_entry is not None:
                for path, label in [(".kitchensync/x", "builtin-ignored"),
                                     ("plain.txt",      "not-ignored")]:
                    v_ign = _bool(_is_ignored(s, t_ignored, m, path, False, nid()))
                    v_ent = _bool(_is_ignored_entry(s, t_entry, m, path, "file", nid()))
                    print(f"[05.1] {label}: is_ignored(false)={v_ign!r}, "
                          f"is_ignored_entry(file)={v_ent!r}")
                    if v_ign != v_ent:
                        failures.append(
                            f"05.1 [{label}]: is_ignored_entry(file) {v_ent!r} "
                            f"!= is_ignored(false) {v_ign!r}")
            else:
                print("[05.1] skipped — required tools missing")

            # --- 05.2: is_ignored_entry(m, path, "dir") == is_ignored(m, path, true) ---
            if m is not None and t_ignored is not None and t_entry is not None:
                for path, label in [(".kitchensync/y", "builtin-ignored"),
                                     ("subdir",         "not-ignored")]:
                    v_ign = _bool(_is_ignored(s, t_ignored, m, path, True, nid()))
                    v_ent = _bool(_is_ignored_entry(s, t_entry, m, path, "dir", nid()))
                    print(f"[05.2] {label}: is_ignored(true)={v_ign!r}, "
                          f"is_ignored_entry(dir)={v_ent!r}")
                    if v_ign != v_ent:
                        failures.append(
                            f"05.2 [{label}]: is_ignored_entry(dir) {v_ent!r} "
                            f"!= is_ignored(true) {v_ign!r}")
            else:
                print("[05.2] skipped — required tools missing")

            # --- 05.3: is_ignored_entry(m, path, "symlink") is always true ---
            if m is not None and t_entry is not None:
                v = _bool(_is_ignored_entry(s, t_entry, m, "anything/at/all", "symlink", nid()))
                print(f"[05.3] is_ignored_entry(empty, anything/at/all, symlink)={v!r}")
                if v is not True:
                    failures.append(f"05.3: expected true for symlink, got {v!r}")
            else:
                print("[05.3] skipped — required tools missing")

            # --- 05.4: is_ignored_entry(m, path, "special") is always true ---
            if m is not None and t_entry is not None:
                v = _bool(_is_ignored_entry(s, t_entry, m, "anything/at/all", "special", nid()))
                print(f"[05.4] is_ignored_entry(empty, anything/at/all, special)={v!r}")
                if v is not True:
                    failures.append(f"05.4: expected true for special, got {v!r}")
            else:
                print("[05.4] skipped — required tools missing")

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
