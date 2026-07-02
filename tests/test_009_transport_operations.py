# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import platform
import queue
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

LITERAL_WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
WORKSPACE_ROOT = (
    LITERAL_WORKSPACE_ROOT
    if LITERAL_WORKSPACE_ROOT.exists()
    else Path(__file__).resolve().parents[1]
)
KITCHENSYNC_EXE = WORKSPACE_ROOT / "released" / "kitchensync.exe"


def bundled_uv() -> Path:
    system = platform.system().lower()
    if system == "windows":
        return WORKSPACE_ROOT / "aitc" / "bin" / "uv.exe"
    if system == "darwin":
        return WORKSPACE_ROOT / "aitc" / "bin" / "uv.mac"
    return WORKSPACE_ROOT / "aitc" / "bin" / "uv.linux"


def record(failures: list[str], condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def run_kitchensync(args: list[str], env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(KITCHENSYNC_EXE), *args],
        cwd=str(WORKSPACE_ROOT),
        env=env,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=45,
        shell=False,
        check=False,
    )


def write_bytes(path: Path, data: bytes, mod_time: float) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    os.utime(path, (mod_time, mod_time))


def read_bytes(path: Path) -> bytes | None:
    try:
        return path.read_bytes()
    except OSError:
        return None


def close_enough_mtime(actual: float, expected: float) -> bool:
    return abs(actual - expected) <= 5.0


def find_bak_entry(peer: Path, name: str) -> list[Path]:
    bak = peer / ".kitchensync" / "BAK"
    if not bak.exists():
        return []
    return [path for path in bak.glob(f"*/{name}") if path.exists()]


def snapshot_rows(peer: Path) -> list[dict[str, object]]:
    db = peer / ".kitchensync" / "snapshot.db"
    if not db.exists():
        return []
    try:
        with sqlite3.connect(str(db)) as conn:
            conn.row_factory = sqlite3.Row
            rows = conn.execute(
                "SELECT basename, byte_size, mod_time, last_seen, deleted_time FROM snapshot"
            ).fetchall()
    except sqlite3.Error:
        return []
    return [dict(row) for row in rows]


def row_for(rows: list[dict[str, object]], basename: str) -> dict[str, object] | None:
    for row in rows:
        if row.get("basename") == basename:
            return row
    return None


def prepare_local_source(peer: Path) -> dict[str, float]:
    base = time.time() - 7200
    write_bytes(peer / "root.txt", b"root file\n", base + 10)
    write_bytes(peer / "nested" / "child.bin", b"0123456789", base + 20)
    write_bytes(peer / "nested" / "deeper" / "leaf.txt", b"leaf\n", base + 30)
    write_bytes(peer / "replace.txt", b"new replacement\n", base + 40)
    write_bytes(peer / "type_conflict", b"file wins\n", base + 50)
    (peer / "empty_dir").mkdir(parents=True, exist_ok=True)
    os.utime(peer / "empty_dir", (base + 60, base + 60))
    return {
        "root.txt": base + 10,
        "child.bin": base + 20,
        "leaf.txt": base + 30,
        "replace.txt": base + 40,
        "type_conflict": base + 50,
        "empty_dir": base + 60,
    }


