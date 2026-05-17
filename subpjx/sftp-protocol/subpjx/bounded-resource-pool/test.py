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

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
MCP_JAR = Path(
    "C:/Users/human/Desktop/prjx/kitchensync/subpjx/sftp-protocol/subpjx/bounded-resource-pool"
    "/released/bounded-resource-pool_MCP.jar"
)

failures: list[str] = []


def check(name: str, cond: bool, msg: str = "") -> None:
    label = "PASS" if cond else "FAIL"
    print(f"{label} [{name}]" + (f": {msg}" if msg else ""))
    if not cond:
        failures.append(f"FAIL [{name}]: {msg}")


def is_err(resp: dict) -> bool:
    if "error" in resp:
        return True
    t = _text(resp).lower()
    return any(w in t for w in ("error", "exception", "invalid", "illegal", "closed", "fail"))


def _text(resp: dict) -> str:
    content = resp.get("result", {}).get("content", [])
    return content[0].get("text", "") if content else ""


def _json(resp: dict) -> dict:
    try:
        v = json.loads(_text(resp))
        return v if isinstance(v, dict) else {}
    except (json.JSONDecodeError, TypeError):
        return {}


def _list(resp: dict) -> list:
    try:
        v = json.loads(_text(resp))
        return v if isinstance(v, list) else []
    except (json.JSONDecodeError, TypeError):
        return []


# ── MCP harness ───────────────────────────────────────────────────────────────

def _drain(stream, sink=None):
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
            proc.wait(timeout=2)
        raise RuntimeError(
            f"MCP_PORT not seen\nstdout: {''.join(stdout_buf)}\nstderr: {''.join(stderr_buf)}"
        )
    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    return proc, port


_rid = 0


def rpc(sock: socket.socket, method: str, params=None, rpc_id: int | None = None) -> dict:
    global _rid
    if rpc_id is None:
        _rid += 1
        rpc_id = _rid
    msg: dict = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg, separators=(",", ":")) + "\n").encode("utf-8"))
    data = b""
    deadline = time.time() + 15
    while b"\n" not in data and time.time() < deadline:
        chunk = sock.recv(65536)
        if not chunk:
            break
        data += chunk
    line, _, _ = data.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def call(sock: socket.socket, tool: str, args: dict | None = None) -> dict:
    p: dict = {"name": tool}
    if args is not None:
        p["arguments"] = args
    return rpc(sock, "tools/call", p)


def conn(port: int) -> socket.socket:
    return socket.create_connection(("127.0.0.1", port), timeout=5)


def shutdown_mcp(proc: subprocess.Popen, port: int) -> None:
    try:
        with conn(port) as s:
            rpc(s, "aitc/shutdown", rpc_id=9999)
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


# ── tests ─────────────────────────────────────────────────────────────────────

def t_settings_validation(port: int) -> None:
    with conn(port) as s:
        r0 = call(s, "pool_for", {"key": "sv0", "max_resources": 0, "idle_keep_alive_ttl": "PT1S"})
        check("settings-max-zero-rejected", is_err(r0), f"got: {_text(r0)!r}")
        r1 = call(s, "pool_for", {"key": "sv1", "max_resources": -1, "idle_keep_alive_ttl": "PT1S"})
        check("settings-max-negative-rejected", is_err(r1), f"got: {_text(r1)!r}")
        r2 = call(s, "pool_for", {"key": "sv2", "max_resources": 1, "idle_keep_alive_ttl": "PT0S"})
        check("settings-ttl-zero-rejected", is_err(r2), f"got: {_text(r2)!r}")
        r3 = call(s, "pool_for", {"key": "sv3", "max_resources": 1, "idle_keep_alive_ttl": "-PT1S"})
        check("settings-ttl-negative-rejected", is_err(r3), f"got: {_text(r3)!r}")


