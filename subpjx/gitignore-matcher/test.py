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

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync/subpjx/gitignore-matcher")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
MCP_JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/subpjx/gitignore-matcher/released/gitignore-matcher_MCP.jar")

failures: list[str] = []
_rpc_id = 0


def next_id() -> int:
    global _rpc_id
    _rpc_id += 1
    return _rpc_id


def check(cond: bool, msg: str) -> None:
    if not cond:
        failures.append(msg)
        print(f"FAIL: {msg}")
    else:
        print(f"OK:   {msg}")


# --------------------------------------------------------------------------- #
# MCP harness                                                                  #
# --------------------------------------------------------------------------- #

def drain(stream, sink: list | None = None) -> None:
    for line in stream:
        if sink is not None:
            sink.append(line)


def launch_mcp() -> tuple[subprocess.Popen[str], int]:
    proc = subprocess.Popen(
        [str(JAVA), "-jar", str(MCP_JAR)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if proc.stdout is None or proc.stderr is None:
        proc.terminate()
        raise RuntimeError("MCP server pipes were not created")

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
            proc.wait(timeout=2)
        raise RuntimeError(
            "MCP server did not advertise MCP_PORT\n"
            f"--- stdout ---\n{''.join(stdout_buf)}\n"
            f"--- stderr ---\n{''.join(stderr_buf)}"
        )

    threading.Thread(target=drain, args=(proc.stdout,), daemon=True).start()
    return proc, port


def rpc(sock: socket.socket, method: str, params=None, rpc_id: int | None = None) -> dict:
    if rpc_id is None:
        rpc_id = next_id()
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg, separators=(",", ":")) + "\n").encode("utf-8"))
    data = b""
    deadline = time.time() + 10
    while b"\n" not in data and time.time() < deadline:
        chunk = sock.recv(65536)
        if not chunk:
            break
        data += chunk
    line, _, _ = data.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def call_tool(sock: socket.socket, name: str, arguments: dict) -> dict:
    return rpc(sock, "tools/call", {"name": name, "arguments": arguments})


def shutdown_mcp(proc: subprocess.Popen[str], port: int) -> None:
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=3) as s:
            rpc(s, "aitc/shutdown", rpc_id=999)
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


# --------------------------------------------------------------------------- #
# Response parsing                                                             #
# --------------------------------------------------------------------------- #

def get_result(resp: dict) -> dict | list | None:
    """Return parsed result content, or None if the response is an error."""
    if "error" in resp:
        return None
    result = resp.get("result", {})
    if isinstance(result, dict) and result.get("isError"):
        return None
    content = result.get("content", []) if isinstance(result, dict) else []
    for item in content:
        if isinstance(item, dict) and item.get("type") == "text":
            try:
                return json.loads(item["text"])
            except (json.JSONDecodeError, KeyError):
                return item.get("text")
    return result if result != {} else None


def get_error(resp: dict) -> dict | None:
    """Return error info dict, or None if the response is not an error."""
    if "error" in resp:
        return resp["error"]
    result = resp.get("result", {})
    if isinstance(result, dict) and result.get("isError"):
        content = result.get("content", [])
        for item in content:
            if isinstance(item, dict) and item.get("type") == "text":
                try:
                    return json.loads(item["text"])
                except (json.JSONDecodeError, KeyError):
                    return {"message": item.get("text", "")}
        return {"message": "isError=true"}
    return None


# --------------------------------------------------------------------------- #
# Shorthand builders                                                           #
# --------------------------------------------------------------------------- #

def default_options() -> dict:
    return {
        "always_excluded_directory_names": [".kitchensync"],
        "default_excluded_directory_names": [".git"],
        "ignore_symlinks": True,
        "ignore_special_entries": True,
    }


def layer(base_path: str, pattern_text: str, source_name: str | None = None) -> dict:
    return {"base_path": base_path, "pattern_text": pattern_text, "source_name": source_name}


def entry(relative_path: str, kind: str) -> dict:
    return {"relative_path": relative_path, "kind": kind}


