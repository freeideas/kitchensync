#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises 02_match-stack.md: deciding ignored/not-ignored across a layered pattern stack."""

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
    resp = _rpc(sock, "tools/call",
                {"name": "compile", "arguments": {"text": text}},
                rpc_id)
    return (resp.get("result") or {}).get("patterns")


def _match(sock, stack, relative_path, is_directory, rpc_id):
    resp = _rpc(sock, "tools/call",
                {"name": "match", "arguments": {
                    "stack": stack,
                    "relative_path": relative_path,
                    "is_directory": is_directory,
                }},
                rpc_id)
    return (resp.get("result") or {}).get("result")


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rid = 1

            pat_log = _compile(s, "*.log", rid); rid += 1
            pat_neg_debug = _compile(s, "!debug.log", rid); rid += 1
            pat_log_then_neg = _compile(s, "*.log\n!debug.log", rid); rid += 1
            pat_exact = _compile(s, "debug.log", rid); rid += 1

            # 02.1 — empty stack returns NotIgnored for every path
            v = _match(s, [], "foo/bar.log", False, rid); rid += 1
            print(f"[02.1] empty stack → NotIgnored: {v!r}")
            if v != "NotIgnored":
                failures.append(f"02.1: expected NotIgnored, got {v!r}")

            # 02.2 — no pattern matches → NotIgnored
            v = _match(s, [{"scope": "", "patterns": pat_log}], "readme.txt", False, rid); rid += 1
            print(f"[02.2] no matching pattern → NotIgnored: {v!r}")
            if v != "NotIgnored":
                failures.append(f"02.2: expected NotIgnored, got {v!r}")

            # 02.3 — most-recently-applied matching pattern is positive → Ignored
            v = _match(s, [{"scope": "", "patterns": pat_log}], "debug.log", False, rid); rid += 1
            print(f"[02.3] positive pattern matches → Ignored: {v!r}")
            if v != "Ignored":
                failures.append(f"02.3: expected Ignored, got {v!r}")

            # 02.4 — most-recently-applied matching pattern is a negation → NotIgnored
            # pat_log_then_neg: "*.log" then "!debug.log"; last match on "debug.log" is the negation
            v = _match(s, [{"scope": "", "patterns": pat_log_then_neg}], "debug.log", False, rid); rid += 1
            print(f"[02.4] most-recent match is negation → NotIgnored: {v!r}")
            if v != "NotIgnored":
                failures.append(f"02.4: expected NotIgnored, got {v!r}")

            # 02.5 — deeper stack entries override shallower ones
            # Entry 0 (shallower): *.log → Ignored; Entry 1 (deeper): !debug.log → overrides to NotIgnored
            stack_05 = [
                {"scope": "", "patterns": pat_log},
                {"scope": "", "patterns": pat_neg_debug},
            ]
            v = _match(s, stack_05, "debug.log", False, rid); rid += 1
            print(f"[02.5] deeper negation overrides shallower positive → NotIgnored: {v!r}")
            if v != "NotIgnored":
                failures.append(f"02.5: expected NotIgnored (deeper entry overrides), got {v!r}")

            # 02.6 — scope D applies only to candidates strictly inside D
            stack_06 = [{"scope": "subdir", "patterns": pat_log}]
            v1 = _match(s, stack_06, "debug.log", False, rid); rid += 1
            print(f"[02.6] path outside scope → NotIgnored: {v1!r}")
            if v1 != "NotIgnored":
                failures.append(f"02.6: path outside scope expected NotIgnored, got {v1!r}")

            v2 = _match(s, stack_06, "subdir/debug.log", False, rid); rid += 1
            print(f"[02.6] path inside scope → Ignored: {v2!r}")
            if v2 != "Ignored":
                failures.append(f"02.6: path inside scope expected Ignored, got {v2!r}")

            # 02.7 — components of D stripped from candidate before matching
            # Pattern "debug.log" at scope "subdir": stripping "subdir/" from "subdir/debug.log"
            # yields "debug.log", which matches the pattern.
            stack_07 = [{"scope": "subdir", "patterns": pat_exact}]
            v = _match(s, stack_07, "subdir/debug.log", False, rid); rid += 1
            print(f"[02.7] scope components stripped before matching → Ignored: {v!r}")
            if v != "Ignored":
                failures.append(f"02.7: expected Ignored (scope stripped before match), got {v!r}")

            # 02.8 — empty-string scope applies to every candidate
            # *.log at scope "" matches "any/depth/file.log" because no-slash patterns match at any depth
            stack_08 = [{"scope": "", "patterns": pat_log}]
            v = _match(s, stack_08, "any/depth/file.log", False, rid); rid += 1
            print(f"[02.8] empty scope applies to every candidate → Ignored: {v!r}")
            if v != "Ignored":
                failures.append(f"02.8: expected Ignored (empty scope covers all candidates), got {v!r}")

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
