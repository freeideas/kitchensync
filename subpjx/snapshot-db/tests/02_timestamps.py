#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Exercises 02_timestamps requirements: now() format, UTC correctness, monotonicity."""

from __future__ import annotations

import json, os, re, socket, subprocess, sys, threading, time
from datetime import datetime, timezone
from pathlib import Path

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

TS_RE = re.compile(r"^\d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}_\d{6}Z$")


def _drain(stream):
    for _ in stream:
        pass


def _launch():
    proc = subprocess.Popen(
        [str(UV), "run", "--script", str(BUILD_PY), "launch-mcp", PROJECT],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding="utf-8",
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


def _rpc(sock, method, params, rpc_id):
    msg = {"jsonrpc": "2.0", "id": rpc_id, "method": method}
    if params is not None:
        msg["params"] = params
    sock.sendall((json.dumps(msg) + "\n").encode("utf-8"))
    buf = b""
    deadline = time.time() + 10
    while time.time() < deadline:
        chunk = sock.recv(8192)
        if not chunk:
            break
        buf += chunk
        if b"\n" in buf:
            break
    line, _, _ = buf.partition(b"\n")
    return json.loads(line.decode("utf-8"))


def _parse_ts(ts: str) -> datetime:
    return datetime.strptime(ts[:-1], "%Y-%m-%d_%H-%M-%S_%f").replace(tzinfo=timezone.utc)


def _parse_ts_or_none(ts: str):
    try:
        return _parse_ts(ts)
    except ValueError:
        return None


def _unwrap(resp):
    result = resp.get("result", {})
    if not isinstance(result, dict):
        return result
    content = result.get("content", [])
    if content and content[0].get("type") == "text":
        text = content[0]["text"]
        try:
            return json.loads(text)
        except (json.JSONDecodeError, ValueError):
            return text
    return result


def _timestamp(resp):
    value = _unwrap(resp)
    return value if isinstance(value, str) else ""


def _timestamps(resp):
    value = _unwrap(resp)
    return value if isinstance(value, list) else []


def _micros(ts: str) -> int:
    dt = _parse_ts_or_none(ts)
    if dt is None:
        raise ValueError(f"invalid timestamp fields: {ts!r}")
    epoch = datetime(1970, 1, 1, tzinfo=timezone.utc)
    delta = dt - epoch
    return ((delta.days * 86400 + delta.seconds) * 1_000_000
            + delta.microseconds)


def _micros_or_none(ts: str):
    try:
        return _micros(ts)
    except ValueError:
        return None


def main() -> int:
    proc, port = _launch()
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=10) as s:
            failures = []
            _id = [0]

            def rpc(method, params=None):
                _id[0] += 1
                return _rpc(s, method, params, _id[0])

            before = datetime.now(timezone.utc)
            resp = rpc("tools/call", {"name": "now", "arguments": {}})
            after = datetime.now(timezone.utc)
            ts = _timestamp(resp)

            # 02.1 — now() matches YYYY-MM-DD_HH-mm-ss_ffffffZ, all fields zero-padded
            print(f"[02.1] now() = {ts!r}")
            if not TS_RE.match(ts):
                failures.append(f"02.1: {ts!r} does not match YYYY-MM-DD_HH-mm-ss_ffffffZ")

            # 02.2 — fields correspond to current UTC wall-clock time (not local time)
            print(f"[02.2] UTC window = [{before.isoformat()}, {after.isoformat()}]")
            if TS_RE.match(ts):
                ts_dt = _parse_ts_or_none(ts)
                if ts_dt is None:
                    failures.append(f"02.2: invalid calendar fields in {ts!r}")
                elif not (before <= ts_dt <= after):
                    failures.append(
                        f"02.2: {ts_dt.isoformat()} outside UTC window "
                        f"[{before.isoformat()}, {after.isoformat()}]"
                    )
            else:
                failures.append(f"02.2: bad format {ts!r}, cannot check UTC")

            # 02.3 — successive calls return strictly increasing values
            stamps = _timestamps(rpc("tools/call", {"name": "now-n", "arguments": {"count": 20}}))
            print(f"[02.3] successive now() = {stamps}")
            bad_formats = [stamp for stamp in stamps if not isinstance(stamp, str) or not TS_RE.match(stamp)]
            if bad_formats:
                failures.append(f"02.3: now-n returned badly formatted timestamps: {bad_formats}")
            elif len(stamps) != 20:
                failures.append(f"02.3: now-n returned {len(stamps)} timestamps, want 20")
            else:
                all_stamps = [ts] + stamps
                micros = [_micros_or_none(stamp) for stamp in all_stamps]
                if any(micros_value is None for micros_value in micros):
                    failures.append(f"02.3: invalid calendar fields in timestamps: {all_stamps}")
                elif not all(micros[i] < micros[i + 1] for i in range(len(micros) - 1)):
                    failures.append(f"02.3: not strictly increasing: {all_stamps}")
                if sorted(all_stamps) != all_stamps:
                    failures.append(f"02.3: timestamp strings are not lexicographically increasing: {all_stamps}")

            # 02.4 — not reasonably testable through this MCP wrapper. The exact
            # +1 microsecond fallback only occurs when the wall clock has not
            # advanced past the last returned value, but the wrapper exposes no
            # way to freeze or inject the wall clock. Rapid now()/now-n calls only
            # observe host timing, so requiring a 1us delta would be flaky.
            deltas = []
            if len(stamps) == 20 and not bad_formats:
                parsed = [_micros_or_none(stamp) for stamp in stamps]
                if not any(value is None for value in parsed):
                    deltas = [parsed[i + 1] - parsed[i] for i in range(len(parsed) - 1)]
            print(f"[02.4] exact 1us fallback not directly controllable; observed deltas={deltas}")

            if failures:
                print("\nFAILURES:")
                for f in failures:
                    print(f"  - {f}")
                return 1
            print("\nAll assertions passed.")
            return 0
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()


if __name__ == "__main__":
    sys.exit(main())
