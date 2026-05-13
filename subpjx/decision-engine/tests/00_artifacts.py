#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises reqs/00_artifacts.md through the decision-engine MCP wrapper."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")
TOOL_NAME = "decide-entry"

# 00.3 not reasonably testable: this Python test can reach the library only by
# launching the MCP wrapper and sending JSON-RPC over a socket, which necessarily
# performs process, stdout, and loopback-network I/O. The decide_entry API has no
# path, host, or storage handle, so arbitrary library I/O has no cheap portable
# observable through this project's MCP wrapper or CLI.


def _drain(stream: Any) -> None:
    for _ in stream:
        pass


def _launch() -> tuple[subprocess.Popen[str], int]:
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    if proc.stdout is None or proc.stderr is None:
        proc.terminate()
        raise RuntimeError("MCP server pipes were not captured")

    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
        if not line:
            continue
        line = line.strip()
        if line.startswith("MCP_PORT="):
            port = int(line.split("=", 1)[1])
            threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
            threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
            return proc, port

    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
    stderr = proc.stderr.read()
    raise RuntimeError(f"MCP server did not advertise MCP_PORT: {stderr.strip()}")


def _rpc(
    sock: socket.socket,
    method: str,
    params: dict[str, Any] | None = None,
    rpc_id: int = 1,
) -> dict[str, Any]:
    request: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        request["params"] = params
    sock.sendall((json.dumps(request, separators=(",", ":")) + "\n").encode("utf-8"))

    buffer = b""
    deadline = time.time() + 10
    while time.time() < deadline:
        chunk = sock.recv(8192)
        if not chunk:
            break
        buffer += chunk
        if b"\n" in buffer:
            break
    line, _, _ = buffer.partition(b"\n")
    if not line:
        raise RuntimeError(f"no response for {method}")
    return json.loads(line.decode("utf-8"))


def _decision_arguments() -> dict[str, Any]:
    return {
        "roles": {
            "alpha": "canon",
            "beta": "contributing",
            "gamma": "subordinate",
        },
        "observations": {
            "alpha": {"kind": "File", "mod_time": 1001, "byte_size": 11},
            "beta": {"kind": "Absent"},
            "gamma": {"kind": "Absent"},
        },
        "histories": {
            "alpha": {
                "mod_time": 1000,
                "byte_size": 11,
                "last_seen": 1000,
                "deleted_time": None,
            },
        },
        "tolerance": 0,
    }


def _call_decide(sock: socket.socket, arguments: dict[str, Any], rpc_id: int) -> tuple[dict[str, Any], Any]:
    response = _rpc(
        sock,
        "tools/call",
        {"name": TOOL_NAME, "arguments": arguments},
        rpc_id=rpc_id,
    )
    return response, response.get("result")


def _canonical(value: Any) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


def _expected_decision() -> dict[str, Any]:
    return {
        "actions": {
            "alpha": {"kind": "NoOp"},
            "beta": {"kind": "ReceiveFile", "source": "alpha"},
            "gamma": {"kind": "ReceiveFile", "source": "alpha"},
        },
        "classifications": {
            "alpha": "Modified",
            "beta": "NoOpinion",
            "gamma": "NoOpinion",
        },
        "entry_kind": "File",
        "winning_byte_size": 11,
        "winning_mod_time": 1001,
        "winning_source": "alpha",
    }


def _print_result(label: str, passed: bool) -> None:
    print(f"[{label}] {'PASS' if passed else 'FAIL'}")


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            failures: list[str] = []
            arguments = _decision_arguments()
            expected = _expected_decision()

            tools_response = _rpc(sock, "tools/list", rpc_id=1)
            tools = (tools_response.get("result") or {}).get("tools", [])
            tool_names = [tool.get("name") for tool in tools if isinstance(tool, dict)]
            response1: dict[str, Any] | None = None
            decision1: Any = None
            if tool_names == [TOOL_NAME]:
                response1, decision1 = _call_decide(sock, arguments, rpc_id=2)

            req_001 = response1 is not None and "error" not in response1 and decision1 == expected
            _print_result("00.1 decide-entry accepts roles, observations, histories, and tolerance and returns one decision", req_001)
            if not req_001:
                failures.append(
                    "00.1: expected one callable decide-entry operation returning "
                    f"{expected!r}; tools={tool_names!r}, response={response1!r}"
                )

            response2: dict[str, Any] | None = None
            decision2: Any = None
            if tool_names == [TOOL_NAME]:
                response2, decision2 = _call_decide(sock, arguments, rpc_id=3)
            req_002 = (
                response1 is not None
                and response2 is not None
                and "error" not in response1
                and "error" not in response2
                and _canonical(decision1) == _canonical(decision2)
            )
            _print_result("00.2 same decide-entry inputs return the same decision", req_002)
            if not req_002:
                failures.append(f"00.2: repeated call differed; first={decision1!r}, second={decision2!r}")

            if failures:
                print("\nFAILURES:")
                for failure in failures:
                    print(f"  - {failure}")
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
