#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise subordinate decision behavior through the real MCP wrapper."""

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


def _default_uv() -> Path:
    if sys.platform.startswith("win"):
        return Path("./aitc/bin/uv.exe")
    if sys.platform == "darwin":
        return Path("./aitc/bin/uv.mac")
    return Path("./aitc/bin/uv.linux")


BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", str(_default_uv())))
PROJECT = os.environ.get("AITC_PROJECT", ".")


def _drain(stream: Any) -> None:
    try:
        for _ in stream:
            pass
    except Exception:
        pass


def _enqueue_lines(stream: Any, lines: "queue.SimpleQueue[str]") -> None:
    try:
        for line in stream:
            lines.put(line)
    except Exception:
        pass


def _launch_mcp() -> tuple[subprocess.Popen[str], int]:
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        bufsize=1,
    )
    stdout_lines: queue.SimpleQueue[str] = queue.SimpleQueue()
    if proc.stdout is not None:
        threading.Thread(target=_enqueue_lines, args=(proc.stdout, stdout_lines), daemon=True).start()
    if proc.stderr is not None:
        threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()

    port: int | None = None
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            break
        try:
            line = stdout_lines.get(timeout=0.1)
        except queue.Empty:
            time.sleep(0.05)
            continue
        line = line.strip()
        if line.startswith("MCP_PORT="):
            port = int(line.split("=", 1)[1])
            break

    if port is None:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        raise RuntimeError("MCP server did not advertise MCP_PORT")

    return proc, port