def do_compile(sock: socket.socket, layers: list[dict], options: dict | None = None) -> dict | None:
    resp = call_tool(sock, "compile", {"layers": layers, "options": options or default_options()})
    return get_result(resp)


def do_match(sock: socket.socket, matcher, ent: dict) -> dict | None:
    resp = call_tool(sock, "match", {"matcher": matcher, "entry": ent})
    return get_result(resp)


def do_filter(sock: socket.socket, matcher, entries: list[dict]) -> list | None:
    resp = call_tool(sock, "filter", {"matcher": matcher, "entries": entries})
    result = get_result(resp)
    if isinstance(result, list):
        return result
    if isinstance(result, dict):
        return result.get("entries") or result.get("result")
    return None


def entry_paths(lst: list) -> list[str]:
    return [e.get("relative_path") if isinstance(e, dict) else e for e in lst]


# --------------------------------------------------------------------------- #
# Tests                                                                        #
# --------------------------------------------------------------------------- #

def test_basic_patterns(sock: socket.socket) -> None:
    m = do_compile(sock, [layer("", "*.log\nbuild/\n!important.log")])
    check(m is not None, "compile returns a matcher")
    if m is None:
        return

    r = do_match(sock, m, entry("app.log", "regular_file"))
    check(r is not None, "match returns a result for app.log")
    if r:
        check(r.get("ignored") is True, "*.log matches app.log -> ignored")
        check(r.get("rule_kind") == "pattern", "app.log: rule_kind is pattern")
        check(r.get("negated") is not True, "app.log: not negated")
        check(r.get("pattern") == "*.log", "app.log: matched pattern is *.log")

    r = do_match(sock, m, entry("important.log", "regular_file"))
    if r:
        check(r.get("ignored") is False, "!important.log re-includes important.log")
        check(r.get("negated") is True, "important.log: negated=true")
        check(r.get("rule_kind") == "pattern", "important.log: rule_kind is pattern")

    r = do_match(sock, m, entry("src/build", "directory"))
    if r:
        check(r.get("ignored") is True, "build/ matches directory src/build")

    r = do_match(sock, m, entry("src/build/out.bin", "regular_file"))
    if r:
        check(r.get("ignored") is True, "descendant of ignored dir is ignored")

    r = do_match(sock, m, entry("src/main.txt", "regular_file"))
    if r:
        check(r.get("ignored") is False, "unmatched src/main.txt is not ignored")
        check(r.get("rule_kind") == "none", "unmatched entry: rule_kind is none")

    # directory-only pattern does not match regular file with same basename
    r = do_match(sock, m, entry("build", "regular_file"))
    if r:
        check(r.get("ignored") is False, "build/ pattern does not match regular file named build")


def test_filter(sock: socket.socket) -> None:
    m = do_compile(sock, [layer("", "*.log\nbuild/\n!important.log")])
    if m is None:
        check(False, "compile for filter test failed")
        return

    ins = [
        entry("app.log", "regular_file"),
        entry("important.log", "regular_file"),
        entry("src/build", "directory"),
        entry("src/build/out.bin", "regular_file"),
        entry("src/main.txt", "regular_file"),
    ]
    result = do_filter(sock, m, ins)
    check(result is not None, "filter returns a result")
    if result is not None:
        paths = entry_paths(result)
        check("important.log" in paths, "filter keeps important.log")
        check("src/main.txt" in paths, "filter keeps src/main.txt")
        check("app.log" not in paths, "filter removes app.log")
        check("src/build" not in paths, "filter removes src/build")
        check("src/build/out.bin" not in paths, "filter removes src/build/out.bin")
        if "important.log" in paths and "src/main.txt" in paths:
            check(paths.index("important.log") < paths.index("src/main.txt"), "filter preserves input order")


