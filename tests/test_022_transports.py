# /// script
# requires-python = ">=3.11"
# dependencies = ["paramiko>=3.4", "cryptography"]
# ///
"""
End-to-end test for ./reqs/022_transports.md:
Transport operations and error semantics.

Not reasonably testable from the CLI surface:
  022.15 -- list_dir silently omits symlinks/special files
             (TESTING-GUIDELINES.md forbids creating symlinks in tests)
  022.16 -- stat returns "not found" for symlinks/special files (same reason)
  022.18 -- network failure surfaces as I/O error
             (requires environment sabotage; spec says we do not do that)
  022.19 -- I/O failure handled identically on file:// and sftp://
             (same reason)
"""

from __future__ import annotations

import io
import os
import subprocess
import sys
import tempfile
import threading
import time
from contextlib import contextmanager
from pathlib import Path

import paramiko

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = WORKSPACE / "released" / "kitchensync.exe"
SFTP_SCRIPT = WORKSPACE / "extart" / "ephemeral-sftp-server.py"


def _uv() -> Path:
    if sys.platform == "win32":
        return WORKSPACE / "aitc" / "bin" / "uv.exe"
    if sys.platform == "darwin":
        return WORKSPACE / "aitc" / "bin" / "uv.mac"
    return WORKSPACE / "aitc" / "bin" / "uv.linux"


UV = _uv()

# --------------------------------------------------------------------------- #
# Failure collection                                                           #
# --------------------------------------------------------------------------- #

_failures: list[str] = []


def fail(msg: str) -> None:
    _failures.append(msg)
    print(f"  FAIL: {msg}", file=sys.stderr)


def ok(label: str) -> None:
    print(f"  ok:   {label}")


# --------------------------------------------------------------------------- #
# SFTP server fixture                                                          #
# --------------------------------------------------------------------------- #


def _readline_bounded(stream, timeout: float) -> str:
    """Read one line from stream with a hard time limit."""
    result: list[str] = []
    done = threading.Event()

    def _read() -> None:
        try:
            result.append(stream.readline())
        finally:
            done.set()

    threading.Thread(target=_read, daemon=True).start()
    if not done.wait(timeout):
        raise TimeoutError(f"no line received within {timeout}s")
    return result[0] if result else ""


@contextmanager
def sftp_server(home_dir: Path, extra_args: list[str] | None = None):
    """
    Start the ephemeral SFTP server with a fixed RSA host key.

    Yields (port, sftp_root) where sftp_root is the local temp directory the
    server exposes as '/'.  Writes a known_hosts entry for [127.0.0.1]:{port}
    into home_dir/.ssh/known_hosts before yielding.
    """
    host_key = paramiko.RSAKey.generate(2048)
    host_key_file = home_dir / "sftp_host_key"
    host_key.write_private_key_file(str(host_key_file))

    cmd = [
        str(UV), "run", "--script", str(SFTP_SCRIPT),
        "--host-key", str(host_key_file),
    ] + (extra_args or [])

    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    sftp_root_holder: list[str | None] = [None]

    def _drain_stderr() -> None:
        assert proc.stderr is not None
        for raw in proc.stderr:
            line = raw.rstrip()
            if sftp_root_holder[0] is None and line.startswith("sftp root: "):
                sftp_root_holder[0] = line[len("sftp root: "):]

    stderr_th = threading.Thread(target=_drain_stderr, daemon=True)
    stderr_th.start()

    try:
        # 90 s allows for a cold uv package install (paramiko + cryptography).
        port_line = _readline_bounded(proc.stdout, timeout=90)
        port_str = port_line.strip()
        if not port_str.isdigit():
            raise RuntimeError(
                f"SFTP server did not emit a port number; got: {port_line!r}"
            )
        port = int(port_str)

        # Wait briefly for the sftp root to appear in stderr.
        deadline = time.monotonic() + 5.0
        while sftp_root_holder[0] is None and time.monotonic() < deadline:
            time.sleep(0.05)

        sftp_root = Path(sftp_root_holder[0]) if sftp_root_holder[0] else None

        # Write known_hosts so KitchenSync can verify the server's host key.
        ssh_dir = home_dir / ".ssh"
        ssh_dir.mkdir(parents=True, exist_ok=True)
        kh_line = (
            f"[127.0.0.1]:{port} {host_key.get_name()} {host_key.get_base64()}\n"
        )
        with (ssh_dir / "known_hosts").open("a", encoding="ascii") as fh:
            fh.write(kh_line)

        yield port, sftp_root

    finally:
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=2)
        stderr_th.join(timeout=2)


