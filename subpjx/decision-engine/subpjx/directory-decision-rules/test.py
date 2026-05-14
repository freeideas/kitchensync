#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise directory decision API via MCP."""

from __future__ import annotations

import json
import os
import re
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")


def _drain(stream) -> None:
    for _ in stream:
        pass


def _launch_mcp() -> tuple[subprocess.Popen[str], int]:
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )

    port: Optional[int] = None
    deadline = time.time() + 30.0
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline() if proc.stdout is not None else ""
        if not line:
            continue
        line = line.strip()
        if line.startswith("MCP_PORT="):
            try:
                port = int(line.split("=", 1)[1])
            except ValueError:
                continue
            break

    if port is None:
        proc.terminate()
        raise RuntimeError("did not receive MCP_PORT from wrapper")

    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
    return proc, port


def _read_json_message(sock: socket.socket, timeout: float = 10.0) -> Dict[str, Any]:
    buffer = b""
    deadline = time.time() + timeout
    while time.time() < deadline:
        sock.settimeout(1.0)
        try:
            chunk = sock.recv(65536)
        except TimeoutError:
            continue
        if not chunk:
            break
        buffer += chunk
        if b"\n" not in buffer:
            continue
        line, _, _ = buffer.partition(b"\n")
        line = line.strip()
        if not line:
            continue
        return json.loads(line.decode("utf-8"))
    raise TimeoutError("timed out waiting for MCP response")


def _rpc(sock: socket.socket, method: str, params: Optional[Dict[str, Any]], request_id: int) -> Dict[str, Any]:
    payload: Dict[str, Any] = {"jsonrpc": "2.0", "id": request_id, "method": method}
    if params is not None:
        payload["params"] = params
    sock.sendall((json.dumps(payload) + "\n").encode("utf-8"))
    return _read_json_message(sock)


def _call_tool(sock: socket.socket, tool_name: str, arguments: Dict[str, Any], request_id: int) -> Dict[str, Any]:
    return _rpc(
        sock,
        "tools/call",
        {"name": tool_name, "arguments": arguments},
        request_id,
    )


def _normalize_name(value: str) -> str:
    return re.sub(r"[^a-z0-9]", "", str(value).lower())


def _find_decide_directory_tool(tools: List[Dict[str, Any]]) -> Optional[Dict[str, Any]]:
    if not tools:
        return None

    candidates = {
        "decidedirectory",
        "decidedirectoryrules",
        "decidedirectoryrule",
        "decidedirectorystate",
        "decidedirectorydecision",
    }

    for tool in tools:
        name = str(tool.get("name", ""))
        if _normalize_name(name) in candidates:
            return tool

    for tool in tools:
        name = str(tool.get("name", "")).lower()
        description = str(tool.get("description", "")).lower()
        haystack = f"{name} {description}".lower()
        if "decide" in haystack and "directory" in haystack:
            return tool

    if len(tools) == 1:
        return tools[0]

    return None


def _build_argument_name(tool: Dict[str, Any]) -> str:
    schema = tool.get("inputSchema")
    if not isinstance(schema, dict):
        return "peer_states"

    props = schema.get("properties")
    if isinstance(props, dict):
        for candidate in ("peer_states", "peerStates", "peerstates"):
            if candidate in props:
                return str(candidate)

    required = schema.get("required")
    if isinstance(required, list) and len(required) == 1:
        return str(required[0])

    if isinstance(props, dict):
        for key in props.keys():
            key_l = str(key).lower()
            if "peer" in key_l and "state" in key_l:
                return str(key)

    return "peer_states"


def _contains_token(payload: Any, token: str) -> bool:
    if payload is None:
        return False
    if isinstance(payload, str):
        return token in payload.lower()
    if isinstance(payload, dict):
        if token in str(payload.get("message", "")).lower():
            return True
        if token in str(payload.get("code", "")).lower():
            return True
        for value in payload.values():
            if _contains_token(value, token):
                return True
        return False
    if isinstance(payload, list):
        return any(_contains_token(item, token) for item in payload)
    return token in str(payload).lower()


def _string_set(value: Any) -> set[str]:
    if not isinstance(value, list):
        return set()
    return {str(item) for item in value if isinstance(item, str)}


def _peer_in_any(decision: Dict[str, Any], peer_id: str, keys: List[str]) -> bool:
    for key in keys:
        if peer_id in _string_set(decision.get(key)):
            return True
    return False