def test_pattern_syntax(sock: socket.socket) -> None:
    # ? and bracket expressions
    m = do_compile(sock, [layer("", "?.txt\n[abc].md\n[!0-9].json")])
    if m is None:
        check(False, "compile for syntax test failed")
        return

    r = do_match(sock, m, entry("a.txt", "regular_file"))
    if r:
        check(r.get("ignored") is True, "? matches single char: a.txt")
    r = do_match(sock, m, entry("ab.txt", "regular_file"))
    if r:
        check(r.get("ignored") is False, "? does not match two chars: ab.txt")
    r = do_match(sock, m, entry("a.md", "regular_file"))
    if r:
        check(r.get("ignored") is True, "[abc] matches a.md")
    r = do_match(sock, m, entry("d.md", "regular_file"))
    if r:
        check(r.get("ignored") is False, "[abc] does not match d.md")
    r = do_match(sock, m, entry("x.json", "regular_file"))
    if r:
        check(r.get("ignored") is True, "[!0-9] matches non-digit x.json")
    r = do_match(sock, m, entry("5.json", "regular_file"))
    if r:
        check(r.get("ignored") is False, "[!0-9] does not match digit 5.json")

    # leading slash anchoring and interior slashes
    m2 = do_compile(sock, [layer("", "/root_only.txt\nsrc/*.c")])
    if m2:
        r = do_match(sock, m2, entry("root_only.txt", "regular_file"))
        if r:
            check(r.get("ignored") is True, "leading / anchors to root: root_only.txt")
        r = do_match(sock, m2, entry("sub/root_only.txt", "regular_file"))
        if r:
            check(r.get("ignored") is False, "anchored pattern does not match in subdir")
        r = do_match(sock, m2, entry("src/main.c", "regular_file"))
        if r:
            check(r.get("ignored") is True, "interior slash src/*.c matches src/main.c")
        r = do_match(sock, m2, entry("other/main.c", "regular_file"))
        if r:
            check(r.get("ignored") is False, "src/*.c does not match other/main.c")

    # ** forms
    m3 = do_compile(sock, [layer("", "**/logs\nbuild/**\na/**/b.txt")])
    if m3:
        r = do_match(sock, m3, entry("logs", "directory"))
        if r:
            check(r.get("ignored") is True, "**/ matches logs at root")
        r = do_match(sock, m3, entry("x/y/logs", "directory"))
        if r:
            check(r.get("ignored") is True, "**/ matches logs at any depth")
        r = do_match(sock, m3, entry("build/output.bin", "regular_file"))
        if r:
            check(r.get("ignored") is True, "/** matches everything under build")
        r = do_match(sock, m3, entry("a/x/b.txt", "regular_file"))
        if r:
            check(r.get("ignored") is True, "/**/ matches intermediate dirs in a/**/b.txt")
        r = do_match(sock, m3, entry("a/b.txt", "regular_file"))
        if r:
            check(r.get("ignored") is True, "/**/ matches zero dirs in a/**/b.txt")

    # basename matching at any depth (pattern without /)
    m4 = do_compile(sock, [layer("", "*.log")])
    if m4:
        r = do_match(sock, m4, entry("deep/nested/file.log", "regular_file"))
        if r:
            check(r.get("ignored") is True, "no-slash pattern matches basename at any depth")


def test_blank_comments_escape(sock: socket.socket) -> None:
    text = "# comment line\n\n\\#literal_hash\n\\!literal_bang\ntrailing   \n"
    m = do_compile(sock, [layer("", text)])
    if m is None:
        check(False, "compile for escape test failed")
        return

    r = do_match(sock, m, entry("comment_line", "regular_file"))
    if r:
        check(r.get("ignored") is False, "comment line is not a pattern")
    r = do_match(sock, m, entry("#literal_hash", "regular_file"))
    if r:
        check(r.get("ignored") is True, "\\# is literal # in pattern")
    r = do_match(sock, m, entry("!literal_bang", "regular_file"))
    if r:
        check(r.get("ignored") is True, "\\! is literal ! in pattern")
    r = do_match(sock, m, entry("trailing", "regular_file"))
    if r:
        check(r.get("ignored") is True, "trailing spaces stripped from pattern name")

    # escaped trailing space: backslash before space preserves the space
    m2 = do_compile(sock, [layer("", "trailing\\ ")])
    if m2:
        r = do_match(sock, m2, entry("trailing ", "regular_file"))
        if r:
            check(r.get("ignored") is True, "escaped trailing space preserved: matches entry with trailing space")
        r = do_match(sock, m2, entry("trailing", "regular_file"))
        if r:
            check(r.get("ignored") is False, "escaped trailing space: entry without space does not match")


