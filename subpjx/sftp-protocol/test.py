#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import base64
import json
import socket
import subprocess
import sys
import tempfile
import threading
import time
import uuid
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync/subpjx/sftp-protocol")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
MCP_JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/subpjx/sftp-protocol/released/sftp-protocol_MCP.jar")

SFTP_HOST = "ordinarydata.com"
SFTP_USER = "ace"
SFTP_PORT = 22
SFTP_BASE = "/tmp/testks"

failures: list[str] = []
_rpc_id = 0


def next_id() -> int:
    global _rpc_id
    _rpc_id += 1
    return _rpc_id


def uid() -> str:
    return uuid.uuid4().hex[:8]


def new_root(label: str) -> str:
    return f"{SFTP_BASE}/{label}_{uid()}"


def make_loc(root_path: str, host: str = SFTP_HOST, port: int | None = SFTP_PORT) -> dict:
    loc = {"user": SFTP_USER, "host": host, "root_path": root_path}
    if port is not None:
        loc["port"] = port
    return loc


DEFAULT_SETTINGS = {
    "max_connections": 5,
    "connect_timeout": "PT30S",
    "idle_keep_alive_ttl": "PT30S",
}
DEFAULT_AUTH: dict = {}

# ---------- MCP harness ----------

def _drain(stream, buf: list | None = None) -> None:
    for line in stream:
        if buf is not None:
            buf.append(line)


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
    threading.Thread(target=_drain, args=(proc.stderr, stderr_buf), daemon=True).start()
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
            proc.wait()
        raise RuntimeError(
            "MCP server did not advertise MCP_PORT\n"
            f"stdout:\n{''.join(stdout_buf)}\nstderr:\n{''.join(stderr_buf)}"
        )
    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    return proc, port


def rpc(sock: socket.socket, method: str, params=None, timeout: float = 30.0) -> dict:
    msg: dict = {"jsonrpc": "2.0", "id": next_id(), "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg, separators=(",", ":")) + "\n").encode("utf-8"))
    data = b""
    deadline = time.time() + timeout
    while b"\n" not in data and time.time() < deadline:
        try:
            chunk = sock.recv(65536)
        except socket.timeout:
            continue
        if not chunk:
            break
        data += chunk
    line, _, _ = data.partition(b"\n")
    if not line:
        return {"error": {"code": -1, "message": "no response received within timeout"}}
    try:
        return json.loads(line.decode("utf-8"))
    except json.JSONDecodeError as exc:
        return {"error": {"code": -1, "message": f"JSON parse error: {exc}"}}


def call(sock: socket.socket, tool: str, args: dict, timeout: float = 30.0) -> dict:
    return rpc(sock, "tools/call", {"name": tool, "arguments": args}, timeout=timeout)


def shutdown_mcp(proc: subprocess.Popen, port: int) -> None:
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=3) as s:
            s.settimeout(1.0)
            rpc(s, "aitc/shutdown")
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


# ---------- result helpers ----------

def is_error(resp: dict) -> bool:
    if "error" in resp:
        return True
    return bool(resp.get("result", {}).get("isError", False))


def get_text(resp: dict) -> str:
    if "error" in resp:
        err = resp["error"]
        return err.get("message", str(err))
    content = resp.get("result", {}).get("content", [])
    return content[0].get("text", "") if content else ""


def parse_dict(resp: dict) -> dict:
    try:
        data = json.loads(get_text(resp))
        return data if isinstance(data, dict) else {}
    except Exception:
        return {}


def parse_list(resp: dict) -> list:
    try:
        data = json.loads(get_text(resp))
        return data if isinstance(data, list) else []
    except Exception:
        return []


# ---------- assertion helpers ----------

def check(cond: bool, msg: str) -> None:
    if cond:
        print(f"OK  {msg}", flush=True)
    else:
        failures.append(msg)
        print(f"FAIL {msg}", flush=True)


