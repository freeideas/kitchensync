#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise gitignore-pattern-hierarchy public API through MCP."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Dict, List

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = Path(os.environ.get("AITC_PROJECT", "."))


def _drain(stream) -> None:
    for _ in stream:
        pass


def _launch_mcp() -> tuple[subprocess.Popen[str], int]:
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", str(PROJECT)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )

    port: int | None = None
    deadline = time.time() + 30.0
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline() if proc.stdout else ""
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


def _rpc(sock: socket.socket, method: str, params: Dict[str, Any] | None, request_id: int) -> Dict[str, Any]:
    request: Dict[str, Any] = {"jsonrpc": "2.0", "id": request_id, "method": method}
    if params is not None:
        request["params"] = params
    sock.sendall((json.dumps(request) + "\n").encode("utf-8"))

    buffer = b""
    deadline = time.time() + 10.0
    while time.time() < deadline:
        sock.settimeout(1.0)
        try:
            chunk = sock.recv(65536)
        except TimeoutError:
            continue
        if not chunk:
            break
        buffer += chunk
        while b"\n" in buffer:
            line, _, rest = buffer.partition(b"\n")
            buffer = rest
            line = line.strip()
            if not line:
                continue
            return json.loads(line.decode("utf-8"))
    raise TimeoutError(f"timeout waiting for response to request id={request_id}")


def _call_tool(sock: socket.socket, tool_name: str, arguments: Dict[str, Any], request_id: int) -> Dict[str, Any]:
    return _rpc(
        sock,
        "tools/call",
        {"name": tool_name, "arguments": arguments},
        request_id,
    )


def _contains_token(value: Any, token: str) -> bool:
    token = token.lower()
    if isinstance(value, dict):
        for v in value.values():
            if _contains_token(v, token):
                return True
        return False
    if isinstance(value, (list, tuple)):
        return any(_contains_token(v, token) for v in value)
    return token in str(value).lower()


def _find_tool(
    tools: List[Dict[str, Any]],
    exact_names: List[str],
    required_tokens: List[str],
) -> Dict[str, Any] | None:
    lower_exact = {name.lower() for name in exact_names}
    for tool in tools:
        if str(tool.get("name", "")).lower() in lower_exact:
            return tool

    for tool in tools:
        name = str(tool.get("name", "")).lower()
        if all(token in name for token in required_tokens):
            return tool

    for tool in tools:
        schema = tool.get("inputSchema") or {}
        required = {str(x).lower() for x in schema.get("required", [])}
        if any(token in required for token in required_tokens):
            return tool
        for prop in required:
            if any(token in prop for token in required_tokens):
                return tool

    return None


def _find_key(props: Dict[str, Any], candidates: List[str]) -> str | None:
    for key in candidates:
        if key in props:
            return key
    return None


def _extract_content_text(result: Any) -> Any:
    if not isinstance(result, dict):
        return result

    if "content" in result and isinstance(result["content"], list):
        for item in result["content"]:
            if isinstance(item, dict):
                text = item.get("text")
                if isinstance(text, str):
                    text = text.strip()
                    if not text:
                        continue
                    if text.startswith("{") or text.startswith("["):
                        try:
                            parsed = json.loads(text)
                            return parsed
                        except json.JSONDecodeError:
                            pass
                    return text
            elif isinstance(item, str):
                text = item.strip()
                if not text:
                    continue
                if text.startswith("{") or text.startswith("["):
                    try:
                        return json.loads(text)
                    except json.JSONDecodeError:
                        pass
                return text

    if "result" in result and isinstance(result["result"], (dict, list)):
        return result["result"]

    return result


def _normalize_path(path: str) -> str:
    return "/".join(part for part in path.replace("\\", "/").split("/") if part and part != ".")


def _relative_path(path: str, base: str) -> str | None:
    path = _normalize_path(path)
    base = _normalize_path(base)
    if base == "":
        return path
    base_parts = base.split("/")
    path_parts = path.split("/")
    if path_parts[: len(base_parts)] != base_parts:
        return None
    return "/".join(path_parts[len(base_parts) :])


def _extract_result_list(payload: Any, schema: Any = None) -> tuple[bool, List[Any] | str]:
    if isinstance(payload, list):
        return True, payload

    if not isinstance(payload, dict):
        return False, "result payload is not dict/list"

    if "value" in payload and isinstance(payload["value"], list):
        return True, payload["value"]
    if "patterns" in payload and isinstance(payload["patterns"], list):
        return True, payload["patterns"]
    if "result" in payload and isinstance(payload["result"], list):
        return True, payload["result"]
    if "items" in payload and isinstance(payload["items"], list):
        return True, payload["items"]

    for value in payload.values():
        if isinstance(value, list):
            return True, value

    return False, "result payload has no list field"