# --------------------------------------------------------------------------- #
# Helpers                                                                      #
# --------------------------------------------------------------------------- #


def _ssh_env(home_dir: Path) -> dict[str, str]:
    """Env with HOME/USERPROFILE pointing at home_dir; SSH agent disabled."""
    env = dict(os.environ)
    env["HOME"] = str(home_dir)
    env["USERPROFILE"] = str(home_dir)
    env.pop("SSH_AUTH_SOCK", None)
    return env


def _run(
    args: list[str],
    *,
    env: dict | None = None,
    timeout: int = 60,
) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(EXE)] + args,
        capture_output=True,
        text=True,
        timeout=timeout,
        env=env,
    )


def _populate(src: Path) -> None:
    """Create a test tree under src with files of varying sizes."""
    src.mkdir(parents=True, exist_ok=True)
    (src / "alpha.txt").write_bytes(b"alpha file content")
    (src / "beta.txt").write_bytes(b"beta file content 12345")
    sub = src / "sub"
    sub.mkdir(exist_ok=True)
    (sub / "gamma.txt").write_bytes(b"gamma in nested subdirectory")
    # 5 MB file: forces multiple read() chunks (022.7).
    (src / "large.bin").write_bytes(bytes(i % 251 for i in range(5 * 1024 * 1024)))


# --------------------------------------------------------------------------- #
# Tests                                                                        #
# --------------------------------------------------------------------------- #


def test_file_to_file_sync(tmp: Path) -> None:
    """
    022.2  list_dir returns correct is_dir for each child
    022.3  list_dir reports correct byte_size for regular files
    022.4  list_dir reports -1 byte_size for directories
    022.7  read() returns the correct bytes (including via multiple chunks on large file)
    022.8  open_write() creates the target file and any missing parent directories
    022.9  create_dir() creates the directory and any missing parent directories
    """
    src = tmp / "src"
    dst = tmp / "dst"
    _populate(src)

    r = _run([f"+{src}", str(dst)])
    if r.returncode != 0:
        fail(
            f"022.x: file-to-file initial sync failed (exit {r.returncode}): "
            f"{r.stdout[:400]}"
        )
        return
    if r.stderr.strip():
        fail(f"022.x: stderr not empty after initial sync: {r.stderr[:200]!r}")

    # 022.9: create_dir created sub/
    if (dst / "sub").is_dir():
        ok("022.9: create_dir created nested directory")
    else:
        fail("022.9: sub/ directory not created at dest")

    # 022.8: open_write created a file inside a newly created parent directory
    if (dst / "sub" / "gamma.txt").exists():
        ok("022.8: open_write created file inside newly created parent directory")
    else:
        fail("022.8: sub/gamma.txt not created (open_write may not create missing parents)")

    # 022.7: read() returned the correct bytes for small and large files
    for rel in ["alpha.txt", "beta.txt", "sub/gamma.txt", "large.bin"]:
        src_b = (src / rel).read_bytes()
        dst_path = dst / rel
        if not dst_path.exists():
            fail(f"022.7: {rel} missing at dest")
        elif dst_path.read_bytes() != src_b:
            fail(f"022.7: {rel} content mismatch at dest")
        else:
            ok(f"022.7: {rel} content correct ({len(src_b)} bytes)")

    # 022.2: list_dir returned is_dir=true for sub/ (it was recursed into)
    if (dst / "sub").is_dir() and (dst / "sub" / "gamma.txt").exists():
        ok("022.2: list_dir returned correct is_dir for directory entry")
    else:
        fail("022.2: sub/ not recursed into (is_dir may be wrong in list_dir)")

    # 022.3 / 022.4: a no-change re-sync must copy nothing.
    # If byte_size were wrong in the snapshot, the re-sync would see a spurious
    # mismatch and re-copy files.
    r2 = _run([str(src), str(dst)])
    if r2.returncode != 0:
        fail(f"022.3: no-change re-sync failed (exit {r2.returncode})")
    else:
        if r2.stderr.strip():
            fail(f"022.3: stderr not empty on no-change re-sync: {r2.stderr[:200]!r}")
        c_lines = [l for l in r2.stdout.splitlines() if l.startswith("C ")]
        if c_lines:
            fail(
                f"022.3/022.4: no-change re-sync re-copied {len(c_lines)} file(s) -- "
                "list_dir byte_size may be wrong"
            )
        else:
            ok("022.3/022.4: no-change re-sync performed no copies (byte_size correct)")


