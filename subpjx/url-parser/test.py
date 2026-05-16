#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import json
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any


PROJECT_DIR = Path(__file__).resolve().parent
JAVA = PROJECT_DIR.parents[1] / "tools" / "compiler" / "jdk" / "bin" / "java"
MCP_JAR = PROJECT_DIR / "released" / "url-parser_MCP.jar"

CONTEXT = {
    "current_working_directory": "/home/ace/work",
    "current_os_user": "ace",
}


class CheckRunner:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def check(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)

    def equal(self, actual: Any, expected: Any, message: str) -> None:
        if actual != expected:
            self.failures.append(f"{message}: expected {expected!r}, got {actual!r}")


class McpClient:
    def __init__(self, port: int) -> None:
        self.port = port
        self.next_id = 1

    def rpc(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        rpc_id = self.next_id
        self.next_id += 1
        message: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
        if params is not None:
            message["params"] = params

        with socket.create_connection(("127.0.0.1", self.port), timeout=10) as sock:
            sock.settimeout(10)
            sock.sendall((json.dumps(message, separators=(",", ":")) + "\n").encode("utf-8"))
            data = b""
            while b"\n" not in data:
                chunk = sock.recv(65536)
                if not chunk:
                    break
                data += chunk

        line, _, _ = data.partition(b"\n")
        if not line:
            raise RuntimeError(f"JSON-RPC method {method} returned no response")
        return json.loads(line.decode("utf-8"))

    def call_tool(self, name: str, arguments: dict[str, Any]) -> Any:
        response = self.rpc("tools/call", {"name": name, "arguments": arguments})
        if "error" in response:
            return {"__rpc_error__": response["error"]}
        result = response.get("result")
        if isinstance(result, dict) and result.get("isError"):
            return {"__tool_error__": result}
        return unwrap_mcp_result(result)

    def shutdown(self) -> None:
        try:
            self.rpc("aitc/shutdown")
        except Exception:
            pass


def collect_stream(stream, sink: list[str]) -> None:
    try:
        for line in stream:
            sink.append(line)
    except Exception:
        pass


def launch_mcp() -> tuple[subprocess.Popen[str], int, list[str], list[str]]:
    proc = subprocess.Popen(
        [str(JAVA), "-jar", str(MCP_JAR)],
        cwd=str(PROJECT_DIR),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if proc.stdout is None or proc.stderr is None:
        proc.terminate()
        raise RuntimeError("MCP server pipes were not created")

    port = None
    startup_stdout: list[str] = []
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
        if not line:
            time.sleep(0.05)
            continue
        startup_stdout.append(line)
        if line.startswith("MCP_PORT="):
            port = int(line.strip().split("=", 1)[1])
            break
    if port is None:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        stderr_text = proc.stderr.read() if proc.stderr is not None else ""
        stdout_text = "".join(startup_stdout)
        detail = f" exit={proc.returncode}"
        if stdout_text:
            detail += f" stdout={stdout_text!r}"
        if stderr_text:
            detail += f" stderr={stderr_text!r}"
        raise RuntimeError(f"MCP server did not advertise MCP_PORT;{detail}")

    extra_stdout: list[str] = []
    stderr_lines: list[str] = []
    threading.Thread(target=collect_stream, args=(proc.stdout, extra_stdout), daemon=True).start()
    threading.Thread(target=collect_stream, args=(proc.stderr, stderr_lines), daemon=True).start()
    return proc, port, extra_stdout, stderr_lines


def shutdown_mcp(proc: subprocess.Popen[str], port: int | None) -> None:
    if port is not None:
        try:
            McpClient(port).shutdown()
        except Exception:
            pass
    try:
        proc.wait(timeout=5)
        return
    except subprocess.TimeoutExpired:
        pass
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()


def unwrap_mcp_result(result: Any) -> Any:
    if not isinstance(result, dict):
        return result
    if "structuredContent" in result:
        return result["structuredContent"]
    if "content" not in result:
        return result

    content = result["content"]
    if not isinstance(content, list) or not content:
        return result
    first = content[0]
    if not isinstance(first, dict):
        return result
    if "json" in first:
        return first["json"]
    text = first.get("text")
    if isinstance(text, str):
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            return text
    return result


def error_category(value: Any) -> str | None:
    if isinstance(value, dict):
        for key in ("category", "error_category", "code", "type"):
            item = value.get(key)
            if isinstance(item, str):
                return item
        message = value.get("message")
        if isinstance(message, str):
            return error_category(message)
        for key in ("error", "__rpc_error__", "__tool_error__"):
            category = error_category(value.get(key))
            if category:
                return category
        if "content" in value:
            return error_category(unwrap_mcp_result(value))
    if isinstance(value, str):
        for category in ERROR_CATEGORIES:
            if category in value:
                return category
    return None


def identity(value: Any) -> Any:
    if isinstance(value, dict):
        for key in ("canonical_identity", "identity", "value", "result"):
            if key in value:
                return value[key]
    return value


def parsed(value: Any) -> dict[str, Any]:
    if not isinstance(value, dict):
        return {"__non_object__": value}
    return value


def settings_of(url: dict[str, Any]) -> dict[str, Any]:
    settings = url.get("settings")
    return settings if isinstance(settings, dict) else {}


def parse_peer(client: McpClient, text: str, context: dict[str, str] | None = None) -> Any:
    return client.call_tool("parse-peer-operand", {"text": text, "context": context or CONTEXT})


def parse_url(client: McpClient, text: str, context: dict[str, str] | None = None) -> Any:
    return client.call_tool("parse-url", {"text": text, "context": context or CONTEXT})


def normalize_identity(client: McpClient, text: str, context: dict[str, str] | None = None) -> Any:
    return client.call_tool("normalize-identity", {"text": text, "context": context or CONTEXT})


ERROR_CASES = {
    "": "empty_operand",
    "http://example.com/path": "unsupported_scheme",
    "file://host/path": "invalid_file_url",
    "sftp:///path": "invalid_sftp_url",
    "sftp:relative/path": "invalid_sftp_url",
    "sftp://host:abc/path": "invalid_sftp_url",
    "sftp://host": "invalid_sftp_url",
    "sftp://host/path?mc=1&mc=2": "invalid_setting",
    "sftp://host/path?bad=1": "invalid_setting",
    "sftp://host/path?mc=0": "invalid_setting",
    "sftp://host/path?mc=abc": "invalid_setting",
    "sftp://host/%zz": "invalid_percent_encoding",
    "sftp://host/%": "invalid_percent_encoding",
    "[]": "invalid_fallback_group",
    "[sftp://host/a,,sftp://host/b]": "invalid_fallback_group",
    "[sftp://host/a,[sftp://host/b]]": "invalid_fallback_group",
    "[sftp://host/a,sftp://host/b": "invalid_fallback_group",
    "+-sftp://host/path": "invalid_role_prefix",
    "[+sftp://host/a,sftp://host/b]": "invalid_role_prefix",
}

ERROR_CATEGORIES = set(ERROR_CASES.values()) | {"invalid_context"}


def run_checks(client: McpClient, checks: CheckRunner) -> None:
    peer = parsed(parse_peer(client, "+[sftp://Host:22//photos/?mc=5&ct=60,sftp://bilbo:p%40ss@backup.example:2222/photos?ka=30]"))
    candidates = peer.get("candidates", [])
    checks.equal(peer.get("role"), "canon", "leading + applies canon role to the whole fallback group")
    checks.equal(len(candidates), 2, "fallback group returns exactly two candidates")
    if len(candidates) >= 2:
        first = parsed(candidates[0])
        second = parsed(candidates[1])
        checks.equal(first.get("scheme"), "sftp", "first fallback candidate is sftp")
        checks.equal(first.get("canonical_identity"), "sftp://ace@host/photos", "first fallback canonical identity normalizes scheme host default port path and query")
        checks.equal(first.get("user"), "ace", "missing sftp username is filled from current_os_user")
        checks.equal(first.get("host"), "host", "sftp host is lowercased")
        checks.equal(first.get("port"), 22, "default sftp port is reported as 22")
        checks.equal(first.get("path"), "/photos", "sftp path collapses repeated slashes and removes trailing slash")
        checks.equal(first.get("endpoint_key"), "ace@host:22", "endpoint key includes filled user host and normalized port")
        first_settings = settings_of(first)
        checks.equal(first_settings.get("max_connections"), 5, "mc setting parses as max_connections on declaring candidate")
        checks.equal(first_settings.get("connect_timeout_seconds"), 60, "ct setting parses as connect_timeout_seconds on declaring candidate")
        checks.check("idle_keep_alive_seconds" not in first_settings, "settings stay associated with their own fallback candidate")

        checks.equal(second.get("canonical_identity"), "sftp://bilbo@backup.example:2222/photos", "second fallback order and explicit non-default port are preserved")
        checks.equal(second.get("user"), "bilbo", "explicit sftp username is preserved")
        checks.equal(second.get("password"), "p@ss", "inline sftp password is percent-decoded")
        checks.equal(second.get("port"), 2222, "explicit sftp port is parsed")
        checks.equal(second.get("endpoint_key"), "bilbo@backup.example:2222", "endpoint key excludes path and password")
        checks.equal(settings_of(second).get("idle_keep_alive_seconds"), 30, "ka setting parses as idle_keep_alive_seconds on declaring candidate")

    bare_abs = parsed(parse_peer(client, "/var//tmp/data/"))
    abs_candidates = bare_abs.get("candidates", [])
    checks.equal(bare_abs.get("role"), "normal", "operand without prefix has normal role")
    checks.equal(len(abs_candidates), 1, "non-bracket operand returns exactly one candidate")
    if abs_candidates:
        url = parsed(abs_candidates[0])
        checks.equal(url.get("scheme"), "file", "bare POSIX absolute path parses as file")
        checks.equal(url.get("canonical_identity"), "file:///var/tmp/data", "bare absolute file identity collapses repeated slashes and trailing slash")
        checks.equal(url.get("path"), "/var/tmp/data", "bare absolute file path is normalized")

    relative = parsed(parse_peer(client, "./missing/../data/"))
    rel_candidates = relative.get("candidates", [])
    if rel_candidates:
        url = parsed(rel_candidates[0])
        checks.equal(url.get("canonical_identity"), "file:///home/ace/work/data", "relative path resolves lexically against supplied cwd without filesystem fixtures")
        checks.equal(url.get("path"), "/home/ace/work/data", "relative path field is absolute after lexical resolution")

    windows = parsed(parse_peer(client, "-c:\\photos\\raw\\"))
    win_candidates = windows.get("candidates", [])
    checks.equal(windows.get("role"), "subordinate", "leading - produces subordinate role")
    if win_candidates:
        url = parsed(win_candidates[0])
        checks.equal(url.get("canonical_identity"), "file:///c:/photos/raw", "Windows drive path identity converts separators and strips trailing slash")
        checks.equal(url.get("path"), "c:/photos/raw", "Windows drive path field converts separators")

    file_url = parsed(parse_url(client, "file:///home/ace/work//data/#frag"))
    checks.equal(file_url.get("canonical_identity"), "file:///home/ace/work/data", "file URL identity strips fragment")

    file_root = parsed(parse_url(client, "file:///"))
    checks.equal(file_root.get("canonical_identity"), "file:///", "file root identity keeps the root path slash")
    checks.equal(file_root.get("path"), "/", "file root path keeps the root slash")

    sftp_root = parsed(parse_url(client, "sftp://host/"))
    checks.equal(sftp_root.get("canonical_identity"), "sftp://ace@host/", "sftp root identity keeps the root path slash")
    checks.equal(sftp_root.get("path"), "/", "sftp root path keeps the root slash")

    sftp_fragment = identity(normalize_identity(client, "sftp://host/path#frag"))
    checks.equal(sftp_fragment, "sftp://ace@host/path", "sftp canonical identity strips fragment")

    unreserved = identity(normalize_identity(client, "sftp://User@Example.COM/%7Edocs/%41%2Fkeep"))
    checks.equal(unreserved, "sftp://User@example.com/~docs/A%2Fkeep", "canonical identity decodes unreserved percent escapes but keeps reserved escapes encoded")

    pw_one = parsed(parse_url(client, "sftp://user:one@host/path?mc=7"))
    pw_two = parsed(parse_url(client, "sftp://user:two@host/path?ct=8"))
    checks.equal(pw_one.get("canonical_identity"), pw_two.get("canonical_identity"), "passwords and query settings do not affect canonical identity")
    checks.equal(pw_one.get("password"), "one", "first password remains observable")
    checks.equal(pw_two.get("password"), "two", "second password remains observable")

    same_missing_user = identity(normalize_identity(client, "sftp://host/path"))
    same_explicit_user = identity(normalize_identity(client, "sftp://ace@host:22/path"))
    checks.equal(same_missing_user, same_explicit_user, "missing username and explicit current user with port 22 produce the same identity")

    for text, expected_category in ERROR_CASES.items():
        result = parse_peer(client, text)
        checks.equal(error_category(result), expected_category, f"invalid input {text!r} reports {expected_category}")

    invalid_user = parse_url(client, "sftp://host/path", {"current_working_directory": "/home/ace/work", "current_os_user": ""})
    checks.equal(error_category(invalid_user), "invalid_context", "empty current_os_user reports invalid_context")
    invalid_cwd = parse_url(client, "./data", {"current_working_directory": "relative/work", "current_os_user": "ace"})
    checks.equal(error_category(invalid_cwd), "invalid_context", "non-absolute current_working_directory reports invalid_context")


def main() -> int:
    checks = CheckRunner()
    proc: subprocess.Popen[str] | None = None
    port: int | None = None
    extra_stdout: list[str] = []
    stderr_lines: list[str] = []

    try:
        proc, port, extra_stdout, stderr_lines = launch_mcp()
        run_checks(McpClient(port), checks)
        time.sleep(0.2)
        checks.equal(extra_stdout, [], "public operations do not emit stdout after MCP_PORT startup")
        checks.equal(stderr_lines, [], "public operations do not emit stderr")
    except Exception as exc:
        checks.failures.append(f"test harness failed: {exc}")
    finally:
        if proc is not None:
            shutdown_mcp(proc, port)

    if checks.failures:
        print(f"FAIL: {len(checks.failures)} check(s) failed", file=sys.stderr)
        for failure in checks.failures:
            print(f"- {failure}", file=sys.stderr)
        return 1
    print("PASS: url-parser public API behavior matches SPEC.md")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
