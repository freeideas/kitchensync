#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise url-normalizer public API via MCP."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional


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
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
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
        while b"\n" in buffer:
            line, _, rest = buffer.partition(b"\n")
            buffer = rest
            line = line.strip()
            if not line:
                continue
            return json.loads(line.decode("utf-8"))
    raise TimeoutError("timeout waiting for JSON-RPC response")


def _rpc(sock: socket.socket, method: str, params: Optional[Dict[str, Any]], request_id: int) -> Dict[str, Any]:
    message: Dict[str, Any] = {"jsonrpc": "2.0", "id": request_id, "method": method}
    if params is not None:
        message["params"] = params
    sock.sendall((json.dumps(message) + "\n").encode("utf-8"))
    return _read_json_message(sock)


def _call_tool(sock: socket.socket, tool_name: str, arguments: Dict[str, Any], request_id: int) -> Dict[str, Any]:
    return _rpc(sock, "tools/call", {"name": tool_name, "arguments": arguments}, request_id)


def _contains_token(value: Any, token: str) -> bool:
    token = token.lower()
    if isinstance(value, dict):
        for k, v in value.items():
            if _contains_token(k, token) or _contains_token(v, token):
                return True
        return False
    if isinstance(value, (list, tuple)):
        return any(_contains_token(item, token) for item in value)
    return token in str(value).lower()


def _error_token(response: Dict[str, Any]) -> Optional[str]:
    error = response.get("error")
    if not isinstance(error, dict):
        return None
    return str(error.get("message", error)).lower()


def _extract_normalized_value(result: Any) -> Optional[str]:
    if isinstance(result, str):
        return result
    if not isinstance(result, dict):
        return None
    for key in ("normalized_url", "value", "url", "result", "text"):
        value = result.get(key)
        if isinstance(value, str):
            return value
    if "content" in result:
        content = result.get("content")
        if isinstance(content, list):
            for item in content:
                if isinstance(item, dict):
                    text = item.get("text")
                    if isinstance(text, str):
                        return text
                if isinstance(item, str):
                    return item
        elif isinstance(content, str):
            return content
    return None


def _find_normalize_tool(tools: List[Dict[str, Any]]) -> Optional[Dict[str, Any]]:
    candidates = [
        "normalize-url",
        "normalize_url",
        "url-normalize",
        "normalize",
    ]
    for tool in tools:
        name = str(tool.get("name", ""))
        lower = name.lower()
        if lower in candidates or lower.endswith("normalizeurl"):
            return tool
        if "normalize" in lower and "url" in lower:
            return tool
    for tool in tools:
        if tool.get("name") in candidates:
            return tool
    return None


def _build_context(text: str) -> Dict[str, str]:
    return {"current_working_directory": text, "current_os_user": "test-user"}


def _build_arguments(tool: Dict[str, Any], url_text: str, context: Dict[str, str]) -> Dict[str, Any]:
    schema = tool.get("inputSchema") or {}
    props = schema.get("properties") or {}
    required = schema.get("required") or []

    if not isinstance(required, list):
        required = []

    text_candidates = ("text", "url", "url_text", "input", "value", "path")
    context_candidates = ("context", "parse_context", "parseContext", "current_context", "ctx")

    def _pick_text_key() -> str:
        for key in text_candidates:
            if key in required and key in props:
                return key
        for key in required:
            if key in props and props[key].get("type") == "string":
                return key
        for key in text_candidates:
            if key in props:
                return key
        return required[0] if required else "text"

    text_key = _pick_text_key()

    for key in context_candidates:
        if key in props:
            return {text_key: url_text, key: context}

    if "current_working_directory" in props and "current_os_user" in props:
        return {
            text_key: url_text,
            "current_working_directory": context["current_working_directory"],
            "current_os_user": context["current_os_user"],
        }

    # Last-resort fallback.
    return {text_key: url_text, "context": context}


def _expected_file_url(abs_path: str) -> str:
    normalized = abs_path.replace("\\", "/")
    if normalized.startswith("//"):
        return f"file:{normalized}"
    if normalized.startswith("/"):
        return f"file://{normalized}"
    if len(normalized) >= 3 and normalized[1] == ":":
        return f"file:///{normalized}"
    return f"file:///{normalized}"


def _assert_equal_url(
    failures: List[str],
    case_id: str,
    actual: Optional[str],
    expected: str,
) -> None:
    if actual is None:
        failures.append(f"{case_id}: normalize_url did not return a string result")
        return
    if actual != expected:
        failures.append(f"{case_id}: expected '{expected}', got '{actual}'")


def _assert_error(
    failures: List[str],
    case_id: str,
    response: Dict[str, Any],
    token: str,
    context: str,
) -> None:
    error = response.get("error")
    if error is None:
        failures.append(f"{case_id}: expected error '{token}' for {context}, but call succeeded with {response.get('result')!r}")
        return
    if not _contains_token(error, token):
        failures.append(
            f"{case_id}: expected error '{token}' for {context}, got {error!r}"
        )


