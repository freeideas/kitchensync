#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise open_read/read/close_read and open_write/write/close_write streaming I/O."""

from __future__ import annotations

import base64, json, os, shutil, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TEST_BASE = Path("/home/ace/Desktop/prjx/kitchensync/tmp/testks/streaming-io")
LOCALHOST = "localhost"
SSH_PORT = 22
USER = "ace"


def _credential():
    agent_sock = os.environ.get("SSH_AUTH_SOCK")
    if agent_sock:
        return {"type": "agent", "socket_path": agent_sock}
    for key in ("~/.ssh/id_ed25519", "~/.ssh/id_rsa", "~/.ssh/id_ecdsa"):
        p = Path(key).expanduser()
        if p.exists():
            return {"type": "private_key_file", "path": str(p)}
    raise RuntimeError("No SSH credential available (SSH_AUTH_SOCK not set, no key file found)")


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
    deadline = time.time() + 15
    while time.time() < deadline:
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call(sock, name, arguments, rid):
    return _rpc(sock, "tools/call", {"name": name, "arguments": arguments}, rid)


def _is_not_found(r):
    msg = ((r.get("error") or {}).get("message") or "").lower()
    res = r.get("result") or {}
    return "not_found" in msg or "not found" in msg or res.get("error") == "not_found"


def main() -> int:
    if TEST_BASE.exists():
        shutil.rmtree(TEST_BASE)
    TEST_BASE.mkdir(parents=True)

    proc, mcp_port = _launch()
    failures = []
    rid = 1

    try:
        with socket.create_connection(("127.0.0.1", mcp_port), timeout=10) as s:
            cred = _credential()

            # Open session — prerequisite for all assertions
            r = _call(s, "open-session", {
                "host": LOCALHOST,
                "port": SSH_PORT,
                "user": USER,
                "credentials": [cred],
                "connect_timeout_secs": 10,
            }, rid); rid += 1
            if "error" in r:
                print(f"[setup] open-session failed: {r['error']}")
                failures.append("setup: open-session failed — cannot continue")
                return 1
            session = r["result"]["session"]
            print(f"[setup] session opened: {session!r}")

            # Create a file for the read tests via SFTP write path
            read_path = str(TEST_BASE / "read_test.bin")
            setup_chunk1 = b"chunk-one-"
            setup_chunk2 = b"chunk-two"
            setup_content = setup_chunk1 + setup_chunk2

            r = _call(s, "open-write", {"session": session, "path": read_path}, rid); rid += 1
            if "error" in r:
                failures.append(f"setup: open-write for read fixture failed: {r['error']}")
                return 1
            wh = r["result"]["handle"]
            _call(s, "write", {"handle": wh, "bytes": base64.b64encode(setup_chunk1).decode()}, rid); rid += 1
            _call(s, "write", {"handle": wh, "bytes": base64.b64encode(setup_chunk2).decode()}, rid); rid += 1
            _call(s, "close-write", {"handle": wh}, rid); rid += 1

            # --- 02.19: open_read on existing file returns a handle ---
            r = _call(s, "open-read", {"session": session, "path": read_path}, rid); rid += 1
            print(f"[02.19] open-read result: {r}")
            read_handle = (r.get("result") or {}).get("handle")
            if "error" in r or read_handle is None:
                failures.append(f"02.19: open-read on existing file did not return a handle; got: {r}")
            else:
                print("[02.19] PASS: open-read on existing file returned a handle")

            # --- 02.20: repeated reads yield full content, each at most max_bytes ---
            accumulated = b""
            MAX = 5
            if read_handle:
                for _ in range(200):  # safety limit
                    r = _call(s, "read", {"handle": read_handle, "max_bytes": MAX}, rid); rid += 1
                    if "error" in r:
                        failures.append(f"02.20: read returned error: {r['error']}")
                        break
                    res = r.get("result") or {}
                    raw = res.get("bytes") or res.get("data") or ""
                    eof = res.get("eof", False)
                    if eof or not raw:
                        break
                    decoded = base64.b64decode(raw)
                    if len(decoded) > MAX:
                        failures.append(f"02.20: read returned {len(decoded)} bytes but max_bytes={MAX}")
                    accumulated += decoded
            print(f"[02.20] accumulated {len(accumulated)} bytes: {accumulated!r}")
            if "02.20" not in " ".join(failures):
                if accumulated != setup_content:
                    failures.append(f"02.20: expected {setup_content!r}, got {accumulated!r}")
                else:
                    print("[02.20] PASS: repeated reads yielded full file content within max_bytes each")

            # --- 02.21: read signals EOF after file fully consumed ---
            if read_handle:
                r = _call(s, "read", {"handle": read_handle, "max_bytes": 64}, rid); rid += 1
                res = r.get("result") or {}
                print(f"[02.21] post-exhaustion read: {r}")
                eof_signalled = res.get("eof", False) or not (res.get("bytes") or res.get("data"))
                if not eof_signalled:
                    failures.append(f"02.21: read after exhaustion did not signal EOF; got: {r}")
                else:
                    print("[02.21] PASS: read signals EOF once file is fully consumed")
                _call(s, "close-read", {"handle": read_handle}, rid); rid += 1

            # --- 02.22: open_read returns not_found for a missing path ---
            missing = str(TEST_BASE / "no-such-file-xyzzy-99.bin")
            r = _call(s, "open-read", {"session": session, "path": missing}, rid); rid += 1
            print(f"[02.22] open-read on missing path: {r}")
            if not _is_not_found(r):
                failures.append(f"02.22: expected not_found for missing path, got: {r}")
            else:
                print("[02.22] PASS: open-read returns not_found for missing path")

            # --- 02.23: open_write/write/close_write creates file with concatenated chunks ---
            write_path = str(TEST_BASE / "written.bin")
            ch1, ch2 = b"hello-", b"world"
            r = _call(s, "open-write", {"session": session, "path": write_path}, rid); rid += 1
            if "error" in r:
                failures.append(f"02.23: open-write failed: {r['error']}")
            else:
                wh2 = r["result"]["handle"]
                _call(s, "write", {"handle": wh2, "bytes": base64.b64encode(ch1).decode()}, rid); rid += 1
                _call(s, "write", {"handle": wh2, "bytes": base64.b64encode(ch2).decode()}, rid); rid += 1
                _call(s, "close-write", {"handle": wh2}, rid); rid += 1
                actual = Path(write_path).read_bytes() if Path(write_path).exists() else None
                print(f"[02.23] file content: {actual!r}")
                if actual != ch1 + ch2:
                    failures.append(f"02.23: expected {ch1+ch2!r}, got {actual!r}")
                else:
                    print("[02.23] PASS: file contains concatenation of written chunks")

            # --- 02.24: open_write creates missing parent directories ---
            deep_path = str(TEST_BASE / "deep" / "nested" / "dir" / "file.bin")
            r = _call(s, "open-write", {"session": session, "path": deep_path}, rid); rid += 1
            if "error" in r:
                failures.append(f"02.24: open-write with missing parents failed: {r['error']}")
            else:
                wh3 = r["result"]["handle"]
                _call(s, "write", {"handle": wh3, "bytes": base64.b64encode(b"data").decode()}, rid); rid += 1
                _call(s, "close-write", {"handle": wh3}, rid); rid += 1
                exists = Path(deep_path).exists()
                print(f"[02.24] deep file exists: {exists}")
                if not exists:
                    failures.append("02.24: open-write did not create missing parent directories")
                else:
                    print("[02.24] PASS: open-write created missing parent directories")

            _call(s, "close-session", {"session": session}, rid); rid += 1

    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
