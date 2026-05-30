#!/usr/bin/env uvrun
# /// script
# requires-python = ">=3.11"
# ///

from __future__ import annotations

import os
import sqlite3
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Iterable

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
if hasattr(sys.stderr, "reconfigure"):
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")


WORKSPACE_ROOT = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync")
PROJECT_DIR = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\proj")
WINDOWS_EXE = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\released\\kitchensync.exe")
POSIX_EXE = Path(r"C:\\Users\\human\\Desktop\\prjx\\kitchensync\\released\\kitchensync")
RELEASED_EXE = WINDOWS_EXE if os.name == "nt" else POSIX_EXE

TIMESTAMP_FORMAT = "%Y-%m-%d_%H-%M-%S_%fZ"


def _add_failure(failures: list[str], condition: bool, req: str, detail: str) -> None:
    if not condition:
        failures.append(f"{req}: {detail}")


def _run_kitchensync(args: Iterable[str], cwd: Path, timeout_seconds: float = 30.0) -> subprocess.CompletedProcess[str]:
    cmd = [str(RELEASED_EXE), *args]
    try:
        return subprocess.run(
            cmd,
            cwd=str(cwd),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout_seconds,
            check=False,
        )
    except FileNotFoundError as exc:
        return subprocess.CompletedProcess(
            args=cmd,
            returncode=127,
            stdout="",
            stderr=f"failed to launch released executable: {exc}",
        )
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(
            args=cmd,
            returncode=124,
            stdout="",
            stderr="kitchensync invocation timed out",
        )


