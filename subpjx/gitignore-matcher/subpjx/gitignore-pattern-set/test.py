#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercise the gitignore pattern set through its MCP wrapper."""

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

REQUIRED_TOOLS = {"compile-pattern-set", "empty-pattern-set", "match-entry"}


class RpcClient:
    def __init__(self, sock: socket.socket) -> None:
        self.sock = sock
        self.next_id = 1
        self.buffer = b""

    def request(self, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
        rpc_id = self.next_id
        self.next_id += 1
        message: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
        if params is not None:
            message["params"] = params
        self.sock.sendall((json.dumps(message, separators=(",", ":")) + "\n").encode("utf-8"))

        deadline = time.time() + 20
        while b"\n" not in self.buffer:
            remaining = deadline - time.time()
            if remaining <= 0:
                raise TimeoutError(f"timed out waiting for {method}")
            self.sock.settimeout(remaining)
            chunk = self.sock.recv(65536)
            if not chunk:
                raise ConnectionError(f"connection closed waiting for {method}")
            self.buffer += chunk

        line, _, self.buffer = self.buffer.partition(b"\n")
        response = json.loads(line.decode("utf-8"))
        if response.get("id") != rpc_id:
            raise RuntimeError(f"response id mismatch for {method}: {response}")
        return response

    def call_tool(self, name: str, arguments: dict[str, Any]) -> dict[str, Any]:
        return self.request("tools/call", {"name": name, "arguments": arguments})


def drain(stream: Any, sink: list[str]) -> None:
    for line in stream:
        sink.append(line)


def launch_mcp() -> tuple[subprocess.Popen[str], int, list[str], list[str]]:
    stdout_lines: list[str] = []
    stderr_lines: list[str] = []
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    if proc.stdout is None or proc.stderr is None:
        proc.terminate()
        raise RuntimeError("MCP server pipes were not created")

    port = None
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
        if not line:
            continue
        stdout_lines.append(line)
        stripped = line.strip()
        if stripped.startswith("MCP_PORT="):
            port = int(stripped.split("=", 1)[1])
            break

    if port is None:
        proc.terminate()
        raise RuntimeError("MCP server did not advertise MCP_PORT")

    threading.Thread(target=drain, args=(proc.stdout, stdout_lines), daemon=True).start()
    threading.Thread(target=drain, args=(proc.stderr, stderr_lines), daemon=True).start()
    return proc, port, stdout_lines, stderr_lines


def shutdown_mcp(proc: subprocess.Popen[str], port: int) -> None:
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=3) as sock:
            RpcClient(sock).request("aitc/shutdown")
    except Exception:
        pass

    try:
        proc.wait(timeout=5)
        return
    except subprocess.TimeoutExpired:
        proc.terminate()

    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait(timeout=5)


def schema_props(tool: dict[str, Any]) -> dict[str, Any]:
    schema = tool.get("inputSchema")
    if not isinstance(schema, dict):
        return {}
    props = schema.get("properties")
    return props if isinstance(props, dict) else {}


def source(pattern_text: str, source_name: str | None = "spec") -> dict[str, Any]:
    value: dict[str, Any] = {"pattern_text": pattern_text}
    if source_name is not None:
        value["source_name"] = source_name
    return value


def entry(relative_path: str, kind: str = "regular_file") -> dict[str, str]:
    return {"relative_path": relative_path, "kind": kind}


def assert_equal(failures: list[str], label: str, actual: Any, expected: Any) -> None:
    if actual != expected:
        failures.append(f"{label}: expected {expected!r}, got {actual!r}")


def expect_success(failures: list[str], label: str, response: dict[str, Any]) -> dict[str, Any]:
    if "error" in response:
        failures.append(f"{label}: expected success, got {response['error']}")
        return {}
    result = response.get("result")
    if not isinstance(result, dict):
        failures.append(f"{label}: expected object result, got {response}")
        return {}
    return result


