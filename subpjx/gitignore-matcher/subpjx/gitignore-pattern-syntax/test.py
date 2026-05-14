#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise gitignore-pattern-syntax public API via MCP."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Dict, Iterable, List

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = Path(os.environ.get("AITC_PROJECT", "."))


def _drain(stream) -> None:
    for _line in stream:
        pass


def _launch_mcp() -> tuple[subprocess.Popen[str], int]:
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", str(PROJECT)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )

    port = None
    deadline = time.time() + 30.0
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
    raise TimeoutError(f"timed out waiting response for request id={request_id}")


def _call_tool(sock: socket.socket, tool_name: str, arguments: Dict[str, Any], request_id: int) -> Dict[str, Any]:
    return _rpc(
        sock,
        "tools/call",
        {"name": tool_name, "arguments": arguments},
        request_id,
    )


def _as_list(value: Any, name: str) -> list[Any]:
    if not isinstance(value, list):
        raise TypeError(f"{name} is not a list")
    return value


def _find_tool_by_name(tools: List[Dict[str, Any]], names: Iterable[str]) -> Dict[str, Any] | None:
    wanted = set(names)
    for tool in tools:
        if tool.get("name") in wanted:
            return tool
    return None


def _find_tool_by_shape(
    tools: List[Dict[str, Any]],
    *,
    required_count: int | None = None,
    required_patterns: Iterable[str] | None = None,
) -> Dict[str, Any] | None:
    required_patterns = tuple(required_patterns or ())
    for tool in tools:
        schema = tool.get("inputSchema") or {}
        props = schema.get("properties", {})
        required = set(schema.get("required", []))

        if required_count is not None and len(required) != required_count:
            continue

        if required_patterns and not required_patterns <= set(required):
            continue

        good = False
        for key in required:
            prop = props.get(key, {})
            if prop.get("type") == "array":
                good = True
                break
        if good:
            return tool
    return None


def _choose_pattern_arg_field(schema: Dict[str, Any], *, mode: str) -> str:
    props = schema.get("properties", {})
    required = schema.get("required", [])

    if len(required) == 1:
        return required[0]
    for candidate in ("pattern_lines", "patternline", "patterns", "pattern-rule", "lines", "line"):
        if candidate in props:
            return candidate
    raise KeyError(f"unable to infer {mode} argument field from schema")


def _build_pattern_line(
    schema: Dict[str, Any],
    text: str,
) -> Any:
    if schema.get("type") == "string":
        return text
    if schema.get("type") == "object":
        props = schema.get("properties", {})
        required = schema.get("required") or []
        if required:
            key = required[0]
        elif len(props) == 1:
            key = next(iter(props))
        else:
            key = "text"
        return {key: text}
    return text


def _build_pattern_lines(tool_schema: Dict[str, Any], texts: List[str]) -> List[Any]:
    arg_field = _choose_pattern_arg_field(tool_schema, mode="compile")
    arg_props = tool_schema["properties"]
    item_schema = arg_props[arg_field]
    item_schema = item_schema.get("items", {})
    return [ _build_pattern_line(item_schema, text) for text in texts ]


def _build_match_input(schema: Dict[str, Any], *, path: str, is_directory: bool) -> Dict[str, Any]:
    req = schema.get("required", [])
    props = schema.get("properties", {})
    if "input" in req and isinstance(props.get("input"), dict):
        return {"path": path, "is_directory": is_directory}
    if "path" in req or "path" in props:
        return {"path": path, "is_directory": is_directory}
    # Fallback for odd wrappers that use single object field names.
    for key, value in props.items():
        if isinstance(value, dict) and set(value.get("properties", {})) >= {"path", "is_directory"}:
            return {key: {"path": path, "is_directory": is_directory}}
    raise KeyError("unable to infer match input shape from schema")