def _write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def _write_bytes(path: Path, payload: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(payload)


def _snapshot_db(peer_root: Path) -> Path:
    return peer_root / ".kitchensync" / "snapshot.db"


def _load_snapshot_rows(peer_root: Path) -> list[dict[str, object]]:
    db_path = _snapshot_db(peer_root)
    if not db_path.is_file():
        return []

    conn: sqlite3.Connection | None = None
    try:
        conn = sqlite3.connect(str(db_path))
        conn.row_factory = sqlite3.Row
        rows = conn.execute(
            "SELECT id, parent_id, basename, mod_time, byte_size, last_seen, deleted_time FROM snapshot ORDER BY id;"
        ).fetchall()
        return [dict(row) for row in rows]
    except Exception:
        return []
    finally:
        if conn is not None:
            conn.close()


def _snapshot_rows_by_path(peer_root: Path) -> tuple[dict[str, dict[str, object]], dict[str, dict[str, object]]]:
    rows = _load_snapshot_rows(peer_root)
    if not rows:
        return {}, {}

    by_id: dict[str, dict[str, object]] = {}
    for row in rows:
        row_id = row.get("id")
        if row_id is not None:
            by_id[str(row_id)] = row

    parent_ids = {
        str(row["parent_id"])
        for row in rows
        if row.get("parent_id") is not None
        and str(row.get("parent_id")) not in by_id
    }
    sentinel = next(iter(parent_ids), None)

    memo: dict[str, str | None] = {}
    visiting: set[str] = set()

    def resolve(row_id: str) -> str | None:
        if row_id in memo:
            return memo[row_id]
        if row_id in visiting:
            return None

        row = by_id.get(row_id)
        if row is None:
            return None

        parent_id = row.get("parent_id")
        basename = row.get("basename")
        if basename is None:
            return None

        visiting.add(row_id)
        parent_text = str(parent_id) if parent_id is not None else None

        if parent_text is None:
            path = None
        elif sentinel is not None and parent_text == sentinel:
            path = str(basename)
        else:
            if parent_text not in by_id:
                path = None
            else:
                prefix = resolve(parent_text)
                if prefix is None:
                    path = None
                else:
                    path = f"{prefix}/{basename}"

        visiting.remove(row_id)
        memo[row_id] = path
        return path

    path_by_path: dict[str, dict[str, object]] = {}
    for row_id in by_id:
        path = resolve(row_id)
        if path is None:
            continue
        existing = path_by_path.get(path)
        if existing is None:
            path_by_path[path] = by_id[row_id]

    return path_by_path, by_id


def _snapshot_row(peer_root: Path, rel_path: str) -> dict[str, object] | None:
    path_map, _ = _snapshot_rows_by_path(peer_root)
    return path_map.get(rel_path.replace("\\", "/"))


def _update_snapshot_row(
    peer_root: Path,
    row_id: str,
    updates: dict[str, str | int | None],
) -> bool:
    db_path = _snapshot_db(peer_root)
    if not db_path.is_file():
        return False

    if not updates:
        return True

    conn: sqlite3.Connection | None = None
    try:
        conn = sqlite3.connect(str(db_path))
        set_clause = ", ".join(f"{key} = ?" for key in updates)
        values = list(updates.values())
        values.append(row_id)
        conn.execute(f"UPDATE snapshot SET {set_clause} WHERE id = ?", values)
        conn.commit()
        return True
    except Exception:
        return False
    finally:
        if conn is not None:
            conn.close()


def _parse_timestamp(value: str | None) -> datetime | None:
    if value is None:
        return None
    try:
        return datetime.strptime(value, TIMESTAMP_FORMAT).replace(tzinfo=timezone.utc)
    except ValueError:
        return None


def _format_timestamp(value: datetime) -> str:
    if value.tzinfo is None:
        value = value.replace(tzinfo=timezone.utc)
    return value.astimezone(timezone.utc).strftime(TIMESTAMP_FORMAT)


def _file_timestamp(path: Path) -> datetime:
    return datetime.fromtimestamp(path.stat().st_mtime, tz=timezone.utc)


def _assert_timestamp_like(failures: list[str], req: str, label: str, value: str | None) -> None:
    parsed = _parse_timestamp(value)
    _add_failure(failures, parsed is not None, req, f"{label}: {value!r} is not in database timestamp format")


def _assert_timestamp_close_to_file(
    failures: list[str],
    req: str,
    label: str,
    fs_path: Path,
    db_timestamp: str,
) -> None:
    db_time = _parse_timestamp(db_timestamp)
    _add_failure(failures, db_time is not None, req, f"{label}: malformed timestamp {db_timestamp!r}")
    if db_time is None:
        return
    fs_time = _file_timestamp(fs_path)
    _add_failure(
        failures,
        abs((fs_time - db_time).total_seconds()) <= 2.0,
        req,
        f"{label}: db mod_time={db_timestamp!r} diverges from filesystem timestamp {fs_time.strftime(TIMESTAMP_FORMAT)}",
    )


def _case_present_absent_updates(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_009_present_absent_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        peer = workspace / "peer"
        canon.mkdir()
        peer.mkdir()

        source = canon / "present.txt"
        _write_text(source, "seed")

        first = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        _add_failure(failures, first is not None, "009.1", "kitchensync invocation returned no result")
        if first is None:
            return
        _add_failure(failures, first.returncode == 0, "009.1", f"initial sync exited {first.returncode}")

        canon_row = _snapshot_row(canon, "present.txt")
        peer_row = _snapshot_row(peer, "present.txt")

        _add_failure(failures, peer_row is not None, "009.1", "peer snapshot missing row for confirmed present file")
        _add_failure(failures, canon_row is not None, "009.1", "canon snapshot missing row for confirmed present file")

        if peer_row is not None:
            _assert_timestamp_like(failures, "009.2", "peer row mod_time", str(peer_row.get("mod_time")))
            _assert_timestamp_close_to_file(failures, "009.2", "peer row mod_time", source, str(peer_row.get("mod_time")))
            _add_failure(failures, peer_row.get("byte_size") == len("seed"), "009.3", "peer snapshot byte_size does not match source file size")
            _add_failure(failures, peer_row.get("last_seen") is not None, "009.4", "peer snapshot row for confirmed present file did not set last_seen")
            _add_failure(failures, peer_row.get("deleted_time") is None, "009.5", "peer snapshot row for confirmed present file had deleted_time set")

        pre_peer_row = dict(peer_row) if peer_row is not None else None
        if peer_row is not None:
            _add_failure(failures, int(peer_row.get("byte_size", -1)) == len("seed"), "009.3", "peer snapshot byte_size mismatch")

        (peer / "present.txt").unlink()
        second = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        _add_failure(failures, second is not None, "009.6", "kitchensync invocation returned no result")
        if second is None:
            return
        _add_failure(failures, second.returncode == 0, "009.6", f"sync after deletion exited {second.returncode}")

        peer_row_after_absent = _snapshot_row(peer, "present.txt")
        _add_failure(failures, peer_row_after_absent is not None, "009.6", "peer snapshot row for deleted-but-tracked file was removed")
        if peer_row_after_absent is not None and pre_peer_row is not None:
            _add_failure(
                failures,
                peer_row_after_absent.get("deleted_time") == pre_peer_row.get("last_seen"),
                "009.7",
                "peer snapshot row deleted_time was not set from previous last_seen",
            )
            _add_failure(
                failures,
                peer_row_after_absent.get("last_seen") == pre_peer_row.get("last_seen"),
                "009.8",
                "peer snapshot row last_seen changed while confirming absence",
            )

        third = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        _add_failure(failures, third is not None, "009.9", "kitchensync invocation returned no result")
        if third is None:
            return
        _add_failure(failures, third.returncode == 0, "009.9", f"reconciling stale row after second sync exited {third.returncode}")

        peer_row_after_recheck = _snapshot_row(peer, "present.txt")
        _add_failure(failures, peer_row_after_recheck is not None, "009.9", "peer snapshot row for already-tombstoned entry disappeared unexpectedly")
        if peer_row_after_recheck is not None and peer_row_after_absent is not None:
            _add_failure(
                failures,
                peer_row_after_recheck.get("deleted_time") == peer_row_after_absent.get("deleted_time"),
                "009.9",
                "tombstoned row changed when absence was reconfirmed",
            )


def _case_copy_intention_and_completion(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_009_copy_intent_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        peer = workspace / "peer"
        canon.mkdir()
        peer.mkdir()

        source = canon / "push.txt"
        peer_block = peer / ".kitchensync"
        _write_text(source, "v1")

        _write_bytes(peer_block, b"block")
        first = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        _add_failure(failures, first is not None, "009.10", "kitchensync invocation returned no result")
        if first is None:
            return
        _add_failure(failures, first.returncode == 0, "009.10", f"forced-failure setup run exited {first.returncode}")

        row_before = _snapshot_row(peer, "push.txt")
        _add_failure(failures, row_before is not None, "009.10", "destination row missing after failed copy enqueue")
        if row_before is not None:
            _assert_timestamp_like(failures, "009.10", "peer mod_time for intended copy", str(row_before.get("mod_time")))
            _assert_timestamp_close_to_file(failures, "009.10", "peer mod_time for intended copy", source, str(row_before.get("mod_time")))
            _add_failure(
                failures,
                int(row_before.get("byte_size", -1)) == len("v1"),
                "009.11",
                "destination row byte_size did not match winning entry on intended copy",
            )
            _add_failure(
                failures,
                row_before.get("deleted_time") is None,
                "009.12",
                "destination row deleted_time was not NULL after intended copy",
            )
            _add_failure(
                failures,
                row_before.get("last_seen") is None,
                "009.13",
                "destination row last_seen was not NULL for new intended-copy row before completion",
            )

        second = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        _add_failure(failures, second is not None, "009.15", "kitchensync invocation returned no result")
        if second is None:
            return
        _add_failure(failures, second.returncode == 0, "009.15", f"copy completion run exited {second.returncode}")
        row_after = _snapshot_row(peer, "push.txt")
        _add_failure(failures, row_after is not None, "009.15", "destination row missing after copy completion")
        _add_failure(
            failures,
            row_after is not None and row_after.get("last_seen") is not None,
            "009.15",
            "destination row last_seen remained NULL after copy completion",
        )

        pre_retry_row = _snapshot_row(peer, "push.txt")
        time.sleep(6.1)
        _write_text(source, "v2")
        (peer / "push.txt").unlink(missing_ok=True)
        _write_bytes(peer_block, b"block")

        third = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        _add_failure(failures, third is not None, "009.14", "kitchensync invocation returned no result")
        if third is None:
            return

        row_retry = _snapshot_row(peer, "push.txt")
        _add_failure(failures, row_retry is not None, "009.14", "destination row missing after failed re-enqueue")
        if row_retry is not None and pre_retry_row is not None:
            _add_failure(
                failures,
                row_retry.get("last_seen") == pre_retry_row.get("last_seen"),
                "009.14",
                "destination row last_seen changed during failed re-enqueue",
            )
            _add_failure(
                failures,
                row_retry.get("deleted_time") is None,
                "009.17",
                "destination row deleted_time was set during failed copy path",
            )
            _add_failure(
                failures,
                row_retry.get("mod_time") is not None,
                "009.10",
                "destination row mod_time missing after failed re-enqueue",
            )

        peer_block.unlink()
        fourth = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        _add_failure(failures, fourth is not None, "009.16", "kitchensync invocation returned no result")
        if fourth is None:
            return
        _add_failure(failures, fourth.returncode == 0, "009.16", f"final copy completion run exited {fourth.returncode}")
        row_final = _snapshot_row(peer, "push.txt")
        _add_failure(failures, row_final is not None, "009.16", "destination row missing after final copy")
        _add_failure(
            failures,
            row_final is not None and row_final.get("last_seen") is not None,
            "009.16",
            "destination row last_seen remained NULL after successful copy",
        )

        if peer_block.exists():
            peer_block.unlink()


def _case_directory_create_displace(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_009_directory_displace_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        peer = workspace / "peer"
        canon.mkdir()
        peer.mkdir()

        _write_text(canon / "create_me" / "inner.txt", "alpha")
        _write_text(canon / "keep.txt", "stable")

        first = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        _add_failure(failures, first is not None, "009.18", "kitchensync invocation returned no result")
        if first is None:
            return
        _add_failure(failures, first.returncode == 0, "009.18", f"initial directory sync exited {first.returncode}")

        dir_row = _snapshot_row(peer, "create_me")
        _add_failure(failures, dir_row is not None, "009.18", "peer snapshot row for created directory missing")
        if dir_row is not None:
            _add_failure(failures, dir_row.get("last_seen") is not None, "009.18", "peer directory row missing last_seen after inline directory creation")
            _add_failure(failures, dir_row.get("deleted_time") is None, "009.18", "peer directory row had deleted_time set after creation")
            _add_failure(failures, int(dir_row.get("byte_size", -2)) == -1, "009.18", "directory rows must store byte_size = -1")

        dir_pre_row = _snapshot_row(peer, "create_me")
        inner_pre_row = _snapshot_row(peer, "create_me/inner.txt")
        keep_pre_row = _snapshot_row(peer, "keep.txt")

        if (canon / "create_me").exists():
            for child in sorted((canon / "create_me").rglob("*"), reverse=True):
                if child.is_file():
                    child.unlink()
                else:
                    child.rmdir()
            (canon / "create_me").rmdir()

        delete_result = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        _add_failure(failures, delete_result is not None, "009.20", "kitchensync invocation returned no result")
        if delete_result is None:
            return
        _add_failure(failures, delete_result.returncode == 0, "009.20", f"directory deletion sync exited {delete_result.returncode}")

        dir_post_row = _snapshot_row(peer, "create_me")
        inner_post_row = _snapshot_row(peer, "create_me/inner.txt")
        _add_failure(failures, dir_post_row is not None, "009.20", "peer directory row should remain for displaced directory")
        _add_failure(failures, inner_post_row is not None, "009.22", "descendant directory row should remain and cascade should apply")
        if dir_pre_row is not None and dir_post_row is not None:
            _add_failure(
                failures,
                dir_post_row.get("deleted_time") == dir_pre_row.get("last_seen"),
                "009.20",
                "directory deleted_time was not set from previous last_seen",
            )
        if inner_pre_row is not None and inner_post_row is not None:
            _add_failure(
                failures,
                inner_post_row.get("deleted_time") == dir_post_row.get("deleted_time"),
                "009.22",
                "directory displacement cascade did not apply same deleted_time to descendant rows",
            )

        (canon / "keep.txt").unlink()
        _write_bytes(peer_block := (peer / ".kitchensync"), b"block")

        fail_disp = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        _add_failure(failures, fail_disp is not None, "009.21", "kitchensync invocation returned no result")
        if fail_disp is None:
            return
        keep_post_row = _snapshot_row(peer, "keep.txt")
        if keep_pre_row is not None and keep_post_row is not None:
            _add_failure(
                failures,
                keep_post_row.get("last_seen") == keep_pre_row.get("last_seen"),
                "009.21",
                "displacement failure changed destination row instead of leaving it unchanged",
            )
            _add_failure(
                failures,
                keep_post_row.get("deleted_time") == keep_pre_row.get("deleted_time"),
                "009.21",
                "displacement failure changed destination deleted_time",
            )
        _add_failure(failures, (peer / "keep.txt").exists(), "009.21", "displaced file disappeared when displacement operation failed")
        if peer_block.exists():
            peer_block.unlink()


def _case_snapshot_cleanup(failures: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="ks_009_cleanup_") as raw_root:
        workspace = Path(raw_root)
        canon = workspace / "canon"
        peer = workspace / "peer"
        canon.mkdir()
        peer.mkdir()

        _write_text(canon / "tomb_old.txt", "old")
        _write_text(canon / "tomb_recent.txt", "recent")
        _write_text(canon / "orphan_stale.txt", "orphan")
        _write_text(canon / "listed.txt", "listed")

        seed = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        _add_failure(failures, seed is not None, "009.26", "kitchensync invocation returned no result")
        if seed is None:
            return
        _add_failure(failures, seed.returncode == 0, "009.26", f"seed sync exited {seed.returncode}")

        orphan_row = _snapshot_row(peer, "orphan_stale.txt")
        listed_row = _snapshot_row(peer, "listed.txt")
        _add_failure(failures, orphan_row is not None, "009.29", "orphan row missing before manual stale setup")
        _add_failure(failures, listed_row is not None, "009.30", "listed row missing before cleanup setup")

        (canon / "tomb_old.txt").unlink()
        (peer / "tomb_old.txt").unlink()
        (canon / "tomb_recent.txt").unlink()
        (peer / "tomb_recent.txt").unlink()
        second = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        _add_failure(failures, second is not None, "009.26", "kitchensync invocation returned no result")
        if second is None:
            return
        _add_failure(failures, second.returncode == 0, "009.26", f"tombstone setup sync exited {second.returncode}")

        tomb_old_row_peer = _snapshot_row(peer, "tomb_old.txt")
        tomb_recent_row_peer = _snapshot_row(peer, "tomb_recent.txt")
        orphan_row_peer = _snapshot_row(peer, "orphan_stale.txt")
        listed_row_peer = _snapshot_row(peer, "listed.txt")

        tomb_old_row_canon = _snapshot_row(canon, "tomb_old.txt")
        tomb_recent_row_canon = _snapshot_row(canon, "tomb_recent.txt")
        orphan_row_canon = _snapshot_row(canon, "orphan_stale.txt")
        listed_row_canon = _snapshot_row(canon, "listed.txt")

        _add_failure(failures, tomb_old_row_peer is not None, "009.26", "tomb_old entry not present after deletion-based marking")
        _add_failure(failures, tomb_recent_row_peer is not None, "009.27", "tomb_recent entry not present after deletion-based marking")
        _add_failure(failures, orphan_row_peer is not None, "009.29", "orphan row missing after deletion setup")
        _add_failure(failures, listed_row_peer is not None, "009.30", "listed row missing after deletion setup")

        old_ts = _format_timestamp(datetime.now(timezone.utc) - timedelta(days=181))
        recent_ts = _format_timestamp(datetime.now(timezone.utc) - timedelta(days=1))
        stale_ts = _format_timestamp(datetime.now(timezone.utc) - timedelta(days=181))

        if tomb_old_row_peer is not None:
            _update_snapshot_row(peer, str(tomb_old_row_peer["id"]), {"deleted_time": old_ts})
        if tomb_old_row_canon is not None:
            _update_snapshot_row(canon, str(tomb_old_row_canon["id"]), {"deleted_time": old_ts})

        if tomb_recent_row_peer is not None:
            _update_snapshot_row(peer, str(tomb_recent_row_peer["id"]), {"deleted_time": recent_ts})
        if tomb_recent_row_canon is not None:
            _update_snapshot_row(canon, str(tomb_recent_row_canon["id"]), {"deleted_time": recent_ts})

        if orphan_row_peer is not None:
            _update_snapshot_row(peer, str(orphan_row_peer["id"]), {"deleted_time": None, "last_seen": stale_ts})
        if orphan_row_canon is not None:
            _update_snapshot_row(canon, str(orphan_row_canon["id"]), {"deleted_time": None, "last_seen": stale_ts})

        if listed_row_peer is not None:
            _update_snapshot_row(peer, str(listed_row_peer["id"]), {"last_seen": stale_ts})
        if listed_row_canon is not None:
            _update_snapshot_row(canon, str(listed_row_canon["id"]), {"last_seen": stale_ts})

        (canon / "orphan_stale.txt").unlink()
        (peer / "orphan_stale.txt").unlink()

        cleanup = _run_kitchensync([f"+{canon}", str(peer)], cwd=workspace)
        _add_failure(failures, cleanup is not None, "009.27", "kitchensync invocation returned no result")
        if cleanup is None:
            return
        _add_failure(failures, cleanup.returncode == 0, "009.27", f"cleanup run exited {cleanup.returncode}")

        tomb_old_after_peer = _snapshot_row(peer, "tomb_old.txt")
        tomb_recent_after_peer = _snapshot_row(peer, "tomb_recent.txt")
        orphan_after_peer = _snapshot_row(peer, "orphan_stale.txt")
        listed_after_peer = _snapshot_row(peer, "listed.txt")

        tomb_old_after_canon = _snapshot_row(canon, "tomb_old.txt")
        tomb_recent_after_canon = _snapshot_row(canon, "tomb_recent.txt")
        orphan_after_canon = _snapshot_row(canon, "orphan_stale.txt")
        listed_after_canon = _snapshot_row(canon, "listed.txt")

        _add_failure(failures, tomb_old_after_peer is None, "009.26", "old tombstone row was not cleaned up on peer")
        _add_failure(failures, tomb_old_after_canon is None, "009.26", "old tombstone row was not cleaned up on canon")

        _add_failure(failures, tomb_recent_after_peer is not None, "009.27", "recent tombstone row was removed too early")
        _add_failure(failures, tomb_recent_after_canon is not None, "009.27", "recent tombstone row was removed too early on canon")

        _add_failure(failures, orphan_after_peer is None, "009.29", "orphan stale non-tomb row was not cleaned up when absent and old")
        _add_failure(failures, orphan_after_canon is None, "009.29", "orphan stale non-tomb row was not cleaned up on canon")

        _add_failure(failures, listed_after_peer is not None, "009.30", "listed row was removed even though still present in peer listing")
        _add_failure(failures, listed_after_canon is not None, "009.30", "listed row was removed even though still present in canon listing")

        # 009.28 (default keep is 180): validated by using a 181-day old deleted row.


def main() -> int:
    failures: list[str] = []

    _add_failure(failures, RELEASED_EXE.is_file(), "precondition", f"released executable missing: {RELEASED_EXE}")
    if not RELEASED_EXE.is_file():
        print("test_009_snapshot_updates.py failed")
        print("  1. precondition: released executable not found")
        return 1

    test_cases = [
        ("009.1-009.9", _case_present_absent_updates),
        ("009.10-009.17", _case_copy_intention_and_completion),
        ("009.18,009.20,009.21,009.22", _case_directory_create_displace),
        ("009.26,009.27,009.28,009.29,009.30", _case_snapshot_cleanup),
    ]

    for label, fn in test_cases:
        try:
            fn(failures)
        except Exception as exc:  # noqa: BLE001
            failures.append(f"{label}: unexpected exception: {exc!r}")

    # Not reasonably testable through observable peer + filesystem behavior in this e2e harness:
    # 009.19 -- inline directory creation failure on destination without changing its existing snapshot row.
    # 009.23 -- successful directory cascade only touches that peer's database and never another peer's DB.
    # 009.24 -- cascade path through an existing tombstone row.
    # 009.25 -- orphaned descendants skipped when intermediate rows are purged.
    # 009.31 -- cleanup pass does not need to remove all obsolete rows in one run.
    # 009.32 -- opportunistic cleanup does not delay first scan.
    # 009.33 -- opportunistic cleanup does not delay first eligible copy.

    if failures:
        print("test_009_snapshot_updates.py failed")
        for index, failure in enumerate(failures, start=1):
            print(f"  {index:02d}. {failure}")
        return 1

    print("test_009_snapshot_updates.py passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
