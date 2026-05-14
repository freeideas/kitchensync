#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise peer-argument parser public API via MCP."""

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


def _rpc(sock: socket.socket, method: str, params: Optional[Dict[str, Any]], request_id: int) -> Dict[str, Any]:
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
        if b"\n" in buffer:
            break
    while b"\n" not in buffer:
        break

    if not buffer:
        raise TimeoutError("timeout waiting for JSON-RPC response")

    line, _, _ = buffer.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _call_tool(sock: socket.socket, tool_name: str, arguments: Dict[str, Any], request_id: int) -> Dict[str, Any]:
    return _rpc(
        sock,
        "tools/call",
        {"name": tool_name, "arguments": arguments},
        request_id,
    )


def _normalize_token(value: str) -> str:
    return "".join(ch.lower() for ch in value if ch.isalnum())


def _contains_token(value: Any, token: str) -> bool:
    token = token.lower()
    if isinstance(value, dict):
        return any(_contains_token(v, token) for v in value.values())
    if isinstance(value, (list, tuple)):
        return any(_contains_token(item, token) for item in value)
    return token in str(value).lower()


def _find_tool_by_shape(tools: List[Dict[str, Any]]) -> Optional[Dict[str, Any]]:
    candidates = {
        "parsepeer",
        "parse_peer",
        "parsepeerargument",
        "peerargument",
        "peerargumentparser",
        "peerparse",
    }
    for tool in tools:
        name = _normalize_token(str(tool.get("name", "")))
        description = _normalize_token(str(tool.get("description", "")))
        for c in candidates:
            if c in name or c in description:
                return tool

    for tool in tools:
        output_schema = tool.get("outputSchema")
        if not isinstance(output_schema, dict):
            continue
        properties = output_schema.get("properties", {})
        if not isinstance(properties, dict):
            continue
        if "role" in properties and "urls" in properties:
            return tool
        if "result" in properties and isinstance(properties.get("result"), dict):
            inner = properties["result"]
            if isinstance(inner, dict):
                inner_props = inner.get("properties", {})
                if isinstance(inner_props, dict) and "role" in inner_props and "urls" in inner_props:
                    return tool
    return None


def _guess_argument_name(tool: Dict[str, Any]) -> str:
    schema = tool.get("inputSchema")
    if not isinstance(schema, dict):
        return "text"

    props = schema.get("properties", {})
    if not isinstance(props, dict):
        props = {}

    required = schema.get("required", [])
    if not isinstance(required, list):
        required = []

    for key in ("peer_argument", "peer", "text", "value", "input", "argument", "url"):
        if key in required and isinstance(props.get(key), dict):
            return key

    for key in ("peer_argument", "peer", "text", "value", "input", "argument", "url"):
        if key in props:
            return key

    if required:
        first_required = required[0]
        if isinstance(first_required, str):
            return first_required

    if props:
        key = next(iter(props))
        if isinstance(key, str):
            return key

    return "text"


def _extract_peer_payload(result: Any) -> Dict[str, Any] | None:
    if result is None:
        return None
    if isinstance(result, str):
        stripped = result.strip()
        if not stripped:
            return None
        try:
            return _extract_peer_payload(json.loads(stripped))
        except Exception:
            return None

    if isinstance(result, dict):
        if "role" in result and "urls" in result:
            return result
        for key in ("value", "result", "peer", "output"):
            extracted = _extract_peer_payload(result.get(key))
            if extracted is not None:
                return extracted
        if "content" in result:
            content_payload = _extract_peer_payload(result["content"])
            if content_payload is not None:
                return content_payload
        for value in result.values():
            extracted = _extract_peer_payload(value)
            if extracted is not None:
                return extracted
        return None

    if isinstance(result, list):
        for item in result:
            extracted = _extract_peer_payload(item)
            if extracted is not None:
                return extracted

    return None


def _extract_urls(value: Any) -> Optional[list[str]]:
    if not isinstance(value, list):
        return None
    urls: list[str] = []
    for item in value:
        if isinstance(item, str):
            urls.append(item)
            continue
        if isinstance(item, dict):
            candidate_keys = (
                "value",
                "text",
                "url",
                "url_text",
                "path",
                "peer_url",
            )
            matched = False
            for key in candidate_keys:
                if key in item and isinstance(item[key], str):
                    urls.append(item[key])
                    matched = True
                    break
            if matched:
                continue
            for child in item.values():
                if isinstance(child, str):
                    urls.append(child)
                    matched = True
                    break
            if matched:
                continue
        return None
    return urls


def _assert_success_case(
    failures: List[str],
    case_id: str,
    response: Dict[str, Any],
    expected_role: str,
    expected_urls: List[str],
) -> None:
    if response.get("error") is not None:
        failures.append(f"{case_id}: expected success, got error {response['error']!r}")
        return

    peer = _extract_peer_payload(response.get("result"))
    if peer is None:
        failures.append(f"{case_id}: parse_peer result did not expose role/urls payload: {response.get('result')!r}")
        return

    role = peer.get("role")
    if role is None:
        failures.append(f"{case_id}: role missing: {peer!r}")
        return
    if str(role).lower() != expected_role:
        failures.append(f"{case_id}: expected role '{expected_role}', got {role!r}")

    urls = _extract_urls(peer.get("urls"))
    if urls is None:
        failures.append(f"{case_id}: urls missing or malformed: {peer.get('urls')!r}")
        return
    if urls != expected_urls:
        failures.append(f"{case_id}: expected urls {expected_urls!r}, got {urls!r}")


