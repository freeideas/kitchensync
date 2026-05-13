#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Pool identity, keying, and URL-to-pool mapping (01.1–01.5)."""

from __future__ import annotations

import getpass, json, os, shutil, socket, subprocess, sys, threading, time, uuid
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")
TESTKS = Path(PROJECT).resolve() / "tmp" / "testks" / "01-pool-identity"

_id_lock = threading.Lock()
_id_seq = 0


def _next_id():
    global _id_seq
    with _id_lock:
        _id_seq += 1
        return _id_seq


def _drain(stream):
    for _ in stream:
        pass


def _launch(env):
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
        env=env,
    )
    port = None
    deadline = time.time() + 30
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
        raise RuntimeError("MCP server did not advertise MCP_PORT")
    threading.Thread(target=_drain, args=(proc.stdout,), daemon=True).start()
    threading.Thread(target=_drain, args=(proc.stderr,), daemon=True).start()
    return proc, port


def _localhost_known_hosts():
    proc = subprocess.run(
        ["ssh-keyscan", "-T", "5", "localhost", "127.0.0.1"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        check=False,
    )
    lines = [line for line in proc.stdout.splitlines() if line.strip() and not line.startswith("#")]
    if not lines:
        raise RuntimeError("ssh-keyscan localhost returned no host keys")
    return "\n".join(lines) + "\n"


def _prepare_ssh_home():
    home = TESTKS / "home"
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir(parents=True)
    ssh_dir.chmod(0o700)

    real_ssh = Path.home() / ".ssh"
    for name in ["id_ed25519", "id_ecdsa", "id_rsa"]:
        src = real_ssh / name
        if src.exists():
            dst = ssh_dir / name
            shutil.copy2(src, dst)
            dst.chmod(0o600)
        pub = real_ssh / f"{name}.pub"
        if pub.exists():
            dst_pub = ssh_dir / f"{name}.pub"
            shutil.copy2(pub, dst_pub)
            dst_pub.chmod(0o644)

    known_hosts = ssh_dir / "known_hosts"
    known_hosts.write_text(_localhost_known_hosts(), encoding="utf-8", newline="\n")
    known_hosts.chmod(0o600)
    return home


def _env_for_home(home):
    env = os.environ.copy()
    prior = env.get("JAVA_TOOL_OPTIONS", "")
    env["JAVA_TOOL_OPTIONS"] = (prior + " " if prior else "") + f"-Duser.home={home}"
    env["HOME"] = str(home)
    return env


def _rpc(sock, method, params=None, timeout=10):
    rpc_id = _next_id()
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + timeout
    while time.time() < deadline:
        sock.settimeout(max(0.05, deadline - time.time()))
        try:
            chunk = sock.recv(8192)
        except (socket.timeout, OSError):
            break
        if not chunk:
            break
        buf += chunk
        while b"\n" in buf:
            line, _, buf = buf.partition(b"\n")
            response = json.loads(line.decode("utf-8"))
            if response.get("id") == rpc_id:
                return response
    return None


def _call(sock, tool, args, timeout=10):
    return _rpc(sock, "tools/call", {"name": tool, "arguments": args}, timeout=timeout)


def _hid(r):
    return (r or {}).get("result", {}).get("handleId")


def main() -> int:
    # Idempotency: clean up test state at the start
    if TESTKS.exists():
        shutil.rmtree(TESTKS)
    TESTKS.mkdir(parents=True)
    (TESTKS / "a").mkdir()
    (TESTKS / "b").mkdir()
    ssh_home = _prepare_ssh_home()

    os_user = getpass.getuser()
    proc, port = _launch(_env_for_home(ssh_home))
    extra_socks = []
    failures = []

    try:
        s = socket.create_connection(("127.0.0.1", port), timeout=10)
        extra_socks.append(s)

        tl = _rpc(s, "tools/list")
        tools = (tl.get("result") or {}).get("tools", [])
        tool_names = {t["name"] for t in tools}

        configure = "configure"
        acquire = "acquire"
        release = "release"
        shutdown = "shutdown"

        for tool in [configure, acquire, release, shutdown]:
            if tool not in tool_names:
                print(f"FATAL: no tool '{tool}' found in {sorted(tool_names)}")
                return 1

        # ── 01.1: same user+host, different paths → same pool ─────────────────
        # Create a pool with max 1 connection. Acquire A for path /a. Then try
        # to acquire B for path /b on a background thread — it must block because
        # both URLs share the same (user, localhost) pool key.
        r = _call(s, configure, {"maxConnections": 1, "connectTimeoutSeconds": 30,
                                 "idleKeepaliveSeconds": 60})
        if "result" not in (r or {}):
            print(f"FATAL: configure failed: {r}")
            return 1

        ra = _call(s, acquire, {"url": f"sftp://localhost/{TESTKS}/a"}, timeout=30)
        ha = _hid(ra)
        if ha is None:
            failures.append(f"01.1: first acquire failed: {ra}")
        else:
            s_11 = socket.create_connection(("127.0.0.1", port), timeout=10)
            extra_socks.append(s_11)
            done_11 = threading.Event()
            result_11 = [None]

            def try_11():
                result_11[0] = _call(s_11, acquire,
                                     {"url": f"sftp://localhost/{TESTKS}/b"},
                                     timeout=15)
                done_11.set()

            threading.Thread(target=try_11, daemon=True).start()
            time.sleep(1.0)
            if done_11.is_set():
                failures.append("01.1: acquire for different path returned immediately — different path not sharing pool")
            else:
                print("[01.1] acquire for same user+host different path blocks (pool cap reached) → PASS")
            _call(s, release, {"handleId": ha}, timeout=5)
            done_11.wait(timeout=5)
            if result_11[0] and "result" in result_11[0] and _hid(result_11[0]):
                _call(s, release, {"handleId": _hid(result_11[0])}, timeout=5)
        _call(s, shutdown, {}, timeout=10)

        # ── 01.2: same host, different ports → different pools ─────────────────
        # Create a pool with max 1 connection and a short connect timeout. Acquire A
        # for port 22. Then acquire B for port 2222 — B must return promptly (error
        # or success) because it targets a different pool and is not blocked by A.
        r = _call(s, configure, {"maxConnections": 1, "connectTimeoutSeconds": 3,
                                 "idleKeepaliveSeconds": 60})
        if "result" not in (r or {}):
            print(f"FATAL: configure (01.2) failed: {r}")
            return 1

        ra2 = _call(s, acquire, {"url": f"sftp://localhost:22/{TESTKS}/a"},
                    timeout=30)
        ha2 = _hid(ra2)
        if ha2 is None:
            failures.append(f"01.2: acquire for port 22 failed: {ra2}")
        else:
            # port 2222 almost certainly has no SSH; connection is refused or times out
            # within connectTimeoutSeconds=3. If it were the same pool as port 22, this
            # acquire would block indefinitely (we hold the only slot). Timeout=8 > 3s
            # connect-timeout, so returning any response (error or success) proves
            # different pool.
            t0 = time.time()
            rb2 = _call(s, acquire, {"url": f"sftp://localhost:2222/{TESTKS}/a"},
                        timeout=8)
            elapsed = time.time() - t0
            if rb2 is None:
                failures.append("01.2: acquire for port 2222 timed out (blocked by port-22 pool) — ports not using distinct pools")
            else:
                print(f"[01.2] acquire for different port returned in {elapsed:.2f}s (not blocked) → distinct pools: PASS")
                if _hid(rb2):
                    _call(s, release, {"handleId": _hid(rb2)}, timeout=5)
            _call(s, release, {"handleId": ha2}, timeout=5)
        _call(s, shutdown, {}, timeout=10)

        # ── 01.3: explicit username in URL → connects as that user ───────────
        # Use an intentionally nonexistent username. If URL usernames are ignored,
        # this would connect as the OS user and succeed; using the explicit username
        # must attempt authentication for that user and fail.
        r = _call(s, configure, {"maxConnections": 2, "connectTimeoutSeconds": 30,
                                 "idleKeepaliveSeconds": 60})
        if "result" not in (r or {}):
            print(f"FATAL: configure (01.3) failed: {r}")
            return 1
        missing_user = "ks_no_such_user_" + uuid.uuid4().hex
        ra3 = _call(s, acquire,
                    {"url": f"sftp://{missing_user}@localhost/{TESTKS}/a"},
                    timeout=30)
        ha3 = _hid(ra3)
        if ha3 is not None:
            failures.append(f"01.3: acquire with explicit nonexistent username '{missing_user}' succeeded — URL username ignored")
            _call(s, release, {"handleId": ha3}, timeout=5)
        elif ra3 is None:
            failures.append(f"01.3: acquire with explicit nonexistent username '{missing_user}' timed out")
        else:
            print(f"[01.3] acquire with explicit nonexistent username '{missing_user}@localhost' failed as that user → PASS")
        _call(s, shutdown, {}, timeout=10)

        # ── 01.4: no username in URL → connects as current OS user ───────────
        # Fill the pool using the explicit OS user (same key). Then try acquiring
        # with no username — it must block because no-username resolves to the OS
        # user, sharing the same pool key as the explicit URL.
        r = _call(s, configure, {"maxConnections": 1, "connectTimeoutSeconds": 30,
                                 "idleKeepaliveSeconds": 60})
        if "result" not in (r or {}):
            print(f"FATAL: configure (01.4) failed: {r}")
            return 1
        ra4 = _call(s, acquire,
                    {"url": f"sftp://{os_user}@localhost/{TESTKS}/a"},
                    timeout=30)
        ha4 = _hid(ra4)
        if ha4 is None:
            failures.append(f"01.4: initial acquire with explicit OS user failed: {ra4}")
        else:
            s_14 = socket.create_connection(("127.0.0.1", port), timeout=10)
            extra_socks.append(s_14)
            done_14 = threading.Event()
            result_14 = [None]

            def try_14():
                result_14[0] = _call(s_14, acquire,
                                     {"url": f"sftp://localhost/{TESTKS}/b"},
                                     timeout=15)
                done_14.set()

            threading.Thread(target=try_14, daemon=True).start()
            time.sleep(1.0)
            if done_14.is_set():
                failures.append("01.4: acquire with no username returned immediately when OS-user slot was full — no-username not resolving to OS user")
            else:
                print("[01.4] acquire with no username blocks when OS-user slot is full → resolves to OS user: PASS")
            _call(s, release, {"handleId": ha4}, timeout=5)
            done_14.wait(timeout=5)
            if result_14[0] and "result" in result_14[0] and _hid(result_14[0]):
                _call(s, release, {"handleId": _hid(result_14[0])}, timeout=5)
        _call(s, shutdown, {}, timeout=10)

        # ── 01.5: two simultaneous acquisitions for same pool count against max_connections ──
        # Acquire two handles from the same (user, host) pool (max 2). A third
        # acquire must block, proving both prior handles are counted against the
        # shared pool cap.
        r = _call(s, configure, {"maxConnections": 2, "connectTimeoutSeconds": 30,
                                 "idleKeepaliveSeconds": 60})
        if "result" not in (r or {}):
            print(f"FATAL: configure (01.5) failed: {r}")
            return 1
        ra5 = _call(s, acquire, {"url": f"sftp://localhost/{TESTKS}/a"}, timeout=30)
        rb5 = _call(s, acquire, {"url": f"sftp://localhost/{TESTKS}/b"}, timeout=30)
        ha5, hb5 = _hid(ra5), _hid(rb5)
        if ha5 is None or hb5 is None:
            failures.append(f"01.5: failed to acquire 2 handles from pool: a={ra5} b={rb5}")
        else:
            s_15 = socket.create_connection(("127.0.0.1", port), timeout=10)
            extra_socks.append(s_15)
            done_15 = threading.Event()
            result_15 = [None]

            def try_15():
                result_15[0] = _call(s_15, acquire,
                                     {"url": f"sftp://localhost/{TESTKS}/a"},
                                     timeout=15)
                done_15.set()

            threading.Thread(target=try_15, daemon=True).start()
            time.sleep(1.0)
            if done_15.is_set():
                failures.append("01.5: third acquire returned immediately when 2/2 slots held — both prior acquisitions not counted against same pool")
            else:
                print("[01.5] third acquire blocks when 2/2 slots held → both URLs in same pool: PASS")
            _call(s, release, {"handleId": ha5}, timeout=5)
            done_15.wait(timeout=5)
            if result_15[0] and "result" in result_15[0] and _hid(result_15[0]):
                _call(s, release, {"handleId": _hid(result_15[0])}, timeout=5)
            _call(s, release, {"handleId": hb5}, timeout=5)
        _call(s, shutdown, {}, timeout=10)

        if failures:
            print("\nFAILURES:")
            for f in failures:
                print(f"  - {f}")
            return 1
        print("\nAll assertions passed.")
        return 0

    finally:
        for sx in extra_socks:
            try:
                sx.close()
            except Exception:
                pass
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


if __name__ == "__main__":
    sys.exit(main())
