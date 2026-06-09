# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko>=3.4", "cryptography"]
# ///

"""
End-to-end tests for 016_snapshot-storage.

Verifies: snapshot path and SQLite format, SWAP writeback cleanup,
five SWAP recovery states, and rename-rejecting SFTP transport compatibility.
"""

import sys

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

import os
import pathlib
import platform
import shutil
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time

import paramiko

# ---- constants ----

WORKSPACE = pathlib.Path("/home/ace/Desktop/prjx/kitchensync")
EXE = WORKSPACE / "released" / "kitchensync.exe"

_plat = platform.system()
if _plat == "Windows":
    UV = WORKSPACE / "aitc" / "bin" / "uv.exe"
elif _plat == "Darwin":
    UV = WORKSPACE / "aitc" / "bin" / "uv.mac"
else:
    UV = WORKSPACE / "aitc" / "bin" / "uv.linux"

# ---- failure collection ----

_failures: list[str] = []


def fail(msg: str) -> None:
    _failures.append(msg)
    print(f"FAIL: {msg}", flush=True)


def check(cond: bool, msg: str) -> bool:
    if not cond:
        fail(msg)
    return cond


# ---- subprocess helpers ----

def run_ks(*args, timeout: int = 90) -> subprocess.CompletedProcess:
    cmd = [str(EXE)] + [str(a) for a in args]
    return subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        timeout=timeout,
        encoding="utf-8",
        errors="replace",
    )


# ---- filesystem helpers ----

def make_peers(base: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
    peer_a = base / "peer_a"
    peer_b = base / "peer_b"
    peer_a.mkdir(parents=True, exist_ok=True)
    peer_b.mkdir(parents=True, exist_ok=True)
    return peer_a, peer_b


def initial_sync(peer_a: pathlib.Path, peer_b: pathlib.Path) -> subprocess.CompletedProcess:
    """First-run sync with peer_a as canon; seeds both peers with a file and snapshot."""
    (peer_a / "seed.txt").write_text("seed file for snapshot test")
    return run_ks(f"+{peer_a}", str(peer_b))


def copy_snapshot(peer: pathlib.Path, dest: pathlib.Path) -> None:
    shutil.copy2(str(peer / ".kitchensync" / "snapshot.db"), str(dest))


def setup_swap_dir(peer: pathlib.Path) -> pathlib.Path:
    """Create and return the SWAP/snapshot.db/ directory under peer's .kitchensync."""
    swap = peer / ".kitchensync" / "SWAP" / "snapshot.db"
    swap.mkdir(parents=True, exist_ok=True)
    return swap


def is_sqlite(path: pathlib.Path) -> bool:
    if not path.exists():
        return False
    try:
        return path.read_bytes()[:16].startswith(b"SQLite format 3\x00")
    except OSError:
        return False


def can_open_snapshot_table(path: pathlib.Path) -> bool:
    if not path.exists():
        return False
    try:
        uri = path.as_uri() + "?mode=ro"
        con = sqlite3.connect(uri, uri=True)
        try:
            con.execute("SELECT COUNT(*) FROM snapshot").fetchone()
            return True
        finally:
            con.close()
    except Exception:
        return False


def journal_mode(path: pathlib.Path) -> str:
    if not path.exists():
        return "error"
    try:
        uri = path.as_uri() + "?mode=ro"
        con = sqlite3.connect(uri, uri=True)
        try:
            return con.execute("PRAGMA journal_mode").fetchone()[0]
        finally:
            con.close()
    except Exception:
        return "error"


# ---- in-process SFTP server that rejects rename-over-existing (016.12) ----

class _AnyAuthServer(paramiko.ServerInterface):
    def check_channel_request(self, kind: str, chanid: int) -> int:
        if kind == "session":
            return paramiko.OPEN_SUCCEEDED
        return paramiko.OPEN_FAILED_ADMINISTRATIVELY_PROHIBITED

    def get_allowed_auths(self, username: str) -> str:
        return "password"

    def check_auth_password(self, username: str, password: str) -> int:
        return paramiko.AUTH_SUCCESSFUL


class _SFTPHandle(paramiko.SFTPHandle):
    def stat(self):
        try:
            return paramiko.SFTPAttributes.from_stat(os.fstat(self.readfile.fileno()))
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def chattr(self, attr):
        return paramiko.SFTP_OK


class _RejectRenameSFTP(paramiko.SFTPServerInterface):
    ROOT: str = ""

    def _real(self, path: str) -> str:
        return self.ROOT + self.canonicalize(path)

    def list_folder(self, path: str):
        real = self._real(path)
        try:
            out = []
            for name in os.listdir(real):
                attr = paramiko.SFTPAttributes.from_stat(
                    os.stat(os.path.join(real, name))
                )
                attr.filename = name
                out.append(attr)
            return out
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def stat(self, path: str):
        try:
            return paramiko.SFTPAttributes.from_stat(os.stat(self._real(path)))
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)

    def lstat(self, path: str):
        return self.stat(path)

    def open(self, path: str, flags: int, attr):
        real = self._real(path)
        try:
            fd = os.open(real, flags | getattr(os, "O_BINARY", 0), 0o666)
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        if (flags & os.O_CREAT) and attr is not None:
            attr._flags &= ~attr.FLAG_PERMISSIONS
            paramiko.SFTPServer.set_file_attr(real, attr)
        if flags & os.O_WRONLY:
            fmode = "ab" if (flags & os.O_APPEND) else "wb"
        elif flags & os.O_RDWR:
            fmode = "a+b" if (flags & os.O_APPEND) else "r+b"
        else:
            fmode = "rb"
        try:
            fobj = os.fdopen(fd, fmode)
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        h = _SFTPHandle(flags)
        h.filename = real
        h.readfile = fobj
        h.writefile = fobj
        return h

    def remove(self, path: str) -> int:
        try:
            os.remove(self._real(path))
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        return paramiko.SFTP_OK

    def rename(self, oldpath: str, newpath: str) -> int:
        real_new = self._real(newpath)
        if os.path.exists(real_new):
            return paramiko.SFTP_FAILURE  # reject rename when destination exists
        try:
            os.rename(self._real(oldpath), real_new)
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        return paramiko.SFTP_OK

    def mkdir(self, path: str, attr) -> int:
        real = self._real(path)
        try:
            os.mkdir(real)
            if attr is not None:
                paramiko.SFTPServer.set_file_attr(real, attr)
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        return paramiko.SFTP_OK

    def rmdir(self, path: str) -> int:
        try:
            os.rmdir(self._real(path))
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        return paramiko.SFTP_OK

    def chattr(self, path: str, attr) -> int:
        try:
            paramiko.SFTPServer.set_file_attr(self._real(path), attr)
        except OSError as exc:
            return paramiko.SFTPServer.convert_errno(exc.errno)
        return paramiko.SFTP_OK