def check_local_transport(failures: list[str], tmp: Path) -> None:
    tmp.mkdir(parents=True, exist_ok=True)
    peer_a = tmp / "local-a"
    peer_b = tmp / "local-b"
    peer_a.mkdir()
    peer_b.mkdir()
    mtimes = prepare_local_source(peer_a)

    write_bytes(tmp / "outside-root.txt", b"outside\n", mtimes["root.txt"] + 100)
    write_bytes(peer_b / "replace.txt", b"old replacement\n", mtimes["root.txt"])
    write_bytes(peer_b / "extra.txt", b"remove me\n", mtimes["root.txt"])
    write_bytes(peer_b / "type_conflict" / "kept_in_bak.txt", b"directory loser\n", mtimes["root.txt"])

    result = run_kitchensync([f"+{peer_a}", str(peer_b)])
    record(failures, result.returncode == 0, "009 local sync should exit 0")
    record(failures, result.stderr == "", "009 local sync should keep stderr empty")
    record(failures, "sync complete" in result.stdout.splitlines(), "009 local sync should report completion")

    record(failures, read_bytes(peer_b / "root.txt") == b"root file\n", "009.1, 009.25-009.34 local file copy should preserve root file bytes")
    record(failures, read_bytes(peer_b / "nested" / "child.bin") == b"0123456789", "009.5-009.13 local traversal should copy immediate child file with size metadata")
    record(failures, read_bytes(peer_b / "nested" / "deeper" / "leaf.txt") == b"leaf\n", "009.31 local open_write should create missing parent directories")
    record(failures, (peer_b / "empty_dir").is_dir(), "009.39-009.40 local create_dir should create missing directories")
    record(failures, not (peer_b / "outside-root.txt").exists(), "009.3-009.4 operations should stay scoped to relative peer-root paths")
    record(failures, read_bytes(peer_b / "replace.txt") == b"new replacement\n", "009.35 and 009.37 local replacement should use safe rename-to-missing flow")
    record(failures, read_bytes(peer_b / "type_conflict") == b"file wins\n", "009.16-009.24 local stat should distinguish file from directory in a type conflict")
    record(failures, not (peer_b / "extra.txt").exists(), "009.35 and 009.38 local canon deletion should remove the obsolete live file path")

    extra_bak = find_bak_entry(peer_b, "extra.txt")
    conflict_bak = find_bak_entry(peer_b, "type_conflict")
    record(failures, bool(extra_bak), "009.35 local same-filesystem rename should displace obsolete file to BAK")
    record(failures, any((path / "kept_in_bak.txt").exists() for path in conflict_bak), "009.36 local directory displacement should preserve subtree contents")

    for rel, expected in (
        ("root.txt", mtimes["root.txt"]),
        ("nested/child.bin", mtimes["child.bin"]),
        ("nested/deeper/leaf.txt", mtimes["leaf.txt"]),
        ("replace.txt", mtimes["replace.txt"]),
    ):
        dst = peer_b / rel
        record(failures, dst.exists() and close_enough_mtime(dst.stat().st_mtime, expected), f"009.17, 009.42 local file mod_time should be preserved for {rel}")

    rows = snapshot_rows(peer_b)
    root_row = row_for(rows, "root.txt")
    child_row = row_for(rows, "child.bin")
    nested_row = row_for(rows, "nested")
    empty_dir_row = row_for(rows, "empty_dir")
    record(failures, root_row is not None and root_row.get("byte_size") == len(b"root file\n"), "009.8-009.9 local snapshot should record regular file size and mod_time")
    record(failures, child_row is not None and child_row.get("byte_size") == 10, "009.6-009.9 local listing should report nested regular file name and byte size")
    record(failures, nested_row is not None and nested_row.get("byte_size") == -1, "009.10-009.13 local listing should report directory name and byte size -1")
    record(failures, empty_dir_row is not None and empty_dir_row.get("byte_size") == -1, "009.19-009.21 local stat/listing should record existing directory metadata")

    second = run_kitchensync([str(peer_a), str(peer_b)])
    record(failures, second.returncode == 0, "009.22 and 009.44 missing local snapshot paths should converge through not-found handling")
    record(failures, second.stderr == "", "009 repeated local sync should keep stderr empty")


class SftpServer:
    def __init__(self, failures: list[str], tmp: Path) -> None:
        self.failures = failures
        self.tmp = tmp
        self.proc: subprocess.Popen[str] | None = None
        self.stderr_lines: queue.Queue[str] = queue.Queue()
        self.port: int | None = None
        self.host_key: str | None = None

    def __enter__(self) -> "SftpServer":
        self.proc = subprocess.Popen(
            [
                str(bundled_uv()),
                "run",
                "--script",
                str(WORKSPACE_ROOT / "extart" / "ephemeral-sftp-server.py"),
                "--user",
                "ks",
                "--password",
                "pw",
            ],
            cwd=str(WORKSPACE_ROOT),
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            shell=False,
        )
        assert self.proc.stdout is not None
        assert self.proc.stderr is not None
        stdout_lines: queue.Queue[str] = queue.Queue()

        def collect_stdout() -> None:
            assert self.proc is not None
            assert self.proc.stdout is not None
            for line in self.proc.stdout:
                stdout_lines.put(line.rstrip("\n"))

        def collect_stderr() -> None:
            assert self.proc is not None
            assert self.proc.stderr is not None
            for line in self.proc.stderr:
                self.stderr_lines.put(line.rstrip("\n"))

        threading.Thread(target=collect_stdout, daemon=True).start()
        threading.Thread(target=collect_stderr, daemon=True).start()
        try:
            line = stdout_lines.get(timeout=20.0).strip()
        except queue.Empty:
            self.failures.append("SFTP server should print its port within 20 seconds")
            return self
        try:
            self.port = int(line)
        except ValueError:
            self.failures.append(f"SFTP server should print a port number, got {line!r}")
            return self

        deadline = time.monotonic() + 10.0
        while time.monotonic() < deadline and self.host_key is None:
            try:
                err_line = self.stderr_lines.get(timeout=0.2)
            except queue.Empty:
                continue
            if err_line.startswith("host key: "):
                self.host_key = err_line.removeprefix("host key: ")
        record(self.failures, self.host_key is not None, "SFTP server should report a host key for known_hosts")
        return self

    def __exit__(self, _exc_type: object, _exc: object, _tb: object) -> None:
        if self.proc is None:
            return
        self.proc.terminate()
        try:
            self.proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait(timeout=10)

    def env(self) -> dict[str, str]:
        home = self.tmp / "home"
        ssh = home / ".ssh"
        ssh.mkdir(parents=True, exist_ok=True)
        known_hosts = ssh / "known_hosts"
        known_hosts.write_text(f"[127.0.0.1]:{self.port} {self.host_key}\n", encoding="ascii", newline="\n")
        env = os.environ.copy()
        env["HOME"] = str(home)
        env["USERPROFILE"] = str(home)
        env["HOMEDRIVE"] = ""
        env["HOMEPATH"] = str(home)
        env.pop("SSH_AUTH_SOCK", None)
        return env

    def url(self, root: str = "/transport-root") -> str:
        return f"sftp://ks:pw@127.0.0.1:{self.port}{root}"