def _assert_error_case(
    failures: List[str],
    case_id: str,
    response: Dict[str, Any],
    expected_error: str,
) -> None:
    error = response.get("error")
    if error is None:
        failures.append(f"{case_id}: expected error '{expected_error}', but call succeeded with {response.get('result')!r}")
        return
    if not _contains_token(error, expected_error):
        failures.append(f"{case_id}: expected error '{expected_error}', got {error!r}")


def _call_parse(sock: socket.socket, tool_name: str, arg_name: str, peer_text: str, request_id: int) -> Dict[str, Any]:
    return _call_tool(sock, tool_name, {arg_name: peer_text}, request_id)


def main() -> int:
    failures: List[str] = []
    proc: subprocess.Popen[str] | None = None
    request_id = 1

    try:
        proc, port = _launch_mcp()
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=10.0) as sock:
                tools_response = _rpc(sock, "tools/list", None, request_id)
                request_id += 1
                tools = tools_response.get("result", {}).get("tools")
                if not isinstance(tools, list):
                    failures.append("01: tools/list did not return a list")
                    tools = []

                if len(tools) == 0:
                    failures.append("01: tools/list returned no tools")

                parse_tool = _find_tool_by_shape(tools)
                if parse_tool is None:
                    failures.append("02: parse_peer tool not found in tools/list")
                else:
                    tool_name = str(parse_tool.get("name"))
                    argument_name = _guess_argument_name(parse_tool)

                    # Positive role behavior.
                    response = _call_parse(sock, tool_name, argument_name, "plain://peer/path", request_id)
                    request_id += 1
                    _assert_success_case(failures, "03", response, "bidirectional", ["plain://peer/path"])

                    response = _call_parse(sock, tool_name, argument_name, "+plain://peer/path", request_id)
                    request_id += 1
                    _assert_success_case(failures, "04", response, "canon", ["plain://peer/path"])

                    response = _call_parse(sock, tool_name, argument_name, "-plain://peer/path", request_id)
                    request_id += 1
                    _assert_success_case(failures, "05", response, "subordinate", ["plain://peer/path"])

                    # Fallback-group behavior: grouped URLs preserve order and include prefix at peer level only.
                    response = _call_parse(
                        sock,
                        tool_name,
                        argument_name,
                        "[alpha.txt,beta.txt,gamma.txt]",
                        request_id,
                    )
                    request_id += 1
                    _assert_success_case(failures, "06", response, "bidirectional", ["alpha.txt", "beta.txt", "gamma.txt"])

                    response = _call_parse(
                        sock,
                        tool_name,
                        argument_name,
                        "+[a.txt?mc=5,b.txt?ct=3]",
                        request_id,
                    )
                    request_id += 1
                    _assert_success_case(failures, "07", response, "canon", ["a.txt?mc=5", "b.txt?ct=3"])

                    # One URL -> one UrlText when no fallback group.
                    response = _call_parse(sock, tool_name, argument_name, "file:///tmp/path", request_id)
                    request_id += 1
                    payload = _extract_peer_payload(response.get("result")) if response.get("error") is None else None
                    if payload is not None:
                        urls = _extract_urls(payload.get("urls"))
                        if urls is not None and len(urls) != 1:
                            failures.append(f"08: non-fallback peer should contain exactly one URL, got {urls!r}")

                    # URL text is preserved; no scheme dispatch in this operation.
                    response = _call_parse(
                        sock,
                        tool_name,
                        argument_name,
                        "gopher://example.com:70/private/Resource?mc=99",
                        request_id,
                    )
                    request_id += 1
                    _assert_success_case(
                        failures,
                        "09",
                        response,
                        "bidirectional",
                        ["gopher://example.com:70/private/Resource?mc=99"],
                    )

                    # Error cases.
                    response = _call_parse(sock, tool_name, argument_name, "", request_id)
                    request_id += 1
                    _assert_error_case(failures, "10", response, "invalid_peer")

                    response = _call_parse(sock, tool_name, argument_name, "[]", request_id)
                    request_id += 1
                    _assert_error_case(failures, "11", response, "invalid_fallback_group")

                    response = _call_parse(sock, tool_name, argument_name, "[a,b", request_id)
                    request_id += 1
                    _assert_error_case(failures, "12", response, "invalid_fallback_group")

                    response = _call_parse(sock, tool_name, argument_name, "++plain://peer/path", request_id)
                    request_id += 1
                    _assert_error_case(failures, "13", response, "invalid_prefix")

                    response = _call_parse(sock, tool_name, argument_name, "[+a.txt,b.txt]", request_id)
                    request_id += 1
                    _assert_error_case(failures, "14", response, "invalid_prefix")
        finally:
            if proc is not None:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
    except Exception as exc:
        failures.append(f"00: unexpected exception: {exc}")
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

    print("All assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
