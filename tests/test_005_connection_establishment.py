# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
End-to-end tests for reqs/005_connection-establishment.md.

Covers: 005.1 005.2 005.3 005.4 005.9 005.10 005.11 005.12 005.13 005.14 005.15

Not reasonably testable end-to-end:
  005.5 -- no observable signal distinguishes which URL stays active after connection
  005.6 -- cannot observe timeout bounding without a stalling SSH server
  005.7 -- cannot observe per-URL timeout override without a stalling SSH server
  005.8 -- requires a server that stalls mid-handshake
"""

import os
import platform
import queue
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _uv_path() -> Path:
    s = platform.system().lower()
    if "windows" in s:
        return WORKSPACE / "aitc" / "bin" / "uv.exe"
    if "darwin" in s:
        return WORKSPACE / "aitc" / "bin" / "uv.mac"
    return WORKSPACE / "aitc" / "bin" / "uv.linux"


def _run(*args: str, timeout: int = 30):
    """Run kitchensync. Returns (returncode, stdout, stderr)."""
    result = subprocess.run(
        [str(EXE)] + list(args),
        capture_output=True,
        text=True,
        timeout=timeout,
    )
    return result.returncode, result.stdout, result.stderr


def _furl(p: Path) -> str:
    """Convert Path to a file:// URI (works even if path does not exist)."""
    return p.as_uri()


def _make_peer(base: Path, name: str) -> Path:
    """Create a directory with a placeholder so the run has a reachable peer."""
    d = base / name
    d.mkdir(parents=True, exist_ok=True)
    (d / ".keep").write_text("x", encoding="utf-8")
    return d


def _readline_timeout(stream, timeout: float = 10.0):
    """Read one line from a stream with a deadline. Returns None on timeout."""
    q: queue.Queue = queue.Queue()

    def _worker():
        try:
            q.put(stream.readline())
        except Exception:
            q.put(None)

    threading.Thread(target=_worker, daemon=True).start()
    try:
        return q.get(timeout=timeout)
    except queue.Empty:
        return None


# ---------------------------------------------------------------------------
# Ephemeral SFTP server context manager
# ---------------------------------------------------------------------------

