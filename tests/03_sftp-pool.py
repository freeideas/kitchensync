#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko"]
# ///

from __future__ import annotations

import logging
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

import paramiko

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")
logging.getLogger("paramiko").setLevel(logging.CRITICAL)

JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")

FAILURES: list[str] = []


def check(cond: bool, msg: str) -> None:
    if cond:
        print(f"PASS: {msg}")
    else:
        FAILURES.append(msg)
        print(f"FAIL: {msg}")


def run_ks(*args: str, timeout: int = 60, java_home: Path | None = None) -> subprocess.CompletedProcess[str]:
    command = [str(JAVA)]
    if java_home is not None:
        command.append(f"-Duser.home={java_home}")
    command.extend(["-jar", str(JAR), *args])
    return subprocess.run(
        command,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        check=False,
    )


class ServerStats:
    def __init__(self, write_delay: float = 0.0) -> None:
        self.write_delay = write_delay
        self.lock = threading.Lock()
        self.next_conn_id = 1
        self.payload_read_conn_ids: set[int] = set()
        self.payload_write_conn_ids: set[int] = set()
        self.payload_write_paths: list[str] = []
        self.active_payload_writes = 0
        self.max_active_payload_writes = 0
        self.write_events: list[tuple[str, float, float]] = []

    def new_conn_id(self) -> int:
        with self.lock:
            conn_id = self.next_conn_id
            self.next_conn_id += 1
            return conn_id

    def record_payload_read(self, conn_id: int) -> None:
        with self.lock:
            self.payload_read_conn_ids.add(conn_id)

    def begin_payload_write(self, conn_id: int, path: str) -> None:
        with self.lock:
            self.payload_write_conn_ids.add(conn_id)
            self.payload_write_paths.append(path)
            self.active_payload_writes += 1
            self.max_active_payload_writes = max(
                self.max_active_payload_writes, self.active_payload_writes
            )

    def end_payload_write(self) -> None:
        with self.lock:
            self.active_payload_writes -= 1

    def record_write_event(self, path: str, start: float, end: float) -> None:
        with self.lock:
            self.write_events.append((path, start, end))


def is_payload_path(path: str) -> bool:
    clean = path.replace("\\", "/").lstrip("/")
    if clean.endswith("/snapshot.db") or clean in ("snapshot.db", ".kitchensync/snapshot.db"):
        return False
    if clean.startswith(".kitchensync/"):
        return clean.startswith(".kitchensync/TMP/") and not clean.endswith("/snapshot.db")
    return "/.kitchensync/TMP/" in f"/{clean}" or "/.kitchensync/" not in f"/{clean}"


class _Handle(paramiko.SFTPHandle):
    def __init__(
        self,
        flags: int,
        stats: ServerStats,
        conn_id: int,
        path: str,
        payload_write: bool,
    ) -> None:
        super().__init__(flags)
        self.stats = stats
        self.conn_id = conn_id
        self.path = path
        self.payload_write = payload_write
        self.closed_payload = False
        if payload_write:
            self.stats.begin_payload_write(conn_id, path)

    def stat(self):
        try:
            return paramiko.SFTPAttributes.from_stat(os.fstat(self.readfile.fileno()))
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def write(self, offset, data):
        start = time.monotonic()
        if self.payload_write and self.stats.write_delay:
            time.sleep(self.stats.write_delay)
        result = super().write(offset, data)
        if self.payload_write:
            self.stats.record_write_event(self.path, start, time.monotonic())
        return result

    def close(self):
        try:
            return super().close()
        finally:
            if self.payload_write and not self.closed_payload:
                self.closed_payload = True
                self.stats.end_payload_write()

    def chattr(self, attr):
        return paramiko.SFTP_OK