def _handle_sftp_client(client: socket.socket, host_key: paramiko.PKey) -> None:
    transport = paramiko.Transport(client)
    try:
        transport.add_server_key(host_key)
        transport.set_subsystem_handler("sftp", paramiko.SFTPServer, _RejectRenameSFTP)
        transport.start_server(server=_AnyAuthServer())
        channel = transport.accept(timeout=30)
        if channel is None:
            return
        while transport.is_active():
            time.sleep(0.2)
    except Exception:
        pass
    finally:
        try:
            transport.close()
        except Exception:
            pass


class RenameRejectingServer:
    """In-process SFTP server whose rename fails when the destination already exists."""

    def __init__(self, root: pathlib.Path) -> None:
        self._host_key = paramiko.RSAKey.generate(2048)
        _RejectRenameSFTP.ROOT = str(root)
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._sock.bind(("127.0.0.1", 0))
        self._sock.listen(10)
        self._sock.settimeout(1.0)
        self.port: int = self._sock.getsockname()[1]
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._accept_loop, daemon=True)
        self._thread.start()

    @property
    def known_hosts_entry(self) -> str:
        return (
            f"[127.0.0.1]:{self.port} "
            f"{self._host_key.get_name()} "
            f"{self._host_key.get_base64()}"
        )

    def _accept_loop(self) -> None:
        while not self._stop.is_set():
            try:
                client, _ = self._sock.accept()
            except socket.timeout:
                continue
            except OSError:
                break
            threading.Thread(
                target=_handle_sftp_client,
                args=(client, self._host_key),
                daemon=True,
            ).start()

    def stop(self) -> None:
        self._stop.set()
        try:
            self._sock.close()
        except OSError:
            pass


def add_known_hosts_entry(entry: str) -> None:
    kh = pathlib.Path.home() / ".ssh" / "known_hosts"
    kh.parent.mkdir(parents=True, exist_ok=True)
    existing = kh.read_text(encoding="utf-8", errors="replace") if kh.exists() else ""
    if entry not in existing:
        with kh.open("a", encoding="utf-8") as f:
            f.write(entry + "\n")


