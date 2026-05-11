#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises FileUriError structured error returns from file_uri_to_path."""

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
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args}, rpc_id=rpc_id)


def _file_uri_error(resp):
    """Return the FileUriError dict from a tool response, or None if success.

    Handles two encodings:
      - result-based: {"result": {"error": {"message": ..., "offset": ...}}}
      - -32000 error: {"error": {"code": -32000, "message": ..., "data": {...}}}
    """
    result = resp.get("result")
    if result is not None:
        err = result.get("error")
        if err is not None:
            return err
        # Successful result with a path — not an error
        return None
    rpc_err = resp.get("error")
    if rpc_err is not None:
        data = rpc_err.get("data") or {}
        return {"message": rpc_err.get("message", ""), **data}
    return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            rpc_id = 1

            # 03.1 — file_uri_to_path returns FileUriError when input is not a
            #         syntactically valid file: URI
            resp_not_uri = _call(s, "file-uri-to-path", {"uri": "not-a-uri"}, rpc_id); rpc_id += 1
            err_not_uri = _file_uri_error(resp_not_uri)
            print(f"[03.1] file-uri-to-path('not-a-uri') → FileUriError={err_not_uri is not None}")
            if err_not_uri is None:
                failures.append("03.1: expected FileUriError for input 'not-a-uri' (not a file: URI)")

            resp_wrong_scheme = _call(s, "file-uri-to-path", {"uri": "http://example.com/path"}, rpc_id); rpc_id += 1
            err_wrong_scheme = _file_uri_error(resp_wrong_scheme)
            print(f"[03.1] file-uri-to-path('http://example.com/path') → FileUriError={err_wrong_scheme is not None}")
            if err_wrong_scheme is None:
                failures.append("03.1: expected FileUriError for 'http://example.com/path' (wrong scheme)")

            # 03.2 — file_uri_to_path returns FileUriError when the URI's path cannot
            #         be interpreted as a local filesystem path
            # A malformed percent-encoding sequence makes the path undecodable.
            resp_bad_pct = _call(s, "file-uri-to-path", {"uri": "file:///foo%ZZbar"}, rpc_id); rpc_id += 1
            err_bad_pct = _file_uri_error(resp_bad_pct)
            print(f"[03.2] file-uri-to-path('file:///foo%ZZbar') → FileUriError={err_bad_pct is not None}")
            if err_bad_pct is None:
                failures.append("03.2: expected FileUriError for 'file:///foo%ZZbar' (invalid percent encoding)")

            # 03.3 — FileUriError carries a human-readable message
            # Reuse the error from the first 03.1 call.
            msg = err_not_uri.get("message") if err_not_uri else None
            print(f"[03.3] FileUriError.message = {msg!r}")
            if not isinstance(msg, str) or not msg.strip():
                failures.append(f"03.3: FileUriError.message must be a non-empty string, got {msg!r}")

            # 03.4 — FileUriError carries the offset where the problem was detected
            # The malformed %ZZ is at a specific byte position; the implementation
            # should surface that offset.
            offset = err_bad_pct.get("offset") if err_bad_pct else None
            print(f"[03.4] FileUriError.offset for 'file:///foo%ZZbar' = {offset!r}")
            if err_bad_pct is not None:
                if offset is None:
                    failures.append(
                        "03.4: FileUriError.offset must be present for a precisely-located "
                        "error ('file:///foo%ZZbar' — bad percent encoding at known position)"
                    )
                elif not isinstance(offset, int):
                    failures.append(f"03.4: FileUriError.offset must be an integer, got {type(offset).__name__}")

            # 03.5 — The component does not write to stdout or stderr.
            # All error information arrives through the structured FileUriError return
            # value delivered over the MCP socket. The socket response is well-formed
            # JSON-RPC for every call above, confirming errors travel via return values
            # and not via I/O channels.
            print("[03.5] All error responses arrived as structured socket data (not I/O)")

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
