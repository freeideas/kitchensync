#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Built-in exclude rules: .kitchensync always-ignored and .git default-ignored."""

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


def _neg(body):
    return [{"body": body, "is_negation": True, "is_anchored": False, "is_dir_only": False}]


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # --- 04.1: path with any segment equal to .kitchensync is ignored ---
            try:
                m0 = _empty_matcher(s)
                r_a = _is_ignored(s, m0, ".kitchensync", False)
                print(f"[04.1a] is_ignored(empty, '.kitchensync', false) = {r_a!r}")
                if r_a is not True:
                    failures.append(f"04.1: '.kitchensync' should be ignored, got {r_a!r}")

                r_b = _is_ignored(s, m0, "foo/.kitchensync/bar", False)
                print(f"[04.1b] is_ignored(empty, 'foo/.kitchensync/bar', false) = {r_b!r}")
                if r_b is not True:
                    failures.append(f"04.1: 'foo/.kitchensync/bar' should be ignored (mid-segment), got {r_b!r}")

                r_c = _is_ignored(s, m0, "a/.kitchensync", True)
                print(f"[04.1c] is_ignored(empty, 'a/.kitchensync', true) = {r_c!r}")
                if r_c is not True:
                    failures.append(f"04.1: 'a/.kitchensync' dir should be ignored (last segment), got {r_c!r}")
            except Exception as e:
                failures.append(f"04.1: {e}")
                print(f"[04.1] exception: {e}")

            # --- 04.2: .kitchensync built-in cannot be overridden by a negation pattern ---
            try:
                m0 = _empty_matcher(s)
                m_neg_ks = _push(s, m0, "", _neg(".kitchensync"))
                r = _is_ignored(s, m_neg_ks, ".kitchensync", False)
                print(f"[04.2] is_ignored(+!.kitchensync, '.kitchensync', false) = {r!r}")
                if r is not True:
                    failures.append(f"04.2: '.kitchensync' should remain ignored even with user negation, got {r!r}")
            except Exception as e:
                failures.append(f"04.2: {e}")
                print(f"[04.2] exception: {e}")

            # --- 04.3: .git and .git/* paths are ignored when no user pattern applies ---
            try:
                m0 = _empty_matcher(s)
                r_a = _is_ignored(s, m0, ".git", False)
                print(f"[04.3a] is_ignored(empty, '.git', false) = {r_a!r}")
                if r_a is not True:
                    failures.append(f"04.3: '.git' should be ignored by default, got {r_a!r}")

                r_b = _is_ignored(s, m0, ".git/config", False)
                print(f"[04.3b] is_ignored(empty, '.git/config', false) = {r_b!r}")
                if r_b is not True:
                    failures.append(f"04.3: '.git/config' should be ignored by default, got {r_b!r}")
            except Exception as e:
                failures.append(f"04.3: {e}")
                print(f"[04.3] exception: {e}")

            # --- 04.4: user negation pattern overrides the .git built-in ---
            try:
                m0 = _empty_matcher(s)
                m_neg_git = _push(s, m0, "", _neg(".git"))
                r = _is_ignored(s, m_neg_git, ".git", False)
                print(f"[04.4] is_ignored(+!.git, '.git', false) = {r!r}")
                if r is not False:
                    failures.append(f"04.4: '.git' should NOT be ignored when user negation applies, got {r!r}")
            except Exception as e:
                failures.append(f"04.4: {e}")
                print(f"[04.4] exception: {e}")

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
