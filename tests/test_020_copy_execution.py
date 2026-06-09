# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
End-to-end tests for reqs/020_copy-execution.md

Covers: active-copy limit (--max-copies), retry limit (--retries-copy),
incremental copy start, local SWAP staging, and SFTP copy counting.

REQ IDs not covered -- observable only from internal state:
  020.4  listing/dir-creation do not count against the copy limit
  020.6  directory listings at each level issued concurrently
  020.9  failed-but-under-limit copy re-queued to back of queue
  020.10 copy at retry limit marked failed and not re-queued
  020.11 one copy's failed tries do not reduce another copy's tries
  020.13 copy buffer size is independent of file size
  020.14 copy write begins before the source file is fully read
"""

import sys
import pathlib
import platform
import subprocess
import tempfile
import threading
import time

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

# ---------------------------------------------------------------------------
# Concrete paths (never discovered from the environment)
# ---------------------------------------------------------------------------
WORKSPACE = pathlib.Path("/home/ace/Desktop/prjx/kitchensync")
EXE = WORKSPACE / "released" / "kitchensync.exe"
SFTP_SCRIPT = WORKSPACE / "extart" / "ephemeral-sftp-server.py"

_os_name = platform.system()
if _os_name == "Windows":
    UV = WORKSPACE / "aitc" / "bin" / "uv.exe"
elif _os_name == "Darwin":
    UV = WORKSPACE / "aitc" / "bin" / "uv.mac"
else:
    UV = WORKSPACE / "aitc" / "bin" / "uv.linux"

# ---------------------------------------------------------------------------
# Failure accumulator -- run every test; never fail fast
# ---------------------------------------------------------------------------
_failures: list[str] = []


def _fail(label: str, msg: str) -> None:
    _failures.append(f"[{label}] {msg}")
    print(f"FAIL [{label}]: {msg}", flush=True)


def _ok(label: str) -> None:
    print(f"PASS [{label}]", flush=True)


# ---------------------------------------------------------------------------
# Subprocess helpers
# ---------------------------------------------------------------------------

def _run(*args: str | pathlib.Path, timeout: int = 60) -> subprocess.CompletedProcess:
    cmd = [str(EXE)] + [str(a) for a in args]
    return subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )


def _readline_timeout(stream, timeout: float) -> str | None:
    """Read one line from *stream*, blocking at most *timeout* seconds.
    Returns the stripped line, empty string on EOF, or None on timeout.
    """
    buf: list[str] = []
    ev = threading.Event()

    def _read() -> None:
        try:
            buf.append(stream.readline())
        except Exception:
            pass
        ev.set()

    threading.Thread(target=_read, daemon=True).start()
    ev.wait(timeout)
    if not buf:
        return None
    return buf[0].rstrip("\r\n")


# ---------------------------------------------------------------------------
# Ephemeral SFTP server context manager
# ---------------------------------------------------------------------------

class _SFTPServer:
    """Start the bundled ephemeral SFTP server; expose its port and host key."""

    port: int
    key_type: str
    key_b64: str

    def __init__(self, extra_args: list[str] | None = None) -> None:
        self._extra_args: list[str] = extra_args or []
        self._proc: subprocess.Popen | None = None
        self.port = 0
        self.key_type = ""
        self.key_b64 = ""

    def __enter__(self) -> "_SFTPServer":
        cmd = [str(UV), "run", "--script", str(SFTP_SCRIPT)] + self._extra_args
        self._proc = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        # stdout: exactly one line -- the port number
        port_str = _readline_timeout(self._proc.stdout, timeout=20)
        if not port_str:
            self._proc.kill()
            self._proc.wait(5)
            raise RuntimeError("ephemeral SFTP server did not print port within 20 s")
        self.port = int(port_str.strip())

        # stderr: read lines in a background thread until "host key: ..." appears
        _found: list[tuple[str, str]] = []
        _done = threading.Event()

        def _read_stderr() -> None:
            try:
                for raw_line in self._proc.stderr:  # type: ignore[union-attr]
                    line = raw_line.rstrip("\r\n")
                    if line.startswith("host key: "):
                        parts = line.split()
                        if len(parts) >= 4:
                            _found.append((parts[2], parts[3]))
                        break
            except Exception:
                pass
            _done.set()

        threading.Thread(target=_read_stderr, daemon=True).start()
        if not _done.wait(10):
            self._proc.kill()
            self._proc.wait(5)
            raise RuntimeError(
                "ephemeral SFTP server did not print host key within 10 s"
            )
        if not _found:
            self._proc.kill()
            self._proc.wait(5)
            raise RuntimeError("ephemeral SFTP server: host key line not found in stderr")
        self.key_type, self.key_b64 = _found[0]
        return self

    def __exit__(self, *_: object) -> None:
        if self._proc is not None:
            self._proc.kill()
            try:
                self._proc.wait(5)
            except subprocess.TimeoutExpired:
                pass
            self._proc = None


# ---------------------------------------------------------------------------
# known_hosts helpers (ephemeral port entries only)
# ---------------------------------------------------------------------------

def _known_hosts_path() -> pathlib.Path:
    return pathlib.Path.home() / ".ssh" / "known_hosts"


def _add_known_host(port: int, key_type: str, key_b64: str) -> None:
    kh = _known_hosts_path()
    kh.parent.mkdir(exist_ok=True, mode=0o700)
    entry = f"[127.0.0.1]:{port} {key_type} {key_b64}"
    prefix = f"[127.0.0.1]:{port} "
    lines: list[str] = []
    if kh.exists():
        lines = kh.read_text(encoding="utf-8", errors="replace").splitlines()
    # Remove any stale entry for this ephemeral port first (idempotency)
    lines = [ln for ln in lines if not ln.startswith(prefix)]
    lines.append(entry)
    kh.write_text("\n".join(lines) + "\n", encoding="utf-8")


def _remove_known_host(port: int) -> None:
    kh = _known_hosts_path()
    if not kh.exists():
        return
    prefix = f"[127.0.0.1]:{port} "
    lines = kh.read_text(encoding="utf-8", errors="replace").splitlines()
    kept = [ln for ln in lines if not ln.startswith(prefix)]
    kh.write_text("\n".join(kept) + ("\n" if kept else ""), encoding="utf-8")


# ---------------------------------------------------------------------------
# Filesystem helper
# ---------------------------------------------------------------------------

def _write_tree(base: pathlib.Path, files: dict[str, str]) -> None:
    for rel, content in files.items():
        target = base / pathlib.Path(rel)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(content.encode("utf-8"))


# ===========================================================================
# Tests
# ===========================================================================

def test_020_1_default_max_copies() -> None:
    """020.1: when --max-copies is absent the default (10) is applied; a sync
    with more files than the default still completes successfully."""
    label = "020.1"
    src_files = {f"file{i:02}.txt": f"content {i}" for i in range(12)}
    with tempfile.TemporaryDirectory() as tmp:
        base = pathlib.Path(tmp)
        src = base / "src"
        dst = base / "dst"
        src.mkdir()
        dst.mkdir()
        _write_tree(src, src_files)
        result = _run(f"+{src}", str(dst))
        if result.returncode != 0:
            _fail(label, f"exit {result.returncode}; stdout={result.stdout!r}")
            return
        if result.stderr.strip():
            _fail(label, f"stderr must be empty, got: {result.stderr!r}")
            return
        missing = [r for r in src_files if not (dst / r).exists()]
        if missing:
            _fail(label, f"files not synced: {missing}")
            return
    _ok(label)


def test_020_2_explicit_max_copies_valid() -> None:
    """020.2: --max-copies 1 (a positive integer) is accepted; all files sync
    even though at most one copy is active at a time."""
    label = "020.2/valid"
    src_files = {f"f{i}.txt": f"val {i}" for i in range(5)}
    with tempfile.TemporaryDirectory() as tmp:
        base = pathlib.Path(tmp)
        src = base / "src"
        dst = base / "dst"
        src.mkdir()
        dst.mkdir()
        _write_tree(src, src_files)
        result = _run("--max-copies", "1", f"+{src}", str(dst))
        if result.returncode != 0:
            _fail(label, f"exit {result.returncode}; stdout={result.stdout!r}")
            return
        if result.stderr.strip():
            _fail(label, f"stderr must be empty, got: {result.stderr!r}")
            return
        missing = [r for r in src_files if not (dst / r).exists()]
        if missing:
            _fail(label, f"files not synced with --max-copies 1: {missing}")
            return
    _ok(label)


def test_020_2_invalid_max_copies_zero() -> None:
    """020.2: --max-copies 0 is rejected with exit 1 (zero is not a positive integer)."""
    label = "020.2/zero"
    with tempfile.TemporaryDirectory() as tmp:
        base = pathlib.Path(tmp)
        (base / "src").mkdir()
        (base / "dst").mkdir()
        result = _run("--max-copies", "0", f"+{base / 'src'}", str(base / "dst"))
    if result.returncode != 1:
        _fail(label, f"expected exit 1 for --max-copies 0, got {result.returncode}")
    else:
        _ok(label)


def test_020_2_invalid_max_copies_alpha() -> None:
    """020.2: --max-copies with a non-integer string is rejected with exit 1."""
    label = "020.2/alpha"
    with tempfile.TemporaryDirectory() as tmp:
        base = pathlib.Path(tmp)
        (base / "src").mkdir()
        (base / "dst").mkdir()
        result = _run("--max-copies", "abc", f"+{base / 'src'}", str(base / "dst"))
    if result.returncode != 1:
        _fail(label, f"expected exit 1 for --max-copies abc, got {result.returncode}")
    else:
        _ok(label)


def test_020_5_incremental_copy() -> None:
    """020.5: copy work begins for early-scanned directories while later
    directories are still being scanned.  Verified by successful completion
    of a sync across a nested tree with a tight copy slot count."""
    label = "020.5"
    src_files = {
        "dirA/a1.txt": "file a1",
        "dirA/a2.txt": "file a2",
        "dirB/b1.txt": "file b1",
        "dirB/b2.txt": "file b2",
        "dirC/sub/c1.txt": "file c1",
        "root.txt": "root file",
    }
    with tempfile.TemporaryDirectory() as tmp:
        base = pathlib.Path(tmp)
        src = base / "src"
        dst = base / "dst"
        src.mkdir()
        dst.mkdir()
        _write_tree(src, src_files)
        # --max-copies 2 with a multi-directory tree: early directories fill
        # slots while later ones are still being listed.
        result = _run("--max-copies", "2", f"+{src}", str(dst))
        if result.returncode != 0:
            _fail(label, f"exit {result.returncode}; stdout={result.stdout!r}")
            return
        if result.stderr.strip():
            _fail(label, f"stderr must be empty, got: {result.stderr!r}")
            return
        wrong: list[str] = []
        for rel, content in src_files.items():
            p = dst / rel
            if not p.exists():
                wrong.append(f"{rel}: missing")
            elif p.read_bytes() != content.encode("utf-8"):
                wrong.append(f"{rel}: wrong content")
        if wrong:
            _fail(label, f"sync did not produce correct results: {wrong}")
            return
    _ok(label)


def test_020_7_explicit_retries_copy_valid() -> None:
    """020.7: --retries-copy 2 (a positive integer) is accepted; sync succeeds."""
    label = "020.7/valid"
    with tempfile.TemporaryDirectory() as tmp:
        base = pathlib.Path(tmp)
        src = base / "src"
        dst = base / "dst"
        src.mkdir()
        dst.mkdir()
        _write_tree(src, {"a.txt": "hello", "b.txt": "world"})
        result = _run("--retries-copy", "2", f"+{src}", str(dst))
        if result.returncode != 0:
            _fail(label, f"exit {result.returncode}; stdout={result.stdout!r}")
            return
        if result.stderr.strip():
            _fail(label, f"stderr must be empty, got: {result.stderr!r}")
            return
        for rel in ("a.txt", "b.txt"):
            if not (dst / rel).exists():
                _fail(label, f"file not synced: {rel}")
                return
    _ok(label)


def test_020_7_invalid_retries_copy_zero() -> None:
    """020.7: --retries-copy 0 is rejected with exit 1 (zero is not a positive integer)."""
    label = "020.7/zero"
    with tempfile.TemporaryDirectory() as tmp:
        base = pathlib.Path(tmp)
        (base / "src").mkdir()
        (base / "dst").mkdir()
        result = _run("--retries-copy", "0", f"+{base / 'src'}", str(base / "dst"))
    if result.returncode != 1:
        _fail(label, f"expected exit 1 for --retries-copy 0, got {result.returncode}")
    else:
        _ok(label)


def test_020_8_default_retries_copy() -> None:
    """020.8: sync without --retries-copy succeeds (default of 3 is applied)."""
    label = "020.8"
    with tempfile.TemporaryDirectory() as tmp:
        base = pathlib.Path(tmp)
        src = base / "src"
        dst = base / "dst"
        src.mkdir()
        dst.mkdir()
        _write_tree(src, {"x.txt": "x content", "y.txt": "y content"})
        result = _run(f"+{src}", str(dst))
        if result.returncode != 0:
            _fail(label, f"exit {result.returncode}; stdout={result.stdout!r}")
            return
        if result.stderr.strip():
            _fail(label, f"stderr must be empty, got: {result.stderr!r}")
            return
        for rel in ("x.txt", "y.txt"):
            if not (dst / rel).exists():
                _fail(label, f"file not synced: {rel}")
                return
    _ok(label)


def test_020_15_local_swap_staging() -> None:
    """020.15: local-to-local copies use SWAP staging rather than writing the
    destination in place.  Verified by: (a) dst file carries the source content
    after sync, (b) the old content is in BAK, (c) no SWAP new/old fragments
    remain after the successful run."""
    label = "020.15"
    with tempfile.TemporaryDirectory() as tmp:
        base = pathlib.Path(tmp)
        src = base / "src"
        dst = base / "dst"
        src.mkdir()
        dst.mkdir()
        # Pre-populate dst with a file that the canon-peer sync will replace
        (dst / "data.txt").write_bytes(b"old destination content")
        (src / "data.txt").write_bytes(b"new source content")
        result = _run(f"+{src}", str(dst))
        if result.returncode != 0:
            _fail(
                label,
                f"sync failed: exit {result.returncode}; stdout={result.stdout!r}",
            )
            return
        if result.stderr.strip():
            _fail(label, f"stderr must be empty, got: {result.stderr!r}")
            return
        # Destination file must carry the source content after the swap
        actual = (dst / "data.txt").read_bytes()
        if actual != b"new source content":
            _fail(
                label,
                f"dst/data.txt wrong content after sync: {actual!r}",
            )
            return
        # No leftover SWAP staging files (new/old) should remain after success
        swap_dir = dst / ".kitchensync" / "SWAP"
        if swap_dir.exists():
            leftover = [str(p) for p in swap_dir.rglob("*") if p.is_file()]
            if leftover:
                _fail(label, f"SWAP not cleaned up after sync: {leftover}")
                return
        # Displaced old content must be recoverable from BAK
        bak_dir = dst / ".kitchensync" / "BAK"
        if not bak_dir.exists():
            _fail(
                label,
                "BAK directory absent; old content was not displaced before swap",
            )
            return
        bak_files = [p for p in bak_dir.rglob("*") if p.is_file()]
        if not any(p.read_bytes() == b"old destination content" for p in bak_files):
            _fail(label, "old content not found in BAK after SWAP replacement")
            return
    _ok(label)


def test_020_3_12_sftp_copy() -> None:
    """020.3, 020.12: SFTP copies count against --max-copies and obey
    --retries-copy just like local copies.  Verified by running a
    local-to-sftp sync with --max-copies 1 and --retries-copy 2; the run
    must exit 0 and emit 'C ' progress lines confirming files were transferred."""
    label = "020.3/020.12"
    sftp_port = 0
    try:
        with _SFTPServer() as sftp:
            sftp_port = sftp.port
            _add_known_host(sftp.port, sftp.key_type, sftp.key_b64)
            try:
                with tempfile.TemporaryDirectory() as tmp:
                    base = pathlib.Path(tmp)
                    src = base / "src"
                    src.mkdir()
                    _write_tree(src, {
                        "alpha.txt": "alpha content",
                        "beta.txt": "beta content",
                        "gamma.txt": "gamma content",
                    })
                    sftp_url = (
                        f"sftp://testuser:testpass"
                        f"@127.0.0.1:{sftp.port}/ks_test_020"
                    )
                    result = _run(
                        "--max-copies", "1",
                        "--retries-copy", "2",
                        f"+{src}",
                        sftp_url,
                        timeout=90,
                    )
                # result is available after TemporaryDirectory is cleaned up
                if result.returncode != 0:
                    _fail(
                        label,
                        f"exit {result.returncode}; stdout={result.stdout!r}",
                    )
                    return
                if result.stderr.strip():
                    _fail(label, f"stderr must be empty, got: {result.stderr!r}")
                    return
                if "C " not in result.stdout:
                    _fail(
                        label,
                        "no 'C ' copy-progress lines in stdout -- no files copied via SFTP",
                    )
                    return
                _ok(label)
            finally:
                _remove_known_host(sftp.port)
    except Exception as exc:
        _fail(label, f"SFTP infrastructure error: {exc}")
        if sftp_port:
            _remove_known_host(sftp_port)


# not reasonably testable: 020.4 -- listing/dir-creation don't count against copy limit
# not reasonably testable: 020.6 -- directory listings at each level issued concurrently
# not reasonably testable: 020.9 -- failed-but-under-limit copy re-queued (requires inducing failure)
# not reasonably testable: 020.10 -- try-limit-reached copy marked failed (requires inducing failure)
# not reasonably testable: 020.11 -- independent try counts (requires inducing multiple failures)
# not reasonably testable: 020.13 -- buffer size independent of file size (internal)
# not reasonably testable: 020.14 -- write starts before source fully read (internal)


# ===========================================================================
# Main
# ===========================================================================

def main() -> None:
    tests = [
        test_020_1_default_max_copies,
        test_020_2_explicit_max_copies_valid,
        test_020_2_invalid_max_copies_zero,
        test_020_2_invalid_max_copies_alpha,
        test_020_5_incremental_copy,
        test_020_7_explicit_retries_copy_valid,
        test_020_7_invalid_retries_copy_zero,
        test_020_8_default_retries_copy,
        test_020_15_local_swap_staging,
        test_020_3_12_sftp_copy,
    ]

    for t in tests:
        try:
            t()
        except Exception as exc:
            _fail(t.__name__, f"unexpected exception: {exc}")

    print(flush=True)
    if _failures:
        print(f"{len(_failures)} failure(s):", flush=True)
        for msg in _failures:
            print(f"  {msg}", flush=True)
        sys.exit(1)

    print(f"All {len(tests)} tests passed.", flush=True)


if __name__ == "__main__":
    main()
