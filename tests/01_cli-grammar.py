#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko"]
# ///

from __future__ import annotations

import os
import re
import shutil
import socket
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

import paramiko

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

PROJECT_DIR = Path("C:/Users/human/Desktop/prjx/kitchensync")
JAVA = Path("C:/Users/human/Desktop/prjx/kitchensync/tools/compiler/jdk/bin/java.exe")
JAR = Path("C:/Users/human/Desktop/prjx/kitchensync/released/kitchensync.jar")
JAVA_HOME = PROJECT_DIR / "tests" / ".java-home-01-cli-grammar"
KNOWN_HOSTS = JAVA_HOME / ".ssh" / "known_hosts"
POOL_MAX_RE = re.compile(r"connections=\d+/(\d+)")


def run_cli(*args: str, timeout: int = 60) -> subprocess.CompletedProcess[str]:
    command = [str(JAVA), f"-Duser.home={JAVA_HOME}", "-jar", str(JAR), *args]
    try:
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
    except subprocess.TimeoutExpired as exc:
        stdout = exc.stdout if isinstance(exc.stdout, str) else (exc.stdout or b"").decode("utf-8", "replace")
        stderr = exc.stderr if isinstance(exc.stderr, str) else (exc.stderr or b"").decode("utf-8", "replace")
        return subprocess.CompletedProcess(command, 124, stdout, stderr)


def describe(r: subprocess.CompletedProcess[str]) -> str:
    return f"exit={r.returncode}\nstdout:\n{r.stdout}\nstderr:\n{r.stderr}"


def check(failures: list[str], cond: bool, msg: str) -> None:
    if not cond:
        failures.append(msg)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def timestamp(days_from_now: int) -> str:
    t = datetime.now(timezone.utc) + timedelta(days=days_from_now)
    return t.strftime("%Y-%m-%d_%H-%M-%S_") + f"{t.microsecond:06d}Z"


def snapshot_db(peer: Path) -> Path:
    return peer / ".kitchensync" / "snapshot.db"


def snapshot_ids(peer: Path) -> set[str]:
    con = sqlite3.connect(str(snapshot_db(peer)))
    try:
        return {str(row[0]) for row in con.execute("SELECT id FROM snapshot")}
    finally:
        con.close()


# ---------------------------------------------------------------------------
# Minimal in-process SFTP server (paramiko) for offline testing
# ---------------------------------------------------------------------------

class _SFTPHandle(paramiko.SFTPHandle):
    def __init__(self, path: Path, flags: int) -> None:
        super().__init__(flags)
        self._path = path
        if flags & os.O_RDWR:
            mode = "r+b"
        elif flags & os.O_WRONLY:
            mode = "wb"
        else:
            mode = "rb"
        if flags & os.O_CREAT:
            path.parent.mkdir(parents=True, exist_ok=True)
            if not path.exists():
                path.touch()
            if mode == "rb":
                mode = "r+b"
        if flags & os.O_TRUNC:
            path.write_bytes(b"")
            mode = "r+b"
        self._fh = path.open(mode)

    def read(self, offset: int, length: int) -> bytes | int:
        try:
            self._fh.seek(offset)
            return self._fh.read(length)
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def write(self, offset: int, data: bytes) -> int:
        try:
            self._fh.seek(offset)
            self._fh.write(data)
            self._fh.flush()
            return paramiko.SFTP_OK
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def stat(self) -> paramiko.SFTPAttributes | int:
        try:
            return paramiko.SFTPAttributes.from_stat(self._path.stat())
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def chattr(self, attr: paramiko.SFTPAttributes) -> int:
        return paramiko.SFTP_OK

    def close(self) -> None:
        try:
            self._fh.close()
        except OSError:
            pass
        super().close()


class _SFTPInterface(paramiko.SFTPServerInterface):
    def __init__(self, server: object, root: Path) -> None:
        super().__init__(server)
        self._root = root

    def _real(self, path: str) -> Path:
        return self._root / path.lstrip("/")

    def canonicalize(self, path: str) -> str:
        if not path or path == ".":
            return "/"
        return "/" + path.lstrip("/")

    def list_folder(self, path: str) -> list[paramiko.SFTPAttributes] | int:
        real = self._real(path)
        try:
            result = []
            for name in os.listdir(real):
                child = real / name
                try:
                    attr = paramiko.SFTPAttributes.from_stat(child.stat())
                except OSError:
                    attr = paramiko.SFTPAttributes()
                attr.filename = name
                result.append(attr)
            return result
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def stat(self, path: str) -> paramiko.SFTPAttributes | int:
        try:
            return paramiko.SFTPAttributes.from_stat(self._real(path).stat())
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def lstat(self, path: str) -> paramiko.SFTPAttributes | int:
        try:
            return paramiko.SFTPAttributes.from_stat(self._real(path).lstat())
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def open(self, path: str, flags: int, attr: paramiko.SFTPAttributes) -> _SFTPHandle | int:
        try:
            return _SFTPHandle(self._real(path), flags)
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def mkdir(self, path: str, attr: paramiko.SFTPAttributes) -> int:
        try:
            self._real(path).mkdir(parents=True, exist_ok=True)
            return paramiko.SFTP_OK
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def rmdir(self, path: str) -> int:
        try:
            self._real(path).rmdir()
            return paramiko.SFTP_OK
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def remove(self, path: str) -> int:
        try:
            self._real(path).unlink()
            return paramiko.SFTP_OK
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def rename(self, oldpath: str, newpath: str) -> int:
        try:
            self._real(oldpath).replace(self._real(newpath))
            return paramiko.SFTP_OK
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)


