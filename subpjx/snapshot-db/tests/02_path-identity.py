#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["xxhash"]
# ///
"""Exercises path-identity: identify() returns 11-char base62 xxHash64 IDs."""

from __future__ import annotations

import json, os, shutil, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")
ROOT = Path(__file__).resolve().parent.parent
TEST_ROOT = ROOT / "tmp" / "testks" / "02_path_identity"

ALPHABET = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"

_rpc_counter = [0]


def _base62_11(n: int) -> str:
    n &= 0xFFFFFFFFFFFFFFFF
    digits = []
    for _ in range(11):
        digits.append(ALPHABET[n % 62])
        n //= 62
    return "".join(reversed(digits))


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


def _unwrap(resp):
    result = resp.get("result")
    if not isinstance(result, dict):
        return result
    content = result.get("content", [])
    if content and content[0].get("type") == "text":
        text = content[0]["text"]
        try:
            return json.loads(text)
        except (json.JSONDecodeError, ValueError):
            return text
    return result


def _identify(sock, path):
    return _unwrap(_call(sock, "identify", {"path": path}))


def _identify_once(path):
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            return _identify(s, path)
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


def main() -> int:
    shutil.rmtree(TEST_ROOT, ignore_errors=True)
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []

            # 02.1 — identify(p) returns a string exactly 11 characters long
            id_11 = _identify(s, "docs/readme.txt")
            print(f"[02.1] identify('docs/readme.txt') => {id_11!r}")
            if not isinstance(id_11, str) or len(id_11) != 11:
                failures.append(f"02.1: expected 11-char string, got {id_11!r}")
            else:
                print("[02.1] PASS")

            # 02.2 — returned string contains only 0-9, A-Z, a-z
            id_chars = _identify(s, "src/main/App.java")
            print(f"[02.2] identify('src/main/App.java') => {id_chars!r}")
            if not isinstance(id_chars, str):
                failures.append(f"02.2: expected string, got {id_chars!r}")
            elif any(c not in ALPHABET for c in id_chars):
                bad = [c for c in id_chars if c not in ALPHABET]
                failures.append(f"02.2: non-base62 chars {bad!r} in {id_chars!r}")
            else:
                print("[02.2] PASS")

            # 02.3 — identify("") and identify("/") return the same value
            id_empty = _identify(s, "")
            id_slash = _identify(s, "/")
            print(f"[02.3] identify('') => {id_empty!r}, identify('/') => {id_slash!r}")
            if id_empty != id_slash:
                failures.append(f"02.3: identify('') {id_empty!r} != identify('/') {id_slash!r}")
            else:
                print("[02.3] PASS")

            # 02.4 — same input returns same value on every call and across MCP launches
            id_a = _identify(s, "config/settings.json")
            id_b = _identify(s, "config/settings.json")
            id_c = _identify_once("config/settings.json")
            print(f"[02.4] identify('config/settings.json') => {id_a!r}, {id_b!r}, fresh MCP {id_c!r}")
            if id_a != id_b or id_a != id_c:
                failures.append(f"02.4: non-deterministic: {id_a!r}, {id_b!r}, {id_c!r}")
            else:
                print("[02.4] PASS")

            # 02.5 — files and directories at the same path string share an identity
            same_path = "tmp/testks/02_path_identity/same"
            TEST_ROOT.mkdir(parents=True, exist_ok=True)
            (TEST_ROOT / "same").write_text("file", encoding="utf-8")
            id_file = _identify(s, same_path)
            (TEST_ROOT / "same").unlink()
            (TEST_ROOT / "same").mkdir()
            id_dir = _identify(s, same_path)
            shutil.rmtree(TEST_ROOT, ignore_errors=True)
            id_absent = _identify(s, same_path)
            print(f"[02.5] file => {id_file!r}, dir => {id_dir!r}, absent => {id_absent!r}")
            if id_file != id_dir or id_file != id_absent:
                failures.append(f"02.5: identity depended on filesystem state: {id_file!r}, {id_dir!r}, {id_absent!r}")
            else:
                print("[02.5] PASS")

            # 02.6 — equals base62-zero-padded-to-11 of xxHash64(p as UTF-8, seed=0)
            import xxhash
            for test_path in (
                "",
                "/",
                "src/main/App.java",
                "a/b/c",
                "caf\u00e9/\u6771\u4eac.txt",
                "long/" + ("segment/" * 5) + "file.txt",
            ):
                hash_path = "" if test_path == "/" else test_path
                raw = xxhash.xxh64(hash_path.encode("utf-8"), seed=0).intdigest()
                expected = _base62_11(raw)
                actual = _identify(s, test_path)
                print(f"[02.6] identify({test_path!r}) => {actual!r}, expected {expected!r}")
                if actual != expected:
                    failures.append(f"02.6: path={test_path!r} expected {expected!r} got {actual!r}")
                else:
                    print(f"[02.6] PASS path={test_path!r}")

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
