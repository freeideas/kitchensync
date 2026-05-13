#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise type-conflict and directory decision requirements through MCP."""

from __future__ import annotations

import json
import os
import queue
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any


BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
PROJECT = os.environ.get("AITC_PROJECT", ".")
TOOL_NAME = "decide-entry"


def _default_uv() -> Path:
    if "AITC_UV" in os.environ:
        return Path(os.environ["AITC_UV"])
    if sys.platform.startswith("win"):
        return Path("./aitc/bin/uv.exe")
    if sys.platform == "darwin":
        return Path("./aitc/bin/uv.mac")
    return Path("./aitc/bin/uv.linux")


UV = _default_uv()


def _collect_lines(stream: Any, q: queue.Queue[str] | None = None) -> None:
    for line in stream:
        if q is not None:
            q.put(line)


def _stop(proc: subprocess.Popen[str]) -> None:
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()


def _launch() -> tuple[subprocess.Popen[str], int]:
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    if proc.stdout is None or proc.stderr is None:
        _stop(proc)
        raise RuntimeError("MCP server pipes were not created")

    stdout_q: queue.Queue[str] = queue.Queue()
    threading.Thread(target=_collect_lines, args=(proc.stdout, stdout_q), daemon=True).start()
    threading.Thread(target=_collect_lines, args=(proc.stderr,), daemon=True).start()

    deadline = time.time() + 30
    while time.time() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"MCP server exited before advertising MCP_PORT: {proc.returncode}")
        try:
            line = stdout_q.get(timeout=0.1).strip()
        except queue.Empty:
            continue
        if line.startswith("MCP_PORT="):
            return proc, int(line.split("=", 1)[1])

    _stop(proc)
    raise RuntimeError("MCP server did not advertise MCP_PORT")