def remove_known_hosts_entry(entry: str) -> None:
    kh = pathlib.Path.home() / ".ssh" / "known_hosts"
    if not kh.exists():
        return
    lines = kh.read_text(encoding="utf-8", errors="replace").splitlines(keepends=True)
    lines = [ln for ln in lines if ln.rstrip("\n") != entry]
    kh.write_text("".join(lines), encoding="utf-8")


# ---- tests ----

def test_snapshot_created_and_format() -> None:
    """016.1, 016.2, 016.3, 016.6, 016.7"""
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = pathlib.Path(tmp_str)
        peer_a, peer_b = make_peers(tmp)

        # 016.6: no prior snapshot.db; sync creates one
        r = initial_sync(peer_a, peer_b)
        if not check(r.returncode == 0,
                     f"016.1/016.6: initial sync failed: {r.stdout}"):
            return

        snap_a = peer_a / ".kitchensync" / "snapshot.db"
        snap_b = peer_b / ".kitchensync" / "snapshot.db"

        # 016.1: snapshot.db at {peer-root}/.kitchensync/snapshot.db
        check(snap_a.exists(), "016.1: snapshot.db missing from peer_a")
        check(snap_b.exists(), "016.1: snapshot.db missing from peer_b")

        # 016.2: SQLite file, rollback-journal mode (not WAL)
        check(is_sqlite(snap_a), "016.2: peer_a snapshot.db is not a valid SQLite file")
        check(is_sqlite(snap_b), "016.2: peer_b snapshot.db is not a valid SQLite file")
        jm_a = journal_mode(snap_a)
        check(jm_a not in ("wal", "error"),
              f"016.2: peer_a snapshot.db not in rollback-journal mode (got: '{jm_a}')")
        jm_b = journal_mode(snap_b)
        check(jm_b not in ("wal", "error"),
              f"016.2: peer_b snapshot.db not in rollback-journal mode (got: '{jm_b}')")

        # 016.3: no SQLite sidecar files on peers
        for sidecar in ("-wal", "-shm", "-journal"):
            check(not (peer_a / ".kitchensync" / f"snapshot.db{sidecar}").exists(),
                  f"016.3: sidecar snapshot.db{sidecar} found on peer_a")
            check(not (peer_b / ".kitchensync" / f"snapshot.db{sidecar}").exists(),
                  f"016.3: sidecar snapshot.db{sidecar} found on peer_b")

        # 016.7: snapshot.db is self-contained; 'snapshot' table is queryable
        check(can_open_snapshot_table(snap_a),
              "016.7: peer_a snapshot.db: cannot query 'snapshot' table")
        check(can_open_snapshot_table(snap_b),
              "016.7: peer_b snapshot.db: cannot query 'snapshot' table")


def test_swap_writeback_cleanup() -> None:
    """016.8, 016.9, 016.10, 016.11: SWAP artifacts removed after successful writeback."""
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = pathlib.Path(tmp_str)
        peer_a, peer_b = make_peers(tmp)

        r = initial_sync(peer_a, peer_b)
        if not check(r.returncode == 0,
                     f"016.8-016.11: initial sync failed: {r.stdout}"):
            return

        # Second sync triggers full SWAP cycle on peer_b (snapshot.db now exists)
        (peer_a / "second.txt").write_text("added before second run")
        r = run_ks(str(peer_a), str(peer_b))
        if not check(r.returncode == 0,
                     f"016.8-016.11: second sync failed: {r.stdout}"):
            return

        for peer, label in ((peer_a, "peer_a"), (peer_b, "peer_b")):
            swap = peer / ".kitchensync" / "SWAP" / "snapshot.db"
            check(not (swap / "new").exists(),
                  f"016.8/016.10: SWAP new not removed after writeback on {label}")
            check(not (swap / "old").exists(),
                  f"016.9/016.11: SWAP old not removed after writeback on {label}")


def test_recovery_old_and_snapshot_db_exist() -> None:
    """016.13, 016.14: old + snapshot.db exist — delete new if present, then delete old."""
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = pathlib.Path(tmp_str)
        peer_a, peer_b = make_peers(tmp)

        r = initial_sync(peer_a, peer_b)
        if not check(r.returncode == 0, "016.14: initial sync failed"):
            return

        # Simulate incomplete writeback: old + new in SWAP, snapshot.db still present
        swap = setup_swap_dir(peer_b)
        (swap / "old").write_bytes(b"placeholder-old")
        (swap / "new").write_bytes(b"placeholder-new")

        r = run_ks(str(peer_a), str(peer_b))
        if not check(r.returncode == 0,
                     f"016.14: sync after recovery state failed: {r.stdout}"):
            return

        check(not (swap / "old").exists(),
              "016.14: SWAP old not deleted (old + snapshot.db recovery)")
        check(not (swap / "new").exists(),
              "016.14: SWAP new not deleted (old + snapshot.db + new recovery)")
        check((peer_b / ".kitchensync" / "snapshot.db").exists(),
              "016.14: snapshot.db missing after recovery")