class _ServerInterface(paramiko.ServerInterface):
    def check_auth_password(self, username: str, password: str) -> int:
        return paramiko.AUTH_SUCCESSFUL

    def check_auth_publickey(self, username: str, key: paramiko.PKey) -> int:
        return paramiko.AUTH_SUCCESSFUL

    def check_channel_request(self, kind: str, chanid: int) -> int:
        return paramiko.OPEN_SUCCEEDED if kind == "session" else paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def get_allowed_auths(self, username: str) -> str:
        return "password,publickey"


class LocalSFTPServer:
    """In-process SFTP server. Manages a transient known_hosts entry so
    kitchensync accepts the local server's host key without user interaction."""

    def __init__(self, root: Path) -> None:
        self._root = root
        self._stop = threading.Event()
        self._host_key = paramiko.ECDSAKey.generate()
        self._sock: socket.socket | None = None
        self._thread: threading.Thread | None = None
        self._kh_entry = ""
        self._kh_added = False
        self.port = 0

    def __enter__(self) -> "LocalSFTPServer":
        sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        sock.bind(("127.0.0.1", 0))
        sock.listen(32)
        self.port = sock.getsockname()[1]
        self._sock = sock

        stop = self._stop

        def accept_loop() -> None:
            sock.settimeout(0.3)
            while not stop.is_set():
                try:
                    conn, _ = sock.accept()
                except socket.timeout:
                    continue
                except OSError:
                    break
                threading.Thread(target=self._serve, args=(conn,), daemon=True).start()
            sock.close()

        self._thread = threading.Thread(target=accept_loop, daemon=True)
        self._thread.start()

        key = f"{self._host_key.get_name()} {self._host_key.get_base64()}"
        entry = f"[127.0.0.1]:{self.port} {key}\n127.0.0.1 {key}\n"
        self._kh_entry = entry
        KNOWN_HOSTS.parent.mkdir(parents=True, exist_ok=True)
        existing = KNOWN_HOSTS.read_text(encoding="utf-8") if KNOWN_HOSTS.exists() else ""
        if entry not in existing:
            with KNOWN_HOSTS.open("a", encoding="utf-8") as f:
                f.write(entry)

        return self

    def _serve(self, conn: socket.socket) -> None:
        try:
            transport = paramiko.Transport(conn)
            transport.add_server_key(self._host_key)
            transport.set_subsystem_handler("sftp", paramiko.SFTPServer, _SFTPInterface, self._root)
            transport.start_server(server=_ServerInterface())
            while transport.is_active() and not self._stop.is_set():
                time.sleep(0.05)
            transport.close()
        except Exception:
            pass

    def url(self, path: str = "/") -> str:
        return f"sftp://testuser:pw@127.0.0.1:{self.port}{path}"

    def __exit__(self, *_: object) -> None:
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=3)
        if self._kh_added:
            try:
                text = KNOWN_HOSTS.read_text(encoding="utf-8")
                KNOWN_HOSTS.write_text(text.replace(self._kh_entry, ""), encoding="utf-8")
            except OSError:
                pass


# ---------------------------------------------------------------------------
# Stalled TCP listener for connect-timeout testing (01.29)
# ---------------------------------------------------------------------------

class StallSshPort:
    def __init__(self) -> None:
        self._stop = threading.Event()
        self.accepted = threading.Event()
        self.port: int | None = None
        self._thread: threading.Thread | None = None

    def __enter__(self) -> "StallSshPort":
        listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        listener.settimeout(0.2)
        self.port = listener.getsockname()[1]
        stop = self._stop
        accepted = self.accepted

        def serve() -> None:
            conns: list[socket.socket] = []
            try:
                while not stop.is_set():
                    try:
                        conn, _ = listener.accept()
                        accepted.set()
                        conns.append(conn)
                    except socket.timeout:
                        continue
            finally:
                for c in conns:
                    try:
                        c.close()
                    except OSError:
                        pass
                listener.close()

        self._thread = threading.Thread(target=serve, daemon=True)
        self._thread.start()
        return self

    def __exit__(self, *_: object) -> None:
        self._stop.set()
        try:
            with socket.create_connection(("127.0.0.1", self.port or 0), timeout=1):
                pass
        except OSError:
            pass
        if self._thread:
            self._thread.join(timeout=2)