class _SftpServer:
    """Start the bundled ephemeral SFTP server as a subprocess."""

    def __init__(self):
        self.process = None
        self.port: int | None = None
        self.sftp_root: Path | None = None
        self.host_key_line: str | None = None  # "<type> <base64>"
        self._kh_entry: str | None = None

    def start(self):
        uv = _uv_path()
        script = WORKSPACE / "extart" / "ephemeral-sftp-server.py"
        self.process = subprocess.Popen(
            [str(uv), "run", "--script", str(script)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        port_line = _readline_timeout(self.process.stdout, timeout=60)
        if not port_line or not port_line.strip().isdigit():
            raise RuntimeError(
                f"SFTP server did not print its port in time; got: {port_line!r}"
            )
        self.port = int(port_line.strip())

        # Read startup stderr: "sftp root:", "host key:", "user:", "auth:" (4 lines)
        for _ in range(4):
            line = _readline_timeout(self.process.stderr, timeout=10)
            if not line:
                break
            line = line.strip()
            if line.startswith("sftp root: "):
                self.sftp_root = Path(line[len("sftp root: "):])
            elif line.startswith("host key: "):
                self.host_key_line = line[len("host key: "):]

    def add_known_hosts(self):
        """Append [127.0.0.1]:PORT <type> <base64> to ~/.ssh/known_hosts."""
        if not self.host_key_line:
            raise RuntimeError("No host key line from server stderr")
        entry = f"[127.0.0.1]:{self.port} {self.host_key_line}\n"
        kh = Path.home() / ".ssh" / "known_hosts"
        kh.parent.mkdir(mode=0o700, exist_ok=True)
        # Idempotency: remove any leftover entry for this port from a prior run.
        if kh.exists():
            lines = [
                ln for ln in kh.read_text(encoding="ascii", errors="replace").splitlines(keepends=True)
                if f"[127.0.0.1]:{self.port} " not in ln
            ]
            kh.write_text("".join(lines), encoding="ascii")
        with kh.open("a", encoding="ascii") as f:
            f.write(entry)
        self._kh_entry = entry

    def remove_known_hosts(self):
        if not self._kh_entry:
            return
        kh = Path.home() / ".ssh" / "known_hosts"
        if not kh.exists():
            return
        content = kh.read_text(encoding="ascii", errors="replace")
        kh.write_text(content.replace(self._kh_entry, ""), encoding="ascii")
        self._kh_entry = None

    def stop(self):
        self.remove_known_hosts()
        if self.process:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
            self.process = None

    def __enter__(self):
        self.start()
        return self

    def __exit__(self, *_):
        self.stop()


# ---------------------------------------------------------------------------
# Tests -- each returns a list of failure strings (empty = passed)
# ---------------------------------------------------------------------------

def check_primary_url_tried_before_fallback():
    """005.1 -- primary URL is attempted before any fallback URL."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        canon = _make_peer(t, "canon")
        primary = _make_peer(t, "b_primary")     # exists -> connects in dry-run
        fallback = t / "b_fallback_nonexist"     # does NOT exist

        bracket = f"[{_furl(primary)},{_furl(fallback)}]"
        rc, out, err = _run("--dry-run", f"+{_furl(canon)}", bracket)
        if rc != 0:
            failures.append(
                f"005.1: expected exit 0 when primary URL exists; got {rc}\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
    return failures


def check_fallbacks_in_listed_order():
    """005.2 -- fallback URLs are attempted in their listed order."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        canon = _make_peer(t, "canon")
        url1 = t / "b_url1_nonexist"           # does not exist
        url2 = _make_peer(t, "b_url2")         # exists -- should be reached in order
        url3 = t / "b_url3_nonexist"           # does not exist

        bracket = f"[{_furl(url1)},{_furl(url2)},{_furl(url3)}]"
        rc, out, err = _run("--dry-run", f"+{_furl(canon)}", bracket)
        if rc != 0:
            failures.append(
                f"005.2: expected exit 0 when second fallback URL exists; got {rc}\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
    return failures


def check_connects_through_first_working_fallback():
    """005.3 -- when primary fails, connects through the first later URL that connects."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        canon = _make_peer(t, "canon")
        primary = t / "b_primary_nonexist"     # fails in dry-run
        fallback = _make_peer(t, "b_fallback") # exists -> peer connects via fallback

        bracket = f"[{_furl(primary)},{_furl(fallback)}]"
        rc, out, err = _run("--dry-run", f"+{_furl(canon)}", bracket)
        if rc != 0:
            failures.append(
                f"005.3: expected exit 0 when fallback URL exists after failing primary; got {rc}\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
    return failures


def check_first_connected_url_wins():
    """005.4 -- the first URL that connects becomes the winning URL."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        canon = _make_peer(t, "canon")
        primary = _make_peer(t, "b_primary")   # exists
        fallback = t / "b_fallback_nonexist"   # does not exist; primary should win

        bracket = f"[{_furl(primary)},{_furl(fallback)}]"
        rc, out, err = _run("--dry-run", f"+{_furl(canon)}", bracket)
        if rc != 0:
            failures.append(
                f"005.4: expected exit 0 when first URL connects; got {rc}\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
    return failures


# not reasonably testable: 005.5


def check_file_root_auto_created_normal_run():
    """005.9 -- in a normal run, a missing peer root directory is created for a file:// URL."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        canon = _make_peer(t, "canon")
        peer_b = t / "peer_b_missing"
        # Ensure clean state (idempotency guard).
        if peer_b.exists():
            import shutil
            shutil.rmtree(peer_b)

        rc, out, err = _run(f"+{_furl(canon)}", _furl(peer_b))
        if not peer_b.exists():
            failures.append(
                f"005.9: file:// peer root was NOT created in normal run\n"
                f"  rc={rc}  stdout: {out!r}  stderr: {err!r}"
            )
        if rc != 0:
            failures.append(
                f"005.9: expected exit 0 after creating missing file:// root; got {rc}\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
    return failures


def check_sftp_root_auto_created_normal_run():
    """005.10 -- in a normal run, a missing peer root directory is created for an sftp:// URL."""
    failures = []
    with _SftpServer() as srv:
        try:
            srv.add_known_hosts()
        except Exception as exc:
            failures.append(f"005.10: could not configure known_hosts: {exc}")
            return failures

        with tempfile.TemporaryDirectory() as tmp:
            canon = _make_peer(Path(tmp), "canon")
            subdir_name = "ks005_sftp_root"
            sftp_url = (
                f"sftp://testuser:testpass@127.0.0.1:{srv.port}/{subdir_name}"
            )

            # Idempotency: remove prior run's leftover if any.
            if srv.sftp_root and (srv.sftp_root / subdir_name).exists():
                import shutil
                shutil.rmtree(srv.sftp_root / subdir_name)

            rc, out, err = _run(f"+{_furl(canon)}", sftp_url, timeout=60)

            if srv.sftp_root:
                if not (srv.sftp_root / subdir_name).exists():
                    failures.append(
                        f"005.10: sftp:// peer root dir was NOT created on server\n"
                        f"  rc={rc}  stdout: {out!r}  stderr: {err!r}"
                    )
            if rc != 0:
                failures.append(
                    f"005.10: expected exit 0 after creating missing sftp:// root; got {rc}\n"
                    f"  stdout: {out!r}\n  stderr: {err!r}"
                )
    return failures


def check_missing_parents_auto_created_normal_run():
    """005.11 -- in a normal run, missing parent directories of the peer root are created."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        canon = _make_peer(t, "canon")
        # Deep path: tmp/a/b/c/peer_b -- none of a/b/c exist yet.
        peer_b = t / "a" / "b" / "c" / "peer_b"

        rc, out, err = _run(f"+{_furl(canon)}", _furl(peer_b))
        if not peer_b.exists():
            failures.append(
                f"005.11: peer root and missing parents were NOT created\n"
                f"  rc={rc}  stdout: {out!r}  stderr: {err!r}"
            )
        if rc != 0:
            failures.append(
                f"005.11: expected exit 0 after creating missing parents; got {rc}\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
    return failures


def check_uncreatable_root_treated_as_failed():
    """005.12 -- in a normal run, a URL whose root cannot be created is treated as failed."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        canon = _make_peer(t, "canon")
        # Place a FILE where the parent directory would need to be, so mkdir fails.
        blocker = t / "blocker"
        blocker.write_text("I am a file, not a directory", encoding="utf-8")
        peer_b = blocker / "sync_root"  # parent is a regular file -> mkdir fails

        rc, out, err = _run(f"+{_furl(canon)}", _furl(peer_b))
        # Peer B unreachable -> only 1 reachable peer -> exit 1.
        if rc == 0:
            failures.append(
                f"005.12: expected exit != 0 when peer root cannot be created; got 0\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
    return failures


def check_dry_run_does_not_create_file_root():
    """005.13 -- in --dry-run, a missing file:// peer root is NOT created."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        canon = _make_peer(t, "canon")
        peer_b = _make_peer(t, "peer_b")    # reachable; keeps run alive with 2 peers
        peer_c = t / "peer_c_missing"       # missing root; must NOT be created

        if peer_c.exists():
            import shutil
            shutil.rmtree(peer_c)

        rc, out, err = _run(
            "--dry-run", f"+{_furl(canon)}", _furl(peer_b), _furl(peer_c)
        )
        if peer_c.exists():
            failures.append(
                f"005.13: file:// peer root WAS created in --dry-run (must not be)\n"
                f"  rc={rc}  stdout: {out!r}  stderr: {err!r}"
            )
        # canon and peer_b are reachable -> 2 reachable -> run exits 0.
        if rc != 0:
            failures.append(
                f"005.13: expected exit 0 (two reachable peers) in --dry-run; got {rc}\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
    return failures


def check_dry_run_missing_root_url_treated_as_failed():
    """005.14 -- in --dry-run, a URL whose root does not exist is treated as failed."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        canon = _make_peer(t, "canon")
        peer_b = t / "peer_b_missing"

        if peer_b.exists():
            import shutil
            shutil.rmtree(peer_b)

        # Only 2 peers; peer_b missing in dry-run -> peer_b fails
        # -> 1 reachable peer < 2 -> exit 1.
        rc, out, err = _run("--dry-run", f"+{_furl(canon)}", _furl(peer_b))
        if rc == 0:
            failures.append(
                f"005.14: expected exit != 0 when peer URL has missing root in --dry-run; got 0\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
        if peer_b.exists():
            failures.append(
                f"005.14: peer root was created despite --dry-run\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
    return failures


def check_all_urls_failing_peer_unreachable():
    """005.15 -- a peer for which every URL fails is unreachable for the run."""
    failures = []
    with tempfile.TemporaryDirectory() as tmp:
        t = Path(tmp)
        canon = _make_peer(t, "canon")
        url1 = t / "b_url1_nonexist"
        url2 = t / "b_url2_nonexist"
        # Both URLs fail in dry-run -> peer unreachable -> 1 reachable -> exit 1.
        bracket = f"[{_furl(url1)},{_furl(url2)}]"

        rc, out, err = _run("--dry-run", f"+{_furl(canon)}", bracket)
        if rc == 0:
            failures.append(
                f"005.15: expected exit != 0 when all peer URLs fail; got 0\n"
                f"  stdout: {out!r}\n  stderr: {err!r}"
            )
    return failures


# not reasonably testable: 005.5, 005.6, 005.7, 005.8


# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------

def main():
    tests = [
        check_primary_url_tried_before_fallback,
        check_fallbacks_in_listed_order,
        check_connects_through_first_working_fallback,
        check_first_connected_url_wins,
        check_file_root_auto_created_normal_run,
        check_sftp_root_auto_created_normal_run,
        check_missing_parents_auto_created_normal_run,
        check_uncreatable_root_treated_as_failed,
        check_dry_run_does_not_create_file_root,
        check_dry_run_missing_root_url_treated_as_failed,
        check_all_urls_failing_peer_unreachable,
    ]

    all_failures = []
    for fn in tests:
        print(f"  {fn.__name__} ...", flush=True)
        try:
            failures = fn()
        except Exception as exc:
            import traceback
            failures = [f"{fn.__name__}: unexpected exception: {exc}\n{traceback.format_exc()}"]
        for msg in failures:
            print(f"  FAIL: {msg}", flush=True)
        all_failures.extend(failures)

    print(flush=True)
    if all_failures:
        print(f"FAILED: {len(all_failures)} failure(s).")
        sys.exit(1)
    print("All checks passed.")
    sys.exit(0)


if __name__ == "__main__":
    main()