def error_text(response: dict[str, Any]) -> str:
    error = response.get("error")
    if not isinstance(error, dict):
        return ""
    parts: list[str] = []
    if isinstance(error.get("message"), str):
        parts.append(error["message"])
    data = error.get("data")
    if isinstance(data, dict):
        for key, value in data.items():
            if isinstance(value, str):
                parts.append(f"{key}={value}")
    elif isinstance(data, str):
        parts.append(data)
    return " ".join(parts)


def expect_error_category(failures: list[str], label: str, response: dict[str, Any], category: str) -> None:
    if not isinstance(response.get("error"), dict):
        failures.append(f"{label}: expected {category} error, got {response}")
        return
    if category not in error_text(response):
        failures.append(f"{label}: expected {category} in error message or data, got {response['error']}")
    if "result" in response:
        failures.append(f"{label}: error response must not include a partial result, got {response}")


def result_id(result: dict[str, Any]) -> str | None:
    for key in ("pattern_set_id", "patternSetId", "set_id", "setId", "matcher_id", "matcherId", "id"):
        value = result.get(key)
        if isinstance(value, str) and value:
            return value
    return None


def require_tools(failures: list[str], tools_result: dict[str, Any]) -> dict[str, dict[str, Any]]:
    tools = tools_result.get("tools")
    if not isinstance(tools, list):
        failures.append("01: tools/list result must contain a tools array")
        return {}

    by_name: dict[str, dict[str, Any]] = {}
    names: list[str] = []
    for tool in tools:
        if not isinstance(tool, dict):
            failures.append(f"01: tool entry must be an object, got {tool!r}")
            continue
        name = tool.get("name")
        if not isinstance(name, str):
            failures.append(f"01: tool name must be a string, got {tool!r}")
            continue
        names.append(name)
        by_name[name] = tool

    missing = sorted(REQUIRED_TOOLS - set(names))
    if missing:
        failures.append(f"01: missing required public API tools: {', '.join(missing)}")
    return by_name


def compile_args(tool: dict[str, Any], pattern_text: str, source_name: str | None = "spec") -> dict[str, Any]:
    src = source(pattern_text, source_name)
    props = schema_props(tool)
    if "input" in props and "source" not in props and "pattern_text" not in props:
        return {"input": {"source": src}}
    if "pattern_text" in props:
        return src
    return {"source": src}


def match_args(tool: dict[str, Any], pattern_set_id: str, path_entry: dict[str, str]) -> dict[str, Any]:
    props = schema_props(tool)
    id_key = "pattern_set_id"
    for candidate in ("pattern_set_id", "patternSetId", "set_id", "setId", "matcher_id", "matcherId", "id"):
        if candidate in props:
            id_key = candidate
            break
    payload = {id_key: pattern_set_id, "entry": path_entry}
    if "input" in props and id_key not in props and "entry" not in props:
        return {"input": payload}
    return payload


def compile_pattern_set(
    rpc: RpcClient,
    tools: dict[str, dict[str, Any]],
    failures: list[str],
    label: str,
    pattern_text: str,
    source_name: str | None = "spec",
) -> str | None:
    response = rpc.call_tool("compile-pattern-set", compile_args(tools.get("compile-pattern-set", {}), pattern_text, source_name))
    result = expect_success(failures, label, response)
    pattern_set_id = result_id(result)
    if pattern_set_id is None:
        failures.append(f"{label}: result must include a pattern set id string, got {result}")
    return pattern_set_id


def empty_pattern_set(
    rpc: RpcClient,
    failures: list[str],
    label: str,
) -> str | None:
    result = expect_success(failures, label, rpc.call_tool("empty-pattern-set", {}))
    pattern_set_id = result_id(result)
    if pattern_set_id is None:
        failures.append(f"{label}: result must include a pattern set id string, got {result}")
    return pattern_set_id


def match_entry(
    rpc: RpcClient,
    tools: dict[str, dict[str, Any]],
    failures: list[str],
    label: str,
    pattern_set_id: str | None,
    path_entry: dict[str, str],
) -> dict[str, Any]:
    if pattern_set_id is None:
        failures.append(f"{label}: cannot match because pattern set id was missing")
        return {}
    result = expect_success(
        failures,
        label,
        rpc.call_tool("match-entry", match_args(tools.get("match-entry", {}), pattern_set_id, path_entry)),
    )
    nested = result.get("match")
    return nested if isinstance(nested, dict) else result