def _select_pattern_field(item: Any) -> str | None:
    if not isinstance(item, dict):
        return None
    for key in ("pattern_line", "patternLine", "pattern", "line", "text", "value"):
        value = item.get(key)
        if isinstance(value, str):
            return value
    # fallback: pick the first string field (excluding base_path/path keys)
    for key, value in item.items():
        if key.lower() not in {"base_path", "basepath", "path", "input_path"} and isinstance(value, str):
            return value
    return None


def _select_base_field(item: Any) -> str | None:
    if not isinstance(item, dict):
        return None
    for key in ("base_path", "basePath", "base", "source_base_path", "set_base_path"):
        value = item.get(key)
        if isinstance(value, str):
            return value
    return None


def _select_relative_path_field(item: Any) -> str | None:
    if not isinstance(item, dict):
        return None
    for key in ("path", "path_relative", "relative_path", "input_path", "input"):
        value = item.get(key)
        if isinstance(value, str):
            return value
    return None


def _tool_result(resp: Dict[str, Any]) -> tuple[bool, Any]:
    if "error" in resp:
        return False, resp["error"]
    return True, _extract_content_text(resp.get("result"))


def _build_compile_payloads(tool: Dict[str, Any], pattern_sets: List[Dict[str, Any]]) -> List[Dict[str, Any]]:
    schema = tool.get("inputSchema") or {}
    props = schema.get("properties") or {}

    top_keys = [
        "pattern_sets",
        "patternSets",
        "patterns",
        "pattern_sets_argument",
    ]
    if isinstance(schema.get("required"), list):
        for key in schema["required"]:
            if key in props:
                top_keys = [key] + top_keys
                break

    base_candidates = ["base_path", "basePath", "base"]
    line_candidates = ["pattern_lines", "patternLines", "patterns", "lines", "pattern_line", "line"]

    payloads: List[Dict[str, Any]] = []
    for top_key in top_keys:
        if top_key not in props:
            continue
        for base_key in base_candidates:
            for line_key in line_candidates:
                translated = []
                for item in pattern_sets:
                    translated.append({base_key: item["base_path"], line_key: item["pattern_lines"]})
                payloads.append({top_key: translated})
    return payloads or [ {"pattern_sets": pattern_sets} ]


def _build_patterns_payloads(tool: Dict[str, Any], hierarchy: Any, path: str) -> List[Dict[str, Any]]:
    schema = tool.get("inputSchema") or {}
    props = schema.get("properties") or {}
    required = set(schema.get("required") or [])

    hierarchy_keys = [key for key in ("hierarchy", "pattern_hierarchy", "patternHierarchy") if key in props]
    if not hierarchy_keys and required:
        hierarchy_keys = [key for key in required if key in props and isinstance(props[key], dict)]
    if not hierarchy_keys and props:
        hierarchy_keys = [next(iter(props))]

    path_keys = ["path", "path_relative", "relative_path", "input_path"]

    payloads: List[Dict[str, Any]] = []
    for hkey in hierarchy_keys:
        # Common shape: {hierarchy, input: {path: ...}}
        if "input" in props or "input" in required:
            input_schema = props.get("input") if isinstance(props.get("input"), dict) else {}
            input_props = input_schema.get("properties") if isinstance(input_schema, dict) else {}
            chosen_path_key = _find_key(input_props or {}, path_keys) or "path"
            payload = {hkey: hierarchy, "input": {chosen_path_key: path}}
            if "input" not in required and "hierarchy" in required:
                payload = {hkey: hierarchy, "input": {chosen_path_key: path}}
            payloads.append(payload)

        # Alternative flat shape: {hierarchy, path}
        if "path" in props or "path" in required:
            payloads.append({hkey: hierarchy, "path": path})

    # Legacy fallback when property names are unknown.
    if not payloads:
        payloads.append({"hierarchy": hierarchy, "path": path})
    return payloads


