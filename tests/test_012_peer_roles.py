# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///

from __future__ import annotations

import filecmp
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from pathlib import Path


sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path("/home/ace/Desktop/prjx/kitchensync")
KITCHENSYNC_EXE = WORKSPACE_ROOT / "released" / "kitchensync.exe"
TIMEOUT_SECONDS = 30


class CheckRun:
    def __init__(self) -> None:
        self.failures: list[str] = []

    def check(self, condition: bool, message: str) -> None:
        if not condition:
            self.failures.append(message)


def run_sync(*args: str | Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [str(KITCHENSYNC_EXE), *[str(arg) for arg in args]],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=TIMEOUT_SECONDS,
        shell=False,
    )


def write_file(path: Path, text: str, offset_seconds: int = 0) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")
    if offset_seconds:
        stamp = time.time() + offset_seconds
        os.utime(path, (stamp, stamp))


def read_file(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def snapshot_path(peer: Path) -> Path:
    return peer / ".kitchensync" / "snapshot.db"


def snapshot_rows(peer: Path) -> list[tuple[str, int, str | None, str | None]]:
    db = snapshot_path(peer)
    if not db.exists():
        return []
    with sqlite3.connect(str(db)) as conn:
        return list(
            conn.execute(
                "SELECT basename, byte_size, last_seen, deleted_time "
                "FROM snapshot ORDER BY basename, byte_size"
            )
        )


def snapshot_has_basename(peer: Path, basename: str) -> bool:
    return any(row[0] == basename for row in snapshot_rows(peer))


def snapshot_bytes(peer: Path) -> bytes:
    return snapshot_path(peer).read_bytes()


def has_bak_entry(peer: Path, basename: str) -> bool:
    bak_root = peer / ".kitchensync"
    if not bak_root.exists():
        return False
    return any(path.name == basename for path in bak_root.rglob(basename))


def expect_success(checks: CheckRun, proc: subprocess.CompletedProcess[str], label: str) -> None:
    checks.check(proc.returncode == 0, f"{label}: expected exit 0, got {proc.returncode}; stdout={proc.stdout!r}")
    checks.check(proc.stderr == "", f"{label}: expected empty stderr, got {proc.stderr!r}")
    checks.check("sync complete" in proc.stdout.splitlines(), f"{label}: stdout should include sync complete")


def expect_failure(checks: CheckRun, proc: subprocess.CompletedProcess[str], label: str) -> None:
    checks.check(proc.returncode != 0, f"{label}: expected non-zero exit, got 0")
    checks.check(proc.stderr == "", f"{label}: expected empty stderr, got {proc.stderr!r}")


def test_first_sync_requires_canon(checks: CheckRun, root: Path) -> None:
    peer_a = root / "first-no-canon-a"
    peer_b = root / "first-no-canon-b"
    peer_a.mkdir()
    peer_b.mkdir()

    proc = run_sync(peer_a, peer_b)

    expect_failure(checks, proc, "012.4/012.5 first sync without canon")
    checks.check(
        "First sync? Mark the authoritative peer with a leading +" in proc.stdout,
        "012.4: first sync without canon should print the required guidance",
    )


def test_no_contributing_peer(checks: CheckRun, root: Path) -> None:
    canon = root / "no-contrib-canon"
    historical = root / "no-contrib-historical"
    new_peer = root / "no-contrib-new"
    canon.mkdir()
    historical.mkdir()
    new_peer.mkdir()
    write_file(canon / "seed.txt", "seed\n")

    setup = run_sync(f"+{canon}", historical)
    expect_success(checks, setup, "setup for 012.3/012.6/012.7")

    proc = run_sync(f"-{historical}", new_peer)

    expect_failure(checks, proc, "012.3/012.6/012.7 no contributing peer")
    checks.check(
        "No contributing peer reachable - cannot make sync decisions" in proc.stdout,
        "012.6: all-subordinate run should print the required no-contributor message",
    )
    checks.check(
        not (new_peer / "seed.txt").exists(),
        "012.3/012.7: marked subordinate peer with history must not contribute a decision",
    )


def test_subordinate_and_canon_roles(checks: CheckRun, root: Path) -> tuple[Path, Path, Path]:
    canon = root / "roles-canon"
    normal_new = root / "roles-normal-new"
    explicit_sub = root / "roles-explicit-sub"
    canon.mkdir()
    normal_new.mkdir()
    explicit_sub.mkdir()
    write_file(canon / "keep.txt", "canon keep\n")
    write_file(canon / "conflict.txt", "canon old\n", offset_seconds=-100)
    write_file(explicit_sub / "sub-only.txt", "must not influence decisions\n")

    first = run_sync(f"+{canon}", normal_new, f"-{explicit_sub}")

    expect_success(checks, first, "012.1/012.2/012.10/012.11/012.12/012.13 first canon run")
    checks.check(
        read_file(normal_new / "keep.txt") == "canon keep\n",
        "012.1/012.12: snapshotless non-canon peer should receive canon file as subordinate",
    )
    checks.check(
        read_file(explicit_sub / "keep.txt") == "canon keep\n",
        "012.12: explicit subordinate peer should receive selected outcome",
    )
    checks.check(
        not (canon / "sub-only.txt").exists() and not (normal_new / "sub-only.txt").exists(),
        "012.11: subordinate-only entry must not contribute to sync decisions",
    )
    checks.check(
        not (explicit_sub / "sub-only.txt").exists() and has_bak_entry(explicit_sub, "sub-only.txt"),
        "012.10/012.12: subordinate listing should be processed and extra entry displaced",
    )
    checks.check(
        snapshot_has_basename(normal_new, "keep.txt") and snapshot_has_basename(explicit_sub, "keep.txt"),
        "012.13: normal run should upload updated snapshot data to subordinate peers",
    )

    write_file(normal_new / "conflict.txt", "newer non-canon text\n", offset_seconds=200)
    canon_wins = run_sync(f"+{canon}", normal_new)

    expect_success(checks, canon_wins, "012.9 canon conflict")
    checks.check(
        read_file(normal_new / "conflict.txt") == "canon old\n",
        "012.9: canon peer state should win even against a newer conflicting peer file",
    )

    no_canon = run_sync(canon, normal_new)
    expect_success(checks, no_canon, "012.8 no canon with snapshot history")

    write_file(canon / "dry-only.txt", "dry run source\n", offset_seconds=300)
    before_sub_snapshot = snapshot_bytes(explicit_sub)
    dry = run_sync("--dry-run", canon, normal_new, f"-{explicit_sub}")

    expect_success(checks, dry, "012.14 dry-run subordinate snapshot")
    checks.check(
        "dry run" in dry.stdout.lower(),
        "012.14: dry run should report dry run mode on stdout",
    )
    checks.check(
        not (explicit_sub / "dry-only.txt").exists(),
        "012.14: dry run must not copy selected outcomes to subordinate peer",
    )
    checks.check(
        snapshot_bytes(explicit_sub) == before_sub_snapshot,
        "012.14: dry run must not upload updated temporary snapshot data to subordinate peer",
    )

    write_file(explicit_sub / "later-contributor.txt", "from previous subordinate\n", offset_seconds=400)
    later = run_sync(canon, normal_new, explicit_sub)

    expect_success(checks, later, "012.15 later contribution")
    checks.check(
        read_file(canon / "later-contributor.txt") == "from previous subordinate\n"
        and read_file(normal_new / "later-contributor.txt") == "from previous subordinate\n",
        "012.15: a peer that was subordinate in a previous normal run should later contribute when unmarked",
    )

    return canon, normal_new, explicit_sub


def test_offline_peer_roles(checks: CheckRun, root: Path) -> None:
    peer_a = root / "offline-a"
    peer_b = root / "offline-b"
    peer_o = root / "offline-peer"
    peer_a.mkdir()
    peer_b.mkdir()
    peer_o.mkdir()
    write_file(peer_a / "baseline.txt", "baseline\n")

    setup = run_sync(f"+{peer_a}", peer_b, peer_o)
    expect_success(checks, setup, "setup for 012.16-012.19")

    saved_offline = root / "offline-peer-saved"
    shutil.move(str(peer_o), str(saved_offline))
    write_file(saved_offline / "offline-only.txt", "offline change\n", offset_seconds=500)
    before_rows = snapshot_rows(saved_offline)
    before_bytes = snapshot_bytes(saved_offline)
    write_file(peer_o, "not a directory\n")
    write_file(peer_a / "online-only.txt", "online change\n", offset_seconds=600)

    offline_run = run_sync(peer_a, peer_b, peer_o)

    expect_success(checks, offline_run, "012.16/012.17/012.18 offline peer omitted")
    checks.check(
        read_file(peer_b / "online-only.txt") == "online change\n",
        "012.16/012.17: reachable peers should still sync while an unreachable peer is omitted",
    )
    checks.check(
        not (peer_a / "offline-only.txt").exists() and not (peer_b / "offline-only.txt").exists(),
        "012.16/012.17: unreachable peer filesystem entries must be omitted from listings and decisions",
    )
    checks.check(
        snapshot_rows(saved_offline) == before_rows and snapshot_bytes(saved_offline) == before_bytes,
        "012.18: unreachable peer snapshot rows must not be modified during the run",
    )

    peer_o.unlink()
    shutil.move(str(saved_offline), str(peer_o))
    catch_up = run_sync(peer_a, peer_b, peer_o)

    expect_success(checks, catch_up, "012.19 offline peer later reachable")
    checks.check(
        read_file(peer_a / "offline-only.txt") == "offline change\n"
        and read_file(peer_b / "offline-only.txt") == "offline change\n"
        and read_file(peer_o / "online-only.txt") == "online change\n",
        "012.19: when a previously unreachable peer returns, live discrepancies against its snapshot should drive sync decisions",
    )


def main() -> int:
    checks = CheckRun()
    checks.check(KITCHENSYNC_EXE.exists(), f"released executable does not exist: {KITCHENSYNC_EXE}")

    with tempfile.TemporaryDirectory(prefix="kitchensync-012-") as tmp:
        root = Path(tmp)
        try:
            test_first_sync_requires_canon(checks, root)
            test_no_contributing_peer(checks, root)
            test_subordinate_and_canon_roles(checks, root)
            test_offline_peer_roles(checks, root)
        except subprocess.TimeoutExpired as exc:
            checks.failures.append(f"subprocess timed out after {exc.timeout} seconds: {exc.cmd!r}")
        except Exception as exc:
            checks.failures.append(f"unexpected test exception: {type(exc).__name__}: {exc}")

    if checks.failures:
        for failure in checks.failures:
            print(f"FAIL: {failure}")
        return 1

    print("all peer role checks passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
