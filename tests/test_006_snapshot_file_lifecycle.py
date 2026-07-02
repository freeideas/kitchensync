# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
KITCHENSYNC_EXE = Path("/home/ace/Desktop/prjx/kitchensync/released/kitchensync.exe")


# not reasonably testable: 006.16 local temporary {tmp}/{uuid}/snapshot.db path is not observable.
# not reasonably testable: 006.18 download errors other than not found require sabotaging transport state.
# not reasonably testable: 006.19 local temporary database reads are internal implementation detail.
# not reasonably testable: 006.20 local temporary database writes are internal implementation detail.
# not reasonably testable: 006.24 close-before-replace is not directly observable from the CLI.
# not reasonably testable: 006.29 upload failure before SWAP old requires sabotaging transport state.
# not reasonably testable: 006.30 retained SWAP new after upload failure requires sabotaging transport state.
# not reasonably testable: 006.31 upload failure after SWAP old requires sabotaging transport state.
# not reasonably testable: 006.32 transaction completion is internal except through uploaded DB usability.
# not reasonably testable: 006.33 statement finalization is internal except through uploaded DB usability.
# not reasonably testable: 006.34 cursor finalization is internal except through uploaded DB usability.
# not reasonably testable: 006.35 reader finalization is internal except through uploaded DB usability.
# not reasonably testable: 006.36 connection closing is internal except through uploaded DB usability.
# not reasonably testable: 006.37 transport upload source is internal except through uploaded DB usability.
# not reasonably testable: 006.39 true overlapping completion order is not deterministic from this script.


@dataclass
class CheckResult:
    name: str
    failures: list[str]


def record(condition: bool, failures: list[str], message: str) -> None:
    if not condition:
        failures.append(message)