class JsonRpc:
    def __init__(self, sock: socket.socket) -> None:
        self._file = sock.makefile("rwb", buffering=0)
        self._next_id = 1

    def call(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        rpc_id = self._next_id
        self._next_id += 1
        message: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
        if params is not None:
            message["params"] = params
        self._file.write((json.dumps(message, sort_keys=True) + "\n").encode("utf-8"))

        line = self._file.readline()
        if not line:
            raise RuntimeError(f"MCP server closed connection during {method}")
        response = json.loads(line.decode("utf-8"))
        if response.get("id") != rpc_id:
            raise RuntimeError(f"unexpected JSON-RPC id for {method}: {response!r}")
        return response


def _file(mod_time: int, byte_size: int) -> dict[str, Any]:
    return {"kind": "File", "mod_time": mod_time, "byte_size": byte_size}


def _directory() -> dict[str, Any]:
    return {"kind": "Directory"}


def _absent() -> dict[str, Any]:
    return {"kind": "Absent"}


def _history(
    mod_time: int,
    byte_size: int,
    last_seen: int | None,
    deleted_time: int | None,
) -> dict[str, Any]:
    return {
        "mod_time": mod_time,
        "byte_size": byte_size,
        "last_seen": last_seen,
        "deleted_time": deleted_time,
    }


def _arguments(
    roles: dict[str, str],
    observations: dict[str, dict[str, Any]],
    histories: dict[str, dict[str, Any]] | None = None,
    tolerance: int = 5,
) -> dict[str, Any]:
    return {
        "roles": roles,
        "observations": observations,
        "histories": histories or {},
        "tolerance": tolerance,
    }


def _decide(rpc: JsonRpc, arguments: dict[str, Any]) -> tuple[dict[str, Any] | None, str | None]:
    response = rpc.call("tools/call", {"name": TOOL_NAME, "arguments": arguments})
    if "error" in response:
        return None, json.dumps(response["error"], sort_keys=True)
    result = response.get("result")
    if not isinstance(result, dict):
        return None, f"non-object result: {result!r}"
    return result, None


def _has_decide_tool(rpc: JsonRpc) -> bool:
    response = rpc.call("tools/list")
    tools = (response.get("result") or {}).get("tools", [])
    return any(isinstance(tool, dict) and tool.get("name") == TOOL_NAME for tool in tools)


def _entry_kind(decision: dict[str, Any] | None) -> Any:
    return None if decision is None else decision.get("entry_kind")


def _record(failures: list[str], req_id: str, passed: bool, detail: str) -> None:
    status = "PASS" if passed else "FAIL"
    print(f"[{req_id}] {status} {detail}")
    if not passed:
        failures.append(f"{req_id}: {detail}")


def _check(
    rpc: JsonRpc,
    failures: list[str],
    req_id: str,
    detail: str,
    arguments: dict[str, Any],
    predicate: Any,
) -> None:
    try:
        decision, error = _decide(rpc, arguments)
        passed = error is None and bool(predicate(decision))
        failure_detail = error or f"{detail}; result={decision!r}"
    except Exception as exc:
        passed = False
        failure_detail = f"{detail}; call failed: {exc}"
    _record(failures, req_id, passed, failure_detail if not passed else detail)


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            sock.settimeout(10)
            rpc = JsonRpc(sock)
            failures: list[str] = []

            if not _has_decide_tool(rpc):
                for req_id in ("04.1", "04.2", "04.3", "04.4", "04.5", "04.6"):
                    _record(failures, req_id, False, "decide-entry MCP tool is missing")
            else:
                _check(
                    rpc,
                    failures,
                    "04.1",
                    "directory exists when at least one contributor observes Directory and none observes File",
                    _arguments(
                        roles={"dir": "contributing", "missing": "contributing"},
                        observations={"dir": _directory(), "missing": _absent()},
                    ),
                    lambda decision: _entry_kind(decision) == "Directory",
                )

                _check(
                    rpc,
                    failures,
                    "04.2",
                    "File wins when contributing observations include both File and Directory",
                    _arguments(
                        roles={"file": "contributing", "dir": "contributing"},
                        observations={"file": _file(1000, 10), "dir": _directory()},
                    ),
                    lambda decision: _entry_kind(decision) == "File",
                )

                _check(
                    rpc,
                    failures,
                    "04.3",
                    "File-vs-Directory conflict chooses among File observers with no-canon voting rules",
                    _arguments(
                        roles={
                            "larger_file": "contributing",
                            "newer_file": "contributing",
                            "dir": "contributing",
                        },
                        observations={
                            "larger_file": _file(2000, 90),
                            "newer_file": _file(2004, 10),
                            "dir": _directory(),
                        },
                        tolerance=5,
                    ),
                    lambda decision: decision is not None
                    and decision.get("entry_kind") == "File"
                    and decision.get("winning_source") == "larger_file"
                    and decision.get("winning_mod_time") == 2000
                    and decision.get("winning_byte_size") == 90,
                )

                _check(
                    rpc,
                    failures,
                    "04.4",
                    "all contributing Absent observations with tombstone histories produce entry_kind None",
                    _arguments(
                        roles={"deleted_a": "contributing", "deleted_b": "contributing"},
                        observations={"deleted_a": _absent(), "deleted_b": _absent()},
                        histories={
                            "deleted_a": _history(100, -1, 110, 120),
                            "deleted_b": _history(105, -1, 115, 125),
                        },
                    ),
                    lambda decision: _entry_kind(decision) == "None",
                )

                _check(
                    rpc,
                    failures,
                    "04.5",
                    "Absent contributor with no history does not block collective tombstone deletion",
                    _arguments(
                        roles={"deleted": "contributing", "no_history": "contributing"},
                        observations={"deleted": _absent(), "no_history": _absent()},
                        histories={"deleted": _history(100, -1, 110, 120)},
                    ),
                    lambda decision: _entry_kind(decision) == "None",
                )

                _check(
                    rpc,
                    failures,
                    "04.6",
                    "all contributing Absent observations with no history rows produce entry_kind None",
                    _arguments(
                        roles={"first": "contributing", "second": "contributing"},
                        observations={"first": _absent(), "second": _absent()},
                    ),
                    lambda decision: _entry_kind(decision) == "None",
                )

            if failures:
                print("\nFAILURES:")
                for failure in failures:
                    print(f"  - {failure}")
                return 1
            print("\nAll assertions passed.")
            return 0
    finally:
        _stop(proc)


if __name__ == "__main__":
    sys.exit(main())
