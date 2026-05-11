#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Built-in excludes: .kitchensync, .git/, symlinks, and special files."""

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

            # Discover available tools
            tl = _rpc(s, "tools/list")
            tools = {t["name"]: t for t in (tl.get("result") or {}).get("tools", [])}
            print(f"[info] tools available: {sorted(tools)}")

            # Build a baseline empty matcher (no user patterns)
            empty = _call(s, "empty_matcher", {})

            # 04.1 — .kitchensync at any depth is ignored (empty matcher, no patterns)
            r = _call(s, "is_ignored", {"matcher": empty, "path": ".kitchensync", "is_dir": True})
            ok = r is True
            print(f"[04.1] .kitchensync ignored at root: {ok}")
            if not ok:
                failures.append("04.1: is_ignored('.kitchensync', dir) should be true")

            r2 = _call(s, "is_ignored", {"matcher": empty, "path": "a/b/.kitchensync", "is_dir": True})
            ok2 = r2 is True
            print(f"[04.1] .kitchensync ignored nested: {ok2}")
            if not ok2:
                failures.append("04.1: is_ignored('a/b/.kitchensync', dir) should be true")

            # 04.2 — paths inside .kitchensync are ignored
            r3 = _call(s, "is_ignored", {"matcher": empty, "path": ".kitchensync/snapshot.db", "is_dir": False})
            ok3 = r3 is True
            print(f"[04.2] path inside .kitchensync ignored: {ok3}")
            if not ok3:
                failures.append("04.2: is_ignored('.kitchensync/snapshot.db', file) should be true")

            r4 = _call(s, "is_ignored", {"matcher": empty, "path": "sub/.kitchensync/data", "is_dir": False})
            ok4 = r4 is True
            print(f"[04.2] nested path inside .kitchensync ignored: {ok4}")
            if not ok4:
                failures.append("04.2: is_ignored('sub/.kitchensync/data', file) should be true")

            # 04.3 — !.kitchensync negation does not re-include .kitchensync
            ps_neg = _call(s, "compile_patterns", {"text": "!.kitchensync\n"})
            m_neg = _call(s, "push_scope", {"parent": empty, "scope_dir": "", "pattern_set": ps_neg})
            r5 = _call(s, "is_ignored", {"matcher": m_neg, "path": ".kitchensync", "is_dir": True})
            ok5 = r5 is True
            print(f"[04.3] !.kitchensync cannot negate built-in exclude: {ok5}")
            if not ok5:
                failures.append("04.3: is_ignored('.kitchensync') should still be true despite !.kitchensync pattern")

            # 04.4 — .git/ is ignored by default (empty matcher)
            r6 = _call(s, "is_ignored", {"matcher": empty, "path": ".git", "is_dir": True})
            ok6 = r6 is True
            print(f"[04.4] .git/ ignored by default: {ok6}")
            if not ok6:
                failures.append("04.4: is_ignored('.git', dir) should be true with no user patterns")

            # 04.5 — !.git/ pattern re-includes .git
            ps_git = _call(s, "compile_patterns", {"text": "!.git/\n"})
            m_git = _call(s, "push_scope", {"parent": empty, "scope_dir": "", "pattern_set": ps_git})
            r7 = _call(s, "is_ignored", {"matcher": m_git, "path": ".git", "is_dir": True})
            ok7 = r7 is False
            print(f"[04.5] !.git/ re-includes .git: {ok7}")
            if not ok7:
                failures.append("04.5: is_ignored('.git', dir) should be false after !.git/ pattern")

            # 04.6 — is_ignored_entry returns true for symlink kind
            r8 = _call(s, "is_ignored_entry", {"matcher": empty, "path": "readme.md", "kind": "symlink"})
            ok8 = r8 is True
            print(f"[04.6] symlink kind always ignored: {ok8}")
            if not ok8:
                failures.append("04.6: is_ignored_entry(path, 'symlink') should be true")

            # 04.7 — is_ignored_entry returns true for special kind
            r9 = _call(s, "is_ignored_entry", {"matcher": empty, "path": "readme.md", "kind": "special"})
            ok9 = r9 is True
            print(f"[04.7] special kind always ignored: {ok9}")
            if not ok9:
                failures.append("04.7: is_ignored_entry(path, 'special') should be true")

            # 04.8 — is_ignored_entry with file/dir kind matches is_ignored
            # Use a pattern that ignores "build" to get a non-trivial matcher
            ps_build = _call(s, "compile_patterns", {"text": "build\n"})
            m_build = _call(s, "push_scope", {"parent": empty, "scope_dir": "", "pattern_set": ps_build})

            for path, is_dir in [("build", True), ("build", False), ("src/main.java", False), ("docs", True)]:
                kind = "dir" if is_dir else "file"
                r_entry = _call(s, "is_ignored_entry", {"matcher": m_build, "path": path, "kind": kind})
                r_plain = _call(s, "is_ignored", {"matcher": m_build, "path": path, "is_dir": is_dir})
                ok_pair = (r_entry is True) == (r_plain is True)
                print(f"[04.8] is_ignored_entry({path!r}, {kind!r}) == is_ignored({path!r}, {is_dir}): {ok_pair}")
                if not ok_pair:
                    failures.append(
                        f"04.8: is_ignored_entry('{path}', '{kind}')={r_entry} != is_ignored('{path}', {is_dir})={r_plain}"
                    )

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