def _extract_decision(response: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    if not isinstance(response, dict):
        return None
    result = response.get("result")
    if result is None:
        return None
    if isinstance(result, dict) and "decision" in result and isinstance(result["decision"], dict):
        return result["decision"]
    if isinstance(result, dict):
        return result
    return None


def _peer_state(
    peer_id: str,
    role: str,
    live_entry: Optional[Dict[str, Any]] = None,
    snapshot_row: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    item: Dict[str, Any] = {"peer_id": peer_id, "role": role}
    if live_entry is not None:
        item["live_entry"] = live_entry
    if snapshot_row is not None:
        item["snapshot_row"] = snapshot_row
    return item


def _live_directory(mod_time: str, byte_size: int = 0) -> Dict[str, Any]:
    return {
        "entry_type": "directory",
        "mod_time": mod_time,
        "byte_size": byte_size,
    }


def _snapshot_row(
    mod_time: str,
    last_seen: Optional[str] = None,
    deleted_time: Optional[str] = None,
    byte_size: int = 0,
) -> Dict[str, Any]:
    return {
        "mod_time": mod_time,
        "byte_size": byte_size,
        "last_seen": last_seen,
        "deleted_time": deleted_time,
    }


def _assert_success(failures: List[str], case_id: str, response: Dict[str, Any], case_label: str) -> Optional[Dict[str, Any]]:
    if not isinstance(response, dict):
        failures.append(f"{case_id}: {case_label} returned non-dict response {response}")
        return None
    if response.get("error") is not None:
        failures.append(f"{case_id}: {case_label} returned error {response.get('error')}")
        return None
    decision = _extract_decision(response)
    if not isinstance(decision, dict):
        failures.append(f"{case_id}: {case_label} returned invalid decision payload {response.get('result')}")
        return None
    return decision


def _assert_error(failures: List[str], case_id: str, response: Dict[str, Any], expected_token: str, context: str) -> None:
    if not isinstance(response, dict):
        failures.append(f"{case_id}: {context} returned non-dict response {response}")
        return
    error = response.get("error")
    if error is None:
        failures.append(f"{case_id}: {context} expected error '{expected_token}', got success {response.get('result')}")
        return
    if not _contains_token(error, expected_token):
        failures.append(f"{case_id}: {context} expected error '{expected_token}', got {error}")


def main() -> int:
    failures: List[str] = []
    proc: Optional[subprocess.Popen[str]] = None

    try:
        proc, port = _launch_mcp()
        with socket.create_connection(("127.0.0.1", port), timeout=10.0) as sock:
            request_id = 1

            tools_response = _rpc(sock, "tools/list", None, request_id)
            request_id += 1
            tools = tools_response.get("result", {}).get("tools")
            if not isinstance(tools, list):
                failures.append("01: tools/list did not return a tools list")
                tools = []

            tool = _find_decide_directory_tool(tools)
            if tool is None:
                failures.append("02: decide_directory tool not found in tools/list")
                return 1

            arg_name = _build_argument_name(tool)

            def next_request_id() -> int:
                nonlocal request_id
                current = request_id
                request_id += 1
                return current

            def call_tool(peer_states: List[Dict[str, Any]]) -> Dict[str, Any]:
                return _call_tool(
                    sock,
                    str(tool["name"]),
                    {arg_name: peer_states},
                    next_request_id(),
                )

            # 01: any contributing live directory means directory exists for all peers.
            response = call_tool([
                _peer_state("canon-a", "canon", live_entry=_live_directory("2020-01-01T00:00:00Z")),
                _peer_state("peer-b", "bidirectional"),
            ])
            decision = _assert_success(failures, "01", response, "directory present case")
            if decision is not None:
                entry_type = str(decision.get("entry_type", "")).lower()
                if entry_type != "directory":
                    failures.append(f"01: expected entry_type 'directory', got {decision.get('entry_type')!r}")
                if not _peer_in_any(decision, "peer-b", ["create_directory_peer_ids", "target_peer_ids"]):
                    failures.append("01: peer-b was not marked as a peer requiring directory creation")

            # 02: live directory mod_time is irrelevant; a live directory beats deleted snapshots.
            response = call_tool([
                _peer_state("canon-a", "canon", live_entry=_live_directory("1970-01-01T00:00:00Z")),
                _peer_state(
                    "peer-b",
                    "bidirectional",
                    snapshot_row=_snapshot_row(
                        mod_time="2026-01-01T00:00:00Z",
                        last_seen="2026-01-01T00:00:05Z",
                        deleted_time="2026-01-01T00:00:06Z",
                    ),
                ),
            ])
            decision = _assert_success(failures, "02", response, "directory vs deleted snapshot case")
            if decision is not None:
                entry_type = str(decision.get("entry_type", "")).lower()
                if entry_type != "directory":
                    failures.append(f"02: expected live directory to win over deleted snapshot, got {decision.get('entry_type')!r}")
                if _peer_in_any(decision, "peer-b", ["delete_peer_ids", "displace_peer_ids"]):
                    failures.append("02: peer-b should not be deleted/displaced while live directory exists")

            # 03: deletion wins when every contributing peer has deleted snapshot rows and no live directory.
            response = call_tool([
                _peer_state(
                    "peer-a",
                    "bidirectional",
                    snapshot_row=_snapshot_row(
                        mod_time="2026-01-01T00:00:01Z",
                        last_seen="2026-01-01T00:00:02Z",
                        deleted_time="2026-01-01T00:00:03Z",
                    ),
                ),
                _peer_state(
                    "peer-b",
                    "bidirectional",
                    snapshot_row=_snapshot_row(
                        mod_time="2026-01-01T00:00:04Z",
                        last_seen="2026-01-01T00:00:05Z",
                        deleted_time="2026-01-01T00:00:06Z",
                    ),
                ),
            ])
            decision = _assert_success(failures, "03", response, "all deleted snapshot rows")
            if decision is not None:
                reason = str(decision.get("reason", "")).lower()
                if "delete" not in reason:
                    failures.append(f"03: expected deletion reason, got {decision.get('reason')!r}")
                if str(decision.get("entry_type", "")).lower() == "directory":
                    failures.append("03: directory should not exist when all contributing peers report deleted")

            # 04: contributor with no snapshot row does not block deletion.
            response = call_tool([
                _peer_state(
                    "peer-a",
                    "bidirectional",
                    snapshot_row=_snapshot_row(
                        mod_time="2026-01-01T00:00:01Z",
                        last_seen="2026-01-01T00:00:02Z",
                        deleted_time="2026-01-01T00:00:03Z",
                    ),
                ),
                _peer_state("peer-b", "bidirectional"),
            ])
            decision = _assert_success(failures, "04", response, "no-snapshot contributor")
            if decision is not None:
                reason = str(decision.get("reason", "")).lower()
                if "delete" not in reason:
                    failures.append(f"04: expected deletion reason, got {decision.get('reason')!r}")
                if _peer_in_any(decision, "peer-b", ["create_directory_peer_ids", "target_peer_ids"]):
                    failures.append("04: peer-b with no snapshot should not be marked for creation when deletion wins")

            # 05: no contributing live or snapshot rows means subordinate peers with live entries are displaced.
            response = call_tool([
                _peer_state("sub-a", "subordinate", live_entry=_live_directory("2026-01-01T00:00:00Z")),
                _peer_state("peer-b", "bidirectional"),
            ])
            decision = _assert_success(failures, "05", response, "subordinate-only-live case")
            if decision is not None:
                if str(decision.get("entry_type", "")).lower() == "directory":
                    failures.append("05: directory should not be retained when only subordinate has it")
                if not _peer_in_any(decision, "sub-a", ["displace_peer_ids"]):
                    failures.append("05: subordinate peer with directory and no contributing peers was not displaced")

            # 06: directory decision rejects file live entries.
            response = call_tool([
                _peer_state(
                    "bad-entry-type",
                    "canon",
                    live_entry={"entry_type": "file", "mod_time": "2026-01-01T00:00:00Z", "byte_size": 11},
                ),
            ])
            _assert_error(failures, "06", response, "invalid_entry_type", "file-live state")

            # 07: invalid role/shape returns invalid_peer_state.
            response = call_tool([
                _peer_state(
                    "bad-role",
                    "not-a-role",
                    snapshot_row=_snapshot_row("2026-01-01T00:00:00Z"),
                ),
            ])
            _assert_error(failures, "07", response, "invalid_peer_state", "bad role state")

            # 08: malformed timestamps return invalid_timestamp.
            response = call_tool([
                _peer_state(
                    "bad-time",
                    "canon",
                    snapshot_row={
                        "mod_time": "not-a-timestamp",
                        "byte_size": 0,
                        "last_seen": "also-bad",
                        "deleted_time": None,
                    },
                ),
            ])
            _assert_error(failures, "08", response, "invalid_timestamp", "bad timestamps")

    except Exception as exc:
        failures.append(f"00: unexpected failure during test execution: {exc}")
    finally:
        if proc is not None:
            if proc.poll() is None:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()

    if failures:
        print("FAILURES:")
        for failure in failures:
            print(f"- {failure}")
        return 1

    print("All checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())