def test_pattern_order(sock: socket.socket) -> None:
    # last matching overridable rule wins
    m = do_compile(sock, [layer("", "*.log\n!important.log\n*.log")])
    if m is None:
        return
    r = do_match(sock, m, entry("important.log", "regular_file"))
    if r:
        check(r.get("ignored") is True, "last matching *.log overrides earlier !important.log")


def test_hierarchical_layers(sock: socket.socket) -> None:
    m = do_compile(sock, [
        layer("", "*.tmp", source_name="root"),
        layer("docs", "!keep.tmp\nmanual/*.bak", source_name="docs"),
    ])
    if m is None:
        check(False, "compile for hierarchical layers test failed")
        return

    r = do_match(sock, m, entry("scratch.tmp", "regular_file"))
    if r:
        check(r.get("ignored") is True, "root *.tmp matches scratch.tmp")
        check(r.get("source_name") == "root", "scratch.tmp: source_name is root")

    r = do_match(sock, m, entry("docs/draft.tmp", "regular_file"))
    if r:
        check(r.get("ignored") is True, "root *.tmp matches docs/draft.tmp")

    r = do_match(sock, m, entry("docs/keep.tmp", "regular_file"))
    if r:
        check(r.get("ignored") is False, "docs !keep.tmp re-includes docs/keep.tmp")
        check(r.get("source_name") == "docs", "docs/keep.tmp: source_name is docs")

    r = do_match(sock, m, entry("docs/manual/old.bak", "regular_file"))
    if r:
        check(r.get("ignored") is True, "docs manual/*.bak matches docs/manual/old.bak")

    # docs layer patterns only apply within docs/, not at root
    r = do_match(sock, m, entry("keep.tmp", "regular_file"))
    if r:
        check(r.get("ignored") is True, "docs !keep.tmp does not re-include keep.tmp at root")


def test_directory_descendant_cascade(sock: socket.socket) -> None:
    # descendant-only negation does not bypass ignored parent
    m = do_compile(sock, [layer("", "secret/\n!secret/public.txt")])
    if m is None:
        return
    r = do_match(sock, m, entry("secret", "directory"))
    if r:
        check(r.get("ignored") is True, "secret/ directory is ignored")
    r = do_match(sock, m, entry("secret/data.bin", "regular_file"))
    if r:
        check(r.get("ignored") is True, "descendant of ignored dir is still ignored")
    r = do_match(sock, m, entry("secret/public.txt", "regular_file"))
    if r:
        check(r.get("ignored") is True, "descendant-only negation does not bypass ignored parent dir")

    # re-include the directory itself, then descendants become accessible
    m2 = do_compile(sock, [layer("", "secret/\n!secret/")])
    if m2:
        r = do_match(sock, m2, entry("secret", "directory"))
        if r:
            check(r.get("ignored") is False, "!secret/ re-includes the directory")
        r = do_match(sock, m2, entry("secret/data.bin", "regular_file"))
        if r:
            check(r.get("ignored") is False, "descendants accessible after parent re-included")


