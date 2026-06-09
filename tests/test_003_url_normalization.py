#!/usr/bin/env python3
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

import sys

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

import getpass
import os
import queue
import signal
import subprocess
import tempfile
import threading
from pathlib import Path

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = WORKSPACE / "released" / "kitchensync.exe"
SFTP_SCRIPT = WORKSPACE / "extart" / "ephemeral-sftp-server.py"

if sys.platform == "win32":
    UV = WORKSPACE / "aitc" / "bin" / "uv.exe"
elif sys.platform == "darwin":
    UV = WORKSPACE / "aitc" / "bin" / "uv.mac"
else:
    UV = WORKSPACE / "aitc" / "bin" / "uv.linux"

FAILURES = []


def _readline_timeout(fh, timeout=15.0):
    q = queue.Queue()
    def _reader():
        try:
            q.put(fh.readline())
        except Exception as exc:
            q.put(exc)
    threading.Thread(target=_reader, daemon=True).start()
    result = q.get(timeout=timeout)
    if isinstance(result, Exception):
        raise result
    return result


def check(cond, label, proc=None):
    if cond:
        print(f"PASS: {label}")
    else:
        FAILURES.append(label)
        print(f"FAIL: {label}")
        if proc is not None:
            print(f"  exit={proc.returncode}")
            if proc.stdout:
                print(f"  stdout: {proc.stdout[:400]}")
            if proc.stderr:
                print(f"  stderr: {proc.stderr[:400]}")


