#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises 03_negation.md: re-including paths with ! and the parent-directory restriction."""

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
    r = _rpc(sock, "tools/call", {"name": tool, "arguments": args}, rpc_id=rpc_id)
    if "error" in r:
        raise RuntimeError(f"tool {tool!r} error: {r['error']}")
    result = r.get("result", {})
    content = result.get("content")
    if isinstance(content, list) and content and content[0].get("text"):
        return json.loads(content[0]["text"])
    return result


def _compile(sock, text, rpc_id):
    return _call(sock, "compile", {"text": text}, rpc_id)["patterns"]


def _match(sock, stack, relative_path, is_directory, rpc_id):
    return _call(sock, "match", {
        "stack": stack,
        "relative_path": relative_path,
        "is_directory": is_directory,
    }, rpc_id)


def _verdict(result):
    if isinstance(result, str):
        return result
    if isinstance(result, dict):
        for key in ("result", "verdict", "value"):
            if key in result:
                return result[key]
    return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            # 03.1 — negated pattern reclassifies a previously-Ignored path to NotIgnored
            # "*.txt" would ignore "keep.txt"; "!keep.txt" negates that → NotIgnored
            pat_03_1 = _compile(s, "*.txt\n!keep.txt", rid); rid += 1
            r = _match(s, [{"scope": "", "patterns": pat_03_1}], "keep.txt", False, rid); rid += 1
            v = _verdict(r)
            print(f"[03.1] negation reclassifies Ignored → NotIgnored: {v!r}")
            if v != "NotIgnored":
                failures.append(f"03.1: expected NotIgnored, got {v!r}")

            # 03.2 — ancestor Ignored blocks negation re-inclusion
            # "foo" makes ancestor "foo" Ignored; "!foo/bar" would re-include "foo/bar"
            # but the parent-directory restriction prevents it → Ignored
            pat_03_2 = _compile(s, "foo\n!foo/bar", rid); rid += 1
            r = _match(s, [{"scope": "", "patterns": pat_03_2}], "foo/bar", False, rid); rid += 1
            v = _verdict(r)
            print(f"[03.2] ancestor Ignored blocks negation of descendant: {v!r}")
            if v != "Ignored":
                failures.append(f"03.2: expected Ignored (ancestor blocks re-inclusion), got {v!r}")

            # 03.3 — ancestor treated as directory so directory-only patterns apply to it
            # "foo/" is directory-only; "foo/bar" (is_directory=false) wouldn't be ignored
            # by "foo/" directly.  But when checking ancestor "foo", it is treated as
            # is_directory=true, so "foo/" matches it → "foo" is Ignored → "foo/bar" is Ignored.
            # If the ancestor were NOT treated as a directory, "foo/" would miss "foo", the
            # ancestor would be NotIgnored, and the negation "!foo/bar" would make the
            # result NotIgnored instead.
            pat_03_3 = _compile(s, "foo/\n!foo/bar", rid); rid += 1
            r = _match(s, [{"scope": "", "patterns": pat_03_3}], "foo/bar", False, rid); rid += 1
            v = _verdict(r)
            print(f"[03.3] directory-only pattern applies to ancestor (treated as dir): {v!r}")
            if v != "Ignored":
                failures.append(f"03.3: expected Ignored (ancestor treated as directory), got {v!r}")

            # 03.4 — only scopes at/above the ancestor are consulted for the ancestor check
            # Stack: scope "" has "top" (makes ancestor "top" Ignored); scope "top/sub" has
            # "!top" (a negation pattern deeper than ancestor "top").  Per 03.4, scope "top/sub"
            # is excluded from the ancestor-classification check for "top" because "top/sub" is
            # not an ancestor of (or equal to) "top".  Ancestor "top" remains Ignored →
            # path "top/sub/file" is Ignored.
            pat_root = _compile(s, "top", rid); rid += 1
            pat_deep = _compile(s, "!top", rid); rid += 1
            stack_03_4 = [
                {"scope": "", "patterns": pat_root},
                {"scope": "top/sub", "patterns": pat_deep},
            ]
            r = _match(s, stack_03_4, "top/sub/file", False, rid); rid += 1
            v = _verdict(r)
            print(f"[03.4] deeper-scope entry excluded from ancestor check: {v!r}")
            if v != "Ignored":
                failures.append(f"03.4: expected Ignored (deeper scope excluded from ancestor check), got {v!r}")

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
