#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises req 01_compile: parsing gitignore pattern text into a reusable pattern set."""

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


def _compile(sock, text, rpc_id):
    return _rpc(sock, "tools/call",
                {"name": "compile", "arguments": {"text": text}},
                rpc_id)


def _match(sock, patterns, relative_path, is_directory, rpc_id):
    return _rpc(sock, "tools/call",
                {"name": "match", "arguments": {
                    "stack": [{"scope": "", "patterns": patterns}],
                    "relative_path": relative_path,
                    "is_directory": is_directory,
                }},
                rpc_id)


def _patterns_from(resp):
    """Extract the opaque patterns value from a compile response."""
    return (resp.get("result") or {}).get("patterns")


def _verdict(resp):
    """Extract the verdict string from a match response."""
    return (resp.get("result") or {}).get("result")


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rpc_id = 1

            # verify the server is up and has tools
            tl = _rpc(s, "tools/list", rpc_id=rpc_id); rpc_id += 1
            tools = (tl.get("result") or {}).get("tools", [])
            tool_names = {t["name"] for t in tools}
            print(f"[tools/list] found: {sorted(tool_names)}")
            if "compile" not in tool_names:
                failures.append("tools/list: 'compile' tool missing")
            if "match" not in tool_names:
                failures.append("tools/list: 'match' tool missing")

            # --- 01.1: blank lines do not classify any path as Ignored ---
            resp = _compile(s, "\n\n   \n\n", rpc_id); rpc_id += 1
            pats = _patterns_from(resp)
            resp = _match(s, pats, "foo.txt", False, rpc_id); rpc_id += 1
            verdict = _verdict(resp)
            print(f"[01.1] blank-only input → match 'foo.txt' = {verdict!r}")
            if verdict != "NotIgnored":
                failures.append("01.1: blank lines should not ignore any path — got " + repr(verdict))

            # --- 01.2: #-comment lines do not classify any path as Ignored ---
            resp = _compile(s, "# foo.txt\n# *.log\n", rpc_id); rpc_id += 1
            pats = _patterns_from(resp)
            resp = _match(s, pats, "foo.txt", False, rpc_id); rpc_id += 1
            verdict = _verdict(resp)
            print(f"[01.2] comment-only input → match 'foo.txt' = {verdict!r}")
            if verdict != "NotIgnored":
                failures.append("01.2: comment lines should not ignore any path — got " + repr(verdict))

            # --- 01.3: later pattern overrides earlier (negation beats prior positive) ---
            resp = _compile(s, "*.txt\n!foo.txt\n", rpc_id); rpc_id += 1
            pats = _patterns_from(resp)
            resp = _match(s, pats, "foo.txt", False, rpc_id); rpc_id += 1
            verdict = _verdict(resp)
            print(f"[01.3] '*.txt' then '!foo.txt' → match 'foo.txt' = {verdict!r}")
            if verdict != "NotIgnored":
                failures.append("01.3: later negation must override earlier positive — got " + repr(verdict))

            # second direction: later positive overrides earlier negation
            resp = _compile(s, "!foo.txt\n*.txt\n", rpc_id); rpc_id += 1
            pats = _patterns_from(resp)
            resp = _match(s, pats, "foo.txt", False, rpc_id); rpc_id += 1
            verdict = _verdict(resp)
            print(f"[01.3] '!foo.txt' then '*.txt' → match 'foo.txt' = {verdict!r}")
            if verdict != "Ignored":
                failures.append("01.3: later positive must override earlier negation — got " + repr(verdict))

            # --- 01.4: compile("") produces a pattern set that ignores nothing ---
            resp = _compile(s, "", rpc_id); rpc_id += 1
            pats = _patterns_from(resp)
            resp = _match(s, pats, "anything.txt", False, rpc_id); rpc_id += 1
            verdict = _verdict(resp)
            print(f"[01.4] compile('') → match 'anything.txt' = {verdict!r}")
            if verdict != "NotIgnored":
                failures.append("01.4: compile('') must not ignore anything — got " + repr(verdict))

            # --- 01.5: trailing whitespace is stripped ---
            # "foo.txt   " with 3 trailing spaces becomes pattern "foo.txt"
            resp = _compile(s, "foo.txt   \n", rpc_id); rpc_id += 1
            pats = _patterns_from(resp)
            resp = _match(s, pats, "foo.txt", False, rpc_id); rpc_id += 1
            verdict = _verdict(resp)
            print(f"[01.5] 'foo.txt   ' (trailing spaces stripped) → match 'foo.txt' = {verdict!r}")
            if verdict != "Ignored":
                failures.append("01.5: trailing spaces must be stripped so 'foo.txt' matches — got " + repr(verdict))

            # escaped trailing space is preserved (the "unless escaped with \\" half of 01.5)
            resp = _compile(s, "foo.txt\\ \n", rpc_id); rpc_id += 1
            pats = _patterns_from(resp)
            resp = _match(s, pats, "foo.txt ", False, rpc_id); rpc_id += 1
            verdict = _verdict(resp)
            print(f"[01.5] 'foo.txt\\ ' (escaped trailing space preserved) → match 'foo.txt ' = {verdict!r}")
            if verdict != "Ignored":
                failures.append("01.5: escaped trailing space must be preserved so 'foo.txt ' matches — got " + repr(verdict))

            # --- 01.6: \# escape makes the line a pattern, not a comment ---
            resp = _compile(s, "\\#secret.txt\n", rpc_id); rpc_id += 1
            pats = _patterns_from(resp)
            resp = _match(s, pats, "#secret.txt", False, rpc_id); rpc_id += 1
            verdict = _verdict(resp)
            print(f"[01.6] '\\#secret.txt' → match '#secret.txt' = {verdict!r}")
            if verdict != "Ignored":
                failures.append("01.6: \\# escape must produce pattern matching '#secret.txt' — got " + repr(verdict))

            # --- 01.7: \! escape makes the leading ! literal, not a negation marker ---
            resp = _compile(s, "\\!foo.txt\n", rpc_id); rpc_id += 1
            pats = _patterns_from(resp)
            resp = _match(s, pats, "!foo.txt", False, rpc_id); rpc_id += 1
            verdict = _verdict(resp)
            print(f"[01.7] '\\!foo.txt' → match '!foo.txt' = {verdict!r}")
            if verdict != "Ignored":
                failures.append("01.7: \\! escape must produce pattern matching '!foo.txt' literally — got " + repr(verdict))

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
