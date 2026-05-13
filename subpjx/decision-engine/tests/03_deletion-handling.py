#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise deletion-vote decision handling through the MCP wrapper."""

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
    if proc.stdout is None:
        proc.terminate()
        raise RuntimeError("MCP server stdout was not captured")

    deadline = time.time() + 30
    while time.time() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"MCP server exited before advertising MCP_PORT: {proc.returncode}")
        line = proc.stdout.readline()
        if not line:
            continue
        line = line.strip()
        if line.startswith("MCP_PORT="):
            port = int(line.split("=", 1)[1])
            threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
            if proc.stderr is not None:
                threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
            return proc, port

    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
    raise RuntimeError("MCP server did not advertise MCP_PORT")


class RpcClient:
    def __init__(self, sock: socket.socket) -> None:
        self.sock = sock
        self.next_id = 1
        self.buffer = b""

    def call(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        rpc_id = self.next_id
        self.next_id += 1
        message: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
        if params is not None:
            message["params"] = params
        self.sock.sendall((json.dumps(message, separators=(",", ":")) + "\n").encode("utf-8"))

        deadline = time.time() + 10
        while b"\n" not in self.buffer and time.time() < deadline:
            chunk = self.sock.recv(8192)
            if not chunk:
                break
            self.buffer += chunk
        line, sep, rest = self.buffer.partition(b"\n")
        self.buffer = rest
        if not sep:
            raise RuntimeError(f"timeout waiting for JSON-RPC response to {method}")
        return json.loads(line.decode("utf-8"))


def _file(mod_time: int, byte_size: int) -> dict[str, Any]:
    return {"kind": "File", "mod_time": mod_time, "byte_size": byte_size}


def _absent() -> dict[str, Any]:
    return {"kind": "Absent"}


def _directory() -> dict[str, Any]:
    return {"kind": "Directory"}


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


def _decide(rpc: RpcClient, args: dict[str, Any]) -> tuple[dict[str, Any] | None, str | None]:
    response = rpc.call("tools/call", {"name": TOOL_NAME, "arguments": args})
    if "error" in response:
        return None, json.dumps(response["error"], sort_keys=True)
    result = response.get("result")
    if not isinstance(result, dict):
        return None, f"non-object result: {result!r}"
    decision = result.get("decision", result)
    if not isinstance(decision, dict):
        return None, f"non-object decision: {decision!r}"
    return decision, None


def _entry_kind(decision: dict[str, Any] | None) -> Any:
    if decision is None:
        return None
    return decision.get("entry_kind")


def _action(decision: dict[str, Any] | None, participant: str) -> Any:
    if decision is None:
        return None
    actions = decision.get("actions")
    if not isinstance(actions, dict):
        return None
    return actions.get(participant)


def _action_kind(action: Any) -> Any:
    if isinstance(action, str):
        return action
    if isinstance(action, dict):
        return action.get("kind", action.get("type", action.get("action")))
    return None


def _action_source(action: Any) -> Any:
    if isinstance(action, dict):
        return action.get("source")
    return None


def _is_receive_from(decision: dict[str, Any] | None, participant: str, source: str) -> bool:
    action = _action(decision, participant)
    return _action_kind(action) == "ReceiveFile" and _action_source(action) == source


def _has_tool(rpc: RpcClient) -> bool:
    response = rpc.call("tools/list")
    tools = (response.get("result") or {}).get("tools", [])
    return any(isinstance(tool, dict) and tool.get("name") == TOOL_NAME for tool in tools)


def _case_0316(rpc: RpcClient) -> tuple[bool, str]:
    decision, error = _decide(
        rpc,
        {
            "roles": {
                "first_deleter": "contributing",
                "live": "contributing",
                "second_deleter": "contributing",
                "holder": "subordinate",
            },
            "observations": {
                "first_deleter": _absent(),
                "live": _file(100, 10),
                "second_deleter": _absent(),
                "holder": _file(98, 8),
            },
            "histories": {
                "first_deleter": _history(90, 10, 90, 105),
                "second_deleter": _history(90, 10, 90, 106),
            },
            "tolerance": 5,
        },
    )
    passed = (
        error is None
        and _entry_kind(decision) == "None"
        and _action_kind(_action(decision, "live")) == "Displace"
        and _action_kind(_action(decision, "holder")) == "Displace"
    )
    detail = error or "maximum deleted_time 106 beats live mod_time 100 by more than tolerance"
    return passed, detail


def _case_0317(rpc: RpcClient) -> tuple[bool, str]:
    decision, error = _decide(
        rpc,
        {
            "roles": {
                "deleter": "contributing",
                "older": "contributing",
                "newest": "contributing",
            },
            "observations": {
                "deleter": _absent(),
                "older": _file(90, 99),
                "newest": _file(103, 22),
            },
            "histories": {
                "deleter": _history(90, 10, 90, 108),
            },
            "tolerance": 5,
        },
    )
    passed = (
        error is None
        and _entry_kind(decision) == "File"
        and decision is not None
        and decision.get("winning_source") == "newest"
        and decision.get("winning_mod_time") == 103
        and decision.get("winning_byte_size") == 22
    )
    detail = error or "deleted_time 108 is exactly tolerance seconds after live mod_time 103, so newest live file survives"
    return passed, detail


def _case_0318(rpc: RpcClient) -> tuple[bool, str]:
    decision, error = _decide(
        rpc,
        {
            "roles": {
                "absent": "contributing",
                "live": "contributing",
                "holder": "subordinate",
            },
            "observations": {
                "absent": _absent(),
                "live": _file(100, 10),
                "holder": _file(99, 10),
            },
            "histories": {
                "absent": _history(90, 10, 106, None),
            },
            "tolerance": 5,
        },
    )
    passed = (
        error is None
        and _entry_kind(decision) == "None"
        and _action_kind(_action(decision, "live")) == "Displace"
        and _action_kind(_action(decision, "holder")) == "Displace"
    )
    detail = error or "AbsentUnconfirmed last_seen 106 beats live mod_time 100 by more than tolerance"
    return passed, detail


def _case_0319(rpc: RpcClient) -> tuple[bool, str]:
    file_decision, file_error = _decide(
        rpc,
        {
            "roles": {
                "never-confirmed": "contributing",
                "recent-absent": "contributing",
                "live": "contributing",
            },
            "observations": {
                "never-confirmed": _absent(),
                "recent-absent": _absent(),
                "live": _file(100, 10),
            },
            "histories": {
                "never-confirmed": _history(90, 10, None, None),
                "recent-absent": _history(90, 10, 105, None),
            },
            "tolerance": 5,
        },
    )
    dir_decision, dir_error = _decide(
        rpc,
        {
            "roles": {
                "missing-dir": "contributing",
                "live-dir": "contributing",
            },
            "observations": {
                "missing-dir": _absent(),
                "live-dir": _directory(),
            },
            "histories": {
                "missing-dir": _history(90, -1, 105, None),
                "live-dir": _history(100, -1, 100, None),
            },
            "tolerance": 5,
        },
    )
    passed = (
        file_error is None
        and dir_error is None
        and _entry_kind(file_decision) == "File"
        and file_decision is not None
        and file_decision.get("winning_source") == "live"
        and _is_receive_from(file_decision, "never-confirmed", "live")
        and _is_receive_from(file_decision, "recent-absent", "live")
        and _entry_kind(dir_decision) == "Directory"
        and _action_kind(_action(dir_decision, "missing-dir")) == "CreateDirectory"
    )
    detail = file_error or dir_error or "AbsentUnconfirmed participants with null or boundary last_seen receive the surviving file or directory"
    return passed, detail


def _case_0320(rpc: RpcClient) -> tuple[bool, str]:
    decision, error = _decide(
        rpc,
        {
            "roles": {
                "deleted": "contributing",
                "uninvolved": "contributing",
                "holder": "subordinate",
            },
            "observations": {
                "deleted": _absent(),
                "uninvolved": _absent(),
                "holder": _directory(),
            },
            "histories": {
                "deleted": _history(90, -1, 90, 120),
            },
            "tolerance": 5,
        },
    )
    passed = error is None and _entry_kind(decision) == "None"
    detail = error or "no contributing participant has a live observation"
    return passed, detail


def _record(failures: list[str], req_id: str, passed: bool, detail: str) -> None:
    status = "PASS" if passed else "FAIL"
    print(f"[{req_id}] {status}: {detail}")
    if not passed:
        failures.append(f"{req_id}: {detail}")


def _run_all(rpc: RpcClient) -> int:
    failures: list[str] = []

    if not _has_tool(rpc):
        for req_id in ("03.16", "03.17", "03.18", "03.19", "03.20"):
            _record(failures, req_id, False, f"MCP tool {TOOL_NAME!r} is not listed")
    else:
        for req_id, case in (
            ("03.16", _case_0316),
            ("03.17", _case_0317),
            ("03.18", _case_0318),
            ("03.19", _case_0319),
            ("03.20", _case_0320),
        ):
            try:
                passed, detail = case(rpc)
            except Exception as exc:
                passed, detail = False, f"exception while exercising requirement: {exc}"
            _record(failures, req_id, passed, detail)

    if failures:
        print("\nFAILURES:")
        for failure in failures:
            print(f"  - {failure}")
        return 1
    print("\nAll assertions passed.")
    return 0


def main() -> int:
    proc: subprocess.Popen[str] | None = None
    try:
        try:
            proc, port = _launch()
        except Exception as exc:
            failures = []
            for req_id in ("03.16", "03.17", "03.18", "03.19", "03.20"):
                _record(failures, req_id, False, f"MCP launch failed: {exc}")
            print("\nFAILURES:")
            for failure in failures:
                print(f"  - {failure}")
            return 1

        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            return _run_all(RpcClient(sock))
    finally:
        if proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()


if __name__ == "__main__":
    sys.exit(main())
