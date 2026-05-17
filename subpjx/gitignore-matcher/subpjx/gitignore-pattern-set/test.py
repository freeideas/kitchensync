#!/usr/bin/env uvrun
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
from itertools import count
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
MCP_JAR = Path(
    "C:/Users/human/Desktop/prjx/kitchensync/subpjx/gitignore-matcher"
    "/subpjx/gitignore-pattern-set/released/gitignore-pattern-set_MCP.jar"
)

failures: list[str] = []
_rpc_id = count(1)


def fail(msg: str) -> None:
    failures.append(msg)
    print(f"FAIL: {msg}")


def ok(msg: str) -> None:
    print(f"OK:   {msg}")


def drain(stream, sink=None):
    for line in stream:
        if sink is not None:
            sink.append(line)


def launch_mcp() -> tuple[subprocess.Popen, int]:
    proc = subprocess.Popen(
        [str(JAVA), "-jar", str(MCP_JAR)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    stderr_buf: list[str] = []
    threading.Thread(target=drain, args=(proc.stderr, stderr_buf), daemon=True).start()

    stdout_buf: list[str] = []
    port = None
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
        stdout_buf.append(line)
        if line.startswith("MCP_PORT="):
            port = int(line.strip().split("=", 1)[1])
            break

    if port is None:
        proc.terminate()
        try:
            proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            proc.kill()
        raise RuntimeError(
            "MCP server did not advertise MCP_PORT\n"
            f"stdout: {''.join(stdout_buf)}\nstderr: {''.join(stderr_buf)}"
        )

    threading.Thread(target=drain, args=(proc.stdout,), daemon=True).start()
    return proc, port


def rpc_one(port: int, method: str, params=None) -> dict:
    """Open a fresh connection, send one JSON-RPC request, read one response, close."""
    msg: dict = {"jsonrpc": "2.0", "id": next(_rpc_id), "method": method}
    if params is not None:
        msg["params"] = params
    payload = (json.dumps(msg, separators=(",", ":")) + "\n").encode("utf-8")
    with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
        sock.sendall(payload)
        data = b""
        deadline = time.time() + 10
        while b"\n" not in data and time.time() < deadline:
            chunk = sock.recv(65536)
            if not chunk:
                break
            data += chunk
    line, _, _ = data.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def shutdown_mcp(proc: subprocess.Popen, port: int) -> None:
    try:
        rpc_one(port, "aitc/shutdown")
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


def call_tool(port: int, name: str, arguments: dict) -> dict:
    return rpc_one(port, "tools/call", {"name": name, "arguments": arguments})


def parse_result(resp: dict) -> tuple[dict | None, str | None]:
    """Return (data, error_category). error_category is None on success."""
    if "error" in resp:
        return None, f"rpc_error:{resp['error'].get('message', str(resp['error']))}"
    result = resp.get("result", {})
    content = result.get("content", [])
    if not content:
        return result, None
    try:
        data = json.loads(content[0].get("text", "{}"))
    except json.JSONDecodeError:
        data = {"_raw": content[0].get("text", "")}
    if result.get("isError"):
        return data, data.get("error", "tool_error")
    if isinstance(data, dict) and "error" in data:
        return data, data["error"]
    return data, None


def compile_set(port: int, pattern_text: str, source_name: str | None = None) -> tuple[dict | None, str | None]:
    args: dict = {"pattern_text": pattern_text}
    if source_name is not None:
        args["source_name"] = source_name
    return parse_result(call_tool(port, "compile", args))


def empty_set(port: int) -> tuple[dict | None, str | None]:
    return parse_result(call_tool(port, "empty", {}))


def match_entry(port: int, set_id: str, relative_path: str, kind: str) -> tuple[dict | None, str | None]:
    return parse_result(call_tool(port, "match", {
        "id": set_id,
        "relative_path": relative_path,
        "kind": kind,
    }))


def get_id(data: dict | None, label: str) -> str | None:
    if data is None:
        fail(f"{label}: result is None")
        return None
    sid = data.get("id")
    if sid is None:
        fail(f"{label}: no 'id' in result {data!r}")
    return sid


def check_match(
    port: int,
    set_id: str,
    path: str,
    kind: str,
    expected_decision: str,
    label: str,
    *,
    expected_pattern: str | None = None,
    expected_negated: bool | None = None,
    expected_line: int | None = None,
    expected_source: str | None = None,
) -> None:
    data, err = match_entry(port, set_id, path, kind)
    if err:
        fail(f"{label}: unexpected error {err!r}")
        return
    decision = data.get("decision") if data else None
    if decision != expected_decision:
        fail(f"{label}: expected decision={expected_decision!r}, got {decision!r} data={data!r}")
        return
    ok(f"{label}: decision={decision!r}")
    if data is None:
        return
    negated = data.get("negated", False)
    expected_neg = expected_negated if expected_negated is not None else (expected_decision == "include")
    if negated != expected_neg:
        fail(f"{label}: expected negated={expected_neg}, got {negated!r}")
    if expected_pattern is not None and data.get("pattern") != expected_pattern:
        fail(f"{label}: expected pattern={expected_pattern!r}, got {data.get('pattern')!r}")
    if expected_line is not None and data.get("line_number") != expected_line:
        fail(f"{label}: expected line_number={expected_line}, got {data.get('line_number')!r}")
    if expected_source is not None and data.get("source_name") != expected_source:
        fail(f"{label}: expected source_name={expected_source!r}, got {data.get('source_name')!r}")
    if expected_decision == "none":
        if data.get("negated", False):
            fail(f"{label}: negated must be False for none decision")
        for field in ("pattern", "line_number", "source_name"):
            if data.get(field) is not None:
                fail(f"{label}: {field} must be absent for none decision, got {data[field]!r}")


def run_tests(port: int) -> None:

    # --- empty(): every valid path returns none ---
    data, err = empty_set(port)
    if err:
        fail(f"empty(): error {err!r}")
    else:
        sid = get_id(data, "empty()")
        if sid:
            check_match(port, sid, "anything.txt", "regular_file", "none", "empty: regular_file")
            check_match(port, sid, "a/b/c", "directory", "none", "empty: directory")

    # --- basic patterns (spec example): *.log, build/, !important.log ---
    # Verifies: basename wildcard, trailing-slash dir pattern, negation,
    # source_name and line_number in result, negated flag, pattern field.
    data, err = compile_set(port, "*.log\nbuild/\n!important.log\n", source_name="basic")
    if err:
        fail(f"compile basic: error {err!r}")
    else:
        sid = get_id(data, "compile basic")
        if sid:
            check_match(port, sid, "app.log", "regular_file", "ignore", "basic: app.log",
                        expected_pattern="*.log", expected_line=1, expected_negated=False,
                        expected_source="basic")
            check_match(port, sid, "important.log", "regular_file", "include",
                        "basic: important.log negation",
                        expected_pattern="!important.log", expected_line=3, expected_negated=True,
                        expected_source="basic")
            check_match(port, sid, "src/build", "directory", "ignore", "basic: src/build dir",
                        expected_pattern="build/", expected_line=2)
            check_match(port, sid, "src/build/out.bin", "regular_file", "ignore",
                        "basic: descendant of src/build",
                        expected_pattern="build/", expected_line=2)
            check_match(port, sid, "src/main.txt", "regular_file", "none", "basic: unmatched path")
            # pattern without / matches basename at any depth
            check_match(port, sid, "sub/dir/app.log", "regular_file", "ignore",
                        "basic: *.log matches basename at any depth",
                        expected_pattern="*.log", expected_line=1, expected_source="basic")

    # --- parsing: blank lines, comments, escaped #/!, escaped trailing space, line_number ---
    # line 1: blank, line 2: comment, line 3: \#literal, line 4: \!literal, line 5: name\<space>
    data, err = compile_set(port, "\n# comment\n\\#literal\n\\!literal\nname\\ \n")
    if err:
        fail(f"compile parse: error {err!r}")
    else:
        sid = get_id(data, "compile parse")
        if sid:
            check_match(port, sid, "#literal", "regular_file", "ignore",
                        "parse: escaped # matches literal",
                        expected_pattern="\\#literal", expected_line=3)
            check_match(port, sid, "!literal", "regular_file", "ignore",
                        "parse: escaped ! matches literal",
                        expected_pattern="\\!literal", expected_line=4)
            check_match(port, sid, "name ", "regular_file", "ignore",
                        "parse: escaped trailing space matches",
                        expected_pattern="name\\ ", expected_line=5)
            check_match(port, sid, "name", "regular_file", "none",
                        "parse: name without trailing space no match")

    # --- anchoring and double-star (spec example) ---
    data, err = compile_set(port, "/docs/*.md\n**/tmp/**\n")
    if err:
        fail(f"compile anchor: error {err!r}")
    else:
        sid = get_id(data, "compile anchor")
        if sid:
            check_match(port, sid, "docs/readme.md", "regular_file", "ignore",
                        "anchor: /docs/*.md matches root",
                        expected_pattern="/docs/*.md", expected_line=1)
            check_match(port, sid, "src/docs/readme.md", "regular_file", "none",
                        "anchor: leading / anchors to root only")
            check_match(port, sid, "tmp/cache.bin", "regular_file", "ignore",
                        "anchor: **/tmp/** at root level",
                        expected_pattern="**/tmp/**", expected_line=2)
            check_match(port, sid, "src/tmp/cache.bin", "regular_file", "ignore",
                        "anchor: **/tmp/** nested",
                        expected_pattern="**/tmp/**", expected_line=2)

    # --- ? wildcard ---
    data, err = compile_set(port, "?.txt\n")
    if err:
        fail(f"compile ?: error {err!r}")
    else:
        sid = get_id(data, "compile ?")
        if sid:
            check_match(port, sid, "a.txt", "regular_file", "ignore", "?: single char matches")
            check_match(port, sid, "ab.txt", "regular_file", "none", "?: two chars no match")
            check_match(port, sid, ".txt", "regular_file", "none", "?: zero chars no match")

    # --- bracket expressions: [abc], [0-9], [!0-9] ---
    data, err = compile_set(port, "[abc].txt\n[0-9].log\n[!0-9].cfg\n")
    if err:
        fail(f"compile bracket: error {err!r}")
    else:
        sid = get_id(data, "compile bracket")
        if sid:
            check_match(port, sid, "a.txt", "regular_file", "ignore", "bracket [abc]: a")
            check_match(port, sid, "c.txt", "regular_file", "ignore", "bracket [abc]: c")
            check_match(port, sid, "d.txt", "regular_file", "none", "bracket [abc]: d no match")
            check_match(port, sid, "5.log", "regular_file", "ignore", "bracket [0-9]: digit")
            check_match(port, sid, "a.log", "regular_file", "none", "bracket [0-9]: letter no match")
            check_match(port, sid, "a.cfg", "regular_file", "ignore", "bracket [!0-9]: non-digit")
            check_match(port, sid, "5.cfg", "regular_file", "none", "bracket [!0-9]: digit no match")

    # --- pattern order: last matching pattern wins; negation overrides earlier ignore ---
    data, err = compile_set(port, "*.txt\n!a.txt\n")
    if err:
        fail(f"compile order: error {err!r}")
    else:
        sid = get_id(data, "compile order")
        if sid:
            check_match(port, sid, "a.txt", "regular_file", "include",
                        "order: negation overrides earlier ignore",
                        expected_negated=True, expected_line=2)
            check_match(port, sid, "b.txt", "regular_file", "ignore",
                        "order: non-negated still applies", expected_line=1)

    # --- directory-only patterns: dir and descendants matched; non-dir at same path is NOT matched ---
    data, err = compile_set(port, "build/\n")
    if err:
        fail(f"compile dir-only: error {err!r}")
    else:
        sid = get_id(data, "compile dir-only")
        if sid:
            check_match(port, sid, "build", "directory", "ignore",
                        "dir-only: directory at path", expected_pattern="build/")
            check_match(port, sid, "build", "regular_file", "none",
                        "dir-only: regular_file at same path NOT matched")
            check_match(port, sid, "build", "symlink", "none",
                        "dir-only: symlink at same path NOT matched")
            check_match(port, sid, "build/out.bin", "regular_file", "ignore",
                        "dir-only: descendant regular_file", expected_pattern="build/")
            check_match(port, sid, "build/sub", "directory", "ignore",
                        "dir-only: descendant directory", expected_pattern="build/")
            # basename pattern (no interior slash) matches at any depth
            check_match(port, sid, "src/build", "directory", "ignore",
                        "dir-only: nested dir any depth")
            check_match(port, sid, "src/build/out.bin", "regular_file", "ignore",
                        "dir-only: nested descendant")

    # --- symlink and special entries have no built-in exclusion -- matched by path syntax ---
    data, err = compile_set(port, "*.so\n")
    if err:
        fail(f"compile symlink/special: error {err!r}")
    else:
        sid = get_id(data, "compile symlink/special")
        if sid:
            check_match(port, sid, "lib.so", "symlink", "ignore",
                        "symlink: matched by path syntax")
            check_match(port, sid, "dev.so", "special", "ignore",
                        "special: matched by path syntax")
            check_match(port, sid, "lib.txt", "symlink", "none",
                        "symlink: no match on unmatched path")

    # --- malformed bracket expression treated as literal text ---
    data, err = compile_set(port, "[abc\n")
    if err:
        fail(f"compile malformed bracket: error {err!r}")
    else:
        sid = get_id(data, "compile malformed bracket")
        if sid:
            check_match(port, sid, "[abc", "regular_file", "ignore",
                        "malformed bracket: literal match")
            check_match(port, sid, "a", "regular_file", "none",
                        "malformed bracket: no fuzzy match")

    # --- case-sensitive matching ---
    data, err = compile_set(port, "UPPER.LOG\n")
    if err:
        fail(f"compile case: error {err!r}")
    else:
        sid = get_id(data, "compile case")
        if sid:
            check_match(port, sid, "UPPER.LOG", "regular_file", "ignore",
                        "case: exact case matches")
            check_match(port, sid, "upper.log", "regular_file", "none",
                        "case: different case no match")

    # --- source_name absent from result when not supplied to compile ---
    data, err = compile_set(port, "*.txt\n")
    if err:
        fail(f"compile no-source: error {err!r}")
    else:
        sid = get_id(data, "compile no-source")
        if sid:
            d, e = match_entry(port, sid, "a.txt", "regular_file")
            if e:
                fail(f"no-source match: error {e!r}")
            elif d and d.get("decision") == "ignore":
                ok("no-source: decision=ignore")
                if d.get("source_name") is not None:
                    fail(f"no-source: source_name should be absent, got {d.get('source_name')!r}")
                else:
                    ok("no-source: source_name absent")
            else:
                fail(f"no-source: expected ignore, got {d!r}")

    # --- ** forms ---
    # **/foo.txt: matches zero or more leading dirs
    data, err = compile_set(port, "**/foo.txt\n")
    if err:
        fail(f"compile **/: error {err!r}")
    else:
        sid = get_id(data, "compile **/")
        if sid:
            check_match(port, sid, "foo.txt", "regular_file", "ignore", "**: zero dirs")
            check_match(port, sid, "a/foo.txt", "regular_file", "ignore", "**: one dir")
            check_match(port, sid, "a/b/foo.txt", "regular_file", "ignore", "**: two dirs")

    # src/**: matches everything inside src/
    data, err = compile_set(port, "src/**\n")
    if err:
        fail(f"compile /**: error {err!r}")
    else:
        sid = get_id(data, "compile /**")
        if sid:
            check_match(port, sid, "src/a.txt", "regular_file", "ignore", "/**: direct child")
            check_match(port, sid, "src/a/b.txt", "regular_file", "ignore", "/**: nested")
            check_match(port, sid, "other/a.txt", "regular_file", "none", "/**: different root no match")

    # a/**/b.txt: zero or more intermediate dirs
    data, err = compile_set(port, "a/**/b.txt\n")
    if err:
        fail(f"compile /**/: error {err!r}")
    else:
        sid = get_id(data, "compile /**/")
        if sid:
            check_match(port, sid, "a/b.txt", "regular_file", "ignore", "/**/: zero intermediate dirs")
            check_match(port, sid, "a/x/b.txt", "regular_file", "ignore", "/**/: one intermediate dir")
            check_match(port, sid, "a/x/y/b.txt", "regular_file", "ignore", "/**/: two intermediate dirs")
            check_match(port, sid, "c/b.txt", "regular_file", "none", "/**/: wrong root no match")

    # --- interior slash makes pattern root-relative ---
    data, err = compile_set(port, "src/*.txt\n")
    if err:
        fail(f"compile interior slash: error {err!r}")
    else:
        sid = get_id(data, "compile interior slash")
        if sid:
            check_match(port, sid, "src/a.txt", "regular_file", "ignore",
                        "interior slash: root-relative match")
            check_match(port, sid, "other/src/a.txt", "regular_file", "none",
                        "interior slash: not at root no match")

    # --- invalid paths report invalid_path error ---
    data, err = compile_set(port, "*.txt\n")
    if err:
        fail(f"compile for invalid-path tests: error {err!r}")
    else:
        sid = get_id(data, "compile for invalid-path tests")
        if sid:
            invalid_cases = [
                ("", "regular_file", "empty path"),
                ("/abs", "regular_file", "leading slash"),
                ("dir/", "regular_file", "trailing slash"),
                ("a/../b", "regular_file", "dotdot segment"),
                ("a/./b", "regular_file", "dot segment"),
                ("a//b", "regular_file", "empty segment"),
                ("a\\b", "regular_file", "backslash in path"),
                ("a\x00b", "regular_file", "NUL in path"),
            ]
            for path, kind, label in invalid_cases:
                d, e = match_entry(port, sid, path, kind)
                if e == "invalid_path":
                    ok(f"invalid_path: {label}")
                elif e:
                    fail(f"invalid_path {label}: wrong error category {e!r}")
                else:
                    fail(f"invalid_path {label}: expected error, got decision={d.get('decision') if d else None!r}")

    # --- NUL in pattern_text -> invalid_pattern_text, no partial result ---
    data, err = compile_set(port, "*.txt\x00bad\n")
    if err == "invalid_pattern_text":
        ok("invalid_pattern_text: NUL in pattern_text rejected")
        if data and data.get("id") is not None:
            fail("invalid_pattern_text: error response must not include a partial pattern set id")
    elif err:
        fail(f"invalid_pattern_text: wrong error category {err!r}")
    else:
        fail("invalid_pattern_text: expected error for NUL in pattern_text, got none")

    # --- concurrent match: compiled set is safe for concurrent calls to match ---
    # not reasonably testable: public operations do not emit stdout or stderr
    # (MCP wrapper startup output is indistinguishable from library output via socket)
    data, err = compile_set(port, "*.log\n!important.log\n")
    if err:
        fail(f"compile concurrent: error {err!r}")
    else:
        sid = get_id(data, "compile concurrent")
        if sid:
            conc: list[tuple[str | None, str | None]] = []
            lock = threading.Lock()

            def concurrent_match(path: str) -> None:
                try:
                    d, e = match_entry(port, sid, path, "regular_file")
                    decision = d.get("decision") if d else None
                    with lock:
                        conc.append((decision, e))
                except Exception as ex:
                    with lock:
                        conc.append((None, str(ex)))

            threads = (
                [threading.Thread(target=concurrent_match, args=("app.log",)) for _ in range(6)]
                + [threading.Thread(target=concurrent_match, args=("important.log",)) for _ in range(6)]
            )
            for t in threads:
                t.start()
            for t in threads:
                t.join(timeout=15)

            # results arrive in arbitrary order; check counts not positions
            ignore_count = sum(1 for dec, e in conc if dec == "ignore" and e is None)
            include_count = sum(1 for dec, e in conc if dec == "include" and e is None)
            errors = [(dec, e) for dec, e in conc if e is not None]
            if len(conc) == 12 and ignore_count == 6 and include_count == 6 and not errors:
                ok("concurrent: 12 simultaneous match calls returned correct decisions")
            else:
                fail(
                    f"concurrent: expected 6 ignore + 6 include, "
                    f"got ignore={ignore_count} include={include_count} "
                    f"errors={errors} total={len(conc)}"
                )


def main() -> None:
    proc, port = launch_mcp()
    try:
        run_tests(port)
    finally:
        shutdown_mcp(proc, port)

    if failures:
        print(f"\n{len(failures)} failure(s):")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    print("\nAll checks passed.")


if __name__ == "__main__":
    main()
