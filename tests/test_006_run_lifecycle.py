# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
import sys

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

import subprocess
import tempfile
from pathlib import Path

WORKSPACE = Path("/home/ace/Desktop/prjx/kitchensync")
EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")

_failures: list[str] = []


def _pass(msg: str) -> None:
    print(f"PASS: {msg}", flush=True)


def _fail(msg: str) -> None:
    _failures.append(msg)
    print(f"FAIL: {msg}", flush=True)


def _check(cond: bool, msg: str) -> None:
    if cond:
        _pass(msg)
    else:
        _fail(msg)


def _run(*args: str, timeout: int = 30) -> subprocess.CompletedProcess:
    try:
        return subprocess.run(
            [str(EXE)] + list(args),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        _fail(f"process timed out after {timeout}s with args: {list(args)}")
        return subprocess.CompletedProcess(
            args=[str(EXE)] + list(args), returncode=-1, stdout="", stderr=""
        )


# not reasonably testable: 006.1 - whether connection attempts proceed concurrently rather
# than sequentially is an internal implementation detail; it is not observable via exit code,
# stdout, stderr, or filesystem state produced by the released executable.


def check_006_2_fewer_than_two_reachable() -> None:
    """006.2: when fewer than two peers are reachable, KitchenSync exits 1."""
    with tempfile.TemporaryDirectory() as td:
        peer_a = Path(td) / "peerA"
        peer_a.mkdir()
        # peerB intentionally not created; in --dry-run a missing root is unreachable.
        peer_b = Path(td) / "peerB"
        result = _run("--dry-run", str(peer_a), str(peer_b), timeout=10)
        _check(result.returncode == 1, "006.2: fewer than two reachable peers exits 1")


def check_006_3_canon_unreachable() -> None:
    """006.3: when the canon (+) peer is unreachable, KitchenSync exits 1."""
    with tempfile.TemporaryDirectory() as td:
        peer_a = Path(td) / "peerA"
        peer_a.mkdir()
        peer_b = Path(td) / "peerB"
        peer_b.mkdir()
        # Canon root not created; in --dry-run a missing root is unreachable.
        # Two other peers are reachable, so fewer-than-two (006.2) does not fire first.
        canon = Path(td) / "canon_missing"
        result = _run(
            "--dry-run",
            "+" + str(canon),
            str(peer_a),
            str(peer_b),
            timeout=10,
        )
        _check(result.returncode == 1, "006.3: unreachable canon peer exits 1")


def check_006_4_5_no_snapshots_no_canon() -> None:
    """006.4 + 006.5: no snapshot data and no canon -> message + exit 1."""
    with tempfile.TemporaryDirectory() as td:
        peer_a = Path(td) / "peerA"
        peer_a.mkdir()
        peer_b = Path(td) / "peerB"
        peer_b.mkdir()
        # Neither peer has a snapshot.db and no + prefix is given.
        result = _run(str(peer_a), str(peer_b), timeout=10)
        _check(
            "First sync? Mark the authoritative peer with a leading +" in result.stdout,
            "006.4: prints first-sync suggestion when no snapshots and no canon",
        )
        _check(
            result.returncode == 1,
            "006.5: exits 1 when no snapshot data and no canon peer",
        )


def check_006_6_7_no_contributing_peer() -> None:
    """006.6 + 006.7: no contributing peer after auto-subordination -> message + exit 1."""
    with tempfile.TemporaryDirectory() as td:
        peer_a = Path(td) / "peerA"
        peer_a.mkdir()
        peer_b = Path(td) / "peerB"
        peer_b.mkdir()

        # Phase 1: create snapshot.db on both peers so the no-snapshot check (step 6)
        # does not fire in phase 2.  An empty canon run on two empty directories
        # creates the snapshot infrastructure and exits 0.
        prep = _run("+" + str(peer_a), str(peer_b), timeout=20)
        if prep.returncode != 0:
            _fail(
                "006.6/006.7 setup: prep sync failed "
                f"(exit {prep.returncode}); skipping phase-2 assertions"
            )
            return

        # Phase 2: all peers explicitly subordinate (-); both have snapshot.db so
        # the no-snapshot exit does not fire, but no contributing peer remains.
        result = _run("-" + str(peer_a), "-" + str(peer_b), timeout=10)
        _check(
            "No contributing peer reachable - cannot make sync decisions"
            in result.stdout,
            "006.6: prints no-contributing-peer message when all peers are subordinate",
        )
        _check(
            result.returncode == 1,
            "006.7: exits 1 when no contributing peer is reachable",
        )


# not reasonably testable: 006.8 - whether copy work for an already-scanned directory begins
# while traversal continues into later directories is an internal concurrency detail.  No
# observable surface (exit code, stdout, stderr, or filesystem state) can distinguish a true
# overlapped pipeline from a serialized scan-then-copy implementation.


def check_006_9_10_11_normal_run() -> None:
    """006.9 + 006.10 + 006.11: normal run copies all files, writes snapshots back, exits 0."""
    with tempfile.TemporaryDirectory() as td:
        peer_a = Path(td) / "peerA"
        peer_a.mkdir()
        peer_b = Path(td) / "peerB"
        peer_b.mkdir()
        (peer_a / "alpha.txt").write_text("alpha content", encoding="utf-8")
        (peer_a / "beta.txt").write_text("beta content", encoding="utf-8")
        result = _run("+" + str(peer_a), str(peer_b), timeout=30)
        _check(result.returncode == 0, "006.11: normal run exits 0")
        _check(
            (peer_b / "alpha.txt").exists(),
            "006.9: alpha.txt copy completed before run exits",
        )
        _check(
            (peer_b / "beta.txt").exists(),
            "006.9: beta.txt copy completed before run exits",
        )
        _check(
            (peer_a / ".kitchensync" / "snapshot.db").exists(),
            "006.10: updated snapshot written back to peerA",
        )
        _check(
            (peer_b / ".kitchensync" / "snapshot.db").exists(),
            "006.10: updated snapshot written back to peerB",
        )


def check_006_12_unreachable_peer_excluded() -> None:
    """006.12: unreachable peer excluded from the run; two reachable peers sync normally."""
    # not reasonably testable: 006.13 - the unreachable SFTP peer's filesystem is
    # inaccessible from the test (its port is closed), so we cannot observe whether its
    # snapshot rows were left unmodified.  The test for 006.12 implicitly relies on this
    # property: if kitchensync had tried to write the snapshot it would have failed and
    # the run behaviour would differ from what 006.12 asserts.
    with tempfile.TemporaryDirectory() as td:
        peer_a = Path(td) / "peerA"
        peer_a.mkdir()
        peer_b = Path(td) / "peerB"
        peer_b.mkdir()
        (peer_a / "data.txt").write_text("payload", encoding="utf-8")

        # sftp://127.0.0.1:1/sync uses port 1, which is virtually never listening;
        # the TCP connection is refused immediately.  --timeout-conn 2 caps any wait.
        result = _run(
            "+" + str(peer_a),
            str(peer_b),
            "sftp://127.0.0.1:1/sync",
            "--timeout-conn",
            "2",
            timeout=20,
        )
        _check(
            result.returncode == 0,
            "006.12: run exits 0 with two reachable peers despite one unreachable SFTP peer",
        )
        _check(
            (peer_b / "data.txt").exists(),
            "006.12: data.txt synced to reachable peerB; unreachable peer did not block the run",
        )


def main() -> None:
    if not EXE.exists():
        print(f"ERROR: released executable not found: {EXE}", flush=True)
        sys.exit(1)

    check_006_2_fewer_than_two_reachable()
    check_006_3_canon_unreachable()
    check_006_4_5_no_snapshots_no_canon()
    check_006_6_7_no_contributing_peer()
    check_006_9_10_11_normal_run()
    check_006_12_unreachable_peer_excluded()

    if _failures:
        print(f"\n{len(_failures)} failure(s):", flush=True)
        for f in _failures:
            print(f"  - {f}", flush=True)
        sys.exit(1)

    print("\nAll checks passed.", flush=True)
    sys.exit(0)


if __name__ == "__main__":
    main()