class RpcClient:
    def __init__(self, port: int) -> None:
        self._sock = socket.create_connection(("127.0.0.1", port), timeout=10)
        self._sock.settimeout(10)
        self._next_id = 1

    def close(self) -> None:
        self._sock.close()

    def call(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        rpc_id = self._next_id
        self._next_id += 1
        request: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
        if params is not None:
            request["params"] = params
        payload = json.dumps(request, separators=(",", ":"), ensure_ascii=False)
        self._sock.sendall((payload + "\n").encode("utf-8"))

        data = bytearray()
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            chunk = self._sock.recv(8192)
            if not chunk:
                break
            data.extend(chunk)
            if b"\n" in data:
                break
        line, _, _ = bytes(data).partition(b"\n")
        if not line:
            raise RuntimeError(f"no JSON-RPC response for {method}")
        return json.loads(line.decode("utf-8"))


def _pick_decide_tool(tools: list[dict[str, Any]]) -> str | None:
    names = [str(tool.get("name", "")) for tool in tools]
    if "decide-entry" in names:
        return "decide-entry"
    for name in names:
        if "decide" in name and "entry" in name:
            return name
    return None


def _tools(client: RpcClient) -> tuple[list[dict[str, Any]], str | None]:
    response = client.call("tools/list")
    if "error" in response:
        return [], response["error"].get("message", "tools/list failed")
    result = response.get("result")
    if not isinstance(result, dict):
        return [], "tools/list result was not an object"
    tools = result.get("tools")
    if not isinstance(tools, list):
        return [], "tools/list result did not contain tools"
    return [tool for tool in tools if isinstance(tool, dict)], None


def _decide(client: RpcClient, tool: str, arguments: dict[str, Any]) -> tuple[dict[str, Any] | None, str | None]:
    response = client.call("tools/call", {"name": tool, "arguments": arguments})
    if "error" in response:
        return None, response["error"].get("message", "tools/call failed")
    result = response.get("result")
    if isinstance(result, dict):
        return result, None
    return None, "tools/call result was not an object"


def _get(mapping: dict[str, Any], *names: str) -> Any:
    for name in names:
        if name in mapping:
            return mapping[name]
    return None


def _action(decision: dict[str, Any], participant: str) -> tuple[str | None, str | None]:
    actions = _get(decision, "actions", "participant_actions", "participantActions")
    if not isinstance(actions, dict):
        return None, None
    raw = actions.get(participant)
    if isinstance(raw, str):
        return raw, None
    if isinstance(raw, dict):
        kind = _get(raw, "kind", "type", "action")
        source = _get(raw, "source", "winning_source", "winningSource")
        return str(kind) if kind is not None else None, str(source) if source is not None else None
    return None, None


def _classification(decision: dict[str, Any], participant: str) -> str | None:
    classifications = _get(decision, "classifications", "participant_classifications", "participantClassifications")
    if not isinstance(classifications, dict):
        return None
    raw = classifications.get(participant)
    return str(raw) if raw is not None else None


def _metadata(decision: dict[str, Any]) -> tuple[Any, Any, Any, Any]:
    return (
        _get(decision, "entry_kind", "entryKind"),
        _get(decision, "winning_mod_time", "winningModTime"),
        _get(decision, "winning_byte_size", "winningByteSize"),
        _get(decision, "winning_source", "winningSource"),
    )


def _base_arguments(include_subordinate: bool) -> dict[str, Any]:
    roles = {"c_live": "contributing", "c_absent": "contributing"}
    observations: dict[str, Any] = {
        "c_live": {"kind": "File", "mod_time": 100, "byte_size": 10},
        "c_absent": {"kind": "Absent"},
    }
    if include_subordinate:
        roles["s_noise"] = "subordinate"
        roles["s_missing"] = "subordinate"
        observations["s_noise"] = {"kind": "File", "mod_time": 10000, "byte_size": 999}
        observations["s_missing"] = {"kind": "Absent"}
    return {
        "roles": roles,
        "observations": observations,
        "histories": {},
        "tolerance": 5,
    }


def _deleted_arguments() -> dict[str, Any]:
    return {
        "roles": {"c_absent": "contributing", "s_holder": "subordinate"},
        "observations": {
            "c_absent": {"kind": "Absent"},
            "s_holder": {"kind": "File", "mod_time": 200, "byte_size": 20},
        },
        "histories": {},
        "tolerance": 5,
    }


def _check_04_7(client: RpcClient, tool: str) -> str | None:
    baseline, baseline_error = _decide(client, tool, _base_arguments(False))
    with_subordinate, subordinate_error = _decide(client, tool, _base_arguments(True))
    if baseline_error or subordinate_error or baseline is None or with_subordinate is None:
        return f"04.7: decide-entry failed: {baseline_error or subordinate_error}"

    expected_metadata = ("File", 100, 10, "c_live")
    baseline_metadata = _metadata(baseline)
    subordinate_metadata = _metadata(with_subordinate)
    if baseline_metadata != expected_metadata:
        return f"04.7: baseline metadata was {baseline_metadata}, expected {expected_metadata}"
    if subordinate_metadata != expected_metadata:
        return f"04.7: subordinate influenced metadata: {subordinate_metadata}, expected {expected_metadata}"
    for participant in ("c_live", "c_absent"):
        if _action(baseline, participant) != _action(with_subordinate, participant):
            return (
                f"04.7: action for {participant} changed from "
                f"{_action(baseline, participant)} to {_action(with_subordinate, participant)}"
            )
        if _classification(baseline, participant) != _classification(with_subordinate, participant):
            return (
                f"04.7: classification for {participant} changed from "
                f"{_classification(baseline, participant)} to {_classification(with_subordinate, participant)}"
            )
    if _action(with_subordinate, "c_live")[0] != "NoOp":
        return f"04.7: winner action was {_action(with_subordinate, 'c_live')}, expected NoOp"
    if _action(with_subordinate, "c_absent") != ("ReceiveFile", "c_live"):
        return f"04.7: absent contributor action was {_action(with_subordinate, 'c_absent')}, expected ReceiveFile from c_live"
    if _action(with_subordinate, "s_noise")[0] != "Displace":
        return f"04.7: nonmatching subordinate action was {_action(with_subordinate, 's_noise')}, expected Displace"
    if _action(with_subordinate, "s_missing") != ("ReceiveFile", "c_live"):
        return f"04.7: absent subordinate action was {_action(with_subordinate, 's_missing')}, expected ReceiveFile from c_live"
    return None


def _check_04_8(client: RpcClient, tool: str) -> str | None:
    decision, error = _decide(client, tool, _deleted_arguments())
    if error or decision is None:
        return f"04.8: decide-entry failed: {error}"
    entry_kind = _get(decision, "entry_kind", "entryKind")
    if entry_kind not in ("None", None):
        return f"04.8: entry_kind was {entry_kind!r}, expected 'None'"
    if _action(decision, "s_holder")[0] != "Displace":
        return f"04.8: subordinate holder action was {_action(decision, 's_holder')}, expected Displace"
    return None


def _run_assertion(req_id: str, failures: list[str], check: Any) -> None:
    try:
        failure = check()
    except Exception as exc:
        failure = f"{req_id}: unexpected exception: {exc}"
    if failure is None:
        print(f"[{req_id}] PASS")
    else:
        print(f"[{req_id}] FAIL - {failure}")
        failures.append(failure)


def main() -> int:
    proc: subprocess.Popen[str] | None = None
    client: RpcClient | None = None
    failures: list[str] = []
    try:
        try:
            proc, port = _launch_mcp()
            client = RpcClient(port)
            tools, tools_error = _tools(client)
            tool = _pick_decide_tool(tools)
            if tools_error:
                print(f"MCP surface unavailable: {tools_error}")
            elif tool is None:
                print(f"MCP tools available: {[tool.get('name') for tool in tools]}")
            else:
                print(f"MCP tool selected: {tool}")
        except Exception as exc:
            client = None
            tool = None
            print(f"MCP launch/connect failed: {exc}")

        if client is None or tool is None:
            _run_assertion("04.7", failures, lambda: "04.7: decide-entry MCP tool unavailable")
            _run_assertion("04.8", failures, lambda: "04.8: decide-entry MCP tool unavailable")
        else:
            _run_assertion("04.7", failures, lambda: _check_04_7(client, tool))
            _run_assertion("04.8", failures, lambda: _check_04_8(client, tool))

        if failures:
            print("\nFAILURES:")
            for failure in failures:
                print(f"  - {failure}")
            return 1
        print("\nAll assertions passed.")
        return 0
    finally:
        if client is not None:
            client.close()
        if proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()


if __name__ == "__main__":
    sys.exit(main())
