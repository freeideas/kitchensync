#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Verify path-id and parent-id behavior through the MCP wrapper."""

from __future__ import annotations

import json
import os
import re
import socket
import subprocess
import sys
import threading
import queue
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

PATH_ID = re.compile(r"^[0-9A-Za-z]{11}$")


def _collect_lines(stream, output_queue: queue.Queue[str]):
    for line in stream:
        output_queue.put(line)


def _launch():
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    import time

    stdout_lines: queue.Queue[str] = queue.Queue()
    stderr_lines: queue.Queue[str] = queue.Queue()
    threading.Thread(
        target=_collect_lines, args=(proc.stdout, stdout_lines), daemon=True
    ).start()
    threading.Thread(
        target=_collect_lines, args=(proc.stderr, stderr_lines), daemon=True
    ).start()

    port = None
    deadline = time.time() + 30
    while time.time() < deadline:
        if proc.poll() is not None:
            break
        try:
            line = stdout_lines.get(timeout=0.1)
        except queue.Empty:
            continue
        line = line.strip()
        if line.startswith("MCP_PORT="):
            port = int(line.split("=", 1)[1])
            break

    if port is None:
        proc.terminate()
        stderr_output = []
        while not stderr_lines.empty():
            stderr_output.append(stderr_lines.get())
        raise RuntimeError(
            f"MCP_PORT was not advertised; stderr={''.join(stderr_output)!r}"
        )

    return proc, port


def _rpc(sock: socket.socket, method: str, params, rpc_id: int):
    message = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        message["params"] = params
    sock.sendall((json.dumps(message) + "\n").encode("utf-8"))

    buffer = b""
    while True:
        chunk = sock.recv(8192)
        if not chunk:
            raise RuntimeError("MCP socket closed before receiving a response")
        buffer += chunk
        if b"\n" in buffer:
            line, _, _ = buffer.partition(b"\n")
            return json.loads(line.decode("utf-8"))


def _find_tool(tools, predicate, *, fallback=None):
    matches = [tool for tool in tools if predicate(tool)]
    if matches:
        return matches[0]
    return fallback


def _norm_name(tool_name: str) -> str:
    return (tool_name or "").strip().lower().replace("_", "-")


def _pick_string_field(schema):
    if not isinstance(schema, dict):
        return None

    properties = schema.get("properties") or {}
    if schema.get("type") == "string":
        return None

    required = schema.get("required")
    if isinstance(required, list):
        candidates = [
            key
            for key in required
            if isinstance(properties.get(key), dict)
            and properties.get(key, {}).get("type") == "string"
        ]
        if len(candidates) == 1:
            return candidates[0]

    string_fields = [
        name
        for name, spec in properties.items()
        if isinstance(spec, dict) and spec.get("type") == "string"
    ]
    if len(string_fields) == 1:
        return string_fields[0]

    return None


def _tool_field(tool, direction: str):
    if direction == "arg":
        schema = tool.get("inputSchema")
    else:
        schema = tool.get("outputSchema")
    return _pick_string_field(schema)


def _extract_string(result, schema_key):
    if isinstance(result, str):
        return result
    if not isinstance(result, dict):
        return None
    if schema_key is not None:
        value = result.get(schema_key)
        if isinstance(value, str):
            return value
    for value in result.values():
        if isinstance(value, str):
            return value
    return None


def _tool_error(resp):
    if not isinstance(resp, dict):
        return None, None, None
    error = resp.get("error")
    if not isinstance(error, dict):
        return None, None, None
    return error.get("code"), error.get("message"), error.get("data")


def _tool_result(resp):
    if not isinstance(resp, dict):
        return None
    return resp.get("result")


def _record(failures, idx, condition, message):
    if not condition:
        failures.append(f"[{idx:02d}] {message}")


def _assert_path_id_format(failures, idx, label, value):
    _record(
        failures,
        idx,
        isinstance(value, str) and PATH_ID.fullmatch(value) is not None,
        f"{label}: returned value {value!r} is not 11-char base62 string",
    )


def _assert_tool_error(failures, idx, resp, expected_code, expected_message_fragment, context):
    code, message, _data = _tool_error(resp)
    _record(
        failures,
        idx,
        code == expected_code and isinstance(message, str)
        and expected_message_fragment in message,
        f"{context}: expected tool error {expected_code} with '{expected_message_fragment}', "
        f"got code={code!r}, message={message!r}",
    )