def t_same_key_pool_immutable(port: int) -> None:
    with conn(port) as s:
        call(s, "pool_for", {"key": "sk", "max_resources": 1, "idle_keep_alive_ttl": "PT30S"})
        # Later call with different settings must be ignored.
        call(s, "pool_for", {
            "key": "sk",
            "max_resources": 99,
            "idle_keep_alive_ttl": "PT0.2S",
            "fail_open_count": 1,
        })
        a = _json(call(s, "acquire", {"key": "sk"}))
        check("same-key-acquire-ok", bool(a.get("lease_id")), f"acquire: {a}")
        r1 = a.get("resource")
        call(s, "lease_close", {"lease_id": a.get("lease_id")})
        time.sleep(0.6)
        b = _json(call(s, "acquire", {"key": "sk"}))
        check("same-key-ttl-unchanged", r1 == b.get("resource"),
              f"later pool_for changed ttl; first={r1!r} second={b.get('resource')!r}")
        evs = _list(call(s, "get_events", {"key": "sk"}))
        if evs:
            check("same-key-max-unchanged",
                  evs[-1].get("max_resources") == 1,
                  f"max changed after second pool_for; last event: {evs[-1]}")
        call(s, "lease_close", {"lease_id": b.get("lease_id")})


def t_resource_reuse(port: int) -> None:
    with conn(port) as s:
        call(s, "pool_for", {"key": "rr", "max_resources": 2, "idle_keep_alive_ttl": "PT30S"})
        a1 = _json(call(s, "acquire", {"key": "rr"}))
        call(s, "lease_close", {"lease_id": a1.get("lease_id")})
        a2 = _json(call(s, "acquire", {"key": "rr"}))
        check("resource-reuse",
              a1.get("resource") is not None and a1.get("resource") == a2.get("resource"),
              f"r1={a1.get('resource')!r} r2={a2.get('resource')!r}")
        call(s, "lease_close", {"lease_id": a2.get("lease_id")})


def t_independent_keys(port: int) -> None:
    with conn(port) as s:
        call(s, "pool_for", {"key": "ik-A", "max_resources": 1, "idle_keep_alive_ttl": "PT30S"})
        call(s, "pool_for", {"key": "ik-B", "max_resources": 1, "idle_keep_alive_ttl": "PT30S"})
        aA = _json(call(s, "acquire", {"key": "ik-A"}))
        # Exhausting ik-A must not affect ik-B capacity.
        aB = _json(call(s, "acquire", {"key": "ik-B"}))
        check("independent-keys",
              bool(aA.get("lease_id")) and bool(aB.get("lease_id")),
              f"lA={aA.get('lease_id')} lB={aB.get('lease_id')}")
        call(s, "lease_close", {"lease_id": aA.get("lease_id")})
        call(s, "lease_close", {"lease_id": aB.get("lease_id")})
        againA = _json(call(s, "acquire", {"key": "ik-A"}))
        againB = _json(call(s, "acquire", {"key": "ik-B"}))
        check("independent-idle-A",
              againA.get("resource") == aA.get("resource"),
              f"key A did not keep its own idle resource: {againA}")
        check("independent-idle-B",
              againB.get("resource") == aB.get("resource"),
              f"key B did not keep its own idle resource: {againB}")
        check("independent-idle-not-crossed",
              againA.get("resource") != againB.get("resource"),
              f"keys reused the same idle resource: A={againA} B={againB}")
        call(s, "lease_close", {"lease_id": againA.get("lease_id")})
        call(s, "lease_close", {"lease_id": againB.get("lease_id")})


def t_invalidate(port: int) -> None:
    with conn(port) as s:
        call(s, "pool_for", {"key": "inv", "max_resources": 1, "idle_keep_alive_ttl": "PT30S"})
        a1 = _json(call(s, "acquire", {"key": "inv"}))
        l1, r1 = a1.get("lease_id"), a1.get("resource")
        call(s, "lease_invalidate", {"lease_id": l1})
        call(s, "lease_close", {"lease_id": l1})
        a2 = _json(call(s, "acquire", {"key": "inv"}))
        l2, r2 = a2.get("lease_id"), a2.get("resource")
        check("invalidate-new-resource", r1 != r2, f"r1={r1!r} r2={r2!r}")
        check("invalidate-capacity-not-leaked", bool(l2),
              "acquire after invalidate failed -- capacity leaked")
        if l2:
            call(s, "lease_close", {"lease_id": l2})