def run_kitchensync(args: list[str], failures: list[str], name: str) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(
            [str(KITCHENSYNC_EXE), *args],
            cwd=str(WORKSPACE_ROOT),
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        failures.append(f"{name}: KitchenSync timed out")
        stdout = exc.stdout if isinstance(exc.stdout, str) else ""
        stderr = exc.stderr if isinstance(exc.stderr, str) else ""
        return subprocess.CompletedProcess([str(KITCHENSYNC_EXE), *args], 124, stdout, stderr)


def require_success(result: subprocess.CompletedProcess[str], failures: list[str], name: str) -> None:
    record(result.returncode == 0, failures, f"{name}: expected exit 0, got {result.returncode}; stdout={result.stdout!r}")
    record(result.stderr == "", failures, f"{name}: expected empty stderr, got {result.stderr!r}")
    record("sync complete" in result.stdout.splitlines(), failures, f"{name}: stdout did not contain sync complete: {result.stdout!r}")


def metadata_dir(peer: Path) -> Path:
    return peer / ".kitchensync"


def snapshot_path(peer: Path) -> Path:
    return metadata_dir(peer) / "snapshot.db"


def snapshot_swap_dir(peer: Path) -> Path:
    return metadata_dir(peer) / "SWAP" / "snapshot.db"


def create_snapshot_db(path: Path, marker: str = "") -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        path.unlink()
    con = sqlite3.connect(str(path))
    try:
        mode = con.execute("PRAGMA journal_mode=DELETE").fetchone()[0]
        if str(mode).lower() != "delete":
            raise AssertionError(f"sqlite refused rollback journal mode for {path}: {mode}")
        con.execute(
            """
            CREATE TABLE IF NOT EXISTS snapshot (
                id TEXT PRIMARY KEY,
                parent_id TEXT NOT NULL,
                basename TEXT NOT NULL,
                mod_time TEXT NOT NULL,
                byte_size INTEGER NOT NULL,
                last_seen TEXT,
                deleted_time TEXT
            )
            """
        )
        con.execute("CREATE INDEX IF NOT EXISTS snapshot_parent_id ON snapshot(parent_id)")
        con.execute("CREATE INDEX IF NOT EXISTS snapshot_last_seen ON snapshot(last_seen)")
        con.execute("CREATE INDEX IF NOT EXISTS snapshot_deleted_time ON snapshot(deleted_time)")
        if marker:
            con.execute("PRAGMA user_version = 1")
            con.execute(
                "INSERT OR REPLACE INTO snapshot VALUES (?, '/', ?, '2000-01-01_00-00-00_000000Z', 0, NULL, NULL)",
                (marker, marker),
            )
        con.commit()
    finally:
        con.close()


def assert_sqlite_snapshot_usable(path: Path, failures: list[str], label: str) -> None:
    record(path.exists(), failures, f"{label}: expected {path} to exist")
    if not path.exists():
        return
    try:
        con = sqlite3.connect(str(path))
        try:
            integrity = con.execute("PRAGMA integrity_check").fetchone()[0]
            journal_mode = con.execute("PRAGMA journal_mode").fetchone()[0]
            tables = {
                row[0]
                for row in con.execute("SELECT name FROM sqlite_master WHERE type = 'table'")
            }
        finally:
            con.close()
    except sqlite3.Error as exc:
        failures.append(f"{label}: snapshot is not a usable SQLite database: {exc}")
        return
    record(integrity == "ok", failures, f"{label}: expected SQLite integrity_check ok, got {integrity!r}")
    record(str(journal_mode).lower() == "delete", failures, f"{label}: expected rollback journal mode, got {journal_mode!r}")
    record("snapshot" in tables, failures, f"{label}: expected snapshot table in uploaded database, got {sorted(tables)!r}")


def assert_no_snapshot_sidecars(peer: Path, failures: list[str], label: str) -> None:
    meta = metadata_dir(peer)
    if not meta.exists():
        failures.append(f"{label}: expected metadata directory {meta} to exist")
        return
    sidecars = sorted(
        p.relative_to(peer).as_posix()
        for p in meta.rglob("*")
        if p.is_file() and p.name.startswith("snapshot.db") and p.name != "snapshot.db"
    )
    record(not sidecars, failures, f"{label}: SQLite sidecar files were uploaded: {sidecars!r}")


def snapshot_basenames(path: Path) -> set[str]:
    con = sqlite3.connect(str(path))
    try:
        return {row[0] for row in con.execute("SELECT basename FROM snapshot")}
    finally:
        con.close()


def test_first_run_creates_and_uploads_snapshots(root: Path) -> CheckResult:
    failures: list[str] = []
    peer_a = root / "first-a"
    peer_b = root / "first-b"
    peer_a.mkdir(parents=True)
    peer_b.mkdir(parents=True)
    (peer_a / "payload.txt").write_text("from canon\n", encoding="utf-8", newline="\n")

    result = run_kitchensync([f"+{peer_a}", str(peer_b), "--verbosity", "error"], failures, "first run")
    require_success(result, failures, "first run")

    record((peer_b / "payload.txt").read_text(encoding="utf-8") == "from canon\n", failures, "006.21/006.22: copied file was not present before successful completion")
    for label, peer in (("canon peer", peer_a), ("second peer", peer_b)):
        snap = snapshot_path(peer)
        assert_sqlite_snapshot_usable(snap, failures, f"006.1/006.2/006.17/006.22/006.38 {label}")
        assert_no_snapshot_sidecars(peer, failures, f"006.3/006.4/006.38 {label}")
        names = snapshot_basenames(snap) if snap.exists() else set()
        record("payload.txt" in names, failures, f"006.21/006.22 {label}: uploaded snapshot does not record completed file copy")
    return CheckResult("first_run_creates_and_uploads_snapshots", failures)


def test_snapshot_replacement_cleans_swap(root: Path) -> CheckResult:
    failures: list[str] = []
    peer_a = root / "replace-a"
    peer_b = root / "replace-b"
    peer_a.mkdir(parents=True)
    peer_b.mkdir(parents=True)
    create_snapshot_db(snapshot_path(peer_a), "old-canon")
    create_snapshot_db(snapshot_path(peer_b), "old-target")
    (peer_a / "fresh.txt").write_text("fresh\n", encoding="utf-8", newline="\n")

    result = run_kitchensync([f"+{peer_a}", str(peer_b), "--verbosity", "error"], failures, "replacement")
    require_success(result, failures, "replacement")

    for label, peer in (("canon peer", peer_a), ("target peer", peer_b)):
        swap = snapshot_swap_dir(peer)
        record(not (swap / "new").exists(), failures, f"006.23/006.26 {label}: SWAP new remained after normal upload")
        record(not (swap / "old").exists(), failures, f"006.25/006.27/006.28 {label}: SWAP old remained after normal upload")
        assert_sqlite_snapshot_usable(snapshot_path(peer), failures, f"006.25/006.26/006.27/006.28 {label}")
        names = snapshot_basenames(snapshot_path(peer)) if snapshot_path(peer).exists() else set()
        record("fresh.txt" in names, failures, f"006.22 {label}: replacement upload did not include updated snapshot data")
    return CheckResult("snapshot_replacement_cleans_swap", failures)


def seed_swap_case(peer: Path, live: bool, old: bool, new: bool) -> None:
    peer.mkdir(parents=True, exist_ok=True)
    if live:
        create_snapshot_db(snapshot_path(peer), "live-marker")
    swap = snapshot_swap_dir(peer)
    if old:
        create_snapshot_db(swap / "old", "old-marker")
    if new:
        create_snapshot_db(swap / "new", "new-marker")


def run_recovery_case(root: Path, case_name: str, live: bool, old: bool, new: bool) -> list[str]:
    failures: list[str] = []
    control = root / f"{case_name}-control"
    peer = root / f"{case_name}-peer"
    control.mkdir(parents=True)
    seed_swap_case(control, live=True, old=False, new=False)
    seed_swap_case(peer, live=live, old=old, new=new)

    result = run_kitchensync([f"+{control}", str(peer), "--verbosity", "error"], failures, case_name)
    require_success(result, failures, case_name)

    swap = snapshot_swap_dir(peer)
    record(snapshot_path(peer).exists(), failures, f"{case_name}: 006.5 expected recovery/upload to leave a live snapshot.db")
    record(not (swap / "old").exists(), failures, f"{case_name}: expected SWAP old to be removed or consumed")
    record(not (swap / "new").exists(), failures, f"{case_name}: expected SWAP new to be removed or consumed")
    assert_sqlite_snapshot_usable(snapshot_path(peer), failures, f"{case_name}: recovered snapshot")
    return failures


def test_startup_snapshot_swap_recovery(root: Path) -> CheckResult:
    failures: list[str] = []
    cases = [
        ("old-live", True, True, False),  # 006.6, 006.7
        ("old-new-live", True, True, True),  # 006.8
        ("old-new-no-live", False, True, True),  # 006.9, 006.10
        ("old-only-no-live", False, True, False),  # 006.11
        ("new-live", True, False, True),  # 006.12, 006.13
        ("new-only-no-live", False, False, True),  # 006.14
    ]
    for case_name, live, old, new in cases:
        failures.extend(run_recovery_case(root, case_name, live, old, new))
    return CheckResult("startup_snapshot_swap_recovery", failures)


def test_failed_snapshot_recovery_excludes_peer(root: Path) -> CheckResult:
    failures: list[str] = []
    canon = root / "failed-recovery-canon"
    healthy = root / "failed-recovery-healthy"
    broken = root / "failed-recovery-broken"
    canon.mkdir(parents=True)
    healthy.mkdir(parents=True)
    broken.mkdir(parents=True)
    create_snapshot_db(snapshot_path(canon), "canon")
    create_snapshot_db(snapshot_path(healthy), "healthy")
    (snapshot_swap_dir(broken)).parent.mkdir(parents=True, exist_ok=True)
    (snapshot_swap_dir(broken)).write_text("not a directory\n", encoding="utf-8", newline="\n")
    (canon / "survives.txt").write_text("survives\n", encoding="utf-8", newline="\n")

    result = run_kitchensync([f"+{canon}", str(healthy), str(broken), "--verbosity", "error"], failures, "failed recovery")

    require_success(result, failures, "failed recovery")
    record((healthy / "survives.txt").exists(), failures, "006.15: healthy reachable peer did not sync after broken peer recovery failed")
    record(not (broken / "survives.txt").exists(), failures, "006.15: peer with failed snapshot recovery was not excluded")
    record((snapshot_swap_dir(broken)).is_file(), failures, "006.15: broken SWAP state should remain for the excluded peer")
    return CheckResult("failed_snapshot_recovery_excludes_peer", failures)


def test_later_run_replaces_uploaded_snapshot_state(root: Path) -> CheckResult:
    failures: list[str] = []
    peer_a = root / "last-a"
    peer_b = root / "last-b"
    peer_a.mkdir(parents=True)
    peer_b.mkdir(parents=True)
    (peer_a / "first.txt").write_text("first\n", encoding="utf-8", newline="\n")

    first = run_kitchensync([f"+{peer_a}", str(peer_b), "--verbosity", "error"], failures, "last first")
    require_success(first, failures, "last first")
    (peer_a / "second.txt").write_text("second\n", encoding="utf-8", newline="\n")
    second = run_kitchensync([f"+{peer_a}", str(peer_b), "--verbosity", "error"], failures, "last second")
    require_success(second, failures, "last second")

    names = snapshot_basenames(snapshot_path(peer_b)) if snapshot_path(peer_b).exists() else set()
    record({"first.txt", "second.txt"}.issubset(names), failures, f"006.39: final peer snapshot did not reflect latest completed upload, names={sorted(names)!r}")
    return CheckResult("later_run_replaces_uploaded_snapshot_state", failures)


def main() -> int:
    all_failures: list[str] = []
    if not KITCHENSYNC_EXE.exists():
        all_failures.append(f"released executable does not exist: {KITCHENSYNC_EXE}")
    with tempfile.TemporaryDirectory(prefix="kitchensync-006-") as tmp:
        root = Path(tmp)
        for test in (
            test_first_run_creates_and_uploads_snapshots,
            test_snapshot_replacement_cleans_swap,
            test_startup_snapshot_swap_recovery,
            test_failed_snapshot_recovery_excludes_peer,
            test_later_run_replaces_uploaded_snapshot_state,
        ):
            case_root = root / test.__name__
            if case_root.exists():
                shutil.rmtree(case_root)
            case_root.mkdir(parents=True)
            result = test(case_root)
            for failure in result.failures:
                all_failures.append(f"{result.name}: {failure}")

    if all_failures:
        print("FAIL")
        for failure in all_failures:
            print(f"- {failure}")
        return 1
    print("PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