def check_error(resp: dict, category: str, msg: str) -> None:
    if not is_error(resp):
        failures.append(f"{msg}: expected '{category}' error, got success: {get_text(resp)[:120]}")
        print(f"FAIL {msg}", flush=True)
        return
    text = get_text(resp).lower()
    check(category in text, f"{msg} -- category={category}")


# ---------- tests ----------

def test_basic_filesystem(sock: socket.socket) -> None:
    root = new_root("basic")
    r = call(sock, "open_unpooled", {
        "location": make_loc(root),
        "settings": DEFAULT_SETTINGS,
        "auth_config": DEFAULT_AUTH,
    })
    check(not is_error(r), "open_unpooled: passwordless auth succeeds for known host")
    if is_error(r):
        print(f"  (skipping: {get_text(r)[:200]})", flush=True)
        return
    fs_id = parse_dict(r).get("filesystem_id", "")
    check(bool(fs_id), "open_unpooled returns filesystem_id")

    # create_dir: creates missing parents, idempotent
    check(not is_error(call(sock, "create_dir", {"filesystem_id": fs_id, "path": ""})),
          "create_dir: root (empty path)")
    check(not is_error(call(sock, "create_dir", {"filesystem_id": fs_id, "path": "a/b/c"})),
          "create_dir: creates nested missing parents")
    check(not is_error(call(sock, "create_dir", {"filesystem_id": fs_id, "path": "a/b/c"})),
          "create_dir: idempotent on existing directory")

    # stat on directory
    r = call(sock, "stat", {"filesystem_id": fs_id, "path": "a/b"})
    check(not is_error(r), "stat: existing directory")
    d = parse_dict(r)
    check(d.get("is_dir") is True, "stat: is_dir=true for directory")
    check(d.get("byte_size") == -1, "stat: byte_size=-1 for directory")

    # open_write / write / close_write -- binary content (all 256 byte values)
    binary_content = bytes(range(256))
    b64_in = base64.b64encode(binary_content).decode("ascii")
    r = call(sock, "open_write", {"filesystem_id": fs_id, "path": "a/b/c/data.bin"})
    check(not is_error(r), "open_write: creates file with parent dirs present")
    wh_id = parse_dict(r).get("write_handle_id", "")
    check(not is_error(call(sock, "write", {
        "filesystem_id": fs_id, "write_handle_id": wh_id, "data": b64_in,
    })), "write: binary content (all 256 byte values)")
    check(not is_error(call(sock, "close_write", {
        "filesystem_id": fs_id, "write_handle_id": wh_id,
    })), "close_write: flushes and closes")

    # open_write creates missing parent directories
    r = call(sock, "open_write", {"filesystem_id": fs_id, "path": "newdir/sub/file.txt"})
    check(not is_error(r), "open_write: creates missing parent directories")
    if not is_error(r):
        wh2 = parse_dict(r).get("write_handle_id", "")
        call(sock, "write", {"filesystem_id": fs_id, "write_handle_id": wh2, "data": ""})
        call(sock, "close_write", {"filesystem_id": fs_id, "write_handle_id": wh2})

    # open_read / read / close_read -- binary round-trip
    r = call(sock, "open_read", {"filesystem_id": fs_id, "path": "a/b/c/data.bin"})
    check(not is_error(r), "open_read: opens written file")
    rh_id = parse_dict(r).get("read_handle_id", "")
    r = call(sock, "read", {"filesystem_id": fs_id, "read_handle_id": rh_id, "max_bytes": 65536})
    rd = parse_dict(r)
    got_bytes = base64.b64decode(rd.get("data", "")) if rd.get("data") else b""
    check(got_bytes == binary_content,
          "read: binary round-trip without text conversion (all 256 byte values)")
    check(not is_error(call(sock, "close_read", {
        "filesystem_id": fs_id, "read_handle_id": rh_id,
    })), "close_read")

    # set_mod_time, stat, list_dir report metadata
    mod_time = "2026-05-15T10:30:00Z"
    check(not is_error(call(sock, "set_mod_time", {
        "filesystem_id": fs_id, "path": "a/b/c/data.bin", "instant": mod_time,
    })), "set_mod_time")
    r = call(sock, "stat", {"filesystem_id": fs_id, "path": "a/b/c/data.bin"})
    check(not is_error(r), "stat: regular file after set_mod_time")
    entry = parse_dict(r)
    check(entry.get("name") == "data.bin", "stat: name is final path component")
    check(entry.get("is_dir") is False, "stat: is_dir=false for regular file")
    check(entry.get("byte_size") == 256, "stat: byte_size matches written content")
    check("mod_time" in entry, "stat: mod_time field present")
    check(str(entry.get("mod_time", "")).startswith("2026-05-15"),
          "stat: mod_time reflects set value (allowing server precision rounding)")

    r = call(sock, "list_dir", {"filesystem_id": fs_id, "path": "a/b/c"})
    check(not is_error(r), "list_dir: directory with one file")
    entries = parse_list(r)
    names = [e.get("name") for e in entries]
    check("data.bin" in names, "list_dir: includes written file")
    check(all("name" in e and "is_dir" in e and "mod_time" in e and "byte_size" in e
              for e in entries),
          "list_dir: each entry has name, is_dir, mod_time, byte_size")

    # list_dir on root (empty string) lists only immediate children
    r = call(sock, "list_dir", {"filesystem_id": fs_id, "path": ""})
    check(not is_error(r), "list_dir: root (empty string path)")
    root_entries = parse_list(r)
    root_names = [e.get("name") for e in root_entries]
    check("a" in root_names, "list_dir root: immediate child 'a' present")
    check("a/b" not in root_names, "list_dir: immediate children only, no nested paths")

    # rename
    check(not is_error(call(sock, "rename", {
        "filesystem_id": fs_id, "src": "a/b/c/data.bin", "dst": "a/b/c/renamed.bin",
    })), "rename: moves file")
    check_error(call(sock, "stat", {"filesystem_id": fs_id, "path": "a/b/c/data.bin"}),
                "not_found", "stat on renamed-away source returns not_found")
    check(not is_error(call(sock, "stat", {"filesystem_id": fs_id, "path": "a/b/c/renamed.bin"})),
          "stat: rename destination exists")

    # delete_file
    check(not is_error(call(sock, "delete_file", {
        "filesystem_id": fs_id, "path": "a/b/c/renamed.bin",
    })), "delete_file")
    check_error(call(sock, "stat", {"filesystem_id": fs_id, "path": "a/b/c/renamed.bin"}),
                "not_found", "stat after delete_file returns not_found")

    # delete_dir (now empty)
    check(not is_error(call(sock, "delete_dir", {"filesystem_id": fs_id, "path": "a/b/c"})),
          "delete_dir: removes empty directory")
    check_error(call(sock, "stat", {"filesystem_id": fs_id, "path": "a/b/c"}),
                "not_found", "stat after delete_dir returns not_found")

    check(not is_error(call(sock, "close_filesystem", {"filesystem_id": fs_id})),
          "close_filesystem: unpooled session")


