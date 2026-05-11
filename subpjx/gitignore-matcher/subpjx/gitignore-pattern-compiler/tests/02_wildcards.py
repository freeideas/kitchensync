#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Tests single-segment wildcards and character classes per 02_wildcards.md."""

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
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _compile_pattern(sock, pattern_text):
    """Compile a single-line pattern file and return the CompiledPattern at index 0."""
    r = _rpc(sock, "tools/call", {
        "name": "compile-patterns",
        "arguments": {"text": pattern_text},
    })
    if "error" in r:
        raise RuntimeError(f"compile-patterns error: {r['error']}")
    result = r["result"]
    if "pattern_set" not in result:
        raise RuntimeError(f"compile-patterns missing 'pattern_set': {result}")
    ps = result["pattern_set"]

    r2 = _rpc(sock, "tools/call", {
        "name": "pattern-at",
        "arguments": {"set": ps, "index": 0},
    })
    if "error" in r2:
        raise RuntimeError(f"pattern-at error: {r2['error']}")
    return r2["result"]


def _matches(sock, compiled_pattern, path):
    """Invoke the matches predicate on a compiled pattern returned by pattern-at."""
    r = _rpc(sock, "tools/call", {
        "name": "matches",
        "arguments": {"compiled_pattern": compiled_pattern, "path": path},
    })
    if "error" in r:
        raise RuntimeError(f"matches error: {r['error']}")
    result = r["result"]
    if "matches" not in result:
        raise RuntimeError(f"matches result missing 'matches' key: {result}")
    return result["matches"]


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            tl = _rpc(s, "tools/list")
            tools = (tl.get("result") or {}).get("tools", [])
            print(f"tools/list returned {len(tools)} tool(s): {[t['name'] for t in tools]}")

            # 02.1 — * matches a possibly-empty run of non-/ characters
            try:
                cp = _compile_pattern(s, "*.txt")
                yes_normal = _matches(s, cp, "foo.txt")
                yes_empty  = _matches(s, cp, ".txt")        # empty run — still matches
                no_slash   = _matches(s, cp, "foo/bar.txt") # * must not cross /
                ok = yes_normal and yes_empty and not no_slash
                print(f"[02.1] *: 'foo.txt'->{yes_normal}, '.txt'->{yes_empty}, 'foo/bar.txt'->{no_slash} -> {'PASS' if ok else 'FAIL'}")
                if not ok:
                    failures.append("02.1: * wildcard misbehaved")
            except Exception as e:
                failures.append(f"02.1: exception: {e}")
                print(f"[02.1] FAIL: {e}")

            # 02.2 — ? matches exactly one non-/ character
            try:
                cp = _compile_pattern(s, "?.txt")
                yes_one   = _matches(s, cp, "a.txt")   # one char
                no_zero   = _matches(s, cp, ".txt")    # zero chars — must not match
                no_two    = _matches(s, cp, "ab.txt")  # two chars — must not match
                cp2 = _compile_pattern(s, "?")
                no_sep    = _matches(s, cp2, "/")      # / is excluded from ?
                ok = yes_one and not no_zero and not no_two and not no_sep
                print(f"[02.2] ?: 'a.txt'->{yes_one}, '.txt'->{no_zero}, 'ab.txt'->{no_two}, '/'->{no_sep} -> {'PASS' if ok else 'FAIL'}")
                if not ok:
                    failures.append("02.2: ? wildcard misbehaved")
            except Exception as e:
                failures.append(f"02.2: exception: {e}")
                print(f"[02.2] FAIL: {e}")

            # 02.3 — [abc] matches exactly one character from the listed set
            try:
                cp = _compile_pattern(s, "[abc]")
                yes_a  = _matches(s, cp, "a")
                yes_b  = _matches(s, cp, "b")
                yes_c  = _matches(s, cp, "c")
                no_d   = _matches(s, cp, "d")
                no_two = _matches(s, cp, "ab")  # two chars — must not match
                ok = yes_a and yes_b and yes_c and not no_d and not no_two
                print(f"[02.3] [abc]: a->{yes_a}, b->{yes_b}, c->{yes_c}, d->{no_d}, ab->{no_two} -> {'PASS' if ok else 'FAIL'}")
                if not ok:
                    failures.append("02.3: [abc] character class misbehaved")
            except Exception as e:
                failures.append(f"02.3: exception: {e}")
                print(f"[02.3] FAIL: {e}")

            # 02.4 — [!abc] and [^abc] each match one char NOT in the set
            try:
                cp_bang = _compile_pattern(s, "[!abc]")
                cp_hat  = _compile_pattern(s, "[^abc]")
                all_ok = True
                for label, cp in [("[!abc]", cp_bang), ("[^abc]", cp_hat)]:
                    yes_d = _matches(s, cp, "d")
                    no_a  = _matches(s, cp, "a")
                    no_b  = _matches(s, cp, "b")
                    ok = yes_d and not no_a and not no_b
                    print(f"[02.4] {label}: d->{yes_d}, a->{no_a}, b->{no_b} -> {'PASS' if ok else 'FAIL'}")
                    if not ok:
                        all_ok = False
                if not all_ok:
                    failures.append("02.4: negated character class misbehaved")
            except Exception as e:
                failures.append(f"02.4: exception: {e}")
                print(f"[02.4] FAIL: {e}")

            # 02.5 — [a-z] range matches any character within the inclusive range
            try:
                cp = _compile_pattern(s, "[a-z]")
                yes_a = _matches(s, cp, "a")
                yes_m = _matches(s, cp, "m")
                yes_z = _matches(s, cp, "z")
                no_A  = _matches(s, cp, "A")
                no_0  = _matches(s, cp, "0")
                ok = yes_a and yes_m and yes_z and not no_A and not no_0
                print(f"[02.5] [a-z]: a->{yes_a}, m->{yes_m}, z->{yes_z}, A->{no_A}, 0->{no_0} -> {'PASS' if ok else 'FAIL'}")
                if not ok:
                    failures.append("02.5: [a-z] range misbehaved")
            except Exception as e:
                failures.append(f"02.5: exception: {e}")
                print(f"[02.5] FAIL: {e}")

            # 02.6 — ] as first char of a class is literal (e.g. []abc])
            try:
                cp = _compile_pattern(s, "[]abc]")
                yes_bracket = _matches(s, cp, "]")
                yes_a       = _matches(s, cp, "a")
                no_d        = _matches(s, cp, "d")
                ok = yes_bracket and yes_a and not no_d
                print(f"[02.6] []abc]: ']'->{yes_bracket}, a->{yes_a}, d->{no_d} -> {'PASS' if ok else 'FAIL'}")
                if not ok:
                    failures.append("02.6: ] as first class char misbehaved")
            except Exception as e:
                failures.append(f"02.6: exception: {e}")
                print(f"[02.6] FAIL: {e}")

            # 02.7 — backslash before a metacharacter causes it to match literally
            try:
                cp = _compile_pattern(s, r"\*.txt")  # raw: \*.txt  → literal * then .txt
                yes_literal = _matches(s, cp, "*.txt")   # literal * in path
                no_wildcard = _matches(s, cp, "foo.txt") # * was NOT a wildcard
                ok = yes_literal and not no_wildcard
                print(f"[02.7] \\*.txt: '*.txt'->{yes_literal}, 'foo.txt'->{no_wildcard} -> {'PASS' if ok else 'FAIL'}")
                if not ok:
                    failures.append("02.7: backslash escape misbehaved")
            except Exception as e:
                failures.append(f"02.7: exception: {e}")
                print(f"[02.7] FAIL: {e}")

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