class _SFTPImpl(paramiko.SFTPServerInterface):
    FXF_WRITE = 0x02
    FXF_CREAT = 0x08
    FXF_TRUNC = 0x10

    def __init__(self, server, root: str, stats: ServerStats, conn_id: int):
        self.root = root
        self.stats = stats
        self.conn_id = conn_id

    def _real(self, path: str) -> str:
        return os.path.normpath(os.path.join(self.root, path.lstrip("/\\")))

    def list_folder(self, path):
        real = self._real(path)
        try:
            items = []
            for name in os.listdir(real):
                attrs = paramiko.SFTPAttributes.from_stat(os.stat(os.path.join(real, name)))
                attrs.filename = name
                items.append(attrs)
            return items
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def stat(self, path):
        try:
            return paramiko.SFTPAttributes.from_stat(os.stat(self._real(path)))
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    lstat = stat

    def open(self, path, flags, attr):
        real = self._real(path)
        try:
            Path(real).parent.mkdir(parents=True, exist_ok=True)
            access_mode = flags & 0x03
            write_intent = flags != 1 and (
                bool(flags & (self.FXF_WRITE | self.FXF_CREAT | self.FXF_TRUNC))
                or access_mode in (os.O_WRONLY, os.O_RDWR)
            )
            payload = is_payload_path(path)
            if write_intent:
                truncate = bool(flags & (self.FXF_TRUNC | os.O_TRUNC))
                mode = "w+b" if truncate or not os.path.exists(real) else "r+b"
            else:
                mode = "rb"
                if payload:
                    self.stats.record_payload_read(self.conn_id)

            handle = _Handle(flags, self.stats, self.conn_id, path, payload and write_intent)
            file_obj = open(real, mode)
            handle.readfile = file_obj
            handle.writefile = file_obj
            return handle
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def mkdir(self, path, attr):
        try:
            Path(self._real(path)).mkdir(parents=True, exist_ok=True)
            return paramiko.SFTP_OK
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def rmdir(self, path):
        try:
            os.rmdir(self._real(path))
            return paramiko.SFTP_OK
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def remove(self, path):
        try:
            os.remove(self._real(path))
            return paramiko.SFTP_OK
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def rename(self, oldpath, newpath):
        try:
            new_real = self._real(newpath)
            Path(new_real).parent.mkdir(parents=True, exist_ok=True)
            os.replace(self._real(oldpath), new_real)
            return paramiko.SFTP_OK
        except OSError as e:
            return paramiko.SFTPServer.convert_errno(e.errno)

    def chattr(self, path, attr):
        return paramiko.SFTP_OK


class _SSHImpl(paramiko.ServerInterface):
    def check_auth_password(self, username, password):
        return paramiko.AUTH_SUCCESSFUL

    def check_auth_publickey(self, username, key):
        return paramiko.AUTH_SUCCESSFUL

    def check_auth_none(self, username):
        return paramiko.AUTH_FAILED

    def check_channel_request(self, kind, chanid):
        if kind == "session":
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def get_allowed_auths(self, username):
        return "password,publickey"


def _sftp_handle_conn(
    conn: socket.socket, host_key: paramiko.PKey, root: str, stats: ServerStats
) -> None:
    conn_id = stats.new_conn_id()
    transport = paramiko.Transport(conn)
    transport.add_server_key(host_key)
    transport.set_subsystem_handler("sftp", paramiko.SFTPServer, _SFTPImpl, root, stats, conn_id)
    try:
        transport.start_server(server=_SSHImpl())
        transport.join()
    except Exception:
        pass
    finally:
        try:
            transport.close()
        except Exception:
            pass


def start_sftp_server(root: Path, stats: ServerStats | None = None) -> tuple[socket.socket, int, paramiko.PKey, ServerStats]:
    host_key = paramiko.RSAKey.generate(2048)
    server_stats = stats or ServerStats()
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", 0))
    srv.listen(50)
    port = srv.getsockname()[1]

    def accept_loop() -> None:
        while True:
            try:
                conn, _ = srv.accept()
            except OSError:
                return
            threading.Thread(
                target=_sftp_handle_conn,
                args=(conn, host_key, str(root), server_stats),
                daemon=True,
            ).start()

    threading.Thread(target=accept_loop, daemon=True).start()
    return srv, port, host_key, server_stats


def start_slow_server() -> tuple[socket.socket, int]:
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", 0))
    srv.listen(20)
    port = srv.getsockname()[1]

    def accept_loop() -> None:
        while True:
            try:
                conn, _ = srv.accept()
            except OSError:
                return
            threading.Thread(
                target=lambda c: (time.sleep(120), c.close()), args=(conn,), daemon=True
            ).start()

    threading.Thread(target=accept_loop, daemon=True).start()
    return srv, port


def kh_add(home: Path, port: int, host_key: paramiko.PKey) -> None:
    kh = home / ".ssh" / "known_hosts"
    kh.parent.mkdir(parents=True, exist_ok=True)
    entry_host = f"[127.0.0.1]:{port}"
    line = f"{entry_host} {host_key.get_name()} {host_key.get_base64()}\n"
    existing = kh.read_text(encoding="utf-8") if kh.exists() else ""
    kept = [line for line in existing.splitlines(keepends=True) if not line.startswith(f"{entry_host} ")]
    kept.append(line)
    kh.write_text("".join(kept), encoding="utf-8")


