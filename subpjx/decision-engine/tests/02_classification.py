#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise decision-engine classification requirements through the MCP wrapper."""

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

CLASSIFICATIONS = {
    "Unchanged",
    "Modified",
    "Resurrected",
    "New",
    "Deleted",
    "AbsentUnconfirmed",
    "NoOpinion",
}

REQ_DESCRIPTIONS = {
    "02.1": "File matching non-tombstone history is Unchanged",
    "02.2": "File differing from non-tombstone history is Modified",
    "02.3": "File and Directory over tombstone history are Resurrected",
    "02.4": "File and Directory with no history are New",
    "02.5": "Absent with tombstone history is Deleted",
    "02.6": "Absent with non-tombstone history is AbsentUnconfirmed",
    "02.7": "Absent with no history is NoOpinion",
    "02.8": "default tolerance treats +5s mod_time and last_seen deltas as equal",
}


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
        raise RuntimeError("MCP server stdout was not piped")

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
        try:
            _, stderr = proc.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            _, stderr = proc.communicate()
        raise RuntimeError(f"MCP server did not advertise MCP_PORT: {stderr.strip()}")

    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    if proc.stderr is not None:
        threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
    return proc, port


def _rpc(sock: socket.socket, method: str, params: dict[str, Any] | None = None, rpc_id: int = 1) -> dict[str, Any]:
    msg: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg, separators=(",", ":")) + "\n").encode("utf-8"))

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
    if not line:
        raise RuntimeError(f"no JSON-RPC response for {method}")
    return json.loads(line.decode("utf-8"))


class McpClient:
    def __init__(self, sock: socket.socket, tool: str) -> None:
        self.sock = sock
        self.tool = tool
        self.next_id = 10

    def call(self, arguments: dict[str, Any]) -> dict[str, Any]:
        self.next_id += 1
        response = _rpc(
            self.sock,
            "tools/call",
            {"name": self.tool, "arguments": arguments},
            self.next_id,
        )
        if "error" in response:
            raise RuntimeError(json.dumps(response["error"], sort_keys=True))
        result = response.get("result")
        if not isinstance(result, dict):
            raise RuntimeError(f"tools/call returned non-object result: {result!r}")
        return result


def _find_decide_tool(tools: list[dict[str, Any]]) -> str | None:
    names = [tool.get("name") for tool in tools if isinstance(tool.get("name"), str)]
    for wanted in ("decide-entry", "decide_entry"):
        if wanted in names:
            return wanted
    for name in names:
        if "decide" in name and "entry" in name:
            return name
    return None


def _file(mod_time: int, byte_size: int = 10) -> dict[str, Any]:
    return {"kind": "File", "mod_time": mod_time, "byte_size": byte_size}


def _directory() -> dict[str, Any]:
    return {"kind": "Directory"}


def _absent() -> dict[str, Any]:
    return {"kind": "Absent"}


def _history(
    mod_time: int,
    byte_size: int = 10,
    last_seen: int | None = None,
    deleted_time: int | None = None,
) -> dict[str, Any]:
    return {
        "mod_time": mod_time,
        "byte_size": byte_size,
        "last_seen": mod_time if last_seen is None else last_seen,
        "deleted_time": deleted_time,
    }


def _arguments(
    observations: dict[str, dict[str, Any]],
    histories: dict[str, dict[str, Any]],
    *,
    tolerance: int | None = 5,
) -> dict[str, Any]:
    arguments: dict[str, Any] = {
        "roles": {participant: "contributing" for participant in observations},
        "observations": observations,
        "histories": histories,
    }
    if tolerance is not None:
        arguments["tolerance"] = tolerance
    return arguments


def _normalize_classification(value: Any) -> str | None:
    if isinstance(value, str):
        return value if value in CLASSIFICATIONS else None
    if isinstance(value, dict):
        for key in ("classification", "kind", "type", "value"):
            found = _normalize_classification(value.get(key))
            if found is not None:
                return found
    return None


def _classification_map(result: dict[str, Any]) -> dict[str, str]:
    for key in ("classifications", "classification", "participant_classifications", "participantClassifications"):
        value = result.get(key)
        if isinstance(value, dict):
            mapped = {
                str(participant): classification
                for participant, raw in value.items()
                if (classification := _normalize_classification(raw)) is not None
            }
            if mapped:
                return mapped

    stack: list[Any] = [result]
    while stack:
        item = stack.pop()
        if isinstance(item, dict):
            mapped = {
                str(participant): classification
                for participant, raw in item.items()
                if (classification := _normalize_classification(raw)) is not None
            }
            if mapped:
                return mapped
            stack.extend(item.values())
        elif isinstance(item, list):
            stack.extend(item)
    return {}


def _entry_kind(result: dict[str, Any]) -> str | None:
    for key in ("entry_kind", "entryKind"):
        value = result.get(key)
        if value is None:
            return "None" if key in result else None
        if isinstance(value, str):
            return value
        if isinstance(value, dict):
            for inner in ("kind", "type", "value"):
                inner_value = value.get(inner)
                if isinstance(inner_value, str):
                    return inner_value
    return None


def _action_name(value: Any) -> str | None:
    if isinstance(value, str):
        return value
    if isinstance(value, dict):
        for key in ("action", "kind", "type", "value"):
            raw = value.get(key)
            if isinstance(raw, str):
                return raw
    return None


def _actions(result: dict[str, Any]) -> dict[str, str]:
    for key in ("actions", "participant_actions", "participantActions"):
        value = result.get(key)
        if isinstance(value, dict):
            return {
                str(participant): action
                for participant, raw in value.items()
                if (action := _action_name(raw)) is not None
            }
    return {}