def test_recovery_old_and_new_no_snapshot() -> None:
    """016.13, 016.15: old + new exist, snapshot.db missing — rename new to snapshot.db, delete old."""
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = pathlib.Path(tmp_str)
        peer_a, peer_b = make_peers(tmp)

        r = initial_sync(peer_a, peer_b)
        if not check(r.returncode == 0, "016.15: initial sync failed"):
            return

        valid_snap = tmp / "valid_snap.db"
        copy_snapshot(peer_b, valid_snap)

        snap_b = peer_b / ".kitchensync" / "snapshot.db"
        swap = setup_swap_dir(peer_b)
        shutil.copy2(str(valid_snap), str(swap / "new"))
        (swap / "old").write_bytes(b"placeholder-old")
        snap_b.unlink()

        r = run_ks(str(peer_a), str(peer_b))
        if not check(r.returncode == 0,
                     f"016.15: sync after recovery state failed: {r.stdout}"):
            return

        check(not (swap / "old").exists(),
              "016.15: SWAP old not deleted after recovery")
        check(not (swap / "new").exists(),
              "016.15: SWAP new not moved after recovery")
        check(snap_b.exists(),
              "016.15: snapshot.db not created from SWAP new during recovery")


def test_recovery_old_only_no_snapshot() -> None:
    """016.13, 016.16: old exists, new missing, snapshot.db missing — rename old to snapshot.db."""
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = pathlib.Path(tmp_str)
        peer_a, peer_b = make_peers(tmp)

        r = initial_sync(peer_a, peer_b)
        if not check(r.returncode == 0, "016.16: initial sync failed"):
            return

        valid_snap = tmp / "valid_snap.db"
        copy_snapshot(peer_b, valid_snap)

        snap_b = peer_b / ".kitchensync" / "snapshot.db"
        swap = setup_swap_dir(peer_b)
        shutil.copy2(str(valid_snap), str(swap / "old"))
        snap_b.unlink()

        r = run_ks(str(peer_a), str(peer_b))
        if not check(r.returncode == 0,
                     f"016.16: sync after recovery state failed: {r.stdout}"):
            return

        check(not (swap / "old").exists(),
              "016.16: SWAP old not renamed to snapshot.db during recovery")
        check(not (swap / "new").exists(),
              "016.16: unexpected SWAP new after recovery")
        check(snap_b.exists(),
              "016.16: snapshot.db not recreated from SWAP old during recovery")


def test_recovery_new_snapshot_db_exists() -> None:
    """016.13, 016.17: old missing, new exists, snapshot.db exists — delete new."""
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = pathlib.Path(tmp_str)
        peer_a, peer_b = make_peers(tmp)

        r = initial_sync(peer_a, peer_b)
        if not check(r.returncode == 0, "016.17: initial sync failed"):
            return

        snap_b = peer_b / ".kitchensync" / "snapshot.db"
        swap = setup_swap_dir(peer_b)
        (swap / "new").write_bytes(b"placeholder-new")
        # snapshot.db still present; no old

        r = run_ks(str(peer_a), str(peer_b))
        if not check(r.returncode == 0,
                     f"016.17: sync after recovery state failed: {r.stdout}"):
            return

        check(not (swap / "new").exists(),
              "016.17: SWAP new not deleted (old missing, snapshot.db exists recovery)")
        check(not (swap / "old").exists(),
              "016.17: unexpected SWAP old after recovery")
        check(snap_b.exists(),
              "016.17: snapshot.db missing after recovery")


