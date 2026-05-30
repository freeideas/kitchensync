#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end test for reqs/017_peer-roles-and-startup-state.md."""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path(r"C:\Users\human\Desktop\prjx\kitchensync")
PROJECT_DIR = Path(r"C:\Users\human\Desktop\prjx\kitchensync\proj")
RELEASED_BINARY = (
    WORKSPACE_ROOT / "released" / ("kitchensync.exe" if os.name == "nt" else "kitchensync")
)

FIRST_SYNC_MESSAGE = "First sync? Mark the authoritative peer with a leading +"
NO_CONTRIBUTING_MESSAGE = "No contributing peer reachable - cannot make sync decisions"


def _snapshot_db(peer: Path) -> Path:
    return peer / ".kitchensync" / "snapshot.db"


def _run_kitchensync(
    root: Path,
    peers: list[str],
    *,
    timeout_seconds: float = 20.0,
) -> subprocess.CompletedProcess[str] | None:
    command = [str(RELEASED_BINARY), *peers]
    try:
        return subprocess.run(
            command,
            cwd=str(root),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return None
    except FileNotFoundError:
        return None


def _run_and_check(
    failures: list[str],
    req_id: str,
    root: Path,
    peers: list[str],
    *,
    expected_exit: int,
    timeout_seconds: float = 20.0,
) -> subprocess.CompletedProcess[str] | None:
    result = _run_kitchensync(root, peers, timeout_seconds=timeout_seconds)
    if result is None:
        failures.append(f"{req_id}: command timed out for peers={peers!r}")
        return None

    if result.returncode != expected_exit:
        failures.append(
            f"{req_id}: expected exit {expected_exit} for peers={peers!r}, got {result.returncode}; "
            f"stdout={result.stdout!r}; stderr={result.stderr!r}"
        )

    return result


def _write_text(path: Path, value: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value, encoding="utf-8")


def _assert_text(failures: list[str], req_id: str, path: Path, expected: str) -> None:
    if not path.is_file():
        failures.append(f"{req_id}: expected file at {path}")
        return

    if path.read_text(encoding="utf-8") != expected:
        failures.append(
            f"{req_id}: unexpected content at {path}; expected {expected!r}, "
            f"got {path.read_text(encoding=\"utf-8\")!r}"
        )


def _assert_not_exists(failures: list[str], req_id: str, path: Path) -> None:
    if path.exists():
        failures.append(f"{req_id}: expected path to be absent at {path}")


def _remove_snapshot(peer: Path) -> None:
    snapshot_root = peer / ".kitchensync"
    if snapshot_root.exists():
        shutil.rmtree(snapshot_root)


def _seed_sync_with_snapshot(
    failures: list[str],
    req_id: str,
    root: Path,
    peers: list[Path],
) -> bool:
    for peer in peers:
        _write_text(peer / "seed.txt", "seed")

    peer_args = [f"+{peers[0].name}", *(peer.name for peer in peers[1:])]
    result = _run_and_check(
        failures,
        req_id,
        root,
        peer_args,
        expected_exit=0,
    )
    if result is None or result.returncode != 0:
        return False

    for peer in peers:
        if not _snapshot_db(peer).is_file():
            failures.append(f"{req_id}: missing snapshot at {_snapshot_db(peer)}")
            return False

    return True


def _check_canon_with_missing_snapshot_is_contributor_and_authoritative(failures: list[str]) -> None:
    """017.1, 017.2, 017.9."""
    with tempfile.TemporaryDirectory(prefix="ks_017_") as raw_root:
        root = Path(raw_root)
        canon = root / "peer_a"
        ordinary = root / "peer_b"

        if not _seed_sync_with_snapshot(failures, "017.1/017.2/017.9", root, [canon, ordinary]):
            return

        _remove_snapshot(canon)
        _write_text(canon / "shared.txt", "from-canon")
        _write_text(ordinary / "shared.txt", "from-ordinary")

        result = _run_and_check(
            failures,
            "017.1/017.2",
            root,
            [f"+{canon.name}", ordinary.name],
            expected_exit=0,
        )
        if result is None:
            return

        _assert_text(failures, "017.1/017.2", ordinary / "shared.txt", "from-canon")
        _assert_text(failures, "017.1/017.2", canon / "shared.txt", "from-canon")

        if FIRST_SYNC_MESSAGE in result.stdout:
            failures.append(
                "017.9: first-sync suggestion was printed even with a designated canon peer"
            )


def _check_explicit_subordinate_does_not_contribute(failures: list[str]) -> None:
    """017.3, 017.4."""
    with tempfile.TemporaryDirectory(prefix="ks_017_") as raw_root:
        root = Path(raw_root)
        normal_a = root / "normal_a"
        normal_b = root / "normal_b"
        subordinate = root / "subordinate"

        if not _seed_sync_with_snapshot(
            failures,
            "017.3/017.4",
            root,
            [normal_a, normal_b, subordinate],
        ):
            return

        # normal peers supply the group decision.
        _write_text(normal_a / "shared.txt", "target-value")
        _write_text(normal_b / "shared.txt", "target-value")
        # subordinate contributes a file that should not shape decisions.
        _write_text(subordinate / "rogue.txt", "subordinate-only")

        result = _run_and_check(
            failures,
            "017.3/017.4",
            root,
            [normal_a.name, normal_b.name, f"-{subordinate.name}"],
            expected_exit=0,
        )
        if result is None:
            return

        _assert_text(failures, "017.4", normal_a / "shared.txt", "target-value")
        _assert_text(failures, "017.4", normal_b / "shared.txt", "target-value")
        _assert_text(failures, "017.4", subordinate / "shared.txt", "target-value")
        _assert_not_exists(failures, "017.3", normal_a / "rogue.txt")
        _assert_not_exists(failures, "017.3", normal_b / "rogue.txt")
        _assert_not_exists(failures, "017.3", subordinate / "rogue.txt")


def _check_auto_subordinate_snapshotless_noncanon(failures: list[str]) -> None:
    """017.5."""
    with tempfile.TemporaryDirectory(prefix="ks_017_") as raw_root:
        root = Path(raw_root)
        canon = root / "canon"
        auto_subordinate = root / "auto_subordinate"

        if not _seed_sync_with_snapshot(
            failures,
            "017.5",
            root,
            [canon, auto_subordinate],
        ):
            return

        _remove_snapshot(auto_subordinate)
        _write_text(canon / "group.txt", "authoritative")
        _write_text(auto_subordinate / "rogue.txt", "local-only")

        result = _run_and_check(
            failures,
            "017.5",
            root,
            [canon.name, auto_subordinate.name],
            expected_exit=0,
        )
        if result is None:
            return

        _assert_text(failures, "017.5", canon / "group.txt", "authoritative")
        _assert_text(failures, "017.5", auto_subordinate / "group.txt", "authoritative")
        _assert_not_exists(failures, "017.5", auto_subordinate / "rogue.txt")


def _check_snapshotless_subordinate_becomes_contributor_next_run(failures: list[str]) -> None:
    """017.6."""
    with tempfile.TemporaryDirectory(prefix="ks_017_") as raw_root:
        root = Path(raw_root)
        canon = root / "canon"
        peer = root / "former_subordinate"

        if not _seed_sync_with_snapshot(
            failures,
            "017.6",
            root,
            [canon, peer],
        ):
            return

        # Make the peer subordinate explicitly; its snapshot is still downloaded and updated.
        if _run_and_check(
            failures,
            "017.6",
            root,
            [f"+{canon.name}", f"-{peer.name}"],
            expected_exit=0,
        ) is None:
            return

        _write_text(peer / "promoted.txt", "later-contributor")

        result = _run_and_check(
            failures,
            "017.6",
            root,
            [canon.name, peer.name],
            expected_exit=0,
        )
        if result is None:
            return

        _assert_text(failures, "017.6", canon / "promoted.txt", "later-contributor")


def _check_first_sync_fails_without_canon(failures: list[str]) -> None:
    """017.7, 017.8."""
    with tempfile.TemporaryDirectory(prefix="ks_017_") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        result = _run_and_check(
            failures,
            "017.7/017.8",
            root,
            [peer_a.name, peer_b.name],
            expected_exit=1,
        )
        if result is None:
            return

        if FIRST_SYNC_MESSAGE not in result.stdout:
            failures.append(
                "017.7: expected first-sync suggestion message when no canon and no snapshots"
            )


def _check_no_contributing_peer_after_roles(failures: list[str]) -> None:
    """017.10, 017.11."""
    with tempfile.TemporaryDirectory(prefix="ks_017_") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"

        if not _seed_sync_with_snapshot(
            failures,
            "017.10/017.11",
            root,
            [peer_a, peer_b],
        ):
            return

        result = _run_and_check(
            failures,
            "017.10/017.11",
            root,
            [f"-{peer_a.name}", f"-{peer_b.name}"],
            expected_exit=1,
        )
        if result is None:
            return

        if NO_CONTRIBUTING_MESSAGE not in result.stdout:
            failures.append(
                "017.10: expected no-contributing-peer message when all reachable peers are subordinate"
            )


def main() -> int:
    failures: list[str] = []

    if not WORKSPACE_ROOT.is_dir():
        failures.append(f"precondition: workspace missing at {WORKSPACE_ROOT}")
    if not PROJECT_DIR.is_dir():
        failures.append(f"precondition: project directory missing at {PROJECT_DIR}")
    if not RELEASED_BINARY.is_file():
        failures.append(f"precondition: released executable missing at {RELEASED_BINARY}")

    if failures:
        print("FAIL: test_017_peer_roles_and_startup_state.py (precondition)")
        for failure in failures:
            print(f"  - {failure}")
        return 1

    # 017.1, 017.2: explicit canon remains contributing without local snapshot.
    # 017.9: no first-sync suggestion when canon is designated.
    _check_canon_with_missing_snapshot_is_contributor_and_authoritative(failures)

    # 017.3: explicit subordinate peer does not contribute live data/snapshot history.
    # 017.4: subordinate peer still receives the group outcome.
    _check_explicit_subordinate_does_not_contribute(failures)

    # 017.5: snapshotless non-canon peers are auto-subordinate but still receive updates.
    _check_auto_subordinate_snapshotless_noncanon(failures)

    # 017.6: peer subordinate in earlier run contributes after it has snapshot history.
    _check_snapshotless_subordinate_becomes_contributor_next_run(failures)

    # 017.7: first sync guidance message when no canonical/autority is possible.
    # 017.8: exit non-zero for that condition.
    _check_first_sync_fails_without_canon(failures)

    # 017.10: no contributing peer reachable after auto/subordinate roles applied.
    # 017.11: exit non-zero for that condition.
    _check_no_contributing_peer_after_roles(failures)

    if failures:
        print("FAIL: test_017_peer_roles_and_startup_state.py")
        for failure in failures:
            print(f"  - {failure}")
        return 1

    print("PASS: test_017_peer_roles_and_startup_state.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