def _expect_classifications(
    client: McpClient,
    req_id: str,
    description: str,
    arguments: dict[str, Any],
    expected: dict[str, str],
    failures: list[str],
) -> dict[str, Any] | None:
    try:
        result = client.call(arguments)
        actual = _classification_map(result)
        ok = all(actual.get(participant) == classification for participant, classification in expected.items())
        print(f"[{req_id}] {description}: {'PASS' if ok else 'FAIL'}")
        if not ok:
            failures.append(f"{req_id}: expected classifications {expected}, got {actual}")
        return result
    except Exception as exc:
        print(f"[{req_id}] {description}: FAIL")
        failures.append(f"{req_id}: {exc}")
        return None


def _expect_02_8(client: McpClient, failures: list[str]) -> None:
    description = "default tolerance treats +5s mod_time and last_seen deltas as equal"
    try:
        mod_result = client.call(
            _arguments(
                {"mod_default": _file(105)},
                {"mod_default": _history(100)},
                tolerance=None,
            )
        )
        mod_classes = _classification_map(mod_result)
        mod_ok = mod_classes.get("mod_default") == "Unchanged"

        last_seen_result = client.call(
            _arguments(
                {"live": _file(100), "missing": _absent()},
                {
                    "live": _history(100),
                    "missing": _history(90, last_seen=105, deleted_time=None),
                },
                tolerance=None,
            )
        )
        last_seen_actions = _actions(last_seen_result)
        last_seen_ok = (
            _entry_kind(last_seen_result) == "File"
            and last_seen_actions.get("missing") == "ReceiveFile"
        )

        ok = mod_ok and last_seen_ok
        print(f"[02.8] {description}: {'PASS' if ok else 'FAIL'}")
        if not ok:
            failures.append(
                "02.8: expected mod_default Unchanged and missing ReceiveFile with entry_kind File, "
                f"got classifications={mod_classes}, entry_kind={_entry_kind(last_seen_result)}, "
                f"actions={last_seen_actions}"
            )
    except Exception as exc:
        print(f"[02.8] {description}: FAIL")
        failures.append(f"02.8: {exc}")


def _run_assertions(client: McpClient) -> list[str]:
    failures: list[str] = []

    _expect_classifications(
        client,
        "02.1",
        "File matching non-tombstone history is Unchanged",
        _arguments({"p": _file(100)}, {"p": _history(100)}),
        {"p": "Unchanged"},
        failures,
    )
    _expect_classifications(
        client,
        "02.2",
        "File differing from non-tombstone history is Modified",
        _arguments({"p": _file(120)}, {"p": _history(100)}),
        {"p": "Modified"},
        failures,
    )
    _expect_classifications(
        client,
        "02.3",
        "File and Directory over tombstone history are Resurrected",
        _arguments(
            {"file_peer": _file(100), "dir_peer": _directory()},
            {
                "file_peer": _history(90, deleted_time=95),
                "dir_peer": _history(90, byte_size=-1, deleted_time=95),
            },
        ),
        {"file_peer": "Resurrected", "dir_peer": "Resurrected"},
        failures,
    )
    _expect_classifications(
        client,
        "02.4",
        "File and Directory with no history are New",
        _arguments({"file_peer": _file(100), "dir_peer": _directory()}, {}),
        {"file_peer": "New", "dir_peer": "New"},
        failures,
    )
    _expect_classifications(
        client,
        "02.5",
        "Absent with tombstone history is Deleted",
        _arguments({"p": _absent()}, {"p": _history(100, deleted_time=120)}),
        {"p": "Deleted"},
        failures,
    )
    _expect_classifications(
        client,
        "02.6",
        "Absent with non-tombstone history is AbsentUnconfirmed",
        _arguments({"p": _absent()}, {"p": _history(100, deleted_time=None)}),
        {"p": "AbsentUnconfirmed"},
        failures,
    )
    _expect_classifications(
        client,
        "02.7",
        "Absent with no history is NoOpinion",
        _arguments({"p": _absent()}, {}),
        {"p": "NoOpinion"},
        failures,
    )
    _expect_02_8(client, failures)

    return failures


def _surface_failures(reason: str) -> list[str]:
    failures = []
    for req_id, description in REQ_DESCRIPTIONS.items():
        print(f"[{req_id}] {description}: FAIL")
        failures.append(f"{req_id}: {reason}")
    return failures


def _print_failures(failures: list[str]) -> int:
    print("\nFAILURES:")
    for failure in failures:
        print(f"  - {failure}")
    return 1


def main() -> int:
    proc: subprocess.Popen[str] | None = None
    try:
        proc, port = _launch()
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            response = _rpc(sock, "tools/list", rpc_id=1)
            if "error" in response:
                print(f"tools/list failed: {json.dumps(response['error'], sort_keys=True)}")
                return _print_failures(_surface_failures("tools/list failed"))
            tools = (response.get("result") or {}).get("tools", [])
            if not isinstance(tools, list):
                print("tools/list did not return a tools array")
                return _print_failures(_surface_failures("tools/list did not return a tools array"))
            tool_name = _find_decide_tool(tools)
            print(f"tools/list returned {len(tools)} tool(s); decide tool={tool_name!r}")
            if tool_name is None:
                return _print_failures(_surface_failures("decide-entry tool was not exposed"))

            failures = _run_assertions(McpClient(sock, tool_name))
            if failures:
                return _print_failures(failures)
            print("\nAll assertions passed.")
            return 0
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