def has_overlap(events_a: list[tuple[str, float, float]], events_b: list[tuple[str, float, float]]) -> bool:
    for _, start_a, end_a in events_a:
        for _, start_b, end_b in events_b:
            if start_a < end_b and start_b < end_a:
                return True
    return False


def test_file_peer_pool_flags() -> None:
    """03.63: file:// peers sync normally; --mc/--ct/--ka flags have no effect on them."""
    tmp = Path(tempfile.mkdtemp(prefix="ks03_"))
    src = tmp / "src"
    dst = tmp / "dst"
    src.mkdir()
    dst.mkdir()
    (src / "hello.txt").write_text("hello world", encoding="utf-8")
    try:
        result = run_ks("--mc", "1", "--ct", "1", "--ka", "1", f"+{src}", str(dst))
        check(result.returncode == 0, "03.63: file:// sync with pool flags exits 0")
        check(
            (dst / "hello.txt").read_text(encoding="utf-8") == "hello world",
            "03.63: file content synced correctly despite pool flags",
        )
    finally:
        shutil.rmtree(str(tmp), ignore_errors=True)


def test_ct_timeout_uses_fallback() -> None:
    """03.62: an SFTP handshake timeout fails that URL and the next fallback URL is tried."""
    tmp = Path(tempfile.mkdtemp(prefix="ks03_"))
    home = tmp / "home"
    src = tmp / "src"
    sftp_root = tmp / "sftp"
    home.mkdir()
    src.mkdir()
    sftp_root.mkdir()
    (src / "fallback.txt").write_text("fallback", encoding="utf-8")
    slow_srv, slow_port = start_slow_server()
    good_srv, good_port, good_key, _ = start_sftp_server(sftp_root)
    kh_add(home, good_port, good_key)
    try:
        started = time.monotonic()
        result = run_ks(
            "--ct",
            "2",
            f"+{src}",
            f"[sftp://kstest:kspass@127.0.0.1:{slow_port}/dst,"
            f"sftp://kstest:kspass@127.0.0.1:{good_port}/dst]",
            java_home=home,
            timeout=30,
        )
        elapsed = time.monotonic() - started
        check(result.returncode == 0, "03.62: run succeeds through fallback after timed-out SFTP URL")
        check(elapsed < 20, f"03.62: ct=2 bounds the failed handshake before fallback (took {elapsed:.1f}s)")
        check((sftp_root / "dst" / "fallback.txt").exists(), "03.62: next fallback URL receives the file")
    finally:
        slow_srv.close()
        good_srv.close()
        shutil.rmtree(str(tmp), ignore_errors=True)


def test_per_url_ct_override() -> None:
    """03.59: per-URL ct=N overrides global --ct for that URL."""
    tmp = Path(tempfile.mkdtemp(prefix="ks03_"))
    src = tmp / "src"
    src.mkdir()
    (src / "ct.txt").write_text("ct", encoding="utf-8")
    slow_srv, slow_port = start_slow_server()
    try:
        started = time.monotonic()
        try:
            result = run_ks(
                "--ct",
                "60",
                f"+{src}",
                f"sftp://kstest:kspass@127.0.0.1:{slow_port}/dst?ct=2",
                timeout=30,
            )
        except subprocess.TimeoutExpired:
            check(False, "03.59: per-URL ct=2 did not override global ct=60")
            return
        elapsed = time.monotonic() - started
        check(result.returncode != 0, "03.59: timed-out SFTP URL without fallback fails the run")
        check(elapsed < 20, f"03.59: per-URL ct=2 is honored over global ct=60 (took {elapsed:.1f}s)")
    finally:
        slow_srv.close()
        shutil.rmtree(str(tmp), ignore_errors=True)