def test_host_key_verification(sock: socket.socket) -> None:
    # Known host verified by open_unpooled success in test_basic_filesystem.
    # Unknown host: empty known_hosts -> host_key_rejected.
    tmp = tempfile.NamedTemporaryFile(mode="w", suffix=".known_hosts", delete=False)
    tmp.close()
    empty_kh = Path(tmp.name)
    try:
        r = call(sock, "open_unpooled", {
            "location": make_loc(new_root("hkv")),
            "settings": DEFAULT_SETTINGS,
            "auth_config": {"known_hosts_path": str(empty_kh)},
        })
        check_error(r, "host_key_rejected",
                    "open_unpooled with empty known_hosts returns host_key_rejected")
    finally:
        empty_kh.unlink(missing_ok=True)


def test_invalid_paths(sock: socket.socket) -> None:
    root = new_root("invpath")
    r = call(sock, "open_unpooled", {
        "location": make_loc(root), "settings": DEFAULT_SETTINGS, "auth_config": DEFAULT_AUTH,
    })
    if is_error(r):
        failures.append("test_invalid_paths: could not open filesystem")
        return
    fs_id = parse_dict(r).get("filesystem_id", "")
    call(sock, "create_dir", {"filesystem_id": fs_id, "path": ""})

    check_error(call(sock, "stat", {"filesystem_id": fs_id, "path": "/absolute/path"}),
                "invalid_path", "stat: absolute path returns invalid_path")
    check_error(call(sock, "stat", {"filesystem_id": fs_id, "path": "foo/../bar"}),
                "invalid_path", "stat: path with .. segment returns invalid_path")
    check_error(call(sock, "stat", {"filesystem_id": fs_id, "path": "foo\x00bar"}),
                "invalid_path", "stat: path with NUL byte returns invalid_path")

    call(sock, "close_filesystem", {"filesystem_id": fs_id})