# ---------------------------------------------------------------------------
# Individual requirement checks
# ---------------------------------------------------------------------------

def check_default_verbosity(failures: list[str], root: Path) -> None:
    # 01.31: omitting -vl defaults to info verbosity
    src = root / "vl-src"
    dst = root / "vl-dst"
    write_text(src / "f.txt", "verbosity test\n")

    result = run_cli(
        "--mc", "10", "--ct", "10", "--ka", "5",
        f"+{src.resolve().as_uri()}",
        dst.resolve().as_uri(),
        timeout=30,
    )
    out = result.stdout + result.stderr
    check(failures, result.returncode == 0, f"01.31: default -vl sync failed:\n{describe(result)}")
    # info level shows copy-progress lines; trace level additionally shows pool stats
    check(failures, "f.txt" in out,
          f"01.31: omitting -vl should produce info-level copy progress; got:\n{out!r}")
    check(failures, "connections=" not in out,
          f"01.31: omitting -vl should not expose trace-level pool stats; got:\n{out!r}")


def check_default_max_connections(failures: list[str], root: Path) -> None:
    # 01.24: omitting --mc defaults to max SFTP connections = 10
    src = root / "mc-src"
    sftp_root = root / "mc-sftp"
    write_text(src / "mc.txt", "max connections\n")
    (sftp_root / "mc-dst").mkdir(parents=True, exist_ok=True)

    with LocalSFTPServer(sftp_root) as server:
        result = run_cli(
            "-vl", "trace", "--ct", "10", "--ka", "5",
            f"+{src.resolve().as_uri()}",
            server.url("/mc-dst"),
            timeout=90,
        )

    out = result.stdout + result.stderr
    check(failures, result.returncode == 0, f"01.24: default --mc SFTP sync failed:\n{describe(result)}")
    maxes = [int(m) for m in POOL_MAX_RE.findall(out)]
    check(failures, bool(maxes),
          f"01.24: trace output should include pool stats (connections=N/M); got:\n{out!r}")
    check(failures, all(m == 10 for m in maxes),
          f"01.24: omitting --mc should default max connections to 10; found {maxes!r} in:\n{out!r}")


def check_default_connect_timeout(failures: list[str], root: Path) -> None:
    # 01.29: omitting --ct defaults SSH handshake timeout to 30 seconds.
    # A stalled TCP listener accepts the connection but never sends the SSH banner,
    # so kitchensync must wait ~30s before giving up and trying the fallback URL.
    src = root / "ct-src"
    dst = root / "ct-dst"
    write_text(src / "ct.txt", "connect timeout\n")
    dst.mkdir(parents=True, exist_ok=True)

    with StallSshPort() as stalled:
        if stalled.port is None:
            failures.append("01.29: stalled listener did not allocate a port")
            return
        stall_url = f"sftp://testuser@127.0.0.1:{stalled.port}/never"
        good_url = dst.resolve().as_uri()
        started = time.monotonic()
        result = run_cli(
            "--mc", "1", "--ka", "5",
            f"+{src.resolve().as_uri()}",
            f"[{stall_url},{good_url}]",
            timeout=75,
        )
        elapsed = time.monotonic() - started

    check(failures, stalled.accepted.is_set(),
          "01.29: default --ct test should have attempted the stalled peer")
    check(failures, result.returncode == 0,
          f"01.29: fallback sync should succeed after --ct timeout:\n{describe(result)}")
    check(failures, elapsed >= 25,
          f"01.29: omitting --ct should wait ~30s (default) before fallback; elapsed {elapsed:.2f}s")
    check(failures, elapsed < 60,
          f"01.29: default --ct fallback should complete well under 60s; elapsed {elapsed:.2f}s")


def insert_retention_rows(peer: Path, old_time: str, fresh_time: str) -> dict[str, str]:
    row_ids = {
        "old_deleted": "CLIGRAMOD01",
        "fresh_deleted": "CLIGRAMFD01",
    }
    fixtures = [
        ("old_deleted",   "__cli_grammar_old_deleted.txt",   old_time,   old_time),
        ("fresh_deleted", "__cli_grammar_fresh_deleted.txt",  fresh_time, fresh_time),
    ]
    con = sqlite3.connect(str(snapshot_db(peer)))
    con.row_factory = sqlite3.Row
    try:
        template = con.execute("SELECT * FROM snapshot LIMIT 1").fetchone()
        if template is None:
            raise RuntimeError("no template row found in snapshot")
        columns = list(template.keys())
        sql = f"INSERT INTO snapshot ({','.join(columns)}) VALUES ({','.join('?' for _ in columns)})"
        for key, basename, last_seen, deleted_time in fixtures:
            values = dict(template)
            values.update({
                "id": row_ids[key], "basename": basename, "mod_time": fresh_time,
                "byte_size": 1, "last_seen": last_seen, "deleted_time": deleted_time,
            })
            con.execute(sql, [values[col] for col in columns])
        con.commit()
    finally:
        con.close()
    return row_ids