def t_events(port: int) -> None:
    with conn(port) as s:
        call(s, "pool_for", {"key": "ev", "max_resources": 2, "idle_keep_alive_ttl": "PT30S"})
        a1 = _json(call(s, "acquire", {"key": "ev"}))
        a2 = _json(call(s, "acquire", {"key": "ev"}))
        call(s, "lease_close", {"lease_id": a1.get("lease_id")})
        call(s, "lease_close", {"lease_id": a2.get("lease_id")})
        evs = _list(call(s, "get_events", {"key": "ev"}))
        check("events-is-list", isinstance(evs, list), f"type: {type(evs)}")
        if isinstance(evs, list):
            check("events-count-ge-4", len(evs) >= 4,
                  f"expected >=4 (2 acquires + 2 releases), got {len(evs)}: {evs}")
            for i, ev in enumerate(evs[:4]):
                check(f"event-{i}-key", ev.get("key") == "ev", f"key={ev.get('key')!r}")
                check(f"event-{i}-max", ev.get("max_resources") == 2,
                      f"max_resources={ev.get('max_resources')}")
                check(f"event-{i}-open-present", "open_resources" in ev, str(ev))


def t_idle_timeout(port: int) -> None:
    ttl_ms = 500
    with conn(port) as s:
        call(s, "pool_for", {"key": "ito", "max_resources": 1,
                              "idle_keep_alive_ttl": f"PT{ttl_ms / 1000}S"})
        a1 = _json(call(s, "acquire", {"key": "ito"}))
        l1, r1 = a1.get("lease_id"), a1.get("resource")
        call(s, "lease_close", {"lease_id": l1})
        time.sleep(ttl_ms / 1000 + 1.2)
        a2 = _json(call(s, "acquire", {"key": "ito"}))
        l2, r2 = a2.get("lease_id"), a2.get("resource")
        check("idle-timeout-new-resource", r1 != r2,
              f"expected new resource after {ttl_ms}ms TTL; r1={r1!r} r2={r2!r}")
        evs = _list(call(s, "get_events", {"key": "ito"}))
        if evs:
            timeout_events = [e for e in evs if e.get("open_resources") == 0]
            opens = [e.get("open_resources") for e in evs]
            check("idle-timeout-event-open-zero", 0 in opens,
                  f"expected event with open_resources=0; counts: {opens}")
            if timeout_events:
                ev = timeout_events[-1]
                check("idle-timeout-event-key", ev.get("key") == "ito", str(ev))
                check("idle-timeout-event-max", ev.get("max_resources") == 1, str(ev))
        if l2:
            call(s, "lease_close", {"lease_id": l2})


def t_idle_reset(port: int) -> None:
    ttl_ms = 800
    with conn(port) as s:
        call(s, "pool_for", {"key": "ir", "max_resources": 1,
                              "idle_keep_alive_ttl": f"PT{ttl_ms / 1000}S"})
        a1 = _json(call(s, "acquire", {"key": "ir"}))
        l1, r1 = a1.get("lease_id"), a1.get("resource")
        call(s, "lease_close", {"lease_id": l1})
        # Re-acquire at 40% of TTL -- resets the idle timer.
        time.sleep(ttl_ms / 1000 * 0.4)
        a2 = _json(call(s, "acquire", {"key": "ir"}))
        l2, r2 = a2.get("lease_id"), a2.get("resource")
        check("idle-reset-reuses-resource", r1 == r2, f"r1={r1!r} r2={r2!r}")
        call(s, "lease_close", {"lease_id": l2})
        # 40% of TTL since last release -- within TTL because timer was reset on re-acquire.
        time.sleep(ttl_ms / 1000 * 0.4)
        a3 = _json(call(s, "acquire", {"key": "ir"}))
        l3, r3 = a3.get("lease_id"), a3.get("resource")
        check("idle-reset-timer-reset", r1 == r3,
              f"timer not reset on reuse; r1={r1!r} r3={r3!r}")
        if l3:
            call(s, "lease_close", {"lease_id": l3})


def t_failed_open(port: int) -> None:
    with conn(port) as s:
        call(s, "pool_for", {
            "key": "fo", "max_resources": 1, "idle_keep_alive_ttl": "PT30S",
            "fail_open_count": 1,
        })
        r1 = call(s, "acquire", {"key": "fo"})
        if not is_err(r1):
            # not reasonably testable: no open-failure injection visible in this MCP surface
            return
        a2 = _json(call(s, "acquire", {"key": "fo"}))
        check("failed-open-no-capacity-leak", bool(a2.get("lease_id")),
              f"acquire after failed open failed -- capacity leaked: {a2}")
        if a2.get("lease_id"):
            call(s, "lease_close", {"lease_id": a2.get("lease_id")})