def test_error_categories(sock: socket.socket) -> None:
    root = new_root("errcats")
    r = call(sock, "open_unpooled", {
        "location": make_loc(root), "settings": DEFAULT_SETTINGS, "auth_config": DEFAULT_AUTH,
    })
    if is_error(r):
        failures.append("test_error_categories: could not open filesystem")
        return
    fs_id = parse_dict(r).get("filesystem_id", "")
    call(sock, "create_dir", {"filesystem_id": fs_id, "path": ""})

    check_error(call(sock, "stat", {"filesystem_id": fs_id, "path": "no_such_file.txt"}),
                "not_found", "stat on missing path returns not_found")
    check_error(call(sock, "list_dir", {"filesystem_id": fs_id, "path": "no_such_dir"}),
                "not_found", "list_dir on missing path returns not_found")
    check_error(call(sock, "open_read", {"filesystem_id": fs_id, "path": "no_such_file.txt"}),
                "not_found", "open_read on missing file returns not_found")
    check_error(call(sock, "delete_file", {"filesystem_id": fs_id, "path": "no_such_file.txt"}),
                "not_found", "delete_file on missing file returns not_found")

    # permission_denied: not reasonably testable without a pre-arranged server fixture

    call(sock, "close_filesystem", {"filesystem_id": fs_id})

    # io_error / connection failure for unreachable endpoint (port 1 = discard, always refused)
    r = call(sock, "open_unpooled", {
        "location": make_loc("/tmp", host=SFTP_HOST, port=1),
        "settings": {"max_connections": 1, "connect_timeout": "PT3S", "idle_keep_alive_ttl": "PT5S"},
        "auth_config": DEFAULT_AUTH,
    }, timeout=15.0)
    check(is_error(r), "open_unpooled to port 1 (refused) returns an error")
    if is_error(r):
        text = get_text(r).lower()
        check(any(c in text for c in ("io_error", "host_key_rejected", "authentication_failed")),
              f"unreachable endpoint error is a connection-related category (got: {get_text(r)[:120]})")