def test_recovery_new_only_no_snapshot() -> None:
    """016.13, 016.18: old missing, new exists, snapshot.db missing — rename new to snapshot.db."""
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = pathlib.Path(tmp_str)
        peer_a, peer_b = make_peers(tmp)

        r = initial_sync(peer_a, peer_b)
        if not check(r.returncode == 0, "016.18: initial sync failed"):
            return

        valid_snap = tmp / "valid_snap.db"
        copy_snapshot(peer_b, valid_snap)

        snap_b = peer_b / ".kitchensync" / "snapshot.db"
        swap = setup_swap_dir(peer_b)
        shutil.copy2(str(valid_snap), str(swap / "new"))
        snap_b.unlink()
        # no old

        r = run_ks(str(peer_a), str(peer_b))
        if not check(r.returncode == 0,
                     f"016.18: sync after recovery state failed: {r.stdout}"):
            return

        check(not (swap / "new").exists(),
              "016.18: SWAP new not renamed to snapshot.db during recovery")
        check(not (swap / "old").exists(),
              "016.18: unexpected SWAP old after recovery")
        check(snap_b.exists(),
              "016.18: snapshot.db not recreated from SWAP new during recovery")


def test_sftp_rename_reject_compat() -> None:
    """016.12: Snapshot writeback succeeds on transport that rejects rename-over-existing."""
    with tempfile.TemporaryDirectory() as tmp_str:
        tmp = pathlib.Path(tmp_str)

        peer_a = tmp / "peer_a"
        peer_a.mkdir()
        (peer_a / "file1.txt").write_text("data for 016.12 rename-reject test")

        sftp_root = tmp / "sftp_root"
        sftp_root.mkdir()
        peer_b_dir = sftp_root / "peer_b"
        peer_b_dir.mkdir()

        server = RenameRejectingServer(sftp_root)
        kh_entry = server.known_hosts_entry
        add_known_hosts_entry(kh_entry)
        try:
            peer_b_url = f"sftp://tester:anypass@127.0.0.1:{server.port}/peer_b"

            # First sync: no existing snapshot.db on SFTP peer — SWAP write + rename-to-new-path
            r = run_ks(f"+{peer_a}", peer_b_url, timeout=120)
            if not check(r.returncode == 0,
                         f"016.12: first SFTP sync failed on rename-rejecting server: "
                         f"{r.stdout}"):
                return

            check(
                (peer_b_dir / ".kitchensync" / "snapshot.db").exists(),
                "016.12: snapshot.db not created on SFTP peer after first sync",
            )

            # Second sync: snapshot.db exists on peer_b; SWAP writeback must use the
            # write-new / move-old / rename-new / delete-old sequence, not rename-over-existing.
            (peer_a / "file2.txt").write_text("second file for 016.12")
            r = run_ks(str(peer_a), peer_b_url, timeout=120)
            check(
                r.returncode == 0,
                f"016.12: second SFTP sync failed on rename-rejecting server: {r.stdout}",
            )

            snap_b = peer_b_dir / ".kitchensync" / "snapshot.db"
            check(snap_b.exists(),
                  "016.12: snapshot.db missing from SFTP peer after second sync")

            swap = peer_b_dir / ".kitchensync" / "SWAP" / "snapshot.db"
            check(not (swap / "new").exists(),
                  "016.12: SWAP new not cleaned up after rename-rejecting SFTP sync")
            check(not (swap / "old").exists(),
                  "016.12: SWAP old not cleaned up after rename-rejecting SFTP sync")
        finally:
            server.stop()
            remove_known_hosts_entry(kh_entry)


# not reasonably testable: 016.4 -- temp download path {tmp}/{uuid}/snapshot.db is process-internal
# not reasonably testable: 016.5 -- local-copy-only modification before writeback is inobservable
# not reasonably testable: 016.19 -- overlapping concurrent runs not deterministically reproducible
# not reasonably testable: 016.20 -- upload failure before old exists requires fault injection
# not reasonably testable: 016.21 -- upload failure after old exists requires fault injection


# ---- main ----

def main() -> int:
    tests = [
        test_snapshot_created_and_format,
        test_swap_writeback_cleanup,
        test_recovery_old_and_snapshot_db_exist,
        test_recovery_old_and_new_no_snapshot,
        test_recovery_old_only_no_snapshot,
        test_recovery_new_snapshot_db_exists,
        test_recovery_new_only_no_snapshot,
        test_sftp_rename_reject_compat,
    ]

    for test_fn in tests:
        print(f"\n--- {test_fn.__name__} ---", flush=True)
        try:
            test_fn()
        except subprocess.TimeoutExpired as exc:
            fail(f"{test_fn.__name__}: timed out: {exc}")
        except Exception as exc:
            fail(f"{test_fn.__name__}: unexpected exception: {exc}")

    if _failures:
        print(f"\n{len(_failures)} check(s) FAILED:", flush=True)
        for f in _failures:
            print(f"  - {f}", flush=True)
        return 1

    print("\nAll checks passed.", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