def test_builtin_exclusions(sock: socket.socket) -> None:
    # .kitchensync: always excluded, cannot be re-included
    m = do_compile(sock, [layer("", "!.kitchensync/\n!.git/\n!link")])
    if m is None:
        check(False, "compile for builtin exclusions test failed")
        return

    r = do_match(sock, m, entry(".kitchensync", "directory"))
    if r:
        check(r.get("ignored") is True, ".kitchensync ignored despite negation")
        check(r.get("rule_kind") == "always_builtin", ".kitchensync: rule_kind is always_builtin")

    r = do_match(sock, m, entry(".kitchensync/snapshot.db", "regular_file"))
    if r:
        check(r.get("ignored") is True, ".kitchensync descendant always ignored")
        check(r.get("rule_kind") == "always_builtin", ".kitchensync descendant: rule_kind is always_builtin")

    # .git: default excluded, can be re-included
    r = do_match(sock, m, entry(".git", "directory"))
    if r:
        check(r.get("ignored") is False, ".git re-included by !.git/ negation")

    r = do_match(sock, m, entry(".git/config", "regular_file"))
    if r:
        check(r.get("ignored") is False, ".git/config accessible after .git re-included")

    # without negation, .git is default excluded
    m2 = do_compile(sock, [layer("", "")])
    if m2:
        r = do_match(sock, m2, entry(".git", "directory"))
        if r:
            check(r.get("ignored") is True, ".git ignored by default")
            check(r.get("rule_kind") == "default_builtin", ".git: rule_kind is default_builtin")
        r = do_match(sock, m2, entry(".git/config", "regular_file"))
        if r:
            check(r.get("ignored") is True, ".git/config ignored when .git is ignored")

    # symlinks: always ignored, cannot be re-included
    r = do_match(sock, m, entry("link", "symlink"))
    if r:
        check(r.get("ignored") is True, "symlink ignored despite !link negation")
        check(r.get("rule_kind") == "always_builtin", "symlink: rule_kind is always_builtin")

    # special entries: always ignored
    if m2:
        r = do_match(sock, m2, entry("fifo0", "special"))
        if r:
            check(r.get("ignored") is True, "special entry ignored by default")
            check(r.get("rule_kind") == "always_builtin", "special: rule_kind is always_builtin")


def test_empty_and_extend(sock: socket.socket) -> None:
    resp = call_tool(sock, "empty", {"options": default_options()})
    me = get_result(resp)
    check(me is not None, "empty() returns a matcher")
    if me is None:
        return

    r = do_match(sock, me, entry(".kitchensync", "directory"))
    if r:
        check(r.get("ignored") is True, "empty matcher still excludes .kitchensync")
    r = do_match(sock, me, entry("anything.log", "regular_file"))
    if r:
        check(r.get("ignored") is False, "empty matcher does not exclude regular files")

    # extend adds a layer
    resp2 = call_tool(sock, "extend", {"matcher": me, "layer": layer("", "*.log")})
    mx = get_result(resp2)
    check(mx is not None, "extend() returns a matcher")
    if mx:
        r = do_match(sock, mx, entry("app.log", "regular_file"))
        if r:
            check(r.get("ignored") is True, "extended matcher sees new *.log pattern")
        # original matcher is immutable -- unaffected by extend
        r = do_match(sock, me, entry("app.log", "regular_file"))
        if r:
            check(r.get("ignored") is False, "original matcher unaffected after extend (immutable)")


def test_match_result_details(sock: socket.socket) -> None:
    m = do_compile(sock, [layer("", "# comment\n\n*.log", source_name="my-source")])
    if m is None:
        return
    r = do_match(sock, m, entry("app.log", "regular_file"))
    if r:
        check(r.get("source_name") == "my-source", "source_name propagated in MatchResult")
        check(r.get("line_number") == 3, "line_number accounts for comment and blank (line 3)")
        check(r.get("pattern") == "*.log", "pattern text in MatchResult")


def test_error_invalid_paths(sock: socket.socket) -> None:
    m = do_compile(sock, [layer("", "*.log")])
    if m is None:
        return

    bad_paths = [
        ("/leading-slash", "leading slash in entry path"),
        ("a//b", "empty segment in entry path"),
        ("a/./b", ". segment in entry path"),
        ("a/../b", ".. segment in entry path"),
        ("", "empty entry path"),
        ("a/", "trailing slash in entry path"),
        ("a\\b", "backslash in entry path"),
        ("a\x00b", "NUL in entry path"),
    ]
    for path, desc in bad_paths:
        resp = call_tool(sock, "match", {"matcher": m, "entry": entry(path, "regular_file")})
        err = get_error(resp)
        check(err is not None, f"error for {desc}")
        if err:
            cat = str(err.get("category") or err.get("error_category") or err)
            check("invalid_path" in cat.lower(), f"category is invalid_path for {desc}, got: {cat}")

    # invalid base_path in layer
    resp = call_tool(sock, "compile", {
        "layers": [layer("/bad-base", "*.log")],
        "options": default_options(),
    })
    err = get_error(resp)
    check(err is not None, "leading slash in layer base_path produces error")
    if err:
        cat = str(err.get("category") or err.get("error_category") or err)
        check("invalid_path" in cat.lower(), f"base_path error category is invalid_path, got: {cat}")