def test_shared_endpoint_uses_first_mc_setting() -> None:
    """
    03.58/03.96: same user@host:port with different paths shares one pool.
    03.59/03.97/03.107: the earliest peer URL's per-URL mc overrides global mc
    and later URLs for the same endpoint do not replace it.
    03.60: callers wait instead of exceeding mc.
    """
    tmp = Path(tempfile.mkdtemp(prefix="ks03_"))
    home = tmp / "home"
    src = tmp / "src"
    sftp_root = tmp / "sftp"
    home.mkdir()
    src.mkdir()
    sftp_root.mkdir()
    for index in range(4):
        (src / f"shared_{index}.txt").write_text(f"shared {index}", encoding="utf-8")

    srv, port, host_key, stats = start_sftp_server(sftp_root, ServerStats(write_delay=0.25))
    kh_add(home, port, host_key)
    try:
        result = run_ks(
            "--mc",
            "5",
            f"+{src}",
            f"sftp://kstest:kspass@127.0.0.1:{port}/left?mc=1",
            f"sftp://kstest:kspass@127.0.0.1:{port}/right?mc=5",
            java_home=home,
            timeout=60,
        )
        check(result.returncode == 0, "03.58+03.60: shared endpoint sync exits 0")
        check(
            stats.max_active_payload_writes == 1,
            f"03.58+03.97+03.107: shared endpoint obeys first mc=1 setting (max active writes {stats.max_active_payload_writes})",
        )
        present = all(
            (sftp_root / side / f"shared_{index}.txt").exists()
            for side in ("left", "right")
            for index in range(4)
        )
        check(present, "03.60: callers waiting on mc=1 eventually transfer all files")
    finally:
        srv.close()
        shutil.rmtree(str(tmp), ignore_errors=True)


def test_idle_connection_reused_within_ka() -> None:
    """03.61: a returned connection remains alive for ka seconds and is reused within that window."""
    tmp = Path(tempfile.mkdtemp(prefix="ks03_"))
    home = tmp / "home"
    src = tmp / "src"
    sftp_root = tmp / "sftp"
    home.mkdir()
    src.mkdir()
    sftp_root.mkdir()
    for index in range(3):
        (src / f"reuse_{index}.txt").write_text(f"reuse {index}", encoding="utf-8")

    srv, port, host_key, stats = start_sftp_server(sftp_root)
    kh_add(home, port, host_key)
    try:
        result = run_ks(
            "--mc",
            "1",
            "--ka",
            "60",
            f"+{src}",
            f"sftp://kstest:kspass@127.0.0.1:{port}/dst",
            java_home=home,
            timeout=60,
        )
        check(result.returncode == 0, "03.61: sync with reusable idle SFTP connection exits 0")
        check(
            len(stats.payload_write_conn_ids) == 1,
            f"03.61: all payload writes reused one transfer connection within ka (used {len(stats.payload_write_conn_ids)})",
        )
    finally:
        srv.close()
        shutil.rmtree(str(tmp), ignore_errors=True)


def test_distinct_ports_have_independent_pools_and_can_run_concurrently() -> None:
    """
    03.96: same user+host on different ports are different pool identities.
    03.100: explicit non-default SFTP ports connect to those SSH ports.
    03.101: enqueued copies execute concurrently when required connections are available.
    """
    tmp = Path(tempfile.mkdtemp(prefix="ks03_"))
    home = tmp / "home"
    src = tmp / "src"
    root_a = tmp / "sftp_a"
    root_b = tmp / "sftp_b"
    home.mkdir()
    src.mkdir()
    root_a.mkdir()
    root_b.mkdir()
    for index in range(4):
        (src / f"port_{index}.txt").write_text(f"port {index}", encoding="utf-8")

    srv_a, port_a, key_a, stats_a = start_sftp_server(root_a, ServerStats(write_delay=0.4))
    srv_b, port_b, key_b, stats_b = start_sftp_server(root_b, ServerStats(write_delay=0.4))
    kh_add(home, port_a, key_a)
    kh_add(home, port_b, key_b)
    try:
        result = run_ks(
            f"+{src}",
            f"sftp://kstest:kspass@127.0.0.1:{port_a}/pa?mc=1",
            f"sftp://kstest:kspass@127.0.0.1:{port_b}/pb?mc=1",
            java_home=home,
            timeout=60,
        )
        check(result.returncode == 0, "03.96+03.101: sync to two SFTP ports exits 0")
        check((root_a / "pa" / "port_0.txt").exists(), "03.100: explicit non-default port A receives files")
        check((root_b / "pb" / "port_0.txt").exists(), "03.100: explicit non-default port B receives files")
        check(
            has_overlap(stats_a.write_events, stats_b.write_events),
            "03.96+03.101: different port pools allow destination writes to overlap",
        )
    finally:
        srv_a.close()
        srv_b.close()
        shutil.rmtree(str(tmp), ignore_errors=True)