def main() -> int:
    failures: List[str] = []
    proc: subprocess.Popen[str] | None = None
    request_id = 1

    try:
        proc, port = _launch_mcp()
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            tools_response = _rpc(sock, "tools/list", None, request_id)
            request_id += 1
            tools = tools_response.get("result", {}).get("tools")
            if not isinstance(tools, list):
                failures.append("01 tools/list did not return a list")
                tools = []

            compile_tool = _find_tool(
                tools,
                [
                    "compile_hierarchy",
                    "compile-hierarchy",
                    "compile-hier",
                ],
                ["compile", "hierarchy"],
            )
            if compile_tool is None:
                failures.append("01 unable to locate compile_hierarchy in tools/list output")

            patterns_tool = _find_tool(
                tools,
                [
                    "patterns_for_path",
                    "patterns-for-path",
                    "pattern-for-path",
                ],
                ["patterns", "path"],
            )
            if patterns_tool is None:
                failures.append("02 unable to locate patterns_for_path in tools/list output")

            if compile_tool is None or patterns_tool is None:
                return 1

            base_pattern_sets = [
                {"base_path": "", "pattern_lines": ["*.root", "!keep", "weird[.]{1}pattern"]},
                {"base_path": "services", "pattern_lines": ["services/**/*.tmp", "!services/.keep", "svc/**/a?c"]},
                {"base_path": "services/api", "pattern_lines": ["api-root", "api-**/*.log"]},
            ]

            compile_response = None
            for payload in _build_compile_payloads(compile_tool, base_pattern_sets):
                try:
                    compile_response = _call_tool(sock, compile_tool["name"], payload, request_id)
                    request_id += 1
                except Exception as exc:
                    compile_response = {"error": {"message": str(exc)}}
                ok, payload_obj = _tool_result(compile_response)
                if ok:
                    hierarchy = _extract_content_text(payload_obj)
                    if hierarchy is not None:
                        break
                compile_response = compile_response
            else:
                hierarchy = None

            if not isinstance(hierarchy, (dict, list, str)):
                failures.append(f"03 compile_hierarchy failed: {compile_response}")
                return 1

            pattern_response = None
            target_path = "services/api/src/app.log"
            patterns_payloads = _build_patterns_payloads(patterns_tool, hierarchy, target_path)
            for payload in patterns_payloads:
                try:
                    pattern_response = _call_tool(sock, patterns_tool["name"], payload, request_id)
                    request_id += 1
                except Exception as exc:
                    pattern_response = {"error": {"message": str(exc)}}
                ok, extracted = _tool_result(pattern_response)
                if not ok:
                    continue
                extracted = _extract_content_text(extracted)
                if isinstance(extracted, dict):
                    is_list, result_payload = _extract_result_list(extracted)
                    if is_list:
                        scoped_patterns = result_payload
                        break
                elif isinstance(extracted, list):
                    scoped_patterns = extracted
                    break
            else:
                scoped_patterns = None

            if not isinstance(scoped_patterns, list):
                failures.append(f"03 patterns_for_path did not return scoped pattern lines for input '{target_path}': {pattern_response}")
                scoped_patterns = []

            expected = [
                ("*.root", "", "services/api/src/app.log"),
                ("!keep", "", "services/api/src/app.log"),
                ("weird[.]{1}pattern", "", "services/api/src/app.log"),
                ("services/**/*.tmp", "services", "api/src/app.log"),
                ("!services/.keep", "services", "api/src/app.log"),
                ("svc/**/a?c", "services", "api/src/app.log"),
                ("api-root", "services/api", "src/app.log"),
                ("api-**/*.log", "services/api", "src/app.log"),
            ]

            received_patterns = []
            for item in scoped_patterns:
                pattern_text = _select_pattern_field(item)
                base_text = _select_base_field(item)
                rel_text = _select_relative_path_field(item)
                if pattern_text is not None:
                    received_patterns.append((pattern_text, base_text, rel_text))

            if len(received_patterns) != len(expected):
                failures.append(
                    f"04 expected {len(expected)} scoped lines for '{target_path}', got {len(received_patterns)}"
                )

            for index, (pattern_text, base_expected, rel_expected) in enumerate(expected, start=1):
                if index > len(received_patterns):
                    failures.append(f"04-{index:02d}: missing {pattern_text!r} at hierarchy slot {index}")
                    continue
                recv_pattern, recv_base, recv_rel = received_patterns[index - 1]
                if recv_pattern != pattern_text:
                    failures.append(
                        f"04-{index:02d}: expected pattern {pattern_text!r} at position {index}, got {recv_pattern!r}"
                    )
                if recv_base is not None and _normalize_path(recv_base) != _normalize_path(base_expected):
                    failures.append(
                        f"04-{index:02d}: expected base_path {_normalize_path(base_expected)!r}, got {recv_base!r}"
                    )
                if recv_rel is not None and _normalize_path(recv_rel) != _normalize_path(rel_expected):
                    failures.append(
                        f"04-{index:02d}: expected input path {_normalize_path(rel_expected)!r}, got {recv_rel!r}"
                    )
                if recv_pattern is not None and recv_pattern != pattern_text:
                    failures.append(f"05 pattern text was normalized unexpectedly: expected {pattern_text!r}, got {recv_pattern!r}")

            for item_pattern, item_base, item_rel in received_patterns:
                if item_base is None or item_rel is None:
                    continue
                expected_rel = _relative_path(target_path, item_base or "")
                if expected_rel is not None and _normalize_path(item_rel) != _normalize_path(expected_rel):
                    failures.append(
                        f"04 path-relative output mismatch: base {item_base!r} should produce {_normalize_path(expected_rel)!r}, got {_normalize_path(item_rel)!r}"
                    )

            non_matching_path = "other-branch/readme.txt"
            non_match_patterns = None
            for payload in _build_patterns_payloads(patterns_tool, hierarchy, non_matching_path):
                try:
                    non_match_response = _call_tool(sock, patterns_tool["name"], payload, request_id)
                    request_id += 1
                except Exception as exc:
                    non_match_response = {"error": {"message": str(exc)}}
                ok, extracted = _tool_result(non_match_response)
                if not ok:
                    continue
                extracted = _extract_content_text(extracted)
                if isinstance(extracted, dict):
                    is_list, lst = _extract_result_list(extracted)
                    if is_list:
                        non_match_patterns = lst
                        break
                elif isinstance(extracted, list):
                    non_match_patterns = extracted
                    break

            if non_match_patterns is None:
                failures.append(f"06 patterns_for_path for '{non_matching_path}' returned no usable result: {non_match_response}")
            elif len(non_match_patterns) != 3:
                failures.append(
                    f"06 expected only root patterns for '{non_matching_path}', got {len(non_match_patterns)} item(s)"
                )

            non_io_path = "ghost-folder/no-such-file.txt"
            io_response = None
            for payload in _build_patterns_payloads(patterns_tool, hierarchy, non_io_path):
                try:
                    io_response = _call_tool(sock, patterns_tool["name"], payload, request_id)
                    request_id += 1
                except Exception as exc:
                    io_response = {"error": {"message": str(exc)}}
                io_ok, _ = _tool_result(io_response)
                if io_ok:
                    break

            if not isinstance(io_response, dict) or "error" in io_response:
                failures.append(f"07 patterns_for_path should handle nonexistent paths without I/O (no error) for '{non_io_path}': {io_response}")

            invalid_base_set = [{"base_path": "../outside", "pattern_lines": ["*.tmp"]}]
            invalid_base_response = None
            for payload in _build_compile_payloads(compile_tool, invalid_base_set):
                try:
                    invalid_base_response = _call_tool(sock, compile_tool["name"], payload, request_id)
                    request_id += 1
                except Exception as exc:
                    invalid_base_response = {"error": {"message": str(exc)}}
                if "error" in invalid_base_response:
                    break
            if "error" not in invalid_base_response:
                failures.append("08 expected malformed base_path to fail, but compile_hierarchy succeeded")
            else:
                if not _contains_token(invalid_base_response["error"], "invalid_path"):
                    failures.append(f"08 malformed base_path did not return invalid_path: {invalid_base_response}")

            invalid_path_response = None
            for payload in _build_patterns_payloads(patterns_tool, hierarchy, "/abs/path/file.txt"):
                try:
                    invalid_path_response = _call_tool(sock, patterns_tool["name"], payload, request_id)
                    request_id += 1
                except Exception as exc:
                    invalid_path_response = {"error": {"message": str(exc)}}
                if "error" in invalid_path_response:
                    break
            if "error" not in invalid_path_response:
                failures.append("09 expected malformed input path to fail, but patterns_for_path succeeded")
            else:
                if not _contains_token(invalid_path_response["error"], "invalid_path"):
                    failures.append(f"09 malformed input path did not return invalid_path: {invalid_path_response}")

    except Exception as exc:
        failures.append(f"00 unexpected exception: {exc}")
    finally:
        if proc is not None:
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