def _extract_rules(compiled: Any, schema: Dict[str, Any]) -> List[Any]:
    if isinstance(compiled, list):
        return compiled
    if not isinstance(compiled, dict):
        raise TypeError("compiled patterns result is not list or object")

    keys = ["pattern_rules", "rules", "value", "result"]
    for key in keys:
        if key in compiled and isinstance(compiled[key], list):
            return compiled[key]

    for _k, value in compiled.items():
        if isinstance(value, list):
            return value

    # Handle schema-based case where output is a single required list property.
    out_required = schema.get("required", [])
    out_props = schema.get("properties", {})
    if out_required:
        key = out_required[0]
        if isinstance(compiled.get(key), list):
            return compiled[key]

    raise TypeError("compiled patterns result has no list field")


def _compile_patterns(
    sock: socket.socket,
    tool: Dict[str, Any],
    patterns: List[str],
    request_id: int,
) -> List[Any] | str:
    schema = tool["inputSchema"]
    arg_field = _choose_pattern_arg_field(schema, mode="compile")
    arg_value = _build_pattern_lines(schema["properties"][arg_field], patterns)
    result = _call_tool(
        sock,
        tool["name"],
        {arg_field: arg_value},
        request_id,
    )
    if "error" in result:
        return str(result["error"])
    out = result.get("result")
    if out is None:
        return "compile tool returned no result"
    try:
        return _extract_rules(out, tool["outputSchema"])
    except Exception as exc:  # pragma: no cover - defensive for unexpected schema
        return f"compile result shape unsupported: {exc}"


def _match_patterns(
    sock: socket.socket,
    tool: Dict[str, Any],
    rules: List[Any],
    path: str,
    is_directory: bool,
    request_id: int,
) -> Any:
    input_schema = tool["inputSchema"]
    props = input_schema.get("properties", {})
    required = set(input_schema.get("required", []))

    args: Dict[str, Any] = {}
    if "pattern_rules" in required and "input" in required:
        args["pattern_rules"] = rules
        args["input"] = _build_match_input(props["input"], path=path, is_directory=is_directory)
    elif {"path", "is_directory"}.issubset(required):
        args["pattern_rules"] = rules
        args["path"] = path
        args["is_directory"] = is_directory
    elif "rules" in required and "input" in required:
        args["rules"] = rules
        args["input"] = _build_match_input(props["input"], path=path, is_directory=is_directory)
    elif "input" in required and any(key in required for key in ("pattern_rules", "rules")):
        key = "pattern_rules" if "pattern_rules" in required else "rules"
        args[key] = rules
        args["input"] = _build_match_input(props["input"], path=path, is_directory=is_directory)
    else:
        # Last resort: pass two known keys used by the spec if present.
        key = "rules" if "rules" in props else "pattern_rules"
        args[key] = rules
        if "input" in props and isinstance(props["input"], dict):
            args["input"] = _build_match_input(props["input"], path=path, is_directory=is_directory)
        elif {"path", "is_directory"}.issubset(required):
            args["path"] = path
            args["is_directory"] = is_directory
        elif "input" in required:
            args["input"] = {"path": path, "is_directory": is_directory}
        else:
            failures_msg = "match tool schema did not expose any usable input binding"
            return {"error": {"message": failures_msg, "code": -32000}}
    return _call_tool(sock, tool["name"], args, request_id)


def _match_result_status(resp: Dict[str, Any], tool: Dict[str, Any]) -> str | None:
    if "result" not in resp:
        return None
    result = resp["result"]
    if not isinstance(result, dict):
        return None
    if not result.get("matches", False):
        return "not-matched"
    if "ignored" in result:
        return "ignored" if result["ignored"] else "included"
    if "included" in result:
        return "included" if result["included"] else "ignored"
    if "outcome" in result and isinstance(result["outcome"], str):
        return result["outcome"]
    if "status" in result and isinstance(result["status"], str):
        return result["status"]
    # No explicit result state field: assume a positive ignored outcome.
    return "ignored"


def _is_error_payload(value: Any) -> bool:
    return isinstance(value, dict) and "error" in value


def _expect_error_message(error: Dict[str, Any], token: str) -> bool:
    message = str(error.get("message", "")).lower()
    data = error.get("data")
    if token in message:
        return True
    if isinstance(data, dict):
        for value in data.values():
            if token in str(value).lower():
                return True
    if isinstance(data, str):
        return token in data.lower()
    return False


