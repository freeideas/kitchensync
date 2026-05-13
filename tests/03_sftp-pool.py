#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko==3.5.1"]
# ///
"""SFTP connection pool: pool identity (03.58), per-URL overrides (03.59), mc cap (03.60),
ka keep-alive (03.61), ct timeout with fallback (03.62), file:// exemption (03.63),
transfer borrows from both pools (03.64)."""

from __future__ import annotations

import errno, logging, os, posixpath, re, shutil, socket, subprocess, sys, threading, time
from pathlib import Path

import paramiko
from paramiko import SFTPAttributes, SFTPHandle, SFTPServer

logging.getLogger("paramiko").setLevel(logging.CRITICAL + 1)

BUILD_PY = Path(os.environ.get("AITC_BUILD_PY", "./aitc/languages/java/build.py"))
UV = Path(os.environ.get("AITC_UV", "./aitc/bin/uv.linux"))
PROJECT = os.environ.get("AITC_PROJECT", ".")

USER = "pooltest"
PASSWORD = "poolpass"
HOST = "127.0.0.1"
ENDPOINT = f"{USER}@{HOST}"
SFTP_PORT = 0

TMP = (Path(PROJECT) / "tmp" / "testks" / "03_sftp-pool").resolve()
TEST_HOME = TMP / "home"


def sftp(path: Path, **query) -> str:
    rel = "/" + path.relative_to(TMP).as_posix()
    url = f"sftp://{USER}:{PASSWORD}@{HOST}:{SFTP_PORT}{rel}"
    if query:
        url += "?" + "&".join(f"{k}={v}" for k, v in query.items())
    return url


TRACE_EVENT = re.compile(r"endpoint=(\S+) connections=(\d+)/(\d+)")
TIMEOUT_EXIT = 124


def invoke(args: list[str], timeout: int = 60) -> tuple[int, str, str]:
    env = os.environ.copy()
    env.pop("SSH_AUTH_SOCK", None)
    env["JAVA_TOOL_OPTIONS"] = (
        (env.get("JAVA_TOOL_OPTIONS", "") + " ").strip()
        + f"-Duser.home={TEST_HOME}"
    ).strip()
    try:
        proc = subprocess.run(
            [str(UV), "run", "--script", str(BUILD_PY), "invoke-cli", PROJECT] + args,
            capture_output=True, text=True, encoding="utf-8", timeout=timeout, env=env,
        )
        return proc.returncode, proc.stdout, proc.stderr
    except subprocess.TimeoutExpired as ex:
        stdout = ex.stdout.decode("utf-8", errors="replace") if isinstance(ex.stdout, bytes) else ex.stdout
        stderr = ex.stderr.decode("utf-8", errors="replace") if isinstance(ex.stderr, bytes) else ex.stderr
        return TIMEOUT_EXIT, stdout or "", stderr or ""


def trace_counts(stdout: str, endpoint: str = ENDPOINT) -> list[tuple[int, int]]:
    return [
        (int(match.group(2)), int(match.group(3)))
        for match in TRACE_EVENT.finditer(stdout)
        if match.group(1) == endpoint
    ]


def max_in_use(stdout: str, endpoint: str = ENDPOINT) -> int:
    counts = trace_counts(stdout, endpoint)
    return max((used for used, _ in counts), default=0)


def saw_trace_count(stdout: str, used: int, max_conn: int, endpoint: str = ENDPOINT) -> bool:
    return (used, max_conn) in trace_counts(stdout, endpoint)


class PasswordServer(paramiko.ServerInterface):
    def check_auth_password(self, username: str, password: str) -> int:
        if username == USER and password == PASSWORD:
            return paramiko.AUTH_SUCCESSFUL
        return paramiko.AUTH_FAILED

    def get_allowed_auths(self, username: str) -> str:
        return "password"

    def check_channel_request(self, kind: str, chanid: int) -> int:
        if kind == "session":
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED


class RootedSftp(paramiko.SFTPServerInterface):
    def __init__(self, server, root: str, blocked_write_prefixes: tuple[str, ...]):
        super().__init__(server)
        self.root = Path(root).resolve()
        self.blocked_write_prefixes = blocked_write_prefixes

    @staticmethod
    def _remote(path: str) -> str:
        return posixpath.normpath("/" + path.lstrip("/"))

    def _local(self, path: str) -> Path:
        normalized = self._remote(path)
        local = (self.root / normalized.lstrip("/")).resolve()
        if local == self.root or self.root in local.parents:
            return local
        raise OSError(errno.EACCES, "path outside test root")

    def _write_blocked(self, path: str) -> bool:
        normalized = self._remote(path)
        for prefix in self.blocked_write_prefixes:
            if normalized == prefix or normalized.startswith(prefix + "/"):
                return True
        return False

    @staticmethod
    def _attr(path: Path) -> SFTPAttributes:
        attrs = SFTPAttributes.from_stat(path.stat())
        attrs.filename = path.name
        return attrs

    def list_folder(self, path: str):
        try:
            return [self._attr(child) for child in self._local(path).iterdir()]
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def stat(self, path: str):
        try:
            return SFTPAttributes.from_stat(self._local(path).stat())
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def lstat(self, path: str):
        return self.stat(path)

    def open(self, path: str, flags: int, attr: SFTPAttributes):
        try:
            if flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC) and self._write_blocked(path):
                return paramiko.SFTP_PERMISSION_DENIED
            local = self._local(path)
            if flags & os.O_CREAT:
                local.parent.mkdir(parents=True, exist_ok=True)
            fd = os.open(local, flags, getattr(attr, "st_mode", None) or 0o666)
            mode = "r+b" if flags & os.O_RDWR else "wb" if flags & os.O_WRONLY else "rb"
            file_obj = os.fdopen(fd, mode)
            handle = SFTPHandle(flags)
            if "r" in mode or "+" in mode:
                handle.readfile = file_obj
            if "w" in mode or "+" in mode:
                handle.writefile = file_obj
            return handle
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def remove(self, path: str):
        try:
            self._local(path).unlink()
            return paramiko.SFTP_OK
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def rename(self, oldpath: str, newpath: str):
        try:
            os.replace(self._local(oldpath), self._local(newpath))
            return paramiko.SFTP_OK
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def posix_rename(self, oldpath: str, newpath: str):
        return self.rename(oldpath, newpath)

    def mkdir(self, path: str, attr: SFTPAttributes):
        try:
            self._local(path).mkdir(mode=getattr(attr, "st_mode", None) or 0o777)
            return paramiko.SFTP_OK
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def rmdir(self, path: str):
        try:
            self._local(path).rmdir()
            return paramiko.SFTP_OK
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)

    def chattr(self, path: str, attr: SFTPAttributes):
        try:
            local = self._local(path)
            if attr.st_atime is not None or attr.st_mtime is not None:
                current = local.stat()
                atime = attr.st_atime if attr.st_atime is not None else current.st_atime
                mtime = attr.st_mtime if attr.st_mtime is not None else current.st_mtime
                os.utime(local, (atime, mtime))
            return paramiko.SFTP_OK
        except OSError as ex:
            return SFTPServer.convert_errno(ex.errno)