def main() -> int:
    failures = []
    proc = None

    try:
        proc, port = _launch()
    except Exception as exc:
        failures.append(f"[01] Failed to launch MCP wrapper: {exc!r}")
        print("\nFAILURES:")
        for item in failures:
            print(f"  - {item}")
        return 1

    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            request_id = 1
            tools_list = _rpc(sock, "tools/list", None, request_id)
            request_id += 1

            _record(
                failures,
                2,
                isinstance(tools_list, dict) and "error" not in tools_list,
                f"tools/list returned protocol error: {tools_list!r}",
            )

            tools = []
            if isinstance(tools_list, dict):
                tools = (tools_list.get("result") or {}).get("tools", [])

            _record(
                failures,
                3,
                isinstance(tools, list) and len(tools) > 0,
                f"tools/list result missing tools array: {tools!r}",
            )

            tool_lookup = {
                _norm_name(tool.get("name", "")): tool for tool in tools if isinstance(tool, dict)
            }

            path_id_tool = (
                _find_tool(tools, lambda t: _norm_name(t.get("name", "")) in {"path-id", "pathid"})
                or _find_tool(
                    tools,
                    lambda t: ("path" in _norm_name(t.get("name", "")))
                    and "parent" not in _norm_name(t.get("name", "")),
                )
                or _find_tool(
                    tools,
                    lambda t: "path id" in ((t.get("description", "") or "").lower())
                    and "parent" not in ((t.get("description", "") or "").lower()),
                )
                or _find_tool(
                    tools,
                    lambda t: "path id" in _norm_name(t.get("name", "")),
                )
            )
            parent_id_tool = (
                _find_tool(tools, lambda t: _norm_name(t.get("name", "")) in {"parent-id", "parentid"})
                or _find_tool(
                    tools,
                    lambda t: ("parent" in _norm_name(t.get("name", ""))
                               and ("path" in _norm_name(t.get("name", "")))),
                )
                or _find_tool(
                    tools,
                    lambda t: "parent id" in ((t.get("description", "") or "").lower())
                    and "path" in ((t.get("description", "") or "").lower()),
                )
            )

            # If discovery failed but two path-related tools are present, force split by name.
            if path_id_tool is None or parent_id_tool is None:
                pathish = [
                    tool
                    for tool in tools
                    if isinstance(tool, dict)
                    and ("path" in _norm_name(tool.get("name", "")))
                ]
                if len(pathish) == 2 and path_id_tool is None and parent_id_tool is None:
                    if "parent" in _norm_name(pathish[0].get("name", "")):
                        parent_id_tool, path_id_tool = pathish[0], pathish[1]
                    elif "parent" in _norm_name(pathish[1].get("name", "")):
                        parent_id_tool, path_id_tool = pathish[1], pathish[0]

            _record(
                failures,
                4,
                path_id_tool is not None and parent_id_tool is not None,
                "Could not identify both path-id and parent-id tools from tools/list",
            )

            if path_id_tool is None or parent_id_tool is None:
                print("\nFAILURES:")
                for item in failures:
                    print(f"  - {item}")
                return 1

            path_id_name = path_id_tool.get("name")
            parent_id_name = parent_id_tool.get("name")

            path_arg = _tool_field(path_id_tool, "arg")
            parent_arg = _tool_field(parent_id_tool, "arg")
            _record(
                failures,
                5,
                isinstance(path_arg, str) and isinstance(parent_arg, str),
                f"Could not infer required argument names from schemas: path={path_arg!r}, parent={parent_arg!r}",
            )

            path_result_key = _tool_field(path_id_tool, "result")
            parent_result_key = _tool_field(parent_id_tool, "result")

            valid_path = "dir/file.txt"
            valid_nested = "dir/file.txt/child"
            root_child_a = "alpha"
            root_child_b = "beta"

            path_resp_1 = _rpc(
                sock,
                "tools/call",
                {"name": path_id_name, "arguments": {path_arg: valid_path}},
                request_id,
            )
            request_id += 1
            _record(
                failures,
                6,
                "error" not in path_resp_1,
                f"path-id({valid_path!r}) returned error: {path_resp_1!r}",
            )
            path_id_value_1 = _extract_string(
                _tool_result(path_resp_1), path_result_key
            )
            _record(
                failures,
                7,
                isinstance(path_id_value_1, str),
                f"path-id({valid_path!r}) result could not be read as string: {path_resp_1!r}",
            )
            _assert_path_id_format(failures, 8, "path-id result", path_id_value_1)

            path_resp_2 = _rpc(
                sock,
                "tools/call",
                {"name": path_id_name, "arguments": {path_arg: valid_path}},
                request_id,
            )
            request_id += 1
            path_id_value_2 = _extract_string(
                _tool_result(path_resp_2), path_result_key
            )
            _record(
                failures,
                9,
                path_id_value_1 == path_id_value_2,
                f"path-id({valid_path!r}) is not deterministic: first={path_id_value_1!r} second={path_id_value_2!r}",
            )

            directory_resp = _rpc(
                sock,
                "tools/call",
                {"name": path_id_name, "arguments": {path_arg: f"{valid_nested}/"}},
                request_id,
            )
            request_id += 1
            directory_value = _extract_string(
                _tool_result(directory_resp), path_result_key
            )
            _record(
                failures,
                10,
                "error" not in directory_resp,
                f"path-id({valid_nested!r}/) errored unexpectedly: {directory_resp!r}",
            )

            normalized_resp = _rpc(
                sock,
                "tools/call",
                {"name": path_id_name, "arguments": {path_arg: valid_nested}},
                request_id,
            )
            request_id += 1
            normalized_value = _extract_string(
                _tool_result(normalized_resp), path_result_key
            )
            _record(
                failures,
                11,
                directory_value == normalized_value,
                f"directory and file paths should hash identically: {directory_value!r} != {normalized_value!r}",
            )

            parent_resp_nested = _rpc(
                sock,
                "tools/call",
                {"name": parent_id_name, "arguments": {parent_arg: valid_nested}},
                request_id,
            )
            request_id += 1
            parent_nested = _extract_string(
                _tool_result(parent_resp_nested), parent_result_key
            )
            _record(
                failures,
                12,
                isinstance(parent_nested, str),
                f"parent-id({valid_nested!r}) returned non-string or missing result: {parent_resp_nested!r}",
            )
            _assert_path_id_format(failures, 13, "parent-id result", parent_nested)

            parent_target = valid_nested.rsplit("/", 1)[0]
            parent_target_resp = _rpc(
                sock,
                "tools/call",
                {"name": path_id_name, "arguments": {path_arg: parent_target}},
                request_id,
            )
            request_id += 1
            parent_target_value = _extract_string(
                _tool_result(parent_target_resp), path_result_key
            )
            _record(
                failures,
                14,
                parent_nested == parent_target_value,
                f"parent-id({valid_nested!r}) should be path-id({parent_target!r}); "
                f"got {parent_nested!r} vs {parent_target_value!r}",
            )

            root_parent_a = _rpc(
                sock,
                "tools/call",
                {"name": parent_id_name, "arguments": {parent_arg: root_child_a}},
                request_id,
            )
            request_id += 1
            root_parent_b = _rpc(
                sock,
                "tools/call",
                {"name": parent_id_name, "arguments": {parent_arg: root_child_b}},
                request_id,
            )
            request_id += 1
            root_parent_a_id = _extract_string(_tool_result(root_parent_a), parent_result_key)
            root_parent_b_id = _extract_string(_tool_result(root_parent_b), parent_result_key)

            _record(
                failures,
                15,
                root_parent_a_id == root_parent_b_id,
                f"root children should share the same parent id: {root_child_a!r}->{root_parent_a_id!r}, "
                f"{root_child_b!r}->{root_parent_b_id!r}",
            )
            _assert_path_id_format(failures, 16, "root-child parent id format", root_parent_a_id)
            _assert_path_id_format(failures, 17, "root-child parent id format", root_parent_b_id)

            invalid_path = "/invalid"
            invalid_path_resp = _rpc(
                sock,
                "tools/call",
                {"name": path_id_name, "arguments": {path_arg: invalid_path}},
                request_id,
            )
            request_id += 1
            _assert_tool_error(
                failures,
                18,
                invalid_path_resp,
                -32000,
                "invalid_path",
                f"path-id invalid input {invalid_path!r}",
            )

            invalid_parent_resp = _rpc(
                sock,
                "tools/call",
                {"name": parent_id_name, "arguments": {parent_arg: invalid_path}},
                request_id,
            )
            request_id += 1
            _assert_tool_error(
                failures,
                19,
                invalid_parent_resp,
                -32000,
                "invalid_path",
                f"parent-id invalid input {invalid_path!r}",
            )

    finally:
        if proc is not None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()

    if failures:
        print("\nFAILURES:")
        for item in failures:
            print(f"  - {item}")
        return 1

    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
