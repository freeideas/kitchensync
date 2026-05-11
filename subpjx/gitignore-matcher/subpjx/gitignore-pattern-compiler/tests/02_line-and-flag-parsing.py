#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Line preprocessing and flag parsing: blank/comment skipping, whitespace stripping, negation/anchoring/dir-only flags."""

from __future__ import annotations

import json, os, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

_rpc_counter = [0]


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
    _rpc_counter[0] += 1
    msg = {"jsonrpc": "2.0", "id": _rpc_counter[0], "method": method}
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


def _call(sock, tool, arguments):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": arguments})


def _compile(sock, text):
    resp = _call(sock, "compile-patterns", {"text": text})
    return resp.get("result", {})


def _count(sock, ps):
    resp = _call(sock, "pattern-count", {"pattern_set": ps})
    return resp.get("result", {}).get("count", -1)


def _at(sock, ps, index):
    resp = _call(sock, "pattern-at", {"pattern_set": ps, "index": index})
    return resp.get("result", {})


def _matches(sock, ps, index, path):
    resp = _call(sock, "matches", {"pattern_set": ps, "index": index, "path": path})
    return resp.get("result", {}).get("matches", None)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # 02.1 — blank line produces no compiled pattern
            r = _compile(s, "   ")
            ps = r.get("pattern_set")
            count = _count(s, ps)
            print(f"[02.1] blank line → count={count}")
            if count != 0:
                failures.append(f"02.1: expected 0 patterns for blank line, got {count}")

            # 02.2 — line whose first non-whitespace character is # produces no pattern
            r = _compile(s, "  # comment line")
            ps = r.get("pattern_set")
            count = _count(s, ps)
            print(f"[02.2] comment line → count={count}")
            if count != 0:
                failures.append(f"02.2: expected 0 patterns for comment line, got {count}")

            # 02.3 — \# produces a pattern whose body begins with literal #
            r = _compile(s, "\\#literal")
            ps = r.get("pattern_set")
            count = _count(s, ps)
            m = _matches(s, ps, 0, "#literal") if count > 0 else None
            print(f"[02.3] \\#literal → count={count}, matches('#literal')={m}")
            if count != 1:
                failures.append(f"02.3: expected 1 pattern for \\# escape (not skipped as comment), got {count}")
            elif m is not True:
                failures.append(f"02.3: expected matches('#literal')=true (body starts with #), got {m}")

            # 02.4 — unescaped trailing whitespace is stripped
            r = _compile(s, "foo   ")
            ps = r.get("pattern_set")
            pat = _at(s, ps, 0)
            src = pat.get("source", None)
            print(f"[02.4] 'foo   ' → source={repr(src)}")
            if src != "foo":
                failures.append(f"02.4: expected source 'foo' after stripping trailing whitespace, got {repr(src)}")

            # 02.5 — backslash-protected trailing whitespace is retained in body
            # Python "foo\\ " is the 5-char string: f o o \ <space>
            r = _compile(s, "foo\\ ")
            ps = r.get("pattern_set")
            pat = _at(s, ps, 0)
            src = pat.get("source", None)
            print(f"[02.5] 'foo\\ ' → source={repr(src)}")
            if src is None or not src.endswith(" "):
                failures.append(f"02.5: expected source to retain trailing space when preceded by backslash, got {repr(src)}")

            # 02.6 — leading ! sets is_negation=true and is removed from body
            r = _compile(s, "!foo")
            ps = r.get("pattern_set")
            pat = _at(s, ps, 0)
            neg = pat.get("is_negation")
            m = _matches(s, ps, 0, "foo")
            print(f"[02.6] '!foo' → is_negation={neg}, matches('foo')={m}")
            if neg is not True:
                failures.append(f"02.6: expected is_negation=true for '!foo', got {neg}")
            if m is not True:
                failures.append(f"02.6: expected matches('foo')=true (! removed from body), got {m}")

            # 02.7 — leading / sets is_anchored=true and is removed from body
            r = _compile(s, "/foo")
            ps = r.get("pattern_set")
            pat = _at(s, ps, 0)
            anch = pat.get("is_anchored")
            m = _matches(s, ps, 0, "foo")
            print(f"[02.7] '/foo' → is_anchored={anch}, matches('foo')={m}")
            if anch is not True:
                failures.append(f"02.7: expected is_anchored=true for '/foo', got {anch}")
            if m is not True:
                failures.append(f"02.7: expected matches('foo')=true (leading / removed from body), got {m}")

            # 02.8 — trailing / sets is_dir_only=true and is removed from body
            r = _compile(s, "foo/")
            ps = r.get("pattern_set")
            pat = _at(s, ps, 0)
            donly = pat.get("is_dir_only")
            m = _matches(s, ps, 0, "foo")
            print(f"[02.8] 'foo/' → is_dir_only={donly}, matches('foo')={m}")
            if donly is not True:
                failures.append(f"02.8: expected is_dir_only=true for 'foo/', got {donly}")
            if m is not True:
                failures.append(f"02.8: expected matches('foo')=true (trailing / removed from body), got {m}")

            # 02.9 — plain pattern has all three flags false
            r = _compile(s, "plainpattern")
            ps = r.get("pattern_set")
            pat = _at(s, ps, 0)
            neg = pat.get("is_negation")
            anch = pat.get("is_anchored")
            donly = pat.get("is_dir_only")
            print(f"[02.9] 'plainpattern' → is_negation={neg}, is_anchored={anch}, is_dir_only={donly}")
            if neg is not False:
                failures.append(f"02.9: expected is_negation=false for plain pattern, got {neg}")
            if anch is not False:
                failures.append(f"02.9: expected is_anchored=false for plain pattern, got {anch}")
            if donly is not False:
                failures.append(f"02.9: expected is_dir_only=false for plain pattern, got {donly}")

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