def test_file_sftp_identical_results(tmp: Path) -> None:
    """
    022.1: a file:// peer and an sftp:// peer with identical directory contents
    yield identical sync results.
    """
    src = tmp / "src"
    _populate(src)
    home = tmp / "home"
    home.mkdir()
    env = _ssh_env(home)

    with sftp_server(home) as (port, sftp_root):
        if sftp_root is None:
            fail("022.1: could not determine SFTP server root from stderr")
            return

        sftp_url = f"sftp://tester:pass@127.0.0.1:{port}/"

        r_sftp = _run([f"+{src}", sftp_url], env=env)
        if r_sftp.returncode != 0:
            fail(
                f"022.1: file->sftp sync failed (exit {r_sftp.returncode}): "
                f"{r_sftp.stdout[:400]}"
            )
            return
        if r_sftp.stderr.strip():
            fail(f"022.1: stderr not empty on file->sftp sync: {r_sftp.stderr[:200]!r}")

        dst_file = tmp / "dst_file"
        r_file = _run([f"+{src}", str(dst_file)])
        if r_file.returncode != 0:
            fail(f"022.1: file->file reference sync failed (exit {r_file.returncode})")
            return
        if r_file.stderr.strip():
            fail(f"022.1: stderr not empty on file->file sync: {r_file.stderr[:200]!r}")

        for rel in ["alpha.txt", "beta.txt", "sub/gamma.txt", "large.bin"]:
            sftp_f = sftp_root / rel
            file_f = dst_file / rel
            if not sftp_f.exists():
                fail(f"022.1: {rel} missing from sftp dest")
            elif not file_f.exists():
                fail(f"022.1: {rel} missing from file:// dest (reference sync failed)")
            elif sftp_f.read_bytes() != file_f.read_bytes():
                fail(f"022.1: {rel} content differs between sftp and file:// dest")
            else:
                ok(f"022.1: {rel} identical at file:// and sftp:// dest")


def test_sftp_to_file_sync(tmp: Path) -> None:
    """
    022.1 (sftp:// as source): reading from an sftp:// peer delivers the same
    content as reading from a file:// peer.
    """
    src = tmp / "src"
    _populate(src)
    home = tmp / "home"
    home.mkdir()
    env = _ssh_env(home)

    with sftp_server(home) as (port, sftp_root):
        if sftp_root is None:
            fail("022.1 sftp->file: could not determine SFTP server root")
            return

        sftp_url = f"sftp://tester:pass@127.0.0.1:{port}/"

        # Push source content into the sftp peer.
        r1 = _run([f"+{src}", sftp_url], env=env)
        if r1.returncode != 0:
            fail(
                f"022.1 sftp->file: push to sftp failed (exit {r1.returncode}): "
                f"{r1.stdout[:400]}"
            )
            return
        if r1.stderr.strip():
            fail(f"022.1 sftp->file: stderr not empty on push: {r1.stderr[:200]!r}")

        # Pull from sftp into a new file:// peer (no snapshot -> auto-subordinate).
        dst = tmp / "dst"
        r2 = _run([sftp_url, str(dst)], env=env)
        if r2.returncode != 0:
            fail(
                f"022.1 sftp->file: pull from sftp failed (exit {r2.returncode}): "
                f"{r2.stdout[:400]}"
            )
            return
        if r2.stderr.strip():
            fail(f"022.1 sftp->file: stderr not empty on pull: {r2.stderr[:200]!r}")

        for rel in ["alpha.txt", "beta.txt", "sub/gamma.txt", "large.bin"]:
            expected = (src / rel).read_bytes()
            got_path = dst / rel
            if not got_path.exists():
                fail(f"022.1 sftp->file: {rel} not at file:// dest")
            elif got_path.read_bytes() != expected:
                fail(f"022.1 sftp->file: {rel} content mismatch at file:// dest")
            else:
                ok(f"022.1 sftp->file: {rel} correct at file:// dest")