def check_sftp_transport(failures: list[str], tmp: Path) -> None:
    tmp.mkdir(parents=True, exist_ok=True)
    with SftpServer(failures, tmp) as server:
        if server.port is None or server.host_key is None:
            return
        env = server.env()
        source = tmp / "sftp-source"
        roundtrip = tmp / "sftp-roundtrip"
        source.mkdir()
        roundtrip.mkdir()
        mtimes = prepare_local_source(source)

        upload = run_kitchensync([f"+{source}", server.url()], env=env)
        record(failures, upload.returncode == 0, "009.2 SFTP upload sync should exit 0")
        record(failures, upload.stderr == "", "009.2 SFTP upload should keep stderr empty")
        record(failures, "sync complete" in upload.stdout.splitlines(), "009.2 SFTP upload should report completion")

        download = run_kitchensync([f"+{server.url()}", str(roundtrip)], env=env)
        record(failures, download.returncode == 0, "009.2 SFTP download sync should exit 0")
        record(failures, download.stderr == "", "009.2 SFTP download should keep stderr empty")
        record(failures, "sync complete" in download.stdout.splitlines(), "009.2 SFTP download should report completion")

        record(failures, read_bytes(roundtrip / "root.txt") == b"root file\n", "009.2, 009.25-009.34 SFTP should stream file bytes back through SSH/SFTP")
        record(failures, read_bytes(roundtrip / "nested" / "child.bin") == b"0123456789", "009.5-009.13 SFTP list_dir should expose immediate regular-file children")
        record(failures, read_bytes(roundtrip / "nested" / "deeper" / "leaf.txt") == b"leaf\n", "009.31 SFTP open_write should create missing parent directories")
        record(failures, (roundtrip / "empty_dir").is_dir(), "009.39-009.40 SFTP create_dir should create missing directories")
        record(failures, not (roundtrip / "sftp-source").exists(), "009.3-009.4 SFTP operations should stay relative to the remote peer root")

        for rel, expected in (
            ("root.txt", mtimes["root.txt"]),
            ("nested/child.bin", mtimes["child.bin"]),
            ("nested/deeper/leaf.txt", mtimes["leaf.txt"]),
        ):
            dst = roundtrip / rel
            record(failures, dst.exists() and close_enough_mtime(dst.stat().st_mtime, expected), f"009.17, 009.42 SFTP mod_time should round-trip for {rel}")

        rows = snapshot_rows(roundtrip)
        record(failures, row_for(rows, "root.txt") is not None, "009.48 file and SFTP peers should produce the same snapshot outcome for copied files")
        record(failures, row_for(rows, "empty_dir") is not None and row_for(rows, "empty_dir").get("byte_size") == -1, "009.10-009.13 SFTP should report directory entries with byte size -1")


def main() -> int:
    failures: list[str] = []
    record(failures, KITCHENSYNC_EXE.exists(), "released kitchensync.exe should exist")
    record(failures, bundled_uv().exists(), "bundled uv executable should exist")

    # not reasonably testable: 009.14, 009.23. The testing guidelines forbid
    # creating symlinks or relying on symlink-specific behavior.
    # not reasonably testable: 009.15, 009.24. Portable special-file setup would
    # require platform-specific device, FIFO, or socket creation outside the
    # specified happy-path CLI surface.
    # not reasonably testable: 009.43. The sync specification records directory
    # mod_time but does not expose a user operation that requires setting a
    # directory's mod_time through the CLI.
    # not reasonably testable: 009.45, 009.46, 009.47. Permission-denied,
    # miscellaneous I/O, and mid-operation network failures require sabotaging
    # the host filesystem or SFTP connection rather than normal user input.

    with tempfile.TemporaryDirectory(prefix="kitchensync-009-") as temp_name:
        tmp = Path(temp_name)
        if KITCHENSYNC_EXE.exists():
            check_local_transport(failures, tmp / "local")
            shutil.rmtree(tmp / "local", ignore_errors=True)
            if bundled_uv().exists():
                check_sftp_transport(failures, tmp / "sftp")

    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        return 1
    print("PASS: 009 transport operations")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