def test_pool_sharing(sock: socket.socket) -> None:
    root_a = new_root("pshare_a")
    root_b = new_root("pshare_b")
    settings = {"max_connections": 2, "connect_timeout": "PT30S", "idle_keep_alive_ttl": "PT30S"}
    later_settings = {"max_connections": 9, "connect_timeout": "PT3S", "idle_keep_alive_ttl": "PT3S"}

    r = call(sock, "create_pool_registry", {})
    if is_error(r):
        failures.append("test_pool_sharing: create_pool_registry failed")
        return
    reg_id = parse_dict(r).get("registry_id", "")

    r_a = call(sock, "pool_for", {
        "registry_id": reg_id,
        "location": make_loc(root_a, host=SFTP_HOST.lower(), port=None),
        "settings": settings,
        "auth_config": DEFAULT_AUTH,
    })
    r_b = call(sock, "pool_for", {
        "registry_id": reg_id,
        "location": make_loc(root_b, host=SFTP_HOST.upper()),
        "settings": later_settings,
        "auth_config": DEFAULT_AUTH,
    })
    if is_error(r_a) or is_error(r_b):
        failures.append("test_pool_sharing: pool_for failed")
        call(sock, "close_pool_registry", {"registry_id": reg_id})
        return

    pool_id_a = parse_dict(r_a).get("pool_id", "")
    pool_id_b = parse_dict(r_b).get("pool_id", "")
    check(bool(pool_id_a) and pool_id_a == pool_id_b,
          "pool_for: same user/lowercased host/normalized port shares one pool despite different roots")

    ra = call(sock, "acquire", {"pool_id": pool_id_a})
    rb = call(sock, "acquire", {"pool_id": pool_id_b})
    check(not is_error(ra) and not is_error(rb),
          "shared pool: can hold two connections (max_connections=2)")
    fs_a = parse_dict(ra).get("filesystem_id", "")
    fs_b = parse_dict(rb).get("filesystem_id", "")
    if fs_a:
        call(sock, "close_pooled_filesystem", {"filesystem_id": fs_a})
    if fs_b:
        call(sock, "close_pooled_filesystem", {"filesystem_id": fs_b})

    r = call(sock, "get_pool_events", {"pool_id": pool_id_a})
    events = parse_list(r)
    if not is_error(r) and events:
        check(all(e.get("endpoint") == f"{SFTP_USER}@{SFTP_HOST}:22" for e in events),
              "pool events: endpoint uses lowercased host and normalized default port")
        check(all(e.get("max_connections") == 2 for e in events),
              "pool_for: later calls for same key do not change first max_connections")
    else:
        failures.append("test_pool_sharing: could not observe pool events for endpoint/settings")

    check(not is_error(call(sock, "close_pool_registry", {"registry_id": reg_id})),
          "close_pool_registry: closes registry")
    check(not is_error(call(sock, "close_pool_registry", {"registry_id": reg_id})),
          "close_pool_registry: idempotent")


def test_pool_events(sock: socket.socket) -> None:
    root = new_root("pevents")
    settings = {"max_connections": 2, "connect_timeout": "PT30S", "idle_keep_alive_ttl": "PT30S"}

    r = call(sock, "create_pool_registry", {})
    if is_error(r):
        failures.append("test_pool_events: create_pool_registry failed")
        return
    reg_id = parse_dict(r).get("registry_id", "")

    r = call(sock, "pool_for", {
        "registry_id": reg_id,
        "location": make_loc(root),
        "settings": settings,
        "auth_config": DEFAULT_AUTH,
    })
    if is_error(r):
        failures.append(f"test_pool_events: pool_for failed: {get_text(r)[:120]}")
        call(sock, "close_pool_registry", {"registry_id": reg_id})
        return
    pool_id = parse_dict(r).get("pool_id", "")

    # acquire x2, release x2 -> at least 4 events
    r1 = call(sock, "acquire", {"pool_id": pool_id})
    r2 = call(sock, "acquire", {"pool_id": pool_id})
    if is_error(r1) or is_error(r2):
        failures.append("test_pool_events: acquire failed")
        call(sock, "close_pool_registry", {"registry_id": reg_id})
        return
    fs1 = parse_dict(r1).get("filesystem_id", "")
    fs2 = parse_dict(r2).get("filesystem_id", "")
    call(sock, "close_pooled_filesystem", {"filesystem_id": fs1})
    call(sock, "close_pooled_filesystem", {"filesystem_id": fs2})

    r = call(sock, "get_pool_events", {"pool_id": pool_id})
    check(not is_error(r), "get_pool_events succeeds")
    events = parse_list(r)
    check(len(events) >= 4,
          f"pool events: at least 4 events (2 acquire + 2 release), got {len(events)}")
    if events:
        ev = events[0]
        check("endpoint" in ev, "pool event: has endpoint field")
        check("open_connections" in ev, "pool event: has open_connections field")
        check("max_connections" in ev, "pool event: has max_connections field")
        endpoint = str(ev.get("endpoint", ""))
        check(endpoint == f"{SFTP_USER}@{SFTP_HOST}:22",
              f"pool event endpoint is exact user@host:port pool key (got {endpoint!r})")
        check(all(e.get("max_connections") == 2 for e in events),
              "pool events: max_connections=2 throughout")
        open_counts = [e.get("open_connections") for e in events]
        check(1 in open_counts and 2 in open_counts,
              f"pool events: open_connections spans 1 and 2 (got {open_counts})")

    call(sock, "close_pool_registry", {"registry_id": reg_id})


