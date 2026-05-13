#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Glob metacharacter and double-star semantics (req 03_glob-tokens)."""

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
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _compile(sock, text, rpc_id):
    resp = _rpc(sock, "tools/call", {"name": "compile", "arguments": {"text": text}}, rpc_id)
    if "error" in resp:
        raise RuntimeError(f"compile failed: {resp['error']}")
    result = resp.get("result", {})
    # Extract the inner patterns value if wrapped under a "patterns" key.
    return result.get("patterns", result)


def _match(sock, scope_patterns, path, is_directory, rpc_id):
    stack = [{"scope": s, "patterns": p} for s, p in scope_patterns]
    resp = _rpc(sock, "tools/call", {
        "name": "match",
        "arguments": {"stack": stack, "relative_path": path, "is_directory": is_directory},
    }, rpc_id)
    if "error" in resp:
        raise RuntimeError(f"match failed: {resp['error']}")
    result = resp.get("result", {})
    return result.get("result", "")


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            _id = 0

            def nid():
                nonlocal _id
                _id += 1
                return _id

            # Sanity: both tools must be present before running assertions.
            tl = _rpc(s, "tools/list", rpc_id=nid())
            tools = {t["name"] for t in (tl.get("result") or {}).get("tools", [])}
            print(f"[init] tools present: {sorted(tools)}")
            if "compile" not in tools or "match" not in tools:
                failures.append("init: 'compile' and 'match' must appear in tools/list")
                print("FAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1

            def chk(pattern_text, path, is_dir):
                """Compile pattern_text and match path against it at root scope."""
                p = _compile(s, pattern_text, nid())
                return _match(s, [("", p)], path, is_dir, nid())

            # --- 03.1: * matches any run (including empty) of non-/ chars ---

            r = chk("*.log", "error.log", False)
            print(f"[03.1a] *.log vs error.log → {r}")
            if r != "Ignored":
                failures.append("03.1a: *.log should match error.log")

            r = chk("*.log", ".log", False)
            print(f"[03.1b] *.log vs .log (empty run) → {r}")
            if r != "Ignored":
                failures.append("03.1b: *.log should match .log (empty run of non-/ chars)")

            # Pattern with / is anchored; * inside src/ must not span into sub/.
            r = chk("src/*.log", "src/sub/error.log", False)
            print(f"[03.1c] src/*.log vs src/sub/error.log (* cannot cross /) → {r}")
            if r != "NotIgnored":
                failures.append("03.1c: src/*.log must not match src/sub/error.log")

            # --- 03.2: ? matches exactly one non-/ char ---

            r = chk("fo?", "foo", False)
            print(f"[03.2a] fo? vs foo → {r}")
            if r != "Ignored":
                failures.append("03.2a: fo? should match foo")

            r = chk("fo?", "fo", False)
            print(f"[03.2b] fo? vs fo (zero chars, should not match) → {r}")
            if r != "NotIgnored":
                failures.append("03.2b: fo? must not match fo (needs exactly one char)")

            r = chk("fo?", "fooo", False)
            print(f"[03.2c] fo? vs fooo (two extra chars, should not match) → {r}")
            if r != "NotIgnored":
                failures.append("03.2c: fo? must not match fooo (? is exactly one char)")

            r = chk("src/f?o.txt", "src/f/o.txt", False)
            print(f"[03.2d] src/f?o.txt vs src/f/o.txt (? cannot match /) → {r}")
            if r != "NotIgnored":
                failures.append("03.2d: src/f?o.txt must not match src/f/o.txt (? cannot match /)")

            # --- 03.3: [abc] matches exactly one char from the listed set ---

            r = chk("[abc].txt", "a.txt", False)
            print(f"[03.3a] [abc].txt vs a.txt → {r}")
            if r != "Ignored":
                failures.append("03.3a: [abc].txt should match a.txt")

            r = chk("[abc].txt", "d.txt", False)
            print(f"[03.3b] [abc].txt vs d.txt → {r}")
            if r != "NotIgnored":
                failures.append("03.3b: [abc].txt must not match d.txt")

            # --- 03.4: [a-z] matches exactly one char in the inclusive range ---

            r = chk("[a-z].txt", "b.txt", False)
            print(f"[03.4a] [a-z].txt vs b.txt → {r}")
            if r != "Ignored":
                failures.append("03.4a: [a-z].txt should match b.txt")

            r = chk("[a-z].txt", "1.txt", False)
            print(f"[03.4b] [a-z].txt vs 1.txt → {r}")
            if r != "NotIgnored":
                failures.append("03.4b: [a-z].txt must not match 1.txt")

            # --- 03.5: [!abc] matches exactly one char NOT in the listed set ---

            r = chk("[!abc].txt", "d.txt", False)
            print(f"[03.5a] [!abc].txt vs d.txt → {r}")
            if r != "Ignored":
                failures.append("03.5a: [!abc].txt should match d.txt")

            r = chk("[!abc].txt", "a.txt", False)
            print(f"[03.5b] [!abc].txt vs a.txt → {r}")
            if r != "NotIgnored":
                failures.append("03.5b: [!abc].txt must not match a.txt")

            # --- 03.6: leading **/ matches at any depth below declaring scope ---

            r = chk("**/foo.txt", "foo.txt", False)
            print(f"[03.6a] **/foo.txt vs foo.txt (zero depth) → {r}")
            if r != "Ignored":
                failures.append("03.6a: **/foo.txt should match foo.txt at zero depth")

            r = chk("**/foo.txt", "a/foo.txt", False)
            print(f"[03.6b] **/foo.txt vs a/foo.txt (one level deep) → {r}")
            if r != "Ignored":
                failures.append("03.6b: **/foo.txt should match a/foo.txt")

            r = chk("**/foo.txt", "a/b/foo.txt", False)
            print(f"[03.6c] **/foo.txt vs a/b/foo.txt (two levels deep) → {r}")
            if r != "Ignored":
                failures.append("03.6c: **/foo.txt should match a/b/foo.txt")

            # --- 03.7: trailing /** matches every path inside the directory ---

            r = chk("dir/**", "dir/foo.txt", False)
            print(f"[03.7a] dir/** vs dir/foo.txt → {r}")
            if r != "Ignored":
                failures.append("03.7a: dir/** should match dir/foo.txt")

            r = chk("dir/**", "dir/sub/foo.txt", False)
            print(f"[03.7b] dir/** vs dir/sub/foo.txt → {r}")
            if r != "Ignored":
                failures.append("03.7b: dir/** should match dir/sub/foo.txt")

            # --- 03.8: a/**/b matches b zero or more directories below a ---

            r = chk("a/**/b", "a/b", False)
            print(f"[03.8a] a/**/b vs a/b (zero intermediate dirs) → {r}")
            if r != "Ignored":
                failures.append("03.8a: a/**/b should match a/b (zero dirs between)")

            r = chk("a/**/b", "a/x/b", False)
            print(f"[03.8b] a/**/b vs a/x/b (one intermediate dir) → {r}")
            if r != "Ignored":
                failures.append("03.8b: a/**/b should match a/x/b")

            r = chk("a/**/b", "a/x/y/b", False)
            print(f"[03.8c] a/**/b vs a/x/y/b (two intermediate dirs) → {r}")
            if r != "Ignored":
                failures.append("03.8c: a/**/b should match a/x/y/b")

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
