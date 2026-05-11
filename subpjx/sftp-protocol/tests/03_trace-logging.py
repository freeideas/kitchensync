#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Verify trace log lines emitted on acquire and release (REQ 03.10–03.12)."""

from __future__ import annotations

import json, os, re, socket, subprocess, sys, threading, time
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TEST_USER = "ace"
TEST_HOST = "localhost"
TEST_MC = 3


def _drain(stream):
    for _ in stream:
        pass


def _collect(stream, dest):
    for line in stream:
        dest.append(line.rstrip("\n"))


def _launch():
    env = dict(os.environ)
    env["VERBOSITY"] = "trace"
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8", env=env,
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
    stderr_log: list[str] = []
    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    threading.Thread(target=_collect, args=(proc.stderr, stderr_log), daemon=True).start()
    return proc, port, stderr_log


def _rpc(sock, rpc_id, method, params=None):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + 20
    while time.time() < deadline:
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _wait_for(log, start, keyword, timeout=3.0):
    """Return lines past start that contain keyword, waiting up to timeout."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        hits = [l for l in log[start:] if keyword in l]
        if hits:
            return hits
        time.sleep(0.05)
    return [l for l in log[start:] if keyword in l]


def main() -> int:
    proc, port, stderr_log = _launch()
    failures: list[str] = []
    rpc_id = [0]

    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:

            def rpc(method, params=None):
                rpc_id[0] += 1
                return _rpc(s, rpc_id[0], method, params)

            # Setup: open an endpoint against the real SFTP server on localhost.
            r = rpc("tools/call", {"name": "open-endpoint", "arguments": {
                "user": TEST_USER,
                "host": TEST_HOST,
                "settings": {"mc": TEST_MC, "ct": 10, "ka": 60},
            }})
            if "error" in r:
                failures.append(
                    f"setup: open-endpoint failed: {r['error'].get('message')}"
                )
            else:
                endpoint_id = (r.get("result") or {}).get("endpoint_id")
                print(f"[setup] endpoint_id={endpoint_id!r}")

                # REQ 03.10 — acquire emits exactly one trace log line
                before_acq = len(stderr_log)
                r_acq = rpc("tools/call", {"name": "acquire", "arguments": {
                    "endpoint_id": endpoint_id,
                }})

                if "error" in r_acq:
                    print(f"[03.10] acquire error: {r_acq['error'].get('message')}")
                    failures.append("03.10: acquire call failed — no trace line to check")
                    failures.append("03.11: skipped (acquire failed)")
                else:
                    conn_id = (r_acq.get("result") or {}).get("connection_id")
                    _wait_for(stderr_log, before_acq, "connections=")
                    time.sleep(0.15)  # let any additional lines arrive
                    acq_hits = [l for l in stderr_log[before_acq:] if "connections=" in l]
                    print(f"[03.10] trace lines after acquire: {len(acq_hits)}")
                    if len(acq_hits) != 1:
                        failures.append(
                            f"03.10: expected exactly 1 trace log line on acquire, got {len(acq_hits)}"
                        )

                    # REQ 03.12 — acquire line contains endpoint=<user@host> and connections=<in_use>/<mc>
                    if acq_hits:
                        acq_line = acq_hits[0]
                        print(f"[03.12/acquire] {acq_line!r}")
                        if f"endpoint={TEST_USER}@{TEST_HOST}" not in acq_line:
                            failures.append(
                                f"03.12: acquire log missing 'endpoint={TEST_USER}@{TEST_HOST}'"
                            )
                        m = re.search(r"connections=(\d+)/(\d+)", acq_line)
                        if not m:
                            failures.append(
                                "03.12: acquire log 'connections=' not in <in_use>/<mc> format"
                            )
                        else:
                            if int(m.group(2)) != TEST_MC:
                                failures.append(
                                    f"03.12: acquire log connections denominator is {m.group(2)}, expected {TEST_MC}"
                                )
                            else:
                                print(f"[03.12/acquire] connections={m.group(1)}/{m.group(2)} — ok")

                    # REQ 03.11 — release emits exactly one trace log line
                    before_rel = len(stderr_log)
                    r_rel = rpc("tools/call", {"name": "release", "arguments": {
                        "connection_id": conn_id,
                    }})

                    if "error" in r_rel:
                        print(f"[03.11] release error: {r_rel['error'].get('message')}")
                        failures.append("03.11: release call failed — no trace line to check")
                    else:
                        _wait_for(stderr_log, before_rel, "connections=")
                        time.sleep(0.15)  # let any additional lines arrive
                        rel_hits = [l for l in stderr_log[before_rel:] if "connections=" in l]
                        print(f"[03.11] trace lines after release: {len(rel_hits)}")
                        if len(rel_hits) != 1:
                            failures.append(
                                f"03.11: expected exactly 1 trace log line on release, got {len(rel_hits)}"
                            )

                        # REQ 03.12 — release line contains endpoint=<user@host> and connections=<in_use>/<mc>
                        if rel_hits:
                            rel_line = rel_hits[0]
                            print(f"[03.12/release] {rel_line!r}")
                            if f"endpoint={TEST_USER}@{TEST_HOST}" not in rel_line:
                                failures.append(
                                    f"03.12: release log missing 'endpoint={TEST_USER}@{TEST_HOST}'"
                                )
                            m = re.search(r"connections=(\d+)/(\d+)", rel_line)
                            if not m:
                                failures.append(
                                    "03.12: release log 'connections=' not in <in_use>/<mc> format"
                                )
                            else:
                                if int(m.group(2)) != TEST_MC:
                                    failures.append(
                                        f"03.12: release log connections denominator is {m.group(2)}, expected {TEST_MC}"
                                    )
                                else:
                                    print(f"[03.12/release] connections={m.group(1)}/{m.group(2)} — ok")

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