def assert_match(
    failures: list[str],
    label: str,
    actual: dict[str, Any],
    *,
    decision: str,
    negated: bool,
    pattern: str | None = None,
    source_name: str | None = None,
    line_number: int | None = None,
) -> None:
    assert_equal(failures, f"{label} decision", actual.get("decision"), decision)
    assert_equal(failures, f"{label} negated", actual.get("negated"), negated)
    if decision == "none":
        for key in ("pattern", "source_name", "sourceName", "line_number", "lineNumber"):
            if key in actual and actual.get(key) is not None:
                failures.append(f"{label}: decision=none must not include {key}, got {actual}")
        return
    if pattern is not None:
        assert_equal(failures, f"{label} pattern", actual.get("pattern"), pattern)
    if source_name is not None:
        actual_source = actual.get("source_name", actual.get("sourceName"))
        assert_equal(failures, f"{label} source_name", actual_source, source_name)
    if line_number is not None:
        actual_line = actual.get("line_number", actual.get("lineNumber"))
        assert_equal(failures, f"{label} line_number", actual_line, line_number)


def run_concurrent_matches(
    port: int,
    tools: dict[str, dict[str, Any]],
    pattern_set_id: str | None,
    failures: list[str],
) -> None:
    if pattern_set_id is None:
        failures.append("08 concurrent match: pattern set id was missing")
        return

    cases = [
        (entry("app.log"), "ignore", False, "*.log"),
        (entry("important.log"), "include", True, "!important.log"),
        (entry("src/main.txt"), "none", False, None),
        (entry("src/build/out.bin", "special"), "ignore", False, "build/"),
    ]
    thread_failures: list[str] = []
    lock = threading.Lock()

    def worker(index: int) -> None:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
                rpc = RpcClient(sock)
                local_failures: list[str] = []
                for iteration in range(25):
                    path_entry, decision, negated, pattern = cases[(index + iteration) % len(cases)]
                    actual = match_entry(
                        rpc,
                        tools,
                        local_failures,
                        f"08 thread {index} iter {iteration}",
                        pattern_set_id,
                        path_entry,
                    )
                    if actual.get("decision") != decision or actual.get("negated") != negated:
                        local_failures.append(
                            "08 concurrent match: "
                            f"thread {index} iter {iteration} expected "
                            f"decision={decision} negated={negated}, got {actual}"
                        )
                    if pattern is not None and actual.get("pattern") != pattern:
                        local_failures.append(
                            f"08 concurrent match: thread {index} iter {iteration} expected pattern {pattern!r}, got {actual}"
                        )
                with lock:
                    thread_failures.extend(local_failures)
        except Exception as exc:
            with lock:
                thread_failures.append(f"08 concurrent match: thread {index} raised {exc!r}")

    threads = [threading.Thread(target=worker, args=(index,)) for index in range(8)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join(timeout=15)
        if thread.is_alive():
            failures.append("08 concurrent match: worker thread did not finish")

    failures.extend(thread_failures)


def main() -> int:
    proc: subprocess.Popen[str] | None = None
    port = 0
    try:
        proc, port, stdout_lines, stderr_lines = launch_mcp()
        failures: list[str] = []
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            rpc = RpcClient(sock)

            time.sleep(0.1)
            stdout_before = len(stdout_lines)
            stderr_before = len(stderr_lines)

            tools_result = expect_success(failures, "01 tools/list", rpc.request("tools/list"))
            tools = require_tools(failures, tools_result)
            print(f"[01] tools/list exposed {len(tools_result.get('tools', []))} tool(s)")

            pattern_text = "\n".join(
                [
                    "# comment",
                    "",
                    r"\#literal",
                    r"\!literal",
                    r"name\ ",
                    "plain-space   ",
                    "*.log",
                    "!important.log",
                    "src/?ain.[ch]",
                    "[ab]rack.txt",
                    "[!0-9]ode.txt",
                    "/root-only.txt",
                    "docs/*.md",
                    "build/",
                    "**/tmp/**",
                    "assets/**",
                    "a/**/b.txt",
                    "foo***bar",
                    "range-[a-z].txt",
                    "*.LOG",
                    "ordered.txt",
                    "!ordered.txt",
                    "ordered.*",
                    "mid#hash",
                    "zero*.txt",
                    "br/[ab]tail.txt",
                ]
            )
            pattern_set = compile_pattern_set(rpc, tools, failures, "02 compile pattern set", pattern_text, "spec")
            pattern_cases = [
                ("02 escaped #", entry("#literal"), "ignore", False, r"\#literal", 3),
                ("02 escaped !", entry("!literal"), "ignore", False, r"\!literal", 4),
                ("02 escaped trailing space", entry("name "), "ignore", False, r"name\ ", 5),
                ("02 unescaped trailing spaces", entry("plain-space"), "ignore", False, None, 6),
                ("02 basename star root", entry("app.log"), "ignore", False, "*.log", 7),
                ("02 basename star nested", entry("logs/app.log"), "ignore", False, "*.log", 7),
                ("02 negation overrides", entry("important.log"), "include", True, "!important.log", 8),
                ("02 question and bracket", entry("src/main.c"), "ignore", False, "src/?ain.[ch]", 9),
                ("02 bracket set", entry("brack.txt"), "ignore", False, "[ab]rack.txt", 10),
                ("02 bracket negation", entry("code.txt"), "ignore", False, "[!0-9]ode.txt", 11),
                ("02 leading slash anchors", entry("root-only.txt"), "ignore", False, "/root-only.txt", 12),
                ("02 interior slash", entry("docs/readme.md"), "ignore", False, "docs/*.md", 13),
                ("02 directory pattern directory", entry("src/build", "directory"), "ignore", False, "build/", 14),
                ("02 directory pattern descendant file", entry("src/build/out.bin"), "ignore", False, "build/", 14),
                ("02 directory pattern descendant symlink", entry("build/link", "symlink"), "ignore", False, "build/", 14),
                ("02 start and end double-star root", entry("tmp/cache.bin"), "ignore", False, "**/tmp/**", 15),
                ("02 start and end double-star nested", entry("src/tmp/cache.bin"), "ignore", False, "**/tmp/**", 15),
                ("02 end double-star", entry("assets/icons/logo.png"), "ignore", False, "assets/**", 16),
                ("02 middle double-star shallow", entry("a/b.txt"), "ignore", False, "a/**/b.txt", 17),
                ("02 middle double-star deep", entry("a/x/y/b.txt"), "ignore", False, "a/**/b.txt", 17),
                ("02 consecutive stars", entry("foobazbar"), "ignore", False, "foo***bar", 18),
                ("02 bracket range", entry("range-m.txt"), "ignore", False, "range-[a-z].txt", 19),
                ("02 case-sensitive uppercase pattern", entry("app.LOG"), "ignore", False, "*.LOG", 20),
                ("02 later matching pattern wins", entry("ordered.txt"), "ignore", False, "ordered.*", 23),
                ("02 interior hash literal", entry("mid#hash"), "ignore", False, "mid#hash", 24),
                ("02 star matches zero characters", entry("zero.txt"), "ignore", False, "zero*.txt", 25),
                ("02 bracket matches one character", entry("br/atail.txt"), "ignore", False, "br/[ab]tail.txt", 26),
            ]
            for label, path_entry, decision, negated, pattern, line_number in pattern_cases:
                assert_match(
                    failures,
                    label,
                    match_entry(rpc, tools, failures, label, pattern_set, path_entry),
                    decision=decision,
                    negated=negated,
                    pattern=pattern,
                    source_name="spec",
                    line_number=line_number,
                )
            none_cases = [
                ("02 comments ignored", entry("# comment")),
                ("02 blank lines ignored", entry("blank")),
                ("02 escaped trailing space does not match trimmed", entry("name")),
                ("02 anchored pattern excludes nested path", entry("sub/root-only.txt")),
                ("02 slash pattern is root-relative", entry("src/docs/readme.md")),
                ("02 interior slash excludes deeper path", entry("docs/nested/readme.md")),
                ("02 question mark requires one character", entry("src/ain.c")),
                ("02 question mark does not match slash", entry("src/x/ain.c")),
                ("02 bracket expression does not match slash", entry("br/a/tail.txt")),
                ("02 directory-only does not match file at same path", entry("build")),
                ("02 symlink has no directory behavior at same path", entry("build", "symlink")),
                ("02 special has no directory behavior at same path", entry("build", "special")),
                ("02 matching is case-sensitive", entry("app.Log")),
            ]
            for label, path_entry in none_cases:
                assert_match(
                    failures,
                    label,
                    match_entry(rpc, tools, failures, label, pattern_set, path_entry),
                    decision="none",
                    negated=False,
                )
            print("[02] pattern parsing, precedence, metadata, and wildcard semantics exercised")

            empty_set = empty_pattern_set(rpc, failures, "03 empty pattern set")
            for label, path_entry in [
                ("03 empty regular file", entry("app.log")),
                ("03 empty directory", entry("build", "directory")),
                ("03 empty symlink", entry("link", "symlink")),
                ("03 empty special", entry("device", "special")),
            ]:
                assert_match(
                    failures,
                    label,
                    match_entry(rpc, tools, failures, label, empty_set, path_entry),
                    decision="none",
                    negated=False,
                )
            no_source_set = compile_pattern_set(rpc, tools, failures, "03 compile without source name", "*.txt\n", None)
            no_source_match = match_entry(rpc, tools, failures, "03 match without source name", no_source_set, entry("note.txt"))
            assert_match(
                failures,
                "03 source name absent when omitted",
                no_source_match,
                decision="ignore",
                negated=False,
                pattern="*.txt",
                line_number=1,
            )
            for key in ("source_name", "sourceName"):
                if key in no_source_match and no_source_match.get(key) is not None:
                    failures.append(f"03 source name absent when omitted: {key} must be absent, got {no_source_match}")
            print("[03] empty pattern set returns none for valid paths")

            dir_only = compile_pattern_set(rpc, tools, failures, "04 compile directory-only set", "build/\n", "dir")
            for label, path_entry, decision in [
                ("04 directory path matches directory", entry("build", "directory"), "ignore"),
                ("04 nested directory path matches directory", entry("src/build", "directory"), "ignore"),
                ("04 descendant regular file matches", entry("build/out.bin"), "ignore"),
                ("04 descendant symlink matches", entry("src/build/link", "symlink"), "ignore"),
                ("04 descendant special matches", entry("src/build/socket", "special"), "ignore"),
                ("04 regular file same path does not match", entry("build", "regular_file"), "none"),
                ("04 symlink same path does not match", entry("build", "symlink"), "none"),
                ("04 special same path does not match", entry("build", "special"), "none"),
            ]:
                assert_match(
                    failures,
                    label,
                    match_entry(rpc, tools, failures, label, dir_only, path_entry),
                    decision=decision,
                    negated=False,
                    pattern="build/" if decision == "ignore" else None,
                    source_name="dir" if decision == "ignore" else None,
                    line_number=1 if decision == "ignore" else None,
                )
            print("[04] directory-only patterns distinguish matched directories from non-directories")

            literal_brackets = compile_pattern_set(rpc, tools, failures, "05 compile malformed bracket set", "[abc\nfile[.txt\n", "bad-bracket")
            for label, path_entry, pattern, line_number in [
                ("05 malformed bracket literal 1", entry("[abc"), "[abc", 1),
                ("05 malformed bracket literal 2", entry("file[.txt"), "file[.txt", 2),
            ]:
                assert_match(
                    failures,
                    label,
                    match_entry(rpc, tools, failures, label, literal_brackets, path_entry),
                    decision="ignore",
                    negated=False,
                    pattern=pattern,
                    source_name="bad-bracket",
                    line_number=line_number,
                )
            print("[05] malformed bracket expressions are literal patterns")

            no_builtin = compile_pattern_set(rpc, tools, failures, "06 compile syntax-only set", "*.tmp\n", "plain")
            for label, path_entry, decision, pattern in [
                ("06 symlink matched by path syntax", entry("link.tmp", "symlink"), "ignore", "*.tmp"),
                ("06 special matched by path syntax", entry("socket.tmp", "special"), "ignore", "*.tmp"),
                ("06 symlink has no automatic exclusion", entry("link", "symlink"), "none", None),
                ("06 special has no automatic exclusion", entry("socket", "special"), "none", None),
            ]:
                assert_match(
                    failures,
                    label,
                    match_entry(rpc, tools, failures, label, no_builtin, path_entry),
                    decision=decision,
                    negated=False,
                    pattern=pattern,
                    source_name="plain" if pattern else None,
                    line_number=1 if pattern else None,
                )
            print("[06] symlink and special entries are path-syntax matches only")

            invalid_path_set = compile_pattern_set(rpc, tools, failures, "07 compile invalid-path set", "*.log\n", "invalid")
            invalid_paths = [
                ("07 empty path", entry("")),
                ("07 leading slash", entry("/abs")),
                ("07 trailing slash", entry("dir/")),
                ("07 empty segment", entry("a//b")),
                ("07 dot segment", entry("a/./b")),
                ("07 dotdot segment", entry("a/../b")),
                ("07 backslash", entry(r"a\b")),
                ("07 nul path", entry("a\0b")),
            ]
            for label, path_entry in invalid_paths:
                response = rpc.call_tool(
                    "match-entry",
                    match_args(tools.get("match-entry", {}), invalid_path_set or "missing", path_entry),
                )
                expect_error_category(failures, label, response, "invalid_path")

            nul_response = rpc.call_tool(
                "compile-pattern-set",
                compile_args(tools.get("compile-pattern-set", {}), "ok\nbad\0pattern\n", "nul"),
            )
            expect_error_category(failures, "07 nul-containing pattern text", nul_response, "invalid_pattern_text")
            # Text that the host language API cannot represent is not reasonably testable through JSON-RPC.
            print("[07] invalid paths and NUL pattern text report required error categories")

            run_concurrent_matches(port, tools, pattern_set, failures)
            before = match_entry(rpc, tools, failures, "08 immutable before extra compile", pattern_set, entry("important.log"))
            other = compile_pattern_set(rpc, tools, failures, "08 compile independent set", "!important.log\n*.log\n", "other")
            after = match_entry(rpc, tools, failures, "08 immutable after extra compile", pattern_set, entry("important.log"))
            changed = match_entry(rpc, tools, failures, "08 independent set has own order", other, entry("important.log"))
            assert_match(failures, "08 original before", before, decision="include", negated=True, pattern="!important.log", source_name="spec", line_number=8)
            assert_match(failures, "08 original after", after, decision="include", negated=True, pattern="!important.log", source_name="spec", line_number=8)
            assert_match(failures, "08 independent order", changed, decision="ignore", negated=False, pattern="*.log", source_name="other", line_number=2)
            # Cross-operating-system determinism is not reasonably testable from one test process.
            print("[08] compiled pattern sets are immutable, deterministic, and concurrent-match safe")

            time.sleep(0.1)
            if len(stdout_lines) != stdout_before:
                failures.append(f"09 public operations wrote to stdout: {stdout_lines[stdout_before:]!r}")
            if len(stderr_lines) != stderr_before:
                failures.append(f"09 public operations wrote to stderr: {stderr_lines[stderr_before:]!r}")
            print("[09] public operations did not emit stdout or stderr")

            if failures:
                print("\nFAILURES:")
                for failure in failures:
                    print(f"  - {failure}")
                return 1

            print("\nAll assertions passed.")
            return 0
    finally:
        if proc is not None:
            shutdown_mcp(proc, port)


if __name__ == "__main__":
    sys.exit(main())
