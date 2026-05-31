#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

"""End-to-end test for reqs/008_decision_making.md."""

from __future__ import annotations

import os
import subprocess
import sys
import tempfile
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")

WORKSPACE_ROOT = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync")
RELEASED_BINARY = (
    WORKSPACE_ROOT / "released" / ("kitchensync.exe" if sys.platform == "win32" else "kitchensync")
)


def _write_text(path: Path, value: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(value, encoding="utf-8")


def _set_mtime(path: Path, when: float) -> None:
    os.utime(path, (when, when), follow_symlinks=True)


def _run_kitchensync(
    root: Path,
    peers: list[str],
    *,
    extra_args: list[str] | None = None,
    timeout_seconds: float = 20.0,
) -> subprocess.CompletedProcess[str] | None:
    command: list[str] = [str(RELEASED_BINARY)]
    if extra_args:
        command.extend(extra_args)
    command.extend(peers)

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
    except FileNotFoundError:
        return None
    except subprocess.TimeoutExpired:
        return None


def _run_and_check(
    failures: list[str],
    req_id: str,
    root: Path,
    peers: list[str],
    *,
    expected_exit: int = 0,
    extra_args: list[str] | None = None,
    timeout_seconds: float = 20.0,
) -> subprocess.CompletedProcess[str] | None:
    result = _run_kitchensync(
        root,
        peers,
        extra_args=extra_args,
        timeout_seconds=timeout_seconds,
    )

    if result is None:
        failures.append(f"{req_id}: command did not execute or timed out")
        return None

    if result.returncode != expected_exit:
        failures.append(
            f"{req_id}: expected exit {expected_exit}, got {result.returncode}; "
            f"command={peers}; stdout={result.stdout!r}; stderr={result.stderr!r}"
        )
        return result

    if result.stdout == "" and expected_exit == 0 and result.stderr != "":
        failures.append(f"{req_id}: command returned {expected_exit} but produced stderr output: {result.stderr!r}")

    return result


def _seed_sync(
    failures: list[str],
    req_id: str,
    root: Path,
    peers: list[Path],
    seed_name: str,
) -> subprocess.CompletedProcess[str] | None:
    for peer in peers:
        if not (peer / seed_name).exists():
            _write_text(peer / seed_name, "seed")

    peer_names = [
        f"+{peers[0].name}",
        *(p.name for p in peers[1:]),
    ]

    return _run_and_check(failures, req_id, root, peer_names)


def _assert_dir_exists(failures: list[str], req_id: str, path: Path) -> None:
    if not path.is_dir():
        failures.append(f"{req_id}: expected directory to exist at {path}")


def _assert_not_exists(failures: list[str], req_id: str, path: Path) -> None:
    if path.exists():
        failures.append(f"{req_id}: expected path to be absent at {path}")


def _assert_text(failures: list[str], req_id: str, path: Path, expected: str) -> None:
    if not path.is_file():
        failures.append(f"{req_id}: expected file at {path}")
        return

    if path.read_text(encoding="utf-8") != expected:
        failures.append(
            f"{req_id}: unexpected content at {path}; expected {expected!r}, "
            f"got {path.read_text(encoding="utf-8")!r}"
        )


def _assert_mtime_near(
    failures: list[str],
    req_id: str,
    path: Path,
    expected: float,
    *,
    tol: float = 1e-6,
) -> None:
    if not path.is_file():
        failures.append(f"{req_id}: expected file at {path}")
        return

    if abs(path.stat().st_mtime - expected) > tol:
        failures.append(
            f"{req_id}: unexpected mtime for {path}; expected {expected}, "
            f"got {path.stat().st_mtime}"
        )


def _check_unchanged_008_1_9_27(failures: list[str]) -> None:
    """008.1, 008.9, 008.27."""
    with tempfile.TemporaryDirectory(prefix="ks_008_001") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        rel = "state.bin"
        _write_text(peer_a / rel, "alpha")
        _write_text(peer_b / rel, "alpha")

        if _seed_sync(failures, "008.1", root, [peer_a, peer_b], rel) is None:
            return

        before_a = (peer_a / rel).stat().st_mtime
        before_b = (peer_b / rel).stat().st_mtime

        if _run_and_check(failures, "008.9", root, [f"+{peer_a.name}", peer_b.name]) is None:
            return

        _assert_text(failures, "008.1", peer_a / rel, "alpha")
        _assert_text(failures, "008.1", peer_b / rel, "alpha")
        _assert_mtime_near(failures, "008.9", peer_a / rel, before_a, tol=1e-6)
        _assert_mtime_near(failures, "008.9", peer_b / rel, before_b, tol=1e-6)
        _assert_mtime_near(failures, "008.27", peer_a / rel, before_a, tol=1e-6)


def _check_size_modified_008_2(failures: list[str]) -> None:
    """008.2."""
    with tempfile.TemporaryDirectory(prefix="ks_008_002") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        rel = "payload.txt"
        _write_text(peer_a / rel, "small")
        _write_text(peer_b / rel, "small")

        if _seed_sync(failures, "008.2", root, [peer_a, peer_b], rel) is None:
            return

        base_mtime = (peer_a / rel).stat().st_mtime
        _write_text(peer_a / rel, "x" * 80)
        _set_mtime(peer_a / rel, base_mtime)

        if _run_and_check(failures, "008.2", root, [f"+{peer_a.name}", peer_b.name]) is None:
            return

        _assert_text(failures, "008.2", peer_a / rel, "x" * 80)
        _assert_text(failures, "008.2", peer_b / rel, "x" * 80)


def _check_modified_selection_and_propagation_008_10_24(failures: list[str]) -> None:
    """008.10, 008.24."""
    with tempfile.TemporaryDirectory(prefix="ks_008_010") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        rel = "modified-live.txt"
        _write_text(peer_a / rel, "base")
        _write_text(peer_b / rel, "base")

        if _seed_sync(failures, "008.10", root, [peer_a, peer_b], rel) is None:
            return

        base_mtime = (peer_a / rel).stat().st_mtime
        _write_text(peer_a / rel, "winning-content")
        _write_text(peer_b / rel, "loser-content")
        _set_mtime(peer_a / rel, base_mtime + 8.0)
        _set_mtime(peer_b / rel, base_mtime + 1.0)

        if _run_and_check(failures, "008.10", root, [peer_a.name, peer_b.name]) is None:
            return

        _assert_text(failures, "008.10", peer_a / rel, "winning-content")
        _assert_text(failures, "008.24", peer_b / rel, "winning-content")


def _check_absence_only_votes_008_15(failures: list[str]) -> None:
    """008.15."""
    with tempfile.TemporaryDirectory(prefix="ks_008_015") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        rel = "absent-only.txt"
        _write_text(peer_a / rel, "persist")
        _write_text(peer_b / rel, "persist")

        if _seed_sync(failures, "008.15", root, [peer_a, peer_b], rel) is None:
            return

        (peer_a / rel).unlink()
        (peer_b / rel).unlink()

        if _run_and_check(failures, "008.15", root, [peer_a.name, peer_b.name]) is None:
            return

        _assert_not_exists(failures, "008.15", peer_a / rel)
        _assert_not_exists(failures, "008.15", peer_b / rel)


def _check_mtime_modified_008_3(failures: list[str]) -> None:
    """008.3."""
    with tempfile.TemporaryDirectory(prefix="ks_008_003") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        rel = "timed.txt"
        _write_text(peer_a / rel, "match")
        _write_text(peer_b / rel, "match")

        if _seed_sync(failures, "008.3", root, [peer_a, peer_b], rel) is None:
            return

        base_mtime = (peer_a / rel).stat().st_mtime
        # Same size as before, mod time shifted by more than 5 seconds.
        _write_text(peer_a / rel, "match")
        _set_mtime(peer_a / rel, base_mtime + 6.0)

        if _run_and_check(failures, "008.3", root, [f"+{peer_a.name}", peer_b.name]) is None:
            return

        _assert_text(failures, "008.3", peer_b / rel, "match")


def _check_new_entry_008_5_11_25(failures: list[str]) -> None:
    """008.5, 008.11, 008.25."""
    with tempfile.TemporaryDirectory(prefix="ks_008_005") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        _write_text(peer_a / "seed.txt", "seed")
        _write_text(peer_b / "seed.txt", "seed")

        if _seed_sync(failures, "008.5", root, [peer_a, peer_b], "seed.txt") is None:
            return

        rel = "new-live.txt"
        _write_text(peer_a / rel, "newly-seen")

        if _run_and_check(failures, "008.5", root, [f"+{peer_a.name}", peer_b.name]) is None:
            return

        _assert_text(failures, "008.11", peer_a / rel, "newly-seen")
        _assert_text(failures, "008.11", peer_b / rel, "newly-seen")


def _check_tie_rules_008_12_13_22(failures: list[str]) -> None:
    """008.12, 008.13, 008.22."""
    with tempfile.TemporaryDirectory(prefix="ks_008_012") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        rel = "tie.txt"
        _write_text(peer_a / rel, "base")
        _write_text(peer_b / rel, "base")

        if _seed_sync(failures, "008.12", root, [peer_a, peer_b], rel) is None:
            return

        base_mtime = (peer_a / rel).stat().st_mtime
        _write_text(peer_a / rel, "short")
        _write_text(peer_b / rel, "much-longer-content")
        _set_mtime(peer_a / rel, base_mtime + 3.0)
        _set_mtime(peer_b / rel, base_mtime + 3.0)

        if _run_and_check(failures, "008.22", root, [peer_a.name, peer_b.name]) is None:
            return

        _assert_text(failures, "008.22", peer_a / rel, "much-longer-content")
        _assert_text(failures, "008.22", peer_b / rel, "much-longer-content")

    with tempfile.TemporaryDirectory(prefix="ks_008_013") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        rel = "newest.txt"
        _write_text(peer_a / rel, "base")
        _write_text(peer_b / rel, "base")

        if _seed_sync(failures, "008.13", root, [peer_a, peer_b], rel) is None:
            return

        base_mtime = (peer_a / rel).stat().st_mtime
        _write_text(peer_a / rel, "older")
        _write_text(peer_b / rel, "newer")
        _set_mtime(peer_a / rel, base_mtime + 1.0)
        _set_mtime(peer_b / rel, base_mtime + 7.0)

        if _run_and_check(failures, "008.13", root, [peer_a.name, peer_b.name]) is None:
            return

        _assert_text(failures, "008.13", peer_a / rel, "newer")
        _assert_text(failures, "008.13", peer_b / rel, "newer")


def _check_deletion_vs_file_008_16_17_23(failures: list[str]) -> None:
    """008.16, 008.17, 008.23."""
    with tempfile.TemporaryDirectory(prefix="ks_008_016") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        rel = "delete-vs-file.txt"
        _write_text(peer_a / rel, "initial")
        _write_text(peer_b / rel, "initial")

        if _seed_sync(failures, "008.16", root, [peer_a, peer_b], rel) is None:
            return

        base_mtime = (peer_a / rel).stat().st_mtime
        (peer_b / rel).unlink()
        _set_mtime(peer_a / rel, base_mtime - 10.0)

        if _run_and_check(failures, "008.16", root, [peer_a.name, peer_b.name]) is None:
            return

        _assert_not_exists(failures, "008.16", peer_a / rel)
        _assert_not_exists(failures, "008.16", peer_b / rel)

        _write_text(peer_a / rel, "resurrected")
        _set_mtime(peer_a / rel, base_mtime + 2.0)

        if _run_and_check(failures, "008.17", root, [peer_a.name, peer_b.name]) is None:
            return

        _assert_text(failures, "008.23", peer_a / rel, "resurrected")
        _assert_text(failures, "008.23", peer_b / rel, "resurrected")


def _check_directory_and_subordinate_008_32_33_35_39(failures: list[str]) -> None:
    """008.32, 008.33, 008.35, 008.39."""
    with tempfile.TemporaryDirectory(prefix="ks_008_032") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_c = root / "peer_c"
        peer_a.mkdir()
        peer_b.mkdir()
        peer_c.mkdir()

        if _seed_sync(failures, "008.32", root, [peer_a, peer_b], "seed.txt") is None:
            return

        live_dir = "live-dir"
        (peer_a / live_dir).mkdir()

        if _run_and_check(failures, "008.32", root, [peer_a.name, peer_b.name]) is None:
            return

        _assert_dir_exists(failures, "008.32", peer_a / live_dir)
        _assert_dir_exists(failures, "008.32", peer_b / live_dir)

        dead_dir = "dead-dir"
        (peer_a / dead_dir).mkdir()
        (peer_b / dead_dir).mkdir()
        _write_text(peer_a / dead_dir / "inside.txt", "x")
        _write_text(peer_b / dead_dir / "inside.txt", "x")

        if _seed_sync(failures, "008.33", root, [peer_a, peer_b], "seed.txt") is None:
            return

        for child in (peer_a / dead_dir, peer_b / dead_dir):
            for item in child.iterdir():
                if item.is_file() or item.is_symlink():
                    item.unlink()
                elif item.is_dir():
                    item.rmdir()
            child.rmdir()

        if _run_and_check(failures, "008.33", root, [peer_a.name, peer_b.name]) is None:
            return

        _assert_not_exists(failures, "008.33", peer_a / dead_dir)
        _assert_not_exists(failures, "008.33", peer_b / dead_dir)

        sub_path = "subordinate-only"
        _write_text(peer_c / sub_path, "only-here")
        if _seed_sync(failures, "008.39", root, [peer_a, peer_b, peer_c], "seed.txt") is None:
            return

        if _run_and_check(
            failures,
            "008.39",
            root,
            [peer_a.name, peer_b.name, f"-{peer_c.name}"],
            extra_args=["--verbosity", "error"],
        ) is None:
            return

        _assert_not_exists(failures, "008.39", peer_c / sub_path)

        sub_dir = "subordinate-only-dir"
        (peer_c / sub_dir).mkdir()

        if _run_and_check(
            failures,
            "008.35",
            root,
            [peer_a.name, peer_b.name, f"-{peer_c.name}"],
            extra_args=["--verbosity", "error"],
        ) is None:
            return

        _assert_not_exists(failures, "008.35", peer_c / sub_dir)


def _check_directory_no_snapshot_peer_008_34(failures: list[str]) -> None:
    """008.34."""
    with tempfile.TemporaryDirectory(prefix="ks_008_034") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_c = root / "peer_c"
        peer_a.mkdir()
        peer_b.mkdir()
        peer_c.mkdir()

        if _seed_sync(failures, "008.34", root, [peer_a, peer_b], "seed.txt") is None:
            return

        dead_dir = "ghost-dir"
        (peer_a / dead_dir).mkdir()
        (peer_b / dead_dir).mkdir()
        _write_text(peer_a / dead_dir / "marker", "x")
        _write_text(peer_b / dead_dir / "marker", "x")

        if _run_and_check(failures, "008.34", root, [peer_a.name, peer_b.name]) is None:
            return

        (peer_a / dead_dir / "marker").unlink()
        (peer_b / dead_dir / "marker").unlink()
        peer_a_dir = peer_a / dead_dir
        peer_b_dir = peer_b / dead_dir
        peer_a_dir.rmdir()
        peer_b_dir.rmdir()

        if _run_and_check(failures, "008.34", root, [peer_a.name, peer_b.name]) is None:
            return

        _assert_not_exists(failures, "008.34", peer_a / dead_dir)
        _assert_not_exists(failures, "008.34", peer_b / dead_dir)

        if _run_and_check(
            failures,
            "008.34",
            root,
            [peer_a.name, peer_b.name, peer_c.name],
        ) is None:
            return

        _assert_not_exists(failures, "008.34", peer_c / dead_dir)


def _check_canon_rules_008_28_29_30(failures: list[str]) -> None:
    """008.28, 008.29, 008.30."""
    with tempfile.TemporaryDirectory(prefix="ks_008_028") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_a.mkdir()
        peer_b.mkdir()

        if _seed_sync(failures, "008.28", root, [peer_a, peer_b], "seed.txt") is None:
            return

        win_file = "canon-wins-file.txt"
        _write_text(peer_a / win_file, "from-canon")
        _write_text(peer_b / win_file, "from-sub")

        if _run_and_check(failures, "008.28", root, [f"+{peer_a.name}", peer_b.name]) is None:
            return

        _assert_text(failures, "008.28", peer_b / win_file, "from-canon")

        absent_file = "canon-absence.txt"
        _write_text(peer_b / absent_file, "to-delete")

        if _run_and_check(failures, "008.29", root, [f"+{peer_a.name}", peer_b.name]) is None:
            return

        _assert_not_exists(failures, "008.29", peer_a / absent_file)
        _assert_not_exists(failures, "008.29", peer_b / absent_file)

        canon_dir = "canon-directory"
        (peer_a / canon_dir).mkdir()
        if _run_and_check(failures, "008.30", root, [f"+{peer_a.name}", peer_b.name]) is None:
            return

        _assert_dir_exists(failures, "008.30", peer_a / canon_dir)
        _assert_dir_exists(failures, "008.30", peer_b / canon_dir)


def _check_conflict_rules_008_36_37_38(failures: list[str]) -> None:
    """008.36, 008.37, 008.38."""
    with tempfile.TemporaryDirectory(prefix="ks_008_036") as raw_root:
        root = Path(raw_root)
        peer_a = root / "peer_a"
        peer_b = root / "peer_b"
        peer_c = root / "peer_c"
        peer_a.mkdir()
        peer_b.mkdir()
        peer_c.mkdir()

        if _seed_sync(failures, "008.36", root, [peer_a, peer_b, peer_c], "seed.txt") is None:
            return

        canon_conflict = "type-conflict"
        _write_text(peer_a / canon_conflict, "canon-file")
        (peer_b / canon_conflict).mkdir()

        if _run_and_check(failures, "008.36", root, [f"+{peer_a.name}", peer_b.name]) is None:
            return

        _assert_text(failures, "008.36", peer_b / canon_conflict, "canon-file")

        conflict = "type-conflict-no-canon"
        _write_text(peer_a / conflict, "winner")
        (peer_b / conflict).mkdir()
        _write_text(peer_c / conflict, "loser")

        # Make peer_a clearly newest among file entries for 008.38.
        newest_time = (peer_a / conflict).stat().st_mtime + 10.0
        _set_mtime(peer_a / conflict, newest_time)
        _set_mtime(peer_c / conflict, newest_time - 10.0)

        if _run_and_check(failures, "008.38", root, [peer_a.name, peer_b.name, peer_c.name]) is None:
            return

        _assert_text(failures, "008.37", peer_a / conflict, "winner")
        _assert_text(failures, "008.37", peer_b / conflict, "winner")
        _assert_text(failures, "008.37", peer_c / conflict, "winner")


def main() -> int:
    failures: list[str] = []

    if not RELEASED_BINARY.is_file():
        failures.append(f"precondition: released executable not found at {RELEASED_BINARY}")
        for failure in failures:
            print(f"FAIL: {failure}", file=sys.stderr)
        return 1

    _check_unchanged_008_1_9_27(failures)
    _check_size_modified_008_2(failures)
    _check_modified_selection_and_propagation_008_10_24(failures)
    _check_absence_only_votes_008_15(failures)
    _check_mtime_modified_008_3(failures)
    _check_new_entry_008_5_11_25(failures)
    _check_tie_rules_008_12_13_22(failures)
    _check_deletion_vs_file_008_16_17_23(failures)
    _check_directory_and_subordinate_008_32_33_35_39(failures)
    _check_directory_no_snapshot_peer_008_34(failures)
    _check_canon_rules_008_28_29_30(failures)
    _check_conflict_rules_008_36_37_38(failures)

    # not reasonably testable: 008.4 (live with tombstone requires direct row-state introspection),
    # not reasonably testable: 008.7, 008.8 (snapshot-metadata absence classes are not directly observable),
    # not reasonably testable: 008.14 (requires selecting the newest deletion estimate across peers),
    # not reasonably testable: 008.18, 008.19, 008.20, 008.21 (absent-unconfirmed edge cases rely on last_seen timing),
    # not reasonably testable: 008.26 (all-contributing-peer no-vote state is observable only as no-op behavior),
    # not reasonably testable: 008.31 (directory mod-time arbitration is internal).

    if failures:
        print("FAIL: test_008_decision_making.py", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1

    print("PASS: test_008_decision_making.py")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