class LocalSftpServer:
    def __init__(self, root: Path, blocked_write_prefixes: tuple[str, ...] = ()):
        self.root = root
        self.blocked_write_prefixes = tuple(
            "/" + prefix.strip("/") for prefix in blocked_write_prefixes
        )
        self.key = paramiko.RSAKey.generate(2048)
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.sock.bind((HOST, 0))
        self.sock.listen(32)
        self.port = self.sock.getsockname()[1]
        self.transports: list[paramiko.Transport] = []
        self.lock = threading.Lock()
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._serve, daemon=True)
        self.thread.start()

    def _serve(self) -> None:
        self.sock.settimeout(0.2)
        while not self.stop.is_set():
            try:
                client, _ = self.sock.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            transport = paramiko.Transport(client)
            transport.add_server_key(self.key)
            transport.set_subsystem_handler(
                "sftp",
                SFTPServer,
                RootedSftp,
                str(self.root),
                self.blocked_write_prefixes,
            )
            try:
                transport.start_server(server=PasswordServer())
                with self.lock:
                    self.transports.append(transport)
            except Exception:
                transport.close()

    def connection_count(self) -> int:
        with self.lock:
            return len(self.transports)

    def write_known_hosts(self, home: Path) -> None:
        ssh = home / ".ssh"
        ssh.mkdir(parents=True, exist_ok=True)
        (ssh / "known_hosts").write_text(
            f"[{HOST}]:{self.port} {self.key.get_name()} {self.key.get_base64()}\n",
            encoding="utf-8",
        )

    def close(self) -> None:
        self.stop.set()
        try:
            self.sock.close()
        except OSError:
            pass
        with self.lock:
            transports = list(self.transports)
        for transport in transports:
            transport.close()


def start_blackhole() -> tuple[socket.socket, int]:
    """Accepts TCP connections but never speaks SSH — simulates a hung SSH server."""
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", 0))
    srv.listen(16)
    port = srv.getsockname()[1]
    held: list[socket.socket] = []

    def _loop() -> None:
        srv.settimeout(60)
        try:
            while True:
                try:
                    conn, _ = srv.accept()
                    held.append(conn)
                except OSError:
                    break
        finally:
            for c in held:
                try:
                    c.close()
                except OSError:
                    pass

    threading.Thread(target=_loop, daemon=True).start()
    return srv, port