def _assert(
    failures: List[str],
    condition: bool,
    message: str,
) -> None:
    if not condition:
        failures.append(message)


def main() -> int:
    failures: List[str] = []
    proc: subprocess.Popen[str] | None = None
    try:
        proc, port = _launch_mcp()
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
                request_id = 1

                tools_response = _rpc(sock, "tools/list", None, request_id)
                request_id += 1
                tools = tools_response.get("result", {}).get("tools")
                if not isinstance(tools, list):
                    failures.append("01 tools/list did not return a list of tools")
                    return 1

                compile_tool = _find_tool_by_name(
                    tools,
                    [
                        "compile-patterns",
                        "compile-pattern",
                        "compile-pattern-lines",
                        "compile-pattern-line",
                        "pattern-compile",
                        "build-patterns",
                    ],
                ) or _find_tool_by_shape(tools, required_count=1, required_patterns=("pattern_lines",))

                if compile_tool is None:
                    failures.append(
                        "01 unable to locate compile_patterns tool in tools/list output"
                    )
                    return 1

                match_tool = _find_tool_by_name(
                    tools,
                    [
                        "match-patterns",
                        "match-pattern",
                        "match-pattern-input",
                        "evaluate-patterns",
                        "pattern-match",
                        "match-pattern-rules",
                    ],
                ) or _find_tool_by_shape(
                    tools,
                    required_patterns=("pattern_rules",),
                )

                if match_tool is None:
                    failures.append(
                        "02 unable to locate match_patterns tool in tools/list output"
                    )
                    return 1

                # Compile-time happy path and shape checks.
                rule_payload = _compile_patterns(
                    sock,
                    compile_tool,
                    ["*.log", "!important.log", "build/", "**/temp"],
                    request_id,
                )
                request_id += 1
                _assert(failures, not isinstance(rule_payload, str), f"03 compile_patterns call failed: {rule_payload}")
                rules = rule_payload if isinstance(rule_payload, list) else []

                # Extension match behavior.
                if isinstance(rules, list):
                    ext_rules = _compile_patterns(sock, compile_tool, ["*.log"], request_id)
                    request_id += 1
                    if isinstance(ext_rules, list):
                        resp = _match_patterns(
                            sock,
                            match_tool,
                            ext_rules,
                            "logs/debug.log",
                            False,
                            request_id,
                        )
                        request_id += 1
                        if _is_error_payload(resp):
                            _assert(
                                failures,
                                False,
                                f"04 match_patterns failed unexpectedly on '*.log': {resp}",
                            )
                        else:
                            _assert(
                                failures,
                                _match_result_status(resp, match_tool) == "ignored",
                                f"04 '*.log' should produce ignored for file 'logs/debug.log', got {resp}",
                            )
                    else:
                        failures.append(f"04 compile_patterns returned invalid shape for ['*.log']: {ext_rules}")

                # Directory-only pattern behavior.
                dir_rules = _compile_patterns(
                    sock,
                    compile_tool,
                    ["build/"],
                    request_id,
                )
                request_id += 1
                if isinstance(dir_rules, list):
                    dir_match = _match_patterns(
                        sock,
                        match_tool,
                        dir_rules,
                        "build",
                        True,
                        request_id,
                    )
                    request_id += 1
                    if _is_error_payload(dir_match):
                        failures.append(f"05 directory-only match failed: {dir_match}")
                    else:
                        _assert(
                            failures,
                            _match_result_status(dir_match, match_tool) == "ignored",
                            f"05 build/ should ignore directory path 'build' when is_directory=True, got {dir_match}",
                        )

                    file_match = _match_patterns(
                        sock,
                        match_tool,
                        dir_rules,
                        "build",
                        False,
                        request_id,
                    )
                    request_id += 1
                    if _is_error_payload(file_match):
                        failures.append(f"06 directory-only non-directory match unexpectedly failed: {file_match}")
                    else:
                        _assert(
                            failures,
                            _match_result_status(file_match, match_tool) != "ignored",
                            f"06 build/ should not ignore non-directory path 'build' when is_directory=False, got {file_match}",
                        )
                else:
                    failures.append(f"05 compile_patterns returned invalid shape for ['build/']: {dir_rules}")

                # Negation and override behavior.
                neg_rules = _compile_patterns(
                    sock,
                    compile_tool,
                    ["*.log", "!important.log"],
                    request_id,
                )
                request_id += 1
                if isinstance(neg_rules, list):
                    neg_inc = _match_patterns(
                        sock,
                        match_tool,
                        neg_rules,
                        "important.log",
                        False,
                        request_id,
                    )
                    request_id += 1
                    if _is_error_payload(neg_inc):
                        failures.append(f"07 negation test failed (should be included): {neg_inc}")
                    else:
                        _assert(
                            failures,
                            _match_result_status(neg_inc, match_tool) == "included",
                            f"07 !important.log should include path 'important.log', got {neg_inc}",
                        )
                else:
                    failures.append(f"07 compile_patterns returned invalid shape for negation case: {neg_rules}")

                override_rules = _compile_patterns(
                    sock,
                    compile_tool,
                    ["*.log", "!important.log", "*.log"],
                    request_id,
                )
                request_id += 1
                if isinstance(override_rules, list):
                    override_match = _match_patterns(
                        sock,
                        match_tool,
                        override_rules,
                        "important.log",
                        False,
                        request_id,
                    )
                    request_id += 1
                    if _is_error_payload(override_match):
                        failures.append(f"08 override test failed: {override_match}")
                    else:
                        _assert(
                            failures,
                            _match_result_status(override_match, match_tool) == "ignored",
                            "08 later rules should override earlier rules for important.log, expecting ignored",
                        )

                # Recursive pattern behavior.
                rec_rules = _compile_patterns(
                    sock,
                    compile_tool,
                    ["**/temp"],
                    request_id,
                )
                request_id += 1
                if isinstance(rec_rules, list):
                    rec_match = _match_patterns(
                        sock,
                        match_tool,
                        rec_rules,
                        "a/b/c/temp",
                        False,
                        request_id,
                    )
                    request_id += 1
                    if _is_error_payload(rec_match):
                        failures.append(f"09 recursive pattern test failed: {rec_match}")
                    else:
                        _assert(
                            failures,
                            _match_result_status(rec_match, match_tool) == "ignored",
                            "09 **/temp should match path at nested depth, got non-ignored",
                        )

                # Error case: invalid pattern text returns invalid_pattern.
                invalid_pattern = _compile_patterns(
                    sock,
                    compile_tool,
                    ["[a-"],
                    request_id,
                )
                request_id += 1
                if isinstance(invalid_pattern, str):
                    # We expect an error payload text containing invalid_pattern.
                    _assert(
                        failures,
                        "invalid_pattern" in invalid_pattern.lower(),
                        f"10 invalid pattern text should fail with invalid_pattern, got {invalid_pattern}",
                    )
                else:
                    failures.append("10 expected invalid pattern text to return an error")

                # Error case: malformed path returns invalid_path.
                malformed_rules = _compile_patterns(
                    sock,
                    compile_tool,
                    ["*.log"],
                    request_id,
                )
                request_id += 1
                if isinstance(malformed_rules, list):
                    malformed_input = _match_patterns(
                        sock,
                        match_tool,
                        malformed_rules,
                        "/absolute/path.log",
                        False,
                        request_id,
                    )
                    request_id += 1
                    if _is_error_payload(malformed_input):
                        err = malformed_input.get("error", {})
                        if not _expect_error_message(err, "invalid_path"):
                            failures.append(
                                f"11 malformed path must return invalid_path, got error payload {malformed_input}"
                            )
                    else:
                        failures.append("11 malformed absolute path should return invalid_path error, but call succeeded")
                else:
                    failures.append("11 compile_patterns returned invalid shape for malformed-path setup")

        finally:
            if proc:
                proc.terminate()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
    except Exception as exc:
        failures.append(f"00 unexpected exception: {exc}")
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