def test_same_endpoint_mc_allows_concurrent_transfers_when_available() -> None:
    """03.101: enqueued copies to one endpoint run concurrently up to that endpoint's mc limit."""
    tmp = Path(tempfile.mkdtemp(prefix="ks03_"))
    home = tmp / "home"
    src = tmp / "src"
    sftp_root = tmp / "sftp"
    home.mkdir()
    src.mkdir()
    sftp_root.mkdir()
    for index in range(6):
        (src / f"concurrent_{index}.txt").write_text(f"concurrent {index}", encoding="utf-8")

    srv, port, host_key, stats = start_sftp_server(sftp_root, ServerStats(write_delay=0.35))
    kh_add(home, port, host_key)
    try:
        result = run_ks(
            f"+{src}",
            f"sftp://kstest:kspass@127.0.0.1:{port}/dst?mc=2",
            java_home=home,
            timeout=60,
        )
        check(result.returncode == 0, "03.101: sync with mc=2 exits 0")
        check(
            stats.max_active_payload_writes >= 2,
            f"03.101: endpoint uses available mc=2 transfer capacity (max active writes {stats.max_active_payload_writes})",
        )
    finally:
        srv.close()
        shutil.rmtree(str(tmp), ignore_errors=True)


def test_sftp_to_sftp_borrows_source_and_destination_connections() -> None:
    """
    03.64: a transfer borrows from both source and destination pools.
    03.112: startup SFTP connections do not consume the mc=1 transfer limit.
    """
    tmp = Path(tempfile.mkdtemp(prefix="ks03_"))
    home = tmp / "home"
    root_src = tmp / "sftp_src"
    root_dst = tmp / "sftp_dst"
    home.mkdir()
    (root_src / "canon").mkdir(parents=True)
    root_dst.mkdir()
    for index in range(2):
        (root_src / "canon" / f"remote_{index}.txt").write_text(f"remote {index}", encoding="utf-8")

    srv_src, port_src, key_src, stats_src = start_sftp_server(root_src)
    srv_dst, port_dst, key_dst, stats_dst = start_sftp_server(root_dst)
    kh_add(home, port_src, key_src)
    kh_add(home, port_dst, key_dst)
    try:
        result = run_ks(
            "--mc",
            "1",
            f"+sftp://kstest:kspass@127.0.0.1:{port_src}/canon",
            f"sftp://kstest:kspass@127.0.0.1:{port_dst}/copy",
            java_home=home,
            timeout=60,
        )
        check(result.returncode == 0, "03.64+03.112: SFTP-to-SFTP sync with mc=1 exits 0")
        check(stats_src.payload_read_conn_ids, "03.64: source pool connection is used for payload reads")
        check(stats_dst.payload_write_conn_ids, "03.64: destination pool connection is used for payload writes")
        check(
            (root_dst / "copy" / "remote_0.txt").exists() and (root_dst / "copy" / "remote_1.txt").exists(),
            "03.64: payload transfers complete and connections are returned for later work",
        )
    finally:
        srv_src.close()
        srv_dst.close()
        shutil.rmtree(str(tmp), ignore_errors=True)


# Not reasonably testable from the root CLI surface:
# 03.59 (ka): per-URL ka only changes the internal idle-retirement timer; no CLI-visible
#             signal exposes the chosen timer value without waiting on implementation timing.
# 03.97/03.107 (ka): first-winning/earliest-peer selection for ka affects only the
#             internal idle-retirement timer and has no stable CLI-visible signal.
# 03.100 (omitted/default port 22): a portable offline test cannot require binding a local
#             SSH server to fixed port 22; explicit non-default ports are tested above.
# 03.106: keep-alive timer reset requires observing the internal idle timer between pool
#             borrows; one CLI run does not provide a stable public timing signal for it.
# 03.114: the transfer pool object's lazy creation point is internal and has no separate
#             CLI-visible signal.


def main() -> None:
    tests = [
        test_file_peer_pool_flags,
        test_ct_timeout_uses_fallback,
        test_per_url_ct_override,
        test_shared_endpoint_uses_first_mc_setting,
        test_idle_connection_reused_within_ka,
        test_distinct_ports_have_independent_pools_and_can_run_concurrently,
        test_same_endpoint_mc_allows_concurrent_transfers_when_available,
        test_sftp_to_sftp_borrows_source_and_destination_connections,
    ]
    for test in tests:
        try:
            test()
        except Exception as e:
            FAILURES.append(f"{test.__name__} raised {type(e).__name__}: {e}")
            print(f"FAIL: {test.__name__} raised {type(e).__name__}: {e}")

    if FAILURES:
        print(f"\n{len(FAILURES)} FAILURE(S):")
        for failure in FAILURES:
            print(f"  - {failure}")
        raise SystemExit(1)
    print("\nAll checks passed.")


if __name__ == "__main__":
    main()