def make_retention_dirs(level: Path) -> dict[str, Path]:
    paths = {
        "old_tmp":   level / ".kitchensync" / "TMP" / timestamp(-3),
        "fresh_tmp": level / ".kitchensync" / "TMP" / timestamp(-1),
        "old_bak":   level / ".kitchensync" / "BAK" / timestamp(-91),
        "fresh_bak": level / ".kitchensync" / "BAK" / timestamp(-89),
    }
    write_text(paths["old_tmp"]   / "uuid-old"   / "stale.txt", "old tmp\n")
    write_text(paths["fresh_tmp"] / "uuid-fresh" / "kept.txt",  "fresh tmp\n")
    write_text(paths["old_bak"]   / "stale.txt",                "old bak\n")
    write_text(paths["fresh_bak"] / "kept.txt",                 "fresh bak\n")
    return paths


def check_default_retention(failures: list[str], root: Path) -> None:
    # 01.32/01.33/01.34: --xd/--bd/--td default to 2/90/180 days
    peer_a = root / "ret-a"
    peer_b = root / "ret-b"
    peer_a.mkdir(parents=True)
    peer_b.mkdir(parents=True)
    write_text(peer_a / "seed.txt", "retention seed\n")

    setup = run_cli(f"+{peer_a.resolve().as_uri()}", peer_b.resolve().as_uri())
    check(failures, setup.returncode == 0, f"retention setup sync failed:\n{describe(setup)}")
    if not snapshot_db(peer_a).is_file():
        failures.append("01.32/33/34: snapshot.db not created by setup sync")
        return

    row_ids = insert_retention_rows(peer_a, timestamp(-181), timestamp(-179))
    dirs = make_retention_dirs(peer_a)

    result = run_cli(f"+{peer_a.resolve().as_uri()}", peer_b.resolve().as_uri())
    check(failures, result.returncode == 0, f"default retention sync failed:\n{describe(result)}")

    ids_after = snapshot_ids(peer_a) if snapshot_db(peer_a).is_file() else set()

    # 01.34: --td defaults to 180 days
    check(failures, row_ids["old_deleted"] not in ids_after,
          "01.34: omitting --td should purge tombstones older than 180 days")
    check(failures, row_ids["fresh_deleted"] in ids_after,
          "01.34: omitting --td should keep tombstones newer than 180 days")

    # 01.32: --xd defaults to 2 days
    check(failures, not dirs["old_tmp"].exists(),
          "01.32: omitting --xd should remove TMP dirs older than 2 days")
    check(failures, dirs["fresh_tmp"].is_dir(),
          "01.32: omitting --xd should keep TMP dirs newer than 2 days")

    # 01.33: --bd defaults to 90 days
    check(failures, not dirs["old_bak"].exists(),
          "01.33: omitting --bd should remove BAK dirs older than 90 days")
    check(failures, dirs["fresh_bak"].is_dir(),
          "01.33: omitting --bd should keep BAK dirs newer than 90 days")


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------

def main() -> int:
    failures: list[str] = []
    if JAVA_HOME.exists():
        shutil.rmtree(JAVA_HOME)
    KNOWN_HOSTS.parent.mkdir(parents=True, exist_ok=True)
    KNOWN_HOSTS.write_text("", encoding="utf-8")

    # 01.30: --ka omitted -> SFTP idle keep-alive TTL defaults to 30 seconds.
    # not reasonably testable: requires observing an internal SFTP pool's idle
    # connection lifetime across a 30s gap -- no observable CLI surface.

    with tempfile.TemporaryDirectory(prefix="ks01_cli_grammar_", dir=str(PROJECT_DIR / "tests")) as tmp:
        root = Path(tmp)
        try:
            check_default_verbosity(failures, root)
            check_default_max_connections(failures, root)
            check_default_connect_timeout(failures, root)
            check_default_retention(failures, root)
        except Exception as exc:
            failures.append(f"unexpected error: {type(exc).__name__}: {exc}")

    if failures:
        print("FAILURES:", file=sys.stderr)
        for f in failures:
            print(f"\n- {f}", file=sys.stderr)
        return 1
    print("01_cli-grammar passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