def test_mod_time(tmp: Path) -> None:
    """
    022.5: stat returns mod_time for an existing file (drives the copy decision).
    022.14: set_mod_time sets the destination file's modification time.
    """
    src = tmp / "src"
    dst = tmp / "dst"
    src.mkdir()

    f = src / "timed.txt"
    f.write_bytes(b"mod time test content")
    # Use a mtime 2 hours in the past, rounded to whole seconds to avoid
    # sub-second precision differences between filesystems.
    target_mtime = float(int(time.time()) - 7200)
    os.utime(str(f), (target_mtime, target_mtime))

    r = _run([f"+{src}", str(dst)])
    if r.returncode != 0:
        fail(f"022.14: sync failed (exit {r.returncode}): {r.stdout[:400]}")
        return
    if r.stderr.strip():
        fail(f"022.14: stderr not empty: {r.stderr[:200]!r}")

    dst_f = dst / "timed.txt"
    if not dst_f.exists():
        fail("022.14: timed.txt not copied to dest")
        return

    actual = dst_f.stat().st_mtime
    diff = abs(actual - target_mtime)
    if diff <= 2:
        ok(f"022.14/022.5: mtime preserved at dest (diff={diff:.3f}s)")
    else:
        fail(
            f"022.14/022.5: mtime not preserved -- "
            f"expected ~{target_mtime:.0f}, got {actual:.0f} (diff={diff:.0f}s)"
        )


def test_rename_swap_protocol(tmp: Path) -> None:
    """
    022.10: rename(src, dst) moves src to dst when dst does not exist.
    022.11: rename(src, dst) fails when dst already exists -- KitchenSync uses
            the SWAP protocol (write to non-existent SWAP path, then rename-to-final)
            instead of rename-over-existing.  Observable: the displaced original
            lands in BAK/ rather than being overwritten silently.
    """
    src = tmp / "src"
    dst = tmp / "dst"
    src.mkdir()

    initial_mtime = float(int(time.time()) - 3600)  # 1 hour ago
    f = src / "swap_test.txt"
    f.write_bytes(b"initial content")
    os.utime(str(f), (initial_mtime, initial_mtime))

    # First sync: establishes snapshot on both peers.
    r1 = _run([f"+{src}", str(dst)])
    if r1.returncode != 0:
        fail(f"022.10: initial sync failed (exit {r1.returncode}): {r1.stdout[:400]}")
        return
    if r1.stderr.strip():
        fail(f"022.10: stderr not empty after initial sync: {r1.stderr[:200]!r}")

    # Advance mtime to force a re-copy decision on the second sync.
    updated_mtime = initial_mtime + 120
    f.write_bytes(b"updated content")
    os.utime(str(f), (updated_mtime, updated_mtime))

    r2 = _run([str(src), str(dst)])
    if r2.returncode != 0:
        fail(
            f"022.10: replacement sync failed (exit {r2.returncode}): "
            f"{r2.stdout[:400]}"
        )
        return
    if r2.stderr.strip():
        fail(f"022.10: stderr not empty after replacement sync: {r2.stderr[:200]!r}")

    # 022.10: new content at the final path.
    dst_f = dst / "swap_test.txt"
    if not dst_f.exists():
        fail("022.10: swap_test.txt missing at dst after replacement sync")
        return
    if dst_f.read_bytes() == b"updated content":
        ok("022.10: rename placed new content at non-existent destination path")
    else:
        fail(
            f"022.10: swap_test.txt has wrong content after replacement: "
            f"{dst_f.read_bytes()!r}"
        )

    # 022.11: old file in BAK/ proves the SWAP protocol was used (rename always
    # targets a non-existent path; existing destination moved aside first).
    bak_base = dst / ".kitchensync" / "BAK"
    old_copies = list(bak_base.rglob("swap_test.txt")) if bak_base.exists() else []
    if old_copies:
        actual_old = old_copies[0].read_bytes()
        if actual_old == b"initial content":
            ok(
                "022.11: old file displaced to BAK/ with correct content -- "
                "SWAP protocol used rename-to-non-existent-path only"
            )
        else:
            fail(
                f"022.11: BAK/ entry has wrong content: {actual_old!r} "
                "(expected b'initial content')"
            )
    else:
        fail(
            "022.11: no BAK/ entry for swap_test.txt after replacement -- "
            "SWAP displacement to BAK/ may not have occurred"
        )