def ks(*args, cwd=None, timeout=60):
    return subprocess.run(
        [str(EXE)] + [str(a) for a in args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        cwd=str(cwd) if cwd else None,
        timeout=timeout,
    )


class SftpServer:
    """Start the ephemeral SFTP server and manage the known_hosts entry."""

    def __init__(self, *flags):
        self._flags = list(flags)
        self.proc = None
        self.port = None
        self._known_hosts_entry = None

    def __enter__(self):
        cmd = [str(UV), "run", "--script", str(SFTP_SCRIPT)] + self._flags
        self.proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            start_new_session=True,
        )
        self.port = int(_readline_timeout(self.proc.stdout).strip())
        # Server writes "host key: <type> <base64>" early in its stderr startup block.
        for _ in range(20):
            line = _readline_timeout(self.proc.stderr)
            if not line:
                break
            if line.startswith("host key:"):
                self._add_known_hosts(line.split("host key:", 1)[1].strip())
                break
        return self

    def _add_known_hosts(self, key_str):
        known_hosts = Path.home() / ".ssh" / "known_hosts"
        known_hosts.parent.mkdir(mode=0o700, exist_ok=True)
        entry = f"[127.0.0.1]:{self.port} {key_str}\n"
        self._known_hosts_entry = entry
        with open(known_hosts, "a", encoding="utf-8") as fh:
            fh.write(entry)

    def __exit__(self, *_):
        if self.proc:
            try:
                os.killpg(os.getpgid(self.proc.pid), signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                try:
                    os.killpg(os.getpgid(self.proc.pid), signal.SIGKILL)
                except ProcessLookupError:
                    pass
                self.proc.wait()
        if self._known_hosts_entry:
            known_hosts = Path.home() / ".ssh" / "known_hosts"
            if known_hosts.exists():
                text = known_hosts.read_text(encoding="utf-8", errors="replace")
                known_hosts.write_text(
                    text.replace(self._known_hosts_entry, ""), encoding="utf-8"
                )


# ──────────────────────────────────────────────────────────────────────────────
# 003.6 / 003.7 / 003.12: bare paths and relative paths → file:// URL
# ──────────────────────────────────────────────────────────────────────────────
def run_file_url():
    with tempfile.TemporaryDirectory(prefix="ks003f_") as _tmp:
        tmp = Path(_tmp)
        src = tmp / "src"
        dst = tmp / "dst"
        src.mkdir()
        dst.mkdir()
        (src / "a.txt").write_text("hello")

        # 003.6: bare absolute path (no scheme) treated as file://
        r = ks(f"+{src}", str(dst), "--dry-run", timeout=30)
        check(r.returncode == 0, f"003.6: bare absolute path accepted as file:// peer", r)

        # 003.5: trailing slash on a bare absolute path removed before use
        r = ks(f"+{src}/", str(dst), "--dry-run", timeout=30)
        check(r.returncode == 0, f"003.5: trailing slash on bare path accepted (peer reachable)", r)

        # 003.7 / 003.12: relative ./src resolved from cwd to the correct absolute dir
        # If the relative path is not resolved correctly the peer dir won't be found.
        r = ks("+./src", str(dst), "--dry-run", cwd=str(tmp), timeout=30)
        check(r.returncode == 0,
              f"003.7/003.12: ./src resolved from cwd '{tmp}' (peer reachable)", r)

        # 003.11: c:/photos/ → file:///c:/photos is Windows-only
        # not reasonably testable: 003.11 on Linux (drive-letter paths don't exist)

        # 003.8 (file:// variant): percent-encoded unreserved chars decoded in file URL
        # %64='d', %61='a', %74='t', %61='a' → 'data'
        # Create only the decoded directory; if product decodes the URL it finds the dir
        # (peer reachable → exit 0); if it does not, the literal '%64...' dir is missing
        # (peer unreachable → exit 1).
        data_dir = tmp / "data"
        data_dir.mkdir()
        (data_dir / "b.txt").write_text("b")
        encoded_seg = "%64%61%74%61"  # 'data' percent-encoded (all unreserved chars)
        if sys.platform == "win32":
            encoded_url = "file:///" + str(tmp / encoded_seg).replace("\\", "/")
        else:
            encoded_url = "file://" + str(tmp / encoded_seg)
        r = ks(f"+{encoded_url}", str(dst), "--dry-run", timeout=30)
        check(r.returncode == 0,
              "003.8: percent-encoded unreserved chars decoded in file:// URL path", r)


# ──────────────────────────────────────────────────────────────────────────────
# SFTP URL normalization: 003.1, 003.4, 003.5, 003.9, 003.13, 003.14, 003.15
# Uses inline password so the test does not depend on SSH key files.
# ──────────────────────────────────────────────────────────────────────────────
def run_sftp_url():
    with SftpServer() as srv, tempfile.TemporaryDirectory(prefix="ks003s_") as _tmp:
        tmp = Path(_tmp)
        port = srv.port

        def local_peer(name):
            d = tmp / name
            d.mkdir(exist_ok=True)
            (d / "x.txt").write_text("x")
            return d

        # 003.1 + 003.5 + 003.13: uppercase scheme + trailing slash normalized away
        # SFTP://u:p@127.0.0.1:PORT/sftp01/ → sftp://u:p@127.0.0.1:PORT/sftp01
        r = ks(
            f"+{local_peer('a')}",
            f"SFTP://u:p@127.0.0.1:{port}/sftp01/",
            timeout=30,
        )
        check(r.returncode == 0,
              f"003.1/003.5/003.13: uppercase SFTP scheme and trailing slash normalized (exit={r.returncode})", r)

        # 003.4 + 003.14: consecutive slashes in path collapsed to one
        # sftp://u:p@127.0.0.1:PORT//sftp02 → sftp://u:p@127.0.0.1:PORT/sftp02
        r = ks(
            f"+{local_peer('b')}",
            f"sftp://u:p@127.0.0.1:{port}//sftp02",
            timeout=30,
        )
        check(r.returncode == 0,
              f"003.4/003.14: double slash in path collapsed to single slash (exit={r.returncode})", r)

        # 003.9 + 003.15: query string stripped before connecting
        # sftp://u:p@127.0.0.1:PORT/sftp03?timeout-conn=60 → sftp://u:p@127.0.0.1:PORT/sftp03
        r = ks(
            f"+{local_peer('c')}",
            f"sftp://u:p@127.0.0.1:{port}/sftp03?timeout-conn=60",
            timeout=30,
        )
        check(r.returncode == 0,
              f"003.9/003.15: query-string parameter stripped from SFTP URL (exit={r.returncode})", r)

        # 003.3: port-22 removal — tested indirectly
        # The server runs on a random non-22 port. Providing that port explicitly
        # connects successfully (non-22 port is preserved). Port-22 removal (the
        # complementary case) requires a server on port 22, which is not available
        # in this environment; see comment below.
        r = ks(
            f"+{local_peer('d')}",
            f"sftp://u:p@127.0.0.1:{port}/sftp04",
            timeout=30,
        )
        check(r.returncode == 0,
              f"003.3: explicit non-22 port {port} preserved in URL (exit={r.returncode})", r)
        # not reasonably testable: 003.3 (port-22 removal) requires a server bound to
        # port 22, which is not available in the ephemeral test environment.

        # 003.2: hostname lowercasing
        # not reasonably testable: 003.2 — the loopback address is always 127.0.0.1
        # (no case variation); DNS case-insensitivity makes it impossible to distinguish
        # OS-level case folding from URL normalization in kitchensync.


# ──────────────────────────────────────────────────────────────────────────────
# 003.10 / 003.16: OS user inserted when no username in SFTP URL
# Requires ~/.ssh/id_ed25519.pub; skipped if absent.
# ──────────────────────────────────────────────────────────────────────────────
def run_sftp_username_insertion():
    os_user = getpass.getuser()
    ed25519_pub = Path.home() / ".ssh" / "id_ed25519.pub"
    if not ed25519_pub.exists():
        print("SKIP 003.10/003.16: ~/.ssh/id_ed25519.pub not found")
        return

    # Server accepts only the current OS user, authenticated by public key.
    # If kitchensync inserts the OS user into the URL, authentication succeeds.
    # If it leaves the username blank or uses a wrong name, auth fails → exit 1.
    with SftpServer(
        "--user", os_user,
        "--authorized-key", str(ed25519_pub),
    ) as srv, tempfile.TemporaryDirectory(prefix="ks003u_") as _tmp:
        tmp = Path(_tmp)
        local = tmp / "local"
        local.mkdir()
        port = srv.port

        # URL deliberately omits the username; normalization must insert os_user.
        r = ks(
            f"+{local}",
            f"sftp://127.0.0.1:{port}/usertest",
            timeout=30,
        )
        check(r.returncode == 0,
              f"003.10/003.16: username-less SFTP URL gets OS user '{os_user}' inserted (exit={r.returncode})", r)


# ──────────────────────────────────────────────────────────────────────────────
# Main
# ──────────────────────────────────────────────────────────────────────────────
run_file_url()
run_sftp_url()
run_sftp_username_insertion()

if FAILURES:
    print(f"\n{len(FAILURES)} FAILURE(S):")
    for f in FAILURES:
        print(f"  - {f}")
    sys.exit(1)

print("\nAll checks passed.")
sys.exit(0)