def test_pool_blocking(sock: socket.socket, port: int) -> None:
    root = new_root("blocking")
    settings = {"max_connections": 1, "connect_timeout": "PT30S", "idle_keep_alive_ttl": "PT30S"}

    r = call(sock, "create_pool_registry", {})
    if is_error(r):
        failures.append("test_pool_blocking: create_pool_registry failed")
        return
    reg_id = parse_dict(r).get("registry_id", "")

    r = call(sock, "pool_for", {
        "registry_id": reg_id,
        "location": make_loc(root),
        "settings": settings,
        "auth_config": DEFAULT_AUTH,
    })
    if is_error(r):
        failures.append(f"test_pool_blocking: pool_for failed: {get_text(r)[:120]}")
        call(sock, "close_pool_registry", {"registry_id": reg_id})
        return
    pool_id = parse_dict(r).get("pool_id", "")

    r = call(sock, "acquire", {"pool_id": pool_id})
    if is_error(r):
        failures.append(f"test_pool_blocking: first acquire failed: {get_text(r)[:120]}")
        call(sock, "close_pool_registry", {"registry_id": reg_id})
        return
    fs1_id = parse_dict(r).get("filesystem_id", "")

    # Second acquire on a separate socket connection -- must block
    acquired = threading.Event()
    second_resp: list[dict] = []

    def do_second() -> None:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=5) as s2:
                s2.settimeout(1.0)
                second_resp.append(call(s2, "acquire", {"pool_id": pool_id}, timeout=20.0))
        except Exception as exc:
            second_resp.append({"_exc": str(exc)})
        finally:
            acquired.set()

    t = threading.Thread(target=do_second, daemon=True)
    t.start()
    time.sleep(1.5)
    check(not acquired.is_set(),
          "pool max_connections=1: second acquire blocks while first is held")

    call(sock, "close_pooled_filesystem", {"filesystem_id": fs1_id})
    acquired.wait(timeout=15.0)

    ok = bool(second_resp) and "_exc" not in second_resp[0] and not is_error(second_resp[0])
    check(ok, "pool max_connections=1: second acquire succeeds after first is released")
    if ok:
        fs2_id = parse_dict(second_resp[0]).get("filesystem_id", "")
        if fs2_id:
            call(sock, "close_pooled_filesystem", {"filesystem_id": fs2_id})

    call(sock, "close_pool_registry", {"registry_id": reg_id})


def test_pool_capacity_no_leak(sock: socket.socket) -> None:
    root = new_root("noleak")
    settings = {"max_connections": 1, "connect_timeout": "PT30S", "idle_keep_alive_ttl": "PT30S"}

    r = call(sock, "create_pool_registry", {})
    if is_error(r):
        failures.append("test_pool_capacity_no_leak: create_pool_registry failed")
        return
    reg_id = parse_dict(r).get("registry_id", "")

    r = call(sock, "pool_for", {
        "registry_id": reg_id,
        "location": make_loc(root),
        "settings": settings,
        "auth_config": DEFAULT_AUTH,
    })
    if is_error(r):
        failures.append(f"test_pool_capacity_no_leak: pool_for failed: {get_text(r)[:120]}")
        call(sock, "close_pool_registry", {"registry_id": reg_id})
        return
    pool_id = parse_dict(r).get("pool_id", "")

    r = call(sock, "acquire", {"pool_id": pool_id}, timeout=45.0)
    if is_error(r) and "timed out" in get_text(r).lower():
        r = call(sock, "acquire", {"pool_id": pool_id}, timeout=45.0)
    if is_error(r):
        failures.append(f"test_pool_capacity_no_leak: acquire failed: {get_text(r)[:120]}")
        call(sock, "close_pool_registry", {"registry_id": reg_id})
        return
    fs_id = parse_dict(r).get("filesystem_id", "")
    call(sock, "create_dir", {"filesystem_id": fs_id, "path": ""})
    # Trigger a not_found error (does not make the connection unusable)
    call(sock, "stat", {"filesystem_id": fs_id, "path": "nonexistent.txt"})
    call(sock, "close_pooled_filesystem", {"filesystem_id": fs_id})

    # Capacity must not be permanently reduced -- re-acquire must succeed immediately
    r = call(sock, "acquire", {"pool_id": pool_id}, timeout=10.0)
    check(not is_error(r),
          "pool capacity not leaked: re-acquire succeeds after failed operation + release")
    if not is_error(r):
        fs2_id = parse_dict(r).get("filesystem_id", "")
        if fs2_id:
            call(sock, "close_pooled_filesystem", {"filesystem_id": fs2_id})

    call(sock, "close_pool_registry", {"registry_id": reg_id})