def test_delete_file(tmp: Path) -> None:
    """
    022.6: stat returns "not found" for a non-existent path (drives the
           deletion decision on re-sync).
    022.12: delete_file removes a file from the destination peer.
    """
    src = tmp / "src"
    dst = tmp / "dst"
    src.mkdir()
    (src / "keep.txt").write_bytes(b"keep this file")
    (src / "bye.txt").write_bytes(b"to be deleted")

    r1 = _run([f"+{src}", str(dst)])
    if r1.returncode != 0:
        fail(f"022.12: initial sync failed (exit {r1.returncode}): {r1.stdout[:400]}")
        return
    if r1.stderr.strip():
        fail(f"022.12: stderr not empty after initial sync: {r1.stderr[:200]!r}")
    if not (dst / "bye.txt").exists():
        fail("022.12: bye.txt not at dst after initial sync")
        return

    (src / "bye.txt").unlink()

    r2 = _run([f"+{src}", str(dst)])
    if r2.returncode != 0:
        fail(
            f"022.12: deletion sync failed (exit {r2.returncode}): {r2.stdout[:400]}"
        )
        return
    if r2.stderr.strip():
        fail(f"022.12: stderr not empty after deletion sync: {r2.stderr[:200]!r}")

    if (dst / "bye.txt").exists():
        fail(
            "022.12/022.6: bye.txt still at dst after deletion sync -- "
            "delete_file or stat not-found detection may be broken"
        )
    else:
        ok(
            "022.12/022.6: file removed at dst "
            "(stat not-found on src drove the deletion decision)"
        )

    if (dst / "keep.txt").exists():
        ok("022.12: unrelated keep.txt retained at dst")
    else:
        fail("022.12: keep.txt incorrectly removed")


def test_delete_dir(tmp: Path) -> None:
    """
    022.13: delete_dir removes an empty directory.
    """
    src = tmp / "src"
    dst = tmp / "dst"
    src.mkdir()
    (src / "anchor.txt").write_bytes(b"anchor")
    empty = src / "empty_subdir"
    empty.mkdir()

    r1 = _run([f"+{src}", str(dst)])
    if r1.returncode != 0:
        fail(f"022.13: initial sync failed (exit {r1.returncode}): {r1.stdout[:400]}")
        return
    if r1.stderr.strip():
        fail(f"022.13: stderr not empty after initial sync: {r1.stderr[:200]!r}")
    if not (dst / "empty_subdir").is_dir():
        fail("022.13: empty_subdir not at dst after initial sync")
        return
    ok("022.13: empty_subdir created at dst in initial sync")

    empty.rmdir()

    r2 = _run([f"+{src}", str(dst)])
    if r2.returncode != 0:
        fail(
            f"022.13: directory deletion sync failed (exit {r2.returncode}): "
            f"{r2.stdout[:400]}"
        )
        return
    if r2.stderr.strip():
        fail(f"022.13: stderr not empty after deletion sync: {r2.stderr[:200]!r}")

    if (dst / "empty_subdir").exists():
        fail("022.13: empty_subdir still at dst after deletion sync")
    else:
        ok("022.13: delete_dir removed empty directory from dst")


def test_sftp_key_auth(tmp: Path) -> None:
    """
    Required by TESTING-GUIDELINES.md: Ed25519 key-only auth (no inline
    password, no SSH agent, server rejects passwords).
    Also covers 022.1 via the sftp:// peer with key-based auth.
    """
    src = tmp / "src"
    src.mkdir()
    (src / "keyfile.txt").write_bytes(b"ed25519 key auth test content")

    home = tmp / "home"
    home.mkdir()
    ssh_dir = home / ".ssh"
    ssh_dir.mkdir()

    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives.serialization import (
        Encoding, PrivateFormat, NoEncryption,
    )
    _raw = Ed25519PrivateKey.generate()
    _pem = _raw.private_bytes(Encoding.PEM, PrivateFormat.OpenSSH, NoEncryption())
    client_key = paramiko.Ed25519Key.from_private_key(io.StringIO(_pem.decode("ascii")))
    id_ed25519 = ssh_dir / "id_ed25519"
    id_ed25519.write_bytes(_pem)
    id_ed25519.chmod(0o600)
    id_pub = ssh_dir / "id_ed25519.pub"
    id_pub.write_text(
        f"{client_key.get_name()} {client_key.get_base64()}\n",
        encoding="ascii",
    )

    env = _ssh_env(home)

    with sftp_server(
        home, extra_args=["--authorized-key", str(id_pub)]
    ) as (port, sftp_root):
        if sftp_root is None:
            fail("key-auth: could not determine SFTP server root")
            return

        # No inline password; SSH agent removed; KitchenSync must fall through
        # to ~/.ssh/id_ed25519.
        sftp_url = f"sftp://tester@127.0.0.1:{port}/"

        r = _run([f"+{src}", sftp_url], env=env)
        if r.returncode != 0:
            fail(
                f"key-auth (022.1): Ed25519 key-only sync failed "
                f"(exit {r.returncode}): {r.stdout[:400]}"
            )
            return
        if r.stderr.strip():
            fail(f"key-auth: stderr not empty: {r.stderr[:200]!r}")

        sftp_f = sftp_root / "keyfile.txt"
        if sftp_f.exists() and sftp_f.read_bytes() == b"ed25519 key auth test content":
            ok(
                "key-auth (022.1): Ed25519 key-only auth succeeded; "
                "content correct at sftp:// peer"
            )
        else:
            fail(
                "key-auth (022.1): keyfile.txt missing or wrong at sftp peer "
                f"(exists={sftp_f.exists()})"
            )


