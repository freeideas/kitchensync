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
from pathlib import Path
from typing import Any

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync/subpjx/url-parser")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
MCP_JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/subpjx/url-parser/released/url-parser_MCP.jar")


def drain(stream: Any, sink: list[str] | None = None) -> None:
    for line in stream:
        if sink is not None:
            sink.append(line)


def launch_mcp() -> tuple[subprocess.Popen[str], int]:
    proc = subprocess.Popen(
        [str(JAVA), "-jar", str(MCP_JAR)],
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

    stderr_buf: list[str] = []
    threading.Thread(target=drain, args=(proc.stderr, stderr_buf), daemon=True).start()

    stdout_buf: list[str] = []
    port: int | None = None
    deadline = time.time() + 30
    while time.time() < deadline and proc.poll() is None:
        line = proc.stdout.readline()
        if line == "":
            time.sleep(0.05)
            continue
        stdout_buf.append(line)
        if line.startswith("MCP_PORT="):
            port = int(line.strip().split("=", 1)[1])
            break

    if port is None:
        _terminate(proc)
        raise RuntimeError(
            "MCP server did not advertise MCP_PORT\n"
            f"--- subprocess stdout ---\n{''.join(stdout_buf)}"
            f"--- subprocess stderr ---\n{''.join(stderr_buf)}"
        )

    threading.Thread(target=drain, args=(proc.stdout,), daemon=True).start()
    return proc, port


def rpc(sock: socket.socket, method: str, params: dict[str, Any] | None = None, rpc_id: int = 1) -> Any:
    message: dict[str, Any] = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        message["params"] = params
    sock.sendall((json.dumps(message, separators=(",", ":")) + "\n").encode("utf-8"))
    data = b""
    deadline = time.time() + 10
    while b"\n" not in data and time.time() < deadline:
        chunk = sock.recv(65536)
        if not chunk:
            break
        data += chunk
    line, _, _ = data.partition(b"\n")
    if not line:
        raise RuntimeError(f"no response to {method!r}")
    return json.loads(line.decode("utf-8"))


def _terminate(proc: subprocess.Popen[str]) -> None:
    try:
        proc.wait(timeout=1)
        return
    except subprocess.TimeoutExpired:
        pass
    proc.terminate()
    try:
        proc.wait(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()


def shutdown_mcp(proc: subprocess.Popen[str], port: int) -> None:
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=3) as s:
            rpc(s, "aitc/shutdown", rpc_id=999)
    except Exception:
        pass
    _terminate(proc)


def unwrap(response: Any) -> tuple[bool, Any]:
    """Returns (success, payload). On error payload is the error object."""
    if "error" in response:
        return False, response["error"]
    result = response.get("result")
    if isinstance(result, dict) and result.get("isError") is True:
        return False, result
    if isinstance(result, dict) and "content" in result:
        content = result["content"]
        if isinstance(content, list) and content:
            parts = [i.get("text", "") for i in content if isinstance(i, dict) and i.get("type") == "text"]
            text = "\n".join(parts).strip()
            if text:
                try:
                    return True, json.loads(text)
                except json.JSONDecodeError:
                    return True, text
    return True, result


class Client:
    def __init__(self, sock: socket.socket) -> None:
        self.sock = sock
        self._id = 0

    def call(self, tool: str, arguments: dict[str, Any]) -> tuple[bool, Any]:
        self._id += 1
        return unwrap(rpc(self.sock, "tools/call", {"name": tool, "arguments": arguments}, self._id))


def ctx(cwd: str = "/home/ace/work", user: str = "ace") -> dict[str, str]:
    return {"current_working_directory": cwd, "current_os_user": user}


def peer_from(payload: Any) -> dict[str, Any]:
    if isinstance(payload, dict) and "role" in payload and "candidates" in payload:
        return payload
    if isinstance(payload, dict) and isinstance(payload.get("parsed_peer"), dict):
        return payload["parsed_peer"]
    raise AssertionError(f"unexpected parse-peer-operand payload: {payload!r}")


def url_from(payload: Any) -> dict[str, Any]:
    if isinstance(payload, dict) and "scheme" in payload and "canonical_identity" in payload:
        return payload
    if isinstance(payload, dict) and isinstance(payload.get("parsed_url"), dict):
        return payload["parsed_url"]
    raise AssertionError(f"unexpected parse-url payload: {payload!r}")


def identity_from(payload: Any) -> str:
    if isinstance(payload, str):
        return payload
    if isinstance(payload, dict):
        for k in ("canonical_identity", "identity", "result"):
            if isinstance(payload.get(k), str):
                return payload[k]
    raise AssertionError(f"unexpected normalize-identity payload: {payload!r}")


def setting(url: dict[str, Any], key: str) -> Any:
    return (url.get("settings") or {}).get(key)


def error_cat(payload: Any) -> str:
    if isinstance(payload, dict):
        for k in ("category", "error", "error_category", "code"):
            v = payload.get(k)
            if isinstance(v, str):
                return v
        if "message" in payload:
            return str(payload["message"])
        if "content" in payload:
            texts = [i.get("text", "") for i in payload["content"] if isinstance(i, dict)]
            return " ".join(texts)
    return str(payload)


def run(client: Client, failures: list[str]) -> None:
    def ok(cond: bool, msg: str, detail: Any = None) -> bool:
        if not cond:
            failures.append(msg if detail is None else f"{msg}: {detail!r}")
        return cond

    def eq(actual: Any, expected: Any, msg: str) -> None:
        ok(actual == expected, f"{msg}; expected {expected!r}, got {actual!r}")

    def success(result: tuple[bool, Any], msg: str) -> tuple[bool, Any] | None:
        s, p = result
        if not ok(s, f"{msg} should succeed", p):
            return None
        return s, p

    def error(tool: str, args: dict[str, Any], cat: str, msg: str) -> None:
        s, p = client.call(tool, args)
        if not ok(not s, f"{msg}; expected error {cat!r}, got success", p):
            return
        got = error_cat(p)
        ok(cat in got, f"{msg}; expected category {cat!r} in error, got {got!r}", p)

    # ----------------------------------------------------------------
    # Bare POSIX absolute path -- double slashes collapsed, trailing slash removed
    # ----------------------------------------------------------------
    r = success(client.call("parse-peer-operand", {"text": "/var//data/", "context": ctx()}), "bare POSIX absolute path")
    if r:
        peer = peer_from(r[1])
        cands = peer.get("candidates", [])
        url = cands[0] if cands else {}
        eq(peer.get("role"), "normal", "bare path has normal role")
        eq(len(cands), 1, "non-bracket operand has exactly one candidate")
        eq(url.get("scheme"), "file", "bare path scheme")
        eq(url.get("canonical_identity"), "file:///var/data", "bare POSIX absolute path identity -- double slash collapsed, trailing slash removed")
        eq(url.get("path"), "/var/data", "bare POSIX absolute path field")

    # ----------------------------------------------------------------
    # Relative path (./) -- lexical resolution against cwd, no filesystem access
    # ----------------------------------------------------------------
    r = success(client.call("parse-peer-operand", {"text": "./missing/../data/", "context": ctx()}), "relative path ./")
    if r:
        url = peer_from(r[1])["candidates"][0]
        eq(url.get("canonical_identity"), "file:///home/ace/work/data", "relative ./ path identity resolves against cwd")
        eq(url.get("path"), "/home/ace/work/data", "relative ./ path field")

    # ----------------------------------------------------------------
    # Relative path (../) -- lexical resolution against cwd
    # ----------------------------------------------------------------
    r = success(client.call("parse-url", {"text": "../sibling", "context": ctx(cwd="/home/ace/work")}), "relative path ../")
    if r:
        url = url_from(r[1])
        eq(url.get("canonical_identity"), "file:///home/ace/sibling", "relative ../ path resolves lexically against cwd")

    # ----------------------------------------------------------------
    # Windows drive path -- backslash converted, trailing slashes removed
    # ----------------------------------------------------------------
    r = success(client.call("parse-peer-operand", {"text": r"-c:\photos\raw\\", "context": ctx()}), "Windows drive backslash path")
    if r:
        peer = peer_from(r[1])
        url = peer["candidates"][0]
        eq(peer.get("role"), "subordinate", "minus prefix produces subordinate role")
        eq(url.get("canonical_identity"), "file:///c:/photos/raw", "Windows drive backslash path identity")
        eq(url.get("path"), "c:/photos/raw", "Windows drive path uses forward slashes")
        eq(url.get("scheme"), "file", "Windows drive path scheme")

    # Windows drive path -- forward-slash form
    r = success(client.call("parse-url", {"text": "c:/photos", "context": ctx()}), "Windows drive forward slash path")
    if r:
        url = url_from(r[1])
        eq(url.get("canonical_identity"), "file:///c:/photos", "Windows drive forward slash identity")
        eq(url.get("scheme"), "file", "Windows drive forward slash scheme")

    # ----------------------------------------------------------------
    # file:// URL -- fragment stripped, double slashes collapsed
    # ----------------------------------------------------------------
    r = success(client.call("parse-url", {"text": "file:///home/ace/work//data/#frag", "context": ctx()}), "file URL")
    if r:
        url = url_from(r[1])
        eq(url.get("scheme"), "file", "file URL scheme")
        eq(url.get("canonical_identity"), "file:///home/ace/work/data", "file URL identity strips fragment and normalizes path")

    # ----------------------------------------------------------------
    # SFTP -- omitted user filled from current_os_user, hostname lowercased,
    # scheme lowercased, port 22 omitted from identity, double slashes collapsed,
    # query stripped from identity, all three settings parsed, endpoint_key
    # ----------------------------------------------------------------
    r = success(client.call("parse-url", {"text": "SFTP://Host:22//docs/?mc=5&ct=60&ka=30", "context": ctx()}), "SFTP default-user normalization")
    if r:
        url = url_from(r[1])
        eq(url.get("scheme"), "sftp", "SFTP scheme lowercased")
        eq(url.get("user"), "ace", "omitted SFTP user filled from context")
        eq(url.get("host"), "host", "SFTP host lowercased")
        eq(url.get("port"), 22, "SFTP omitted port normalized to 22")
        eq(url.get("path"), "/docs", "SFTP double slashes collapsed in path")
        eq(url.get("endpoint_key"), "ace@host:22", "SFTP endpoint_key is user@host:port")
        eq(url.get("canonical_identity"), "sftp://ace@host/docs", "SFTP identity omits port 22 and query")
        ok(not url.get("password"), "SFTP no inline password", url.get("password"))
        eq(setting(url, "max_connections"), 5, "mc setting")
        eq(setting(url, "connect_timeout_seconds"), 60, "ct setting")
        eq(setting(url, "idle_keep_alive_seconds"), 30, "ka setting")

    # ----------------------------------------------------------------
    # SFTP -- explicit user, percent-decoded inline password, non-default port,
    # identity excludes password, non-default port present in identity,
    # settings associated with declaring candidate
    # ----------------------------------------------------------------
    r = success(client.call("parse-url", {"text": "sftp://bilbo:p%40ss@Backup.Example:2222/photos///?mc=7", "context": ctx()}), "SFTP explicit credentials")
    if r:
        url = url_from(r[1])
        eq(url.get("user"), "bilbo", "explicit SFTP user preserved")
        eq(url.get("password"), "p@ss", "inline password percent-decoded")
        eq(url.get("host"), "backup.example", "explicit SFTP host lowercased")
        eq(url.get("port"), 2222, "explicit SFTP non-default port preserved")
        eq(url.get("path"), "/photos", "SFTP path trailing slashes removed")
        eq(url.get("endpoint_key"), "bilbo@backup.example:2222", "SFTP endpoint_key with non-default port")
        eq(url.get("canonical_identity"), "sftp://bilbo@backup.example:2222/photos", "SFTP identity excludes password; non-default port included")
        eq(setting(url, "max_connections"), 7, "query setting associated with declaring candidate")

    # ----------------------------------------------------------------
    # SFTP -- percent encoding: unreserved chars decoded in identity,
    # reserved chars (like %2F) kept encoded to preserve URL structure
    # ----------------------------------------------------------------
    r = success(client.call("parse-url", {"text": "sftp://Host/%7Euser/%2Fliteral", "context": ctx()}), "SFTP percent-encoded path")
    if r:
        url = url_from(r[1])
        eq(url.get("canonical_identity"), "sftp://ace@host/~user/%2Fliteral",
           "unreserved %7E decoded to ~; reserved %2F preserved in canonical identity")

    # ----------------------------------------------------------------
    # Passwords and query settings do not change canonical_identity;
    # sftp://host/path and sftp://user@host:22/path share canonical identity
    # ----------------------------------------------------------------
    ok1, p1 = client.call("normalize-identity", {"text": "sftp://user:one@host/path?mc=1", "context": ctx()})
    ok2, p2 = client.call("normalize-identity", {"text": "sftp://user:two@host:22/path?mc=9", "context": ctx()})
    if ok(ok1 and ok2, "normalize-identity password/settings variants both succeed", (p1, p2)):
        eq(identity_from(p1), identity_from(p2), "passwords, settings, and explicit default port do not affect canonical identity")

    ok1, p1 = client.call("normalize-identity", {"text": "sftp://myhost/data", "context": ctx(user="myuser")})
    ok2, p2 = client.call("normalize-identity", {"text": "sftp://myuser@myhost:22/data", "context": ctx(user="myuser")})
    if ok(ok1 and ok2, "normalize-identity user-insertion variants both succeed", (p1, p2)):
        eq(identity_from(p1), identity_from(p2), "sftp://host/path and sftp://user@host:22/path have the same identity when user matches")

    # ----------------------------------------------------------------
    # Fallback group -- role applied to whole group, candidate order preserved,
    # each candidate keeps its own settings, endpoint_key present on each
    # ----------------------------------------------------------------
    r = success(
        client.call("parse-peer-operand", {
            "text": "+[sftp://Host:22//photos/?mc=5&ct=60,sftp://bilbo:p%40ss@backup.example:2222/photos?ka=45]",
            "context": ctx(),
        }),
        "fallback group",
    )
    if r:
        peer = peer_from(r[1])
        eq(peer.get("role"), "canon", "plus prefix applies to whole fallback group")
        cands = peer.get("candidates", [])
        eq(len(cands), 2, "fallback group has two candidates")
        if len(cands) == 2:
            c0, c1 = cands
            eq(c0.get("canonical_identity"), "sftp://ace@host/photos", "first fallback candidate identity")
            eq(c0.get("endpoint_key"), "ace@host:22", "first fallback candidate endpoint_key")
            eq(setting(c0, "max_connections"), 5, "first candidate mc setting")
            eq(setting(c0, "connect_timeout_seconds"), 60, "first candidate ct setting")
            ok(not setting(c0, "idle_keep_alive_seconds"), "first candidate has no ka setting")
            eq(c1.get("canonical_identity"), "sftp://bilbo@backup.example:2222/photos", "second fallback candidate identity")
            eq(c1.get("password"), "p@ss", "second fallback candidate password decoded")
            eq(setting(c1, "idle_keep_alive_seconds"), 45, "second candidate ka setting")
            ok(not setting(c1, "max_connections"), "second candidate has no mc setting")

    # ----------------------------------------------------------------
    # Role prefixes +, -, and no prefix produce canon, subordinate, normal;
    # non-bracket operand has exactly one candidate
    # ----------------------------------------------------------------
    for text, expected_role in (("+./data", "canon"), ("-./data", "subordinate"), ("./data", "normal")):
        r = success(client.call("parse-peer-operand", {"text": text, "context": ctx()}), f"role prefix {text!r}")
        if r:
            peer = peer_from(r[1])
            eq(peer.get("role"), expected_role, f"{text!r} role")
            eq(len(peer.get("candidates", [])), 1, f"{text!r} has exactly one candidate (non-bracket)")

    # ================================================================
    # ERROR CASES
    # ================================================================

    error("parse-url",          {"text": "",                                      "context": ctx()},                       "empty_operand",          "empty URL candidate is rejected")
    error("parse-peer-operand", {"text": "",                                      "context": ctx()},                       "empty_operand",          "empty peer operand is rejected")
    error("parse-peer-operand", {"text": "++./data",                              "context": ctx()},                       "invalid_role_prefix",    "multiple role prefixes are rejected")
    error("parse-peer-operand", {"text": "[+sftp://h1/path,sftp://h2/path]",      "context": ctx()},                       "invalid_role_prefix",    "role prefix inside fallback group is rejected")
    error("parse-peer-operand", {"text": "[]",                                    "context": ctx()},                       "invalid_fallback_group", "empty fallback group is rejected")
    error("parse-peer-operand", {"text": "[sftp://host/a",                        "context": ctx()},                       "invalid_fallback_group", "unbalanced open bracket is rejected")
    error("parse-peer-operand", {"text": "sftp://host/a]",                        "context": ctx()},                       "invalid_fallback_group", "unbalanced close bracket is rejected")
    error("parse-peer-operand", {"text": "[sftp://host/a,,sftp://host/b]",        "context": ctx()},                       "invalid_fallback_group", "empty candidate inside fallback group is rejected")
    error("parse-peer-operand", {"text": "[sftp://host/a,[sftp://host/b]]",       "context": ctx()},                       "invalid_fallback_group", "nested fallback group is rejected")
    error("parse-url",          {"text": "http://example.com/path",               "context": ctx()},                       "unsupported_scheme",     "unsupported scheme http is rejected")
    error("parse-url",          {"text": "ftp://example.com/path",                "context": ctx()},                       "unsupported_scheme",     "unsupported scheme ftp is rejected")
    error("parse-url",          {"text": "file://host/path",                      "context": ctx()},                       "invalid_file_url",       "file URL with non-empty authority is rejected")
    error("parse-url",          {"text": "sftp:///path",                          "context": ctx()},                       "invalid_sftp_url",       "SFTP URL missing host is rejected")
    error("parse-url",          {"text": "sftp://host:abc/path",                  "context": ctx()},                       "invalid_sftp_url",       "SFTP URL with non-numeric port is rejected")
    error("parse-url",          {"text": "sftp://host",                           "context": ctx()},                       "invalid_sftp_url",       "SFTP URL without absolute path is rejected")
    error("parse-url",          {"text": "sftp://host/path?mc=1&mc=2",            "context": ctx()},                       "invalid_setting",        "duplicate query setting is rejected")
    error("parse-url",          {"text": "sftp://host/path?unknown=1",            "context": ctx()},                       "invalid_setting",        "unknown query setting is rejected")
    error("parse-url",          {"text": "sftp://host/path?ct=0",                 "context": ctx()},                       "invalid_setting",        "zero (non-positive) ct setting is rejected")
    error("parse-url",          {"text": "sftp://host/path?ka=-1",                "context": ctx()},                       "invalid_setting",        "negative ka setting is rejected")
    error("parse-url",          {"text": "sftp://host/path?mc=abc",               "context": ctx()},                       "invalid_setting",        "non-integer mc setting is rejected")
    error("parse-url",          {"text": "sftp://host/%zz",                       "context": ctx()},                       "invalid_percent_encoding","non-hex percent escape is rejected")
    error("parse-url",          {"text": "sftp://host/%",                         "context": ctx()},                       "invalid_percent_encoding","incomplete percent escape is rejected")
    error("parse-url",          {"text": "./data",                                "context": ctx(cwd="relative/work")},    "invalid_context",        "non-absolute cwd is rejected")
    error("parse-url",          {"text": "sftp://host/path",                      "context": ctx(user="")},                "invalid_context",        "empty current_os_user is rejected")

    # not reasonably testable: "public operations do not print to stdout or stderr"
    # -- the tools/call surface carries only parsed results and error objects;
    #    raw subprocess streams are not observable through this surface.


def main() -> int:
    failures: list[str] = []
    proc: subprocess.Popen[str] | None = None
    port: int | None = None
    try:
        proc, port = launch_mcp()
        with socket.create_connection(("127.0.0.1", port), timeout=10) as sock:
            run(Client(sock), failures)
    except Exception as exc:
        failures.append(f"test harness error: {exc!r}")
    finally:
        if proc is not None and port is not None:
            shutdown_mcp(proc, port)

    if failures:
        print("FAIL")
        for i, f in enumerate(failures, 1):
            print(f"{i}. {f}")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