def main() -> int:
    global SFTP_PORT
    if TMP.exists():
        shutil.rmtree(TMP)
    TMP.mkdir(parents=True)

    failures: list[str] = []
    blackhole_srv = None
    sftp_srv = None

    try:
        sftp_srv = LocalSftpServer(TMP, blocked_write_prefixes=("/64_fail_dst",))
        SFTP_PORT = sftp_srv.port
        sftp_srv.write_known_hosts(TEST_HOME)
        blackhole_srv, bh_port = start_blackhole()

        # ── 03.58 ── Two SFTP URLs same user+host share one pool ──────────────
        # The peers differ only by path. During the cross-SFTP transfer, trace
        # output must show one endpoint reaching connections=2/2, not two
        # independent path-scoped pools at 1/2.
        print("[03.58] same-user+host SFTP peers share one path-independent pool")
        p58_a = TMP / "58_a"
        p58_b = TMP / "58_b"
        p58_a.mkdir(parents=True)
        p58_b.mkdir(parents=True)
        (p58_a / "from_a.txt").write_text("hello from a")

        rc, out, err = invoke([
            "+" + sftp(p58_a, mc=2),
            sftp(p58_b, mc=2),
            "-vl",
            "trace",
        ])
        print(f"[03.58] exit={rc}")
        if rc != 0:
            failures.append(
                f"03.58: sync between two same-user+host SFTP peers failed (exit {rc})\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
        elif not (p58_b / "from_a.txt").exists():
            failures.append("03.58: file not propagated across same-user+host SFTP peers")
        elif not saw_trace_count(out, 2, 2):
            failures.append(
                "03.58: trace never showed one user+host pool serving both paths "
                f"at connections=2/2\n  stdout: {out!r}"
            )
        else:
            print("[03.58] PASS: one user+host pool served both SFTP paths")

        # ── 03.59 ── Per-URL settings override global flags ───────────────────
        # mc: both SFTP URLs have ?mc=2 while global --mc is 1. A cross-SFTP
        # transfer between same-user+host paths needs two borrowed connections
        # from the shared pool, so success plus trace connections=2/2 observes
        # the per-URL mc override.
        print("[03.59] per-URL ?mc=2 overrides global --mc 1")
        p59_mc_a = TMP / "59_mc_a"
        p59_mc_b = TMP / "59_mc_b"
        p59_mc_a.mkdir(parents=True)
        p59_mc_b.mkdir(parents=True)
        (p59_mc_a / "mc.txt").write_text("mc")

        rc, out, err = invoke([
            "+" + sftp(p59_mc_a, mc=2),
            sftp(p59_mc_b, mc=2),
            "--mc",
            "1",
            "-vl",
            "trace",
        ], timeout=15)
        print(f"[03.59/mc] exit={rc}")
        if rc != 0:
            failures.append(
                f"03.59: per-URL ?mc=2 did not override global --mc 1; sync failed "
                f"(exit {rc})\n  stdout: {out!r}\n  stderr: {err!r}"
            )
        elif not (p59_mc_b / "mc.txt").exists():
            failures.append("03.59: file not propagated while testing per-URL mc override")
        elif not saw_trace_count(out, 2, 2):
            failures.append(
                "03.59: trace did not show the per-URL mc=2 pool limit "
                f"overriding global --mc 1\n  stdout: {out!r}"
            )
        else:
            print("[03.59/mc] PASS: per-URL mc override controlled the pool cap")

        # ct: URL points at a blackhole with ?ct=2. Global --ct 30 is set.
        # Per-URL ct=2 must win: sync fails within ~2 s, not ~30 s.
        print("[03.59] per-URL ?ct=2 overrides global --ct 30")
        p59 = TMP / "59_file"
        p59.mkdir(parents=True)
        (p59 / "x.txt").write_text("x")

        bh_url = f"sftp://{USER}:{PASSWORD}@127.0.0.1:{bh_port}/ks59?ct=2"
        t0 = time.monotonic()
        rc, out, err = invoke(
            ["+" + p59.as_uri(), bh_url, "--ct", "30"],
            timeout=15,
        )
        elapsed = time.monotonic() - t0
        print(f"[03.59] exit={rc}, elapsed={elapsed:.1f}s")
        if rc == 0:
            failures.append("03.59: blackhole SFTP peer unexpectedly connected")
        elif elapsed >= 8:
            failures.append(
                f"03.59: per-URL ?ct=2 did not override --ct 30; "
                f"sync took {elapsed:.1f}s (expected <8 s)"
            )
        else:
            print(f"[03.59] PASS: per-URL ct=2 applied; timed out in {elapsed:.1f}s")

        # ka override TTL expiry is not reasonably testable through the one-shot
        # CLI: the CLI does not expose pool internals, cannot schedule an idle
        # gap inside a run, and shuts down the pool when the process exits. The
        # reusable-within-ka behavior itself is exercised in 03.61 below.

        # ── 03.60 ── Pool cap mc=1 respected ──────────────────────────────────
        # Several file copies borrow from the same SFTP source pool. Trace output
        # must never exceed one in-use pooled connection, and all files must
        # still arrive, showing callers waited rather than exceeding the cap.
        print("[03.60] mc=1 caps in-use pooled connections while transfers complete")
        p60_sftp = TMP / "60_sftp"
        p60_file = TMP / "60_file"
        p60_sftp.mkdir(parents=True)
        p60_file.mkdir(parents=True)
        for i in range(4):
            (p60_sftp / f"q{i}.txt").write_text(f"q{i}")

        rc, out, err = invoke([
            "+" + sftp(p60_sftp, mc=1),
            p60_file.as_uri(),
            "-vl",
            "trace",
        ])
        print(f"[03.60] exit={rc}")
        if rc != 0:
            failures.append(
                f"03.60: sync with mc=1 SFTP peer failed (exit {rc})\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
        elif not all((p60_file / f"q{i}.txt").exists() for i in range(4)):
            failures.append("03.60: not all files propagated with mc=1 pool")
        elif max_in_use(out) > 1:
            failures.append(
                f"03.60: pool exceeded mc=1; max trace in-use was {max_in_use(out)}\n"
                f"  stdout: {out!r}"
            )
        elif not saw_trace_count(out, 1, 1):
            failures.append(
                "03.60: trace never showed an mc=1 pool acquisition\n"
                f"  stdout: {out!r}"
            )
        else:
            print("[03.60] PASS: in-use pooled connections stayed within mc=1")

        # ── 03.61 ── Ka keep-alive: pooled connection reused within window ────
        # Copy multiple files in one run with mc=1 and ka=30. A listing
        # connection plus one reusable pooled connection is enough; repeated new
        # SSH sessions would show that returned connections are not kept alive.
        print("[03.61] returned SFTP connection is reused within ka window")
        p61_sftp = TMP / "61_sftp"
        p61_file = TMP / "61_file"
        p61_sftp.mkdir(parents=True)
        p61_file.mkdir(parents=True)
        for i in range(3):
            (p61_sftp / f"r{i}.txt").write_text(f"r{i}")

        before_connections = sftp_srv.connection_count()
        rc, out, err = invoke([
            "+" + sftp(p61_sftp, mc=1, ka=30),
            p61_file.as_uri(),
            "-vl",
            "trace",
        ])
        accepted = sftp_srv.connection_count() - before_connections
        print(f"[03.61] exit={rc}")
        if rc != 0:
            failures.append(
                f"03.61: sync exercising ka keep-alive failed (exit {rc})\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
        elif not all((p61_file / f"r{i}.txt").exists() for i in range(3)):
            failures.append("03.61: not all files propagated during ka keep-alive sync")
        elif accepted > 3:
            failures.append(
                "03.61: too many SSH sessions opened for one listing connection "
                f"plus one reusable pooled connection (accepted {accepted})"
            )
        elif not saw_trace_count(out, 0, 1):
            failures.append(
                "03.61: trace did not show the mc=1 pooled connection returned "
                f"after use\n  stdout: {out!r}"
            )
        else:
            print("[03.61] PASS: returned connection stayed reusable within ka=30s")

        # Expiration after the ka window is not reasonably testable through this
        # CLI because the process shuts the pool down at run end; there is no
        # exposed wrapper hook to keep the same pool alive and idle across a
        # controlled wait.

        # ── 03.62 ── ct timeout → fallback URL tried ──────────────────────────
        # Peer uses fallback syntax: first URL is a blackhole with ct=2 (times
        # out); second URL is the real SFTP endpoint.  Sync must succeed because
        # the fallback is tried after the handshake timeout fires.
        print("[03.62] ct timeout fires and fallback URL is tried")
        p62_sftp = TMP / "62_sftp"
        p62_file = TMP / "62_file"
        p62_sftp.mkdir(parents=True)
        p62_file.mkdir(parents=True)
        (p62_sftp / "s.txt").write_text("s")

        real_sftp = sftp(p62_sftp)
        fallback_peer = f"[sftp://{USER}:{PASSWORD}@127.0.0.1:{bh_port}/ks62?ct=2,{real_sftp}]"

        t0 = time.monotonic()
        rc, out, err = invoke(
            ["+" + fallback_peer, p62_file.as_uri()],
            timeout=20,
        )
        elapsed = time.monotonic() - t0
        print(f"[03.62] exit={rc}, elapsed={elapsed:.1f}s")
        if rc != 0:
            failures.append(
                f"03.62: fallback sync failed (exit {rc}); expected fallback to succeed\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
        elif not (p62_file / "s.txt").exists():
            failures.append("03.62: file not propagated via fallback URL")
        else:
            print(f"[03.62] PASS: fallback tried after ct=2 timeout; sync completed in {elapsed:.1f}s")

        # ── 03.63 ── file:// peers: --mc/--ct/--ka flags have no effect ───────
        # A sync between two file:// peers with all three pool flags set must
        # complete normally and emit no pool trace lines.
        print("[03.63] file:// peers unaffected by --mc/--ct/--ka flags")
        p63_a = TMP / "63_a"
        p63_b = TMP / "63_b"
        p63_a.mkdir(parents=True)
        p63_b.mkdir(parents=True)
        (p63_a / "t.txt").write_text("t")

        rc, out, err = invoke([
            "+" + p63_a.as_uri(),
            p63_b.as_uri(),
            "--mc", "5",
            "--ct", "5",
            "--ka", "5",
            "-vl", "trace",
        ])
        print(f"[03.63] exit={rc}")
        if rc != 0:
            failures.append(
                f"03.63: file:// sync with --mc/--ct/--ka failed (exit {rc})\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
        elif not (p63_b / "t.txt").exists():
            failures.append("03.63: file not propagated for file:// peers")
        elif TRACE_EVENT.search(out):
            failures.append(
                "03.63: file:// sync emitted SFTP pool trace events\n"
                f"  stdout: {out!r}"
            )
        else:
            print("[03.63] PASS: file:// peers unaffected by --mc/--ct/--ka flags")

        # ── 03.64 ── Transfer borrows one connection from each pool ───────────
        # Source and destination share user+host here, so both borrows come from
        # the same pool. The trace must reach 2/2 during transfer and return to
        # 0/2 afterward.
        print("[03.64] cross-SFTP transfer borrows and returns both connections")
        p64_src = TMP / "64_src"
        p64_dst = TMP / "64_dst"
        p64_src.mkdir(parents=True)
        p64_dst.mkdir(parents=True)
        (p64_src / "u.txt").write_text("u")

        rc, out, err = invoke([
            "+" + sftp(p64_src, mc=2),
            sftp(p64_dst, mc=2),
            "-vl",
            "trace",
        ])
        print(f"[03.64] exit={rc}")
        if rc != 0:
            failures.append(
                f"03.64: cross-SFTP transfer failed (exit {rc})\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
        elif not (p64_dst / "u.txt").exists():
            failures.append("03.64: file not transferred across SFTP peers")
        elif not saw_trace_count(out, 2, 2):
            failures.append(
                "03.64: trace never showed both source and destination "
                f"connections borrowed for the transfer\n  stdout: {out!r}"
            )
        elif trace_counts(out)[-1:] != [(0, 2)]:
            failures.append(
                "03.64: trace did not show both transfer connections returned "
                f"after success\n  stdout: {out!r}"
            )
        else:
            print("[03.64] PASS: successful transfer borrowed and returned both connections")

        # Failed transfer return path: the server denies writes under the
        # destination root after both pooled connections have been borrowed.
        p64_fail_src = TMP / "64_fail_src"
        p64_fail_dst = TMP / "64_fail_dst"
        p64_fail_src.mkdir(parents=True)
        p64_fail_dst.mkdir(parents=True)
        (p64_fail_src / "blocked.txt").write_text("blocked")

        rc, out, err = invoke([
            "+" + sftp(p64_fail_src, mc=2),
            sftp(p64_fail_dst, mc=2),
            "-vl",
            "trace",
        ])
        counts = trace_counts(out)
        print(f"[03.64/fail] exit={rc}")
        if not saw_trace_count(out, 2, 2):
            failures.append(
                "03.64: failed transfer path did not borrow both connections "
                f"before the write failed\n  stdout: {out!r}\n  stderr: {err!r}"
            )
        elif counts[-1:] != [(0, 2)]:
            failures.append(
                "03.64: failed transfer path did not return both connections\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
        elif (p64_fail_dst / "blocked.txt").exists():
            failures.append("03.64: denied destination unexpectedly received failed transfer")
        else:
            print("[03.64/fail] PASS: failed transfer returned both borrowed connections")

    finally:
        if blackhole_srv is not None:
            try:
                blackhole_srv.close()
            except OSError:
                pass
        if sftp_srv is not None:
            sftp_srv.close()
        shutil.rmtree(TMP, ignore_errors=True)

    if failures:
        print("\nFAILURES:")
        for f in failures:
            print(f"  - {f}")
        return 1
    print("\nAll assertions passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