def test_idle_ttl(sock: socket.socket) -> None:
    root = new_root("idle_ttl")
    idle_secs = 4
    settings = {
        "max_connections": 1,
        "connect_timeout": "PT30S",
        "idle_keep_alive_ttl": f"PT{idle_secs}S",
    }

    r = call(sock, "create_pool_registry", {})
    if is_error(r):
        failures.append("test_idle_ttl: create_pool_registry failed")
        return
    reg_id = parse_dict(r).get("registry_id", "")

    r = call(sock, "pool_for", {
        "registry_id": reg_id,
        "location": make_loc(root),
        "settings": settings,
        "auth_config": DEFAULT_AUTH,
    })
    if is_error(r):
        failures.append(f"test_idle_ttl: pool_for failed: {get_text(r)[:120]}")
        call(sock, "close_pool_registry", {"registry_id": reg_id})
        return
    pool_id = parse_dict(r).get("pool_id", "")

    r = call(sock, "acquire", {"pool_id": pool_id})
    if is_error(r):
        failures.append(f"test_idle_ttl: acquire failed: {get_text(r)[:120]}")
        call(sock, "close_pool_registry", {"registry_id": reg_id})
        return
    fs_id = parse_dict(r).get("filesystem_id", "")
    # Release into idle state
    call(sock, "close_pooled_filesystem", {"filesystem_id": fs_id})

    # Wait for idle TTL to expire
    time.sleep(idle_secs + 3)

    r = call(sock, "get_pool_events", {"pool_id": pool_id})
    check(not is_error(r), "test_idle_ttl: get_pool_events after TTL")
    events = parse_list(r)
    if events:
        open_counts = [e.get("open_connections") for e in events]
        check(0 in open_counts,
              f"idle connection closed after {idle_secs}s TTL: open_connections=0 event observed (counts={open_counts})")
    else:
        failures.append("test_idle_ttl: no pool events recorded")

    call(sock, "close_pool_registry", {"registry_id": reg_id})


# not reasonably testable: list_dir/stat symlink exclusion -- the SFTP API (and
# its MCP wrapper) does not expose symlink creation, so symlink-exclusion
# behavior cannot be verified through tools/call alone.

# ---------- main ----------

def main() -> None:
    proc, port = launch_mcp()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=5) as sock:
            sock.settimeout(1.0)
            test_basic_filesystem(sock)
            test_host_key_verification(sock)
            test_invalid_paths(sock)
            test_error_categories(sock)
            test_pool_sharing(sock)
            test_pool_events(sock)
            test_pool_blocking(sock, port)
            test_pool_capacity_no_leak(sock)
            test_idle_ttl(sock)
    finally:
        shutdown_mcp(proc, port)

    if failures:
        print(f"\n{len(failures)} failure(s):", flush=True)
        for f in failures:
            print(f"  - {f}", flush=True)
        sys.exit(1)
    else:
        print("\nAll checks passed.", flush=True)


if __name__ == "__main__":
    main()