def main() -> int:
    failures: List[str] = []
    proc: subprocess.Popen[str] | None = None
    request_id = 1

    try:
        proc, port = _launch_mcp()
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
                tools_response = _rpc(sock, "tools/list", None, request_id)
                request_id += 1
                tools = tools_response.get("result", {}).get("tools")
                if not isinstance(tools, list):
                    failures.append("01: tools/list did not return a tools list")
                    tools = []

                normalize_tool = _find_normalize_tool(tools)
                if normalize_tool is None:
                    failures.append("02: normalize_url tool not found in tools/list")
                else:
                    context = _build_context("/tmp/url-parser-normalizer-cwd")
                    cwd = Path(context["current_working_directory"])

                    args_base = _build_arguments(
                        normalize_tool,
                        "SFTP://Bob@Example.Com:22//alpha//beta//file.txt?x=1&y=2",
                        context,
                    )
                    response = _call_tool(sock, normalize_tool["name"], args_base, request_id)
                    request_id += 1
                    if response.get("error") is not None:
                        failures.append(f"03: normalize_url happy-path failed: {response}")
                    else:
                        result = _extract_normalized_value(response.get("result"))
                        expected = "sftp://Bob@example.com/alpha/beta/file.txt"
                        _assert_equal_url(failures, "03", result, expected)

                    response = _call_tool(
                        sock,
                        normalize_tool["name"],
                        _build_arguments(normalize_tool, "sftp://example.com/data/", context),
                        request_id,
                    )
                    request_id += 1
                    if response.get("error") is not None:
                        failures.append(f"04: lowercase+default-port+trailing-slash check failed: {response}")
                    else:
                        result = _extract_normalized_value(response.get("result"))
                        _assert_equal_url(failures, "04", result, "sftp://test-user@example.com/data")

                    response = _call_tool(
                        sock,
                        normalize_tool["name"],
                        _build_arguments(normalize_tool, "/tmp/logs//2023///app/", context),
                        request_id,
                    )
                    request_id += 1
                    if response.get("error") is not None:
                        failures.append(f"05: absolute file-path normalization failed: {response}")
                    else:
                        result = _extract_normalized_value(response.get("result"))
                        _assert_equal_url(failures, "05", result, "file:///tmp/logs/2023/app")

                    response = _call_tool(
                        sock,
                        normalize_tool["name"],
                        _build_arguments(normalize_tool, "relative/path/file.txt", context),
                        request_id,
                    )
                    request_id += 1
                    if response.get("error") is not None:
                        failures.append(f"06: bare relative path normalization failed: {response}")
                    else:
                        result = _extract_normalized_value(response.get("result"))
                        expected = _expected_file_url(str((cwd / "relative/path/file.txt").resolve()))
                        _assert_equal_url(failures, "06", result, expected)

                    response = _call_tool(
                        sock,
                        normalize_tool["name"],
                        _build_arguments(normalize_tool, "file:///tmp/tmp/%7Edemo//", context),
                        request_id,
                    )
                    request_id += 1
                    if response.get("error") is not None:
                        failures.append(f"07: percent decoding + slash collapsing failed: {response}")
                    else:
                        result = _extract_normalized_value(response.get("result"))
                        _assert_equal_url(failures, "07", result, "file:///tmp/tmp/~demo")

                    response = _call_tool(
                        sock,
                        normalize_tool["name"],
                        _build_arguments(normalize_tool, "file:///tmp/keep?mc=1&ct=2&ka=3", context),
                        request_id,
                    )
                    request_id += 1
                    if response.get("error") is not None:
                        failures.append(f"08: query stripping failed: {response}")
                    else:
                        result = _extract_normalized_value(response.get("result"))
                        _assert_equal_url(failures, "08", result, "file:///tmp/keep")

                    response = _call_tool(
                        sock,
                        normalize_tool["name"],
                        _build_arguments(normalize_tool, "sftp://host.example.com/path/to/res", context),
                        request_id,
                    )
                    request_id += 1
                    if response.get("error") is not None:
                        failures.append(f"09: missing SFTP username insertion failed: {response}")
                    else:
                        result = _extract_normalized_value(response.get("result"))
                        _assert_equal_url(failures, "09", result, "sftp://test-user@host.example.com/path/to/res")

                    response = _call_tool(
                        sock,
                        normalize_tool["name"],
                        _build_arguments(normalize_tool, "not a valid url", context),
                        request_id,
                    )
                    request_id += 1
                    _assert_error(failures, "10", response, "invalid_url", "invalid URL input")

                    response = _call_tool(
                        sock,
                        normalize_tool["name"],
                        _build_arguments(normalize_tool, "ftp://example.com/resource", context),
                        request_id,
                    )
                    request_id += 1
                    _assert_error(failures, "11", response, "unsupported_scheme", "unsupported scheme")

                    response = _call_tool(
                        sock,
                        normalize_tool["name"],
                        _build_arguments(normalize_tool, "sftp://example.com:99999/resource", context),
                        request_id,
                    )
                    request_id += 1
                    _assert_error(failures, "12", response, "invalid_port", "invalid port")

                    response = _call_tool(
                        sock,
                        normalize_tool["name"],
                        _build_arguments(normalize_tool, "file:///tmp/%ZZ", context),
                        request_id,
                    )
                    request_id += 1
                    _assert_error(failures, "13", response, "invalid_percent_encoding", "malformed percent encoding")
        finally:
            if proc is not None:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
    finally:
        if proc is not None and proc.poll() is None:
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