def test_error_nul_pattern(sock: socket.socket) -> None:
    resp = call_tool(sock, "compile", {
        "layers": [layer("", "*.log\x00evil")],
        "options": default_options(),
    })
    err = get_error(resp)
    check(err is not None, "NUL in pattern text produces error")
    if err:
        cat = str(err.get("category") or err.get("error_category") or err)
        check("invalid_pattern_text" in cat.lower(), f"NUL error category is invalid_pattern_text, got: {cat}")


def test_error_invalid_options(sock: socket.socket) -> None:
    bad_names = [
        ("", "empty directory name"),
        ("a/b", "directory name with slash"),
        ("a\\b", "directory name with backslash"),
        (".", ". as directory name"),
        ("..", ".. as directory name"),
        ("\x00", "NUL as directory name"),
    ]
    for name, desc in bad_names:
        opts = {**default_options(), "always_excluded_directory_names": [name]}
        resp = call_tool(sock, "compile", {"layers": [], "options": opts})
        err = get_error(resp)
        check(err is not None, f"invalid always_excluded name ({desc}) produces error")
        if err:
            cat = str(err.get("category") or err.get("error_category") or err)
            check("invalid_options" in cat.lower(), f"options error category is invalid_options for {desc}, got: {cat}")


def test_concurrent_match(port: int) -> None:
    with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
        resp = call_tool(s, "compile", {
            "layers": [layer("", "*.log")],
            "options": default_options(),
        })
        mc = get_result(resp)
    check(mc is not None, "compile for concurrency test")
    if mc is None:
        return

    results: list[str] = []
    lock = threading.Lock()

    def do_concurrent(tid: int) -> None:
        path = "app.log" if tid % 2 == 0 else "main.txt"
        expected = tid % 2 == 0
        r = None
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=5) as s:
                r = do_match(s, mc, entry(path, "regular_file"))
                ok = r is not None and r.get("ignored") == expected
        except Exception as e:
            ok = False
            with lock:
                results.append(f"thread {tid}: exception {e}")
            return
        if not ok:
            with lock:
                results.append(f"thread {tid}: got {r}, expected ignored={expected}")

    threads = [threading.Thread(target=do_concurrent, args=(i,)) for i in range(8)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=15)

    check(not results, f"concurrent match -- failures: {results}")


# --------------------------------------------------------------------------- #
# Main                                                                         #
# --------------------------------------------------------------------------- #

def main() -> None:
    proc, port = launch_mcp()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            test_basic_patterns(sock)
            test_filter(sock)
            test_pattern_syntax(sock)
            test_blank_comments_escape(sock)
            test_pattern_order(sock)
            test_hierarchical_layers(sock)
            test_directory_descendant_cascade(sock)
            test_builtin_exclusions(sock)
            test_empty_and_extend(sock)
            test_match_result_details(sock)
            test_error_invalid_paths(sock)
            test_error_nul_pattern(sock)
            test_error_invalid_options(sock)
        test_concurrent_match(port)
        # not reasonably testable: "no public operation emits stdout or stderr" --
        # library output cannot be isolated from MCP wrapper output through tools/call.
    finally:
        shutdown_mcp(proc, port)

    if failures:
        print(f"\n{len(failures)} failure(s):")
        for f in failures:
            print(f"  - {f}")
        sys.exit(1)
    else:
        print(f"\nAll checks passed.")


if __name__ == "__main__":
    main()