def t_blocking_acquire(port: int) -> None:
    with conn(port) as sA, conn(port) as sB:
        call(sA, "pool_for", {"key": "ba", "max_resources": 1, "idle_keep_alive_ttl": "PT30S"})
        a1 = _json(call(sA, "acquire", {"key": "ba"}))
        l1, r1 = a1.get("lease_id"), a1.get("resource")

        holder: list = []
        done = threading.Event()

        def do_acquire():
            try:
                holder.append(_json(call(sB, "acquire", {"key": "ba"})))
            except Exception as e:
                holder.append({"error": str(e)})
            done.set()

        threading.Thread(target=do_acquire, daemon=True).start()
        time.sleep(0.4)
        check("blocking-still-waiting", not done.is_set(),
              "second acquire returned before first lease was released")

        call(sA, "lease_close", {"lease_id": l1})
        done.wait(timeout=10)
        check("blocking-unblocked-after-release", done.is_set(),
              "second acquire never unblocked after release")

        if holder and isinstance(holder[0], dict):
            r2, l2 = holder[0].get("resource"), holder[0].get("lease_id")
            check("blocking-reuses-resource", r1 == r2, f"r1={r1!r} r2={r2!r}")
            if l2:
                call(sB, "lease_close", {"lease_id": l2})


def t_registry_close(port: int) -> None:
    with conn(port) as s:
        call(s, "pool_for", {"key": "rc", "max_resources": 1, "idle_keep_alive_ttl": "PT30S"})
        _json(call(s, "acquire", {"key": "rc"}))  # leave lease open intentionally

        call(s, "registry_close", {})
        call(s, "registry_close", {})  # idempotent -- must not throw

        after = call(s, "acquire", {"key": "rc"})
        check("registry-close-blocks-acquire", is_err(after),
              f"expected error on acquire after close; got: {_text(after)!r}")


def t_cancellation_no_leak(port: int) -> None:
    with conn(port) as sA:
        call(sA, "pool_for", {"key": "cn", "max_resources": 1, "idle_keep_alive_ttl": "PT30S"})
        a1 = _json(call(sA, "acquire", {"key": "cn"}))
        l1 = a1.get("lease_id")

        sB = conn(port)

        def do_blocked():
            try:
                call(sB, "acquire", {"key": "cn"})
            except Exception:
                pass

        t = threading.Thread(target=do_blocked, daemon=True)
        t.start()
        time.sleep(0.3)
        try:
            sB.close()
        except Exception:
            pass
        t.join(timeout=2)

        call(sA, "lease_close", {"lease_id": l1})
        time.sleep(0.3)  # allow server to notice sB disconnected

        with conn(port) as sC:
            a3 = _json(call(sC, "acquire", {"key": "cn"}))
            check("cancellation-no-capacity-leak", bool(a3.get("lease_id")),
                  f"acquire after disconnect failed -- capacity leaked: {a3}")
            if a3.get("lease_id"):
                call(sC, "lease_close", {"lease_id": a3.get("lease_id")})


# not reasonably testable: Java thread interrupt status is not observable through
# tools/call -- there is no MCP mechanism to interrupt a blocked server-side thread.
# not reasonably testable: ResourceFactory.close failure behavior is not
# observable through tools/call unless the wrapper exposes close-failure injection.
# not reasonably testable: listener failure handling is not observable through
# tools/call unless the wrapper exposes listener-failure injection.
# not reasonably testable: public Java API stdout/stderr silence is outside the
# tools/call surface; the MCP process stdout is reserved for wrapper transport.

def main() -> None:
    proc1, port1 = launch_mcp()
    try:
        t_settings_validation(port1)
        t_same_key_pool_immutable(port1)
        t_resource_reuse(port1)
        t_independent_keys(port1)
        t_invalidate(port1)
        t_events(port1)
        t_idle_timeout(port1)
        t_idle_reset(port1)
        t_failed_open(port1)
        t_blocking_acquire(port1)
        t_cancellation_no_leak(port1)
    finally:
        shutdown_mcp(proc1, port1)

    # Registry-close test gets its own server since it mutates global state.
    proc2, port2 = launch_mcp()
    try:
        t_registry_close(port2)
    finally:
        shutdown_mcp(proc2, port2)

    if failures:
        print(f"\n--- {len(failures)} failure(s) ---")
        for f in failures:
            print(f)
        sys.exit(1)
    else:
        print("\nAll checks passed.")


if __name__ == "__main__":
    main()