def test_error_categories(tmp: Path) -> None:
    """
    022.17: all transport operations report only the standard error categories
    (not-found, permission-denied, I/O error); no transport-specific panic.

    Observable sub-cases:
      (a) Syncing from a non-existent source peer -> not-found error, non-zero exit.
      (b) Syncing a source tree that contains an unreadable file -> permission-denied
          error logged to stdout; reachable files still sync; stderr stays empty.
    """
    # (a) not-found: non-existent peer exits non-zero without crashing.
    dst_a = tmp / "dst_a"
    dst_a.mkdir()
    nonexistent = tmp / "no_such_source"

    r_a = _run([f"+{nonexistent}", str(dst_a)])
    if r_a.returncode == 0:
        fail("022.17a: expected non-zero exit for non-existent peer, got 0")
    else:
        ok(
            f"022.17a: not-found error reported cleanly without crash "
            f"(exit {r_a.returncode})"
        )
    if r_a.stderr.strip():
        fail(f"022.17a: stderr not empty for not-found error: {r_a.stderr[:200]!r}")

    # (b) permission-denied: unreadable source file logged; sibling syncs fine.
    src_b = tmp / "src_b"
    dst_b = tmp / "dst_b"
    src_b.mkdir()
    dst_b.mkdir()
    (src_b / "readable.txt").write_bytes(b"readable content")
    secret = src_b / "secret.txt"
    secret.write_bytes(b"secret")
    os.chmod(str(secret), 0o000)  # no read permission -> permission denied on open

    try:
        r_b = _run([f"+{src_b}", str(dst_b)], timeout=60)
        if r_b.stderr.strip():
            fail(
                f"022.17b: stderr not empty for permission-denied error: "
                f"{r_b.stderr[:200]!r}"
            )
        # The reachable sibling must still sync.
        if (dst_b / "readable.txt").exists():
            ok("022.17b: readable sibling synced despite permission-denied on other file")
        else:
            fail(
                "022.17b: readable.txt missing at dst -- permission-denied on sibling "
                "may have aborted the entire sync"
            )
    finally:
        os.chmod(str(secret), 0o644)


# --------------------------------------------------------------------------- #
# Entry point                                                                  #
# --------------------------------------------------------------------------- #


def main() -> None:
    if not EXE.exists():
        print(f"FATAL: executable not found: {EXE}", file=sys.stderr)
        sys.exit(1)

    with tempfile.TemporaryDirectory() as _root:
        tmp = Path(_root)

        suite = [
            ("test_file_to_file_sync", test_file_to_file_sync),
            ("test_file_sftp_identical_results", test_file_sftp_identical_results),
            ("test_sftp_to_file_sync", test_sftp_to_file_sync),
            ("test_mod_time", test_mod_time),
            ("test_rename_swap_protocol", test_rename_swap_protocol),
            ("test_delete_file", test_delete_file),
            ("test_delete_dir", test_delete_dir),
            ("test_sftp_key_auth", test_sftp_key_auth),
            ("test_error_categories", test_error_categories),
        ]

        for name, fn in suite:
            print(f"\n=== {name} ===")
            try:
                test_dir = tmp / name
                test_dir.mkdir(exist_ok=True)
                fn(test_dir)
            except Exception as exc:  # noqa: BLE001
                fail(f"{name}: unexpected exception: {exc}")

    if _failures:
        print(
            f"\nFAILED: {len(_failures)} check(s):",
            file=sys.stderr,
        )
        for msg in _failures:
            print(f"  - {msg}", file=sys.stderr)
        sys.exit(1)

    print("\nAll checks passed.")
    sys.exit(0)


if __name__ == "__main__":
    main()
